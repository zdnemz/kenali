//! Pure mapping from Win32 `CPUS_*` usage-scenario values to the shared
//! [`Scenario`] type, plus the Windows-only E_NOTIMPL gate.
//!
//! The mapping is plain integer-in / `Option<Scenario>`-out so it is
//! host-testable. The merged `kenali_core::scenarios::scenario_supported` is
//! the single source of truth for which scenarios are allowed (only Unlock).

use kenali_core::scenarios::scenario_supported;
use kenali_core::types::Scenario;

// Numeric values from `wincred.h`. Kept as local constants so the mapping
// stays platform-agnostic and host-testable. `windows` 0.62 has no CPUS_REMOTE
// constant; value 6 is reserved here and mapped to `Scenario::Remote`.
const CPUS_INVALID: i32 = 0;
const CPUS_LOGON: i32 = 1;
const CPUS_UNLOCK_WORKSTATION: i32 = 2;
const CPUS_CHANGE_PASSWORD: i32 = 3;
const CPUS_CREDUI: i32 = 4;
const CPUS_PLAP: i32 = 5;
const CPUS_REMOTE: i32 = 6;

/// Map a raw `CREDENTIAL_PROVIDER_USAGE_SCENARIO.0` to a [`Scenario`].
///
/// Unknown values map to `None` and are therefore declined — never silently
/// treated as supported.
pub fn map_usage_scenario(raw: i32) -> Option<Scenario> {
    match raw {
        CPUS_UNLOCK_WORKSTATION => Some(Scenario::Unlock),
        CPUS_LOGON => Some(Scenario::Logon),
        CPUS_CHANGE_PASSWORD => Some(Scenario::ChangePassword),
        CPUS_CREDUI => Some(Scenario::CredUi),
        CPUS_PLAP => Some(Scenario::Plap),
        CPUS_REMOTE => Some(Scenario::Remote),
        CPUS_INVALID => None,
        _ => None,
    }
}

/// Pure allow decision for a raw CPUS value. `true` only for Unlock.
pub fn scenario_allowed(raw: i32) -> bool {
    map_usage_scenario(raw).map_or(false, |s| scenario_supported(&s))
}

/// Decline any scenario the core policy does not support.
///
/// LogonUI unloads the provider when `SetUsageScenario` returns `E_NOTIMPL`,
/// which is exactly the fail-open-to-password behaviour we want for every
/// scenario other than workstation unlock.
#[cfg(windows)]
pub fn decline_unsupported(raw: i32) -> windows::core::Result<()> {
    if scenario_allowed(raw) {
        Ok(())
    } else {
        Err(windows::core::Error::from(
            windows::Win32::Foundation::E_NOTIMPL,
        ))
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn maps_all_known_scenarios() {
        assert_eq!(map_usage_scenario(CPUS_UNLOCK_WORKSTATION), Some(Scenario::Unlock));
        assert_eq!(map_usage_scenario(CPUS_LOGON), Some(Scenario::Logon));
        assert_eq!(map_usage_scenario(CPUS_CHANGE_PASSWORD), Some(Scenario::ChangePassword));
        assert_eq!(map_usage_scenario(CPUS_CREDUI), Some(Scenario::CredUi));
        assert_eq!(map_usage_scenario(CPUS_PLAP), Some(Scenario::Plap));
        assert_eq!(map_usage_scenario(CPUS_REMOTE), Some(Scenario::Remote));
    }

    #[test]
    fn unknown_and_invalid_are_declined() {
        assert_eq!(map_usage_scenario(CPUS_INVALID), None);
        assert_eq!(map_usage_scenario(i32::MAX), None);
        assert!(!scenario_allowed(CPUS_INVALID));
        assert!(!scenario_allowed(i32::MIN));
    }

    #[test]
    fn only_unlock_is_allowed() {
        assert!(scenario_allowed(CPUS_UNLOCK_WORKSTATION));
        assert!(!scenario_allowed(CPUS_LOGON));
        assert!(!scenario_allowed(CPUS_REMOTE));
        assert!(!scenario_allowed(CPUS_CHANGE_PASSWORD));
        assert!(!scenario_allowed(CPUS_CREDUI));
        assert!(!scenario_allowed(CPUS_PLAP));
    }
}
