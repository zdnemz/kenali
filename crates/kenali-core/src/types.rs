//! Platform-agnostic domain types shared by the IPC, service, UI and vision
//! crates. All enums are `snake_case` on the wire and `#[non_exhaustive]` so a
//! new variant is never a silent `match` break in a downstream crate.
//!
//! None of these values are secret. Sid is an account identifier, not a
//! credential; scores and biometric features live in [`crate::secret`].

use serde::{Deserialize, Serialize};

/// Security identifier of an account, stored as its string form. Non-secret.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Sid(pub String);

impl Sid {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Credential Provider scenario (`CPUS_*`). Only Unlock is on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Scenario {
    Unlock,
    Logon,
    Remote,
    ChangePassword,
    CredUi,
    Plap,
}

/// Tile status reported over the auth pipe to the CP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Phase {
    Armed,
    Searching,
    Guidance,
    Challenge,
    Verified,
    Denied,
    Fallback,
    Disabled,
    ServiceDown,
}

/// Generic, score-free reason a face attempt was denied. Shown only as generic
/// UI wording — never a similarity number or a PAD margin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DenyReason {
    NoFace,
    MultipleFaces,
    PadRejected,
    Attention,
    LowSimilarity,
    Cooldown,
    CredentialStale,
    CameraMismatch,
    NotEnrolled,
    WrongSid,
    RemoteSession,
    NoRetry,
    Timeout,
    Other,
}

/// Opaque template identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TemplateId(pub u64);

/// Which management flow is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EnrolMode {
    New,
    AddTemplate,
    RefreshCredential,
}

/// Progress stage reported by the enrol wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EnrolStage {
    Prerequisites,
    Password,
    Capture,
    Seal,
    SelfTest,
    Summary,
}

/// Guided capture pose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Pose {
    Straight,
    Left,
    Right,
    Up,
    Down,
}

/// File kind used when deriving AEAD associated data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FileType {
    Template,
    Secret,
    Config,
    State,
}

/// Status snapshot returned by `GetStatus`. Contains no scores or secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    pub enabled: bool,
    pub camera_ok: bool,
    pub enrolled_sid: Option<Sid>,
    pub kill_switch: bool,
    pub face_paused: bool,
    /// Generic pause wording ("Paused for 10 min: 5 failures"). No scores.
    pub pause_note: Option<String>,
    pub service_pid: u32,
    pub template_count: u16,
}

/// Metadata for one stored template (never the embedding itself).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateMeta {
    pub id: TemplateId,
    /// Unix timestamp in seconds of capture.
    pub captured_at: u64,
    /// Free-form lighting condition label, e.g. `indoor`.
    pub lighting: String,
    pub glasses: bool,
    pub pose: Pose,
}

/// Result of a dry-run test. Never unlocks anything; no scores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestReport {
    pub completed: bool,
    pub matched: bool,
    pub latency_ms: u32,
    pub detail: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sid_helpers() {
        let sid = Sid::new("S-1-5-21-...");
        assert_eq!(sid.as_str(), "S-1-5-21-...");
    }

    #[test]
    fn enums_are_wire_serializable() {
        fn assert_serde<T: Serialize>() {}
        assert_serde::<Scenario>();
        assert_serde::<DenyReason>();
        assert_serde::<Phase>();
    }
}
