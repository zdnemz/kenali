//! `kenali-camera` — Media Foundation capture and replay sources.
//! This is an FFI crate: `unsafe` is permitted here, but every unsafe block
//! must carry a `// SAFETY:` comment. Wave 1 declares the boundary only.

use windows::Win32::Foundation::HANDLE;

/// Placeholder handle alias tying this shell to the `windows` 0.62 bindings.
pub type CameraHandle = HANDLE;
