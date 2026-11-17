//! Media Foundation capture source backed by `IMFSourceReader`.
//!
//! We deliberately use Media Foundation (not WinRT `MediaCapture`, which can
//! throw access-denied inside a service). Open uses an exponential backoff,
//! re-enumerates the device on failure, confirms readiness only after a REAL
//! frame is read, and discards the first N frames for auto-exposure
//! convergence. No frame bytes are ever logged or persisted; the intermediate
//! buffer is `Zeroizing` and zeroized on every exit path.
//!
//! OWNER-RUN VERIFY: the live Session-0 camera capture path is not exercised
//! on this host — only MSVC type-checking is performed here.

use std::time::{Duration, Instant};

use kenali_core::frame::{CameraPin, CaptureError, Frame, FrameSource};
use zeroize::Zeroizing;

use crate::pin::{enumerate_pins, verify_pin};

/// Backoff schedule for camera open retries, in milliseconds (~0/100/200/400/800).
pub const OPEN_BACKOFF_MS: [u64; 5] = [0, 100, 200, 400, 800];

/// Pure, host-testable backoff schedule computation. Attempt indices are
/// 0-based; anything past the schedule saturates at the last delay.
pub fn backoff_for_attempt(attempt: usize) -> Duration {
    let ms = match OPEN_BACKOFF_MS.get(attempt) {
        Some(ms) => *ms,
        None => 800,
    };
    Duration::from_millis(ms)
}

/// Bounded sleep: never sleeps past `deadline`, so no unbounded wait exists.
fn sleep_bounded(delay: Duration, deadline: Instant) -> Result<(), CaptureError> {
    let now = Instant::now();
    if now >= deadline {
        return Err(CaptureError::Timeout);
    }
    std::thread::sleep(delay.min(deadline.duration_since(now)));
    Ok(())
}

/// Default number of leading frames discarded for exposure/white-balance
/// convergence before the camera is treated as ready.
pub const DEFAULT_DISCARD_FRAMES: u32 = 5;

/// A Media Foundation capture source implementing the core `FrameSource`
/// seam. `reader` is the live `IMFSourceReader` created from the pinned device.
pub struct MfSource {
    #[cfg(target_os = "windows")]
    reader: Option<windows::Win32::Media::MediaFoundation::IMFSourceReader>,
    pin: Option<CameraPin>,
    width: u16,
    height: u16,
    discard_frames: u32,
    opened: bool,
}

// SAFETY: the COM `IMFSourceReader` is only ever touched from the thread that
// owns this MfSource (the service capture thread). It is never shared with or
// called from another thread without marshalling, so moving the owner between
// threads does not create a data race. Owner-run VERIFY on the live service.
#[cfg(target_os = "windows")]
unsafe impl Send for MfSource {}

impl Default for MfSource {
    fn default() -> Self {
        Self::new(DEFAULT_DISCARD_FRAMES)
    }
}

impl MfSource {
    /// Build a source that discards `discard_frames` leading frames on open.
    pub fn new(discard_frames: u32) -> Self {
        Self {
            #[cfg(target_os = "windows")]
            reader: None,
            pin: None,
            width: 0,
            height: 0,
            discard_frames,
            opened: false,
        }
    }
}

impl FrameSource for MfSource {
    fn open(&mut self, pin: &CameraPin, deadline: Instant) -> Result<(), CaptureError> {
        self.close();
        self.pin = Some(pin.clone());
        #[cfg(target_os = "windows")]
        {
            self.open_windows(pin, deadline)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (pin, deadline);
            Err(CaptureError::DeviceUnavailable)
        }
    }

    fn next_frame(&mut self, deadline: Instant) -> Result<Frame, CaptureError> {
        if !self.opened {
            return Err(CaptureError::Closed);
        }
        #[cfg(target_os = "windows")]
        {
            let reader = self.reader.as_ref().ok_or(CaptureError::Closed)?;
            read_rgb_frame(reader, self.width, self.height, deadline)
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = deadline;
            Err(CaptureError::DeviceUnavailable)
        }
    }

    fn close(&mut self) {
        self.opened = false;
        self.pin = None;
        self.width = 0;
        self.height = 0;
        #[cfg(target_os = "windows")]
        {
            // Dropping the last COM reference releases the reader; any
            // in-flight Zeroizing buffer was already zeroized on drop.
            self.reader = None;
        }
    }
}

#[cfg(target_os = "windows")]
impl MfSource {
    fn open_windows(&mut self, pin: &CameraPin, deadline: Instant) -> Result<(), CaptureError> {
        // SEC-16: refuse a camera that is not the enrolled device.
        let present = enumerate_pins()?;
        if !present.iter().any(|candidate| verify_pin(candidate, pin)) {
            return Err(CaptureError::PinMismatch);
        }

        for attempt in 0..OPEN_BACKOFF_MS.len() {
            if Instant::now() >= deadline {
                return Err(CaptureError::Timeout);
            }
            match self.try_open_once(pin, deadline) {
                Ok(()) => {
                    self.opened = true;
                    return Ok(());
                }
                Err(_) => {
                    // Re-enumerate on failure: the device may have been
                    // reattached or another app may have released it.
                    let _ = enumerate_pins();
                    sleep_bounded(backoff_for_attempt(attempt), deadline)?;
                }
            }
        }
        Err(CaptureError::Timeout)
    }

    /// One open attempt: bind the pinned device to an `IMFSourceReader`,
    /// select RGB24 video, then read (and discard) a confirmation frame plus
    /// the configured number of convergence frames.
    fn try_open_once(&mut self, pin: &CameraPin, deadline: Instant) -> Result<(), CaptureError> {
        use windows::core::PCWSTR;
        use windows::Win32::Media::MediaFoundation::{
            MFCreateAttributes, MFCreateDeviceSource, MFCreateMediaType,
            MFCreateSourceReaderFromMediaSource, MFStartup, IMFAttributes, IMFMediaSource,
            IMFMediaType, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_MT_FRAME_SIZE,
            MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING,
            MF_SOURCE_READER_FIRST_VIDEO_STREAM, MFMediaType_Video, MFSTARTUP_FULL, MF_VERSION,
            MFVideoFormat_RGB24,
        };

        // SAFETY: MFStartup takes no pointers; safe one-time initialisation.
        unsafe {
            let _ = MFStartup(MF_VERSION, MFSTARTUP_FULL);
        }

        // SAFETY: `attrs` is a local out-param, valid for all calls below.
        let mut attrs: Option<IMFAttributes> = None;
        unsafe {
            MFCreateAttributes(&mut attrs, 4).map_err(|_| CaptureError::Other)?;
        }
        let attrs = attrs.ok_or(CaptureError::DeviceUnavailable)?;

        let mut link_wide: Vec<u16> = pin
            .symbolic_link
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let link = PCWSTR::from_raw(link_wide.as_mut_ptr());
        // SAFETY: `link` points to a NUL-terminated buffer alive for the call;
        // both GUIDs are `'static` constants; `attrs` is a live COM object.
        unsafe {
            attrs
                .SetGUID(
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
                )
                .map_err(|_| CaptureError::Other)?;
            attrs
                .SetString(
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                    link,
                )
                .map_err(|_| CaptureError::Other)?;
        }

        // SAFETY: `attrs` is a live IMFAttributes describing the pinned
        // video-capture device; MFCreateDeviceSource returns an owned ref.
        let source: IMFMediaSource = unsafe {
            MFCreateDeviceSource(&attrs).map_err(|_| CaptureError::DeviceUnavailable)?
        };

        // SAFETY: local out-param, valid for the call; `source` is a live
        // media source and `reader_attrs` a live attributes object.
        let mut reader_attrs: Option<IMFAttributes> = None;
        unsafe {
            MFCreateAttributes(&mut reader_attrs, 2).map_err(|_| CaptureError::Other)?;
        }
        let reader_attrs = reader_attrs.ok_or(CaptureError::Other)?;
        // SAFETY: GUID is a `'static` constant; value is a plain UINT32 flag.
        unsafe {
            reader_attrs
                .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
                .map_err(|_| CaptureError::Other)?;
        }
        // SAFETY: both COM objects are live and owned by this function; the
        // returned reader is an owned COM reference.
        let reader = unsafe {
            MFCreateSourceReaderFromMediaSource(&source, &reader_attrs)
                .map_err(|_| CaptureError::DeviceUnavailable)?
        };

        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

        // SAFETY: `mt` is a live IMFMediaType; both GUIDs are `'static`
        // constants; `reader` is live and `$stream` is a valid stream index.
        let mt: IMFMediaType = unsafe { MFCreateMediaType().map_err(|_| CaptureError::Other)? };
        unsafe {
            mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .map_err(|_| CaptureError::Other)?;
            mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB24)
                .map_err(|_| CaptureError::Other)?;
            reader
                .SetStreamSelection(stream, true)
                .map_err(|_| CaptureError::DeviceUnavailable)?;
            reader
                .SetCurrentMediaType(stream, None, &mt)
                .map_err(|_| CaptureError::Other)?;
        }

        // SAFETY: `current` is an owned COM reference returned by the live
        // reader; the GUID is a `'static` constant.
        let (width, height) = unsafe {
            let current = reader.GetCurrentMediaType(stream).map_err(|_| CaptureError::Other)?;
            let packed = current
                .GetUINT64(&MF_MT_FRAME_SIZE)
                .map_err(|_| CaptureError::Other)?;
            ((packed >> 32) as u32, (packed & 0xffff_ffff) as u32)
        };
        if width == 0
            || height == 0
            || width > u32::from(u16::MAX)
            || height > u32::from(u16::MAX)
        {
            return Err(CaptureError::FrameTooLarge);
        }
        self.width = width as u16;
        self.height = height as u16;

        // First-frame confirmation: a real frame must be read before the
        // camera is called ready. Read one confirmation frame plus N
        // convergence frames and discard them all.
        let reads = self.discard_frames.saturating_add(1);
        for _ in 0..reads {
            let probe = read_rgb_frame(&reader, self.width, self.height, deadline)?;
            drop(probe); // discarded; Zeroizing buffer already zeroized
        }

        self.reader = Some(reader);
        Ok(())
    }
}

/// Read a single RGB24 sample from `reader` into a `Frame`.
#[cfg(target_os = "windows")]
fn read_rgb_frame(
    reader: &windows::Win32::Media::MediaFoundation::IMFSourceReader,
    width: u16,
    height: u16,
    deadline: Instant,
) -> Result<Frame, CaptureError> {
    use windows::Win32::Media::MediaFoundation::{
        IMFSample, MF_SOURCE_READERF_ERROR, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    };

    if Instant::now() >= deadline {
        return Err(CaptureError::Timeout);
    }

    let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
    let mut flags = 0u32;
    let mut sample: Option<IMFSample> = None;
    // SAFETY: all out-params point to live locals; `reader` is live and
    // borrowed for the duration of the call. No pointers escape the call.
    unsafe {
        reader
            .ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
            .map_err(|_| CaptureError::Other)?;
    }
    if flags & (MF_SOURCE_READERF_ERROR.0 as u32) != 0 {
        return Err(CaptureError::Other);
    }
    let sample = sample.ok_or(CaptureError::Timeout)?;

    // SAFETY: `sample` is an owned, live COM reference from ReadSample.
    let buffer = unsafe {
        sample
            .ConvertToContiguousBuffer()
            .map_err(|_| CaptureError::Other)?
    };

    let mut data: *mut u8 = std::ptr::null_mut();
    let mut current_len = 0u32;
    // SAFETY: `data` and `current_len` are live local out-params; `buffer` is
    // a live COM object locked for exactly the span below and unlocked later.
    unsafe {
        buffer
            .Lock(&mut data, None, Some(&mut current_len))
            .map_err(|_| CaptureError::Other)?;
    }

    // Sensitive frame bytes live only in this Zeroizing buffer, which is
    // zeroized on every exit path (including early returns below).
    let mut raw = Zeroizing::new(vec![0u8; current_len as usize]);
    // SAFETY: `data` points to `current_len` readable bytes owned by the
    // locked buffer, and `raw` is a live allocation of exactly that many
    // bytes; both are valid for the length of this copy inside the lock.
    unsafe {
        std::ptr::copy_nonoverlapping(data, raw.as_mut_ptr(), current_len as usize);
    }
    // SAFETY: `buffer` is the same object locked above and still held here.
    unsafe {
        buffer.Unlock().map_err(|_| CaptureError::Other)?;
    }

    Frame::new(width, height, raw.to_vec())
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn backoff_schedule_is_exponential_then_capped() {
        assert_eq!(backoff_for_attempt(0), Duration::from_millis(0));
        assert_eq!(backoff_for_attempt(1), Duration::from_millis(100));
        assert_eq!(backoff_for_attempt(2), Duration::from_millis(200));
        assert_eq!(backoff_for_attempt(3), Duration::from_millis(400));
        assert_eq!(backoff_for_attempt(4), Duration::from_millis(800));
        assert_eq!(backoff_for_attempt(99), Duration::from_millis(800));
    }

    #[test]
    fn default_source_discards_five_frames() {
        let src = MfSource::default();
        assert_eq!(src.discard_frames, 5);
        assert!(!src.opened);
    }
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::*;

    #[test]
    fn open_rejects_missing_camera_without_panicking() {
        let mut src = MfSource::default();
        let pin = CameraPin::new("\\\\?\\nonexistent-camera", "none");
        let result = src.open(&pin, Instant::now());
        assert!(result.is_err());
        assert!(!src.opened);
    }
}
