//! Secret newtypes.
//!
//! A password is UTF-16 (`Vec<u16>`) because the Win32 credential path is
//! wide-string based; an embedding is a normalised feature vector. Both are
//! held in [`zeroize::Zeroizing`] so their backing bytes are wiped on drop on
//! every path (including errors and early returns). Neither type implements
//! `Clone` or `Display`, and both have a hand-written `Debug` that only prints
//! a redaction marker — neither the bytes nor their length leak into logs.

use zeroize::Zeroizing;

/// A UTF-16 password. Never converted to `String`, never formatted.
///
/// The wire layer (`kenali_ipc::wire`) converts to/from `PasswordWide`; the
/// conversion helpers here deliberately require no IPC types.
pub struct Password(Zeroizing<Vec<u16>>);

impl core::fmt::Debug for Password {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Password(<redacted>)")
    }
}

impl Password {
    /// Wrap UTF-16 units. The caller must clear any source buffer immediately.
    pub fn from_wide(units: Vec<u16>) -> Self {
        Self(Zeroizing::new(units))
    }

    /// Borrow the UTF-16 units. Only for the wire encoder; never log this.
    pub fn as_wide(&self) -> &[u16] {
        &self.0
    }

    /// Consume and return the UTF-16 units for immediate wire wrapping.
    pub fn into_wide_units(self) -> Vec<u16> {
        self.0.to_vec()
    }

    /// Number of UTF-16 code units. The length is not secret and is safe to log.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A unit-normalised face embedding. Features are biometrics: treat as secret.
pub struct Embedding(Zeroizing<Vec<f32>>);

impl core::fmt::Debug for Embedding {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Embedding(<redacted>)")
    }
}

impl Embedding {
    /// Build from raw features, normalising to unit length.
    ///
    /// A zero or non-finite vector is stored as-is; [`crate::score::cosine`]
    /// handles that deterministically. No `unwrap`, no panics.
    pub fn new(features: Vec<f32>) -> Self {
        let mut features = features;
        let norm_sq: f32 = features
            .iter()
            .filter(|v| v.is_finite())
            .map(|v| v * v)
            .sum();
        if norm_sq.is_finite() && norm_sq > 0.0 {
            let norm = norm_sq.sqrt();
            for v in &mut features {
                if v.is_finite() {
                    *v /= norm;
                }
            }
        }
        Self(Zeroizing::new(features))
    }

    /// Borrow the features. Only for scoring / the AEAD encryptor; never log.
    pub fn as_slice(&self) -> &[f32] {
        &self.0
    }

    /// Consume into a zeroizing buffer for encryption.
    pub fn into_zeroizing(self) -> Zeroizing<Vec<f32>> {
        self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_debug_is_redacted_and_never_leaks_bytes() {
        let secret_units: Vec<u16> = "hunter2".encode_utf16().collect();
        let password = Password::from_wide(secret_units);
        let dbg = format!("{:?}", password);
        assert!(dbg.contains("<redacted>"), "debug must redact: {dbg}");
        assert!(!dbg.contains("hunter2"), "debug leaked the secret: {dbg}");
        assert_eq!(password.len(), 7);
    }

    #[test]
    fn embedding_debug_is_redacted() {
        let emb = Embedding::new(vec![0.5, 0.5]);
        let dbg = format!("{:?}", emb);
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains('0'));
    }

    #[test]
    fn embedding_new_normalises() {
        let emb = Embedding::new(vec![3.0, 4.0]);
        let s = emb.as_slice();
        assert!((s[0] - 0.6).abs() < 1e-6);
        assert!((s[1] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn embedding_zero_vector_does_not_panic() {
        let emb = Embedding::new(vec![0.0, 0.0]);
        assert_eq!(emb.as_slice(), &[0.0, 0.0]);
    }
}
