//! Cross-crate-facing smoke tests: everything here uses only the public API,
//! exactly as `kenali-ipc`, `kenali-store`, `kenali-vision` and the service do.

use std::time::{Duration, Instant};

use kenali_core::config::{ConfigKey, ConfigReport, ConfigValue, validate_config};
use kenali_core::policy::{NO_RETRY_NTSTATUS, PolicyDecision, PolicyEngine, PolicyInput};
use kenali_core::score::cosine;
use kenali_core::secret::{Embedding, Password};
use kenali_core::state::{AttemptEvent, AttemptState, AttemptStateMachine, MAX_FACE_ATTEMPTS_PER_LOCK};
use kenali_core::types::{DenyReason, Scenario, Sid};

#[test]
fn secrets_are_redacted_outside_the_crate() {
    let password = Password::from_wide("correct horse".encode_utf16().collect());
    let debug = format!("{password:?}");
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("correct horse"));

    let embedding = Embedding::new(vec![1.0, 0.0]);
    assert!(format!("{embedding:?}").contains("<redacted>"));

    // Wire conversion helpers require no IPC type.
    assert_eq!(password.as_wide().len(), 13);
    let rebuilt = Password::from_wide(password.into_wide_units());
    assert_eq!(rebuilt.len(), 13);
}

#[test]
fn cosine_is_public_and_deterministic() {
    let a = Embedding::new(vec![1.0, 2.0, 3.0]);
    let b = Embedding::new(vec![1.0, 2.0, 3.0]);
    assert!((cosine(&a, &b) - 1.0).abs() < 1e-6);
}

#[test]
fn state_machine_rejects_second_submit() {
    let now = Instant::now();
    let mut machine = AttemptStateMachine::new();
    let _ = machine.handle(AttemptEvent::Start, now);
    let _ = machine.handle(AttemptEvent::SubmitFailure(DenyReason::NoFace), now);
    assert_eq!(machine.state(), AttemptState::Denied);
    assert!(!machine.can_submit());
    let rejected = format!("{:?}", machine.handle(AttemptEvent::SubmitSuccess, now));
    assert!(rejected.contains("AlreadySubmitted"), "{rejected}");
    assert_eq!(MAX_FACE_ATTEMPTS_PER_LOCK, 3);
}

#[test]
fn policy_public_surface_covers_the_default_table() {
    let now = Instant::now();
    let mut engine = PolicyEngine::with_defaults();
    engine.note_knowledge_unlock(now);
    let sid = Sid::new("S-1-5-21-test");
    let input = PolicyInput {
        scenario: Scenario::Unlock,
        attempted_sid: &sid,
        enrolled_sid: Some(&sid),
        is_remote: false,
        live_session: true,
        camera_pin_matches: true,
        second_face_comparable: false,
        elevated_risk: false,
    };
    assert_eq!(engine.decide(&input, now), PolicyDecision::AllowFace);
    engine.record_ntstatus(NO_RETRY_NTSTATUS[1]);
    assert!(matches!(
        engine.decide(&input, now + Duration::from_secs(1)),
        PolicyDecision::Disabled { .. }
    ));
}

#[test]
fn config_public_surface_tightens_only() {
    let base = ConfigReport::default_policy();
    let tightened = validate_config(
        &base,
        &[(ConfigKey::YawMaxDegrees, ConfigValue::F32(15.0))],
    )
    .expect("tightening accepted");
    assert_eq!(tightened.get(ConfigKey::YawMaxDegrees), Some(ConfigValue::F32(15.0)));
    assert!(validate_config(
        &base,
        &[(ConfigKey::ScopeLogon, ConfigValue::Bool(true))]
    )
    .is_err());
}
