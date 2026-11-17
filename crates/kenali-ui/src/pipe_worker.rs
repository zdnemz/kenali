//! One worker thread owns the single admin-pipe client and an optional
//! short-lived preview-pipe client. Screens send typed `PipeCommand`s down a
//! bounded channel; the worker emits typed `PipeEvent`s back. There is no async
//! runtime and every path handles errors explicitly.

use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender};

use kenali_core::config::{ConfigKey, ConfigReport, ConfigValue};
use kenali_core::secret::Password;
use kenali_core::types::{
    EnrolMode, EnrolStage, Pose, StatusReport, TemplateId, TemplateMeta, TestReport,
};
use kenali_ipc::admin::{AdminRequest, AdminResponse};
use kenali_ipc::preview::PreviewFrame;
use kenali_ipc::wire::password_into_wide;
use kenali_ipc::PROTO_VERSION;
use kenali_winapi::admin_pipe::{AdminPipeClient, ADMIN_CONNECT_TIMEOUT};
use kenali_winapi::preview_pipe::PreviewPipeClient;

/// Typed commands sent by the screens. A 1:1 mirror of `AdminRequest`, plus the
/// local-only `Shutdown` that stops the worker. Password variants carry the
/// `Password` secret newtype — never a `String`.
#[derive(Debug)]
#[non_exhaustive]
pub enum PipeCommand {
    Hello { proto: u16 },
    GetStatus,
    ListTemplates,
    DeleteTemplate { id: TemplateId },
    BeginEnrolment { mode: EnrolMode },
    EnrolPassword(Password),
    SubmitSample,
    FinishEnrolment,
    CancelEnrolment,
    BeginPreview,
    EndPreview,
    BeginTest,
    BeginCalibration,
    GetConfig,
    SetConfig { key: ConfigKey, value: ConfigValue },
    RefreshCredential(Password),
    SetEnabled(bool),
    ExportDiagnostics,
    Shutdown,
}

/// Typed events emitted by the worker: the `AdminResponse` mirror plus RAM-only
/// `Preview` frames and the terminal `Disconnected` notification.
#[derive(Debug)]
#[non_exhaustive]
pub enum PipeEvent {
    Hello { proto: u16, svc_pid: u32 },
    Status(StatusReport),
    TemplateList(Vec<TemplateMeta>),
    Progress { stage: EnrolStage, pose: Pose, kept: u8, needed: u8 },
    Config(ConfigReport),
    TestResult(TestReport),
    DiagnosticsChunk { index: u32, total: u32, bytes: Vec<u8> },
    Preview(PreviewFrame),
    Error { code: u16 },
    Disconnected,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WorkerError {
    #[error("the management worker could not be started")]
    Spawn,
    #[error("the management worker has stopped")]
    Stopped,
}

/// Owns the command sender and the worker thread's join handle.
pub struct WorkerHandle {
    command: Sender<PipeCommand>,
    thread: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    /// Borrow the bounded command sender (installed into `admin_client`).
    pub fn sender(&self) -> &Sender<PipeCommand> {
        &self.command
    }

    /// Enqueue one command; a full or closed queue is a `WorkerError`.
    pub fn send(&self, command: PipeCommand) -> Result<(), WorkerError> {
        self.command.send(command).map_err(|_| WorkerError::Stopped)
    }

    /// Ask the worker to stop and wait for it. Used at shutdown by the owner.
    pub fn shutdown(mut self) {
        let _ = self.command.send(PipeCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Spawn the single worker with bounded channels (commands cap 16, events cap 64).
pub fn spawn_worker() -> Result<(WorkerHandle, Receiver<PipeEvent>), WorkerError> {
    let (command_tx, command_rx) = crossbeam_channel::bounded::<PipeCommand>(16);
    let (event_tx, event_rx) = crossbeam_channel::bounded::<PipeEvent>(64);
    let thread = thread::Builder::new()
        .name("kenali-pipe-worker".to_owned())
        .spawn(move || worker_main(command_rx, event_tx))
        .map_err(|_| WorkerError::Spawn)?;

    let handle = WorkerHandle {
        command: command_tx,
        thread: Some(thread),
    };
    Ok((handle, event_rx))
}

/// Worker body: connect, handshake, then service commands until shutdown or a
/// closed channel. Preview frames are pumped between commands while active.
fn worker_main(commands: Receiver<PipeCommand>, events: Sender<PipeEvent>) {
    let mut admin = match AdminPipeClient::connect(ADMIN_CONNECT_TIMEOUT) {
        Ok(client) => client,
        Err(_) => {
            let _ = events.send(PipeEvent::Disconnected);
            return;
        }
    };

    let hello = AdminRequest::Hello {
        proto: PROTO_VERSION,
    };
    match admin.exchange(&hello) {
        Ok(AdminResponse::Hello { proto, svc_pid }) => {
            if events
                .send(PipeEvent::Hello { proto, svc_pid })
                .is_err()
            {
                return;
            }
        }
        _ => {
            let _ = events.send(PipeEvent::Disconnected);
            return;
        }
    }

    let mut preview: Option<PreviewPipeClient> = None;
    let poll = Duration::from_millis(16);
    loop {
        let alive = if preview.is_some() {
            match commands.recv_timeout(poll) {
                Ok(command) => handle_command(command, &mut admin, &mut preview, &events),
                Err(RecvTimeoutError::Timeout) => pump_preview(preview.as_mut(), &events),
                Err(RecvTimeoutError::Disconnected) => false,
            }
        } else {
            match commands.recv() {
                Ok(command) => handle_command(command, &mut admin, &mut preview, &events),
                Err(_) => false,
            }
        };
        if !alive {
            break;
        }
    }
    let _ = events.send(PipeEvent::Disconnected);
}

/// Read one preview frame (when a client is active) without blocking the loop.
fn pump_preview(preview: Option<&mut PreviewPipeClient>, events: &Sender<PipeEvent>) -> bool {
    let client = match preview {
        Some(client) => client,
        None => return true,
    };
    let deadline = Instant::now() + Duration::from_millis(16);
    match client.recv_frame(deadline) {
        Ok(frame) => events.send(PipeEvent::Preview(frame)).is_ok(),
        Err(_) => true,
    }
}

/// Map one command to its wire request and emit the resulting reply as an event.
/// Returns false when the loop should stop.
fn handle_command(
    command: PipeCommand,
    admin: &mut AdminPipeClient,
    preview: &mut Option<PreviewPipeClient>,
    events: &Sender<PipeEvent>,
) -> bool {
    if matches!(command, PipeCommand::Shutdown) {
        return false;
    }
    if matches!(command, PipeCommand::BeginPreview | PipeCommand::BeginCalibration) {
        match PreviewPipeClient::connect(ADMIN_CONNECT_TIMEOUT) {
            Ok(client) => *preview = Some(client),
            Err(_) => {
                if events.send(PipeEvent::Error { code: 0 }).is_err() {
                    return false;
                }
            }
        }
    }
    if matches!(command, PipeCommand::EndPreview) {
        *preview = None;
    }

    let request = match to_request(command) {
        Some(request) => request,
        None => return true,
    };
    match admin.exchange(&request) {
        Ok(response) => match from_response(response) {
            Some(event) => events.send(event).is_ok(),
            None => true,
        },
        Err(_) => events.send(PipeEvent::Error { code: 0 }).is_ok(),
    }
}

/// 1:1 command → wire request mapping. `Shutdown` has no wire counterpart.
fn to_request(command: PipeCommand) -> Option<AdminRequest> {
    let request = match command {
        PipeCommand::Hello { proto } => AdminRequest::Hello { proto },
        PipeCommand::GetStatus => AdminRequest::GetStatus,
        PipeCommand::ListTemplates => AdminRequest::ListTemplates,
        PipeCommand::DeleteTemplate { id } => AdminRequest::DeleteTemplate { id },
        PipeCommand::BeginEnrolment { mode } => AdminRequest::BeginEnrolment { mode },
        PipeCommand::EnrolPassword(password) => {
            AdminRequest::EnrolPassword(password_into_wide(password))
        }
        PipeCommand::SubmitSample => AdminRequest::SubmitSample,
        PipeCommand::FinishEnrolment => AdminRequest::FinishEnrolment,
        PipeCommand::CancelEnrolment => AdminRequest::CancelEnrolment,
        PipeCommand::BeginPreview => AdminRequest::BeginPreview,
        PipeCommand::EndPreview => AdminRequest::EndPreview,
        PipeCommand::BeginTest => AdminRequest::BeginTest,
        PipeCommand::BeginCalibration => AdminRequest::BeginCalibration,
        PipeCommand::GetConfig => AdminRequest::GetConfig,
        PipeCommand::SetConfig { key, value } => AdminRequest::SetConfig { key, value },
        PipeCommand::RefreshCredential(password) => {
            AdminRequest::RefreshCredential(password_into_wide(password))
        }
        PipeCommand::SetEnabled(enabled) => AdminRequest::SetEnabled(enabled),
        PipeCommand::ExportDiagnostics => AdminRequest::ExportDiagnostics,
        PipeCommand::Shutdown => return None,
    };
    Some(request)
}

/// Wire reply → GUI event. Diagnostics bytes become a plain RAM buffer that the
/// Diagnostics screen writes to an operator-chosen path; nothing is logged.
fn from_response(response: AdminResponse) -> Option<PipeEvent> {
    let event = match response {
        AdminResponse::Hello { proto, svc_pid } => PipeEvent::Hello { proto, svc_pid },
        AdminResponse::Status(report) => PipeEvent::Status(report),
        AdminResponse::TemplateList(list) => PipeEvent::TemplateList(list),
        AdminResponse::Progress {
            stage,
            pose,
            kept,
            needed,
        } => PipeEvent::Progress {
            stage,
            pose,
            kept,
            needed,
        },
        AdminResponse::Config(report) => PipeEvent::Config(report),
        AdminResponse::TestResult(report) => PipeEvent::TestResult(report),
        AdminResponse::DiagnosticsChunk {
            index,
            total,
            bytes,
        } => PipeEvent::DiagnosticsChunk {
            index,
            total,
            bytes: bytes.into_inner(),
        },
        AdminResponse::Error { code } => PipeEvent::Error { code },
    };
    Some(event)
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn command_mapping_is_one_to_one() {
        let blank = Password::from_wide(Vec::new());
        assert!(matches!(
            to_request(PipeCommand::GetStatus),
            Some(AdminRequest::GetStatus)
        ));
        assert!(matches!(
            to_request(PipeCommand::EnrolPassword(blank)),
            Some(AdminRequest::EnrolPassword(_))
        ));
        assert!(to_request(PipeCommand::Shutdown).is_none());
    }
}
