//! `kenali-cp` — COM Credential Provider (`kenali-cp.dll`).
//!
//! This is an FFI crate: `unsafe` is permitted here, but every unsafe block
//! carries a `// SAFETY:` comment. The provider is a P0-S1 spike: it shows one
//! generic tile for workstation unlock only, never submits a credential, and
//! fails open to the password/PIN providers on every entry point.
//!
//! Module-wide reference count: `DllCanUnloadNow` returns `S_OK` only when no
//! COM object created by this DLL is still alive.

mod allowed_sid;
mod credential;
mod guard;
mod kill_switch;
mod provider;
mod scenarios;

use core::sync::atomic::{AtomicI32, Ordering};

/// Live COM objects handed out by this DLL. No `static mut`: all mutation goes
/// through atomics so callbacks on different threads stay race-free.
static CP_REFCOUNT: AtomicI32 = AtomicI32::new(0);

/// Pure helper: LogonUI may unload the module only when the count is zero (or
/// below, which would indicate a bookkeeping bug — still safe to unload).
fn refcount_can_unload(count: i32) -> bool {
    count <= 0
}

#[cfg(windows)]
pub(crate) fn cp_refcount_add() {
    CP_REFCOUNT.fetch_add(1, Ordering::AcqRel);
}

#[cfg(windows)]
pub(crate) fn cp_refcount_release() {
    CP_REFCOUNT.fetch_sub(1, Ordering::AcqRel);
}

// ---------------------------------------------------------------------------
// COM class factory + DLL exports (Windows only)
// ---------------------------------------------------------------------------

#[cfg(windows)]
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, S_FALSE, S_OK,
};
#[cfg(windows)]
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
#[cfg(windows)]
use windows::core::{GUID, HRESULT, Interface};
#[cfg(windows)]
use windows::core::implement;

#[cfg(windows)]
const KENALI_CP_CLSID_GUID: GUID = GUID::from_u128(0x192a78ba_404d_4f82_8625_4b0c11d05f21);

#[cfg(windows)]
#[implement(IClassFactory)]
struct KenaliClassFactory;

#[cfg(windows)]
impl IClassFactory_Impl for KenaliClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: windows::core::Ref<'_, windows::core::IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut core::ffi::c_void,
    ) -> windows::core::Result<()> {
        crate::guard::guard("IClassFactory::CreateInstance", || {
            if !punkouter.is_null() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            if riid.is_null() || ppvobject.is_null() {
                return Err(windows::core::Error::from(E_POINTER));
            }
            // SAFETY: `ppvobject` is a valid writable out-pointer for one
            // pointer; zeroing it guarantees no stale value on a later error.
            unsafe { *ppvobject = core::ptr::null_mut() };

            let provider: windows::Win32::UI::Shell::ICredentialProvider =
                crate::provider::KenaliProvider::new()?.into();
            let mut out: *mut core::ffi::c_void = core::ptr::null_mut();
            // SAFETY: `provider` is a live COM object, `riid` points to a valid
            // GUID and `out` is a valid out-pointer receiving one interface
            // pointer whose reference count has been incremented for us.
            let hr = unsafe { provider.query(riid, &mut out) };
            hr.ok()?;
            // SAFETY: `ppvobject` is a valid writable out-pointer and `out`
            // holds an owned COM reference that is transferred to the caller.
            unsafe { *ppvobject = out };
            Ok(())
        })
    }

    fn LockServer(&self, _flock: windows::core::BOOL) -> windows::core::Result<()> {
        crate::guard::guard("IClassFactory::LockServer", || Ok(()))
    }
}

#[cfg(windows)]
impl Drop for KenaliClassFactory {
    fn drop(&mut self) {
        cp_refcount_release();
    }
}

/// COM DLL entry point: hand the caller a class factory for our CLSID.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn DllGetClassObject(
    rclsid: GUID,
    riid: GUID,
    ppv: *mut *mut core::ffi::c_void,
) -> HRESULT {
    crate::guard::guard("DllGetClassObject", || {
        if rclsid != KENALI_CP_CLSID_GUID {
            return Err(CLASS_E_CLASSNOTAVAILABLE.into());
        }
        if ppv.is_null() {
            return Err(windows::core::Error::from(E_POINTER));
        }
        // SAFETY: `ppv` is a valid writable out-pointer for one pointer;
        // zeroing it keeps the failure path free of stale values.
        unsafe { *ppv = core::ptr::null_mut() };

        let factory: IClassFactory = KenaliClassFactory.into();
        cp_refcount_add();
        let mut out: *mut core::ffi::c_void = core::ptr::null_mut();
        // SAFETY: `factory` is a live COM object, `&riid` points to a valid
        // GUID and `out` is a valid out-pointer receiving one owned interface
        // reference via QueryInterface.
        let hr = unsafe { factory.query(&riid, &mut out) };
        hr.ok()?;
        // SAFETY: `ppv` is a valid writable out-pointer and `out` holds an
        // owned COM reference transferred to the caller (QueryInterface AddRef).
        unsafe { *ppv = out };
        Ok(())
    })
    .into()
}

/// COM DLL entry point: `S_OK` only when no object from this DLL is alive.
#[cfg(windows)]
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    match crate::guard::guard("DllCanUnloadNow", || {
        Ok(refcount_can_unload(CP_REFCOUNT.load(Ordering::Acquire)))
    }) {
        Ok(true) => S_OK,
        _ => S_FALSE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refcount_unload_helper() {
        assert!(refcount_can_unload(0));
        assert!(refcount_can_unload(-1));
        assert!(!refcount_can_unload(1));
        assert!(!refcount_can_unload(i32::MAX));
    }

    #[test]
    fn clsid_string_matches_constant() {
        let expected: u128 = 0x192a78ba_404d_4f82_8625_4b0c11d05f21;
        let digits: String = kenali_winapi::com::KENALI_CP_CLSID
            .chars()
            .filter(|c| *c != '-')
            .collect();
        let parsed = u128::from_str_radix(&digits, 16);
        assert!(matches!(parsed, Ok(value) if value == expected));
    }
}
