//! Typed preview-pipe client (service ⇄ management UI).
//!
//! The preview pipe path is re-used from [`crate::pipe_server`]. The pipe exists
//! only while an enrolment/calibration preview window is active. The server PID
//! is verified before the handle is retained, response buffers are bounded by
//! [`MAX_PREVIEW_BYTES`] and zeroized, and every wait is bounded by an absolute
//! deadline supplied by the caller.

use core::time::Duration;
use std::time::Instant;

use thiserror::Error;
use zeroize::Zeroizing;

use kenali_ipc::codec::{decode, MAX_PREVIEW_BYTES};
use kenali_ipc::preview::PreviewFrame;

use crate::pipe_server::PREVIEW_PIPE_PATH;
use crate::service::{service_pid, KENALI_SERVICE_NAME};

/// Errors from the preview-pipe client. Wording is intentionally generic: no
/// frame data, internal code or Win32 detail is ever exposed.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewPipeError {
    #[error("preview pipe is unavailable")]
    Unavailable,
    #[error("timed out contacting preview pipe")]
    Timeout,
    #[error("preview pipe handshake failed")]
    Handshake,
    #[error("server identity could not be verified")]
    IdentityMismatch,
    #[error("service lookup failed")]
    Service,
    #[error("pipe I/O failed")]
    Io,
    #[error("malformed frame")]
    Malformed,
}

/// A connected, identity-verified handle to the preview pipe.
pub struct PreviewPipeClient {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(not(windows))]
    handle: (),
}

#[cfg(windows)]
mod raw {
    use windows::core::BOOL;
    use windows::Win32::Foundation::HANDLE;

    // Raw declaration for synchronous pipe reads, matching `crate::pipes`.
    // The handle is opened without FILE_FLAG_OVERLAPPED, so a NULL overlapped
    // pointer is correct.
    #[link(name = "kernel32")]
    unsafe extern "system" {
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
impl PreviewPipeClient {
    /// Wait up to `timeout` for a preview-pipe instance, open it synchronously,
    /// force message-read mode and verify the server PID matches the SCM PID.
    /// The handle is closed on every failure path.
    pub fn connect(timeout: Duration) -> Result<Self, PreviewPipeError> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE};
        use windows::Win32::Storage::FileSystem::{
            CreateFileW, FILE_FLAG_WRITE_THROUGH, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        use windows::Win32::System::Pipes::{
            GetNamedPipeServerProcessId, SetNamedPipeHandleState, WaitNamedPipeW,
            PIPE_READMODE_MESSAGE,
        };

        let path: Vec<u16> = PREVIEW_PIPE_PATH
            .encode_utf16()
            .chain(core::iter::once(0))
            .collect();
        // SAFETY: `path` is a local NUL-terminated Vec that outlives the call.
        let path = PCWSTR(path.as_ptr());

        // SAFETY: `path` is valid and NUL-terminated, and the timeout is a
        // finite millisecond value, so WaitNamedPipeW cannot wait forever.
        let ready = unsafe { WaitNamedPipeW(path, duration_to_ms(timeout)) };
        if !ready.as_bool() {
            return Err(PreviewPipeError::Timeout);
        }

        // SAFETY: every argument is valid — `path` is NUL-terminated and alive,
        // the access/share/creation flags are plain integers and the optional
        // security arguments are None.
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
        .map_err(|_| PreviewPipeError::Unavailable)?;

        let read_mode = PIPE_READMODE_MESSAGE;
        // SAFETY: `handle` is a valid pipe handle and `read_mode` points to a
        // single NAMED_PIPE_MODE value.
        if unsafe { SetNamedPipeHandleState(handle, Some(&read_mode), None, None) }.is_err() {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(PreviewPipeError::Handshake);
        }

        let mut server_pid = 0u32;
        // SAFETY: `handle` is a valid pipe handle and `server_pid` is a valid
        // out-pointer receiving exactly one u32 on success.
        if unsafe { GetNamedPipeServerProcessId(handle, &mut server_pid) }.is_err() {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(PreviewPipeError::IdentityMismatch);
        }

        let expected = service_pid(KENALI_SERVICE_NAME).map_err(|_| PreviewPipeError::Service)?;
        if server_pid != expected {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(PreviewPipeError::IdentityMismatch);
        }

        Ok(Self { handle })
    }

    /// Read one framed preview message before `deadline` and decode it under
    /// [`MAX_PREVIEW_BYTES`]. The wire buffer is zeroized on every path.
    pub fn recv_frame(&mut self, deadline: Instant) -> Result<PreviewFrame, PreviewPipeError> {
        let mut buf = Zeroizing::new(vec![0u8; MAX_PREVIEW_BYTES]);
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
            .map_err(|_| PreviewPipeError::Io)?;
            if available > 0 {
                break;
            }
            if Instant::now() >= deadline {
                return Err(PreviewPipeError::Timeout);
            }
            std::thread::sleep(Duration::from_millis(5));
        }

        let mut read = 0u32;
        // SAFETY: `buf` is a live allocation of MAX_PREVIEW_BYTES bytes,
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
            return Err(PreviewPipeError::Io);
        }
        let len = read as usize;
        if len == 0 || len > buf.len() {
            return Err(PreviewPipeError::Malformed);
        }

        decode(&buf[..len], MAX_PREVIEW_BYTES).map_err(|_| PreviewPipeError::Malformed)
    }
}

#[cfg(windows)]
impl Drop for PreviewPipeClient {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is owned by this value and open until drop.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn duration_to_ms_clamps_and_never_returns_zero() {
        assert_eq!(duration_to_ms(Duration::ZERO), 1);
        assert_eq!(duration_to_ms(Duration::from_millis(250)), 250);
        assert_eq!(duration_to_ms(Duration::from_secs(u64::MAX)), u32::MAX);
    }

    #[test]
    fn error_display_is_generic() {
        for error in [
            PreviewPipeError::Unavailable,
            PreviewPipeError::Timeout,
            PreviewPipeError::IdentityMismatch,
        ] {
            let text = error.to_string();
            assert!(!text.contains("frame"));
            assert!(!text.contains("S-1-"));
            assert!(!text.contains("C:\\"));
        }
    }
}
