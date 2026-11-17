//! CNG/TPM implementation of [`kenali_store::keys::KeyHierarchy`].
//!
//! A machine-scoped, non-exportable RSA-2048 key wraps the 32-byte data
//! encryption key (DEK) with RSA-OAEP-SHA256. The default provider is the
//! TPM-backed `MS_PLATFORM_CRYPTO_PROVIDER`; the software
//! `MS_KEY_STORAGE_PROVIDER` is used only when the caller explicitly opts in
//! via `allow_weak_keystore` (a loud warning is emitted). The persisted key
//! is finalised before it is used and `NCRYPT_MACHINE_KEY_FLAG` is applied
//! consistently on both create and open. All CNG errors collapse into
//! generic [`KeyError`] variants so no key material or provider detail ever
//! reaches a log or error message.

use kenali_store::keys::{KeyHandle, KeyError, KeyHierarchy};

/// Persisted-key name used for the DEK wrapping key. Machine-scoped.
const WRAP_KEY_NAME: &str = "KenaliDekWrapKey";
/// RSA modulus size of the wrapping key, in bytes (RSA-2048).
const RSA_MODULUS_BYTES: usize = 256;

/// A CNG-backed key hierarchy. Construction goes through [`TpmKeyHierarchy::open`].
pub struct TpmKeyHierarchy {
    #[cfg(windows)]
    provider: windows::Win32::Security::Cryptography::NCRYPT_PROV_HANDLE,
    #[cfg(windows)]
    key: windows::Win32::Security::Cryptography::NCRYPT_KEY_HANDLE,
    #[cfg(not(windows))]
    _not_windows: (),
}

// CNG handles are plain integer newtypes and are safe to move between threads.
#[cfg(windows)]
unsafe impl Send for TpmKeyHierarchy {}

impl TpmKeyHierarchy {
    /// Open (or create) the machine-scoped wrapping key.
    ///
    /// When `allow_weak_keystore` is false the TPM platform provider is used.
    /// When it is true the software provider is used instead and a loud
    /// warning is emitted: the resulting protection is weaker and is only
    /// appropriate for dev/test.
    #[cfg(windows)]
    pub fn open(allow_weak_keystore: bool) -> Result<Self, KeyError> {
        use windows::Win32::Security::Cryptography::{
            NCryptCreatePersistedKey, NCryptFinalizeKey, NCryptOpenKey, NCryptOpenStorageProvider,
            CERT_KEY_SPEC, MS_KEY_STORAGE_PROVIDER, MS_PLATFORM_CRYPTO_PROVIDER,
            NCRYPT_EXPORT_POLICY_PROPERTY, NCRYPT_FLAGS, NCRYPT_KEY_HANDLE,
            NCRYPT_LENGTH_PROPERTY, NCRYPT_MACHINE_KEY_FLAG, NCRYPT_RSA_ALGORITHM,
        };

        let provider_name = if allow_weak_keystore {
            tracing::warn!(
                "weak software keystore selected via allow_weak_keystore; \
                 DEK wrapping is NOT TPM-protected"
            );
            MS_KEY_STORAGE_PROVIDER
        } else {
            MS_PLATFORM_CRYPTO_PROVIDER
        };

        let mut provider = Default::default();
        // SAFETY: `provider` is a valid out-pointer, `provider_name` is a
        // static NUL-terminated wide string and 0 selects default flags.
        unsafe { NCryptOpenStorageProvider(&mut provider, provider_name, 0) }
            .map_err(|_| KeyError::Unavailable)?;

        let name: Vec<u16> = WRAP_KEY_NAME.encode_utf16().chain(core::iter::once(0)).collect();
        let name_ptr = windows::core::PCWSTR(name.as_ptr());
        let mut key = NCRYPT_KEY_HANDLE(0);

        // Prefer an existing persisted key; only create when none is present.
        // SAFETY: `provider` is a valid provider handle, `name` is a live
        // NUL-terminated wide string and `key` is a valid out-pointer.
        let opened = unsafe {
            NCryptOpenKey(provider, &mut key, name_ptr, CERT_KEY_SPEC(0), NCRYPT_MACHINE_KEY_FLAG)
        };

        if opened.is_err() {
            // SAFETY: `provider` is valid, `name` is NUL-terminated and alive,
            // `key` is a valid out-pointer and the flags mark the key as
            // machine-scoped.
            if unsafe {
                NCryptCreatePersistedKey(
                    provider,
                    &mut key,
                    NCRYPT_RSA_ALGORITHM,
                    name_ptr,
                    CERT_KEY_SPEC(0),
                    NCRYPT_MACHINE_KEY_FLAG,
                )
            }
            .is_err()
            {
                // SAFETY: `provider` is owned by this function and open.
                let _ = unsafe {
                    windows::Win32::Security::Cryptography::NCryptFreeObject(provider.into())
                };
                return Err(KeyError::Unavailable);
            }

            // RSA-2048. NCRYPT_LENGTH_PROPERTY takes a DWORD (u32).
            // SAFETY: `key` is a valid key handle; the byte slice is the
            // little-endian u32 2048 and outlives the call.
            if unsafe {
                windows::Win32::Security::Cryptography::NCryptSetProperty(
                    key.into(),
                    NCRYPT_LENGTH_PROPERTY,
                    &2048u32.to_le_bytes(),
                    NCRYPT_FLAGS(0),
                )
            }
            .is_err()
            {
                // SAFETY: both handles are owned by this function and open.
                let _ = unsafe {
                    let _ = windows::Win32::Security::Cryptography::NCryptFreeObject(key.into());
                    let _ =
                        windows::Win32::Security::Cryptography::NCryptFreeObject(provider.into());
                };
                return Err(KeyError::Unavailable);
            }

            // Export policy 0 makes the key non-exportable.
            // SAFETY: `key` is valid and the slice is a live u32 byte buffer.
            if unsafe {
                windows::Win32::Security::Cryptography::NCryptSetProperty(
                    key.into(),
                    NCRYPT_EXPORT_POLICY_PROPERTY,
                    &0u32.to_le_bytes(),
                    NCRYPT_FLAGS(0),
                )
            }
            .is_err()
            {
                // SAFETY: both handles are owned by this function and open.
                let _ = unsafe {
                    let _ = windows::Win32::Security::Cryptography::NCryptFreeObject(key.into());
                    let _ =
                        windows::Win32::Security::Cryptography::NCryptFreeObject(provider.into());
                };
                return Err(KeyError::Unavailable);
            }

            // The key is not stored or usable until it is finalised.
            // SAFETY: `key` is a valid key handle created above.
            if unsafe { NCryptFinalizeKey(key, NCRYPT_FLAGS(0)) }.is_err() {
                // SAFETY: both handles are owned by this function and open.
                let _ = unsafe {
                    let _ = windows::Win32::Security::Cryptography::NCryptFreeObject(key.into());
                    let _ =
                        windows::Win32::Security::Cryptography::NCryptFreeObject(provider.into());
                };
                return Err(KeyError::Unavailable);
            }
        }

        Ok(Self { provider, key })
    }

    /// Host stub: no CNG provider exists off Windows.
    #[cfg(not(windows))]
    pub fn open(_allow_weak_keystore: bool) -> Result<Self, KeyError> {
        Err(KeyError::Unavailable)
    }
}

impl KeyHierarchy for TpmKeyHierarchy {
    #[cfg(windows)]
    fn wrap_dek(&self, dek: &KeyHandle) -> Result<Vec<u8>, KeyError> {
        use windows::Win32::Security::Cryptography::{
            NCryptEncrypt, BCRYPT_OAEP_PADDING_INFO, BCRYPT_SHA256_ALGORITHM, NCRYPT_PAD_OAEP_FLAG,
        };

        let padding = BCRYPT_OAEP_PADDING_INFO {
            pszAlgId: BCRYPT_SHA256_ALGORITHM,
            pbLabel: core::ptr::null_mut(),
            cbLabel: 0,
        };
        let padding_ptr = &padding as *const BCRYPT_OAEP_PADDING_INFO as *const core::ffi::c_void;
        let plaintext = &dek.as_key_bytes()[..];

        // First call queries the required ciphertext length.
        let mut needed = 0u32;
        // SAFETY: `self.key` is valid, `plaintext` is a live 32-byte slice,
        // `padding_ptr` points at a live padding-info struct and `needed` is a
        // valid out-pointer. A NULL output requests the size only.
        unsafe {
            NCryptEncrypt(
                self.key,
                Some(plaintext),
                Some(padding_ptr),
                None,
                &mut needed,
                NCRYPT_PAD_OAEP_FLAG,
            )
        }
        .map_err(|_| KeyError::Wrap)?;

        if needed == 0 || needed as usize > RSA_MODULUS_BYTES {
            return Err(KeyError::Wrap);
        }

        // The ciphertext is not secret, so a plain buffer is sufficient.
        let mut wrapped = vec![0u8; needed as usize];
        // SAFETY: `wrapped` is a live allocation of `needed` bytes and all
        // other arguments are the same valid values as the sizing call.
        unsafe {
            NCryptEncrypt(
                self.key,
                Some(plaintext),
                Some(padding_ptr),
                Some(wrapped.as_mut_slice()),
                &mut needed,
                NCRYPT_PAD_OAEP_FLAG,
            )
        }
        .map_err(|_| KeyError::Wrap)?;

        Ok(wrapped)
    }

    #[cfg(not(windows))]
    fn wrap_dek(&self, _dek: &KeyHandle) -> Result<Vec<u8>, KeyError> {
        Err(KeyError::Unavailable)
    }

    #[cfg(windows)]
    fn unwrap_dek(&self, wrapped: &[u8]) -> Result<KeyHandle, KeyError> {
        use zeroize::Zeroizing;

        use windows::Win32::Security::Cryptography::{
            NCryptDecrypt, BCRYPT_OAEP_PADDING_INFO, BCRYPT_SHA256_ALGORITHM, NCRYPT_PAD_OAEP_FLAG,
        };

        if wrapped.is_empty() || wrapped.len() > RSA_MODULUS_BYTES {
            return Err(KeyError::Malformed);
        }

        let padding = BCRYPT_OAEP_PADDING_INFO {
            pszAlgId: BCRYPT_SHA256_ALGORITHM,
            pbLabel: core::ptr::null_mut(),
            cbLabel: 0,
        };
        let padding_ptr = &padding as *const BCRYPT_OAEP_PADDING_INFO as *const core::ffi::c_void;

        // The recovered plaintext is secret: hold it in a Zeroizing buffer so
        // it is wiped on every return path (including errors).
        let mut plain = Zeroizing::new(vec![0u8; RSA_MODULUS_BYTES]);
        let mut written = 0u32;
        // SAFETY: `self.key` is valid, `wrapped` is a live slice, `padding_ptr`
        // points at a live struct, `plain` is a live writable buffer of
        // RSA_MODULUS_BYTES and `written` is a valid out-pointer.
        unsafe {
            NCryptDecrypt(
                self.key,
                Some(wrapped),
                Some(padding_ptr),
                Some(plain.as_mut_slice()),
                &mut written,
                NCRYPT_PAD_OAEP_FLAG,
            )
        }
        .map_err(|_| KeyError::Unwrap)?;

        let len = written as usize;
        // A 32-byte DEK is exactly KEY_LEN; anything else is malformed.
        if len != kenali_store::aead::KEY_LEN || len > plain.len() {
            return Err(KeyError::Malformed);
        }

        let mut key_bytes = Zeroizing::new([0u8; kenali_store::aead::KEY_LEN]);
        key_bytes.copy_from_slice(&plain[..len]);
        Ok(KeyHandle::from_bytes(*key_bytes))
    }

    #[cfg(not(windows))]
    fn unwrap_dek(&self, _wrapped: &[u8]) -> Result<KeyHandle, KeyError> {
        Err(KeyError::Unavailable)
    }
}

#[cfg(windows)]
impl Drop for TpmKeyHierarchy {
    fn drop(&mut self) {
        // SAFETY: both handles are owned by this value and open until drop.
        unsafe {
            let _ = windows::Win32::Security::Cryptography::NCryptFreeObject(self.key.into());
            let _ = windows::Win32::Security::Cryptography::NCryptFreeObject(self.provider.into());
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn wrapping_key_name_is_fixed() {
        assert_eq!(WRAP_KEY_NAME, "KenaliDekWrapKey");
    }

    #[test]
    fn error_display_never_mentions_keys_or_secrets() {
        for error in [
            KeyError::Random,
            KeyError::Wrap,
            KeyError::Unwrap,
            KeyError::Malformed,
            KeyError::Unavailable,
        ] {
            let text = error.to_string();
            assert!(!text.contains("secret"));
            assert!(!text.contains("password"));
        }
    }
}
