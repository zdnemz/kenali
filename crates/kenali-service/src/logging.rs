//! Event-Log wrapper with a single redaction gate.
//!
//! Every message passes through [`safe_message`] before it reaches
//! `kenali_winapi::eventlog`. Scores are quantised to 0.05 steps by
//! [`quantize_score`] for any numeric telemetry; secrets, biometrics, SIDs,
//! paths and sub-0.05 decimals are stripped. The Event Log source and event
//! ids are the ones pinned in `kenali-winapi`.

use kenali_winapi::eventlog::{
    log_error as win_log_error, log_info as win_log_info, log_warning as win_log_warning,
    EVENT_DISABLED, EVENT_PAD_BURST, EVENT_SERVICE_START,
};

/// Round a score to the nearest 0.05. Non-finite values collapse to 0.0 so a
/// NaN/Infinity can never leak a precise measurement into a log.
pub fn quantize_score(score: f32) -> f32 {
    if !score.is_finite() {
        return 0.0;
    }
    let steps = (score * 20.0).round();
    (steps / 20.0 * 100.0).round() / 100.0
}

/// Apply the shared redactor to a log message.
pub fn safe_message(message: &str) -> String {
    crate::diagnostics::redact(message)
}

pub fn log_info(event_id: u16, message: &str) {
    let safe = safe_message(message);
    let _ = win_log_info(event_id, &safe);
}

pub fn log_warning(event_id: u16, message: &str) {
    let safe = safe_message(message);
    let _ = win_log_warning(event_id, &safe);
}

pub fn log_error(event_id: u16, message: &str) {
    let safe = safe_message(message);
    let _ = win_log_error(event_id, &safe);
}

/// Pinned lifecycle events. All messages are fixed generic text — no scores.
pub fn log_service_start() {
    log_info(EVENT_SERVICE_START, "Kenali service started");
}

pub fn log_pad_burst() {
    log_warning(EVENT_PAD_BURST, "presentation attack rejections recorded");
}

pub fn log_disabled() {
    log_warning(EVENT_DISABLED, "face unlock disabled by operator");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_is_quantised_to_0_05() {
        assert!((quantize_score(0.363) - 0.35).abs() < 1e-6);
        assert!((quantize_score(0.42) - 0.40).abs() < 1e-6);
        assert!((quantize_score(-1.0) + 1.0).abs() < 1e-6);
        assert_eq!(quantize_score(f32::NAN), 0.0);
    }

    #[test]
    fn safe_message_redacts_sensitive_tokens() {
        let safe = safe_message("sid S-1-1 path C:\\secret score 0.363 ok");
        assert!(!safe.contains("S-1-1"));
        assert!(!safe.contains("C:\\secret"));
        assert!(!safe.contains("0.363"));
    }

    #[test]
    fn lifecycle_wrappers_are_total_on_host() {
        // Host stubs return Ok; the wrappers must not panic on either platform.
        log_service_start();
        log_pad_burst();
        log_disabled();
    }
}
