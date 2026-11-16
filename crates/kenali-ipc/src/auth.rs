//! Auth-pipe messages (CP ⇄ service). Path: `\\.\pipe\kenali.auth`.

use kenali_core::types::{DenyReason, Phase, Scenario, Sid};
use serde::{Deserialize, Serialize};

use crate::wire::{SecretWide, Wide};

/// Requests sent from the Credential Provider to the service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Request {
    Hello {
        proto: u16,
    },
    BeginAttempt {
        scenario: Scenario,
        sid: Sid,
        nonce: [u8; 16],
    },
    Cancel {
        attempt: u64,
    },
    Report {
        attempt: u64,
        nt_status: i32,
    },
}

/// Responses sent from the service to the Credential Provider. `Response` is
/// deliberately not `Clone`: the `Verified` variant carries a one-shot secret.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Response {
    Hello {
        proto: u16,
        svc_pid: u32,
    },
    Status {
        attempt: u64,
        phase: Phase,
    },
    Verified {
        attempt: u64,
        nonce: [u8; 16],
        user: Wide,
        domain: Wide,
        secret: SecretWide,
    },
    Denied {
        attempt: u64,
        reason: DenyReason,
    },
    Error {
        code: u16,
    },
}
