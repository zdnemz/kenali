//! The GUI renders all status and guidance wording from the shared table in
//! `kenali_core`. This module re-exports those constants and adds only the
//! non-status chrome labels (navigation, buttons, generic notices). No numeric
//! comparison values, no internal reason codes and no duplicated tile literals
//! live here.

use kenali_core::types::{Phase, Pose};

// Status/tile wording is re-used verbatim; never re-typed in this crate.
pub use kenali_core::strings::{
    BLINK_TWICE, FACE_UNLOCK_PAUSED, FACE_UNLOCK_READY, FACE_UNLOCK_UNAVAILABLE,
    LOOKING_FOR_YOU, LOOK_AT_THE_SCREEN, MOVE_A_LITTLE_CLOSER, TOO_DARK,
    TURN_YOUR_HEAD_LEFT, USE_YOUR_PASSWORD_OR_PIN, WELCOME,
};

/// Tile wording for a phase, delegated to the single shared mapping.
pub fn tile(phase: Phase) -> &'static str {
    kenali_core::tiles::tile_text(phase)
}

/// Generic denied wording, delegated to the shared table.
pub fn failed(tries_left: u8) -> String {
    kenali_core::strings::not_recognised(tries_left)
}

/// Guidance for a guided-capture pose, re-using only shared table constants.
pub fn pose_guidance(pose: Pose) -> &'static str {
    match pose {
        Pose::Straight | Pose::Right => LOOK_AT_THE_SCREEN,
        Pose::Left => TURN_YOUR_HEAD_LEFT,
        Pose::Up | Pose::Down => MOVE_A_LITTLE_CLOSER,
        _ => LOOK_AT_THE_SCREEN,
    }
}

// ---- Navigation and headers ----

pub const APP_TITLE: &str = "Kenali";
pub const NAV_DASHBOARD: &str = "Dashboard";
pub const NAV_ENROL: &str = "Enrol";
pub const NAV_TEMPLATES: &str = "Templates";
pub const NAV_SETTINGS: &str = "Settings";
pub const NAV_SECURITY: &str = "Security";
pub const NAV_DIAGNOSTICS: &str = "Diagnostics";
pub const NAV_TEST_BENCH: &str = "Test / Bench";

pub const HDR_DASHBOARD: &str = "Dashboard";
pub const HDR_ENROL: &str = "Enrol wizard";
pub const HDR_TEMPLATES: &str = "Stored templates";
pub const HDR_SETTINGS: &str = "Settings";
pub const HDR_SECURITY: &str = "Security";
pub const HDR_DIAGNOSTICS: &str = "Diagnostics";
pub const HDR_TEST_BENCH: &str = "Test / Bench";

// ---- Buttons ----

pub const BTN_START: &str = "Start";
pub const BTN_CONTINUE: &str = "Continue";
pub const BTN_CANCEL: &str = "Cancel";
pub const BTN_SUBMIT_SAMPLE: &str = "Capture sample";
pub const BTN_FINISH: &str = "Finish enrolment";
pub const BTN_DONE: &str = "Done";
pub const BTN_DISABLE: &str = "Disable";
pub const BTN_ENABLE: &str = "Enable";
pub const BTN_REFRESH: &str = "Refresh credentials";
pub const BTN_ADD_TEMPLATE: &str = "Add template";
pub const BTN_DELETE: &str = "Delete";
pub const BTN_EXPORT: &str = "Export redacted ZIP";
pub const BTN_RUN_DRY_RUN: &str = "Run dry-run";
pub const BTN_RUN_CALIBRATION: &str = "Run calibration";
pub const BTN_SAVE: &str = "Save";
pub const BTN_THEME: &str = "Toggle light / dark";

// ---- Labels ----

pub const LBL_SERVICE: &str = "Service";
pub const LBL_CAMERA: &str = "Camera";
pub const LBL_ENROLMENT: &str = "Enrolment";
pub const LBL_KILL_SWITCH: &str = "Kill switch";
pub const LBL_KILL_SWITCH_HINT: &str = "Managed outside this app — status is read-only.";
pub const LBL_SID: &str = "Enrolled account";
pub const LBL_PREVIEW: &str = "Live preview (memory only)";
pub const LBL_PREREQUISITES: &str = "Prerequisites";
pub const LBL_PASSWORD: &str = "Password";
pub const LBL_SAMPLES: &str = "Samples";
pub const LBL_SAVE_PATH: &str = "Save location";
pub const LBL_ON: &str = "On";
pub const LBL_OFF: &str = "Off";
pub const LBL_UNKNOWN: &str = "Unknown";
pub const LBL_READ_ONLY: &str = "Read-only";
pub const LBL_PAD_HINT: &str = "Calibration thresholds are read-only.";
pub const LBL_NEVER_UNLOCKS: &str = "This dry-run never unlocks the session.";
pub const LBL_RECEIVING: &str = "Receiving export…";
pub const LBL_LIGHTING: &str = "Lighting";
pub const LBL_GLASSES: &str = "Glasses";
pub const LBL_POSE: &str = "Pose";
pub const LBL_CAPTURED: &str = "Captured";

// Wizard step captions.
pub const STEP_PREREQUISITES: &str = "Step 1 of 6 — Prerequisites";
pub const STEP_PASSWORD: &str = "Step 2 of 6 — Password";
pub const STEP_CAPTURE: &str = "Step 3 of 6 — Guided capture";
pub const STEP_SEAL: &str = "Step 4 of 6 — Sealing";
pub const STEP_SELF_TEST: &str = "Step 5 of 6 — Self-test";
pub const STEP_SUMMARY: &str = "Step 6 of 6 — Summary";

// ---- Generic notices (never contain a reason code or a comparison value) ----

pub const MSG_ERROR: &str = "Something went wrong. Please try again.";
pub const MSG_CONNECTED: &str = "Connected to the management service.";
pub const MSG_DISCONNECTED: &str = "The management service is unavailable.";
pub const MSG_PREVIEW_CLEARED: &str = "Preview cleared.";
pub const MSG_EXPORT_SAVED: &str = "Export saved.";
pub const MSG_EXPORT_FAILED: &str = "The export could not be saved.";
pub const MSG_DRY_RUN_DONE: &str = "Dry-run finished. The session was not unlocked.";
pub const MSG_REFRESH_DONE: &str = "Credentials refreshed.";
pub const MSG_DISABLED: &str = "Face unlock paused.";
pub const MSG_ENABLED: &str = "Face unlock ready.";
pub const MSG_CONFIRM_DELETE: &str = "Delete this template?";
pub const MSG_NO_TEMPLATES: &str = "No templates stored yet.";
pub const MSG_NO_CONFIG: &str = "Settings are not available yet.";
pub const MSG_NO_STATUS: &str = "Service status is not available yet.";
