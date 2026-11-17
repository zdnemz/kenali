//! Allowed-SID guard for the spike tile.
//!
//! The tile is hidden by default: the enrolled SID comes from the compile-time
//! `KENALI_SPIKE_ALLOWED_SID` environment variable, which is normally unset.
//! The decision is delegated to `kenali_core::scenarios::is_allowed_sid`, so
//! `None` enrolled means "never show the tile".

use kenali_core::types::Sid;

/// Compile-time enrolled SID for the spike. `None` when not set, which hides
/// the tile. A SID is an account identifier (not a secret), but it is never
/// logged or formatted here — only compared.
pub fn enrolled_sid() -> Option<Sid> {
    option_env!("KENALI_SPIKE_ALLOWED_SID")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(Sid::new)
}

/// Core allow decision for an optional current SID.
pub fn is_allowed_sid(current: Option<Sid>) -> bool {
    match current {
        Some(current) => kenali_core::scenarios::is_allowed_sid(&current, enrolled_sid()),
        None => false,
    }
}

/// SID of the active console session, wrapped from the merged Win32 helper.
#[cfg(windows)]
pub fn current_sid() -> Option<Sid> {
    match kenali_winapi::service::current_session_sid() {
        Ok(Some(sid)) => Some(Sid::new(sid)),
        _ => None,
    }
}

/// Whether the current console user is the enrolled user.
pub fn current_session_is_allowed() -> bool {
    #[cfg(windows)]
    {
        is_allowed_sid(current_sid())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_current_sid_never_allows() {
        assert!(!is_allowed_sid(None));
    }

    #[test]
    fn hidden_by_default_when_not_enrolled() {
        // KENALI_SPIKE_ALLOWED_SID is not set for the test build.
        assert!(enrolled_sid().is_none());
        let current = Sid::new("S-1-5-21-current");
        assert!(!is_allowed_sid(Some(current)));
    }

    #[test]
    fn equal_sid_allows_when_enrolled() {
        let current = Sid::new("S-1-5-21-abc");
        assert!(kenali_core::scenarios::is_allowed_sid(
            &current,
            Some(current.clone())
        ));
        let other = Sid::new("S-1-5-21-xyz");
        assert!(!kenali_core::scenarios::is_allowed_sid(&current, Some(other)));
    }
}
