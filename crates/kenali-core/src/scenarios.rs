//! Pure Credential-Provider decision helpers, host-testable on any target.
//!
//! No OS bindings, no I/O, no networking and no logging live here. These
//! functions are shared with the Windows CP crate so its security-relevant
//! decisions can be proven on the host. There is deliberately no
//! `ICredentialProviderFilter`: password and PIN providers are never hidden.

use crate::types::{Scenario, Sid};

/// True only for the CPUS_UNLOCK_WORKSTATION scenario.
///
/// Every other `CPUS_*` scenario is declined by the CP (`E_NOTIMPL`). All
/// known variants are listed explicitly with no wildcard arm, so adding a
/// future variant is a compile error here rather than a silent policy change.
pub fn scenario_supported(s: &Scenario) -> bool {
    match s {
        Scenario::Unlock => true,
        Scenario::Logon
        | Scenario::Remote
        | Scenario::ChangePassword
        | Scenario::CredUi
        | Scenario::Plap => false,
    }
}

/// Whether the current session's SID matches the enrolled account.
///
/// Returns `false` when no account is enrolled — the face tile stays hidden
/// by default. A SID is an account identifier, not a credential, but this
/// helper never logs, formats or otherwise surfaces either value: the sole
/// observable result is the boolean, with no error path carrying the SID.
pub fn is_allowed_sid(current: &Sid, enrolled: Option<Sid>) -> bool {
    match enrolled {
        Some(enrolled) => &enrolled == current,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unlock_is_supported() {
        assert!(scenario_supported(&Scenario::Unlock));
        assert!(!scenario_supported(&Scenario::Logon));
        assert!(!scenario_supported(&Scenario::Remote));
        assert!(!scenario_supported(&Scenario::ChangePassword));
        assert!(!scenario_supported(&Scenario::CredUi));
        assert!(!scenario_supported(&Scenario::Plap));
    }

    #[test]
    fn sid_guard_is_false_when_not_enrolled() {
        let current = Sid::new("S-1-5-21-current");
        assert!(!is_allowed_sid(&current, None));
    }

    #[test]
    fn sid_guard_matches_equal_and_rejects_different() {
        let current = Sid::new("S-1-5-21-current");
        let other = Sid::new("S-1-5-21-other");
        assert!(is_allowed_sid(&current, Some(current.clone())));
        assert!(!is_allowed_sid(&current, Some(other)));
    }
}
