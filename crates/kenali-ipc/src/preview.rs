//! Preview-pipe frames. Path: `\\.\pipe\kenali.preview`. The pipe exists only
//! while `BeginPreview`/`BeginCalibration` is active. 320×240 RGB (230,400 B)
//! fits under `MAX_PREVIEW_BYTES`.

use serde::{Deserialize, Serialize};

use crate::wire::Bytes;

/// One RGB video frame sent to the management UI. Never persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewFrame {
    pub w: u16,
    pub h: u16,
    pub rgb: Bytes,
}
