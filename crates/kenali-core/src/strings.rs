//! The single English string table (PRD §11.1). Generic wording only — no
//! scores, no PAD margins, no usernames, no internal reason codes. Localisation
//! is a later fast-follow; everything renders from these constants.

pub const FACE_UNLOCK_READY: &str = "Face unlock ready";
pub const LOOKING_FOR_YOU: &str = "Looking for you…";
pub const LOOK_AT_THE_SCREEN: &str = "Look at the screen";
pub const MOVE_A_LITTLE_CLOSER: &str = "Move a little closer";
pub const TOO_DARK: &str = "Too dark";
pub const BLINK_TWICE: &str = "Blink twice";
pub const TURN_YOUR_HEAD_LEFT: &str = "Turn your head left";
pub const WELCOME: &str = "Welcome";
pub const USE_YOUR_PASSWORD_OR_PIN: &str = "Use your password or PIN";
pub const FACE_UNLOCK_PAUSED: &str = "Face unlock paused";
pub const FACE_UNLOCK_UNAVAILABLE: &str = "Face unlock unavailable";

/// Format template for the failed tile. `{}` is the number of tries left.
pub const NOT_RECOGNISED_TRIES_LEFT: &str = "Not recognised — {} tries left";

/// Render the failed tile with a try count. No score is included.
pub fn not_recognised(tries_left: u8) -> String {
    format!("Not recognised — {tries_left} tries left")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_exact_prd_wording() {
        assert_eq!(FACE_UNLOCK_READY, "Face unlock ready");
        assert_eq!(LOOKING_FOR_YOU, "Looking for you…");
        assert_eq!(LOOK_AT_THE_SCREEN, "Look at the screen");
        assert_eq!(MOVE_A_LITTLE_CLOSER, "Move a little closer");
        assert_eq!(TOO_DARK, "Too dark");
        assert_eq!(BLINK_TWICE, "Blink twice");
        assert_eq!(TURN_YOUR_HEAD_LEFT, "Turn your head left");
        assert_eq!(WELCOME, "Welcome");
        assert_eq!(USE_YOUR_PASSWORD_OR_PIN, "Use your password or PIN");
        assert_eq!(FACE_UNLOCK_PAUSED, "Face unlock paused");
        assert_eq!(FACE_UNLOCK_UNAVAILABLE, "Face unlock unavailable");
    }

    #[test]
    fn failed_tile_renders_count_without_scores() {
        let text = not_recognised(2);
        assert_eq!(text, "Not recognised — 2 tries left");
        assert!(!text.contains('.'));
        assert_eq!(NOT_RECOGNISED_TRIES_LEFT.replace("{}", "1"), "Not recognised — 1 tries left");
    }
}
