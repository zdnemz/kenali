//! RAM-only preview frame slot.
//!
//! The preview pipe exists only during enrolment/calibration windows. Frames
//! live in a single `Zeroizing` buffer that is replaced on every update and
//! wiped on every exit path (end, cancel, disconnect, drop). Nothing is written
//! to disk and no pixel byte is ever logged.

use kenali_ipc::codec::MAX_PREVIEW_BYTES;
use kenali_ipc::preview::PreviewFrame;
use kenali_ipc::wire::Bytes;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewError {
    #[error("preview window is not open")]
    Closed,
    #[error("preview frame dimensions are invalid")]
    BadDimensions,
    #[error("preview frame exceeds the payload cap")]
    TooLarge,
    #[error("preview frame buffer length does not match its dimensions")]
    BadFrameLength,
}

/// One zeroizing frame slot. `begin`/`end` bracket a management window.
pub struct PreviewSlot {
    width: u16,
    height: u16,
    rgb: Zeroizing<Vec<u8>>,
    active: bool,
}

impl PreviewSlot {
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            rgb: Zeroizing::new(Vec::new()),
            active: false,
        }
    }

    /// Open a preview window, discarding anything left in the slot.
    pub fn begin(&mut self) {
        self.wipe();
        self.active = true;
    }

    /// Close the window and zeroize the frame.
    pub fn end(&mut self) {
        self.wipe();
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Replace the slot with a validated frame. The previous buffer is dropped
    /// (and wiped) here; no raw frame is retained elsewhere.
    pub fn put(&mut self, frame: &PreviewFrame) -> Result<(), PreviewError> {
        if !self.active {
            return Err(PreviewError::Closed);
        }
        if frame.w == 0 || frame.h == 0 {
            return Err(PreviewError::BadDimensions);
        }
        let expected = usize::from(frame.w)
            .checked_mul(usize::from(frame.h))
            .and_then(|n| n.checked_mul(3))
            .ok_or(PreviewError::TooLarge)?;
        if frame.rgb.as_slice().len() != expected {
            return Err(PreviewError::BadFrameLength);
        }
        if expected > MAX_PREVIEW_BYTES {
            return Err(PreviewError::TooLarge);
        }

        let mut next = Zeroizing::new(Vec::with_capacity(expected));
        next.extend_from_slice(frame.rgb.as_slice());
        self.wrgb(next);
        self.width = frame.w;
        self.height = frame.h;
        Ok(())
    }

    /// Snapshot for transport. The returned bytes are a copy made for the wire
    /// only; the slot itself stays RAM-only.
    pub fn snapshot(&self) -> Option<PreviewFrame> {
        if !self.active || self.rgb.is_empty() {
            return None;
        }
        Some(PreviewFrame {
            w: self.width,
            h: self.height,
            rgb: Bytes::new(self.rgb.to_vec()),
        })
    }

    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }

    fn wrgb(&mut self, next: Zeroizing<Vec<u8>>) {
        self.rgb = next;
    }

    fn wipe(&mut self) {
        // Replacing the Zeroizing buffer drops (and wipes) the previous one.
        self.rgb = Zeroizing::new(Vec::new());
        self.width = 0;
        self.height = 0;
        self.active = false;
    }
}

impl Drop for PreviewSlot {
    fn drop(&mut self) {
        self.wipe();
    }
}

impl Default for PreviewSlot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u16, h: u16, len: usize) -> PreviewFrame {
        PreviewFrame {
            w,
            h,
            rgb: Bytes::new(vec![7u8; len]),
        }
    }

    #[test]
    fn put_and_snapshot_round_trip() {
        let mut slot = PreviewSlot::new();
        slot.begin();
        assert!(slot.put(&frame(2, 2, 12)).is_ok());
        assert_eq!(slot.dimensions(), (2, 2));
        match slot.snapshot() {
            Some(snap) => {
                assert_eq!((snap.w, snap.h), (2, 2));
                assert_eq!(snap.rgb.as_slice().len(), 12);
            }
            None => assert!(false, "snapshot must exist while active"),
        }
    }

    #[test]
    fn closed_slot_rejects_frames() {
        let mut slot = PreviewSlot::new();
        assert_eq!(slot.put(&frame(1, 1, 3)), Err(PreviewError::Closed));
        assert!(slot.snapshot().is_none());
    }

    #[test]
    fn bad_length_and_dimensions_are_rejected() {
        let mut slot = PreviewSlot::new();
        slot.begin();
        assert_eq!(slot.put(&frame(0, 1, 0)), Err(PreviewError::BadDimensions));
        assert_eq!(slot.put(&frame(1, 1, 4)), Err(PreviewError::BadFrameLength));
    }

    #[test]
    fn end_zeroizes_and_closes() {
        let mut slot = PreviewSlot::new();
        slot.begin();
        assert!(slot.put(&frame(1, 1, 3)).is_ok());
        slot.end();
        assert!(!slot.is_active());
        assert!(slot.snapshot().is_none());
        assert_eq!(slot.dimensions(), (0, 0));
    }
}
