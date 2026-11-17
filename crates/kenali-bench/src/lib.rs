#![forbid(unsafe_code)]

//! Genuine benchmarking and calibration harness for Kenali.
//!
//! No model weights, camera, TPM, network or async runtime live here: callers
//! supply score vectors or closures, and every timing goes through
//! [`std::time::Instant`]. Models land in W3-07; this crate only measures.
//!
//! Public surface (pinned by W3-04 shared contract):
//! - [`latency`]: `measure_warm`, `measure_cold`, `summarize`
//! - [`throughput`]: `per_frame_ms`
//! - [`calibration`]: `choose_threshold`, `roc_curve`

pub mod calibration;
pub mod latency;
pub mod throughput;
