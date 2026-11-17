//! Auth-attempt orchestration: the single place that binds the merged
//! [`AttemptStateMachine`] (one submit per attempt) to the merged
//! [`PolicyEngine`] (the §10 rule table).
//!
//! No score, biometric or secret is ever rendered here: the only outward-facing
//! values are a `Phase` and a generic `DenyReason`. A verified face releases a
//! one-shot credential to the originating instance for at most
//! [`SECRET_RELEASE_WINDOW`], bound to the nonce of the `BeginAttempt` that
//! produced it. Everything is deterministic over a caller-supplied `Instant`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use kenali_core::config::ConfigReport;
use kenali_core::policy::{
    Challenge, DisableReason, NotOfferedReason, PasswordReason, PolicyDecision, PolicyEngine,
    PolicyInput,
};
use kenali_core::state::{AttemptEvent, AttemptState, AttemptStateMachine};
use kenali_core::types::{DenyReason, Phase, Scenario, Sid};
use kenali_ipc::auth::Response;
use kenali_ipc::wire::{SecretWide, Wide};
use zeroize::Zeroizing;

/// A verified face releases the one-shot credential for at most this long.
/// Beyond it the release is refused and the attempt fails open to password.
pub const SECRET_RELEASE_WINDOW: Duration = Duration::from_secs(3);

/// Facts the service establishes outside the auth pipe (session state, camera
/// identity, enrolment). The default is deliberately "no face": until facts are
/// supplied the policy returns a fallback decision, so the password tile stays
/// available and is never hidden or delayed.
#[derive(Debug, Clone)]
pub struct AttemptContext {
    pub enrolled_sid: Option<Sid>,
    pub camera_pin_matches: bool,
    pub live_session: bool,
    pub second_face_comparable: bool,
    pub elevated_risk: bool,
    pub is_remote: bool,
}

impl Default for AttemptContext {
    fn default() -> Self {
        Self {
            enrolled_sid: None,
            camera_pin_matches: false,
            live_session: false,
            second_face_comparable: false,
            elevated_risk: false,
            is_remote: false,
        }
    }
}

/// The one-shot credential released on a verified face. Held zeroizing; never
/// formatted or logged.
#[derive(Clone)]
pub struct Credential {
    pub user: Vec<u16>,
    pub domain: Vec<u16>,
    pub secret: Zeroizing<Vec<u16>>,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Credential(<redacted>)")
    }
}

struct ActiveAttempt {
    id: u64,
    nonce: [u8; 16],
    sid: Sid,
    scenario: Scenario,
    decided_at: Instant,
    failed_open: bool,
}

/// Result of arming an attempt — maps directly onto an auth-pipe response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginOutcome {
    Searching,
    Challenge(Challenge),
    Denied(DenyReason),
    PasswordFallback,
}

/// Result of the one allowed submit for an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitOutcome {
    Accepted,
    Denied(DenyReason),
    Ignored,
}

/// Wraps the two merged state machines. Every event is total and never panics.
pub struct AuthAttemptMachine {
    attempts: AttemptStateMachine,
    policy: PolicyEngine,
    context: AttemptContext,
    credential: Option<Credential>,
    active: Option<ActiveAttempt>,
    next_id: u64,
    pending: VecDeque<Response>,
}

impl AuthAttemptMachine {
    pub fn new(cfg: ConfigReport) -> Self {
        Self {
            attempts: AttemptStateMachine::new(),
            policy: PolicyEngine::new(cfg),
            context: AttemptContext::default(),
            credential: None,
            active: None,
            next_id: 1,
            pending: VecDeque::new(),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(ConfigReport::default_policy())
    }

    pub fn policy(&self) -> &PolicyEngine {
        &self.policy
    }

    pub fn policy_mut(&mut self) -> &mut PolicyEngine {
        &mut self.policy
    }

    pub fn state(&self) -> AttemptState {
        self.attempts.state()
    }

    pub fn phase(&self) -> Phase {
        phase_for_state(self.attempts.state())
    }

    pub fn set_context(&mut self, context: AttemptContext) {
        self.context = context;
    }

    pub fn context_mut(&mut self) -> &mut AttemptContext {
        &mut self.context
    }

    pub fn set_credential(&mut self, credential: Credential) {
        self.credential = Some(credential);
    }

    pub fn clear_credential(&mut self) {
        self.credential = None;
    }

    /// Event-log warning for a PAD-rejection burst, consumed exactly once.
    pub fn take_pad_warning(&mut self) -> bool {
        self.policy.take_pad_warning()
    }

    /// Arm a new attempt. Returns how the CP tile should behave.
    pub fn begin(
        &mut self,
        scenario: Scenario,
        sid: Sid,
        nonce: [u8; 16],
        now: Instant,
    ) -> (u64, BeginOutcome) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);

        let input = PolicyInput {
            scenario,
            attempted_sid: &sid,
            enrolled_sid: self.context.enrolled_sid.as_ref(),
            is_remote: self.context.is_remote,
            live_session: self.context.live_session,
            camera_pin_matches: self.context.camera_pin_matches,
            second_face_comparable: self.context.second_face_comparable,
            elevated_risk: self.context.elevated_risk,
        };
        let decision = self.policy.decide(&input, now);
        self.policy.note_lock_started();

        let active = ActiveAttempt {
            id,
            nonce,
            sid,
            scenario,
            decided_at: now,
            failed_open: false,
        };
        self.active = Some(active);
        self.attempts.handle(AttemptEvent::Start, now);

        let outcome = match &decision {
            PolicyDecision::AllowFace => BeginOutcome::Searching,
            PolicyDecision::ActiveChallenge(challenge) => {
                let _ = self.attempts.handle(AttemptEvent::ChallengeRequired, now);
                BeginOutcome::Challenge(*challenge)
            }
            PolicyDecision::Reject(reason) => {
                let _ = self.attempts.handle(AttemptEvent::SubmitFailure(*reason), now);
                BeginOutcome::Denied(*reason)
            }
            PolicyDecision::FallbackToPassword
            | PolicyDecision::PasswordRequired(_)
            | PolicyDecision::NotOffered(_)
            | PolicyDecision::Disabled { .. } => {
                self.mark_failed_open();
                BeginOutcome::PasswordFallback
            }
            _ => {
                self.mark_failed_open();
                BeginOutcome::PasswordFallback
            }
        };

        if !matches!(outcome, BeginOutcome::Searching | BeginOutcome::Challenge(_)) {
            self.queue_password_fallback(id);
        }
        (id, outcome)
    }

    /// The single allowed recognition result for the active attempt. A second
    /// submit (success or failure) is ignored — one submit per attempt.
    pub fn submit(&mut self, matched: bool, pad_ok: bool, now: Instant) -> SubmitOutcome {
        if !self.attempts.can_submit() {
            return SubmitOutcome::Ignored;
        }
        let Some(active) = self.active.as_ref() else {
            return SubmitOutcome::Ignored;
        };
        if active.failed_open {
            return SubmitOutcome::Ignored;
        }
        let id = active.id;
        let nonce = active.nonce;
        let window = now.saturating_duration_since(active.decided_at);
        let recent = window <= SECRET_RELEASE_WINDOW;
        let scenario = active.scenario;
        let sid = active.sid.clone();
        let _ = scenario;
        let _ = sid;

        if matched && pad_ok && recent {
            if let Some(credential) = self.credential.as_ref() {
                let response = Response::Verified {
                    attempt: id,
                    nonce,
                    user: Wide::from_units(credential.user.clone()),
                    domain: Wide::from_units(credential.domain.clone()),
                    secret: SecretWide::from_units(credential.secret.to_vec()),
                };
                self.pending.push_back(response);
                let _ = self.attempts.handle(AttemptEvent::SubmitSuccess, now);
                self.policy.record_face_success(now);
                self.active = None;
                return SubmitOutcome::Accepted;
            }
        }

        let reason = if !recent {
            DenyReason::Timeout
        } else if !pad_ok {
            DenyReason::PadRejected
        } else {
            DenyReason::LowSimilarity
        };
        self.policy.record_face_failure(now);
        if !pad_ok {
            self.policy.record_pad_rejection(now);
        }
        let transition = self.attempts.handle(AttemptEvent::SubmitFailure(reason), now);
        let fell_back = matches!(transition, kenali_core::state::TransitionResult::Changed { to: AttemptState::Fallback, .. })
            || self.attempts.state() == AttemptState::Fallback;
        if fell_back {
            self.mark_failed_open();
            self.queue_password_fallback(id);
        } else {
            self.pending.push_back(Response::Denied { attempt: id, reason });
        }
        self.active = None;
        SubmitOutcome::Denied(reason)
    }

    /// A Windows `Report` result. No-retry statuses permanently disable face
    /// and the credential is dropped; wording stays generic at every layer.
    pub fn report(&mut self, nt_status: i32, now: Instant) {
        let no_retry = PolicyEngine::is_no_retry_status(nt_status);
        self.policy.record_ntstatus(nt_status);
        if no_retry {
            self.credential = None;
        }
        let _ = now;
    }

    pub fn cancel(&mut self, now: Instant) {
        let _ = self.attempts.handle(AttemptEvent::Cancel, now);
        self.active = None;
    }

    pub fn service_down(&mut self, now: Instant) {
        self.active = None;
        let _ = self.attempts.handle(AttemptEvent::ServiceDown, now);
    }

    pub fn recovered(&mut self, now: Instant) {
        let _ = self.attempts.handle(AttemptEvent::Recovered, now);
    }

    pub fn new_lock(&mut self, now: Instant) {
        let _ = self.attempts.handle(AttemptEvent::NewLock, now);
        self.active = None;
    }

    pub fn take_pending(&mut self) -> Vec<Response> {
        self.pending.drain(..).collect()
    }

    fn mark_failed_open(&mut self) {
        if let Some(active) = self.active.as_mut() {
            active.failed_open = true;
        }
    }

    fn queue_password_fallback(&mut self, attempt: u64) {
        self.pending.push_back(Response::Status {
            attempt,
            phase: Phase::Fallback,
        });
    }
}

/// Map a state-machine state to the CP tile phase. Total over the
/// `#[non_exhaustive]` input via the wildcard arm.
pub fn phase_for_state(state: AttemptState) -> Phase {
    match state {
        AttemptState::Armed => Phase::Armed,
        AttemptState::Searching => Phase::Searching,
        AttemptState::Guidance => Phase::Guidance,
        AttemptState::Challenge => Phase::Challenge,
        AttemptState::Verified => Phase::Verified,
        AttemptState::Denied => Phase::Denied,
        AttemptState::Fallback => Phase::Fallback,
        AttemptState::Disabled => Phase::Disabled,
        AttemptState::ServiceDown => Phase::ServiceDown,
        _ => Phase::Fallback,
    }
}

/// Map a typed policy decision to how the face tile should behave.
pub fn phase_for_decision(decision: &PolicyDecision) -> Phase {
    match decision {
        PolicyDecision::AllowFace => Phase::Searching,
        PolicyDecision::ActiveChallenge(_) => Phase::Challenge,
        PolicyDecision::Reject(_) => Phase::Denied,
        _ => Phase::Fallback,
    }
}

/// Generic, score-free denial reason for a policy decision.
pub fn deny_reason_for_decision(decision: &PolicyDecision) -> DenyReason {
    match decision {
        PolicyDecision::Reject(reason) => *reason,
        PolicyDecision::Disabled { reason, .. } => match reason {
            DisableReason::WrongPassword => DenyReason::NoRetry,
            DisableReason::CameraChanged => DenyReason::CameraMismatch,
            DisableReason::TooManyFailures | DisableReason::TooManyPadRejections => {
                DenyReason::Cooldown
            }
            _ => DenyReason::Other,
        },
        PolicyDecision::NotOffered(reason) => match reason {
            NotOfferedReason::RemoteSession => DenyReason::RemoteSession,
            NotOfferedReason::OtherSid => DenyReason::WrongSid,
            NotOfferedReason::NotEnrolled => DenyReason::NotEnrolled,
            _ => DenyReason::Other,
        },
        PolicyDecision::PasswordRequired(reason) => match reason {
            PasswordReason::StaleCredential => DenyReason::CredentialStale,
            _ => DenyReason::Other,
        },
        PolicyDecision::FallbackToPassword => DenyReason::Other,
        _ => DenyReason::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> AuthAttemptMachine {
        let mut machine = AuthAttemptMachine::with_defaults();
        let sid = Sid::new("S-1-test");
        machine.set_context(AttemptContext {
            enrolled_sid: Some(sid),
            camera_pin_matches: true,
            live_session: true,
            ..AttemptContext::default()
        });
        let now = Instant::now();
        machine.policy_mut().note_knowledge_unlock(now);
        machine
    }

    #[test]
    fn default_context_fails_open_to_password() {
        let mut machine = AuthAttemptMachine::with_defaults();
        let now = Instant::now();
        let sid = Sid::new("S-1-test");
        let (_id, outcome) = machine.begin(Scenario::Unlock, sid, [0u8; 16], now);
        assert_eq!(outcome, BeginOutcome::PasswordFallback);
        assert_eq!(machine.state(), AttemptState::Searching);
    }

    #[test]
    fn allowed_context_offers_face() {
        let mut machine = ready();
        let sid = Sid::new("S-1-test");
        let (_id, outcome) = machine.begin(Scenario::Unlock, sid, [1u8; 16], Instant::now());
        assert_eq!(outcome, BeginOutcome::Searching);
    }

    #[test]
    fn one_submit_per_attempt_is_enforced() {
        let mut machine = ready();
        let sid = Sid::new("S-1-test");
        let _ = machine.begin(Scenario::Unlock, sid, [2u8; 16], Instant::now());
        assert_eq!(machine.submit(false, false, Instant::now()), SubmitOutcome::Denied(DenyReason::PadRejected));
        assert_eq!(machine.submit(true, true, Instant::now()), SubmitOutcome::Ignored);
    }

    #[test]
    fn verified_releases_nonce_echoed_secret() {
        let mut machine = ready();
        let secret: Vec<u16> = "hash".encode_utf16().collect();
        machine.set_credential(Credential {
            user: "user".encode_utf16().collect(),
            domain: "domain".encode_utf16().collect(),
            secret: Zeroizing::new(secret),
        });
        let sid = Sid::new("S-1-test");
        let (_id, _outcome) = machine.begin(Scenario::Unlock, sid, [3u8; 16], Instant::now());
        assert_eq!(machine.submit(true, true, Instant::now()), SubmitOutcome::Accepted);
        let pending = machine.take_pending();
        let mut found = false;
        for response in pending {
            if let Response::Verified { nonce, .. } = response {
                assert_eq!(nonce, [3u8; 16]);
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn stale_decision_never_releases_secret() {
        let mut machine = ready();
        let secret: Vec<u16> = "hash".encode_utf16().collect();
        machine.set_credential(Credential {
            user: vec![],
            domain: vec![],
            secret: Zeroizing::new(secret),
        });
        let now = Instant::now();
        let sid = Sid::new("S-1-test");
        let (_id, _outcome) = machine.begin(Scenario::Unlock, sid, [4u8; 16], now);
        let later = now + SECRET_RELEASE_WINDOW + Duration::from_secs(1);
        assert_eq!(machine.submit(true, true, later), SubmitOutcome::Denied(DenyReason::Timeout));
        for response in machine.take_pending() {
            assert!(!matches!(response, Response::Verified { .. }));
        }
    }

    #[test]
    fn decision_mapping_is_generic_and_total() {
        let disabled = PolicyDecision::Disabled {
            reason: DisableReason::WrongPassword,
            retry_after: None,
        };
        assert_eq!(phase_for_decision(&disabled), Phase::Fallback);
        assert_eq!(deny_reason_for_decision(&disabled), DenyReason::NoRetry);
        assert_eq!(phase_for_decision(&PolicyDecision::AllowFace), Phase::Searching);
        assert_eq!(phase_for_decision(&PolicyDecision::Reject(DenyReason::NoFace)), Phase::Denied);
    }
}
