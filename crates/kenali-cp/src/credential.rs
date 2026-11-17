//! `ICredentialProviderCredential` implementation for the P0-S1 spike.
//!
//! Renders generic, score-free wording only from `kenali_core` constants. The
//! spike tile never submits a credential: `GetSerialization` always returns
//! `CPGSR_NO_CREDENTIAL_FINISHED` with generic fallback text, so the user
//! falls through to the password/PIN providers. Secret state is held in a
//! zeroizing buffer and scrubbed on `SetDeselected` and drop.

#![cfg(windows)]

use std::sync::Mutex;

use zeroize::{Zeroize, Zeroizing};

use kenali_core::tiles::tile_text;
use kenali_core::types::{Phase, Scenario};
use kenali_ipc::auth::{Request, Response};
use kenali_winapi::pipes::{AUTH_CONNECT_TIMEOUT, AuthPipeClient};
use windows::Win32::Foundation::{
    E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY, E_POINTER, E_UNEXPECTED, NTSTATUS,
};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::System::Com::CoTaskMemAlloc;
use windows::Win32::UI::Shell::{
    CPFS_DISPLAY_IN_BOTH, CPGSR_NO_CREDENTIAL_FINISHED, CPSI_NONE,
    CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION, CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
    CREDENTIAL_PROVIDER_FIELD_STATE, CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE,
    CREDENTIAL_PROVIDER_STATUS_ICON, ICredentialProviderCredential,
    ICredentialProviderCredentialEvents, ICredentialProviderCredential_Impl,
};
use windows::core::{Error, PCWSTR, PWSTR, Ref, Result, BOOL, implement};

use crate::guard::guard;
use crate::{allowed_sid, cp_refcount_add, cp_refcount_release};

/// The spike tile is display-only; it never produces a logon credential.
const SPIKE_NEVER_SUBMIT: bool = true;

struct CredentialState {
    /// UTF-16 secret captured from `SetStringValue`; wiped on every path.
    password: Zeroizing<Vec<u16>>,
    /// Nonce of the current attempt, used to verify the echoed pipe response.
    nonce: Option<[u8; 16]>,
}

impl CredentialState {
    /// Scrub all secret state. Idempotent and safe to call repeatedly.
    fn scrub(&mut self) {
        self.password.zeroize();
        self.nonce = None;
    }
}

#[implement(ICredentialProviderCredential)]
pub struct KenaliCredential {
    state: Mutex<CredentialState>,
}

impl KenaliCredential {
    /// Build the credential and register it in the module ref-count.
    pub fn new() -> Result<Self> {
        let credential = Self {
            state: Mutex::new(CredentialState {
                password: Zeroizing::new(Vec::new()),
                nonce: None,
            }),
        };
        cp_refcount_add();
        Ok(credential)
    }

    /// Wire the bounded auth-pipe exchange: connect, begin an attempt, verify
    /// the echoed nonce and release the one-shot secret on success.
    ///
    /// The spike never calls this (see [`SPIKE_NEVER_SUBMIT`]); it is kept so
    /// the pipe path is type-checked and ready for a non-spike tile.
    fn try_submit(&self) -> Result<()> {
        let sid = match allowed_sid::current_sid() {
            Some(sid) => sid,
            None => return Err(Error::from(E_UNEXPECTED)),
        };
        let nonce = match self.state.lock() {
            Ok(state) => state.nonce.unwrap_or([0u8; 16]),
            Err(_) => return Err(Error::from(E_UNEXPECTED)),
        };
        let request = Request::BeginAttempt {
            scenario: Scenario::Unlock,
            sid,
            nonce,
        };
        let mut client = AuthPipeClient::connect(AUTH_CONNECT_TIMEOUT)
            .map_err(|_| Error::from(E_NOTIMPL))?;
        let response = client
            .exchange(&request)
            .map_err(|_| Error::from(E_NOTIMPL))?;
        match response {
            Response::Verified {
                nonce: echoed,
                secret,
                ..
            } => {
                if echoed != nonce {
                    return Err(Error::from(E_UNEXPECTED));
                }
                // One-shot release: dropping the secret zeroizes it.
                drop(secret);
                Ok(())
            }
            _ => Err(Error::from(E_NOTIMPL)),
        }
    }
}

impl Drop for KenaliCredential {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.scrub();
        }
        cp_refcount_release();
    }
}

/// Allocate a NUL-terminated COM status string that LogonUI frees with
/// `CoTaskMemFree`.
fn alloc_com_wide(text: &str) -> Result<PWSTR> {
    let units: Vec<u16> = text.encode_utf16().chain(core::iter::once(0)).collect();
    let byte_len = units
        .len()
        .checked_mul(2)
        .ok_or_else(|| Error::from(E_OUTOFMEMORY))?;
    // SAFETY: `CoTaskMemAlloc` either returns null or a writable allocation of
    // at least `byte_len` bytes; `byte_len` is nonzero (we appended a NUL).
    let raw = unsafe { CoTaskMemAlloc(byte_len) };
    if raw.is_null() {
        return Err(Error::from(E_OUTOFMEMORY));
    }
    // SAFETY: `raw` is a writable allocation of `byte_len` bytes and `units`
    // has exactly `byte_len / 2` elements, so the copy is fully in bounds.
    unsafe {
        core::ptr::copy_nonoverlapping(units.as_ptr(), raw.cast::<u16>(), units.len());
    }
    Ok(PWSTR(raw.cast::<u16>()))
}

impl ICredentialProviderCredential_Impl for KenaliCredential_Impl {
    fn Advise(
        &self,
        _pcpce: Ref<'_, ICredentialProviderCredentialEvents>,
    ) -> Result<()> {
        guard("ICredentialProviderCredential::Advise", || Ok(()))
    }

    fn UnAdvise(&self) -> Result<()> {
        guard("ICredentialProviderCredential::UnAdvise", || Ok(()))
    }

    fn SetSelected(&self) -> Result<BOOL> {
        guard("ICredentialProviderCredential::SetSelected", || Ok(BOOL(0)))
    }

    fn SetDeselected(&self) -> Result<()> {
        guard("ICredentialProviderCredential::SetDeselected", || {
            let mut state = self.state.lock().map_err(|_| Error::from(E_UNEXPECTED))?;
            state.scrub();
            Ok(())
        })
    }

    fn GetFieldState(
        &self,
        dwfieldid: u32,
        pcpfs: *mut CREDENTIAL_PROVIDER_FIELD_STATE,
        pcpfis: *mut CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE,
    ) -> Result<()> {
        guard("ICredentialProviderCredential::GetFieldState", || {
            if dwfieldid != 0 || pcpfs.is_null() || pcpfis.is_null() {
                return Err(Error::from(E_INVALIDARG));
            }
            // SAFETY: both pointers were checked non-null and are valid
            // writable out-pointers for one value each.
            unsafe {
                *pcpfs = CPFS_DISPLAY_IN_BOTH;
                *pcpfis = CREDENTIAL_PROVIDER_FIELD_INTERACTIVE_STATE(0);
            }
            Ok(())
        })
    }

    fn GetStringValue(&self, dwfieldid: u32) -> Result<PWSTR> {
        guard("ICredentialProviderCredential::GetStringValue", || {
            if dwfieldid != 0 {
                return Err(Error::from(E_INVALIDARG));
            }
            alloc_com_wide(tile_text(Phase::Armed))
        })
    }

    fn GetBitmapValue(&self, _dwfieldid: u32) -> Result<HBITMAP> {
        guard("ICredentialProviderCredential::GetBitmapValue", || {
            Ok(HBITMAP(core::ptr::null_mut()))
        })
    }

    fn GetCheckboxValue(
        &self,
        dwfieldid: u32,
        pbchecked: *mut BOOL,
        ppszlabel: *mut PWSTR,
    ) -> Result<()> {
        guard("ICredentialProviderCredential::GetCheckboxValue", || {
            if dwfieldid != 0 || pbchecked.is_null() || ppszlabel.is_null() {
                return Err(Error::from(E_INVALIDARG));
            }
            // SAFETY: both pointers were checked non-null and are valid
            // writable out-pointers for one value each.
            unsafe {
                *pbchecked = BOOL(0);
                *ppszlabel = PWSTR(core::ptr::null_mut());
            }
            Ok(())
        })
    }

    fn GetSubmitButtonValue(&self, _dwfieldid: u32) -> Result<u32> {
        guard("ICredentialProviderCredential::GetSubmitButtonValue", || {
            Err(Error::from(E_NOTIMPL))
        })
    }

    fn GetComboBoxValueCount(
        &self,
        dwfieldid: u32,
        pcitems: *mut u32,
        pdwselecteditem: *mut u32,
    ) -> Result<()> {
        guard("ICredentialProviderCredential::GetComboBoxValueCount", || {
            if dwfieldid != 0 || pcitems.is_null() || pdwselecteditem.is_null() {
                return Err(Error::from(E_INVALIDARG));
            }
            // SAFETY: both pointers were checked non-null and are valid
            // writable out-pointers for one value each.
            unsafe {
                *pcitems = 0;
                *pdwselecteditem = 0;
            }
            Ok(())
        })
    }

    fn GetComboBoxValueAt(&self, _dwfieldid: u32, _dwitem: u32) -> Result<PWSTR> {
        guard("ICredentialProviderCredential::GetComboBoxValueAt", || {
            Err(Error::from(E_NOTIMPL))
        })
    }

    fn SetStringValue(&self, dwfieldid: u32, psz: &PCWSTR) -> Result<()> {
        guard("ICredentialProviderCredential::SetStringValue", || {
            if dwfieldid != 0 || psz.is_null() {
                return Err(Error::from(E_INVALIDARG));
            }
            // SAFETY: the COM contract guarantees `psz` points to a
            // NUL-terminated wide string valid for the duration of this call.
            let wide = unsafe { psz.as_wide() };
            let mut state = self.state.lock().map_err(|_| Error::from(E_UNEXPECTED))?;
            // Overwrite (and thereby zeroize) any previously held secret.
            state.password.zeroize();
            state.password.clear();
            state.password.extend_from_slice(wide);
            Ok(())
        })
    }

    fn SetCheckboxValue(&self, _dwfieldid: u32, _bchecked: BOOL) -> Result<()> {
        guard("ICredentialProviderCredential::SetCheckboxValue", || Ok(()))
    }

    fn SetComboBoxSelectedValue(&self, _dwfieldid: u32, _dwselecteditem: u32) -> Result<()> {
        guard("ICredentialProviderCredential::SetComboBoxSelectedValue", || {
            Ok(())
        })
    }

    fn CommandLinkClicked(&self, _dwfieldid: u32) -> Result<()> {
        guard("ICredentialProviderCredential::CommandLinkClicked", || Ok(()))
    }

    fn GetSerialization(
        &self,
        pcpgsr: *mut CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE,
        pcpcs: *mut CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
        ppszoptionalstatustext: *mut PWSTR,
        pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON,
    ) -> Result<()> {
        guard("ICredentialProviderCredential::GetSerialization", || {
            if pcpgsr.is_null()
                || pcpcs.is_null()
                || ppszoptionalstatustext.is_null()
                || pcpsioptionalstatusicon.is_null()
            {
                return Err(Error::from(E_POINTER));
            }
            if !SPIKE_NEVER_SUBMIT {
                let _ = self.try_submit();
            }
            // SAFETY: all pointers were checked non-null and are valid
            // writable out-pointers; `pcpcs` stays untouched because no
            // credential is returned.
            unsafe {
                *pcpgsr = CPGSR_NO_CREDENTIAL_FINISHED;
                *pcpsioptionalstatusicon = CPSI_NONE;
                *ppszoptionalstatustext = PWSTR(core::ptr::null_mut());
            }
            // Generic, score-free fallback wording only.
            let text = alloc_com_wide(tile_text(Phase::Fallback))?;
            // SAFETY: `ppszoptionalstatustext` is a valid writable out-pointer
            // and `text` is an owned COM allocation transferred to LogonUI.
            unsafe { *ppszoptionalstatustext = text };
            Ok(())
        })
    }

    fn ReportResult(
        &self,
        _ntsstatus: NTSTATUS,
        _ntssubstatus: NTSTATUS,
        _ppszoptionalstatustext: *mut PWSTR,
        _pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON,
    ) -> Result<()> {
        guard("ICredentialProviderCredential::ReportResult", || Ok(()))
    }
}
