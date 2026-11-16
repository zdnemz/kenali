//! Model-swap seams. Every trait is object-safe and free of ORT types so a
//! backend can be replaced (or faked in tests) without touching callers.
//! No model code lives here in Wave 1.

use kenali_core::frame::Frame;
use kenali_core::secret::Embedding;

use crate::VisionError;

/// One detected face. `bbox` is `(x, y, w, h)`; `landmarks` are ordered
/// left eye, right eye, nose tip, left mouth corner, right mouth corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Detection {
    pub score: f32,
    pub bbox: [f32; 4],
    pub landmarks: [(f32, f32); 5],
}

/// Face detector (YuNet in production). Input is a raw RGB frame; detections
/// are returned sorted by descending score.
pub trait Detector {
    fn detect(&mut self, frame: &Frame) -> Result<Vec<Detection>, VisionError>;
}

/// Five-point alignment into a 112×112×3 RGB buffer (HWC, raw 0–255).
pub trait Aligner {
    fn align(
        &mut self,
        frame: &Frame,
        landmarks: &[(f32, f32); 5],
    ) -> Result<Vec<u8>, VisionError>;
}

/// Face embedder (SFace in production). Input is the 112×112×3 aligned buffer;
/// the returned embedding is L2-normalised by `Embedding::new`.
pub trait Embedder {
    fn embed(&mut self, aligned_rgb: &[u8]) -> Result<Embedding, VisionError>;
}

/// Passive presentation-attack classifier. Returns a real-probability in
/// `[0.0, 1.0]`; fusing it with the temporal decision is the caller's job, so
/// thresholds do not live here.
pub trait PadClassifier {
    fn pad_score(&mut self, face_crop_rgb: &[u8]) -> Result<f32, VisionError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Compile-time proof that all four seams stay object-safe and can be
    // implemented without any ORT types.
    struct Stub;
    impl Detector for Stub {
        fn detect(&mut self, _frame: &Frame) -> Result<Vec<Detection>, VisionError> {
            Ok(Vec::new())
        }
    }
    impl Aligner for Stub {
        fn align(
            &mut self,
            _frame: &Frame,
            _landmarks: &[(f32, f32); 5],
        ) -> Result<Vec<u8>, VisionError> {
            Ok(vec![0; 112 * 112 * 3])
        }
    }
    impl Embedder for Stub {
        fn embed(&mut self, _aligned_rgb: &[u8]) -> Result<Embedding, VisionError> {
            Ok(Embedding::new(Vec::new()))
        }
    }
    impl PadClassifier for Stub {
        fn pad_score(&mut self, _face_crop_rgb: &[u8]) -> Result<f32, VisionError> {
            Ok(0.5)
        }
    }

    #[test]
    fn seams_are_object_safe() {
        let frame = Frame::new(1, 1, vec![0, 0, 0]).expect("valid frame");

        let mut detector = Stub;
        let detector: &mut dyn Detector = &mut detector;
        assert!(detector.detect(&frame).expect("detect").is_empty());

        let mut aligner = Stub;
        let aligner: &mut dyn Aligner = &mut aligner;
        assert_eq!(
            aligner
                .align(&frame, &[(0.0, 0.0); 5])
                .expect("align")
                .len(),
            112 * 112 * 3
        );

        let mut embedder = Stub;
        let embedder: &mut dyn Embedder = &mut embedder;
        assert!(embedder.embed(&[]).expect("embed").is_empty());

        let mut pad = Stub;
        let pad: &mut dyn PadClassifier = &mut pad;
        assert_eq!(pad.pad_score(&[]).expect("pad"), 0.5);
    }
}
