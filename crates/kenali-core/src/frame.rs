//! Camera seam. The core crate declares the trait; `kenali-camera` implements
//! it (Media Foundation in production, file/synthetic sources in tests).
//! No raw pixel bytes are ever logged and frames are never persisted by this
//! crate.

use std::time::Instant;

/// Identity of the physical camera captured at enrolment (SEC-16).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CameraPin {
    /// Device interface symbolic link — the stable identity on Windows.
    pub symbolic_link: String,
    /// Hardware ID reported by the device, for diagnostics only.
    pub hardware_id: String,
}

impl CameraPin {
    pub fn new(symbolic_link: impl Into<String>, hardware_id: impl Into<String>) -> Self {
        Self {
            symbolic_link: symbolic_link.into(),
            hardware_id: hardware_id.into(),
        }
    }
}

/// One RGB frame in RAM. Debug prints dimensions only — never pixel bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u16,
    pub height: u16,
    pub rgb: Vec<u8>,
}

impl core::fmt::Debug for Frame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("rgb", &"<redacted>")
            .finish()
    }
}

impl Frame {
    /// Build a frame, rejecting a buffer whose length does not match the
    /// dimensions. No panics, no allocation from an untrusted length here.
    pub fn new(width: u16, height: u16, rgb: Vec<u8>) -> Result<Self, CaptureError> {
        let expected = usize::from(width)
            .checked_mul(usize::from(height))
            .and_then(|n| n.checked_mul(3))
            .ok_or(CaptureError::FrameTooLarge)?;
        if rgb.len() != expected {
            return Err(CaptureError::BadFrameLength);
        }
        Ok(Self {
            width,
            height,
            rgb,
        })
    }
}

/// Capture failures. Generic wording only — no device secrets or paths in the
/// messages beyond what is needed for an Event Log entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    #[error("camera open timed out")]
    Timeout,
    #[error("pinned camera is not available")]
    DeviceUnavailable,
    #[error("camera identity does not match the enrolled pin")]
    PinMismatch,
    #[error("capture window has ended")]
    Closed,
    #[error("frame buffer length does not match dimensions")]
    BadFrameLength,
    #[error("frame dimensions overflow")]
    FrameTooLarge,
    #[error("capture failed")]
    Other,
}

/// The frame-source seam used by the service and by replay/synthetic tests.
pub trait FrameSource: Send {
    fn open(&mut self, pin: &CameraPin, deadline: Instant) -> Result<(), CaptureError>;
    fn next_frame(&mut self, deadline: Instant) -> Result<Frame, CaptureError>;
    fn close(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSource {
        opened: bool,
        pin: Option<CameraPin>,
    }

    impl FrameSource for FakeSource {
        fn open(&mut self, pin: &CameraPin, _deadline: Instant) -> Result<(), CaptureError> {
            self.opened = true;
            self.pin = Some(pin.clone());
            Ok(())
        }
        fn next_frame(&mut self, _deadline: Instant) -> Result<Frame, CaptureError> {
            if !self.opened {
                return Err(CaptureError::Closed);
            }
            Frame::new(1, 1, vec![7, 8, 9])
        }
        fn close(&mut self) {
            self.opened = false;
            self.pin = None;
        }
    }

    #[test]
    fn frame_debug_hides_pixels() {
        let frame = Frame::new(2, 2, vec![0; 12]).expect("valid frame");
        let dbg = format!("{:?}", frame);
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains('0'));
    }

    #[test]
    fn frame_rejects_bad_length() {
        assert_eq!(Frame::new(1, 1, vec![0, 1]), Err(CaptureError::BadFrameLength));
    }

    #[test]
    fn frame_source_seam_works() {
        let mut src = FakeSource {
            opened: false,
            pin: None,
        };
        let now = Instant::now();
        let pin = CameraPin::new("symbolic-link", "hw-id");
        assert_eq!(src.next_frame(now), Err(CaptureError::Closed));
        src.open(&pin, now).expect("open");
        assert_eq!(src.pin, Some(pin));
        assert_eq!(src.next_frame(now).expect("frame").rgb, vec![7, 8, 9]);
        src.close();
        assert_eq!(src.next_frame(now), Err(CaptureError::Closed));
    }
}
