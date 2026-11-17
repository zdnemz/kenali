//! Configuration schema and validation.
//!
//! The UI can only *tighten* the default policy. `validate_config` is the one
//! place that decides what "tightening" means for each key; it is deliberately
//! a pure function of the current report plus the proposed changes so it can
//! be property-tested and called from the service without side effects.

use serde::{Deserialize, Serialize};

/// Every setting the UI may read or submit. PAD thresholds are read-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConfigKey {
    /// `CPUS_UNLOCK_WORKSTATION` enabled. Default on; may be turned off.
    ScopeUnlock,
    /// `CPUS_LOGON` enabled. Default off; the UI may not turn it on.
    ScopeLogon,
    /// Maximum horizontal head angle in degrees. Default/schema max 20.
    YawMaxDegrees,
    /// Maximum vertical head angle in degrees. Default/schema max 20.
    PitchMaxDegrees,
    /// Face-failure cooldown in seconds. Default 600; may only grow.
    FaceFailureCooldownSecs,
    /// PAD-rejection cooldown in seconds. Default 1800; may only grow.
    PadFailureCooldownSecs,
    /// Sliding window for face failures, seconds. Default 600; may only grow.
    FailureWindowSecs,
    /// Failures in the window that trigger a cooldown. Default 5; may shrink.
    MaxFailuresPerWindow,
    /// PAD rejections that trigger a cooldown. Default 3; may shrink.
    MaxPadRejections,
    /// Passive PAD enabled. Read-only in the UI.
    PadEnabled,
    /// Calibrated PAD threshold. Read-only in the UI.
    PadThreshold,
    /// Reject a comparable-size second face. Default on; may not be turned off.
    MultiFaceRejection,
}

/// A typed setting value. `f32` is only ever a finite, bounded number.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConfigValue {
    Bool(bool),
    U8(u8),
    U32(u32),
    F32(f32),
}

/// The full effective configuration returned by `GetConfig`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigReport {
    pub entries: Vec<(ConfigKey, ConfigValue)>,
}

/// Secure-by-default policy table (PRD §10 / FR-05 / FR-07 / SEC-17).
pub const DEFAULT_POLICY: &[(ConfigKey, ConfigValue)] = &[
    (ConfigKey::ScopeUnlock, ConfigValue::Bool(true)),
    (ConfigKey::ScopeLogon, ConfigValue::Bool(false)),
    (ConfigKey::YawMaxDegrees, ConfigValue::F32(20.0)),
    (ConfigKey::PitchMaxDegrees, ConfigValue::F32(20.0)),
    (ConfigKey::FaceFailureCooldownSecs, ConfigValue::U32(600)),
    (ConfigKey::PadFailureCooldownSecs, ConfigValue::U32(1800)),
    (ConfigKey::FailureWindowSecs, ConfigValue::U32(600)),
    (ConfigKey::MaxFailuresPerWindow, ConfigValue::U8(5)),
    (ConfigKey::MaxPadRejections, ConfigValue::U8(3)),
    (ConfigKey::PadEnabled, ConfigValue::Bool(true)),
    (ConfigKey::PadThreshold, ConfigValue::F32(0.5)),
    (ConfigKey::MultiFaceRejection, ConfigValue::Bool(true)),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ConfigError {
    #[error("unknown configuration key")]
    UnknownKey,
    #[error("value type does not match key")]
    TypeMismatch,
    #[error("value outside schema range")]
    OutOfRange,
    #[error("configuration key is read-only")]
    ReadOnly,
    #[error("change would loosen the security policy")]
    WouldLoosen,
}

impl ConfigReport {
    /// A fresh report equal to [`DEFAULT_POLICY`].
    pub fn default_policy() -> Self {
        Self {
            entries: DEFAULT_POLICY.to_vec(),
        }
    }

    /// Look up one key.
    pub fn get(&self, key: ConfigKey) -> Option<ConfigValue> {
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
    }

    /// Replace one key in place, returning whether it was present.
    pub fn set(&mut self, key: ConfigKey, value: ConfigValue) -> bool {
        if let Some((_, slot)) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            *slot = value;
            true
        } else {
            false
        }
    }
}

/// The single tightening rule. `true` means `new` is at least as strict as
/// `old`. Kept public so tests and future callers agree on one definition.
pub fn is_tighter_or_equal(key: ConfigKey, old: ConfigValue, new: ConfigValue) -> bool {
    match (key, old, new) {
        (ConfigKey::ScopeUnlock, ConfigValue::Bool(o), ConfigValue::Bool(n)) => !n || o,
        (ConfigKey::ScopeLogon, ConfigValue::Bool(_), ConfigValue::Bool(n)) => !n,
        (ConfigKey::YawMaxDegrees, ConfigValue::F32(o), ConfigValue::F32(n))
        | (ConfigKey::PitchMaxDegrees, ConfigValue::F32(o), ConfigValue::F32(n)) => n <= o,
        (ConfigKey::FaceFailureCooldownSecs, ConfigValue::U32(o), ConfigValue::U32(n))
        | (ConfigKey::PadFailureCooldownSecs, ConfigValue::U32(o), ConfigValue::U32(n))
        | (ConfigKey::FailureWindowSecs, ConfigValue::U32(o), ConfigValue::U32(n)) => n >= o,
        (ConfigKey::MaxFailuresPerWindow, ConfigValue::U8(o), ConfigValue::U8(n))
        | (ConfigKey::MaxPadRejections, ConfigValue::U8(o), ConfigValue::U8(n)) => n >= 1 && n <= o,
        (ConfigKey::PadEnabled, ConfigValue::Bool(o), ConfigValue::Bool(n)) => n == o,
        (ConfigKey::PadThreshold, ConfigValue::F32(o), ConfigValue::F32(n)) => n == o,
        (ConfigKey::MultiFaceRejection, ConfigValue::Bool(_), ConfigValue::Bool(n)) => n,
        _ => false,
    }
}

fn is_read_only(key: ConfigKey) -> bool {
    matches!(key, ConfigKey::PadEnabled | ConfigKey::PadThreshold)
}

fn same_shape(a: ConfigValue, b: ConfigValue) -> bool {
    matches!(
        (a, b),
        (ConfigValue::Bool(_), ConfigValue::Bool(_))
            | (ConfigValue::U8(_), ConfigValue::U8(_))
            | (ConfigValue::U32(_), ConfigValue::U32(_))
            | (ConfigValue::F32(_), ConfigValue::F32(_))
    )
}

fn within_schema(key: ConfigKey, value: ConfigValue) -> bool {
    match (key, value) {
        (ConfigKey::YawMaxDegrees, ConfigValue::F32(n))
        | (ConfigKey::PitchMaxDegrees, ConfigValue::F32(n)) => n.is_finite() && (0.0..=20.0).contains(&n),
        (ConfigKey::FaceFailureCooldownSecs, ConfigValue::U32(n))
        | (ConfigKey::PadFailureCooldownSecs, ConfigValue::U32(n))
        | (ConfigKey::FailureWindowSecs, ConfigValue::U32(n)) => (60..=86_400).contains(&n),
        (ConfigKey::MaxFailuresPerWindow, ConfigValue::U8(n))
        | (ConfigKey::MaxPadRejections, ConfigValue::U8(n)) => n >= 1,
        (ConfigKey::PadThreshold, ConfigValue::F32(n)) => n.is_finite() && (0.0..=1.0).contains(&n),
        (ConfigKey::ScopeUnlock, ConfigValue::Bool(_))
        | (ConfigKey::ScopeLogon, ConfigValue::Bool(_))
        | (ConfigKey::PadEnabled, ConfigValue::Bool(_))
        | (ConfigKey::MultiFaceRejection, ConfigValue::Bool(_)) => true,
        _ => false,
    }
}

/// Validate and apply a batch of proposed changes.
///
/// Returns a new report; the caller's `current` is never mutated. Any single
/// invalid or loosening change rejects the whole batch. Each change is checked
/// against the state produced by earlier changes in the batch, so a repeated
/// key may tighten a value further but can never loosen it. This matches the
/// PRD rule that the UI can only tighten the default policy.
pub fn validate_config(
    current: &ConfigReport,
    changes: &[(ConfigKey, ConfigValue)],
) -> Result<ConfigReport, ConfigError> {
    let mut next = current.clone();
    for &(key, value) in changes {
        let default = DEFAULT_POLICY
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .ok_or(ConfigError::UnknownKey)?;
        // Compare against the accumulated state, not the original `current`:
        // a later duplicate key must not be able to undo an earlier tightening.
        let old = next.get(key).unwrap_or(default);
        if !same_shape(old, value) {
            return Err(ConfigError::TypeMismatch);
        }
        if is_read_only(key) && value != old {
            return Err(ConfigError::ReadOnly);
        }
        if !within_schema(key, value) {
            return Err(ConfigError::OutOfRange);
        }
        if !is_tighter_or_equal(key, old, value) {
            return Err(ConfigError::WouldLoosen);
        }
        if !next.set(key, value) {
            return Err(ConfigError::UnknownKey);
        }
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn any_key() -> impl Strategy<Value = ConfigKey> {
        prop::sample::select(vec![
            ConfigKey::ScopeUnlock,
            ConfigKey::ScopeLogon,
            ConfigKey::YawMaxDegrees,
            ConfigKey::PitchMaxDegrees,
            ConfigKey::FaceFailureCooldownSecs,
            ConfigKey::PadFailureCooldownSecs,
            ConfigKey::FailureWindowSecs,
            ConfigKey::MaxFailuresPerWindow,
            ConfigKey::MaxPadRejections,
            ConfigKey::PadEnabled,
            ConfigKey::PadThreshold,
            ConfigKey::MultiFaceRejection,
        ])
    }

    fn any_value() -> impl Strategy<Value = ConfigValue> {
        prop_oneof![
            any::<bool>().prop_map(ConfigValue::Bool),
            any::<u8>().prop_map(ConfigValue::U8),
            any::<u32>().prop_map(ConfigValue::U32),
            (-90.0f32..90.0f32).prop_map(ConfigValue::F32),
        ]
    }

    proptest! {
        #[test]
        fn accepted_changes_never_loosen_a_default(key in any_key(), value in any_value()) {
            let base = ConfigReport::default_policy();
            let old = base.get(key).expect("every default key is present");
            if let Ok(next) = validate_config(&base, &[(key, value)]) {
                let actual = next.get(key).expect("accepted key stays present");
                prop_assert!(is_tighter_or_equal(key, old, actual));
            }
        }

        #[test]
        fn batched_changes_stay_tightening(keys in prop::collection::vec(any_key(), 1..6),
                                            values in prop::collection::vec(any_value(), 1..6)) {
            let base = ConfigReport::default_policy();
            let changes: Vec<_> = keys.into_iter().zip(values).collect();
            if let Ok(next) = validate_config(&base, &changes) {
                // Every accepted change is validated against the state produced
                // by earlier changes in the batch, so a repeated key can only
                // tighten further. The final value must therefore be at least
                // as strict as the last (tightest) value proposed for that key,
                // and no accepted value may loosen the original default.
                for (key, proposed) in changes {
                    let old = base.get(key).expect("present");
                    let actual = next.get(key).expect("present");
                    prop_assert!(is_tighter_or_equal(key, old, actual));
                    prop_assert!(is_tighter_or_equal(key, proposed, actual));
                }
            }
        }
    }

    #[test]
    fn batch_rejects_duplicate_key_that_loosens_earlier_tightening() {
        let base = ConfigReport::default_policy();
        assert_eq!(
            validate_config(
                &base,
                &[
                    (ConfigKey::ScopeUnlock, ConfigValue::Bool(false)),
                    (ConfigKey::ScopeUnlock, ConfigValue::Bool(true)),
                ],
            ),
            Err(ConfigError::WouldLoosen)
        );
    }

    #[test]
    fn batch_accepts_duplicate_key_that_tightens_further() {
        let base = ConfigReport::default_policy();
        let next = validate_config(
            &base,
            &[
                (ConfigKey::YawMaxDegrees, ConfigValue::F32(15.0)),
                (ConfigKey::YawMaxDegrees, ConfigValue::F32(10.0)),
            ],
        )
        .expect("monotone tightening accepted");
        assert_eq!(
            next.get(ConfigKey::YawMaxDegrees),
            Some(ConfigValue::F32(10.0))
        );
    }

    #[test]
    fn rejects_widening_yaw_and_disabling_logon_guard() {
        let base = ConfigReport::default_policy();
        assert_eq!(
            validate_config(&base, &[(ConfigKey::YawMaxDegrees, ConfigValue::F32(30.0))]),
            Err(ConfigError::OutOfRange)
        );
        assert_eq!(
            validate_config(&base, &[(ConfigKey::ScopeLogon, ConfigValue::Bool(true))]),
            Err(ConfigError::WouldLoosen)
        );
    }

    #[test]
    fn rejects_narrowing_cooldown_and_read_only_pad() {
        let base = ConfigReport::default_policy();
        assert_eq!(
            validate_config(
                &base,
                &[(ConfigKey::FaceFailureCooldownSecs, ConfigValue::U32(300))]
            ),
            Err(ConfigError::WouldLoosen)
        );
        assert_eq!(
            validate_config(&base, &[(ConfigKey::PadEnabled, ConfigValue::Bool(false))]),
            Err(ConfigError::ReadOnly)
        );
    }

    #[test]
    fn accepts_genuine_tightening() {
        let base = ConfigReport::default_policy();
        let next = validate_config(
            &base,
            &[
                (ConfigKey::YawMaxDegrees, ConfigValue::F32(15.0)),
                (ConfigKey::MaxFailuresPerWindow, ConfigValue::U8(3)),
                (ConfigKey::FaceFailureCooldownSecs, ConfigValue::U32(900)),
                (ConfigKey::ScopeUnlock, ConfigValue::Bool(false)),
            ],
        )
        .expect("tightening accepted");
        assert_eq!(next.get(ConfigKey::YawMaxDegrees), Some(ConfigValue::F32(15.0)));
        assert_eq!(base.get(ConfigKey::YawMaxDegrees), Some(ConfigValue::F32(20.0)));
        assert_eq!(base.entries.len(), DEFAULT_POLICY.len());
    }
}
