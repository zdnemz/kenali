//! Admin-pipe messages (UI / recovery CLI ⇄ service). Path:
//! `\\.\pipe\kenali.admin`. No file paths ever cross this boundary.

use kenali_core::config::{ConfigKey, ConfigReport, ConfigValue};
use kenali_core::types::{
    EnrolMode, EnrolStage, Pose, StatusReport, TemplateId, TemplateMeta, TestReport,
};
use serde::{Deserialize, Serialize};

use crate::wire::{Bytes, PasswordWide};

/// Typed management commands. Not `Clone`: password-bearing variants must not
/// be duplicated in memory.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminRequest {
    Hello {
        proto: u16,
    },
    GetStatus,
    ListTemplates,
    DeleteTemplate {
        id: TemplateId,
    },
    BeginEnrolment {
        mode: EnrolMode,
    },
    EnrolPassword(PasswordWide),
    SubmitSample,
    FinishEnrolment,
    CancelEnrolment,
    BeginPreview,
    EndPreview,
    BeginTest,
    BeginCalibration,
    GetConfig,
    SetConfig {
        key: ConfigKey,
        value: ConfigValue,
    },
    RefreshCredential(PasswordWide),
    SetEnabled(bool),
    ExportDiagnostics,
}

/// Replies on the admin pipe. Diagnostics chunks are also read from this pipe
/// with the tighter `MAX_DIAG_CHUNK_BYTES` cap.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminResponse {
    Hello {
        proto: u16,
        svc_pid: u32,
    },
    Status(StatusReport),
    TemplateList(Vec<TemplateMeta>),
    Progress {
        stage: EnrolStage,
        pose: Pose,
        kept: u8,
        needed: u8,
    },
    Config(ConfigReport),
    TestResult(TestReport),
    DiagnosticsChunk {
        index: u32,
        total: u32,
        bytes: Bytes,
    },
    Error {
        code: u16,
    },
}
