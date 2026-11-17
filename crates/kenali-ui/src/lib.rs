#![forbid(unsafe_code)]

//! `kenali-ui` — single-window egui/eframe management GUI for Kenali.
//!
//! The GUI talks to the service only over the typed admin pipe, plus a
//! short-lived, RAM-only preview pipe during capture windows. It never opens
//! the authentication pipe, never persists a preview frame, and never writes
//! service-owned state. Every user-visible phrase comes from the shared,
//! score-free string table in `kenali_core`.

pub mod admin_client;
pub mod dashboard;
pub mod diagnostics;
pub mod enrol;
pub mod password_field;
pub mod pipe_worker;
pub mod preview;
pub mod security;
pub mod settings;
pub mod strings;
pub mod templates;
pub mod test_bench;
pub mod theme;
pub mod window;

/// Errors surfaced to the GUI. Wording is intentionally generic: no secret,
/// internal reason code, path or account identifier is ever exposed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    #[error("the management channel is not available")]
    WorkerStopped,
    #[error("the management channel was already initialised")]
    AlreadyInitialised,
    #[error("the management channel is not initialised")]
    NotInitialised,
    #[error("the management worker could not be started")]
    WorkerSpawn,
}
