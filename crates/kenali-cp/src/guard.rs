//! Panic firewall for every COM entry point in the credential provider.
//!
//! LogonUI calls into the DLL from a host we do not control; a Rust panic
//! crossing an `extern "system"` boundary aborts the process. Every COM method
//! is wrapped in [`guard`], which converts a panic into `E_UNEXPECTED` and
//! logs *only* the static method name — never the panic payload (which could
//! contain secret material).

#![cfg(windows)]

use std::panic::{catch_unwind, AssertUnwindSafe};

use windows::Win32::Foundation::E_UNEXPECTED;
use windows::core::{Error, Result};

/// Run `f` inside a panic firewall.
///
/// A panic is turned into `E_UNEXPECTED`. The panic payload is deliberately
/// dropped without inspection or logging; only `what` (the static COM method
/// name) is recorded. A normal `Err` from `f` passes through unchanged.
pub fn guard<T>(what: &'static str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_panic_payload) => {
            tracing::error!(target: "kenali_cp", "panic in COM entry point: {}", what);
            Err(Error::from(E_UNEXPECTED))
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn guard_passes_through_ok() {
        let result: Result<u32> = guard("test", || Ok(7));
        assert!(matches!(result, Ok(7)));
    }

    #[test]
    fn guard_passes_through_error_unchanged() {
        let result: Result<u32> = guard("test", || Err(Error::from(E_UNEXPECTED)));
        assert!(result.is_err());
        assert_eq!(result.err().map_or(0, |e| e.code()), E_UNEXPECTED);
    }
}
