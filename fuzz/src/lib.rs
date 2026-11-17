#![forbid(unsafe_code)]

//! Host-checkable byte-slice entry points for the hostile-input harnesses.
//!
//! These entry points exercise the exact codec decoders and frame/preprocess
//! seams used in production. They panic on no input, write no files, log no
//! bytes and never allocate beyond the caps pinned by the codec and frame
//! modules. No fuzzing campaign is run here: the entry points are driven by
//! proptest on the host and are type-checked for the MSVC target.

use kenali_core::frame::Frame;
use kenali_ipc::codec::{
    MAX_DIAG_CHUNK_BYTES, MAX_PREVIEW_BYTES, check_proto, decode_admin_request,
    decode_auth_request, decode_auth_response, decode_diag_chunk, decode_preview,
};
use kenali_vision::preprocess::to_nchw;

/// Upper bound on frame side length derived from harness input.
///
/// 64 × 64 × 3 = 12,288 bytes per candidate frame, so even repeated candidate
/// shapes stay small and deterministic for host testing.
const MAX_FRAME_SIDE: u16 = 64;

/// Run every pinned IPC decoder against `data`, returning the number of
/// successful decodes. Each decoder enforces its own hostile-input cap, so
/// oversized payloads are rejected before deserialization and cannot drive
/// unbounded allocation.
pub fn fuzz_ipc_decode(data: &[u8]) -> usize {
    // Control decoders share the 4096-byte cap and the pinned proto handshake.
    // A successful decode is counted once per decoder, not per byte.
    let mut count = usize::from(decode_auth_request(data).is_ok());
    count += usize::from(decode_admin_request(data).is_ok());
    count += usize::from(decode_auth_response(data).is_ok());

    // The preview cap is larger; short-circuit before calling the decoder for
    // inputs that could not possibly be a preview payload.
    if data.len() <= MAX_PREVIEW_BYTES {
        count += usize::from(decode_preview(data).is_ok());
    }

    // Diagnostic chunks are generic over the payload type; exercise the
    // app-facing chunk record from the admin pipe. The decoder enforces
    // MAX_DIAG_CHUNK_BYTES before any untrusted length is honoured.
    if data.len() <= MAX_DIAG_CHUNK_BYTES {
        count += usize::from(
            decode_diag_chunk::<kenali_ipc::admin::AdminResponse>(data).is_ok(),
        );
    }

    // Exercise the protocol-version check with values derived entirely from
    // the input. Both the accepted and rejected paths are pure comparisons.
    if let Ok(proto) = decode_leading_u16(data) {
        count += usize::from(check_proto(proto).is_ok());
    }

    count
}

/// Read a leading `u16` in little-endian postcard order without allocating or
/// panicking. Shorter inputs simply skip the handshake check.
fn decode_leading_u16(data: &[u8]) -> Result<u16, ()> {
    let bytes = data.get(..2).ok_or(())?;
    let mut raw = [0u8; 2];
    raw.copy_from_slice(bytes);
    Ok(u16::from_le_bytes(raw))
}

/// Build bounded frame dimensions from the first two bytes of `data`, then
/// try a tightly packed RGB buffer of exactly the matching size. `Frame::new`
/// rejects length mismatches without panicking; `to_nchw` is called on the
/// accepted frame and on the raw bytes with the same bounded dimensions so
/// both the valid and empty-output paths are covered.
pub fn fuzz_frame_decode(data: &[u8]) -> usize {
    let width = u16::from(data.first().copied().unwrap_or(0)) % MAX_FRAME_SIDE + 1;
    let height = u16::from(data.get(1).copied().unwrap_or(0)) % MAX_FRAME_SIDE + 1;

    let Some(rgb_len) = usize::from(width)
        .checked_mul(usize::from(height))
        .and_then(|pixels| pixels.checked_mul(3))
    else {
        return 0;
    };

    let mut count = 0;
    if data.len() >= rgb_len {
        // `data[..rgb_len]` is verified in bounds above; no indexing panic.
        let rgb = data[..rgb_len].to_vec();
        if let Ok(frame) = Frame::new(width, height, rgb) {
            count += 1;
            let nchw = to_nchw(&frame.rgb, usize::from(frame.width), usize::from(frame.height), 3);
            count += usize::from(!nchw.is_empty());
        }
    }

    // Also drive the preprocess seam directly with bounded dimensions and the
    // unmodified hostile input; it returns an empty vector on any mismatch.
    let direct = to_nchw(data, usize::from(width), usize::from(height), 3);
    count += usize::from(!direct.is_empty());

    count
}
