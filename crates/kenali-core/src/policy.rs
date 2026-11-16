//! Default policy engine (PRD §10).
//!
//! The engine is pure with respect to time: every method that needs "now" takes
//! an [`Instant`] supplied by the caller, so the whole rule table is
//! deterministically testable. No wall-clock access, no I/O, no logging of
//! scores or biometrics.
//!
//! NTSTATUS values arrive as `i32` from Windows (the high bit is set). The
//! constants below are the six results after which Kenali must never retry.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::{ConfigKey, ConfigReport, ConfigValue, DEFAULT_POLICY};
use crate::state::MAX_FACE_ATTEMPTS_PER_LOCK;
use crate::types::{DenyReason, Scenario, Sid};

/// NTSTATUS values that permanently invalidate a cached credential for this
/// lock lifecycle: LOGON_FAILURE, WRONG_PASSWORD, ACCOUNT_LOCKED_OUT,
/// ACCOUNT_DISABLED, PASSWORD_EXPIRED, PASSWORD_MUST_CHANGE.
///
/// The u32 literals are cast to `i32` so they match the signed value Windows
/// reports over the auth pipe.
pub const NO_RETRY_NTSTATUS: [i32; 6] = [
    0xC000006Du32 as i32,
    0xC000006Au32 as i32,
    0xC0000234u32 as i32,
    0xC0000072u32 as i32,
    0xC0000071u32 as i32,
    0xC0000224u32 as i32,
];

/// A knowledge factor (password/PIN) becomes stale after this long.
pub const KNOWLEDGE_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// Sliding window for PAD rejections (the face-failure window is configurable).
pub const PAD_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Per-attempt facts the service supplies. Everything the engine does not own
/// (session state, remote-ness, camera identity) arrives here.
#[derive(Debug, Clone, Copy)]
pub struct PolicyInput<'a> {
    pub scenario: Scenario,
    pub attempted_sid: &'a Sid,
    pub enrolled_sid: Option<&'a Sid>,
    pub is_remote: bool,
    pub live_session: bool,
    pub camera_pin_matches: bool,
    pub second_face_comparable: bool,
    pub elevated_risk: bool,
}

/// Typed outcome of a policy evaluation. Never a boolean plus a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PolicyDecision {
    /// Face unlock may be offered.
    AllowFace,
    /// Escalate to an active presentation-attack challenge.
    ActiveChallenge(Challenge),
    /// Reject this one attempt; no scores are rendered.
    Reject(DenyReason),
    /// Out of face tries for this lock — use password/PIN until the next lock.
    FallbackToPassword,
    /// A knowledge factor is mandatory for this session.
    PasswordRequired(PasswordReason),
    /// The tile is not offered for this scenario/SID/session.
    NotOffered(NotOfferedReason),
    /// Face unlock is disabled; `retry_after == None` means until an operator
    /// action (credential refresh / re-enrol), not merely until a timer.
    Disabled {
        reason: DisableReason,
        retry_after: Option<Duration>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Challenge {
    Blink,
    TurnHead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PasswordReason {
    /// Boot, restart or first sign-in (SEC-18).
    BootOrFirstSignIn,
    /// More than 24 h since the last knowledge-factor unlock.
    StaleCredential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NotOfferedReason {
    RemoteSession,
    ScenarioDisabled,
    OtherSid,
    NoLiveSession,
    NotEnrolled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DisableReason {
    /// Five consecutive face failures within the configured window.
    TooManyFailures,
    /// Three PAD rejections within ten minutes.
    TooManyPadRejections,
    /// A no-retry NTSTATUS was returned by Windows.
    WrongPassword,
    /// The pinned camera no longer matches (SEC-16).
    CameraChanged,
}

/// The engine's mutable state. All timestamps are caller-supplied `Instant`s.
#[derive(Debug)]
pub struct PolicyEngine {
    cfg: ConfigReport,
    failures: VecDeque<Instant>,
    pad_rejections: VecDeque<Instant>,
    failures_this_lock: u8,
    wrong_password: bool,
    camera_changed: bool,
    last_knowledge_unlock: Option<Instant>,
    pad_warning_due: bool,
}

impl PolicyEngine {
    pub fn new(cfg: ConfigReport) -> Self {
        Self {
            cfg,
            failures: VecDeque::new(),
            pad_rejections: VecDeque::new(),
            failures_this_lock: 0,
            wrong_password: false,
            camera_changed: false,
            last_knowledge_unlock: None,
            pad_warning_due: false,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(ConfigReport::default_policy())
    }

    pub fn config(&self) -> &ConfigReport {
        &self.cfg
    }

    /// `true` when Windows returned a result after which Kenali must never
    /// submit again (SEC-10 / FR-11).
    pub fn is_no_retry_status(nt_status: i32) -> bool {
        NO_RETRY_NTSTATUS.contains(&nt_status)
    }

    // ---- lifecycle / event recording -------------------------------------------------

    /// Boot / restart: no knowledge factor has been observed yet.
    pub fn note_boot(&mut self) {
        self.last_knowledge_unlock = None;
        self.failures_this_lock = 0;
        self.failures.clear();
        self.pad_rejections.clear();
    }

    /// A password/PIN unlock was observed (or a credential refresh succeeded).
    pub fn note_knowledge_unlock(&mut self, now: Instant) {
        self.last_knowledge_unlock = Some(now);
        self.failures.clear();
        self.failures_this_lock = 0;
    }

    /// A new lock screen appeared: the three-tries budget resets.
    pub fn note_lock_started(&mut self) {
        self.failures_this_lock = 0;
    }

    pub fn record_face_failure(&mut self, now: Instant) {
        push_bounded(&mut self.failures, now);
        self.failures_this_lock = self.failures_this_lock.saturating_add(1);
    }

    pub fn record_face_success(&mut self, _now: Instant) {
        self.failures.clear();
        self.failures_this_lock = 0;
    }

    pub fn record_pad_rejection(&mut self, now: Instant) {
        push_bounded(&mut self.pad_rejections, now);
        let max = self.u8_cfg(ConfigKey::MaxPadRejections);
        if recent_count(&self.pad_rejections, PAD_WINDOW, now) == max as usize {
            self.pad_warning_due = true;
        }
    }

    /// A Windows `Report` result arrived; disable if it is a no-retry status.
    pub fn record_ntstatus(&mut self, nt_status: i32) {
        if Self::is_no_retry_status(nt_status) {
            self.wrong_password = true;
        }
    }

    /// Explicit alias for the wrong-password branch (used by the UI flow).
    pub fn record_wrong_password(&mut self) {
        self.wrong_password = true;
    }

    /// The camera identity changed: face stays off until re-enrolment.
    pub fn record_camera_change(&mut self) {
        self.camera_changed = true;
    }

    /// Credential refresh wizard completed successfully.
    pub fn note_credential_refreshed(&mut self, now: Instant) {
        self.wrong_password = false;
        self.last_knowledge_unlock = Some(now);
    }

    /// Re-enrolment completed with the new pinned camera.
    pub fn note_reenrolled(&mut self) {
        self.camera_changed = false;
    }

    /// Event Log warning for a PAD-rejection burst, consumed exactly once.
    pub fn take_pad_warning(&mut self) -> bool {
        std::mem::take(&mut self.pad_warning_due)
    }

    // ---- evaluation ------------------------------------------------------------------

    /// Evaluate the full §10 table for one attempt.
    pub fn decide(&self, input: &PolicyInput<'_>, now: Instant) -> PolicyDecision {
        // 1. Permanent-until-operator disables win over everything else.
        if self.wrong_password {
            return PolicyDecision::Disabled {
                reason: DisableReason::WrongPassword,
                retry_after: None,
            };
        }
        if self.camera_changed || !input.camera_pin_matches {
            return PolicyDecision::Disabled {
                reason: DisableReason::CameraChanged,
                retry_after: None,
            };
        }

        // 2. Timed cooldowns (5 fails / 10 min, 3 PAD rejects / 10 min).
        if let Some((reason, until)) = self.cooldown_disable(now) {
            return PolicyDecision::Disabled {
                reason,
                retry_after: until.map(|t| t.saturating_duration_since(now)),
            };
        }

        // 3. RDP is never offered.
        if input.is_remote || matches!(input.scenario, Scenario::Remote) {
            return PolicyDecision::NotOffered(NotOfferedReason::RemoteSession);
        }

        // 4. Scenario gating: unlock on by default, logon opt-in, rest never.
        match input.scenario {
            Scenario::Unlock => {}
            Scenario::Logon if self.bool_cfg(ConfigKey::ScopeLogon) => {}
            _ => return PolicyDecision::NotOffered(NotOfferedReason::ScenarioDisabled),
        }

        // 5. Boot / restart / first sign-in requires the knowledge factor.
        let Some(last_knowledge) = self.last_knowledge_unlock else {
            return PolicyDecision::PasswordRequired(PasswordReason::BootOrFirstSignIn);
        };
        if now.saturating_duration_since(last_knowledge) >= KNOWLEDGE_MAX_AGE {
            return PolicyDecision::PasswordRequired(PasswordReason::StaleCredential);
        }

        // 6. Unlock is for a live session of the enrolled SID only (SEC-17/18).
        if !input.live_session {
            return PolicyDecision::NotOffered(NotOfferedReason::NoLiveSession);
        }
        match input.enrolled_sid {
            None => return PolicyDecision::NotOffered(NotOfferedReason::NotEnrolled),
            Some(enrolled) if enrolled != input.attempted_sid => {
                return PolicyDecision::NotOffered(NotOfferedReason::OtherSid)
            }
            Some(_) => {}
        }

        // 7. A comparable-size second face rejects this attempt (FR-13).
        if input.second_face_comparable && self.bool_cfg(ConfigKey::MultiFaceRejection) {
            return PolicyDecision::Reject(DenyReason::MultipleFaces);
        }

        // 8. Three failures in one lock fall back to the password tile.
        if self.failures_this_lock >= MAX_FACE_ATTEMPTS_PER_LOCK {
            return PolicyDecision::FallbackToPassword;
        }

        // 9. Elevated risk escalates to an active challenge (FR-06).
        if input.elevated_risk {
            let challenge = if self.failures_this_lock == 0 {
                Challenge::Blink
            } else {
                Challenge::TurnHead
            };
            return PolicyDecision::ActiveChallenge(challenge);
        }

        PolicyDecision::AllowFace
    }

    // ---- helpers ---------------------------------------------------------------------

    fn cooldown_disable(&self, now: Instant) -> Option<(DisableReason, Option<Instant>)> {
        let window = Duration::from_secs(u64::from(self.u32_cfg(ConfigKey::FailureWindowSecs)));
        let max_failures = usize::from(self.u8_cfg(ConfigKey::MaxFailuresPerWindow));
        if recent_count(&self.failures, window, now) >= max_failures {
            let cooldown = Duration::from_secs(u64::from(
                self.u32_cfg(ConfigKey::FaceFailureCooldownSecs),
            ));
            let until = self.failures.back().map(|t| *t + cooldown);
            return Some((DisableReason::TooManyFailures, until));
        }

        let max_pad = usize::from(self.u8_cfg(ConfigKey::MaxPadRejections));
        if recent_count(&self.pad_rejections, PAD_WINDOW, now) >= max_pad {
            let cooldown = Duration::from_secs(u64::from(
                self.u32_cfg(ConfigKey::PadFailureCooldownSecs),
            ));
            let until = self.pad_rejections.back().map(|t| *t + cooldown);
            return Some((DisableReason::TooManyPadRejections, until));
        }
        None
    }

    fn value(&self, key: ConfigKey) -> ConfigValue {
        self.cfg.get(key).unwrap_or_else(|| {
            DEFAULT_POLICY
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| *v)
                .unwrap_or(ConfigValue::Bool(false))
        })
    }

    fn u8_cfg(&self, key: ConfigKey) -> u8 {
        match self.value(key) {
            ConfigValue::U8(n) => n,
            _ => 0,
        }
    }

    fn u32_cfg(&self, key: ConfigKey) -> u32 {
        match self.value(key) {
            ConfigValue::U32(n) => n,
            _ => 0,
        }
    }

    fn bool_cfg(&self, key: ConfigKey) -> bool {
        matches!(self.value(key), ConfigValue::Bool(true))
    }
}

/// Keep the history bounded so an attacker cannot grow it indefinitely; 256
/// events is far above any configured threshold (max 255).
fn push_bounded(history: &mut VecDeque<Instant>, at: Instant) {
    if history.len() == 256 {
        let _ = history.pop_front();
    }
    history.push_back(at);
}

fn recent_count(history: &VecDeque<Instant>, window: Duration, now: Instant) -> usize {
    let cutoff = now.checked_sub(window);
    history
        .iter()
        .rev()
        .take_while(|t| match cutoff {
            Some(c) => **t >= c,
            None => true,
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::time::Duration as StdDuration;

    fn input<'a>(sid: &'a Sid, enrolled: Option<&'a Sid>) -> PolicyInput<'a> {
        PolicyInput {
            scenario: Scenario::Unlock,
            attempted_sid: sid,
            enrolled_sid: enrolled,
            is_remote: false,
            live_session: true,
            camera_pin_matches: true,
            second_face_comparable: false,
            elevated_risk: false,
        }
    }

    fn ready_engine(t0: Instant) -> PolicyEngine {
        let mut e = PolicyEngine::with_defaults();
        e.note_knowledge_unlock(t0);
        e
    }

    #[test]
    fn no_retry_status_table_is_signed_and_complete() {
        assert_eq!(NO_RETRY_NTSTATUS.len(), 6);
        assert_eq!(NO_RETRY_NTSTATUS[0], 0xC000006Du32 as i32);
        assert_eq!(NO_RETRY_NTSTATUS[1], 0xC000006Au32 as i32);
        assert_eq!(NO_RETRY_NTSTATUS[2], 0xC0000234u32 as i32);
        assert_eq!(NO_RETRY_NTSTATUS[3], 0xC0000072u32 as i32);
        assert_eq!(NO_RETRY_NTSTATUS[4], 0xC0000071u32 as i32);
        assert_eq!(NO_RETRY_NTSTATUS[5], 0xC0000224u32 as i32);
        for status in NO_RETRY_NTSTATUS {
            assert!(PolicyEngine::is_no_retry_status(status));
        }
        assert!(!PolicyEngine::is_no_retry_status(0));
    }

    #[test]
    fn boot_and_first_sign_in_are_password_only() {
        let t0 = Instant::now();
        let e = PolicyEngine::with_defaults();
        let sid = Sid::new("S-1-test");
        assert_eq!(
            e.decide(&input(&sid, Some(&sid)), t0),
            PolicyDecision::PasswordRequired(PasswordReason::BootOrFirstSignIn)
        );
    }

    #[test]
    fn unlock_is_allowed_for_enrolled_sid_on_live_session() {
        let t0 = Instant::now();
        let e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        assert_eq!(e.decide(&input(&sid, Some(&sid)), t0), PolicyDecision::AllowFace);
    }

    #[test]
    fn logon_is_off_by_default_and_rdp_never_offered() {
        let t0 = Instant::now();
        let e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        let mut logon = input(&sid, Some(&sid));
        logon.scenario = Scenario::Logon;
        assert_eq!(
            e.decide(&logon, t0),
            PolicyDecision::NotOffered(NotOfferedReason::ScenarioDisabled)
        );
        let mut rdp = input(&sid, Some(&sid));
        rdp.is_remote = true;
        assert_eq!(
            e.decide(&rdp, t0),
            PolicyDecision::NotOffered(NotOfferedReason::RemoteSession)
        );
    }

    #[test]
    fn wrong_sid_and_no_live_session_are_not_offered() {
        let t0 = Instant::now();
        let e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        let other = Sid::new("S-1-other");
        assert_eq!(
            e.decide(&input(&sid, Some(&other)), t0),
            PolicyDecision::NotOffered(NotOfferedReason::OtherSid)
        );
        let mut no_live = input(&sid, Some(&sid));
        no_live.live_session = false;
        assert_eq!(
            e.decide(&no_live, t0),
            PolicyDecision::NotOffered(NotOfferedReason::NoLiveSession)
        );
    }

    #[test]
    fn stale_credential_after_24h_requires_password() {
        let t0 = Instant::now();
        let e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        let later = t0 + KNOWLEDGE_MAX_AGE;
        assert_eq!(
            e.decide(&input(&sid, Some(&sid)), later),
            PolicyDecision::PasswordRequired(PasswordReason::StaleCredential)
        );
    }

    #[test]
    fn three_failures_in_one_lock_falls_back() {
        let t0 = Instant::now();
        let mut e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        e.record_face_failure(t0);
        e.record_face_failure(t0);
        assert_eq!(e.decide(&input(&sid, Some(&sid)), t0), PolicyDecision::AllowFace);
        e.record_face_failure(t0);
        assert_eq!(
            e.decide(&input(&sid, Some(&sid)), t0),
            PolicyDecision::FallbackToPassword
        );
        e.note_lock_started();
        assert_eq!(e.decide(&input(&sid, Some(&sid)), t0), PolicyDecision::AllowFace);
    }

    #[test]
    fn five_failures_in_window_disables_ten_minutes() {
        let t0 = Instant::now();
        let mut e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        for i in 0..5 {
            e.record_face_failure(t0 + StdDuration::from_secs(i));
        }
        let d = e.decide(&input(&sid, Some(&sid)), t0 + StdDuration::from_secs(4));
        match d {
            PolicyDecision::Disabled {
                reason: DisableReason::TooManyFailures,
                retry_after: Some(rem),
            } => assert_eq!(rem, StdDuration::from_secs(600)),
            other => panic!("expected failure cooldown, got {other:?}"),
        }
        let after = t0 + StdDuration::from_secs(4) + StdDuration::from_secs(601);
        e.note_lock_started();
        assert_eq!(e.decide(&input(&sid, Some(&sid)), after), PolicyDecision::AllowFace);
    }

    #[test]
    fn three_pad_rejections_disable_thirty_minutes_with_warning() {
        let t0 = Instant::now();
        let mut e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        for i in 0..3 {
            e.record_pad_rejection(t0 + StdDuration::from_secs(i));
        }
        assert!(e.take_pad_warning());
        assert!(!e.take_pad_warning());
        let d = e.decide(&input(&sid, Some(&sid)), t0 + StdDuration::from_secs(2));
        assert_eq!(
            d,
            PolicyDecision::Disabled {
                reason: DisableReason::TooManyPadRejections,
                retry_after: Some(StdDuration::from_secs(1800)),
            }
        );
    }

    #[test]
    fn wrong_password_disables_until_refresh_and_never_retries() {
        let t0 = Instant::now();
        let mut e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        e.record_ntstatus(0xC000006Au32 as i32);
        let expected = PolicyDecision::Disabled {
            reason: DisableReason::WrongPassword,
            retry_after: None,
        };
        assert_eq!(e.decide(&input(&sid, Some(&sid)), t0), expected);
        assert_eq!(
            e.decide(&input(&sid, Some(&sid)), t0 + StdDuration::from_secs(100_000)),
            expected
        );
        e.note_credential_refreshed(t0 + StdDuration::from_secs(100_001));
        assert_eq!(
            e.decide(&input(&sid, Some(&sid)), t0 + StdDuration::from_secs(100_001)),
            PolicyDecision::AllowFace
        );
    }

    #[test]
    fn camera_change_disables_until_reenrolment() {
        let t0 = Instant::now();
        let mut e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        e.record_camera_change();
        assert_eq!(
            e.decide(&input(&sid, Some(&sid)), t0),
            PolicyDecision::Disabled {
                reason: DisableReason::CameraChanged,
                retry_after: None,
            }
        );
        e.note_reenrolled();
        assert_eq!(e.decide(&input(&sid, Some(&sid)), t0), PolicyDecision::AllowFace);
    }

    #[test]
    fn comparable_second_face_and_risk_are_handled() {
        let t0 = Instant::now();
        let e = ready_engine(t0);
        let sid = Sid::new("S-1-test");
        let mut two_faces = input(&sid, Some(&sid));
        two_faces.second_face_comparable = true;
        assert_eq!(
            e.decide(&two_faces, t0),
            PolicyDecision::Reject(DenyReason::MultipleFaces)
        );
        let mut risk = input(&sid, Some(&sid));
        risk.elevated_risk = true;
        assert_eq!(
            e.decide(&risk, t0),
            PolicyDecision::ActiveChallenge(Challenge::Blink)
        );
    }

    proptest! {
        #[test]
        fn no_retry_status_never_allows_face(status in any::<i32>()) {
            let t0 = Instant::now();
            let mut e = ready_engine(t0);
            e.record_ntstatus(status);
            let sid = Sid::new("S-1-test");
            if PolicyEngine::is_no_retry_status(status) {
                let decision = e.decide(&input(&sid, Some(&sid)), t0);
                let disabled_wrong_password = matches!(
                    decision,
                    PolicyDecision::Disabled {
                        reason: DisableReason::WrongPassword,
                        retry_after: None,
                    }
                );
                prop_assert!(disabled_wrong_password);
            }
        }

        #[test]
        fn never_panics_on_event_orderings(
            events in prop::collection::vec(0u8..9, 0..40),
            offsets in prop::collection::vec(0u64..90_000, 0..40),
        ) {
            let t0 = Instant::now();
            let mut e = ready_engine(t0);
            let sid = Sid::new("S-1-test");
            for (event, offset) in events.into_iter().zip(offsets) {
                let now = t0 + StdDuration::from_secs(offset);
                match event {
                    0 => e.note_boot(),
                    1 => e.note_knowledge_unlock(now),
                    2 => e.note_lock_started(),
                    3 => e.record_face_failure(now),
                    4 => e.record_face_success(now),
                    5 => e.record_pad_rejection(now),
                    6 => e.record_ntstatus(0xC000006Au32 as i32),
                    7 => e.record_camera_change(),
                    _ => { let _ = e.decide(&input(&sid, Some(&sid)), now); }
                }
            }
        }
    }
}
