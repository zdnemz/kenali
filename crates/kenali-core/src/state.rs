//! Lock-screen attempt state machine.
//!
//! This is the single place that enforces *one submit per attempt*: after a
//! success or a denial, any further submit event is rejected rather than
//! silently ignored or acted on. It also owns the three-strikes-per-lock
//! counter and the cooldown transition. Every event is total — unexpected
//! orderings return [`TransitionResult::Ignored`] and never panic.

use std::time::{Duration, Instant};

use crate::types::DenyReason;

/// Face tries allowed on one lock screen before falling back to password.
pub const MAX_FACE_ATTEMPTS_PER_LOCK: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AttemptState {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AttemptEvent {
    /// Arm the camera for a new attempt.
    Start,
    /// A face was detected.
    FaceSeen,
    NeedCloser,
    TooDark,
    LookAtScreen,
    /// Risk was elevated; ask for an active challenge.
    ChallengeRequired,
    /// A single recognition result was accepted. Consumes the submit.
    SubmitSuccess,
    /// A single recognition result was refused. Consumes the submit.
    SubmitFailure(DenyReason),
    /// The user cancelled or the attempt timed out.
    Cancel,
    /// Enter a cooldown for this duration.
    Cooldown(Duration),
    /// The service died — always fail open to password.
    ServiceDown,
    /// The service came back.
    Recovered,
    /// A new lock screen; resets the per-lock failure counter.
    NewLock,
    /// Operator re-enabled face unlock.
    Reenable,
    /// Time advanced; may expire an active cooldown.
    Tick(Instant),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AttemptError {
    #[error("submit already consumed for this attempt")]
    AlreadySubmitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransitionResult {
    Changed { from: AttemptState, to: AttemptState },
    Ignored,
    Rejected(AttemptError),
}

#[derive(Debug)]
pub struct AttemptStateMachine {
    state: AttemptState,
    submitted: bool,
    failures_this_lock: u8,
    disabled_until: Option<Instant>,
}

impl Default for AttemptStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl AttemptStateMachine {
    pub fn new() -> Self {
        Self {
            state: AttemptState::Armed,
            submitted: false,
            failures_this_lock: 0,
            disabled_until: None,
        }
    }

    pub fn state(&self) -> AttemptState {
        self.state
    }

    pub fn failures_this_lock(&self) -> u8 {
        self.failures_this_lock
    }

    pub fn disabled_until(&self) -> Option<Instant> {
        self.disabled_until
    }

    /// Whether a submit event would be accepted right now.
    pub fn can_submit(&self) -> bool {
        !self.submitted
            && !matches!(
                self.state,
                AttemptState::Verified | AttemptState::Denied | AttemptState::Fallback
            )
    }

    /// Handle one event at time `now`. Total over all states and events.
    pub fn handle(&mut self, event: AttemptEvent, now: Instant) -> TransitionResult {
        let effective_now = match event {
            AttemptEvent::Tick(at) => at,
            _ => now,
        };
        let initial = self.state;
        let expired = self.expire_cooldown(effective_now);

        if let AttemptEvent::Tick(_) = event {
            return if expired {
                TransitionResult::Changed {
                    from: initial,
                    to: self.state,
                }
            } else {
                TransitionResult::Ignored
            };
        }

        match event {
            AttemptEvent::Start => {
                if matches!(self.state, AttemptState::ServiceDown | AttemptState::Disabled) {
                    return TransitionResult::Ignored;
                }
                self.submitted = false;
                self.to(AttemptState::Searching)
            }
            AttemptEvent::FaceSeen => {
                if self.state == AttemptState::Searching {
                    self.to(AttemptState::Guidance)
                } else {
                    TransitionResult::Ignored
                }
            }
            AttemptEvent::NeedCloser | AttemptEvent::TooDark | AttemptEvent::LookAtScreen => {
                if matches!(self.state, AttemptState::Searching | AttemptState::Guidance) {
                    self.to(AttemptState::Guidance)
                } else {
                    TransitionResult::Ignored
                }
            }
            AttemptEvent::ChallengeRequired => {
                if matches!(self.state, AttemptState::Searching | AttemptState::Guidance) {
                    self.to(AttemptState::Challenge)
                } else {
                    TransitionResult::Ignored
                }
            }
            AttemptEvent::SubmitSuccess => {
                if !self.can_submit() {
                    return TransitionResult::Rejected(AttemptError::AlreadySubmitted);
                }
                self.submitted = true;
                self.failures_this_lock = 0;
                self.to(AttemptState::Verified)
            }
            AttemptEvent::SubmitFailure(_reason) => {
                if !self.can_submit() {
                    return TransitionResult::Rejected(AttemptError::AlreadySubmitted);
                }
                self.submitted = true;
                self.failures_this_lock = self.failures_this_lock.saturating_add(1);
                let next = if self.failures_this_lock >= MAX_FACE_ATTEMPTS_PER_LOCK {
                    AttemptState::Fallback
                } else {
                    AttemptState::Denied
                };
                self.to(next)
            }
            AttemptEvent::Cancel => {
                if matches!(self.state, AttemptState::ServiceDown | AttemptState::Disabled) {
                    return TransitionResult::Ignored;
                }
                self.submitted = false;
                self.to(AttemptState::Armed)
            }
            AttemptEvent::Cooldown(duration) => {
                self.disabled_until = Some(now + duration);
                self.submitted = true;
                self.to(AttemptState::Disabled)
            }
            AttemptEvent::ServiceDown => {
                self.disabled_until = None;
                self.submitted = true;
                self.to(AttemptState::ServiceDown)
            }
            AttemptEvent::Recovered => {
                if self.state == AttemptState::ServiceDown {
                    self.submitted = false;
                    self.to(AttemptState::Armed)
                } else {
                    TransitionResult::Ignored
                }
            }
            AttemptEvent::NewLock => {
                self.failures_this_lock = 0;
                self.submitted = false;
                if self.disabled_until.is_some() {
                    self.to(AttemptState::Disabled)
                } else {
                    self.to(AttemptState::Armed)
                }
            }
            AttemptEvent::Reenable => {
                if matches!(self.state, AttemptState::Disabled | AttemptState::Fallback) {
                    self.disabled_until = None;
                    self.submitted = false;
                    self.to(AttemptState::Armed)
                } else {
                    TransitionResult::Ignored
                }
            }
            AttemptEvent::Tick(_) => TransitionResult::Ignored,
        }
    }

    fn to(&mut self, next: AttemptState) -> TransitionResult {
        let from = self.state;
        self.state = next;
        if from == next {
            TransitionResult::Ignored
        } else {
            TransitionResult::Changed { from, to: next }
        }
    }

    fn expire_cooldown(&mut self, now: Instant) -> bool {
        if self.state == AttemptState::Disabled {
            if let Some(until) = self.disabled_until {
                if now >= until {
                    self.disabled_until = None;
                    self.submitted = false;
                    self.state = AttemptState::Armed;
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    #[test]
    fn one_submit_per_attempt_success() {
        let t0 = Instant::now();
        let mut m = AttemptStateMachine::new();
        assert_eq!(
            m.handle(AttemptEvent::Start, t0),
            TransitionResult::Changed {
                from: AttemptState::Armed,
                to: AttemptState::Searching
            }
        );
        let _ = m.handle(AttemptEvent::FaceSeen, t0);
        let _ = m.handle(AttemptEvent::SubmitSuccess, t0);
        assert_eq!(m.state(), AttemptState::Verified);
        assert_eq!(
            m.handle(AttemptEvent::SubmitSuccess, t0),
            TransitionResult::Rejected(AttemptError::AlreadySubmitted)
        );
        assert_eq!(
            m.handle(AttemptEvent::SubmitFailure(DenyReason::PadRejected), t0),
            TransitionResult::Rejected(AttemptError::AlreadySubmitted)
        );
        assert_eq!(m.state(), AttemptState::Verified);
    }

    #[test]
    fn one_submit_per_attempt_failure_and_fallback() {
        let t0 = Instant::now();
        let mut m = AttemptStateMachine::new();
        let _ = m.handle(AttemptEvent::Start, t0);
        let _ = m.handle(AttemptEvent::SubmitFailure(DenyReason::NoFace), t0);
        assert_eq!(m.state(), AttemptState::Denied);
        assert_eq!(m.failures_this_lock(), 1);
        let _ = m.handle(AttemptEvent::Cancel, t0);
        assert_eq!(m.state(), AttemptState::Armed);
        assert!(m.can_submit());

        for _ in 0..2 {
            let _ = m.handle(AttemptEvent::Start, t0);
            let _ = m.handle(AttemptEvent::SubmitFailure(DenyReason::LowSimilarity), t0);
        }
        assert_eq!(m.state(), AttemptState::Fallback);
        assert_eq!(m.failures_this_lock(), 3);
        assert!(!m.can_submit());
        assert_eq!(
            m.handle(AttemptEvent::SubmitSuccess, t0),
            TransitionResult::Rejected(AttemptError::AlreadySubmitted)
        );

        let _ = m.handle(AttemptEvent::NewLock, t0);
        assert_eq!(m.state(), AttemptState::Armed);
        assert_eq!(m.failures_this_lock(), 0);
    }

    #[test]
    fn cooldown_expires_on_tick() {
        let t0 = Instant::now();
        let mut m = AttemptStateMachine::new();
        let _ = m.handle(AttemptEvent::Cooldown(StdDuration::from_secs(60)), t0);
        assert_eq!(m.state(), AttemptState::Disabled);
        assert_eq!(m.disabled_until(), Some(t0 + StdDuration::from_secs(60)));
        assert_eq!(m.handle(AttemptEvent::Tick(t0), t0), TransitionResult::Ignored);
        assert_eq!(
            m.handle(AttemptEvent::Tick(t0 + StdDuration::from_secs(61)), t0),
            TransitionResult::Changed {
                from: AttemptState::Disabled,
                to: AttemptState::Armed
            }
        );
        assert_eq!(m.state(), AttemptState::Armed);
        assert!(m.can_submit());
    }

    #[test]
    fn service_down_fails_open_and_recovers() {
        let t0 = Instant::now();
        let mut m = AttemptStateMachine::new();
        let _ = m.handle(AttemptEvent::ServiceDown, t0);
        assert_eq!(m.state(), AttemptState::ServiceDown);
        assert_eq!(m.handle(AttemptEvent::Start, t0), TransitionResult::Ignored);
        let _ = m.handle(AttemptEvent::Recovered, t0);
        assert_eq!(m.state(), AttemptState::Armed);
    }

    #[test]
    fn all_event_state_pairs_are_total() {
        let t0 = Instant::now();
        let events = [
            AttemptEvent::Start,
            AttemptEvent::FaceSeen,
            AttemptEvent::NeedCloser,
            AttemptEvent::TooDark,
            AttemptEvent::LookAtScreen,
            AttemptEvent::ChallengeRequired,
            AttemptEvent::SubmitSuccess,
            AttemptEvent::SubmitFailure(DenyReason::NoFace),
            AttemptEvent::Cancel,
            AttemptEvent::Cooldown(StdDuration::from_secs(1)),
            AttemptEvent::ServiceDown,
            AttemptEvent::Recovered,
            AttemptEvent::NewLock,
            AttemptEvent::Reenable,
            AttemptEvent::Tick(t0),
        ];
        // Drive the machine through many random-looking sequences; the only
        // requirement is that nothing panics and the state stays valid.
        let mut m = AttemptStateMachine::new();
        for round in 0..events.len() * 4 {
            for (i, event) in events.iter().enumerate() {
                let _ = m.handle(*event, t0 + StdDuration::from_secs((round + i) as u64));
                assert!(m.failures_this_lock() <= MAX_FACE_ATTEMPTS_PER_LOCK);
            }
        }
    }
}
