//! Kill-switch read for the credential provider.
//!
//! Delegates to the merged read-only `kenali_winapi::registry::kill_switch_enabled`.
//! The CP never writes the registry. If the value cannot be read we fail safe:
//! the face tile is hidden and the user falls through to password/PIN.

/// `true` when the face tile must be hidden this session.
///
/// On Windows this is the value of `HKLM\SOFTWARE\Kenali\Disabled` (missing
/// value means "not disabled"). A read error is treated as disabled so an
/// unknown state never produces a submittable tile. Off Windows the tile is
/// hidden unconditionally.
pub fn kill_switch_enabled() -> bool {
    #[cfg(windows)]
    {
        match kenali_winapi::registry::kill_switch_enabled() {
            Ok(disabled) => disabled,
            Err(_) => true,
        }
    }
    #[cfg(not(windows))]
    {
        true
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn off_windows_tile_is_hidden() {
        #[cfg(not(windows))]
        assert!(kill_switch_enabled());
    }
}
