//! Similarity scoring. Scores are computed for the policy decision only and
//! must never be rendered in a tile or in the management UI.

use crate::secret::Embedding;

/// Cosine similarity of two embeddings, in `[-1.0, 1.0]`.
///
/// Deterministic degenerate cases (no NaN, no infinities out):
/// - dimension mismatch → `0.0`
/// - zero vector or non-finite norm → `0.0`
/// - non-finite components are skipped in the sums
///
/// The caller decides the operating threshold (calibrated, never a tutorial
/// default); this function only returns the raw similarity.
pub fn cosine(a: &Embedding, b: &Embedding) -> f32 {
    let (x, y) = (a.as_slice(), b.as_slice());
    if x.len() != y.len() || x.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for (&xi, &yi) in x.iter().zip(y.iter()) {
        if xi.is_finite() && yi.is_finite() {
            dot += xi * yi;
            norm_a += xi * xi;
            norm_b += yi * yi;
        }
    }
    if !norm_a.is_finite() || !norm_b.is_finite() || norm_a <= 0.0 || norm_b <= 0.0 {
        return 0.0;
    }
    let denominator = norm_a.sqrt() * norm_b.sqrt();
    if !denominator.is_finite() || denominator <= 0.0 {
        return 0.0;
    }
    (dot / denominator).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_embeddings_score_one() {
        let a = Embedding::new(vec![1.0, 2.0, 3.0]);
        let b = Embedding::new(vec![1.0, 2.0, 3.0]);
        let score = cosine(&a, &b);
        assert!((score - 1.0).abs() < 1e-6, "score was {score}");
    }

    #[test]
    fn orthogonal_embeddings_score_zero() {
        let a = Embedding::new(vec![1.0, 0.0, 0.0]);
        let b = Embedding::new(vec![0.0, 1.0, 0.0]);
        let score = cosine(&a, &b);
        assert!(score.abs() < 1e-6, "score was {score}");
    }

    #[test]
    fn opposite_embeddings_score_minus_one() {
        let a = Embedding::new(vec![1.0, 0.0]);
        let b = Embedding::new(vec![-1.0, 0.0]);
        let score = cosine(&a, &b);
        assert!((score + 1.0).abs() < 1e-6, "score was {score}");
    }

    #[test]
    fn degenerate_inputs_are_nan_free_and_deterministic() {
        let zero = Embedding::new(vec![0.0, 0.0]);
        let normal = Embedding::new(vec![1.0, 0.0]);
        let nan = Embedding::new(vec![f32::NAN, 1.0]);
        let shorter = Embedding::new(vec![1.0]);
        for (a, b) in [(&zero, &normal), (&normal, &zero), (&nan, &normal), (&normal, &shorter)] {
            let score = cosine(a, b);
            assert!(score.is_finite(), "score was {score}");
            assert_eq!(score, 0.0);
        }
    }
}
