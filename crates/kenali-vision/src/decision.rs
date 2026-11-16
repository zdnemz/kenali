//! Temporal decision window.
//!
//! A single good frame is not enough to unlock: we require K consecutive
//! qualifying frames (match **and** passive PAD real), keep only the last N
//! outcomes, and reset the streak on the first bad frame. All state lives in
//! one bounded queue, so `clear`/`drain` leave nothing to leak into the next
//! attempt.

use std::collections::VecDeque;

/// Outcome of feeding one frame into the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Not enough consecutive qualifying frames yet.
    Pending,
    /// K consecutive qualifying frames reached — accept.
    Accept,
    /// This frame was bad; the streak was reset — reject.
    Reject,
}

/// Bounded window of per-frame match/PAD outcomes.
#[derive(Debug, Clone)]
pub struct TemporalWindow {
    capacity: usize,
    k: usize,
    recent: VecDeque<bool>,
}

impl TemporalWindow {
    /// Build a window holding at most `capacity` outcomes, requiring `k`
    /// consecutive qualifying frames to accept.
    ///
    /// Both values are floored at 1, and `capacity` is raised to at least `k`
    /// so a full streak always fits.
    pub fn new(capacity: usize, k: usize) -> Self {
        let k = k.max(1);
        let capacity = capacity.max(k).max(1);
        Self {
            capacity,
            k,
            recent: VecDeque::with_capacity(capacity),
        }
    }

    /// Feed one frame outcome. `true` means the frame matched *and* passed PAD.
    pub fn push(&mut self, qualifying: bool) -> Decision {
        if self.recent.len() == self.capacity {
            self.recent.pop_front();
        }
        self.recent.push_back(qualifying);

        if self.is_ready() {
            Decision::Accept
        } else if !qualifying {
            Decision::Reject
        } else {
            Decision::Pending
        }
    }

    /// Number of trailing qualifying frames.
    pub fn streak(&self) -> usize {
        self.recent.iter().rev().take_while(|&&q| q).count()
    }

    /// True once K consecutive qualifying frames are present.
    pub fn is_ready(&self) -> bool {
        self.streak() >= self.k
    }

    pub fn len(&self) -> usize {
        self.recent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recent.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn required(&self) -> usize {
        self.k
    }

    /// Drain all stored outcomes, leaving a fresh window (no state leak
    /// between attempts).
    pub fn drain(&mut self) -> std::collections::vec_deque::Drain<'_, bool> {
        self.recent.drain(..)
    }

    /// Zeroize-style reset: drop every stored outcome.
    pub fn clear(&mut self) {
        self.recent.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn k_consecutive_then_accept() {
        let mut w = TemporalWindow::new(5, 3);
        assert_eq!(w.push(true), Decision::Pending);
        assert_eq!(w.push(true), Decision::Pending);
        assert!(!w.is_ready());
        assert_eq!(w.push(true), Decision::Accept);
        assert!(w.is_ready());
        assert_eq!(w.streak(), 3);
    }

    #[test]
    fn bad_frame_resets() {
        let mut w = TemporalWindow::new(5, 3);
        assert_eq!(w.push(true), Decision::Pending);
        assert_eq!(w.push(true), Decision::Pending);
        assert_eq!(w.push(false), Decision::Reject);
        assert_eq!(w.streak(), 0);
        assert_eq!(w.push(true), Decision::Pending);
        assert_eq!(w.push(true), Decision::Pending);
        assert_eq!(w.push(true), Decision::Accept);
    }

    #[test]
    fn insufficient_frames_never_accept() {
        let mut w = TemporalWindow::new(8, 4);
        for _ in 0..3 {
            assert_eq!(w.push(true), Decision::Pending);
        }
        assert!(!w.is_ready());
        assert_eq!(w.push(false), Decision::Reject);
        for _ in 0..3 {
            assert_eq!(w.push(true), Decision::Pending);
        }
        assert!(!w.is_ready());
    }

    #[test]
    fn window_is_bounded() {
        let mut w = TemporalWindow::new(3, 3);
        for _ in 0..10 {
            let _ = w.push(true);
        }
        assert_eq!(w.len(), 3);
        assert_eq!(w.capacity(), 3);
    }

    #[test]
    fn capacity_never_smaller_than_required() {
        let w = TemporalWindow::new(1, 5);
        assert_eq!(w.capacity(), 5);
        assert_eq!(w.required(), 5);
    }

    #[test]
    fn clear_and_drain_leave_no_state() {
        let mut w = TemporalWindow::new(4, 2);
        assert_eq!(w.push(true), Decision::Pending);
        assert_eq!(w.push(true), Decision::Accept);
        w.clear();
        assert!(w.is_empty());
        assert_eq!(w.streak(), 0);
        assert_eq!(w.push(true), Decision::Pending);

        let drained: Vec<bool> = w.drain().collect();
        assert_eq!(drained, vec![true]);
        assert!(w.is_empty());
        assert_eq!(w.streak(), 0);
    }
}
