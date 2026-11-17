//! Throughput arithmetic for frame pipelines (NFR-02: <= 40 ms per frame).

use std::time::Duration;

/// Mean milliseconds per frame, amortised over `frames`.
///
/// Returns `None` when `frames == 0` (there is no frame count to divide by, so
/// a rate is undefined). Callers that need a total-only fallback can use
/// `per_frame_ms(total, frames).unwrap_or(0.0)`; this function keeps the edge
/// case explicit instead of silently reporting a fabricated rate.
pub fn per_frame_ms(total: Duration, frames: usize) -> Option<f64> {
    if frames == 0 {
        return None;
    }
    Some(total.as_secs_f64() * 1_000.0 / frames as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ms_per_frame() {
        let got = per_frame_ms(Duration::from_millis(40), 10).expect("frames > 0");
        assert!((got - 4.0).abs() < 1e-9, "got {got}");
    }

    #[test]
    fn one_second_over_100_frames() {
        let got = per_frame_ms(Duration::from_secs(1), 100).expect("frames > 0");
        assert!((got - 10.0).abs() < 1e-9, "got {got}");
    }

    #[test]
    fn zero_frames_returns_none_no_panic() {
        assert_eq!(per_frame_ms(Duration::from_millis(40), 0), None);
        assert_eq!(per_frame_ms(Duration::ZERO, 0), None);
    }

    #[test]
    fn zero_duration_with_frames_is_zero() {
        let got = per_frame_ms(Duration::ZERO, 7).expect("frames > 0");
        assert_eq!(got, 0.0);
    }

    #[test]
    fn sub_millisecond_totals() {
        let got = per_frame_ms(Duration::from_micros(500), 2).expect("frames > 0");
        assert!((got - 0.25).abs() < 1e-9, "got {got}");
    }
}
