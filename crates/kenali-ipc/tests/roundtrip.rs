//! Property tests: every typed message survives an encode/decode round-trip.

use std::fmt::Debug;

use kenali_core::config::{ConfigKey, ConfigValue};
use kenali_core::types::{
    DenyReason, EnrolMode, EnrolStage, Phase, Pose, Scenario, Sid, StatusReport, TemplateId,
    TemplateMeta, TestReport,
};
use kenali_ipc::admin::{AdminRequest, AdminResponse};
use kenali_ipc::auth::{Request, Response};
use kenali_ipc::codec::{decode, encode, MAX_CONTROL_BYTES, MAX_PREVIEW_BYTES};
use kenali_ipc::PROTO_VERSION;
use kenali_ipc::preview::PreviewFrame;
use kenali_ipc::wire::{Bytes, PasswordWide, SecretWide, Wide};
use proptest::prelude::*;
use serde::{de::DeserializeOwned, Serialize};

fn roundtrip<T>(value: &T, cap: usize)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let bytes = encode(value).expect("encode must succeed");
    let back: T = decode(&bytes, cap).expect("decode must succeed");
    assert_eq!(value, &back);
}

fn any_wide() -> impl Strategy<Value = Wide> {
    prop::collection::vec(any::<u16>(), 0..16).prop_map(Wide::from_units)
}

fn any_password() -> impl Strategy<Value = PasswordWide> {
    prop::collection::vec(any::<u16>(), 0..16).prop_map(PasswordWide::from_units)
}

fn any_secret() -> impl Strategy<Value = SecretWide> {
    prop::collection::vec(any::<u16>(), 0..16).prop_map(SecretWide::from_units)
}

fn any_bytes() -> impl Strategy<Value = Bytes> {
    prop::collection::vec(any::<u8>(), 0..64).prop_map(Bytes::new)
}

fn any_sid() -> impl Strategy<Value = Sid> {
    any::<u32>().prop_map(|n| Sid::new(format!("S-1-5-21-{n}")))
}

fn any_scenario() -> impl Strategy<Value = Scenario> {
    prop::sample::select(vec![
        Scenario::Unlock,
        Scenario::Logon,
        Scenario::Remote,
        Scenario::ChangePassword,
        Scenario::CredUi,
        Scenario::Plap,
    ])
}

fn any_phase() -> impl Strategy<Value = Phase> {
    prop::sample::select(vec![
        Phase::Armed,
        Phase::Searching,
        Phase::Guidance,
        Phase::Challenge,
        Phase::Verified,
        Phase::Denied,
        Phase::Fallback,
        Phase::Disabled,
        Phase::ServiceDown,
    ])
}

fn any_deny_reason() -> impl Strategy<Value = DenyReason> {
    prop::sample::select(vec![
        DenyReason::NoFace,
        DenyReason::MultipleFaces,
        DenyReason::PadRejected,
        DenyReason::Attention,
        DenyReason::LowSimilarity,
        DenyReason::Cooldown,
        DenyReason::CredentialStale,
        DenyReason::CameraMismatch,
        DenyReason::NotEnrolled,
        DenyReason::WrongSid,
        DenyReason::RemoteSession,
        DenyReason::NoRetry,
        DenyReason::Timeout,
        DenyReason::Other,
    ])
}

fn any_enrol_mode() -> impl Strategy<Value = EnrolMode> {
    prop::sample::select(vec![
        EnrolMode::New,
        EnrolMode::AddTemplate,
        EnrolMode::RefreshCredential,
    ])
}

fn any_enrol_stage() -> impl Strategy<Value = EnrolStage> {
    prop::sample::select(vec![
        EnrolStage::Prerequisites,
        EnrolStage::Password,
        EnrolStage::Capture,
        EnrolStage::Seal,
        EnrolStage::SelfTest,
        EnrolStage::Summary,
    ])
}

fn any_pose() -> impl Strategy<Value = Pose> {
    prop::sample::select(vec![
        Pose::Straight,
        Pose::Left,
        Pose::Right,
        Pose::Up,
        Pose::Down,
    ])
}

fn any_config_key() -> impl Strategy<Value = ConfigKey> {
    prop::sample::select(vec![
        ConfigKey::ScopeUnlock,
        ConfigKey::ScopeLogon,
        ConfigKey::YawMaxDegrees,
        ConfigKey::PitchMaxDegrees,
        ConfigKey::FaceFailureCooldownSecs,
        ConfigKey::PadFailureCooldownSecs,
        ConfigKey::FailureWindowSecs,
        ConfigKey::MaxFailuresPerWindow,
        ConfigKey::MaxPadRejections,
        ConfigKey::PadEnabled,
        ConfigKey::PadThreshold,
        ConfigKey::MultiFaceRejection,
    ])
}

fn any_config_value() -> impl Strategy<Value = ConfigValue> {
    prop_oneof![
        any::<bool>().prop_map(ConfigValue::Bool),
        any::<u8>().prop_map(ConfigValue::U8),
        any::<u32>().prop_map(ConfigValue::U32),
        (0.0f32..90.0).prop_map(ConfigValue::F32),
    ]
}

fn any_status() -> impl Strategy<Value = StatusReport> {
    (
        any::<bool>(),
        any::<bool>(),
        prop::option::of(any_sid()),
        any::<bool>(),
        any::<bool>(),
        prop::option::of(".*"),
        any::<u32>(),
        any::<u16>(),
    )
        .prop_map(
            |(enabled, camera_ok, enrolled_sid, kill_switch, face_paused, pause_note, service_pid, template_count)| {
                StatusReport {
                    enabled,
                    camera_ok,
                    enrolled_sid,
                    kill_switch,
                    face_paused,
                    pause_note,
                    service_pid,
                    template_count,
                }
            },
        )
}

fn any_template_meta() -> impl Strategy<Value = TemplateMeta> {
    (
        any::<u64>(),
        any::<u64>(),
        ".*",
        any::<bool>(),
        any_pose(),
    )
        .prop_map(|(id, captured_at, lighting, glasses, pose)| TemplateMeta {
            id: TemplateId(id),
            captured_at,
            lighting,
            glasses,
            pose,
        })
}

fn any_test_report() -> impl Strategy<Value = TestReport> {
    (any::<bool>(), any::<bool>(), any::<u32>(), ".*").prop_map(
        |(completed, matched, latency_ms, detail)| TestReport {
            completed,
            matched,
            latency_ms,
            detail,
        },
    )
}

fn any_request() -> impl Strategy<Value = Request> {
    prop_oneof![
        any::<u8>().prop_map(|_| Request::Hello {
            proto: PROTO_VERSION
        }),
        (any_scenario(), any_sid(), any::<[u8; 16]>()).prop_map(|(scenario, sid, nonce)| {
            Request::BeginAttempt {
                scenario,
                sid,
                nonce,
            }
        }),
        any::<u64>().prop_map(|attempt| Request::Cancel { attempt }),
        (any::<u64>(), any::<i32>()).prop_map(|(attempt, nt_status)| Request::Report {
            attempt,
            nt_status,
        }),
    ]
}

fn any_response() -> impl Strategy<Value = Response> {
    prop_oneof![
        any::<u8>().prop_map(|_| Response::Hello {
            proto: PROTO_VERSION,
            svc_pid: 4242,
        }),
        (any::<u64>(), any_phase()).prop_map(|(attempt, phase)| Response::Status {
            attempt,
            phase,
        }),
        (
            any::<u64>(),
            any::<[u8; 16]>(),
            any_wide(),
            any_wide(),
            any_secret(),
        )
            .prop_map(|(attempt, nonce, user, domain, secret)| Response::Verified {
                attempt,
                nonce,
                user,
                domain,
                secret,
            }),
        (any::<u64>(), any_deny_reason()).prop_map(|(attempt, reason)| Response::Denied {
            attempt,
            reason,
        }),
        any::<u16>().prop_map(|code| Response::Error { code }),
    ]
}

fn any_admin_request() -> impl Strategy<Value = AdminRequest> {
    prop_oneof![
        any::<u8>().prop_map(|_| AdminRequest::Hello {
            proto: PROTO_VERSION
        }),
        any::<u8>().prop_map(|_| AdminRequest::GetStatus),
        any::<u8>().prop_map(|_| AdminRequest::ListTemplates),
        any::<u64>().prop_map(|id| AdminRequest::DeleteTemplate {
            id: TemplateId(id)
        }),
        any_enrol_mode().prop_map(|mode| AdminRequest::BeginEnrolment { mode }),
        any_password().prop_map(AdminRequest::EnrolPassword),
        any::<u8>().prop_map(|_| AdminRequest::SubmitSample),
        any::<u8>().prop_map(|_| AdminRequest::FinishEnrolment),
        any::<u8>().prop_map(|_| AdminRequest::CancelEnrolment),
        any::<u8>().prop_map(|_| AdminRequest::BeginPreview),
        any::<u8>().prop_map(|_| AdminRequest::EndPreview),
        any::<u8>().prop_map(|_| AdminRequest::BeginTest),
        any::<u8>().prop_map(|_| AdminRequest::BeginCalibration),
        any::<u8>().prop_map(|_| AdminRequest::GetConfig),
        (any_config_key(), any_config_value()).prop_map(|(key, value)| AdminRequest::SetConfig {
            key,
            value,
        }),
        any_password().prop_map(AdminRequest::RefreshCredential),
        any::<bool>().prop_map(AdminRequest::SetEnabled),
        any::<u8>().prop_map(|_| AdminRequest::ExportDiagnostics),
    ]
}

fn any_admin_response() -> impl Strategy<Value = AdminResponse> {
    prop_oneof![
        any::<u8>().prop_map(|_| AdminResponse::Hello {
            proto: PROTO_VERSION,
            svc_pid: 4242,
        }),
        any_status().prop_map(AdminResponse::Status),
        prop::collection::vec(any_template_meta(), 0..4).prop_map(AdminResponse::TemplateList),
        (any_enrol_stage(), any_pose(), any::<u8>(), any::<u8>()).prop_map(
            |(stage, pose, kept, needed)| AdminResponse::Progress {
                stage,
                pose,
                kept,
                needed,
            },
        ),
        prop::collection::vec((any_config_key(), any_config_value()), 0..6)
            .prop_map(|entries| AdminResponse::Config(kenali_core::config::ConfigReport { entries })),
        any_test_report().prop_map(AdminResponse::TestResult),
        (any::<u32>(), any::<u32>(), any_bytes()).prop_map(|(index, total, bytes)| {
            AdminResponse::DiagnosticsChunk {
                index,
                total,
                bytes,
            }
        }),
        any::<u16>().prop_map(|code| AdminResponse::Error { code }),
    ]
}

fn any_preview_frame() -> impl Strategy<Value = PreviewFrame> {
    (any::<u16>(), any::<u16>(), any_bytes()).prop_map(|(w, h, rgb)| PreviewFrame { w, h, rgb })
}

proptest! {
    #[test]
    fn auth_request_roundtrip(value in any_request()) {
        roundtrip(&value, MAX_CONTROL_BYTES);
    }

    #[test]
    fn auth_response_roundtrip(value in any_response()) {
        roundtrip(&value, MAX_CONTROL_BYTES);
    }

    #[test]
    fn admin_request_roundtrip(value in any_admin_request()) {
        roundtrip(&value, MAX_CONTROL_BYTES);
    }

    #[test]
    fn admin_response_roundtrip(value in any_admin_response()) {
        roundtrip(&value, MAX_CONTROL_BYTES);
    }

    #[test]
    fn preview_frame_roundtrip(value in any_preview_frame()) {
        roundtrip(&value, MAX_PREVIEW_BYTES);
    }

    #[test]
    fn wire_containers_roundtrip(wide in any_wide(), password in any_password(), secret in any_secret(), bytes in any_bytes()) {
        roundtrip(&wide, MAX_CONTROL_BYTES);
        roundtrip(&password, MAX_CONTROL_BYTES);
        roundtrip(&secret, MAX_CONTROL_BYTES);
        roundtrip(&bytes, MAX_CONTROL_BYTES);
    }
}
