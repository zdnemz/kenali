//! Thin, generic Event Log writer over `RegisterEventSourceW`/`ReportEventW`.
//!
//! Messages passed here must be generic fixed text: no scores, biometrics,
//! secrets, SIDs or user names are ever placed in the event string. The
//! source name is the SCM name `KenaliSvc` so the service's events appear
//! under a single, predictable source.

use thiserror::Error;

/// Event Log source name; identical to the SCM service name.
pub const EVENT_SOURCE: &str = "KenaliSvc";
/// Service started.
pub const EVENT_SERVICE_START: u16 = 1000;
/// PAD/burst warning (no score or frame data in the text).
pub const EVENT_PAD_BURST: u16 = 1001;
/// Face unlock disabled via the kill switch.
pub const EVENT_DISABLED: u16 = 1002;

/// Errors from the Event Log writer. Wording is intentionally generic.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum EventLogError {
    #[error("event log source is unavailable")]
    Unavailable,
    #[error("event log write failed")]
    WriteFailed,
}

/// Write one event through a short-lived `RegisterEventSourceW` handle.
#[cfg(windows)]
fn log(event_id: u16, message: &str, kind: windows::Win32::System::EventLog::REPORT_EVENT_TYPE) -> Result<(), EventLogError> {
    use windows::core::PCWSTR;
    use windows::Win32::System::EventLog::{
        DeregisterEventSource, RegisterEventSourceW, ReportEventW,
    };

    let source: Vec<u16> = EVENT_SOURCE.encode_utf16().chain(core::iter::once(0)).collect();
    let text: Vec<u16> = message.encode_utf16().chain(core::iter::once(0)).collect();

    // SAFETY: `source` is a local, NUL-terminated Vec alive for the call; the
    // local-server argument is None (this machine).
    let handle = unsafe { RegisterEventSourceW(None, PCWSTR(source.as_ptr())) }
        .map_err(|_| EventLogError::Unavailable)?;

    let strings = [PCWSTR(text.as_ptr())];
    // SAFETY: `handle` is valid, `strings` is a live slice of one NUL-terminated
    // wide string and all other arguments are plain values (no raw data).
    let result = unsafe { ReportEventW(handle, kind, 0, event_id as u32, None, 0, Some(&strings), None) };
    // SAFETY: `handle` is owned here, opened above and not yet closed.
    let _ = unsafe { DeregisterEventSource(handle) };

    result.map_err(|_| EventLogError::WriteFailed)
}

#[cfg(windows)]
pub fn log_info(event_id: u16, message: &str) -> Result<(), EventLogError> {
    log(event_id, message, windows::Win32::System::EventLog::EVENTLOG_INFORMATION_TYPE)
}

#[cfg(windows)]
pub fn log_warning(event_id: u16, message: &str) -> Result<(), EventLogError> {
    log(event_id, message, windows::Win32::System::EventLog::EVENTLOG_WARNING_TYPE)
}

#[cfg(windows)]
pub fn log_error(event_id: u16, message: &str) -> Result<(), EventLogError> {
    log(event_id, message, windows::Win32::System::EventLog::EVENTLOG_ERROR_TYPE)
}

// Host stubs: no Event Log off Windows. Kept so callers compile on every host.
#[cfg(not(windows))]
pub fn log_info(_event_id: u16, _message: &str) -> Result<(), EventLogError> {
    Ok(())
}

#[cfg(not(windows))]
pub fn log_warning(_event_id: u16, _message: &str) -> Result<(), EventLogError> {
    Ok(())
}

#[cfg(not(windows))]
pub fn log_error(_event_id: u16, _message: &str) -> Result<(), EventLogError> {
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn event_source_and_ids_are_stable() {
        assert_eq!(EVENT_SOURCE, "KenaliSvc");
        assert_eq!(EVENT_SERVICE_START, 1000);
        assert_eq!(EVENT_PAD_BURST, 1001);
        assert_eq!(EVENT_DISABLED, 1002);
    }

    #[test]
    fn error_display_is_generic() {
        for error in [EventLogError::Unavailable, EventLogError::WriteFailed] {
            let text = error.to_string();
            assert!(!text.contains("secret"));
            assert!(!text.contains("score"));
        }
    }
}
