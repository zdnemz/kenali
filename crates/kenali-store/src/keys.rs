//! Key hierarchy boundary.
//!
//! Wave 1 defines the trait and an in-memory [`KeyHandle`] for the 32-byte
//! data-encryption key (DEK). A real TPM/CNG implementation of
//! [`KeyHierarchy`] arrives later in `kenali-winapi`; no caller changes will be
//! needed because callers depend only on the trait.

use zeroize::Zeroizing;

use crate::aead::KEY_LEN;

/// Errors from key hierarchy operations.
///
/// Variants carry no key material, so they are safe to log.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// The platform RNG could not generate key material.
    #[error("secure random generation failed")]
    Random,
    /// A DEK could not be wrapped.
    #[error("key wrapping failed")]
    Wrap,
    /// A wrapped DEK could not be unwrapped.
    #[error("key unwrapping failed")]
    Unwrap,
    /// Wrapped key material had an unexpected length or format.
    #[error("wrapped key material is malformed")]
    Malformed,
    /// No key hierarchy is available on this platform/configuration.
    #[error("key hierarchy is unavailable on this platform")]
    Unavailable,
}

/// A 32-byte data-encryption key held in a zeroizing buffer.
///
/// The bytes are wiped on drop on every path. `Debug` only prints a redaction
/// marker and `Clone` is deliberately not implemented.
pub struct KeyHandle(Zeroizing<[u8; KEY_LEN]>);

impl core::fmt::Debug for KeyHandle {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("KeyHandle(<redacted>)")
    }
}

impl KeyHandle {
    /// Wrap an existing 32-byte key. The caller must zeroize its own copy.
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    /// Generate a fresh random DEK from the OS CSPRNG.
    pub fn generate() -> Result<Self, KeyError> {
        let mut bytes = [0u8; KEY_LEN];
        getrandom::fill(&mut bytes).map_err(|_| KeyError::Random)?;
        Ok(Self::from_bytes(bytes))
    }

    /// Borrow the key bytes for the AEAD layer. Never log this slice.
    pub fn as_key_bytes(&self) -> &[u8; KEY_LEN] {
        &*self.0
    }
}

/// A key-encryption hierarchy able to wrap and unwrap DEKs.
///
/// The Wave-1 soft implementation lives in tests; a later TPM/CNG
/// implementation in `kenali-winapi` implements this same trait so no caller
/// needs to change.
pub trait KeyHierarchy {
    /// Wrap a DEK into opaque bytes suitable for storage.
    fn wrap_dek(&self, dek: &KeyHandle) -> Result<Vec<u8>, KeyError>;

    /// Unwrap bytes produced by [`KeyHierarchy::wrap_dek`] back into a DEK.
    fn unwrap_dek(&self, wrapped: &[u8]) -> Result<KeyHandle, KeyError>;
}
