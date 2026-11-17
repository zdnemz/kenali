#![forbid(unsafe_code)]

//! `kenali-vision` — detector/embedder/PAD traits and preprocessing.
//!
//! Wave 1 declares the crate boundary plus the geometry and decision seams.
//! Model inference (ONNX Runtime via `ort`) lands in a later wave; nothing in
//! this crate loads weights or makes network calls.

pub mod align;
pub mod decision;
pub mod onnx;
pub mod preprocess;
pub mod traits;

/// Errors shared by the vision seams.
///
/// Wording is deliberately generic: pixel bytes, embeddings and model details
/// never appear in a message (they may end up in an Event Log entry).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum VisionError {
    #[error("invalid vision input: {0}")]
    BadInput(&'static str),
    #[error("degenerate landmark geometry")]
    DegenerateLandmarks,
    #[error("singular affine transform")]
    SingularAffine,
    #[error("model backend error: {0}")]
    Backend(String),
}
