//! Read-only kill-switch check. The credential provider reads this value at
//! session creation; no code in this crate ever writes the registry.

use thiserror::Error;

/// Exact registry path string used by the kill switch.
pub const KENALI_REGISTRY_PATH: &str = "HKLM\\SOFTWARE\\Kenali";
/// Value name read under [`KENALI_REGISTRY_PATH`].
pub const KENALI_DISABLED_VALUE: &str = "Disabled";

/// Errors from the read-only registry probe. Wording is intentionally generic.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryError {
    #[error("registry key is unavailable")]
    Unavailable,
    #[error("registry read failed")]
    ReadFailed,
    #[error("unexpected registry value type")]
    InvalidType,
}

/// Return `true` when `HKLM\SOFTWARE\Kenali\Disabled` is the DWORD `1`.
///
/// A missing key/value means "not disabled" (`Ok(false)`); only a genuine read
/// failure is an error. The check is read-only and bounded (no registry APIs
/// here block on external input).
#[cfg(windows)]
pub fn kill_switch_enabled() -> Result<bool, RegistryError> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, KEY_QUERY_VALUE, REG_DWORD,
        REG_VALUE_TYPE, HKEY_LOCAL_MACHINE,
    };
    use windows::core::PCWSTR;

    let subkey: Vec<u16> = "SOFTWARE\\Kenali"
        .encode_utf16()
        .chain(core::iter::once(0))
        .collect();
    let value_name: Vec<u16> = KENALI_DISABLED_VALUE
        .encode_utf16()
        .chain(core::iter::once(0))
        .collect();
    // SAFETY: both Vecs are NUL-terminated above and remain alive for the
    // whole function, so the PCWSTRs are valid for every call below.
    let subkey = PCWSTR(subkey.as_ptr());
    // SAFETY: see above — `value_name` outlives this binding's use.
    let value_name = PCWSTR(value_name.as_ptr());

    let mut key = HKEY(core::ptr::null_mut());
    // SAFETY: `subkey` is a valid NUL-terminated wide string, `key` is a valid
    // out-pointer, and KEY_QUERY_VALUE is a read-only access mask.
    let status = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, subkey, None, KEY_QUERY_VALUE, &mut key) };
    if status == ERROR_FILE_NOT_FOUND_SENTINEL {
        return Ok(false);
    }
    if status != ERROR_SUCCESS {
        return Err(RegistryError::Unavailable);
    }

    let mut value = 0u32;
    let mut value_type = REG_VALUE_TYPE(0);
    let mut size = core::mem::size_of::<u32>() as u32;
    // SAFETY: `key` is a valid handle from RegOpenKeyExW, `value_name` is a
    // valid NUL-terminated wide string, and `value`/`value_type`/`size` are
    // valid out-pointers of the correct types. The data pointer is a valid
    // 4-byte allocation.
    let status = unsafe {
        RegQueryValueExW(
            key,
            value_name,
            None,
            Some(&mut value_type),
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        )
    };
    // SAFETY: `key` is a valid handle opened above and not yet closed.
    let _ = unsafe { RegCloseKey(key) };

    if status == ERROR_FILE_NOT_FOUND_SENTINEL {
        return Ok(false);
    }
    if status != ERROR_SUCCESS {
        return Err(RegistryError::ReadFailed);
    }
    if value_type != REG_DWORD || size < core::mem::size_of::<u32>() as u32 {
        return Err(RegistryError::InvalidType);
    }
    Ok(value == 1)
}

/// `ERROR_FILE_NOT_FOUND`, imported lazily to keep the hot path readable.
#[cfg(windows)]
const ERROR_FILE_NOT_FOUND_SENTINEL: windows::Win32::Foundation::WIN32_ERROR =
    windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_path_and_value_are_stable() {
        assert_eq!(KENALI_REGISTRY_PATH, "HKLM\\SOFTWARE\\Kenali");
        assert_eq!(KENALI_DISABLED_VALUE, "Disabled");
    }

    #[test]
    fn error_display_is_generic() {
        for error in [
            RegistryError::Unavailable,
            RegistryError::ReadFailed,
            RegistryError::InvalidType,
        ] {
            let text = error.to_string();
            assert!(!text.contains("secret"));
            assert!(!text.contains("password"));
        }
    }
}
