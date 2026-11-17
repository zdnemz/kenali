//! Minimal SCM service host: registers a single `KenaliSvc` dispatch table
//! via `StartServiceCtrlDispatcherW` and invokes the caller-supplied `run`
//! function on the service main callback.
//!
//! The FFI (dispatch table, service-main and control handler) stays in this
//! crate; `kenali-service` supplies the opaque `fn() -> i32` workload. The
//! workload owns its own stop signalling; this host only reports lifecycle
//! status to the SCM and returns the workload's exit code.

use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::OnceLock;

use crate::service::KENALI_SERVICE_NAME;

static RUN_FN: OnceLock<fn() -> i32> = OnceLock::new();
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);
static STATUS_HANDLE_BITS: AtomicUsize = AtomicUsize::new(0);

/// Connect this process to the SCM as `KenaliSvc` and run `run` until it
/// returns. The returned value is `run`'s exit code, or `1` if the dispatcher
/// could not be registered (for example when the process was not launched by
/// the SCM). Only one dispatch registration per process is meaningful; a
/// second call keeps the first workload function.
pub fn run_service(run: fn() -> i32) -> i32 {
    let _ = RUN_FN.set(run);

    #[cfg(windows)]
    {
        use windows::core::PWSTR;
        use windows::Win32::System::Services::{
            StartServiceCtrlDispatcherW, SERVICE_TABLE_ENTRYW,
        };

        let mut name: Vec<u16> = KENALI_SERVICE_NAME.encode_utf16().chain(core::iter::once(0)).collect();
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: PWSTR(name.as_mut_ptr()),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: PWSTR::null(),
                lpServiceProc: None,
            },
        ];

        // SAFETY: `table` is a live, NUL-terminated dispatch table with a
        // sentinel entry, as required by StartServiceCtrlDispatcherW.
        if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) }.is_err() {
            return 1;
        }
        EXIT_CODE.load(Ordering::SeqCst)
    }

    #[cfg(not(windows))]
    {
        let code = run();
        EXIT_CODE.store(code, Ordering::SeqCst);
        code
    }
}

#[cfg(windows)]
unsafe extern "system" fn service_main(_argc: u32, _argv: *mut windows::core::PWSTR) {
    use windows::core::PCWSTR;
    use windows::Win32::System::Services::{
        RegisterServiceCtrlHandlerW, SERVICE_START_PENDING, SERVICE_STOPPED,
    };

    let name: Vec<u16> = KENALI_SERVICE_NAME.encode_utf16().chain(core::iter::once(0)).collect();
    // SAFETY: `name` is a live NUL-terminated wide string and `control_handler`
    // is a valid handler entry point.
    let status_handle = match unsafe { RegisterServiceCtrlHandlerW(PCWSTR(name.as_ptr()), Some(control_handler)) } {
        Ok(handle) => handle,
        Err(_) => return,
    };
    STATUS_HANDLE_BITS.store(status_handle.0 as usize, Ordering::SeqCst);

    report_status(status_handle, SERVICE_START_PENDING, 0);
    let code = RUN_FN.get().map_or(1, |run| run());
    EXIT_CODE.store(code, Ordering::SeqCst);
    report_status(status_handle, SERVICE_STOPPED, code as u32);
}

#[cfg(windows)]
unsafe extern "system" fn control_handler(control: u32) {
    use windows::Win32::System::Services::{
        SERVICE_CONTROL_SHUTDOWN, SERVICE_CONTROL_STOP, SERVICE_STATUS_HANDLE, SERVICE_STOP_PENDING,
    };

    if control != SERVICE_CONTROL_STOP && control != SERVICE_CONTROL_SHUTDOWN {
        return;
    }
    let bits = STATUS_HANDLE_BITS.load(Ordering::SeqCst);
    if bits == 0 {
        return;
    }
    let handle = SERVICE_STATUS_HANDLE(bits as *mut core::ffi::c_void);
    report_status(handle, SERVICE_STOP_PENDING, 0);
}

#[cfg(windows)]
fn report_status(
    handle: windows::Win32::System::Services::SERVICE_STATUS_HANDLE,
    state: windows::Win32::System::Services::SERVICE_STATUS_CURRENT_STATE,
    exit_code: u32,
) {
    use windows::Win32::System::Services::{
        SetServiceStatus, SERVICE_ACCEPT_STOP, SERVICE_STATUS,
    };

    let status = SERVICE_STATUS {
        dwServiceType: windows::Win32::System::Services::SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: SERVICE_ACCEPT_STOP,
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: 0,
    };
    // SAFETY: `handle` is a valid status handle and `status` is a live, fully
    // initialised structure that is only borrowed for the call.
    let _ = unsafe { SetServiceStatus(handle, &status) };
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn service_name_matches_event_source() {
        assert_eq!(KENALI_SERVICE_NAME, "KenaliSvc");
        assert_eq!(crate::eventlog::EVENT_SOURCE, KENALI_SERVICE_NAME);
    }

    #[test]
    fn initial_exit_code_is_zero() {
        assert_eq!(EXIT_CODE.load(Ordering::SeqCst), 0);
    }
}
