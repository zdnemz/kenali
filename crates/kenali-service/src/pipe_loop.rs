//! Pipe accept loops and typed message orchestration for all three channels.
//!
//! Connection setup (SYSTEM-only SDDL, bounded `ConnectNamedPipe`, peer-PID
//! check) uses the merged `kenali_winapi::pipe_server` seam. Byte transport is a
//! narrow, deadline-bounded [`PipeTransport`] injected at composition time;
//! this crate is `forbid(unsafe_code)` and therefore does not perform raw Win32
//! I/O itself. All message semantics live here: every payload is decoded with
//! the hostile-input-capped codec (`PROTO_VERSION` handshake), driven through
//! the policy/state machines, and re-encoded as a typed response. Every loop is
//! bounded; on cancel/disconnect/timeout the preview slot is wiped and the
//! auth machine fails open to the password tile.

use std::time::{Duration, Instant};

use kenali_core::config::{validate_config, ConfigError, ConfigReport};
use kenali_core::types::{EnrolMode, EnrolStage, Phase, Sid, StatusReport, TemplateMeta};
use kenali_ipc::admin::{AdminRequest, AdminResponse};
use kenali_ipc::auth::{Request, Response};
use kenali_ipc::codec::{self, MAX_CONTROL_BYTES};
use kenali_ipc::PROTO_VERSION;
use zeroize::Zeroizing;

use crate::auth_attempt::{AuthAttemptMachine, BeginOutcome};
use crate::diagnostics;
use crate::preview::PreviewSlot;

#[cfg(windows)]
use kenali_winapi::pipe_server::{
    accept_client, create_server_pipe, verify_peer_pid, ADMIN_PIPE_PATH, ADMIN_PIPE_SDDL,
    PREVIEW_PIPE_PATH, PREVIEW_PIPE_SDDL,
};
#[cfg(windows)]
use kenali_winapi::pipes::{AUTH_PIPE_PATH, AUTH_PIPE_SDDL};
#[cfg(windows)]
use kenali_winapi::WinHandle;

/// Errors from the service orchestration. Wording is generic: no secret,
/// score, path or biometric detail ever appears.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServiceError {
    #[error("pipe is unavailable")]
    Unavailable,
    #[error("pipe operation timed out")]
    Timeout,
    #[error("peer identity could not be verified")]
    Identity,
    #[error("malformed pipe message")]
    Malformed,
    #[error("protocol version mismatch")]
    Protocol,
    #[error("pipe transport failed")]
    Transport,
    #[error("pipe I/O failed")]
    Io,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum TransportError {
    #[error("transport timed out")]
    Timeout,
    #[error("peer disconnected")]
    Disconnected,
    #[error("transport I/O failed")]
    Io,
    #[error("message exceeds the transport cap")]
    TooLarge,
}

impl From<TransportError> for ServiceError {
    fn from(error: TransportError) -> Self {
        match error {
            TransportError::Timeout => ServiceError::Timeout,
            TransportError::Disconnected
            | TransportError::Io
            | TransportError::TooLarge => ServiceError::Transport,
        }
    }
}

/// A connected pipe's byte pump. Every call is deadline-bounded. The concrete
/// Win32 implementation belongs to the FFI composition layer; a RAM-only one is
/// used by the host tests.
pub trait PipeTransport {
    /// Read one framed message into `buf`, returning the byte count.
    fn read_message(&mut self, buf: &mut [u8], deadline: Instant) -> Result<usize, TransportError>;
    /// Write one framed message before `deadline`.
    fn write_message(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), TransportError>;
}

/// Bounded-timing configuration shared by all three pipes.
#[derive(Debug, Clone, Copy)]
pub struct PipeServerConfig {
    pub accept_timeout: Duration,
    pub io_timeout: Duration,
    pub svc_pid: u32,
    pub expected_peer_pid: u32,
}

impl Default for PipeServerConfig {
    fn default() -> Self {
        let pid = std::process::id();
        Self {
            accept_timeout: Duration::from_secs(2),
            io_timeout: Duration::from_secs(6),
            svc_pid: pid,
            expected_peer_pid: pid,
        }
    }
}

/// Decode one auth request, drive the machine, encode the typed response.
pub fn handle_auth_bytes(
    bytes: &[u8],
    machine: &mut AuthAttemptMachine,
    svc_pid: u32,
    now: Instant,
) -> Result<Vec<u8>, ServiceError> {
    let request = codec::decode_auth_request(bytes).map_err(|_| ServiceError::Malformed)?;
    let response = match request {
        Request::Hello { proto } => {
            if proto != PROTO_VERSION {
                return Err(ServiceError::Protocol);
            }
            Response::Hello {
                proto: PROTO_VERSION,
                svc_pid,
            }
        }
        Request::BeginAttempt { scenario, sid, nonce } => {
            let (attempt, outcome) = machine.begin(scenario, sid, nonce, now);
            match outcome {
                BeginOutcome::Searching => Response::Status {
                    attempt,
                    phase: Phase::Searching,
                },
                BeginOutcome::Challenge(_) => Response::Status {
                    attempt,
                    phase: Phase::Challenge,
                },
                BeginOutcome::Denied(reason) => Response::Denied { attempt, reason },
                BeginOutcome::PasswordFallback => Response::Status {
                    attempt,
                    phase: Phase::Fallback,
                },
            }
        }
        Request::Cancel { attempt } => {
            machine.cancel(now);
            Response::Status {
                attempt,
                phase: Phase::Armed,
            }
        }
        Request::Report { attempt, nt_status } => {
            machine.report(nt_status, now);
            Response::Status {
                attempt,
                phase: Phase::Fallback,
            }
        }
    };
    codec::encode(&response).map_err(|_| ServiceError::Malformed)
}

/// Minimal admin orchestration over in-memory state. No file path is ever
/// accepted: diagnostics are built from this state and returned as chunks.
pub struct AdminService {
    config: ConfigReport,
    enabled: bool,
    enrolled_sid: Option<Sid>,
    preview_open: bool,
    templates: Vec<TemplateMeta>,
}

impl AdminService {
    pub fn new(enrolled_sid: Option<Sid>) -> Self {
        Self {
            config: ConfigReport::default_policy(),
            enabled: true,
            enrolled_sid,
            preview_open: false,
            templates: Vec::new(),
        }
    }

    pub fn config(&self) -> &ConfigReport {
        &self.config
    }

    pub fn is_preview_open(&self) -> bool {
        self.preview_open
    }

    pub fn status_report(&self, svc_pid: u32) -> StatusReport {
        StatusReport {
            enabled: self.enabled,
            camera_ok: true,
            enrolled_sid: self.enrolled_sid.clone(),
            kill_switch: !self.enabled,
            face_paused: false,
            pause_note: None,
            service_pid: svc_pid,
            template_count: u16::try_from(self.templates.len()).unwrap_or(u16::MAX),
        }
    }

    /// Handle one typed admin request, returning zero or more typed responses.
    pub fn handle(
        &mut self,
        request: AdminRequest,
        svc_pid: u32,
    ) -> Vec<AdminResponse> {
        match request {
            AdminRequest::Hello { proto } => {
                if proto != PROTO_VERSION {
                    return vec![AdminResponse::Error { code: 2 }];
                }
                vec![AdminResponse::Hello {
                    proto: PROTO_VERSION,
                    svc_pid,
                }]
            }
            AdminRequest::GetStatus => vec![AdminResponse::Status(self.status_report(svc_pid))],
            AdminRequest::ListTemplates => {
                vec![AdminResponse::TemplateList(self.templates.clone())]
            }
            AdminRequest::DeleteTemplate { id } => {
                self.templates.retain(|meta| meta.id != id);
                vec![AdminResponse::TemplateList(self.templates.clone())]
            }
            AdminRequest::GetConfig => vec![AdminResponse::Config(self.config.clone())],
            AdminRequest::SetConfig { key, value } => {
                match validate_config(&self.config, &[(key, value)]) {
                    Ok(next) => {
                        self.config = next;
                        vec![AdminResponse::Config(self.config.clone())]
                    }
                    Err(error) => {
                        let code = match error {
                            ConfigError::ReadOnly => 3,
                            ConfigError::WouldLoosen => 4,
                            ConfigError::OutOfRange => 5,
                            _ => 1,
                        };
                        vec![AdminResponse::Error { code }]
                    }
                }
            }
            AdminRequest::SetEnabled(enabled) => {
                // The registry `Disabled` value is the inverse of `enabled`.
                match kenali_winapi::registry::set_kill_switch(!enabled) {
                    Ok(()) => {
                        self.enabled = enabled;
                        vec![AdminResponse::Status(self.status_report(svc_pid))]
                    }
                    Err(_) => vec![AdminResponse::Error { code: 6 }],
                }
            }
            AdminRequest::ExportDiagnostics => {
                diagnostics::export(&self.status_report(svc_pid), &self.config)
            }
            AdminRequest::BeginPreview | AdminRequest::BeginCalibration => {
                self.preview_open = true;
                Vec::new()
            }
            AdminRequest::EndPreview | AdminRequest::CancelEnrolment => {
                self.preview_open = false;
                Vec::new()
            }
            AdminRequest::BeginEnrolment { mode } => {
                self.preview_open = true;
                let stage = match mode {
                    EnrolMode::New | EnrolMode::AddTemplate => EnrolStage::Prerequisites,
                    EnrolMode::RefreshCredential => EnrolStage::Password,
                    _ => EnrolStage::Prerequisites,
                };
                vec![AdminResponse::Progress {
                    stage,
                    pose: kenali_core::types::Pose::Straight,
                    kept: 0,
                    needed: 0,
                }]
            }
            AdminRequest::EnrolPassword(password) | AdminRequest::RefreshCredential(password) => {
                let _secret = password.into_secret();
                Vec::new()
            }
            AdminRequest::SubmitSample | AdminRequest::FinishEnrolment | AdminRequest::BeginTest => {
                Vec::new()
            }
        }
    }
}

/// Decode an admin request and encode each typed response.
pub fn handle_admin_bytes(
    bytes: &[u8],
    admin: &mut AdminService,
    svc_pid: u32,
) -> Result<Vec<Vec<u8>>, ServiceError> {
    let request = codec::decode_admin_request(bytes).map_err(|_| ServiceError::Malformed)?;
    let responses = admin.handle(request, svc_pid);
    let mut encoded = Vec::with_capacity(responses.len());
    for response in &responses {
        encoded.push(codec::encode(response).map_err(|_| ServiceError::Malformed)?);
    }
    Ok(encoded)
}

// ---- Bounded accept loops (Windows only) ------------------------------------

#[cfg(windows)]
fn open_and_verify(
    path: &str,
    sddl: &str,
    config: &PipeServerConfig,
) -> Result<WinHandle, ServiceError> {
    let deadline = Instant::now() + config.accept_timeout;
    let listener = create_server_pipe(path, sddl).map_err(|_| ServiceError::Unavailable)?;
    let connected = accept_client(listener, deadline).map_err(|error| match error {
        kenali_winapi::pipes::PipeError::Timeout => ServiceError::Timeout,
        _ => ServiceError::Io,
    })?;
    verify_peer_pid(connected, config.expected_peer_pid).map_err(|_| ServiceError::Identity)?;
    Ok(connected)
}

/// Accept one authenticated CP, then run the bounded auth request/response loop.
/// On any disconnect the machine is driven to `ServiceDown` so the tile fails
/// open to the password prompt.
#[cfg(windows)]
pub fn serve_auth_pipe<T, F>(
    config: &PipeServerConfig,
    make_transport: F,
    machine: &mut AuthAttemptMachine,
) -> Result<(), ServiceError>
where
    T: PipeTransport,
    F: FnOnce(WinHandle) -> T,
{
    let connected = open_and_verify(AUTH_PIPE_PATH, AUTH_PIPE_SDDL, config)?;
    let mut transport = make_transport(connected);
    loop {
        let deadline = Instant::now() + config.io_timeout;
        let mut buf = Zeroizing::new(vec![0u8; MAX_CONTROL_BYTES]);
        let len = match transport.read_message(buf.as_mut_slice(), deadline) {
            Ok(len) if len > 0 && len <= buf.len() => len,
            Ok(_) => break,
            Err(TransportError::Disconnected) | Err(TransportError::Timeout) => break,
            Err(_) => return Err(ServiceError::Transport),
        };
        let encoded = match handle_auth_bytes(&buf[..len], machine, config.svc_pid, Instant::now()) {
            Ok(bytes) => Zeroizing::new(bytes),
            Err(ServiceError::Protocol) => return Err(ServiceError::Protocol),
            Err(_) => continue,
        };
        transport.write_message(encoded.as_slice(), Instant::now() + config.io_timeout)?;

        for pending in machine.take_pending() {
            let bytes = match codec::encode(&pending) {
                Ok(bytes) => Zeroizing::new(bytes),
                Err(_) => continue,
            };
            transport.write_message(bytes.as_slice(), Instant::now() + config.io_timeout)?;
        }
    }
    machine.service_down(Instant::now());
    Ok(())
}

/// Accept one elevated admin client and run the bounded control loop.
#[cfg(windows)]
pub fn serve_admin_pipe<T, F>(
    config: &PipeServerConfig,
    make_transport: F,
    admin: &mut AdminService,
) -> Result<(), ServiceError>
where
    T: PipeTransport,
    F: FnOnce(WinHandle) -> T,
{
    let connected = open_and_verify(ADMIN_PIPE_PATH, ADMIN_PIPE_SDDL, config)?;
    let mut transport = make_transport(connected);
    loop {
        let deadline = Instant::now() + config.io_timeout;
        let mut buf = Zeroizing::new(vec![0u8; MAX_CONTROL_BYTES]);
        let len = match transport.read_message(buf.as_mut_slice(), deadline) {
            Ok(len) if len > 0 && len <= buf.len() => len,
            Ok(_) => break,
            Err(TransportError::Disconnected) | Err(TransportError::Timeout) => break,
            Err(_) => return Err(ServiceError::Transport),
        };
        let responses = match handle_admin_bytes(&buf[..len], admin, config.svc_pid) {
            Ok(responses) => responses,
            Err(_) => continue,
        };
        for response in responses {
            let bytes = Zeroizing::new(response);
            transport.write_message(bytes.as_slice(), Instant::now() + config.io_timeout)?;
        }
    }
    Ok(())
}

/// Serve preview frames only inside an enrolment/calibration window. The pipe
/// is created on entry and dropped (closed) when the window deadline passes;
/// the slot is wiped on every exit path.
#[cfg(windows)]
pub fn serve_preview_pipe<T, F>(
    config: &PipeServerConfig,
    window_deadline: Instant,
    make_transport: F,
    preview: &mut PreviewSlot,
) -> Result<(), ServiceError>
where
    T: PipeTransport,
    F: FnOnce(WinHandle) -> T,
{
    let connected = open_and_verify(PREVIEW_PIPE_PATH, PREVIEW_PIPE_SDDL, config)?;
    let mut transport = make_transport(connected);
    preview.begin();
    let result = (|| -> Result<(), ServiceError> {
        while Instant::now() < window_deadline {
            if let Some(frame) = preview.snapshot() {
                let bytes = match codec::encode(&frame) {
                    Ok(bytes) => Zeroizing::new(bytes),
                    Err(_) => return Err(ServiceError::Malformed),
                };
                transport.write_message(bytes.as_slice(), Instant::now() + config.io_timeout)?;
            } else {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(())
    })();
    preview.end();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use kenali_core::config::{ConfigKey, ConfigValue};

    fn encode_request(value: &Request) -> Option<Vec<u8>> {
        match codec::encode(value) {
            Ok(bytes) => Some(bytes),
            Err(_) => None,
        }
    }

    fn encode_admin(value: &AdminRequest) -> Option<Vec<u8>> {
        match codec::encode(value) {
            Ok(bytes) => Some(bytes),
            Err(_) => None,
        }
    }

    fn decode_response(bytes: &[u8]) -> Option<Response> {
        match codec::decode(bytes, MAX_CONTROL_BYTES) {
            Ok(value) => Some(value),
            Err(_) => None,
        }
    }

    fn decode_admin_response(bytes: &[u8]) -> Option<AdminResponse> {
        match codec::decode(bytes, MAX_CONTROL_BYTES) {
            Ok(value) => Some(value),
            Err(_) => None,
        }
    }

    #[test]
    fn hello_handshake_uses_the_pinned_proto() {
        let bytes = match encode_request(&Request::Hello { proto: PROTO_VERSION }) {
            Some(bytes) => bytes,
            None => {
                assert!(false, "encode must succeed");
                return;
            }
        };
        let mut machine = AuthAttemptMachine::with_defaults();
        let encoded = match handle_auth_bytes(&bytes, &mut machine, 77, Instant::now()) {
            Ok(encoded) => encoded,
            Err(_) => {
                assert!(false, "hello must decode");
                return;
            }
        };
        let response: Response = match decode_response(&encoded) {
            Some(response) => response,
            None => {
                assert!(false, "response must decode");
                return;
            }
        };
        match response {
            Response::Hello { proto, svc_pid } => {
                assert_eq!(proto, PROTO_VERSION);
                assert_eq!(svc_pid, 77);
            }
            _ => assert!(false, "expected Hello"),
        }
    }

    #[test]
    fn wrong_proto_is_rejected() {
        let bytes = match encode_request(&Request::Hello { proto: PROTO_VERSION + 1 }) {
            Some(bytes) => bytes,
            None => {
                assert!(false, "encode must succeed");
                return;
            }
        };
        let mut machine = AuthAttemptMachine::with_defaults();
        match handle_auth_bytes(&bytes, &mut machine, 1, Instant::now()) {
            Err(ServiceError::Protocol) => {}
            _ => assert!(false, "foreign proto must be rejected"),
        }
    }

    #[test]
    fn begin_attempt_maps_policy_to_a_typed_status() {
        let bytes = match encode_request(&Request::BeginAttempt {
            scenario: kenali_core::types::Scenario::Unlock,
            sid: Sid::new("S-1-test"),
            nonce: [9u8; 16],
        }) {
            Some(bytes) => bytes,
            None => {
                assert!(false, "encode must succeed");
                return;
            }
        };
        let mut machine = AuthAttemptMachine::with_defaults();
        let encoded = match handle_auth_bytes(&bytes, &mut machine, 1, Instant::now()) {
            Ok(encoded) => encoded,
            Err(_) => {
                assert!(false, "begin must decode");
                return;
            }
        };
        let response: Response = match decode_response(&encoded) {
            Some(response) => response,
            None => {
                assert!(false, "response must decode");
                return;
            }
        };
        assert!(matches!(response, Response::Status { phase: Phase::Fallback, .. }));
    }

    #[test]
    fn admin_hello_uses_the_pinned_proto() {
        let mut admin = AdminService::new(Some(Sid::new("S-1-test")));
        let bytes = match encode_admin(&AdminRequest::Hello { proto: PROTO_VERSION }) {
            Some(bytes) => bytes,
            None => {
                assert!(false, "encode must succeed");
                return;
            }
        };
        let encoded = match handle_admin_bytes(&bytes, &mut admin, 5) {
            Ok(encoded) => encoded,
            Err(_) => {
                assert!(false, "hello must decode");
                return;
            }
        };
        let response: AdminResponse = match decode_admin_response(&encoded[0]) {
            Some(response) => response,
            None => {
                assert!(false, "response must decode");
                return;
            }
        };
        assert!(matches!(response, AdminResponse::Hello { .. }));
    }

    #[test]
    fn admin_rejects_loosening_config() {
        let mut admin = AdminService::new(None);
        let responses = admin.handle(
            AdminRequest::SetConfig {
                key: ConfigKey::ScopeLogon,
                value: ConfigValue::Bool(true),
            },
            1,
        );
        assert!(responses.iter().any(|r| matches!(r, AdminResponse::Error { .. })));
    }

    #[test]
    fn diagnostics_chunks_are_typed() {
        let mut admin = AdminService::new(None);
        let responses = admin.handle(AdminRequest::ExportDiagnostics, 1);
        assert!(responses.iter().any(|r| matches!(r, AdminResponse::DiagnosticsChunk { .. })));
        assert!(admin.handle(AdminRequest::BeginPreview, 1).is_empty());
        assert!(admin.is_preview_open());
    }
}
