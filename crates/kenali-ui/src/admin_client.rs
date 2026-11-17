//! Single admin-channel call site for every screen.
//!
//! Screens never touch the pipe themselves: they build a typed `PipeCommand`
//! and hand it to [`send`]. The one `AdminPipeClient` connection is owned by the
//! pipe worker thread; this module only forwards commands onto that worker's
//! bounded channel.

use std::sync::OnceLock;

use crossbeam_channel::Sender;

use crate::pipe_worker::PipeCommand;
use crate::AppError;

static COMMAND_SENDER: OnceLock<Sender<PipeCommand>> = OnceLock::new();

/// Install the worker's command sender exactly once, at GUI startup.
pub fn install(sender: Sender<PipeCommand>) -> Result<(), AppError> {
    COMMAND_SENDER
        .set(sender)
        .map_err(|_| AppError::AlreadyInitialised)
}

/// Send one typed management command. This is the only call site used by the
/// seven screens. A full bounded queue or a stopped worker surfaces as an
/// `AppError`; nothing is logged.
pub fn send(command: PipeCommand) -> Result<(), AppError> {
    let sender = COMMAND_SENDER.get().ok_or(AppError::NotInitialised)?;
    sender.send(command).map_err(|_| AppError::WorkerStopped)
}
