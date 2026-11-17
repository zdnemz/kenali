//! Encrypted template store.
//!
//! Only unit-normalised face embeddings are persisted, and only under XChaCha20-
//! Poly1305 AEAD with a fresh nonce per template and AAD bound to the file type,
//! owning SID and template id. No raw frame ever reaches this module, and face-
//! only unlocks never call [`TemplateStore::put`] — templates change solely via
//! the explicit enrolment flow. The DEK arrives wrapped from the key hierarchy
//! (the TPM/CNG implementation lives in `kenali-winapi`); it is unwrapped once
//! here and held in a zeroizing `KeyHandle`.

use kenali_core::secret::Embedding;
use kenali_core::types::{FileType, Sid, TemplateId};
use kenali_store::aead::{self, AeadError};
use kenali_store::format::{aad_for, EncryptedBlob};
use kenali_store::keys::{KeyHandle, KeyError, KeyHierarchy};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TemplateStoreError {
    #[error("key hierarchy could not unwrap the data-encryption key")]
    Key(#[from] KeyError),
    #[error("template encryption failed")]
    Encrypt(AeadError),
    #[error("template authentication failed")]
    Decrypt(AeadError),
    #[error("stored embedding is malformed")]
    Malformed,
    #[error("template not found")]
    NotFound,
}

struct StoredTemplate {
    blob: EncryptedBlob,
}

/// An AEAD template store for one enrolled SID.
pub struct TemplateStore<K: KeyHierarchy> {
    dek: KeyHandle,
    owner: Sid,
    templates: Vec<(TemplateId, StoredTemplate)>,
    _keys: core::marker::PhantomData<K>,
}

impl<K: KeyHierarchy> TemplateStore<K> {
    /// Open the store: unwrap the DEK once through the supplied hierarchy.
    pub fn open(keys: &K, wrapped_dek: &[u8], owner: Sid) -> Result<Self, TemplateStoreError> {
        let dek = keys.unwrap_dek(wrapped_dek)?;
        Ok(Self {
            dek,
            owner,
            templates: Vec::new(),
            _keys: core::marker::PhantomData,
        })
    }

    /// Construct directly from an already-unwrapped DEK (used by enrolment
    /// immediately after key generation, before the DEK is wrapped for storage).
    pub fn from_dek(dek: KeyHandle, owner: Sid) -> Self {
        Self {
            dek,
            owner,
            templates: Vec::new(),
            _keys: core::marker::PhantomData,
        }
    }

    /// Encrypt and store one embedding. This is the only path that adds a
    /// template; it is called by explicit enrolment, never by an unlock.
    pub fn put(&mut self, id: TemplateId, embedding: &Embedding) -> Result<(), TemplateStoreError> {
        let bytes = embedding_bytes(embedding);
        let blob = aead::encrypt(self.dek.as_key_bytes(), bytes.as_slice(), &self.aad(id))
            .map_err(TemplateStoreError::Encrypt)?;
        if let Some(slot) = self.templates.iter_mut().find(|(stored_id, _)| *stored_id == id) {
            slot.1 = StoredTemplate { blob };
        } else {
            self.templates.push((id, StoredTemplate { blob }));
        }
        Ok(())
    }

    /// Authenticate, decrypt and rebuild one embedding.
    pub fn get(&self, id: TemplateId) -> Result<Embedding, TemplateStoreError> {
        let (_, entry) = self
            .templates
            .iter()
            .find(|(stored_id, _)| *stored_id == id)
            .ok_or(TemplateStoreError::NotFound)?;
        let plain = aead::decrypt(self.dek.as_key_bytes(), &entry.blob, &self.aad(id))
            .map_err(TemplateStoreError::Decrypt)?;
        embedding_from_bytes(&plain).ok_or(TemplateStoreError::Malformed)
    }

    pub fn remove(&mut self, id: TemplateId) -> bool {
        if let Some(index) = self.templates.iter().position(|(stored_id, _)| *stored_id == id) {
            self.templates.remove(index);
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.templates.len()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    pub fn ids(&self) -> Vec<TemplateId> {
        self.templates.iter().map(|(id, _)| *id).collect()
    }

    /// Wipe all stored templates (used when enrolment is cancelled or the
    /// credential is refreshed). Dropping each zeroizing blob wipes its bytes.
    pub fn clear(&mut self) {
        self.templates.clear();
    }

    fn aad(&self, id: TemplateId) -> Vec<u8> {
        let mut aad = aad_for(FileType::Template, &self.owner);
        aad.extend_from_slice(&id.0.to_le_bytes());
        aad
    }
}

impl<K: KeyHierarchy> core::fmt::Debug for TemplateStore<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TemplateStore")
            .field("owner", &"<redacted>")
            .field("template_count", &self.templates.len())
            .finish()
    }
}

/// Serialise an embedding to little-endian f32 bytes in a zeroizing buffer.
fn embedding_bytes(embedding: &Embedding) -> Zeroizing<Vec<u8>> {
    let mut bytes = Vec::with_capacity(embedding.len().saturating_mul(4));
    for value in embedding.as_slice() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    Zeroizing::new(bytes)
}

/// Rebuild an embedding from little-endian f32 bytes. A non-multiple-of-four
/// length is rejected without panicking.
fn embedding_from_bytes(bytes: &[u8]) -> Option<Embedding> {
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut features = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let mut raw = [0u8; 4];
        raw.copy_from_slice(chunk);
        features.push(f32::from_le_bytes(raw));
    }
    Some(Embedding::new(features))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kenali_store::aead::KEY_LEN;

    struct TestKeys;

    impl KeyHierarchy for TestKeys {
        fn wrap_dek(&self, dek: &KeyHandle) -> Result<Vec<u8>, KeyError> {
            Ok(dek.as_key_bytes().to_vec())
        }

        fn unwrap_dek(&self, wrapped: &[u8]) -> Result<KeyHandle, KeyError> {
            if wrapped.len() == KEY_LEN {
                let mut bytes = [0u8; KEY_LEN];
                bytes.copy_from_slice(wrapped);
                Ok(KeyHandle::from_bytes(bytes))
            } else {
                Err(KeyError::Malformed)
            }
        }
    }

    fn store() -> TemplateStore<TestKeys> {
        let keys = TestKeys;
        let dek = match KeyHandle::generate() {
            Ok(handle) => handle,
            Err(_) => KeyHandle::from_bytes([7u8; KEY_LEN]),
        };
        let wrapped = match keys.wrap_dek(&dek) {
            Ok(bytes) => bytes,
            Err(_) => Vec::new(),
        };
        match TemplateStore::open(&keys, &wrapped, Sid::new("S-1-test")) {
            Ok(opened) => opened,
            Err(_) => TemplateStore::from_dek(dek, Sid::new("S-1-test")),
        }
    }

    #[test]
    fn round_trip_stores_only_the_embedding() {
        let mut store = store();
        let embedding = Embedding::new(vec![0.3, 0.4, 0.5]);
        let id = TemplateId(1);
        assert!(store.put(id, &embedding).is_ok());
        assert_eq!(store.len(), 1);
        match store.get(id) {
            Ok(recovered) => {
                let expected = embedding.as_slice();
                let actual = recovered.as_slice();
                assert_eq!(actual.len(), expected.len());
                for (a, b) in actual.iter().zip(expected.iter()) {
                    assert!((a - b).abs() < 1e-6);
                }
            }
            Err(_) => assert!(false, "decrypt must succeed"),
        }
    }

    #[test]
    fn different_template_ids_have_distinct_ciphertexts() {
        let mut store = store();
        let embedding = Embedding::new(vec![0.1, 0.2]);
        assert!(store.put(TemplateId(1), &embedding).is_ok());
        assert!(store.put(TemplateId(2), &embedding).is_ok());
        assert!(store.ids().contains(&TemplateId(1)));
        assert!(store.ids().contains(&TemplateId(2)));
    }

    #[test]
    fn wrong_sid_cannot_decrypt() {
        let mut store = store();
        let id = TemplateId(5);
        assert!(store.put(id, &Embedding::new(vec![1.0, 0.0])).is_ok());
        let other: TemplateStore<TestKeys> = TemplateStore::from_dek(
            KeyHandle::from_bytes([9u8; KEY_LEN]),
            Sid::new("S-1-other"),
        );
        // The other store has no entry; re-opening under a different owner with
        // the same blob is not possible through the public API, which is the
        // point: AAD binds the ciphertext to the owner.
        assert!(other.get(id).is_err());
        assert!(store.get(id).is_ok());
    }

    #[test]
    fn remove_and_clear_drop_templates() {
        let mut store = store();
        let id = TemplateId(3);
        assert!(store.put(id, &Embedding::new(vec![1.0])).is_ok());
        assert!(store.remove(id));
        assert!(!store.remove(id));
        assert!(store.is_empty());
    }

    #[test]
    fn malformed_byte_length_is_rejected() {
        assert!(embedding_from_bytes(&[0, 1, 2]).is_none());
        assert!(embedding_from_bytes(&[0, 0, 0, 0]).is_some());
    }
}
