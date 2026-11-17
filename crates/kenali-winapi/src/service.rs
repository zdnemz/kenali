//! SCM and WTS helpers used by the auth-pipe client and the credential
//! provider. No install/registration command lives here — these are read-only
//! queries.

use crate::pipes::PipeError;

/// SCM name of the Kenali service (not the DLL/executable file name).
pub const KENALI_SERVICE_NAME: &str = "KenaliSvc";

/// Return the PID the SCM reports for `name`, or a generic error when the
/// service is absent or cannot be queried. `QueryServiceStatusEx` is a local,
/// bounded SCM call.
#[cfg(windows)]
pub fn service_pid(name: &str) -> Result<u32, PipeError> {
    use windows::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_MANAGER_CONNECT,
        SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_STATUS_PROCESS,
    };
    use windows::core::PCWSTR;

    let name: Vec<u16> = name.encode_utf16().chain(core::iter::once(0)).collect();
    // SAFETY: `name` is a local NUL-terminated Vec that outlives all calls.
    let name = PCWSTR(name.as_ptr());

    // SAFETY: both name arguments are None (local machine / default database)
    // and SC_MANAGER_CONNECT is a read-only SCM access mask.
    let manager = unsafe { OpenSCManagerW(None, None, SC_MANAGER_CONNECT) }
        .map_err(|_| PipeError::Service)?;

    // SAFETY: `manager` is a valid SCM handle and `name` is a valid
    // NUL-terminated service name; SERVICE_QUERY_STATUS is read-only.
    let service = unsafe { OpenServiceW(manager, name, SERVICE_QUERY_STATUS) }
        .map_err(|_| PipeError::Service)?;
    // SAFETY: `manager` is owned here and no longer needed after opening the
    // service.
    let _ = unsafe { CloseServiceHandle(manager) };

    let mut status = SERVICE_STATUS_PROCESS::default();
    // SAFETY: `bytes` points to a live, correctly-sized `SERVICE_STATUS_PROCESS`
    // allocation and `needed` is a valid out-pointer. The slice does not
    // outlive `status`.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(
            &mut status as *mut SERVICE_STATUS_PROCESS as *mut u8,
            core::mem::size_of::<SERVICE_STATUS_PROCESS>(),
        )
    };
    let mut needed = 0u32;
    // SAFETY: `service` is a valid handle, `bytes` is a valid writable buffer
    // and SC_STATUS_PROCESS_INFO matches the SERVICE_STATUS_PROCESS layout.
    let result = unsafe {
        QueryServiceStatusEx(service, SC_STATUS_PROCESS_INFO, Some(bytes), &mut needed)
    };
    // SAFETY: `service` is owned here and open until this point.
    let _ = unsafe { CloseServiceHandle(service) };
    result.map_err(|_| PipeError::Service)?;

    Ok(status.dwProcessId)
}

/// Return the SID string of the user in the active console session, or `None`
/// when no interactive user is present. `WTSQueryUserToken` requires SYSTEM;
/// the credential provider runs under that account.
#[cfg(windows)]
pub fn current_session_sid() -> Result<Option<String>, PipeError> {
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows::Win32::Security::{GetTokenInformation, TOKEN_USER, TokenUser};
    use windows::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
    use windows::core::PWSTR;

    // SAFETY: WTSGetActiveConsoleSessionId takes no arguments and has no
    // preconditions.
    let session = unsafe { WTSGetActiveConsoleSessionId() };
    let mut token = HANDLE(core::ptr::null_mut());
    // SAFETY: `token` is a valid out-pointer and `session` is the value
    // returned by WTSGetActiveConsoleSessionId.
    if unsafe { WTSQueryUserToken(session, &mut token) }.is_err() {
        return Ok(None);
    }

    let mut needed = 0u32;
    // SAFETY: `token` is a valid user token, the information class is
    // TokenUser and the buffer/out-pointer sizes match TOKEN_USER. A failure
    // here merely fills `needed`, which is used for the real allocation.
    let _ = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            None,
            0,
            &mut needed,
        )
    };
    if needed == 0 {
        // SAFETY: `token` is owned here and open.
        let _ = unsafe { CloseHandle(token) };
        return Ok(None);
    }

    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: `buffer` is a live allocation of `needed` bytes, `token` is
    // valid, and the class/length match the TOKEN_USER layout.
    let queried = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr() as *mut core::ffi::c_void),
            needed,
            &mut needed,
        )
    };
    // SAFETY: `token` is owned here and no longer needed.
    let _ = unsafe { CloseHandle(token) };
    queried.map_err(|_| PipeError::Service)?;

    // SAFETY: `buffer` is at least `size_of::<TOKEN_USER>()` bytes (the API
    // succeeded with the reported length) and TOKEN_USER is `Copy`/POD.
    let user: TOKEN_USER = unsafe { core::ptr::read(buffer.as_ptr() as *const TOKEN_USER) };

    let mut sid_string = PWSTR::default();
    // SAFETY: `user.User.Sid` is a valid SID pointer supplied by
    // GetTokenInformation into `buffer`, which is still alive, and
    // `sid_string` is a valid out-pointer receiving a LocalAlloc'd string.
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_string) }
        .map_err(|_| PipeError::Service)?;
    // SAFETY: `sid_string` points to a NUL-terminated wide string allocated by
    // ConvertSidToStringSidW and still valid until LocalFree below.
    let sid = unsafe { sid_string.to_string() }.map_err(|_| PipeError::Service)?;
    // SAFETY: `sid_string` was allocated by ConvertSidToStringSidW with
    // LocalAlloc, so LocalFree is the matching deallocator.
    let _ = unsafe { LocalFree(Some(HLOCAL(sid_string.0 as *mut core::ffi::c_void))) };

    Ok(Some(sid))
}

/// `true` on non-Windows hosts and on Windows when the current token is not
/// elevated. Used by the installer to refuse to run in an unelevated context.
#[cfg(not(windows))]
pub fn needs_elevation() -> bool {
    true
}

/// See the non-Windows definition: returns `true` when elevation is needed.
#[cfg(windows)]
pub fn needs_elevation() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: GetCurrentProcess has no arguments and returns a pseudo-handle
    // that does not need closing.
    let process = unsafe { GetCurrentProcess() };
    let mut token = HANDLE(core::ptr::null_mut());
    // SAFETY: `process` is a valid pseudo-handle, TOKEN_QUERY is a read-only
    // access mask and `token` is a valid out-pointer.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.is_err() {
        return true;
    }

    let mut elevation = TOKEN_ELEVATION::default();
    let mut needed = 0u32;
    // SAFETY: `token` is valid, the class is TokenElevation, and
    // `elevation`/`needed` are correctly-sized valid out-pointers.
    let queried = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut TOKEN_ELEVATION as *mut core::ffi::c_void),
            core::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut needed,
        )
    };
    // SAFETY: `token` is owned here and open.
    let _ = unsafe { CloseHandle(token) };

    match queried {
        Ok(()) => elevation.TokenIsElevated == 0,
        Err(_) => true,
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn service_name_is_stable() {
        assert_eq!(KENALI_SERVICE_NAME, "KenaliSvc");
    }

    #[test]
    fn non_windows_needs_elevation_is_true() {
        #[cfg(not(windows))]
        assert!(needs_elevation());
    }
}
