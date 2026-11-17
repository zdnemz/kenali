//! Physical-camera pinning (SEC-16).
//!
//! A camera is identified by its device-interface symbolic link (the stable
//! identity on Windows) plus a hardware ID used for diagnostics. `verify_pin`
//! is pure logic and is host-tested; `enumerate_pins` is Win32-only and is
//! type-checked against the MSVC target only — no camera is opened on this
//! host. Owner-run VERIFY: the live Session-0 capture path is not exercised
//! here.

use kenali_core::frame::{CameraPin, CaptureError};

/// Pure, host-testable pin comparison.
///
/// The symbolic link is the authoritative identity and is compared
/// case-insensitively (Windows device paths are case-insensitive). The
/// hardware ID is diagnostics-only per `kenali_core::frame::CameraPin`; it is
/// enforced only when both sides supply one, so a harmless firmware string
/// change cannot lock the user out. An empty enrolled link never matches.
pub fn verify_pin(actual: &CameraPin, enrolled: &CameraPin) -> bool {
    pins_equal(actual, enrolled)
}

fn pins_equal(actual: &CameraPin, enrolled: &CameraPin) -> bool {
    if enrolled.symbolic_link.is_empty() {
        return false;
    }
    if !actual
        .symbolic_link
        .eq_ignore_ascii_case(&enrolled.symbolic_link)
    {
        return false;
    }
    if enrolled.hardware_id.is_empty() || actual.hardware_id.is_empty() {
        return true;
    }
    actual.hardware_id.eq_ignore_ascii_case(&enrolled.hardware_id)
}

/// Enumerate the video-capture devices currently present as `CameraPin`s.
///
/// Win32-only. On non-Windows hosts (and when no camera is present) this
/// returns `DeviceUnavailable`; the service owns camera access in production,
/// so callers in tests/VMs must supply their own source.
#[cfg(target_os = "windows")]
pub fn enumerate_pins() -> Result<Vec<CameraPin>, CaptureError> {
    use std::ffi::c_void;
    use std::ptr;
    use windows::Win32::Media::MediaFoundation::{
        MFCreateAttributes, MFEnumDeviceSources, MFStartup, IMFActivate, IMFAttributes,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MFSTARTUP_FULL, MF_VERSION,
    };
    use windows::Win32::System::Com::CoTaskMemFree;

    // SAFETY: MFStartup takes no pointers and is the required one-time
    // initialisation before any other Media Foundation call.
    unsafe {
        let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
    }

    // SAFETY: `attrs` is a local out-param owned by this function and lives
    // for the duration of every call below.
    let mut attrs: Option<IMFAttributes> = None;
    unsafe {
        MFCreateAttributes(&mut attrs, 2).map_err(|_| CaptureError::Other)?;
    }
    let attrs = attrs.ok_or(CaptureError::DeviceUnavailable)?;

    // SAFETY: both GUID references are to `'static` constants and remain
    // valid for the call; `attrs` is a live COM object.
    unsafe {
        attrs
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|_| CaptureError::Other)?;
    }

    let mut activate_ptr: *mut Option<IMFActivate> = ptr::null_mut();
    let mut count: u32 = 0;
    // SAFETY: `activate_ptr` and `count` are valid local out-params; MF owns
    // the returned array which we free with CoTaskMemFree below.
    unsafe {
        MFEnumDeviceSources(&attrs, &mut activate_ptr, &mut count)
            .map_err(|_| CaptureError::Other)?;
    }
    if activate_ptr.is_null() || count == 0 {
        return Ok(Vec::new());
    }

    // SAFETY: `activate_ptr` points to `count` initialised Option<IMFActivate>
    // slots allocated by MFEnumDeviceSources; the slice borrow ends before the
    // buffer is freed.
    let activations = unsafe { std::slice::from_raw_parts(activate_ptr, count as usize) };

    let mut pins = Vec::with_capacity(count as usize);
    for slot in activations {
        let Some(activate) = slot.as_ref() else {
            continue;
        };
        let mut wide = [0u16; 512];
        let mut needed: u32 = 0;
        // SAFETY: `wide` is a writable local buffer of 512 u16 and `needed` is
        // a valid out-param; MF writes at most `wide.len()` code units.
        let get = unsafe {
            activate.GetString(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                &mut wide,
                Some(&mut needed),
            )
        };
        if get.is_err() {
            continue;
        }
        let symbolic_link = String::from_utf16_lossy(&wide)
            .trim_end_matches('\0')
            .to_owned();
        if symbolic_link.is_empty() {
            continue;
        }
        // Hardware ID is diagnostics-only; the friendly name is a stable,
        // human-readable stand-in when no explicit hardware ID is exposed.
        let mut hw_wide = [0u16; 256];
        // SAFETY: `hw_wide` is a writable local buffer; GetString writes at
        // most its length and we ignore errors (diagnostic field only).
        let hw_ok = unsafe {
            activate.GetString(
                &windows::Win32::Media::MediaFoundation::MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
                &mut hw_wide,
                None,
            )
            .is_ok()
        };
        let hardware_id = if hw_ok {
            String::from_utf16_lossy(&hw_wide)
                .trim_end_matches('\0')
                .to_owned()
        } else {
            String::new()
        };
        pins.push(CameraPin::new(symbolic_link, hardware_id));
    }

    // SAFETY: `activate_ptr` was allocated by MFEnumDeviceSources and is
    // released exactly once with CoTaskMemFree after the slice borrow ends.
    unsafe {
        CoTaskMemFree(Some(activate_ptr as *const c_void));
    }
    Ok(pins)
}

/// Non-Windows stub: there is no camera enumeration on this host.
#[cfg(not(target_os = "windows"))]
pub fn enumerate_pins() -> Result<Vec<CameraPin>, CaptureError> {
    Err(CaptureError::DeviceUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_match_requires_symbolic_link() {
        let enrolled = CameraPin::new("\\\\?\\usb#vid_1", "hw-1");
        let same = CameraPin::new("\\\\?\\USB#VID_1", "HW-1");
        assert!(verify_pin(&same, &enrolled));
        let other = CameraPin::new("\\\\?\\usb#vid_2", "hw-1");
        assert!(!verify_pin(&other, &enrolled));
    }

    #[test]
    fn pin_empty_enrolled_never_matches() {
        let enrolled = CameraPin::new("", "hw");
        let actual = CameraPin::new("", "hw");
        assert!(!verify_pin(&actual, &enrolled));
    }

    #[test]
    fn pin_hardware_id_is_diagnostic_when_missing() {
        let enrolled = CameraPin::new("link", "hw-a");
        let actual = CameraPin::new("link", "");
        assert!(verify_pin(&actual, &enrolled));
    }
}
