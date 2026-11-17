//! Operator-UI Win32 helpers: single-instance, elevation relaunch, focus and
//! per-monitor DPI awareness.
//!
//! All errors use generic `Display` text — no absolute path, SID, secret or
//! Win32 detail is ever surfaced. Every unsafe block carries a `// SAFETY:`
//! comment. Runtime behaviour (UAC, DPI, focus) is owner-run; this module is
//! type-checked on the host.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::sync::OnceLock;

use thiserror::Error;

use crate::service::needs_elevation;

/// Named mutex that distinguishes the first UI process from later launches.
/// Displayed as `Local\KenaliUiSingleInstance`.
pub const UI_SINGLE_INSTANCE_MUTEX: &str = r"Local\KenaliUiSingleInstance";

/// Window title used to find an already-running UI window. This must match the
/// title the eframe window is created with.
pub const UI_WINDOW_TITLE: &str = "Kenali";

/// Errors from the UI helpers. Wording is intentionally generic: no path, SID,
/// secret or Win32 detail is ever exposed.
#[derive(Debug, Error, PartialEq, Eq, Clone, Copy)]
#[non_exhaustive]
pub enum UiHelperError {
    #[error("another instance is already running")]
    AlreadyRunning,
    #[error("could not initialise single-instance state")]
    SingleInstance,
    #[error("could not start an elevated instance")]
    ElevationFailed,
    #[error("could not locate the executable")]
    Executable,
    #[error("could not focus the existing window")]
    Focus,
    #[error("could not configure display scaling")]
    Dpi,
}

/// Owns the single-instance mutex for the lifetime of the UI process. Not
/// `Clone`: dropping this value releases the mutex and allows a second launch.
pub struct UiInstance {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
    #[cfg(not(windows))]
    handle: (),
}

#[cfg(windows)]
mod raw {
    use windows::core::BOOL;

    // `SetProcessDpiAwarenessContext` lives in `Win32_UI_HiDpi`, which is not in
    // the pinned windows feature set. Declaring the exact Win32 ABI here keeps
    // the dependency surface unchanged. The parameter is the pointer-sized
    // DPI_AWARENESS_CONTEXT value; -4 is PER_MONITOR_AWARE_V2.
    #[link(name = "user32")]
    unsafe extern "system" {
        #[link_name = "SetProcessDpiAwarenessContext"]
        pub(crate) fn set_process_dpi_awareness_context(value: isize) -> BOOL;
    }
}

/// Convert a path to a NUL-terminated wide string.
#[cfg(windows)]
fn to_wide(path: &Path) -> Result<Vec<u16>, UiHelperError> {
    let wide: Vec<u16> = OsStr::new(path.as_os_str())
        .encode_wide()
        .chain(core::iter::once(0))
        .collect();
    if wide.len() > u32::MAX as usize {
        return Err(UiHelperError::Executable);
    }
    Ok(wide)
}

#[cfg(windows)]
impl UiInstance {
    /// Create (or open) the single-instance mutex. A second launch gets
    /// [`UiHelperError::AlreadyRunning`] and must not open a window.
    pub fn acquire() -> Result<Self, UiHelperError> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::System::Threading::CreateMutexW;

        let name: Vec<u16> = UI_SINGLE_INSTANCE_MUTEX
            .encode_utf16()
            .chain(core::iter::once(0))
            .collect();
        // SAFETY: `name` is a local NUL-terminated Vec that outlives the call.
        let name = PCWSTR(name.as_ptr());

        // SAFETY: the security attributes are None (default DACL), the initial
        // owner flag is a plain bool and `name` is a valid NUL-terminated wide
        // string. A returned handle, if any, is owned and closed on error.
        let handle = unsafe { CreateMutexW(None, true, name) }
            .map_err(|_| UiHelperError::SingleInstance)?;

        // SAFETY: GetLastError takes no arguments and has no preconditions. Its
        // value is meaningful only immediately after the call above.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: `handle` is owned by this function and still open.
            let _ = unsafe { CloseHandle(handle) };
            return Err(UiHelperError::AlreadyRunning);
        }

        Ok(Self { handle })
    }
}

#[cfg(windows)]
impl Drop for UiInstance {
    fn drop(&mut self) {
        // SAFETY: `self.handle` is owned by this value and open until drop.
        let _ = unsafe { windows::Win32::Foundation::CloseHandle(self.handle) };
    }
}

/// Acquire the UI single-instance mutex for this process.
#[cfg(windows)]
pub fn acquire_ui_instance() -> Result<UiInstance, UiHelperError> {
    UiInstance::acquire()
}

/// If the current token is not elevated, spawn an elevated copy of this
/// executable with the `runas` verb and return `Ok(true)` so the caller exits.
/// Returns `Ok(false)` when already elevated (the caller continues).
#[cfg(windows)]
pub fn relaunch_elevated() -> Result<bool, UiHelperError> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_DEFAULT};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    if !needs_elevation() {
        return Ok(false);
    }

    let exe = std::env::current_exe().map_err(|_| UiHelperError::Executable)?;
    let exe_wide = to_wide(&exe)?;
    let verb_wide: Vec<u16> = "runas".encode_utf16().chain(core::iter::once(0)).collect();

    let mut info = SHELLEXECUTEINFOW {
        cbSize: core::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_DEFAULT,
        lpVerb: PCWSTR(verb_wide.as_ptr()),
        lpFile: PCWSTR(exe_wide.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };

    // SAFETY: `info` is a live, correctly initialised SHELLEXECUTEINFOW whose
    // wide string pointers reference locals that outlive the call, and `cbSize`
    // is set to the structure size. ShellExecuteExW is the matching function.
    unsafe { ShellExecuteExW(&mut info) }.map_err(|_| UiHelperError::ElevationFailed)?;

    Ok(true)
}

/// Best-effort attempt to bring an already-running UI window to the foreground.
/// Failure is not fatal: the caller may still proceed or report its own state.
#[cfg(windows)]
pub fn focus_existing_instance() -> Result<(), UiHelperError> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow};

    let title: Vec<u16> = UI_WINDOW_TITLE
        .encode_utf16()
        .chain(core::iter::once(0))
        .collect();

    // SAFETY: the class name is None and `title` is a live, NUL-terminated wide
    // string, which are the only preconditions of FindWindowW.
    let hwnd = unsafe { FindWindowW(None, PCWSTR(title.as_ptr())) }
        .map_err(|_| UiHelperError::Focus)?;

    // SAFETY: `hwnd` is a valid window handle returned by FindWindowW;
    // SetForegroundWindow has no further preconditions and is best-effort.
    let _ = unsafe { SetForegroundWindow(hwnd) };
    Ok(())
}

/// Enable per-monitor DPI awareness v2 for this process. Idempotent: calling it
/// again (or after the process was made aware via its manifest) is a no-op.
#[cfg(windows)]
pub fn set_per_monitor_dpi_awareness() -> Result<(), UiHelperError> {
    static DPI_STATE: OnceLock<Result<(), UiHelperError>> = OnceLock::new();
    DPI_STATE
        .get_or_init(|| {
            // -4 == DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2.
            const PER_MONITOR_AWARE_V2: isize = -4;
            // SAFETY: the argument is a valid DPI_AWARENESS_CONTEXT value and
            // SetProcessDpiAwarenessContext has no other preconditions. A FALSE
            // return means awareness was already set, which is fine here.
            let _ok = unsafe { raw::set_process_dpi_awareness_context(PER_MONITOR_AWARE_V2) };
            Ok(())
        })
        .clone()
}

// Non-Windows stubs keep the crate's API surface available on the host. They
// perform no Win32 work and are never called by host tests.
#[cfg(not(windows))]
pub struct UiInstance {
    handle: (),
}

#[cfg(not(windows))]
pub fn acquire_ui_instance() -> Result<UiInstance, UiHelperError> {
    Ok(UiInstance { handle: () })
}

#[cfg(not(windows))]
pub fn relaunch_elevated() -> Result<bool, UiHelperError> {
    Ok(false)
}

#[cfg(not(windows))]
pub fn focus_existing_instance() -> Result<(), UiHelperError> {
    Ok(())
}

#[cfg(not(windows))]
pub fn set_per_monitor_dpi_awareness() -> Result<(), UiHelperError> {
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn single_instance_name_is_stable() {
        assert_eq!(UI_SINGLE_INSTANCE_MUTEX, r"Local\KenaliUiSingleInstance");
    }

    #[test]
    fn error_display_is_generic() {
        for error in [
            UiHelperError::AlreadyRunning,
            UiHelperError::SingleInstance,
            UiHelperError::ElevationFailed,
            UiHelperError::Executable,
            UiHelperError::Focus,
            UiHelperError::Dpi,
        ] {
            let text = error.to_string();
            assert!(!text.contains("Administrator"));
            assert!(!text.contains("S-1-"));
            assert!(!text.contains("C:\\"));
        }
    }
}
