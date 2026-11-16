//! Host tests for the CP decision helpers, using only the public API exactly
//! as the Windows `kenali-cp` crate will.

use kenali_core::scenarios::{is_allowed_sid, scenario_supported};
use kenali_core::strings::{
    FACE_UNLOCK_PAUSED, FACE_UNLOCK_UNAVAILABLE, USE_YOUR_PASSWORD_OR_PIN,
};
use kenali_core::tiles::tile_text;
use kenali_core::types::{Phase, Scenario, Sid};

#[test]
fn scenario_gate_allows_only_unlock() {
    let all = [
        Scenario::Unlock,
        Scenario::Logon,
        Scenario::Remote,
        Scenario::ChangePassword,
        Scenario::CredUi,
        Scenario::Plap,
    ];
    let supported: Vec<_> = all
        .iter()
        .filter(|s| scenario_supported(s))
        .cloned()
        .collect();
    assert_eq!(supported, vec![Scenario::Unlock]);
}

#[test]
fn sid_gate_never_allows_an_unenrolled_account() {
    let current = Sid::new("S-1-5-21-abc");
    assert!(!is_allowed_sid(&current, None));
    assert!(is_allowed_sid(&current, Some(Sid::new("S-1-5-21-abc"))));
    assert!(!is_allowed_sid(&current, Some(Sid::new("S-1-5-21-xyz"))));
}

#[test]
fn tiles_render_pinned_phases_from_the_existing_table() {
    assert_eq!(tile_text(Phase::ServiceDown), FACE_UNLOCK_UNAVAILABLE);
    assert_eq!(tile_text(Phase::Disabled), FACE_UNLOCK_PAUSED);
    assert_eq!(tile_text(Phase::Fallback), USE_YOUR_PASSWORD_OR_PIN);

    for phase in [
        Phase::Armed,
        Phase::Searching,
        Phase::Guidance,
        Phase::Challenge,
        Phase::Verified,
        Phase::Denied,
        Phase::Fallback,
        Phase::Disabled,
        Phase::ServiceDown,
    ] {
        let text = tile_text(phase);
        assert!(text.chars().all(|c| !c.is_ascii_digit()), "digits in {text}");
    }
}
