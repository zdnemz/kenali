//! AEAD behaviour: round-trips, tamper detection, nonce uniqueness.

use std::collections::HashSet;

use kenali_store::aead::{decrypt, encrypt, KEY_LEN, NONCE_LEN};
use kenali_store::format::{EncryptedBlob, TAG_LEN};
use proptest::prelude::*;

fn key(byte: u8) -> [u8; KEY_LEN] {
    [byte; KEY_LEN]
}

proptest! {
    /// Arbitrary plaintexts and AADs round-trip under a fixed key.
    #[test]
    fn round_trip_arbitrary_plaintext_and_aad(
        plaintext in prop::collection::vec(any::<u8>(), 0..4096),
        aad in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let k = key(0x11);
        let blob = encrypt(&k, &plaintext, &aad).unwrap();
        let out = decrypt(&k, &blob, &aad).unwrap();
        prop_assert_eq!(blob.nonce.len(), NONCE_LEN);
        prop_assert_eq!(blob.ciphertext.len(), plaintext.len() + TAG_LEN);
        prop_assert_eq!(out, plaintext);
    }
}

#[test]
fn tamper_ciphertext_fails() {
    let k = key(0x21);
    let aad = b"context";
    let mut blob = encrypt(&k, b"top secret", aad).unwrap();
    assert!(decrypt(&k, &blob, aad).is_ok());

    let last = blob.ciphertext.len() - 1;
    blob.ciphertext[last] ^= 0x01;
    assert!(decrypt(&k, &blob, aad).is_err());
}

#[test]
fn tamper_aad_fails() {
    let k = key(0x22);
    let blob = encrypt(&k, b"top secret", b"context-a").unwrap();
    assert!(decrypt(&k, &blob, b"context-a").is_ok());
    assert!(decrypt(&k, &blob, b"context-b").is_err());
}

#[test]
fn wrong_key_fails() {
    let aad = b"context";
    let blob = encrypt(&key(0x31), b"top secret", aad).unwrap();
    assert!(decrypt(&key(0x32), &blob, aad).is_err());
}

#[test]
fn malformed_blob_fails_gracefully() {
    let k = key(0x41);
    let blob = EncryptedBlob {
        nonce: [0u8; NONCE_LEN],
        ciphertext: vec![0u8; TAG_LEN],
    };
    // Garbage ciphertext/tag must be an error, never a panic.
    assert!(decrypt(&k, &blob, b"aad").is_err());
}

#[test]
fn blob_serialization_round_trip() {
    let k = key(0x51);
    let blob = encrypt(&k, b"payload", b"aad").unwrap();
    let bytes = blob.to_bytes();
    assert_eq!(&bytes[..NONCE_LEN], &blob.nonce);
    let parsed = EncryptedBlob::from_bytes(&bytes).unwrap();
    assert_eq!(parsed, blob);
    assert_eq!(decrypt(&k, &parsed, b"aad").unwrap(), b"payload");
}

#[test]
fn from_bytes_rejects_short_input() {
    assert!(EncryptedBlob::from_bytes(&[]).is_err());
    assert!(EncryptedBlob::from_bytes(&[0u8; NONCE_LEN + TAG_LEN - 1]).is_err());
    assert!(EncryptedBlob::from_bytes(&[0u8; NONCE_LEN + TAG_LEN]).is_ok());
}

#[test]
fn nonces_are_unique_across_many_encryptions() {
    let k = key(0x61);
    let mut seen = HashSet::new();
    for _ in 0..2_000 {
        let blob = encrypt(&k, b"message", b"aad").unwrap();
        assert!(seen.insert(blob.nonce), "nonce was reused");
    }
    assert_eq!(seen.len(), 2_000);
}

#[test]
fn key_handle_debug_is_redacted() {
    let handle = kenali_store::keys::KeyHandle::from_bytes([0xA5; KEY_LEN]);
    let dbg = format!("{handle:?}");
    assert_eq!(dbg, "KeyHandle(<redacted>)");
}
