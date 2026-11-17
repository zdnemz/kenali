//! Typed admin-pipe client (UI / recovery CLI ⇄ service).
//!
//! This is the only admin-pipe client: both the UI and the recovery CLI use it.
//! The pipe path and SDDL are re-used from [`crate::pipe_server`]. Every call is
//! bounded: [`ADMIN_CONNECT_TIMEOUT`] gates `WaitNamedPipeW` and
//! [`ADMIN_ATTEMPT_TIMEOUT`] gates the response wait. The server PID is checked
//! against the SCM PID before any request bytes are written.
//!
//! Overlapped I/O is intentionally avoided: the handle is opened synchronously
//! (message-read mode) and the response wait is bounded by polling
//! `PeekNamedPipe` against a deadline. All wire buffers are zeroized.

use core::time::Duration;
use std::time::Instant;

use thiserror::Error;
use zeroize::Zeroizing;

use kenali_ipc::admin::{AdminRequest, AdminResponse};
use kenali_ipc::codec::{decode, encode, MAX_CONTROL_BYTES};
use kenali_ipc::PROTO_VERSION;

use crate::pipe_server::ADMIN_PIPE_PATH;
use crate::service::{service_pid, KENALI_SERVICE_NAME};

/// Maximum time spent waiting for an admin-pipe instance to become available.
pub const ADMIN_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
/// Maximum time spent waiting for a response to one exchange (diagnostics
/// batches are slow).
pub const ADMIN_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);

/// Errors from the admin-pipe client. Wording is intentionally generic: no
/// secret, internal code or Win32 detail is ever exposed.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdminPipeError {
    #[error("administration pipe is unavailable")]
    Unavailable,
    #[error("timed out contacting administration pipe")]
    Timeout,
    #[error("administration pipe handshake failed")]
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
}

/// A connected, identity-verified handle to the admin pipe.
pub struct AdminPipeClient {
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

    // Raw declarations for the two synchronous pipe I/O calls, matching the
    // pattern used by `crate::pipes`. The handle is opened without
    // FILE_FLAG_OVERLAPPED, so a NULL overlapped pointer is correct.
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

/// Clamp a `Duration` to a Win32 millisecond timeout. Zero means "infinite" for
/// the Win32 APIs used here, so a sub-millisecond duration is rounded up.
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
impl AdminPipeClient {
    /// Wait up to `timeout` for an admin-pipe instance, open it synchronously,
    /// force message-read mode and verify the server PID matches the SCM PID
    /// reported for [`KENALI_SERVICE_NAME`]. The handle is closed on every
    /// failure path.
    pub fn connect(timeout: Duration) -> Result<Self, AdminPipeError> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_WRITE_THROUGH, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        use windows::Win32::System::Pipes::{
            GetNamedPipeServerProcessId, SetNamedPipeHandleState, WaitNamedPipeW,
            PIPE_READMODE_MESSAGE,
        };

        let path: Vec<u16> = ADMIN_PIPE_PATH
            .encode_utf16()
            .chain(core::iter::once(0))
            .collect();
        // SAFETY: `path` is a local NUL-terminated Vec that stays alive for the
        // whole function, so the PCWSTR is valid for every call.
        let path = PCWSTR(path.as_ptr());

        // SAFETY: `path` is a valid NUL-terminated wide string and the timeout
        // is a finite millisecond value, so WaitNamedPipeW cannot wait forever.
        let ready = unsafe { WaitNamedPipeW(path, duration_to_ms(timeout)) };
        if !ready.as_bool() {
            return Err(AdminPipeError::Timeout);
        }

        // SAFETY: every argument is valid — `path` is NUL-terminated and alive,
        // the access/share/creation flags are plain integers and the two
        // optional security arguments are None.
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
        .map_err(|_| AdminPipeError::Unavailable)?;

        let read_mode = PIPE_READMODE_MESSAGE;
        // SAFETY: `handle` is a valid pipe handle and `read_mode` points to a
        // single NAMED_PIPE_MODE value.
        if unsafe { SetNamedPipeHandleState(handle, Some(&read_mode), None, None) }.is_err() {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(AdminPipeError::Handshake);
        }

        let mut server_pid = 0u32;
        // SAFETY: `handle` is a valid pipe handle and `server_pid` is a valid
        // out-pointer receiving exactly one u32 on success.
        if unsafe { GetNamedPipeServerProcessId(handle, &mut server_pid) }.is_err() {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(AdminPipeError::IdentityMismatch);
        }

        let expected = service_pid(KENALI_SERVICE_NAME).map_err(|_| AdminPipeError::Service)?;
        if server_pid != expected {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(AdminPipeError::IdentityMismatch);
        }

        Ok(Self {
            handle,
            timeout: ADMIN_ATTEMPT_TIMEOUT,
        })
    }

    /// Send one framed admin request and decode one framed admin response.
    /// Every wire buffer is zeroized; a `Hello` reply carrying a foreign
    /// `PROTO_VERSION` is rejected.
    pub fn exchange(&mut self, req: &AdminRequest) -> Result<AdminResponse, AdminPipeError> {
        let framed = Zeroizing::new(encode(req).map_err(|_| AdminPipeError::Io)?);
        if framed.len() > MAX_CONTROL_BYTES {
            return Err(AdminPipeError::Io);
        }

        let mut written = 0u32;
        // SAFETY: `framed` is a live allocation of `framed.len()` bytes,
        // `written` is a valid out-pointer, `self.handle` is a valid
        // synchronous pipe handle and the NULL overlapped pointer matches it.
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
            return Err(AdminPipeError::Io);
        }

        let deadline = Instant::now() + self.timeout;
        let mut buf = Zeroizing::new(vec![0u8; MAX_CONTROL_BYTES]);
        loop {
            let mut available = 0u32;
            // SAFETY: `self.handle` is valid, the buffer pointer is None (this
            // is a query only) and `available` is a valid out-pointer.
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
            .map_err(|_| AdminPipeError::Io)?;
            if available > 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(AdminPipeError::Timeout);
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
            return Err(AdminPipeError::Io);
        }
        let len = read as usize;
        if len == 0 || len > buf.len() {
            return Err(AdminPipeError::Malformed);
        }

        let response: AdminResponse =
            decode(&buf[..len], MAX_CONTROL_BYTES).map_err(|_| AdminPipeError::Malformed)?;

        if let AdminResponse::Hello { proto, .. } = &response {
            if *proto != PROTO_VERSION {
                return Err(AdminPipeError::ProtocolMismatch);
            }
        }

        Ok(response)
    }
}

#[cfg(windows)]
impl Drop for AdminPipeClient {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is owned by this value and open until drop.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn timeout_constants_are_bounded() {
        assert_eq!(ADMIN_CONNECT_TIMEOUT, Duration::from_millis(500));
        assert_eq!(ADMIN_ATTEMPT_TIMEOUT, Duration::from_secs(30));
    }

    #[test]
    fn duration_to_ms_clamps_and_never_returns_zero() {
        assert_eq!(duration_to_ms(Duration::ZERO), 1);
        assert_eq!(duration_to_ms(Duration::from_millis(500)), 500);
        assert_eq!(duration_to_ms(Duration::from_secs(30)), 30_000);
        assert_eq!(duration_to_ms(Duration::from_secs(u64::MAX)), u32::MAX);
    }

    #[test]
    fn error_display_is_generic() {
        for error in [
            AdminPipeError::Unavailable,
            AdminPipeError::Timeout,
            AdminPipeError::IdentityMismatch,
            AdminPipeError::ProtocolMismatch,
        ] {
            let text = error.to_string();
            assert!(!text.contains("password"));
            assert!(!text.contains("S-1-"));
            assert!(!text.contains("C:\\"));
        }
    }
}
