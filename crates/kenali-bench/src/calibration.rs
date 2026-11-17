//! Operating-threshold selection and ROC points for cosine scores (NFR-04).
//!
//! Accept rule throughout this module: a trial is accepted when
//! `score >= threshold` (cosine similarity: higher is more genuine).
//! `fpr`/`far` are the fraction of impostor scores accepted; `tpr`/`tar` are
//! the fraction of genuine scores accepted. No model is loaded here — callers
//! pass labelled score vectors.

/// Tolerance for float representation of an exact FAR (e.g. 1/1000). Only
/// absorbs representation error; it never meaningfully loosens a target.
const FAR_TOL: f64 = 1e-12;

/// Threshold choice with the measured rates it actually achieves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThresholdResult {
    pub threshold: f64,
    pub far: f64,
    pub tar: f64,
}

fn finite(scores: &[f64]) -> Vec<f64> {
    scores.iter().copied().filter(|s| s.is_finite()).collect()
}

fn accept_rate(scores: &[f64], threshold: f64) -> f64 {
    scores.iter().filter(|&&s| s >= threshold).count() as f64 / scores.len() as f64
}

/// Pick the lowest candidate threshold whose measured FAR over `impostor`
/// scores is still within `target_far`. Lower thresholds accept more genuines
/// (higher TAR) but also more impostors, so the lowest *admissible* candidate
/// maximises TAR without breaking the FAR budget.
///
/// Returns `None` when the target is out of `[0, 1]`, either label set is empty,
/// there are no candidates, or no candidate keeps FAR within target — i.e. it
/// never returns a threshold that loosens beyond the caller's target.
pub fn choose_threshold(
    impostor: &[f64],
    genuine: &[f64],
    candidates: &[f64],
    target_far: f64,
) -> Option<ThresholdResult> {
    if !target_far.is_finite() || !(0.0..=1.0).contains(&target_far) {
        return None;
    }
    let imp = finite(impostor);
    let genu = finite(genuine);
    if imp.is_empty() || genu.is_empty() || candidates.is_empty() {
        return None;
    }

    let mut best: Option<ThresholdResult> = None;
    for &threshold in candidates {
        if !threshold.is_finite() {
            continue;
        }
        let far = accept_rate(&imp, threshold);
        if far > target_far + FAR_TOL {
            continue; // would loosen beyond the target: never admissible
        }
        let tar = accept_rate(&genu, threshold);
        let better = best.map_or(true, |b| threshold < b.threshold);
        if better {
            best = Some(ThresholdResult {
                threshold,
                far,
                tar,
            });
        }
    }
    best
}

/// One ROC point at a score threshold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RocPoint {
    pub threshold: f64,
    pub fpr: f64,
    pub tpr: f64,
}

/// Sweep thresholds over the distinct scores in both label sets, returning
/// `(fpr, tpr)` points ordered by descending threshold. Because the accepted
/// set only grows as the threshold falls, both `fpr` and `tpr` are
/// non-decreasing in the returned order (monotonic in score order).
pub fn roc_curve(impostor: &[f64], genuine: &[f64]) -> Vec<RocPoint> {
    let imp = finite(impostor);
    let genu = finite(genuine);
    if imp.is_empty() || genu.is_empty() {
        return Vec::new();
    }

    let mut thresholds: Vec<f64> = imp.iter().chain(genu.iter()).copied().collect();
    thresholds.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    thresholds.dedup();

    thresholds
        .into_iter()
        .map(|threshold| RocPoint {
            threshold,
            fpr: accept_rate(&imp, threshold),
            tpr: accept_rate(&genu, threshold),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choose_at(target: f64) -> ThresholdResult {
        let impostor = [0.0, 0.1, 0.2, 0.3, 0.4];
        let genuine = [0.6, 0.7, 0.8];
        let candidates = [0.0, 0.25, 0.5, 0.75];
        choose_threshold(&impostor, &genuine, &candidates, target).expect("some candidate fits")
    }

    #[test]
    fn chooses_lowest_admissible_threshold_and_respects_far() {
        let r = choose_at(0.21);
        assert_eq!(r.threshold, 0.5); // 0.0 and 0.25 breach FAR; 0.5 is lowest valid
        assert_eq!(r.far, 0.0);
        assert_eq!(r.tar, 1.0);
        assert!(r.far <= 0.21);
    }

    #[test]
    fn never_loosens_beyond_target() {
        let r = choose_at(0.21);
        assert!(r.far <= 0.21, "chosen threshold must keep FAR within target");
        // Threshold exactly at target FAR (1/5 = 0.2) is admissible via tolerance.
        let r2 = choose_at(0.2);
        assert!(r2.far <= 0.2 + FAR_TOL);
    }

    #[test]
    fn returns_none_when_no_candidate_fits() {
        let impostor = [0.9];
        let genuine = [0.95];
        let candidates = [0.0, 0.5];
        assert!(choose_threshold(&impostor, &genuine, &candidates, 0.0).is_none());
    }

    #[test]
    fn returns_none_on_empty_or_invalid_inputs() {
        assert!(choose_threshold(&[], &[0.8], &[0.5], 0.1).is_none());
        assert!(choose_threshold(&[0.1], &[], &[0.5], 0.1).is_none());
        assert!(choose_threshold(&[0.1], &[0.8], &[], 0.1).is_none());
        assert!(choose_threshold(&[0.1], &[0.8], &[0.5], -0.1).is_none());
        assert!(choose_threshold(&[0.1], &[0.8], &[0.5], f64::NAN).is_none());
    }

    #[test]
    fn roc_curve_is_monotonic_in_score_order() {
        let points = roc_curve(&[0.1, 0.3], &[0.2, 0.4]);
        assert_eq!(points.len(), 4);
        // thresholds descending
        for w in points.windows(2) {
            assert!(w[0].threshold >= w[1].threshold);
        }
        // fpr and tpr non-decreasing as threshold falls
        for w in points.windows(2) {
            assert!(w[0].fpr <= w[1].fpr, "fpr must be non-decreasing");
            assert!(w[0].tpr <= w[1].tpr, "tpr must be non-decreasing");
        }
        assert_eq!(points.first().map(|p| p.fpr), Some(0.0));
        assert_eq!(points.last().map(|p| (p.fpr, p.tpr)), Some((1.0, 1.0)));
    }

    #[test]
    fn roc_curve_empty_and_nan_inputs() {
        assert!(roc_curve(&[], &[0.5]).is_empty());
        assert!(roc_curve(&[0.1], &[]).is_empty());
        let p = roc_curve(&[f64::NAN, 0.1], &[0.8, f64::INFINITY]);
        assert_eq!(p.len(), 2); // only finite scores remain
    }
}
