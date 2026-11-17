//! Deterministic synthetic source for fuzzing and benchmarks.
//!
//! Produces a repeatable gradient/pattern without touching any camera or
//! filesystem. Host-runnable and safe on every target.

use std::time::Instant;

use kenali_core::frame::{CameraPin, CaptureError, Frame, FrameSource};

/// A deterministic frame generator implementing `FrameSource`.
pub struct SyntheticSource {
    width: u16,
    height: u16,
    counter: u64,
    opened: bool,
}

impl SyntheticSource {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            counter: 0,
            opened: false,
        }
    }

    /// Byte at `index` for the current counter value — a deterministic mix.
    fn byte_at(&self, index: usize) -> u8 {
        let v = (index as u64)
            .wrapping_mul(31)
            .wrapping_add(self.counter.wrapping_mul(17))
            .wrapping_add(7);
        (v & 0xff) as u8
    }
}

impl FrameSource for SyntheticSource {
    fn open(&mut self, _pin: &CameraPin, _deadline: Instant) -> Result<(), CaptureError> {
        if self.width == 0 || self.height == 0 {
            return Err(CaptureError::BadFrameLength);
        }
        self.counter = 0;
        self.opened = true;
        Ok(())
    }

    fn next_frame(&mut self, _deadline: Instant) -> Result<Frame, CaptureError> {
        if !self.opened {
            return Err(CaptureError::Closed);
        }
        let len = usize::from(self.width)
            .checked_mul(usize::from(self.height))
            .and_then(|n| n.checked_mul(3))
            .ok_or(CaptureError::FrameTooLarge)?;
        let mut rgb = Vec::with_capacity(len);
        for i in 0..len {
            rgb.push(self.byte_at(i));
        }
        self.counter = self.counter.wrapping_add(1);
        Frame::new(self.width, self.height, rgb)
    }

    fn close(&mut self) {
        self.opened = false;
        self.counter = 0;
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn synthetic_frames_are_deterministic() {
        let pin = CameraPin::new("synthetic", "synthetic");
        let mut a = SyntheticSource::new(4, 3);
        let mut b = SyntheticSource::new(4, 3);
        assert!(a.open(&pin, Instant::now()).is_ok());
        assert!(b.open(&pin, Instant::now()).is_ok());
        let f1 = a.next_frame(Instant::now()).expect("frame");
        let f2 = b.next_frame(Instant::now()).expect("frame");
        assert_eq!(f1, f2);
        assert_eq!(f1.rgb.len(), 4 * 3 * 3);
        let g1 = a.next_frame(Instant::now()).expect("frame");
        assert_ne!(f1, g1);
    }

    #[test]
    fn synthetic_requires_open_and_nonzero_size() {
        let pin = CameraPin::new("synthetic", "synthetic");
        let mut zero = SyntheticSource::new(0, 0);
        assert_eq!(zero.open(&pin, Instant::now()), Err(CaptureError::BadFrameLength));
        let mut src = SyntheticSource::new(2, 2);
        assert_eq!(src.next_frame(Instant::now()), Err(CaptureError::Closed));
        assert!(src.open(&pin, Instant::now()).is_ok());
        src.close();
        assert_eq!(src.next_frame(Instant::now()), Err(CaptureError::Closed));
    }
}
