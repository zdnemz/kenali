//! Vision pipeline: wires the four merged model seams into one frame decision
//! and fuses per-frame outcomes through the merged
//! [`kenali_vision::decision::TemporalWindow`].
//!
//! The match threshold is caller-supplied and calibrated — this module never
//! hardcodes a tutorial default. Until a calibration threshold and an enrolled
//! embedding are supplied, processing refuses to unlock (fails closed on face
//! while leaving the password tile available at the policy layer).

use kenali_core::frame::Frame;
use kenali_core::score::cosine;
use kenali_core::secret::Embedding;
use kenali_core::types::DenyReason;
use kenali_vision::decision::{Decision, TemporalWindow};
use kenali_vision::traits::{Aligner, Detection, Detector, Embedder, PadClassifier};
use kenali_vision::VisionError;

/// Consecutive qualifying frames required before the window accepts.
const DEFAULT_REQUIRED_FRAMES: usize = 3;
/// Bounded number of recent outcomes retained by the window.
const DEFAULT_WINDOW_CAPACITY: usize = 8;

/// Conservative default fraction of the primary face's bbox dimensions that a
/// second face must reach on *both* axes before it counts as a comparable face
/// (FR-13). Smaller background faces are ignored. Public so host tests can
/// exercise the threshold exactly.
pub const MULTI_FACE_MIN_RATIO: f32 = 0.5;

/// A second face is comparable only when its bbox is at least `min_ratio` of
/// the primary's width *and* at least `min_ratio` of its height. A face that
/// is smaller on either axis is a background face and must be ignored.
///
/// Detector bboxes are `(x, y, w, h)` with positive dimensions. Non-finite
/// dimensions and non-positive primaries are treated as non-comparable so a
/// malformed detection can never reject an attempt spuriously.
pub fn is_comparable_second_face(
    primary: &Detection,
    other: &Detection,
    min_ratio: f32,
) -> bool {
    let (pw, ph) = (primary.bbox[2], primary.bbox[3]);
    let (ow, oh) = (other.bbox[2], other.bbox[3]);
    if !pw.is_finite()
        || !ph.is_finite()
        || !ow.is_finite()
        || !oh.is_finite()
        || !min_ratio.is_finite()
        || pw <= 0.0
        || ph <= 0.0
    {
        return false;
    }
    ow >= pw * min_ratio && oh >= ph * min_ratio
}

/// Outcome of one processed frame. Carries no score to any UI or log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDecision {
    Pending,
    Accept,
    Reject(DenyReason),
}

/// Owns the four model seams and the temporal decision window.
pub struct VisionPipeline {
    detector: Box<dyn Detector>,
    aligner: Box<dyn Aligner>,
    embedder: Box<dyn Embedder>,
    pad: Box<dyn PadClassifier>,
    window: TemporalWindow,
    threshold: Option<f32>,
    pad_threshold: f32,
    enrolled: Option<Embedding>,
}

impl VisionPipeline {
    pub fn new(
        detector: Box<dyn Detector>,
        aligner: Box<dyn Aligner>,
        embedder: Box<dyn Embedder>,
        pad: Box<dyn PadClassifier>,
    ) -> Self {
        Self {
            detector,
            aligner,
            embedder,
            pad,
            window: TemporalWindow::new(DEFAULT_WINDOW_CAPACITY, DEFAULT_REQUIRED_FRAMES),
            threshold: None,
            pad_threshold: 0.5,
            enrolled: None,
        }
    }

    /// Install the calibrated cosine threshold. Non-finite values are ignored,
    /// so the pipeline stays uncalibrated rather than silently accepting.
    pub fn set_threshold(&mut self, threshold: f32) -> bool {
        if threshold.is_finite() {
            self.threshold = Some(threshold);
            self.window.clear();
            true
        } else {
            false
        }
    }

    pub fn threshold(&self) -> Option<f32> {
        self.threshold
    }

    /// Install the calibrated passive-PAD real-probability threshold.
    pub fn set_pad_threshold(&mut self, threshold: f32) -> bool {
        if threshold.is_finite() {
            self.pad_threshold = threshold;
            self.window.clear();
            true
        } else {
            false
        }
    }

    pub fn set_enrolled(&mut self, embedding: Embedding) {
        self.enrolled = Some(embedding);
        self.window.clear();
    }

    pub fn clear_enrolled(&mut self) {
        self.enrolled = None;
        self.window.clear();
    }

    /// Reset all temporal state (new attempt, cancel or disconnect).
    pub fn reset(&mut self) {
        self.window.clear();
    }

    /// Detect → align → embed → score → PAD → temporal fuse for one frame.
    pub fn process_frame(&mut self, frame: &Frame) -> Result<FrameDecision, VisionError> {
        let (threshold, enrolled) = match (self.threshold, self.enrolled.as_ref()) {
            (Some(t), Some(e)) => (t, e),
            _ => return Err(VisionError::BadInput("pipeline is not calibrated")),
        };

        let detections = self.detector.detect(frame)?;
        if detections.is_empty() {
            self.window.push(false);
            return Ok(FrameDecision::Reject(DenyReason::NoFace));
        }
        // FR-13: only a comparable-size second face rejects. Small background
        // faces are ignored; processing continues with the largest face.
        // Select the largest face by bbox area so ordering of the detector's
        // output cannot change the primary. Deterministic even for NaN areas.
        let mut primary_idx = 0usize;
        for (idx, d) in detections.iter().enumerate().skip(1) {
            let area = |det: &Detection| det.bbox[2].max(0.0) * det.bbox[3].max(0.0);
            if area(d) > area(&detections[primary_idx]) {
                primary_idx = idx;
            }
        }
        let primary = &detections[primary_idx];
        // FR-13 compares the primary against *other* detections only. Skipping
        // the primary index is essential: a face always compares comparable to
        // itself, so including it would reject every non-empty frame.
        let comparable = detections.iter().enumerate().any(|(idx, other)| {
            idx != primary_idx && is_comparable_second_face(primary, other, MULTI_FACE_MIN_RATIO)
        });
        if comparable {
            self.window.push(false);
            return Ok(FrameDecision::Reject(DenyReason::MultipleFaces));
        }

        let best = *primary;
        let aligned = self.aligner.align(frame, &best.landmarks)?;
        let embedding = self.embedder.embed(&aligned)?;
        let similarity = cosine(&embedding, enrolled);
        let pad_real = self.pad.pad_score(&aligned)?;

        let matched = similarity.is_finite() && similarity >= threshold;
        let pad_ok = pad_real.is_finite() && pad_real >= self.pad_threshold;
        let qualifying = matched && pad_ok;

        let decision = self.window.push(qualifying);
        let outcome = match decision {
            Decision::Accept => {
                self.window.clear();
                FrameDecision::Accept
            }
            Decision::Pending => FrameDecision::Pending,
            Decision::Reject => FrameDecision::Reject(if !pad_ok {
                DenyReason::PadRejected
            } else {
                DenyReason::LowSimilarity
            }),
        };
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kenali_core::secret::Embedding;
    use kenali_vision::traits::Detection;

    struct StubDetector {
        count: usize,
    }
    impl Detector for StubDetector {
        fn detect(&mut self, _frame: &Frame) -> Result<Vec<Detection>, VisionError> {
            if self.count == 0 {
                Ok(Vec::new())
            } else {
                Ok(vec![Detection {
                    score: 0.9,
                    bbox: [0.0, 0.0, 1.0, 1.0],
                    landmarks: [(0.0, 0.0); 5],
                }])
            }
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

    struct StubPad {
        real: bool,
    }
    impl PadClassifier for StubPad {
        fn pad_score(&mut self, _face_crop_rgb: &[u8]) -> Result<f32, VisionError> {
            Ok(if self.real { 0.9 } else { 0.1 })
        }
    }

    fn pipeline(count: usize, real: bool) -> VisionPipeline {
        VisionPipeline::new(
            Box::new(StubDetector { count }),
            Box::new(StubAligner),
            Box::new(StubEmbedder),
            Box::new(StubPad { real }),
        )
    }

    fn frame() -> Frame {
        match Frame::new(1, 1, vec![0, 0, 0]) {
            Ok(f) => f,
            Err(_) => Frame { width: 1, height: 1, rgb: vec![0, 0, 0] },
        }
    }

    #[test]
    fn uncalibrated_pipeline_refuses_to_unlock() {
        let mut pipeline = pipeline(1, true);
        match pipeline.process_frame(&frame()) {
            Err(VisionError::BadInput(_)) => {}
            _ => assert!(false, "must refuse before calibration"),
        }
        assert!(pipeline.threshold().is_none());
    }

    #[test]
    fn requires_k_consecutive_qualifying_frames() {
        let mut pipeline = pipeline(1, true);
        assert!(pipeline.set_threshold(0.2));
        pipeline.set_enrolled(Embedding::new(vec![1.0, 0.0]));
        assert_eq!(pipeline.process_frame(&frame()), Ok(FrameDecision::Pending));
        assert_eq!(pipeline.process_frame(&frame()), Ok(FrameDecision::Pending));
        assert_eq!(pipeline.process_frame(&frame()), Ok(FrameDecision::Accept));
        assert_eq!(pipeline.process_frame(&frame()), Ok(FrameDecision::Pending));
    }

    #[test]
    fn pad_failure_rejects_and_resets_streak() {
        let mut pipeline = pipeline(1, false);
        assert!(pipeline.set_threshold(0.2));
        pipeline.set_enrolled(Embedding::new(vec![1.0, 0.0]));
        assert_eq!(
            pipeline.process_frame(&frame()),
            Ok(FrameDecision::Reject(DenyReason::PadRejected))
        );
    }

    #[test]
    fn no_face_is_rejected() {
        let mut pipeline = pipeline(0, true);
        assert!(pipeline.set_threshold(0.2));
        pipeline.set_enrolled(Embedding::new(vec![1.0, 0.0]));
        assert_eq!(pipeline.process_frame(&frame()), Ok(FrameDecision::Reject(DenyReason::NoFace)));
    }

    #[test]
    fn non_finite_threshold_is_ignored() {
        let mut pipeline = pipeline(1, true);
        assert!(!pipeline.set_threshold(f32::NAN));
        assert!(pipeline.threshold().is_none());
    }
}
