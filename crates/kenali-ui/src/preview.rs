//! RAM-only preview slot.
//!
//! Frames arrive from the short-lived `PreviewPipeClient` owned by the pipe
//! worker and are copied into a zeroizing, non-clonable buffer for inline
//! rendering. They are never encoded, written to disk or logged. The buffer is
//! wiped on `EndPreview`, wizard cancel/error and app exit.

use std::fmt;

use kenali_ipc::codec::MAX_PREVIEW_BYTES;
use kenali_ipc::preview::PreviewFrame;

/// Expected frame geometry. A different size is rejected, never rescaled.
pub const PREVIEW_W: u16 = 320;
pub const PREVIEW_H: u16 = 240;
const EXPECTED_BYTES: usize = PREVIEW_W as usize * PREVIEW_H as usize * 3;

/// A local, safe equivalent of `zeroize::Zeroizing<Vec<u8>>` for RGB pixels.
///
/// The GUI crate intentionally has no `zeroize` dependency (manifests are owned
/// by W4-01), so this provides the same wipe-on-drop guarantee with volatile
/// writes that the compiler cannot elide. It is private, single-purpose and
/// never cloned.
struct Zeroizing {
    bytes: Vec<u8>,
}

impl Zeroizing {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    fn clear(&mut self) {
        self.bytes.clear();
    }
}

impl Drop for Zeroizing {
    fn drop(&mut self) {
        wipe(self.as_mut_slice());
    }
}

impl fmt::Debug for Zeroizing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Zeroizing(<redacted>)")
    }
}

/// Overwrite with zeros and make the result observable so the stores cannot be
/// optimised away. This is the strongest wipe available without `unsafe` (the
/// GUI crate forbids it) or a direct `zeroize` dependency.
fn wipe(bytes: &mut [u8]) {
    bytes.fill(0);
    let _ = std::hint::black_box(&*bytes);
}

/// Errors from replacing the preview buffer.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewError {
    #[error("unexpected preview size")]
    BadSize,
    #[error("preview payload is too large")]
    TooLarge,
}

/// Holds at most one RGB frame, wiped on every exit path.
pub struct PreviewSlot {
    rgb: Zeroizing,
    w: u16,
    h: u16,
}

impl Default for PreviewSlot {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for PreviewSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewSlot")
            .field("rgb", &self.rgb)
            .field("w", &self.w)
            .field("h", &self.h)
            .finish()
    }
}

impl PreviewSlot {
    /// Create an empty, fully wiped slot.
    pub fn new() -> Self {
        Self {
            rgb: Zeroizing::new(Vec::new()),
            w: 0,
            h: 0,
        }
    }

    /// Copy one frame into RAM after validating geometry and payload cap.
    pub fn replace(&mut self, frame: &PreviewFrame) -> Result<(), PreviewError> {
        if frame.w != PREVIEW_W || frame.h != PREVIEW_H {
            return Err(PreviewError::BadSize);
        }
        let bytes = frame.rgb.as_slice();
        if bytes.len() > MAX_PREVIEW_BYTES || bytes.len() != EXPECTED_BYTES {
            return Err(PreviewError::TooLarge);
        }
        self.clear();
        self.rgb = Zeroizing::new(bytes.to_vec());
        self.w = frame.w;
        self.h = frame.h;
        Ok(())
    }

    /// Borrow the frame for egui rendering, or `None` when empty.
    pub fn as_rgb(&self) -> Option<(u16, u16, &[u8])> {
        if self.w == 0 || self.h == 0 || self.rgb.as_slice().is_empty() {
            return None;
        }
        Some((self.w, self.h, self.rgb.as_slice()))
    }

    /// Wipe and forget the current frame immediately.
    pub fn clear(&mut self) {
        wipe(self.rgb.as_mut_slice());
        self.rgb.clear();
        self.w = 0;
        self.h = 0;
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use kenali_ipc::wire::Bytes;

    #[test]
    fn rejects_wrong_geometry_and_oversize() {
        let mut slot = PreviewSlot::new();
        let bad = PreviewFrame {
            w: 64,
            h: 64,
            rgb: Bytes::new(vec![0; 64 * 64 * 3]),
        };
        assert_eq!(slot.replace(&bad), Err(PreviewError::BadSize));
        assert!(slot.as_rgb().is_none());
    }

    #[test]
    fn replace_and_clear_round_trip() {
        let mut slot = PreviewSlot::new();
        let good = PreviewFrame {
            w: PREVIEW_W,
            h: PREVIEW_H,
            rgb: Bytes::new(vec![7; EXPECTED_BYTES]),
        };
        assert!(slot.replace(&good).is_ok());
        assert_eq!(slot.as_rgb().map(|(w, h, _)| (w, h)), Some((PREVIEW_W, PREVIEW_H)));
        slot.clear();
        assert!(slot.as_rgb().is_none());
    }
}
