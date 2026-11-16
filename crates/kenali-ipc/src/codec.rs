//! Postcard codec with hostile-input caps and a `PROTO_VERSION` handshake.
//!
//! # Validation order (every IPC byte is hostile)
//!
//! 1. [`decode`] checks `bytes.len() <= max` *before* touching the bytes, so a
//!    giant frame cannot enter the deserializer at all. Wrappers pin `max` to
//!    the right cap for the pipe/message class.
//! 2. For `Hello` variants, [`peek_hello_proto`] consumes only the enum-variant
//!    varint and the following `u16` proto field — no collection is built — so
//!    a version mismatch is rejected before any payload is allocated.
//! 3. `wide`/`Bytes` fields are deserialized by element-by-element visitors in
//!    [`crate::wire`] that enforce their caps before each push. Unknown enum
//!    variants are rejected by postcard/serde as invalid variant indices.
//! 4. Each typed wrapper repeats the proto check on the fully decoded value as
//!    an authoritative backstop.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{admin, auth, preview};

/// Control messages (auth + admin, excluding diagnostics chunks).
pub const MAX_CONTROL_BYTES: usize = 4096;
/// Preview frame payload, 320×240 RGB ≈ 230 KB plus headroom.
pub const MAX_PREVIEW_BYTES: usize = 524_288;
/// One diagnostics ZIP chunk on the admin pipe.
pub const MAX_DIAG_CHUNK_BYTES: usize = 65_536;
/// Total diagnostics ZIP across all chunks.
pub const MAX_DIAG_TOTAL_BYTES: usize = 10_485_760;
/// Maximum UTF-16 units in `Wide`/`PasswordWide`/`SecretWide`.
pub const MAX_WIDE_UNITS: usize = 256;

#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecError {
    #[error("payload too large: {len} bytes (cap {max})")]
    TooLarge { len: usize, max: usize },
    #[error("protocol version mismatch: expected {expected}, got {got}")]
    VersionMismatch { expected: u16, got: u16 },
    #[error("serialization failed")]
    Serialize,
    #[error("deserialization failed")]
    Deserialize,
}

/// Serialize a message. The cap is enforced on decode, by the receiver.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    postcard::to_allocvec(value).map_err(|_| CodecError::Serialize)
}

/// Deserialize a message, refusing any payload longer than `max` before any
/// allocation from untrusted lengths occurs.
pub fn decode<T: DeserializeOwned>(bytes: &[u8], max: usize) -> Result<T, CodecError> {
    if bytes.len() > max {
        return Err(CodecError::TooLarge {
            len: bytes.len(),
            max,
        });
    }
    postcard::from_bytes(bytes).map_err(|_| CodecError::Deserialize)
}

/// Read only the leading enum-variant index and, for the `Hello` variant, the
/// `proto` field. Postcard encodes both as small integers, so this touches no
/// collection and cannot be made to allocate from a hostile length prefix.
fn peek_hello_proto(bytes: &[u8]) -> Result<Option<u16>, CodecError> {
    let mut de = postcard::Deserializer::from_bytes(bytes);
    let variant = u64::deserialize(&mut de).map_err(|_| CodecError::Deserialize)?;
    if variant != 0 {
        return Ok(None);
    }
    let proto = u16::deserialize(&mut de).map_err(|_| CodecError::Deserialize)?;
    Ok(Some(proto))
}

fn verify_proto(proto: u16) -> Result<(), CodecError> {
    if proto == crate::PROTO_VERSION {
        Ok(())
    } else {
        Err(CodecError::VersionMismatch {
            expected: crate::PROTO_VERSION,
            got: proto,
        })
    }
}

/// Handshake helper for callers that read a proto value from a trusted header.
pub fn check_proto(proto: u16) -> Result<(), CodecError> {
    verify_proto(proto)
}

/// Decode any control message with the pinned control cap.
pub fn decode_control<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    decode(bytes, MAX_CONTROL_BYTES)
}

/// Decode a preview frame with the pinned preview cap.
pub fn decode_preview(bytes: &[u8]) -> Result<preview::PreviewFrame, CodecError> {
    decode(bytes, MAX_PREVIEW_BYTES)
}

/// Decode a diagnostics chunk with the pinned per-chunk cap. `MAX_DIAG_TOTAL_BYTES`
/// is enforced by the caller across the stream of chunks.
pub fn decode_diag_chunk<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    decode(bytes, MAX_DIAG_CHUNK_BYTES)
}

/// Decode an auth request, rejecting a mismatched `Hello` proto early.
pub fn decode_auth_request(bytes: &[u8]) -> Result<auth::Request, CodecError> {
    if let Some(proto) = peek_hello_proto(bytes)? {
        verify_proto(proto)?;
    }
    let request: auth::Request = decode_control(bytes)?;
    if let auth::Request::Hello { proto } = &request {
        verify_proto(*proto)?;
    }
    Ok(request)
}

/// Decode an auth response, rejecting a mismatched `Hello` proto early.
pub fn decode_auth_response(bytes: &[u8]) -> Result<auth::Response, CodecError> {
    if let Some(proto) = peek_hello_proto(bytes)? {
        verify_proto(proto)?;
    }
    let response: auth::Response = decode_control(bytes)?;
    if let auth::Response::Hello { proto, .. } = &response {
        verify_proto(*proto)?;
    }
    Ok(response)
}

/// Decode an admin request, rejecting a mismatched `Hello` proto early.
pub fn decode_admin_request(bytes: &[u8]) -> Result<admin::AdminRequest, CodecError> {
    if let Some(proto) = peek_hello_proto(bytes)? {
        verify_proto(proto)?;
    }
    let request: admin::AdminRequest = decode_control(bytes)?;
    if let admin::AdminRequest::Hello { proto } = &request {
        verify_proto(*proto)?;
    }
    Ok(request)
}

/// Decode an admin response, rejecting a mismatched `Hello` proto early.
pub fn decode_admin_response(bytes: &[u8]) -> Result<admin::AdminResponse, CodecError> {
    if let Some(proto) = peek_hello_proto(bytes)? {
        verify_proto(proto)?;
    }
    let response: admin::AdminResponse = decode_control(bytes)?;
    if let admin::AdminResponse::Hello { proto, .. } = &response {
        verify_proto(*proto)?;
    }
    Ok(response)
}
