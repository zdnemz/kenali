//! Typed auth-pipe client (CP ⇄ service) with a SYSTEM-only DACL, a bounded
//! connect and server-PID anti-squatting.
//!
//! The service creates `\\.\pipe\kenali.auth` with the SDDL in
//! [`AUTH_PIPE_SDDL`]; this module is deliberately **client only**. Every
//! blocking call is bounded: [`AUTH_CONNECT_TIMEOUT`] gates `WaitNamedPipeW`
//! and [`AUTH_ATTEMPT_TIMEOUT`] gates the response wait. Secrets (the one-shot
//! `Verified.secret`) are wiped on every path, including errors and panics.
//!
//! Secret-release window is **≤ 3 s after the auth decision**; that lifetime is
//! a service-side property documented in the spike report, not enforced here.
//!
//! Overlapped I/O is intentionally avoided: `Win32_System_IO` is not in the
//! pinned `windows` feature set, so `ReadFile`/`WriteFile`/`OVERLAPPED` are not
//! reachable through the safe wrappers. The handle is opened synchronously
//! (`FILE_FLAG_WRITE_THROUGH`, message-read mode) and the response wait is
//! bounded by polling `PeekNamedPipe` against a deadline.

use core::time::Duration;
use std::time::Instant;

use thiserror::Error;
use zeroize::Zeroizing;

use kenali_ipc::auth::{Request, Response};
use kenali_ipc::codec::{decode, encode, MAX_CONTROL_BYTES};
use kenali_ipc::PROTO_VERSION;

use crate::service::{service_pid, KENALI_SERVICE_NAME};

/// Path of the SYSTEM-only auth pipe (displayed `\\.\pipe\kenali.auth`).
pub const AUTH_PIPE_PATH: &str = "\\\\.\\pipe\\kenali.auth";
/// SDDL for the auth pipe DACL: SYSTEM (local system) only.
pub const AUTH_PIPE_SDDL: &str = "D:P(A;;GA;;;SY)";
/// Maximum time spent waiting for a pipe instance to become available.
pub const AUTH_CONNECT_TIMEOUT: Duration = Duration::from_millis(200);
/// Maximum time spent waiting for a response to one exchange.
pub const AUTH_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(6);

/// Errors from the auth-pipe client. Wording is intentionally generic: no
/// secret, nonce, user name or internal code is ever exposed.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PipeError {
    #[error("authentication pipe is unavailable")]
    Unavailable,
    #[error("timed out contacting authentication pipe")]
    Timeout,
    #[error("authentication pipe handshake failed")]
    Handshake,
    #[error("server identity could not be verified")]
    IdentityMismatch,
    #[error("service lookup failed")]
    Service,
    #[error("protocol version mismatch")]
    ProtocolMismatch,
    #[error("pipe I/O failed")]
    Io,
    #[error("malformed response")]
    Malformed,
    #[error("response did not match the request")]
    NonceMismatch,
}

/// A connected, identity-verified handle to the auth pipe. One-shot: each
/// [`AuthPipeClient::exchange`] consumes one request/response pair.
pub struct AuthPipeClient {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(not(windows))]
    handle: (),
    timeout: Duration,
}

#[cfg(windows)]
mod raw {
    use windows::core::BOOL;
    use windows::Win32::Foundation::HANDLE;

    // Raw declarations for the two synchronous pipe I/O calls. The `windows`
    // crate gates these behind `Win32_System_IO`, which is not in the pinned
    // feature set; declaring the exact Win32 ABI here keeps the dependency
    // surface unchanged. Both are called with a NULL overlapped pointer, so the
    // handle must be synchronous (it is opened without FILE_FLAG_OVERLAPPED).
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "WriteFile"]
        pub(crate) fn write_file(
            hfile: HANDLE,
            lpbuffer: *const u8,
            nnumberofbytestowrite: u32,
            lpnumberofbyteswritten: *mut u32,
            lpoverlapped: *mut core::ffi::c_void,
        ) -> BOOL;

        #[link_name = "ReadFile"]
        pub(crate) fn read_file(
            hfile: HANDLE,
            lpbuffer: *mut u8,
            nnumberofbytestoread: u32,
            lpnumberofbytesread: *mut u32,
            lpoverlapped: *mut core::ffi::c_void,
        ) -> BOOL;
    }
}

/// Clamp a `Duration` to a Win32 millisecond timeout. Zero means "infinite"
/// for the Win32 APIs used here, so a sub-millisecond duration is rounded up.
fn duration_to_ms(value: Duration) -> u32 {
    let ms = value.as_millis();
    if ms == 0 {
        1
    } else if ms > u32::MAX as u128 {
        u32::MAX
    } else {
        ms as u32
    }
}

#[cfg(windows)]
impl AuthPipeClient {
    /// Wait up to `timeout` for an auth-pipe instance, open it synchronously,
    /// force message-read mode and verify the server PID matches the SCM PID
    /// reported for [`KENALI_SERVICE_NAME`]. The handle is closed on every
    /// failure path.
    pub fn connect(timeout: Duration) -> Result<Self, PipeError> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_WRITE_THROUGH, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        use windows::Win32::System::Pipes::{
            GetNamedPipeServerProcessId, SetNamedPipeHandleState, WaitNamedPipeW,
            PIPE_READMODE_MESSAGE,
        };

        let path: Vec<u16> = AUTH_PIPE_PATH.encode_utf16().chain(core::iter::once(0)).collect();
        // SAFETY: `path` is a local Vec that is NUL-terminated above and stays
        // alive for the whole function, so the PCWSTR is valid for every call.
        let path = PCWSTR(path.as_ptr());

        // SAFETY: `path` is a valid NUL-terminated wide string and the timeout
        // is a finite millisecond value, so WaitNamedPipeW cannot wait forever.
        let ready = unsafe { WaitNamedPipeW(path, duration_to_ms(timeout)) };
        if !ready.as_bool() {
            return Err(PipeError::Timeout);
        }

        // SAFETY: every argument is valid — `path` is NUL-terminated and alive,
        // the desired-access/share-mode/creation flags are plain integers, and
        // the two optional security arguments are None.
        let handle = unsafe {
            CreateFileW(
                path,
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_WRITE_THROUGH,
                None,
            )
        }
        .map_err(|_| PipeError::Unavailable)?;

        let read_mode = PIPE_READMODE_MESSAGE;
        // SAFETY: `handle` is a valid pipe handle from CreateFileW and
        // `read_mode` is a valid pointer to a single NAMED_PIPE_MODE value.
        if unsafe { SetNamedPipeHandleState(handle, Some(&read_mode), None, None) }.is_err() {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(PipeError::Handshake);
        }

        let mut server_pid = 0u32;
        // SAFETY: `handle` is a valid pipe handle and `server_pid` is a valid
        // out-pointer that receives exactly one u32 on success.
        if unsafe { GetNamedPipeServerProcessId(handle, &mut server_pid) }.is_err() {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(PipeError::IdentityMismatch);
        }

        let expected = service_pid(KENALI_SERVICE_NAME)?;
        if server_pid != expected {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(PipeError::IdentityMismatch);
        }

        Ok(Self {
            handle,
            timeout: AUTH_ATTEMPT_TIMEOUT,
        })
    }

    /// Send one framed request and decode one framed response.
    ///
    /// The request is encoded with the pinned codec; the response is decoded
    /// with the same codec under the 4096-byte control cap. A `Hello` response
    /// carrying a foreign `PROTO_VERSION` is rejected, and a `Verified`
    /// response whose echoed nonce does not match a `BeginAttempt` request is
    /// rejected. Every buffer holding wire bytes (including the one-shot
    /// secret) is zeroized on all paths.
    pub fn exchange(&mut self, req: &Request) -> Result<Response, PipeError> {
        let framed = Zeroizing::new(encode(req).map_err(|_| PipeError::Io)?);
        if framed.len() > MAX_CONTROL_BYTES {
            return Err(PipeError::Io);
        }

        let mut written = 0u32;
        // SAFETY: `framed` is a live allocation of `framed.len()` bytes,
        // `written` is a valid out-pointer, `self.handle` is a valid
        // synchronous pipe handle and the NULL overlapped pointer is correct
        // for a synchronous handle.
        let ok = unsafe {
            raw::write_file(
                self.handle,
                framed.as_ptr(),
                framed.len() as u32,
                &mut written,
                core::ptr::null_mut(),
            )
        };
        if !ok.as_bool() || written as usize != framed.len() {
            return Err(PipeError::Io);
        }

        let deadline = Instant::now() + self.timeout;
        let mut buf = Zeroizing::new(vec![0u8; MAX_CONTROL_BYTES]);
        loop {
            let mut available = 0u32;
            // SAFETY: `self.handle` is valid, the buffer pointer is None (this
            // is a query only), and `available` is a valid out-pointer.
            unsafe {
                windows::Win32::System::Pipes::PeekNamedPipe(
                    self.handle,
                    None,
                    0,
                    None,
                    Some(&mut available),
                    None,
                )
            }
            .map_err(|_| PipeError::Io)?;
            if available > 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(PipeError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let mut read = 0u32;
        // SAFETY: `buf` is a live allocation of MAX_CONTROL_BYTES bytes,
        // `read` is a valid out-pointer, `self.handle` is valid and the NULL
        // overlapped pointer matches the synchronous handle.
        let ok = unsafe {
            raw::read_file(
                self.handle,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut read,
                core::ptr::null_mut(),
            )
        };
        if !ok.as_bool() {
            return Err(PipeError::Io);
        }
        let len = read as usize;
        if len == 0 || len > buf.len() {
            return Err(PipeError::Malformed);
        }

        let response: Response = decode(&buf[..len], MAX_CONTROL_BYTES)
            .map_err(|_| PipeError::Malformed)?;

        if let Response::Hello { proto, .. } = &response {
            if *proto != PROTO_VERSION {
                return Err(PipeError::ProtocolMismatch);
            }
        }

        if let Response::Verified { nonce: echoed, .. } = &response {
            match req {
                Request::BeginAttempt { nonce, .. } if echoed == nonce => {}
                _ => return Err(PipeError::NonceMismatch),
            }
        }

        Ok(response)
    }
}

#[cfg(windows)]
impl Drop for AuthPipeClient {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is owned by this value and open until drop.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_path_and_sddl_constants_are_stable() {
        assert_eq!(AUTH_PIPE_PATH, "\\\\.\\pipe\\kenali.auth");
        assert_eq!(AUTH_PIPE_SDDL, "D:P(A;;GA;;;SY)");
    }

    #[test]
    fn timeout_constants_are_bounded() {
        assert_eq!(AUTH_CONNECT_TIMEOUT, Duration::from_millis(200));
        assert_eq!(AUTH_ATTEMPT_TIMEOUT, Duration::from_secs(6));
        assert!(AUTH_ATTEMPT_TIMEOUT.as_secs() <= 6);
    }

    #[test]
    fn duration_to_ms_clamps_and_never_returns_zero() {
        assert_eq!(duration_to_ms(Duration::ZERO), 1);
        assert_eq!(duration_to_ms(Duration::from_millis(200)), 200);
        assert_eq!(duration_to_ms(Duration::from_secs(6)), 6_000);
        assert_eq!(duration_to_ms(Duration::from_secs(u64::MAX)), u32::MAX);
    }

    #[test]
    fn error_display_is_generic() {
        for error in [
            PipeError::Unavailable,
            PipeError::Timeout,
            PipeError::IdentityMismatch,
            PipeError::ProtocolMismatch,
            PipeError::NonceMismatch,
        ] {
            let text = error.to_string();
            assert!(!text.contains("secret"));
            assert!(!text.contains("password"));
            assert!(!text.contains("nonce"));
        }
    }
}
