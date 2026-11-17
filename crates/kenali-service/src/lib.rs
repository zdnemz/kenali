#![forbid(unsafe_code)]

//! `kenali-service` — Windows service orchestration for Kenali.
//!
//! Wires the merged core policy/state, the typed IPC codec, the store AEAD, the
//! vision traits and the winapi server seam into the three pipe accept loops in
//! [`pipe_loop`]. This crate is `forbid(unsafe_code)`: raw Win32 byte pumping is
//! supplied to the loops as a deadline-bounded [`pipe_loop::PipeTransport`] by
//! the FFI composition layer. All message semantics, policy decisions, template
//! encryption, temporal fusion, preview wiping, redacted diagnostics and log
//! hygiene live here and are host-testable where they are platform-independent.
//!
//! No service is started, no registry written, no camera opened, no TPM key
//! unwrapped and no pipe created on this host — MSVC type-check only.

pub mod auth_attempt;
pub mod diagnostics;
pub mod logging;
pub mod pipe_loop;
pub mod pipeline;
pub mod preview;
pub mod template_store;

/// The Media Foundation source is owned by the service only during capture
/// windows; it stays behind the merged `FrameSource` seam. Kept here so the
/// camera backend is part of the service's composition surface.
#[cfg(windows)]
pub use kenali_camera::mf::MfSource;

#[cfg(windows)]
pub type ServiceCamera = MfSource;

/// Construct the production camera source (no camera is opened here; the
/// `FrameSource::open` call happens inside an enrolment/unlock window).
#[cfg(windows)]
pub fn open_camera_source() -> ServiceCamera {
    MfSource::new(0)
}

/// SCM workload for `KenaliSvc`. Establishes generic, redacted lifecycle
/// logging. The long-running, bounded accept loops are entered with a platform
/// transport by the owner-run SCM composition in a VM.
pub fn run() -> i32 {
    logging::log_service_start();
    #[cfg(windows)]
    {
        if matches!(kenali_winapi::registry::kill_switch_enabled(), Ok(true)) {
            logging::log_disabled();
        }
    }
    0
}
