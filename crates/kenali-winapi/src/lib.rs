//! `kenali-winapi` — Win32/COM bindings and service host helpers.
//! This is an FFI crate: `unsafe` is permitted here, but every unsafe block
//! must carry a `// SAFETY:` comment. Wave 1 declares the boundary only;
//! Wave 2 adds the auth-pipe client, the read-only kill switch and the
//! SCM/WTS helpers used by the credential provider. Wave 3 adds the server
//! side: SDDL-secured admin/preview pipes, peer-PID verification, the CNG/TPM
//! key hierarchy, the Event Log writer and the SCM service host.

pub mod admin_pipe;
pub mod com;
pub mod eventlog;
pub mod pipe_server;
pub mod pipes;
pub mod preview_pipe;
pub mod registry;
pub mod service;
pub mod service_host;
pub mod tpm;
pub mod ui;

#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;

/// Placeholder handle alias tying this shell to the `windows` 0.62 bindings.
#[cfg(windows)]
pub type WinHandle = HANDLE;
