#![forbid(unsafe_code)]

//! `kenali-ipc` — typed wire messages and postcard codec.
//!
//! Three named pipes are described by this crate (path strings are documentation
//! only; pipe creation lives in the Windows service, not here):
//!
//! - auth pipe `\\.\pipe\kenali.auth` — CP ⇄ service
//! - admin pipe `\\.\pipe\kenali.admin` — UI / recovery CLI ⇄ service
//! - preview pipe `\\.\pipe\kenali.preview` — short-lived, enrolment/calibration only
//!
//! Every byte arriving on any pipe is hostile. [`codec`] caps total message
//! size *before* deserializing and [`wire`] enforces per-field caps while
//! deserializing, element by element, so a forged length prefix never drives
//! a huge allocation. Secrets (`PasswordWide`, `SecretWide`) are redacted in
//! `Debug`, never cloned, and wiped on drop.

pub mod admin;
pub mod auth;
pub mod codec;
pub mod preview;
pub mod wire;

/// Wire protocol version. Both `Hello` handshake variants carry this value; a
/// mismatch is rejected before the rest of a message is deserialized.
pub const PROTO_VERSION: u16 = 1;
