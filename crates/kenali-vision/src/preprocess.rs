//! HWC → NCHW conversion for raw byte frames.
//!
//! YuNet and SFace take raw 0–255 values (not 0–1 normalised), so the byte is
//! widened to `f32` unchanged. This runs on the hot path: one allocation, no
//! per-pixel allocation, and no panics on hostile dimensions.

/// Convert a tightly packed HWC byte buffer into channel-major `f32`.
///
/// Output index for channel `c`, row `y`, column `x` is `c * w * h + y * w + x`.
/// Returns an empty vector when `src.len() != w * h * channels`, when any
/// dimension overflows, or when `w * h * channels == 0`.
pub fn to_nchw(src: &[u8], w: usize, h: usize, channels: usize) -> Vec<f32> {
    let Some(len) = w.checked_mul(h).and_then(|n| n.checked_mul(channels)) else {
        return Vec::new();
    };
    if len == 0 || src.len() != len {
        return Vec::new();
    }

    let mut out = vec![0.0f32; len];
    let plane = w * h;
    for c in 0..channels {
        let mut o = c * plane;
        for i in 0..plane {
            out[o] = src[i * channels + c] as f32;
            o += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn to_nchw_2x2x3_exact_ordering() {
        // HWC row-major: (0,0) red/green/blue, (0,1), (1,0), (1,1).
        let src = [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120];
        let got = to_nchw(&src, 2, 2, 3);
        assert_eq!(
            got,
            vec![
                10.0, 40.0, 70.0, 100.0, // R plane
                20.0, 50.0, 80.0, 110.0, // G plane
                30.0, 60.0, 90.0, 120.0, // B plane
            ]
        );
    }

    #[test]
    fn to_nchw_rejects_bad_length_and_overflow_without_panicking() {
        assert!(to_nchw(&[0, 1, 2], 2, 2, 3).is_empty());
        assert!(to_nchw(&[0; 12], 0, 2, 3).is_empty());
        assert!(to_nchw(&[], 1, 1, 0).is_empty());
        assert!(to_nchw(&[], usize::MAX, usize::MAX, 3).is_empty());
    }

    proptest::proptest! {
        #[test]
        fn to_nchw_matches_definition(
            w in 1usize..8,
            h in 1usize..8,
            channels in 1usize..4,
            bytes in proptest::collection::vec(0u8..=255, 1..=8 * 8 * 3),
        ) {
            let need = w * h * channels;
            if bytes.len() == need {
                let out = to_nchw(&bytes, w, h, channels);
                prop_assert_eq!(out.len(), need);
                for c in 0..channels {
                    for y in 0..h {
                        for x in 0..w {
                            prop_assert_eq!(
                                out[c * w * h + y * w + x],
                                bytes[(y * w + x) * channels + c] as f32
                            );
                        }
                    }
                }
            } else {
                prop_assert!(to_nchw(&bytes, w, h, channels).is_empty());
            }
        }
    }
}
