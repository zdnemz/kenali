//! Five-point alignment and affine warping.
//!
//! `similarity_transform` solves the least-squares similarity (rotation +
//! uniform scale + translation, no reflection) mapping five source landmarks
//! onto the 112×112 template. `warp_affine_112` applies that mapping by inverse
//! mapping with bilinear sampling. No `unsafe`, no ORT, no model weights.

use kenali_core::frame::Frame;

use crate::traits::Aligner;
use crate::VisionError;

/// Target landmarks in the 112×112 aligned face: left eye, right eye, nose
/// tip, left mouth corner, right mouth corner.
pub const ALIGN_POINTS_112: [(f32, f32); 5] = [
    (38.29, 51.70),
    (73.53, 51.50),
    (56.03, 71.74),
    (41.55, 92.37),
    (70.73, 92.20),
];

/// 2×3 affine matrix, row-major: `out_x = a11*x + a12*y + tx`,
/// `out_y = a21*x + a22*y + ty`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a11: f32,
    pub a12: f32,
    pub tx: f32,
    pub a21: f32,
    pub a22: f32,
    pub ty: f32,
}

impl Affine {
    pub const IDENTITY: Self = Self {
        a11: 1.0,
        a12: 0.0,
        tx: 0.0,
        a21: 0.0,
        a22: 1.0,
        ty: 0.0,
    };

    /// Forward map from source coordinates to target coordinates.
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a11 * x + self.a12 * y + self.tx,
            self.a21 * x + self.a22 * y + self.ty,
        )
    }
}

/// Umeyama least-squares similarity transform from `src` to `dst`.
///
/// In 2D a rotation + uniform scale is a complex multiply, so the optimal
/// transform is `a = Σ conj(s_i) d_i / Σ |s_i|²` on centred points, with
/// translation `t = mean_d - a * mean_s`. This is the same least-squares
/// solution as SVD-based Umeyama without the reflection branch, computed in
/// `f64` for stability. Returns [`VisionError::DegenerateLandmarks`] when the
/// points are identical or collinear, and never panics.
pub fn similarity_transform(
    src: &[(f32, f32)],
    dst: &[(f32, f32)],
) -> Result<Affine, VisionError> {
    if src.len() != dst.len() || src.len() < 2 {
        return Err(VisionError::BadInput("need equal point lists of length >= 2"));
    }
    let n = src.len() as f64;

    let mut msx = 0.0f64;
    let mut msy = 0.0f64;
    let mut mdx = 0.0f64;
    let mut mdy = 0.0f64;
    for (&(sx, sy), &(dx, dy)) in src.iter().zip(dst.iter()) {
        msx += sx as f64;
        msy += sy as f64;
        mdx += dx as f64;
        mdy += dy as f64;
    }
    msx /= n;
    msy /= n;
    mdx /= n;
    mdy /= n;

    let mut den = 0.0f64; // Σ |s_i - mean_s|²
    let mut num_re = 0.0f64; // Σ Re(conj(s_i) * d_i)
    let mut num_im = 0.0f64; // Σ Im(conj(s_i) * d_i)
    let mut max_cross = 0.0f64;
    let mut centred: Vec<(f64, f64)> = Vec::with_capacity(src.len());

    for (&(sx, sy), &(dx, dy)) in src.iter().zip(dst.iter()) {
        let sx = sx as f64 - msx;
        let sy = sy as f64 - msy;
        let dx = dx as f64 - mdx;
        let dy = dy as f64 - mdy;
        den += sx * sx + sy * sy;
        num_re += sx * dx + sy * dy;
        num_im += sx * dy - sy * dx;

        for &(px, py) in &centred {
            max_cross = max_cross.max((px * sy - py * sx).abs());
        }
        centred.push((sx, sy));
    }

    if den <= 1e-12 || max_cross <= 1e-6 {
        return Err(VisionError::DegenerateLandmarks);
    }

    let a_re = num_re / den;
    let a_im = num_im / den;
    let tx = mdx - (a_re * msx - a_im * msy);
    let ty = mdy - (a_re * msy + a_im * msx);

    Ok(Affine {
        a11: a_re as f32,
        a12: -a_im as f32,
        tx: tx as f32,
        a21: a_im as f32,
        a22: a_re as f32,
        ty: ty as f32,
    })
}

/// Warp an RGB frame into a 112×112×3 HWC buffer of raw 0–255 bytes.
///
/// `affine` maps source (frame) coordinates to target (112×112) coordinates,
/// i.e. the output of [`similarity_transform`]. Each output pixel is found by
/// inverse mapping plus bilinear sampling.
///
/// Out-of-bounds policy: **black**. A sample whose coordinate falls outside
/// the source frame is zero; for bilinear interpolation each of the four
/// neighbouring taps is checked independently, so pixels near the border can
/// darken toward black. This is cheap and deterministic; a clamp-to-edge
/// policy can be substituted later if evaluation shows a benefit.
pub fn warp_affine_112(frame: &Frame, affine: &Affine) -> Result<Vec<u8>, VisionError> {
    let w = usize::from(frame.width);
    let h = usize::from(frame.height);
    let Some(expected) = w.checked_mul(h).and_then(|n| n.checked_mul(3)) else {
        return Err(VisionError::BadInput("frame dimensions overflow"));
    };
    if expected == 0 || frame.rgb.len() != expected {
        return Err(VisionError::BadInput("frame buffer length mismatch"));
    }

    let (a11, a12, tx, a21, a22, ty) = (
        affine.a11 as f64,
        affine.a12 as f64,
        affine.tx as f64,
        affine.a21 as f64,
        affine.a22 as f64,
        affine.ty as f64,
    );
    let det = a11 * a22 - a12 * a21;
    if det.abs() <= 1e-9 {
        return Err(VisionError::SingularAffine);
    }
    // Inverse of [A | t]: source = A^-1 (target - t).
    let ia11 = a22 / det;
    let ia12 = -a12 / det;
    let ia21 = -a21 / det;
    let ia22 = a11 / det;
    let itx = -(ia11 * tx + ia12 * ty);
    let ity = -(ia21 * tx + ia22 * ty);

    let mut out = Vec::with_capacity(112 * 112 * 3);
    for oy in 0..112u32 {
        for ox in 0..112u32 {
            let sx = ia11 * f64::from(ox) + ia12 * f64::from(oy) + itx;
            let sy = ia21 * f64::from(ox) + ia22 * f64::from(oy) + ity;
            let x0 = sx.floor() as i64;
            let y0 = sy.floor() as i64;
            let fx = sx - x0 as f64;
            let fy = sy - y0 as f64;

            let p00 = tap(&frame.rgb, w, h, x0, y0);
            let p10 = tap(&frame.rgb, w, h, x0 + 1, y0);
            let p01 = tap(&frame.rgb, w, h, x0, y0 + 1);
            let p11 = tap(&frame.rgb, w, h, x0 + 1, y0 + 1);

            for c in 0..3 {
                let v = (1.0 - fx) * (1.0 - fy) * p00[c]
                    + fx * (1.0 - fy) * p10[c]
                    + (1.0 - fx) * fy * p01[c]
                    + fx * fy * p11[c];
                out.push(v.round().clamp(0.0, 255.0) as u8);
            }
        }
    }
    Ok(out)
}

/// One RGB tap, or black when out of bounds.
fn tap(rgb: &[u8], w: usize, h: usize, x: i64, y: i64) -> [f64; 3] {
    if x < 0 || y < 0 {
        return [0.0; 3];
    }
    let (x, y) = (x as usize, y as usize);
    if x >= w || y >= h {
        return [0.0; 3];
    }
    let i = (y * w + x) * 3;
    [
        f64::from(rgb[i]),
        f64::from(rgb[i + 1]),
        f64::from(rgb[i + 2]),
    ]
}

/// Reference aligner: similarity-transform the five landmarks onto the
/// template, then warp. Zero-sized and stateless; a model-backed aligner can
/// replace it through the [`Aligner`] seam.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeometricAligner;

impl Aligner for GeometricAligner {
    fn align(
        &mut self,
        frame: &Frame,
        landmarks: &[(f32, f32); 5],
    ) -> Result<Vec<u8>, VisionError> {
        let affine = similarity_transform(landmarks, &ALIGN_POINTS_112)?;
        warp_affine_112(frame, &affine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn identity_from_target_points() {
        let got = similarity_transform(&ALIGN_POINTS_112, &ALIGN_POINTS_112).expect("transform");
        assert!(close(got.a11, 1.0, 1e-4));
        assert!(close(got.a12, 0.0, 1e-4));
        assert!(close(got.tx, 0.0, 1e-2));
        assert!(close(got.a21, 0.0, 1e-4));
        assert!(close(got.a22, 1.0, 1e-4));
        assert!(close(got.ty, 0.0, 1e-2));
    }

    #[test]
    fn recovers_known_translation_scale_rotation() {
        // Build target = s*R*source + t, then check we recover the same map.
        let scale = 1.5f64;
        let angle = 0.35f64;
        let (cos, sin) = (angle.cos(), angle.sin());
        let (etx, ety) = (7.25f64, -3.5f64);
        let dst: Vec<(f32, f32)> = ALIGN_POINTS_112
            .iter()
            .map(|&(x, y)| {
                let x = f64::from(x);
                let y = f64::from(y);
                (
                    (scale * (cos * x - sin * y) + etx) as f32,
                    (scale * (sin * x + cos * y) + ety) as f32,
                )
            })
            .collect();

        let got = similarity_transform(&ALIGN_POINTS_112, &dst).expect("transform");
        assert!(close(got.a11, (scale * cos) as f32, 1e-3));
        assert!(close(got.a12, (-scale * sin) as f32, 1e-3));
        assert!(close(got.a21, (scale * sin) as f32, 1e-3));
        assert!(close(got.a22, (scale * cos) as f32, 1e-3));
        assert!(close(got.tx, etx as f32, 1e-2));
        assert!(close(got.ty, ety as f32, 1e-2));
    }

    #[test]
    fn degenerate_inputs_return_error_without_panicking() {
        let identical = [(5.0, 5.0); 5];
        assert_eq!(
            similarity_transform(&identical, &ALIGN_POINTS_112),
            Err(VisionError::DegenerateLandmarks)
        );
        let collinear = [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0)];
        assert_eq!(
            similarity_transform(&collinear, &ALIGN_POINTS_112),
            Err(VisionError::DegenerateLandmarks)
        );
        assert!(similarity_transform(&ALIGN_POINTS_112[..1], &ALIGN_POINTS_112[..1]).is_err());
    }

    fn gradient_frame() -> Frame {
        let mut rgb = Vec::with_capacity(112 * 112 * 3);
        for y in 0..112u8 {
            for x in 0..112u8 {
                rgb.extend_from_slice(&[x.wrapping_mul(2), y.wrapping_mul(2), x ^ y]);
            }
        }
        Frame::new(112, 112, rgb).expect("valid frame")
    }

    #[test]
    fn identity_warp_is_unchanged() {
        let frame = gradient_frame();
        let out = warp_affine_112(&frame, &Affine::IDENTITY).expect("warp");
        assert_eq!(out, frame.rgb);
    }

    #[test]
    fn translation_moves_pixels_as_expected() {
        // A = translation by (+3, +2): out = source + (3, 2), so inverse
        // sampling at output (x, y) reads source (x - 3, y - 2).
        let frame = gradient_frame();
        let affine = Affine {
            tx: 3.0,
            ty: 2.0,
            ..Affine::IDENTITY
        };
        let out = warp_affine_112(&frame, &affine).expect("warp");
        for (oy, y) in (0..112).enumerate() {
            for (ox, x) in (0..112).enumerate() {
                let i = (oy * 112 + ox) * 3;
                if x >= 3 && y >= 2 {
                    let si = ((y - 2) * 112 + (x - 3)) * 3;
                    assert_eq!(&out[i..i + 3], &frame.rgb[si..si + 3]);
                } else {
                    // Out-of-bounds neighbours sample as black.
                    assert_eq!(&out[i..i + 3], &[0, 0, 0]);
                }
            }
        }
    }

    #[test]
    fn singular_affine_is_an_error() {
        let frame = gradient_frame();
        let flat = Affine {
            a11: 0.0,
            a12: 0.0,
            tx: 0.0,
            a21: 0.0,
            a22: 0.0,
            ty: 0.0,
        };
        assert_eq!(
            warp_affine_112(&frame, &flat),
            Err(VisionError::SingularAffine)
        );
    }
}
