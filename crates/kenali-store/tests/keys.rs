//! Key hierarchy trait exercised by a soft (in-memory) implementation.
//!
//! The real TPM/CNG implementation arrives in a later wave; these tests prove
//! the trait boundary is usable without it.

use kenali_core::types::{FileType, Sid};
use kenali_store::aead::{decrypt, encrypt, KEY_LEN};
use kenali_store::format::aad_for;
use kenali_store::keys::{KeyError, KeyHandle, KeyHierarchy};

/// Deliberately toy XOR "wrapping" — only to exercise the trait shape.
struct SoftKeyHierarchy {
    kek: [u8; KEY_LEN],
}

impl KeyHierarchy for SoftKeyHierarchy {
    fn wrap_dek(&self, dek: &KeyHandle) -> Result<Vec<u8>, KeyError> {
        let raw = dek.as_key_bytes();
        Ok((0..KEY_LEN).map(|i| raw[i] ^ self.kek[i]).collect())
    }

    fn unwrap_dek(&self, wrapped: &[u8]) -> Result<KeyHandle, KeyError> {
        if wrapped.len() != KEY_LEN {
            return Err(KeyError::Malformed);
        }
        let mut raw = [0u8; KEY_LEN];
        for (i, byte) in wrapped.iter().enumerate() {
            raw[i] = byte ^ self.kek[i];
        }
        Ok(KeyHandle::from_bytes(raw))
    }
}

#[test]
fn wrap_unwrap_round_trip() {
    let hierarchy = SoftKeyHierarchy { kek: [0x33; KEY_LEN] };
    let dek = KeyHandle::from_bytes([0x77; KEY_LEN]);
    let wrapped = hierarchy.wrap_dek(&dek).unwrap();
    assert_ne!(wrapped.as_slice(), dek.as_key_bytes());

    let unwrapped = hierarchy.unwrap_dek(&wrapped).unwrap();
    assert_eq!(unwrapped.as_key_bytes(), dek.as_key_bytes());
}

#[test]
fn unwrapped_dek_can_drive_the_aead() {
    let hierarchy = SoftKeyHierarchy { kek: [0x44; KEY_LEN] };
    let dek = KeyHandle::from_bytes([0x88; KEY_LEN]);
    let wrapped = hierarchy.wrap_dek(&dek).unwrap();
    let unwrapped = hierarchy.unwrap_dek(&wrapped).unwrap();

    let aad = aad_for(FileType::Secret, &Sid::new("S-1-5-21-42"));
    let blob = encrypt(unwrapped.as_key_bytes(), b"credential", &aad).unwrap();
    let plain = decrypt(dek.as_key_bytes(), &blob, &aad).unwrap();
    assert_eq!(plain, b"credential");
}

#[test]
fn unwrap_with_wrong_kek_yields_wrong_key() {
    let wrapper = SoftKeyHierarchy { kek: [0x55; KEY_LEN] };
    let unwrapper = SoftKeyHierarchy { kek: [0x56; KEY_LEN] };
    let dek = KeyHandle::from_bytes([0x99; KEY_LEN]);
    let wrapped = wrapper.wrap_dek(&dek).unwrap();
    let wrong = unwrapper.unwrap_dek(&wrapped).unwrap();
    assert_ne!(wrong.as_key_bytes(), dek.as_key_bytes());
}

#[test]
fn malformed_wrapped_key_is_rejected() {
    let hierarchy = SoftKeyHierarchy { kek: [0x66; KEY_LEN] };
    assert!(matches!(
        hierarchy.unwrap_dek(&[0u8; KEY_LEN - 1]),
        Err(KeyError::Malformed)
    ));
}

#[test]
fn generated_deks_are_distinct_and_redacted() {
    let a = KeyHandle::generate().unwrap();
    let b = KeyHandle::generate().unwrap();
    assert_ne!(a.as_key_bytes(), b.as_key_bytes());
    let dbg = format!("{a:?}");
    assert!(dbg.contains("<redacted>"));
}
