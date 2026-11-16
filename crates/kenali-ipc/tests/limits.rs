//! Hostile-input tests: caps, truncation, unknown variants, redaction, proto.

use kenali_ipc::codec::{
    decode, decode_admin_request, decode_admin_response, decode_auth_request, decode_auth_response,
    decode_diag_chunk, decode_preview, encode, CodecError, MAX_CONTROL_BYTES, MAX_DIAG_CHUNK_BYTES,
    MAX_DIAG_TOTAL_BYTES, MAX_PREVIEW_BYTES, MAX_WIDE_UNITS,
};
use kenali_ipc::wire::{
    password_into_wide, Bytes, PasswordWide, SecretWide, Wide,
};
use kenali_ipc::{
    admin::{AdminRequest, AdminResponse},
    auth::{Request, Response},
    preview::PreviewFrame,
    PROTO_VERSION,
};

#[test]
fn total_message_cap_checked_before_deserializing() {
    let bytes = vec![0x42u8; MAX_CONTROL_BYTES + 1];
    let err = decode::<Request>(&bytes, MAX_CONTROL_BYTES).expect_err("must reject oversize");
    assert!(matches!(err, CodecError::TooLarge { len, max } if len == MAX_CONTROL_BYTES + 1 && max == MAX_CONTROL_BYTES));
}

#[test]
fn wide_cap_rejects_overlong_vector() {
    let units = vec![0x0041u16; MAX_WIDE_UNITS + 1];
    let wire = encode(&Wide::from_units(units)).expect("encode");
    let err = decode::<Wide>(&wire, MAX_CONTROL_BYTES).expect_err("must reject overlong wide");
    assert!(matches!(err, CodecError::Deserialize));
}

#[test]
fn password_and_secret_wide_caps_are_enforced() {
    let pw = encode(&PasswordWide::from_units(vec![7u16; MAX_WIDE_UNITS + 1])).expect("encode");
    assert!(matches!(
        decode::<PasswordWide>(&pw, MAX_CONTROL_BYTES),
        Err(CodecError::Deserialize)
    ));
    let sw = encode(&SecretWide::from_units(vec![7u16; MAX_WIDE_UNITS + 1])).expect("encode");
    assert!(matches!(
        decode::<SecretWide>(&sw, MAX_CONTROL_BYTES),
        Err(CodecError::Deserialize)
    ));
}

#[test]
fn bytes_cap_rejects_overlong_vector() {
    let blob = vec![0u8; MAX_PREVIEW_BYTES + 1];
    let wire = encode(&Bytes::new(blob)).expect("encode");
    // Let it past the total-length gate on purpose; the field-level visitor must
    // still reject it before it can be used as a preview frame.
    let err = decode::<Bytes>(&wire, MAX_PREVIEW_BYTES + 100).expect_err("field cap must fire");
    assert!(matches!(err, CodecError::Deserialize));
}

#[test]
fn truncated_payloads_are_rejected() {
    let full = encode(&Request::Cancel { attempt: 7 }).expect("encode");
    for cut in 1..full.len() {
        assert!(
            decode::<Request>(&full[..cut], MAX_CONTROL_BYTES).is_err(),
            "truncation at {cut} must fail"
        );
    }
    let preview = encode(&PreviewFrame {
        w: 2,
        h: 2,
        rgb: Bytes::new(vec![9; 12]),
    })
    .expect("encode");
    assert!(decode_preview(&preview[..preview.len() - 1]).is_err());
}

#[test]
fn unknown_enum_variant_is_rejected() {
    let mut bytes = encode(&Request::Cancel { attempt: 7 }).expect("encode");
    bytes[0] = 0x7f; // variant index 127 — does not exist
    assert!(matches!(
        decode::<Request>(&bytes, MAX_CONTROL_BYTES),
        Err(CodecError::Deserialize)
    ));
}

#[test]
fn corrupt_payload_is_rejected() {
    let bytes = encode(&AdminRequest::DeleteTemplate {
        id: kenali_core::types::TemplateId(42),
    })
    .expect("encode");
    assert!(decode::<AdminRequest>(&[0xff, 0xff, 0xff, 0xff], MAX_CONTROL_BYTES).is_err());
    // A valid envelope with a corrupted trailing field must also fail, not yield
    // a half-decoded request.
    let truncated = &bytes[..bytes.len().saturating_sub(1)];
    assert!(decode_admin_request(truncated).is_err());
}

#[test]
fn hello_proto_must_match() {
    let good = encode(&Request::Hello {
        proto: PROTO_VERSION,
    })
    .expect("encode");
    assert!(decode_auth_request(&good).is_ok());

    let bad = encode(&Request::Hello {
        proto: PROTO_VERSION + 1,
    })
    .expect("encode");
    assert!(matches!(
        decode_auth_request(&bad),
        Err(CodecError::VersionMismatch { expected, got })
            if expected == PROTO_VERSION && got == PROTO_VERSION + 1
    ));

    let bad_admin = encode(&AdminRequest::Hello {
        proto: PROTO_VERSION + 1,
    })
    .expect("encode");
    assert!(matches!(
        decode_admin_request(&bad_admin),
        Err(CodecError::VersionMismatch { .. })
    ));

    let bad_response = encode(&Response::Hello {
        proto: 0,
        svc_pid: 1,
    })
    .expect("encode");
    assert!(matches!(
        decode_auth_response(&bad_response),
        Err(CodecError::VersionMismatch { .. })
    ));

    let bad_admin_response = encode(&AdminResponse::Hello {
        proto: 0,
        svc_pid: 1,
    })
    .expect("encode");
    assert!(matches!(
        decode_admin_response(&bad_admin_response),
        Err(CodecError::VersionMismatch { .. })
    ));

    assert!(kenali_ipc::codec::check_proto(PROTO_VERSION).is_ok());
    assert!(kenali_ipc::codec::check_proto(PROTO_VERSION + 1).is_err());
}

#[test]
fn bad_hello_proto_is_rejected_before_full_decode() {
    // Encode a Hello with a wrong proto, then damage the *later* svc_pid field
    // so a full deserialization would fail. The proto check must still fire
    // first, proving it runs before the rest of the message is decoded.
    let mut bytes = encode(&Response::Hello {
        proto: 999,
        svc_pid: 7,
    })
    .expect("encode");
    let intact_len = bytes.len();
    bytes.truncate(intact_len - 1);
    assert!(matches!(
        decode_auth_request(&bytes),
        Err(CodecError::VersionMismatch { got: 999, .. })
    ));

    let mut admin_bytes = encode(&AdminResponse::Hello {
        proto: 999,
        svc_pid: 7,
    })
    .expect("encode");
    let intact_len = admin_bytes.len();
    admin_bytes.truncate(intact_len - 1);
    assert!(matches!(
        decode_admin_response(&admin_bytes),
        Err(CodecError::VersionMismatch { got: 999, .. })
    ));
}

#[test]
fn caps_constants_are_pinned() {
    assert_eq!(MAX_CONTROL_BYTES, 4096);
    assert_eq!(MAX_PREVIEW_BYTES, 524_288);
    assert_eq!(MAX_DIAG_CHUNK_BYTES, 65_536);
    assert_eq!(MAX_DIAG_TOTAL_BYTES, 10_485_760);
    assert_eq!(MAX_WIDE_UNITS, 256);
    assert_eq!(PROTO_VERSION, 1);
}

#[test]
fn diag_chunk_wrapper_enforces_its_cap() {
    let frame = PreviewFrame {
        w: 1,
        h: 1,
        rgb: Bytes::new(vec![0u8; 128]),
    };
    let wire = encode(&frame).expect("encode");
    assert!(decode_diag_chunk::<PreviewFrame>(&wire).is_ok());
    let too_big = encode(&PreviewFrame {
        w: 1,
        h: 1,
        rgb: Bytes::new(vec![0u8; MAX_DIAG_CHUNK_BYTES + 1]),
    })
    .expect("encode");
    assert!(matches!(
        decode_diag_chunk::<PreviewFrame>(&too_big),
        Err(CodecError::TooLarge { .. })
    ));
}

#[test]
fn secrets_never_appear_in_debug_or_errors() {
    let units: Vec<u16> = "hunter2".encode_utf16().collect();
    let pw = PasswordWide::from_units(units.clone());
    let sw = SecretWide::from_units(units.clone());
    assert_eq!(format!("{pw:?}"), "PasswordWide(<redacted>)");
    assert_eq!(format!("{sw:?}"), "SecretWide(<redacted>)");

    let response = Response::Verified {
        attempt: 1,
        nonce: [0; 16],
        user: Wide::from_units("alice".encode_utf16().collect()),
        domain: Wide::from_units(vec![]),
        secret: sw,
    };
    let dbg = format!("{response:?}");
    assert!(dbg.contains("<redacted>"));
    assert!(!dbg.to_lowercase().contains("hunter2"));

    let err = format!("{:?}", decode::<Response>(&[0xff, 0xff, 0xff], MAX_CONTROL_BYTES));
    assert!(!err.to_lowercase().contains("hunter2"));
}

#[test]
fn into_secret_transfers_without_plain_copy_in_logs() {
    let units: Vec<u16> = "s3cr3t".encode_utf16().collect();
    let pw = PasswordWide::from_units(units.clone());
    let secret = pw.into_secret(); // pw consumed; no plain copy remains
    assert_eq!(secret.as_wide(), units.as_slice());
    assert_eq!(format!("{secret:?}"), "Password(<redacted>)");

    let back = password_into_wide(secret);
    assert_eq!(back.into_secret().as_wide(), units.as_slice());
}
