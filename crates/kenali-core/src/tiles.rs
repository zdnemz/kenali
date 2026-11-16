//! Pure `Phase` → string-table mapping for the CP status tile.
//!
//! Rendering uses only the existing, score-free constants from
//! [`crate::strings`]. No similarity value, PAD margin or internal reason code
//! is ever shown. All known variants are listed explicitly with no wildcard
//! arm, so adding a future `Phase` is a compile error here rather than a
//! surprise tile.

use crate::types::Phase;

/// Generic, score-free tile wording for the given phase.
pub fn tile_text(p: Phase) -> &'static str {
    use crate::strings::{
        BLINK_TWICE, FACE_UNLOCK_PAUSED, FACE_UNLOCK_READY, FACE_UNLOCK_UNAVAILABLE,
        LOOKING_FOR_YOU, LOOK_AT_THE_SCREEN, USE_YOUR_PASSWORD_OR_PIN, WELCOME,
    };

    match p {
        Phase::Armed => FACE_UNLOCK_READY,
        Phase::Searching => LOOKING_FOR_YOU,
        Phase::Guidance => LOOK_AT_THE_SCREEN,
        Phase::Challenge => BLINK_TWICE,
        Phase::Verified => WELCOME,
        Phase::Denied => USE_YOUR_PASSWORD_OR_PIN,
        Phase::Fallback => USE_YOUR_PASSWORD_OR_PIN,
        Phase::Disabled => FACE_UNLOCK_PAUSED,
        Phase::ServiceDown => FACE_UNLOCK_UNAVAILABLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::{
        BLINK_TWICE, FACE_UNLOCK_PAUSED, FACE_UNLOCK_READY, FACE_UNLOCK_UNAVAILABLE,
        LOOKING_FOR_YOU, LOOK_AT_THE_SCREEN, MOVE_A_LITTLE_CLOSER, TOO_DARK,
        TURN_YOUR_HEAD_LEFT, USE_YOUR_PASSWORD_OR_PIN, WELCOME,
    };

    const TABLE: &[&str] = &[
        FACE_UNLOCK_READY,
        LOOKING_FOR_YOU,
        LOOK_AT_THE_SCREEN,
        MOVE_A_LITTLE_CLOSER,
        TOO_DARK,
        BLINK_TWICE,
        TURN_YOUR_HEAD_LEFT,
        WELCOME,
        USE_YOUR_PASSWORD_OR_PIN,
        FACE_UNLOCK_PAUSED,
        FACE_UNLOCK_UNAVAILABLE,
    ];

    #[test]
    fn pinned_phases_map_exactly() {
        assert_eq!(tile_text(Phase::ServiceDown), FACE_UNLOCK_UNAVAILABLE);
        assert_eq!(tile_text(Phase::Disabled), FACE_UNLOCK_PAUSED);
        assert_eq!(tile_text(Phase::Fallback), USE_YOUR_PASSWORD_OR_PIN);
    }

    #[test]
    fn every_phase_uses_a_known_constant_without_scores() {
        let all = [
            Phase::Armed,
            Phase::Searching,
            Phase::Guidance,
            Phase::Challenge,
            Phase::Verified,
            Phase::Denied,
            Phase::Fallback,
            Phase::Disabled,
            Phase::ServiceDown,
        ];
        for phase in all {
            let text = tile_text(phase);
            assert!(TABLE.contains(&text), "unknown wording for {phase:?}: {text}");
            assert!(!text.chars().any(|c| c.is_ascii_digit()), "digit in tile: {text}");
            let lower = text.to_ascii_lowercase();
            assert!(!lower.contains("score"), "score-like tile: {text}");
            assert!(!lower.contains("similar"), "score-like tile: {text}");
            assert!(!lower.contains("confidence"), "score-like tile: {text}");
        }
    }
}
