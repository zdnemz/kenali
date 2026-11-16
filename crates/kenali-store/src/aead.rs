//! AEAD layer: XChaCha20-Poly1305 with a fresh random nonce per message.
//!
//! Wave 1 implements only the symmetric AEAD. The DEK is a random 32-byte key
//! used directly here; wrapping it under a TPM RSA key is the responsibility of
//! a later [`crate::keys::KeyHierarchy`] implementation in `kenali-winapi`.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::format::EncryptedBlob;

/// Length of an XChaCha20-Poly1305 nonce in bytes (192-bit extended nonce).
pub const NONCE_LEN: usize = 24;
/// Length of the symmetric data-encryption key (DEK) in bytes.
pub const KEY_LEN: usize = 32;

/// Errors from the AEAD layer.
///
/// Variants carry no plaintext, key material or AAD, so they are safe to log.
#[derive(Debug, thiserror::Error)]
pub enum AeadError {
    /// Encryption failed (only possible on internal cipher misconfiguration).
    #[error("encryption failed")]
    Encrypt,
    /// Decryption failed: wrong key, wrong AAD, or a tampered ciphertext.
    #[error("authentication failed: ciphertext, key or AAD is invalid")]
    Authenticate,
    /// The platform RNG could not produce a nonce. No nonce was reused.
    #[error("secure nonce generation failed")]
    Nonce,
}

/// Encrypt `plaintext` under a 32-byte `key` and `aad`.
///
/// A fresh 24-byte nonce is drawn from the OS CSPRNG on every call, so a nonce
/// is never reused across messages. The returned [`EncryptedBlob`] holds the
/// nonce and the ciphertext with its 16-byte Poly1305 tag appended.
pub fn encrypt(
    key: &[u8; KEY_LEN],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<EncryptedBlob, AeadError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| AeadError::Encrypt)?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce_bytes).map_err(|_| AeadError::Nonce)?;
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, Payload { msg: plaintext, aad })
        .map_err(|_| AeadError::Encrypt)?;

    Ok(EncryptedBlob {
        nonce: nonce_bytes,
        ciphertext,
    })
}

/// Authenticate and decrypt `blob` under `key` and `aad`.
///
/// Any tampering, wrong key, wrong AAD or malformed ciphertext yields
/// [`AeadError::Authenticate`] rather than a panic, so hostile input is safe.
pub fn decrypt(
    key: &[u8; KEY_LEN],
    blob: &EncryptedBlob,
    aad: &[u8],
) -> Result<Vec<u8>, AeadError> {
    let cipher = XChaCha20Poly1305::new_from_slice(key).map_err(|_| AeadError::Authenticate)?;
    let nonce = XNonce::from_slice(&blob.nonce);
    cipher
        .decrypt(nonce, Payload { msg: &blob.ciphertext, aad })
        .map_err(|_| AeadError::Authenticate)
}
