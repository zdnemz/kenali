//! Wire containers for values crossing an IPC boundary.
//!
//! # Trust model
//!
//! Every byte on a pipe is hostile. The `Deserialize` implementations below
//! use element-by-element `SeqAccess` visitors: a forged length prefix never
//! pre-allocates a giant buffer, because capacity grows only as real elements
//! arrive and the per-container cap is checked *before* each push. The codec
//! adds a total-message cap checked before deserialization even begins.
//!
//! `PasswordWide` and `SecretWide` also have hand-written redacting `Debug`,
//! no `Clone`, and an explicit `Drop` that wipes the backing units.

use kenali_core::secret::Password;
use serde::de::{Error as DeError, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

use crate::codec::{MAX_PREVIEW_BYTES, MAX_WIDE_UNITS};

/// The single wipe routine used by `Drop` on the secret containers. Keeping it
/// as a named function gives tests a sound, observable proxy: they call it on
/// a buffer they still own instead of reading freed memory after a drop.
fn wipe(units: &mut Vec<u16>) {
    units.zeroize();
}

fn deserialize_wide_units<'de, D>(deserializer: D) -> Result<Vec<u16>, D::Error>
where
    D: Deserializer<'de>,
{
    struct WideUnitsVisitor;

    impl<'de> Visitor<'de> for WideUnitsVisitor {
        type Value = Vec<u16>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a sequence of at most MAX_WIDE_UNITS u16 units")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(unit) = seq.next_element::<u16>()? {
                if out.len() >= MAX_WIDE_UNITS {
                    return Err(A::Error::custom("wide string exceeds MAX_WIDE_UNITS"));
                }
                out.push(unit);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_seq(WideUnitsVisitor)
}

fn deserialize_bytes<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    struct BytesVisitor;

    impl<'de> Visitor<'de> for BytesVisitor {
        type Value = Vec<u8>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a byte sequence within the pinned payload cap")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = Vec::new();
            while let Some(byte) = seq.next_element::<u8>()? {
                if out.len() >= MAX_PREVIEW_BYTES {
                    return Err(A::Error::custom("byte field exceeds MAX_PREVIEW_BYTES"));
                }
                out.push(byte);
            }
            Ok(out)
        }
    }

    deserializer.deserialize_seq(BytesVisitor)
}

/// Non-secret UTF-16 text (user name, domain, labels). The cap is enforced on
/// decode; a caller encoding an over-long value simply produces a message the
/// peer will reject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Wide(Vec<u16>);

impl<'de> Deserialize<'de> for Wide {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(deserialize_wide_units(deserializer)?))
    }
}

impl Wide {
    pub fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    pub fn as_slice(&self) -> &[u16] {
        &self.0
    }

    pub fn into_units(self) -> Vec<u16> {
        self.0
    }
}

/// A UTF-16 password received over the admin pipe. Redacted in `Debug`, never
/// cloned, wiped on drop. Convert to the core [`Password`] secret newtype with
/// [`PasswordWide::into_secret`].
#[derive(Serialize, Eq)]
pub struct PasswordWide(Vec<u16>);

impl fmt::Debug for PasswordWide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PasswordWide(<redacted>)")
    }
}

impl<'de> Deserialize<'de> for PasswordWide {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(deserialize_wide_units(deserializer)?))
    }
}

impl Drop for PasswordWide {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

impl PartialEq for PasswordWide {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.0.as_slice().ct_eq(other.0.as_slice()))
    }
}

impl PasswordWide {
    pub fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    /// Move the units into a zeroizing core secret. The units are *moved*, not
    /// copied: `mem::take` leaves only an empty buffer for `Drop` to wipe.
    pub fn into_secret(mut self) -> Password {
        Password::from_wide(std::mem::take(&mut self.0))
    }
}

/// A UTF-16 secret released over the auth pipe (e.g. the NT hash). Redacted in
/// `Debug`, never cloned, wiped on drop.
#[derive(Serialize, Eq)]
pub struct SecretWide(Vec<u16>);

impl fmt::Debug for SecretWide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretWide(<redacted>)")
    }
}

impl<'de> Deserialize<'de> for SecretWide {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(deserialize_wide_units(deserializer)?))
    }
}

impl Drop for SecretWide {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

impl PartialEq for SecretWide {
    fn eq(&self, other: &Self) -> bool {
        bool::from(self.0.as_slice().ct_eq(other.0.as_slice()))
    }
}

impl SecretWide {
    pub fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    /// Consume and return the units for immediate encryption. No log/format
    /// path touches them.
    pub fn into_units(mut self) -> Vec<u16> {
        std::mem::take(&mut self.0)
    }
}

/// Non-secret byte payload: preview RGB pixels or a diagnostics ZIP chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bytes(Vec<u8>);

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(deserialize_bytes(deserializer)?))
    }
}

impl Bytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

/// Wrap a core secret for transmission. An inherent `impl Password` cannot
/// live in this crate (orphan rule), so this free function is the counterpart
/// to [`PasswordWide::into_secret`].
pub fn password_into_wide(password: Password) -> PasswordWide {
    PasswordWide(password.into_wide_units())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wipe_zeroizes_the_backing_units() {
        // Reading freed memory after a drop would be unsound. Instead we test
        // the exact `wipe` routine Drop calls, on a buffer we still own.
        let mut units = vec![0x0041u16, 0x0042, 0x0043];
        wipe(&mut units);
        assert!(units.iter().all(|&unit| unit == 0));
    }
}
