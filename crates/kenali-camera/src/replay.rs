//! Replay source for tests and VMs.
//!
//! `FileSource` plays a caller-supplied sequence of raw RGB24 frame files
//! (`frame_0000.rgb`, `frame_0001.rgb`, …) from a directory. This crate has no
//! image decoder: the caller supplies pre-decoded raw frames, so no new
//! dependency is needed and no camera is touched. The service owns camera
//! access in production; this source exists only for replay/test use.

use std::path::PathBuf;
use std::time::Instant;

use kenali_core::frame::{CameraPin, CaptureError, Frame, FrameSource};

/// A replay source backed by a directory of raw RGB24 frames.
pub struct FileSource {
    dir: PathBuf,
    width: u16,
    height: u16,
    frames: Vec<PathBuf>,
    index: usize,
    opened: bool,
}

impl FileSource {
    /// Build a replay source for `dir` with fixed frame dimensions.
    pub fn new(dir: impl Into<PathBuf>, width: u16, height: u16) -> Self {
        Self {
            dir: dir.into(),
            width,
            height,
            frames: Vec::new(),
            index: 0,
            opened: false,
        }
    }

    fn expected_len(&self) -> usize {
        usize::from(self.width)
            .saturating_mul(usize::from(self.height))
            .saturating_mul(3)
    }
}

impl FrameSource for FileSource {
    fn open(&mut self, _pin: &CameraPin, _deadline: Instant) -> Result<(), CaptureError> {
        self.close();
        let entries = std::fs::read_dir(&self.dir).map_err(|_| CaptureError::DeviceUnavailable)?;
        let mut frames: Vec<PathBuf> = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| CaptureError::Other)?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("rgb") {
                frames.push(path);
            }
        }
        frames.sort();
        if frames.is_empty() {
            return Err(CaptureError::DeviceUnavailable);
        }
        self.frames = frames;
        self.index = 0;
        self.opened = true;
        Ok(())
    }

    fn next_frame(&mut self, _deadline: Instant) -> Result<Frame, CaptureError> {
        if !self.opened {
            return Err(CaptureError::Closed);
        }
        if self.index >= self.frames.len() {
            return Err(CaptureError::Closed);
        }
        let path = &self.frames[self.index];
        self.index += 1;
        let bytes = std::fs::read(path).map_err(|_| CaptureError::Other)?;
        if bytes.len() != self.expected_len() {
            return Err(CaptureError::BadFrameLength);
        }
        Frame::new(self.width, self.height, bytes)
    }

    fn close(&mut self) {
        self.opened = false;
        self.index = 0;
        self.frames.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kenali-replay-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn replay_reads_frames_in_sorted_order() {
        let dir = temp_dir("order");
        let _ = std::fs::write(dir.join("frame_0001.rgb"), vec![2u8; 12]);
        let _ = std::fs::write(dir.join("frame_0000.rgb"), vec![1u8; 12]);
        let mut src = FileSource::new(&dir, 2, 2);
        let pin = CameraPin::new("replay", "replay");
        assert!(src.open(&pin, Instant::now()).is_ok());
        assert_eq!(src.next_frame(Instant::now()).map(|f| f.rgb[0]), Ok(1));
        assert_eq!(src.next_frame(Instant::now()).map(|f| f.rgb[0]), Ok(2));
        assert_eq!(src.next_frame(Instant::now()), Err(CaptureError::Closed));
        src.close();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn replay_rejects_bad_frame_length() {
        let dir = temp_dir("length");
        let _ = std::fs::write(dir.join("frame_0000.rgb"), vec![0u8; 5]);
        let mut src = FileSource::new(&dir, 2, 2);
        let pin = CameraPin::new("replay", "replay");
        assert!(src.open(&pin, Instant::now()).is_ok());
        assert_eq!(
            src.next_frame(Instant::now()),
            Err(CaptureError::BadFrameLength)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
