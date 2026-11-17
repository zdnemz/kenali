//! `ICredentialProvider` implementation for the P0-S1 spike.
//!
//! Supports only workstation unlock. The single tile is shown only when the
//! core policy allows the scenario, the kill switch is off and the current
//! session SID is enrolled; otherwise `GetCredentialCount` returns zero. The
//! password and PIN providers are never touched (no filter is installed).

#![cfg(windows)]

use std::sync::Mutex;

use kenali_core::tiles::tile_text;
use kenali_core::types::Phase;
use windows::Win32::Foundation::{E_INVALIDARG, E_NOTIMPL, E_POINTER, E_UNEXPECTED};
use windows::core::BOOL;
use windows::Win32::UI::Shell::{
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION, CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR,
    CPFT_LARGE_TEXT, ICredentialProvider, ICredentialProviderCredential, ICredentialProviderEvents,
    ICredentialProvider_Impl,
};
use windows::core::{
    Error, GUID, PWSTR, Ref, Result, implement,
};

use crate::credential::KenaliCredential;
use crate::guard::guard;
use crate::{allowed_sid, cp_refcount_add, cp_refcount_release, kill_switch, scenarios};

struct ProviderState {
    /// Raw `CREDENTIAL_PROVIDER_USAGE_SCENARIO.0` from `SetUsageScenario`.
    scenario: Option<i32>,
}

#[implement(ICredentialProvider)]
pub struct KenaliProvider {
    state: Mutex<ProviderState>,
    /// NUL-terminated label backing `descriptor.pszLabel`; its heap address is
    /// stable for the lifetime of the provider. Kept alive (never re-read) so
    /// the raw pointer in `descriptor` stays valid.
    _label: Box<[u16]>,
    descriptor: Box<CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR>,
}

impl KenaliProvider {
    /// Build the provider and register it in the module ref-count.
    pub fn new() -> Result<Self> {
        let label: Box<[u16]> = tile_text(Phase::Armed)
            .encode_utf16()
            .chain(core::iter::once(0))
            .collect::<Vec<u16>>()
            .into_boxed_slice();
        let descriptor = CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR {
            dwFieldID: 0,
            cpft: CPFT_LARGE_TEXT,
            // SAFETY-free construction: `PWSTR` is a transparent pointer newtype;
            // it is only read while `self._label` is alive (the provider's life).
            pszLabel: PWSTR(label.as_ptr() as *mut u16),
            guidFieldType: GUID::zeroed(),
        };
        let provider = Self {
            state: Mutex::new(ProviderState { scenario: None }),
            _label: label,
            descriptor: Box::new(descriptor),
        };
        cp_refcount_add();
        Ok(provider)
    }

    /// All three gates must pass before any tile may be offered.
    fn tile_visible(&self) -> bool {
        let state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => return false,
        };
        match state.scenario {
            Some(raw) => {
                scenarios::scenario_allowed(raw)
                    && !kill_switch::kill_switch_enabled()
                    && allowed_sid::current_session_is_allowed()
            }
            None => false,
        }
    }
}

impl Drop for KenaliProvider {
    fn drop(&mut self) {
        cp_refcount_release();
    }
}

impl ICredentialProvider_Impl for KenaliProvider_Impl {
    fn SetUsageScenario(
        &self,
        cpus: windows::Win32::UI::Shell::CREDENTIAL_PROVIDER_USAGE_SCENARIO,
        _dwflags: u32,
    ) -> Result<()> {
        guard("ICredentialProvider::SetUsageScenario", || {
            let raw = cpus.0;
            scenarios::decline_unsupported(raw)?;
            let mut state = self.state.lock().map_err(|_| Error::from(E_UNEXPECTED))?;
            state.scenario = Some(raw);
            Ok(())
        })
    }

    fn SetSerialization(
        &self,
        _pcpcs: *const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
    ) -> Result<()> {
        guard("ICredentialProvider::SetSerialization", || {
            // RDP / CredUI serialization is not supported; decline it.
            Err(Error::from(E_NOTIMPL))
        })
    }

    fn Advise(
        &self,
        _pcpe: Ref<'_, ICredentialProviderEvents>,
        _upadvisecontext: usize,
    ) -> Result<()> {
        guard("ICredentialProvider::Advise", || Ok(()))
    }

    fn UnAdvise(&self) -> Result<()> {
        guard("ICredentialProvider::UnAdvise", || Ok(()))
    }

    fn GetFieldDescriptorCount(&self) -> Result<u32> {
        guard("ICredentialProvider::GetFieldDescriptorCount", || Ok(1))
    }

    fn GetFieldDescriptorAt(
        &self,
        dwindex: u32,
    ) -> Result<*mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR> {
        guard("ICredentialProvider::GetFieldDescriptorAt", || {
            if dwindex != 0 {
                return Err(Error::from(E_INVALIDARG));
            }
            Ok(&*self.descriptor as *const CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR
                as *mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR)
        })
    }

    fn GetCredentialCount(
        &self,
        pdwcount: *mut u32,
        pdwdefault: *mut u32,
        pbautologonwithdefault: *mut BOOL,
    ) -> Result<()> {
        guard("ICredentialProvider::GetCredentialCount", || {
            if pdwcount.is_null() || pdwdefault.is_null() || pbautologonwithdefault.is_null() {
                return Err(Error::from(E_POINTER));
            }
            // SAFETY: all three pointers were just checked non-null and are
            // valid writable out-pointers for exactly one value each.
            unsafe {
                *pdwdefault = 0;
                *pbautologonwithdefault = BOOL(0);
                *pdwcount = if self.tile_visible() { 1 } else { 0 };
            }
            Ok(())
        })
    }

    fn GetCredentialAt(&self, dwindex: u32) -> Result<ICredentialProviderCredential> {
        guard("ICredentialProvider::GetCredentialAt", || {
            if dwindex != 0 || !self.tile_visible() {
                return Err(Error::from(E_INVALIDARG));
            }
            Ok(KenaliCredential::new()?.into())
        })
    }
}
