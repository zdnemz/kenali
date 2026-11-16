//! AAD construction: determinism and domain separation.

use kenali_core::types::{FileType, Sid};
use kenali_store::format::{aad_for, MAGIC, FORMAT_VERSION};

#[test]
fn aad_is_deterministic() {
    let a = aad_for(FileType::Secret, &Sid::new("S-1-5-21-1"));
    let b = aad_for(FileType::Secret, &Sid::new("S-1-5-21-1"));
    assert_eq!(a, b);
}

#[test]
fn aad_layout_starts_with_magic_version_and_type() {
    let aad = aad_for(FileType::Secret, &Sid::new("S-1-5-21-1"));
    assert_eq!(&aad[..MAGIC.len()], MAGIC);
    let rest = &aad[MAGIC.len()..];
    assert_eq!(&rest[..2], &FORMAT_VERSION.to_le_bytes());
    assert_eq!(rest[2], 2); // Secret discriminant
}

#[test]
fn different_sids_give_different_aad() {
    let a = aad_for(FileType::Template, &Sid::new("S-1-5-21-1"));
    let b = aad_for(FileType::Template, &Sid::new("S-1-5-21-2"));
    let c = aad_for(FileType::Template, &Sid::new(""));
    assert_ne!(a, b);
    assert_ne!(a, c);
    assert_ne!(b, c);
}

#[test]
fn different_file_types_give_different_aad() {
    let sid = Sid::new("S-1-5-21-1");
    let variants = [
        FileType::Template,
        FileType::Secret,
        FileType::Config,
        FileType::State,
    ];
    for (i, a) in variants.iter().enumerate() {
        for b in variants.iter().skip(i + 1) {
            assert_ne!(aad_for(*a, &sid), aad_for(*b, &sid));
        }
    }
}

#[test]
fn sid_is_encoded_as_utf16le_with_length_prefix() {
    let sid = Sid::new("A\u{00E9}"); // A + é (non-ASCII -> distinct UTF-16 units)
    let aad = aad_for(FileType::Config, &sid);
    let units: Vec<u16> = sid.as_str().encode_utf16().collect();
    let mut expected = Vec::new();
    for u in units {
        expected.extend_from_slice(&u.to_le_bytes());
    }
    assert!(aad.ends_with(&expected));
    // The u64 length prefix immediately precedes the SID bytes.
    let prefix_at = aad.len() - expected.len() - 8;
    let len = u64::from_le_bytes(aad[prefix_at..prefix_at + 8].try_into().unwrap());
    assert_eq!(len, expected.len() as u64);
}

#[test]
fn length_prefix_prevents_sid_collisions() {
    // Without a length prefix, ("AB", "") and ("A", "B") style concatenations
    // could collide. The explicit prefix keeps them distinct.
    let a = aad_for(FileType::State, &Sid::new("AB"));
    let b = aad_for(FileType::State, &Sid::new("A\0B"));
    assert_ne!(a, b);
}
