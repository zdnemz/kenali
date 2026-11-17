//! Host-runnable FR-13 screening tests: comparable-size second faces reject
//! the attempt; small background faces are ignored. No camera or model is
//! opened here — all four model seams are local stubs.

use kenali_core::frame::Frame;
use kenali_core::secret::Embedding;
use kenali_core::types::DenyReason;
use kenali_service::pipeline::{
    is_comparable_second_face, FrameDecision, VisionPipeline, MULTI_FACE_MIN_RATIO,
};
use kenali_vision::traits::{Aligner, Detection, Detector, Embedder, PadClassifier};
use kenali_vision::VisionError;

/// Detector stub returning a caller-supplied list of detections.
struct StubDetector(Vec<Detection>);

impl Detector for StubDetector {
    fn detect(&mut self, _frame: &Frame) -> Result<Vec<Detection>, VisionError> {
        Ok(self.0.clone())
    }
}

struct StubAligner;
impl Aligner for StubAligner {
    fn align(
        &mut self,
        _frame: &Frame,
        _landmarks: &[(f32, f32); 5],
    ) -> Result<Vec<u8>, VisionError> {
        Ok(vec![0; 12])
    }
}

struct StubEmbedder;
impl Embedder for StubEmbedder {
    fn embed(&mut self, _aligned_rgb: &[u8]) -> Result<Embedding, VisionError> {
        Ok(Embedding::new(vec![1.0, 0.0]))
    }
}

struct StubPad;
impl PadClassifier for StubPad {
    fn pad_score(&mut self, _face_crop_rgb: &[u8]) -> Result<f32, VisionError> {
        Ok(0.9)
    }
}

fn detection(x: f32, y: f32, w: f32, h: f32) -> Detection {
    Detection {
        score: 0.5,
        bbox: [x, y, w, h],
        landmarks: [(0.0, 0.0); 5],
    }
}

fn frame() -> Frame {
    Frame::new(1, 1, vec![0, 0, 0]).expect("1x1 RGB frame is valid")
}

fn pipeline(detections: Vec<Detection>) -> VisionPipeline {
    let mut p = VisionPipeline::new(
        Box::new(StubDetector(detections)),
        Box::new(StubAligner),
        Box::new(StubEmbedder),
        Box::new(StubPad),
    );
    assert!(p.set_threshold(0.2));
    p.set_enrolled(Embedding::new(vec![1.0, 0.0]));
    p
}

#[test]
fn threshold_constant_is_conservative_default() {
    assert_eq!(MULTI_FACE_MIN_RATIO, 0.5);
}

#[test]
fn exact_threshold_counts_as_comparable() {
    let primary = detection(0.0, 0.0, 1.0, 1.0);
    let equal = detection(0.0, 0.0, 1.0, 1.0);
    let at_threshold = detection(0.0, 0.0, 0.5, 0.5);

    assert!(is_comparable_second_face(&primary, &equal, MULTI_FACE_MIN_RATIO));
    assert!(is_comparable_second_face(
        &primary,
        &at_threshold,
        MULTI_FACE_MIN_RATIO
    ));
}

#[test]
fn comparable_second_face_rejects() {
    let mut p = pipeline(vec![
        detection(0.0, 0.0, 1.0, 1.0),
        detection(0.4, 0.4, 0.8, 0.8),
    ]);
    assert_eq!(
        p.process_frame(&frame()),
        Ok(FrameDecision::Reject(DenyReason::MultipleFaces))
    );
}

#[test]
fn small_face_on_width_is_ignored() {
    let mut p = pipeline(vec![
        detection(0.0, 0.0, 1.0, 1.0),
        detection(0.5, 0.5, 0.49, 0.9),
    ]);
    assert_eq!(p.process_frame(&frame()), Ok(FrameDecision::Pending));
}

#[test]
fn small_face_on_height_is_ignored() {
    let mut p = pipeline(vec![
        detection(0.0, 0.0, 1.0, 1.0),
        detection(0.5, 0.5, 0.9, 0.49),
    ]);
    assert_eq!(p.process_frame(&frame()), Ok(FrameDecision::Pending));
}

#[test]
fn helper_rejects_non_finite_and_invalid_primary_dimensions() {
    let primary = detection(0.0, 0.0, 1.0, 1.0);
    assert!(!is_comparable_second_face(
        &primary,
        &detection(0.0, 0.0, f32::NAN, 1.0),
        MULTI_FACE_MIN_RATIO
    ));
    assert!(!is_comparable_second_face(
        &detection(0.0, 0.0, 0.0, 1.0),
        &detection(0.0, 0.0, 1.0, 1.0),
        MULTI_FACE_MIN_RATIO
    ));
    assert!(!is_comparable_second_face(
        &primary,
        &detection(0.0, 0.0, 1.0, 1.0),
        f32::NAN
    ));
}

#[test]
fn zero_and_one_face_behaviour_is_unchanged() {
    let mut none = pipeline(Vec::new());
    assert_eq!(
        none.process_frame(&frame()),
        Ok(FrameDecision::Reject(DenyReason::NoFace))
    );

    let mut one = pipeline(vec![detection(0.0, 0.0, 1.0, 1.0)]);
    assert_eq!(one.process_frame(&frame()), Ok(FrameDecision::Pending));
    assert_eq!(one.process_frame(&frame()), Ok(FrameDecision::Pending));
    assert_eq!(one.process_frame(&frame()), Ok(FrameDecision::Accept));
}


