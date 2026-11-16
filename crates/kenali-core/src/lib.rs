#![forbid(unsafe_code)]

//! `kenali-core` — platform-agnostic domain, policy and secret types.
//!
//! No OS bindings, no I/O, no networking. Everything here is `Send` where it
//! needs to cross the auth/admin pipe boundary, and every secret is wrapped in
//! a redacting, zeroizing newtype.

pub mod config;
pub mod frame;
pub mod policy;
pub mod score;
pub mod secret;
pub mod state;
pub mod strings;
pub mod types;
