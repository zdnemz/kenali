//! Encrypted blob format and AAD construction.
//!
//! The AAD binds a ciphertext to the store magic, the format version, the kind
//! of file, and the owning user's SID. It is authenticated (not encrypted) by
//! the AEAD, so tampering with any of those values makes decryption fail.
//!
//! Serialised blob layout: `nonce ‖ ciphertext ‖ tag` — the nonce is a fixed
//! 24 bytes, the Poly1305 tag is a fixed 16 bytes appended by the AEAD.

use kenali_core::types::{FileType, Sid};

use crate::aead::NONCE_LEN;

/// Magic prefix identifying a Kenali encrypted blob (8 ASCII bytes).
pub const MAGIC: &[u8; 8] = b"KENALST1";
/// On-disk / AAD format version.
pub const FORMAT_VERSION: u16 = 1;
/// Poly1305 authentication tag length in bytes.
pub const TAG_LEN: usize = 16;

/// Errors when parsing a serialised [`EncryptedBlob`].
#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    /// Fewer bytes than a nonce plus a tag — cannot be a valid blob.
    #[error("encrypted blob is too short to contain a nonce and a tag")]
    TooShort,
}

/// Ciphertext plus the nonce required to decrypt it.
///
/// `ciphertext` already includes the 16-byte Poly1305 tag. The nonce and
/// ciphertext are not secret; they are safe to serialise and log.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EncryptedBlob {
    /// Fresh nonce used for this message (24 bytes).
    pub nonce: [u8; NONCE_LEN],
    /// Ciphertext with the 16-byte authentication tag appended.
    pub ciphertext: Vec<u8>,
}

impl EncryptedBlob {
    /// Serialise to `nonce ‖ ciphertext ‖ tag`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(NONCE_LEN + self.ciphertext.len());
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
        out
    }

    /// Parse a blob produced by [`EncryptedBlob::to_bytes`].
    ///
    /// Rejects anything shorter than a nonce plus a tag; never panics on
    /// hostile input.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, BlobError> {
        if bytes.len() < NONCE_LEN + TAG_LEN {
            return Err(BlobError::TooShort);
        }
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[..NONCE_LEN]);
        Ok(Self {
            nonce,
            ciphertext: bytes[NONCE_LEN..].to_vec(),
        })
    }
}

/// Map a [`FileType`] to a stable one-byte discriminant.
///
/// Uses an explicit match (never `Debug` or a serde name) so the byte is fixed
/// across refactors. `FileType` is `#[non_exhaustive]`, hence the wildcard arm
/// for any future variant.
fn file_type_byte(file_type: FileType) -> u8 {
    match file_type {
        FileType::Template => 1,
        FileType::Secret => 2,
        FileType::Config => 3,
        FileType::State => 4,
        _ => 0,
    }
}

fn aad_with_version(version: u16, file_type: FileType, sid: &Sid) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.push(file_type_byte(file_type));

    // Encode the SID as UTF-16LE (Win32 identity representation), length-
    // prefixed so distinct SIDs can never produce the same byte sequence.
    let mut sid_bytes = Vec::new();
    for unit in sid.as_str().encode_utf16() {
        sid_bytes.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&(sid_bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(&sid_bytes);
    out
}

/// Build the AAD bound to a ciphertext for `file_type` and `sid`.
///
/// Layout: `magic ‖ version (LE u16) ‖ file type (1 byte) ‖ SID length (LE u64)
/// ‖ SID (UTF-16LE)`. Deterministic and domain-separated by construction.
pub fn aad_for(file_type: FileType, sid: &Sid) -> Vec<u8> {
    aad_with_version(FORMAT_VERSION, file_type, sid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn different_versions_produce_different_aad() {
        let sid = Sid::new("S-1-5-21-1");
        let v1 = aad_with_version(1, FileType::Template, &sid);
        let v2 = aad_with_version(2, FileType::Template, &sid);
        assert_ne!(v1, v2);
        assert_eq!(&v1[8..10], &1u16.to_le_bytes());
        assert_eq!(&v2[8..10], &2u16.to_le_bytes());
    }
}
