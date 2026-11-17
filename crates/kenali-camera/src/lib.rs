//! `kenali-camera` — Media Foundation capture, replay and synthetic sources.
//!
//! This is an FFI crate: `unsafe` is permitted here, but every unsafe block
//! must carry a `// SAFETY:` comment. The camera is owned by the service in
//! production; replay/synthetic sources exist only for tests and VMs.
//! OWNER-RUN VERIFY: the live Session-0 camera capture path is not exercised
//! on this host — MSVC type-check only.

use windows::Win32::Foundation::HANDLE;

/// Placeholder handle alias tying this shell to the `windows` 0.62 bindings.
pub type CameraHandle = HANDLE;

pub mod mf;
pub mod pin;
pub mod replay;
pub mod synthetic;

// Re-export the three `kenali_core::frame::FrameSource` implementations and
// the pin helpers so callers depend on one crate surface.
pub use mf::MfSource;
pub use pin::{enumerate_pins, verify_pin};
pub use replay::FileSource;
pub use synthetic::SyntheticSource;
