//! `kenali-winapi` — Win32/COM bindings and service host helpers.
//! This is an FFI crate: `unsafe` is permitted here, but every unsafe block
//! must carry a `// SAFETY:` comment. Wave 1 declares the boundary only;
//! Wave 2 adds the auth-pipe client, the read-only kill switch and the
//! SCM/WTS helpers used by the credential provider.

pub mod com;
pub mod pipes;
pub mod registry;
pub mod service;

#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;

/// Placeholder handle alias tying this shell to the `windows` 0.62 bindings.
#[cfg(windows)]
pub type WinHandle = HANDLE;
