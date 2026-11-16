#![forbid(unsafe_code)]

//! `kenali-store` — AEAD, key hierarchy and encrypted blob format.
//!
//! Wave 1 provides the platform-agnostic symmetric layer only: XChaCha20-Poly1305
//! encryption with per-message random nonces, AAD construction that binds a
//! ciphertext to the store magic, format version, file type and user SID, and a
//! key-hierarchy trait boundary. A TPM/CNG implementation of the hierarchy
//! arrives in a later wave in `kenali-winapi`.

pub mod aead;
pub mod format;
pub mod keys;
