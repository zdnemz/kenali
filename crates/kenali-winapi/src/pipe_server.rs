//! Server-side creation and acceptance of the admin and preview pipes.
//!
//! The auth-pipe *client* lives in [`crate::pipes`]; this module is the
//! *server* side for the two management channels. Each instance is created
//! with `FILE_FLAG_FIRST_PIPE_INSTANCE`, message mode,
//! `PIPE_REJECT_REMOTE_CLIENTS` and an SDDL that grants only SYSTEM and
//! elevated Administrators. A client connection is accepted with a bounded
//! overlapped `ConnectNamedPipe` (no unbounded wait) and its PID is checked
//! before any request bytes are read.

use std::time::Instant;

use crate::pipes::PipeError;
#[cfg(windows)]
use crate::WinHandle;

/// Path of the admin pipe (displayed `\\.\pipe\kenali.admin`).
pub const ADMIN_PIPE_PATH: &str = "\\\\.\\pipe\\kenali.admin";
/// Path of the preview pipe, open only during enrolment/calibration windows
/// (displayed `\\.\pipe\kenali.preview`).
pub const PREVIEW_PIPE_PATH: &str = "\\\\.\\pipe\\kenali.preview";
/// SDDL for the admin pipe DACL: SYSTEM and built-in Administrators.
pub const ADMIN_PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)";
/// SDDL for the preview pipe DACL: identical to the admin pipe.
pub const PREVIEW_PIPE_SDDL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;BA)";

#[cfg(windows)]
const PIPE_BUFFER_SIZE: u32 = 4096;
#[cfg(windows)]
const PIPE_MAX_INSTANCES: u32 = 1;

/// Convert a `Duration`-free deadline into a Win32 millisecond timeout that
/// is safe to pass to `GetOverlappedResultEx` (which treats `0` as "no wait",
/// never as "infinite").
#[cfg(windows)]
fn remaining_ms(deadline: Instant) -> u32 {
    let now = Instant::now();
    if now >= deadline {
        return 0;
    }
    let ms = (deadline - now).as_millis();
    if ms > u32::MAX as u128 {
        u32::MAX
    } else {
        ms as u32
    }
}

/// Create one server-side named-pipe instance at `path` whose DACL is built
/// from `sddl`. The first instance is claimed with `FIRST_PIPE_INSTANCE`, the
/// pipe is message-mode and rejects remote clients. The security descriptor
/// is freed on every path; a new handle is returned on success.
#[cfg(windows)]
pub fn create_server_pipe(path: &str, sddl: &str) -> Result<WinHandle, PipeError> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
    };
    use windows::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE,
        PIPE_WAIT,
    };

    let sddl_wide: Vec<u16> = sddl.encode_utf16().chain(core::iter::once(0)).collect();
    let mut sd = PSECURITY_DESCRIPTOR(core::ptr::null_mut());
    // SAFETY: `sddl_wide` is a local, NUL-terminated Vec that outlives the
    // call; `sd` is a valid out-pointer and SDDL_REVISION_1 is the correct
    // revision. The returned descriptor is LocalAlloc'd and freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl_wide.as_ptr()),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )
    }
    .map_err(|_| PipeError::Unavailable)?;

    let attributes = SECURITY_ATTRIBUTES {
        nLength: core::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: windows::core::BOOL(0),
    };

    let path_wide: Vec<u16> = path.encode_utf16().chain(core::iter::once(0)).collect();
    let mode = PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    // SAFETY: `path_wide` is NUL-terminated and alive for the call;
    // `attributes` points at a valid descriptor that stays alive until after
    // the call; all integer arguments are plain flags/sizes.
    let handle = unsafe {
        CreateNamedPipeW(
            PCWSTR(path_wide.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            mode,
            PIPE_MAX_INSTANCES,
            PIPE_BUFFER_SIZE,
            PIPE_BUFFER_SIZE,
            0,
            Some(&attributes),
        )
    };
    // SAFETY: `sd` was allocated by ConvertStringSecurityDescriptor... with
    // LocalAlloc, so LocalFree is the matching deallocator. CreateNamedPipeW
    // copied the DACL into the kernel object, so it is safe to free now.
    let _ = unsafe { LocalFree(Some(HLOCAL(sd.0))) };

    if handle.is_invalid() {
        return Err(PipeError::Unavailable);
    }
    Ok(handle)
}

/// Accept a single client on a listening pipe handle, waiting at most until
/// `deadline`. The wait is an overlapped `ConnectNamedPipe` bounded by
/// `GetOverlappedResultEx`; it is never unbounded. On success the (now
/// connected) handle is returned; on every error path the event object and
/// the pipe handle are closed.
#[cfg(windows)]
pub fn accept_client(handle: WinHandle, deadline: Instant) -> Result<WinHandle, PipeError> {
    use windows::core::HRESULT;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED,
    };
    use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResultEx, OVERLAPPED};
    use windows::Win32::System::Pipes::ConnectNamedPipe;
    use windows::Win32::System::Threading::CreateEventW;

    // SAFETY: CreateEventW takes an optional security attribute (None), a
    // manual-reset flag, an initial state and an optional name (None); there
    // are no preconditions beyond these plain values.
    let event = unsafe { CreateEventW(None, true, false, None) }.map_err(|_| PipeError::Io)?;
    let mut overlapped = OVERLAPPED {
        hEvent: event,
        ..Default::default()
    };

    // SAFETY: `handle` is a valid overlapped pipe handle from
    // `create_server_pipe`, and `overlapped` is a live, correctly initialised
    // structure whose `hEvent` is the event created above.
    let connect = unsafe { ConnectNamedPipe(handle, Some(&mut overlapped)) };

    let pending = match connect {
        Ok(()) => false,
        Err(ref e) if e.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => true,
        // A client connected before ConnectNamedPipe was called: the event is
        // not signalled but the pipe is already connected.
        Err(ref e) if e.code() == HRESULT::from_win32(ERROR_PIPE_CONNECTED.0) => false,
        Err(_) => {
            // SAFETY: `event` and `handle` are owned here and still open.
            let _ = unsafe {
                let _ = CloseHandle(event);
                let _ = CloseHandle(handle);
            };
            return Err(PipeError::Io);
        }
    };

    if pending {
        let mut transferred = 0u32;
        // SAFETY: `handle` is valid, `overlapped` is the matching live
        // structure, `transferred` is a valid out-pointer and the timeout is
        // a finite millisecond value (0 means "no wait", never "infinite").
        let connected = unsafe {
            GetOverlappedResultEx(handle, &overlapped, &mut transferred, remaining_ms(deadline), false)
        };
        if connected.is_err() {
            // Abort the still-pending connect before releasing the objects.
            // SAFETY: `handle` is valid and `overlapped` is the live structure
            // describing the pending operation.
            let _ = unsafe { CancelIoEx(handle, Some(&overlapped)) };
            // SAFETY: `event` and `handle` are owned here and still open.
            let _ = unsafe {
                let _ = CloseHandle(event);
                let _ = CloseHandle(handle);
            };
            return Err(PipeError::Timeout);
        }
    }

    // SAFETY: `event` is owned by this function and still open.
    let _ = unsafe { CloseHandle(event) };
    Ok(handle)
}

/// Return the PID of the process on the other end of a connected pipe.
#[cfg(windows)]
pub fn client_pid(handle: WinHandle) -> Result<u32, PipeError> {
    use windows::Win32::System::Pipes::GetNamedPipeClientProcessId;

    let mut pid = 0u32;
    // SAFETY: `handle` is a valid pipe handle and `pid` is a valid out-pointer
    // receiving exactly one u32 on success.
    unsafe { GetNamedPipeClientProcessId(handle, &mut pid) }
        .map_err(|_| PipeError::IdentityMismatch)?;
    Ok(pid)
}

/// Verify that the connected client is `expected_pid` before any request is
/// read. A mismatch is an identity failure with a generic error message.
#[cfg(windows)]
pub fn verify_peer_pid(handle: WinHandle, expected_pid: u32) -> Result<(), PipeError> {
    if client_pid(handle)? != expected_pid {
        return Err(PipeError::IdentityMismatch);
    }
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn pipe_paths_and_sddls_are_stable() {
        assert_eq!(ADMIN_PIPE_PATH, "\\\\.\\pipe\\kenali.admin");
        assert_eq!(PREVIEW_PIPE_PATH, "\\\\.\\pipe\\kenali.preview");
        assert_eq!(ADMIN_PIPE_SDDL, "D:P(A;;GA;;;SY)(A;;GA;;;BA)");
        assert_eq!(PREVIEW_PIPE_SDDL, ADMIN_PIPE_SDDL);
    }

    #[test]
    fn management_sddl_grants_only_system_and_administrators() {
        assert_eq!(crate::pipes::AUTH_PIPE_SDDL, "D:P(A;;GA;;;SY)");
        assert!(ADMIN_PIPE_SDDL.contains("(A;;GA;;;SY)"));
        assert!(ADMIN_PIPE_SDDL.contains("(A;;GA;;;BA)"));
    }
}
