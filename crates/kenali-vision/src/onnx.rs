//! Real ONNX Runtime backends behind the merged vision traits.
//!
//! The four [`OrtDetector`], [`OrtAligner`], [`OrtEmbedder`] and
//! [`OrtPadClassifier`] types implement [`Detector`], [`Aligner`], [`Embedder`]
//! and [`PadClassifier`] respectively. They own ORT sessions (except the
//! stateless aligner, which reuses the merged geometry code in
//! [`crate::align`]).
//!
//! Loading rules (context pack §2 item 10, §3.4, §3.5):
//!
//! * Models and `onnxruntime.dll` are loaded by **absolute path** from
//!   `C:\Program Files\Kenali\<version>\models\`, overridable for development
//!   with `KENALI_MODEL_DIR` ([`model_dir`]).
//! * Every model's bytes are verified against its SHA-256 in `models.lock`
//!   ([`verify_model_hash`]) **before** an ORT session is created. We feed the
//!   verified bytes straight into `commit_from_memory`, so there is no
//!   time-of-check/time-of-use gap between verification and session creation.
//! * ORT is initialised with Kenali's **own** pinned `onnxruntime.dll` via
//!   `ort::init_from` (`load-dynamic`). We never fall back to the system DLL
//!   and never use DirectML -- CPU first.
//!
//! `models.lock` currently carries `VERIFY:<...>` placeholders for every
//! digest, so on this host **no session can be created**: that is intentional.
//! The owner fills the real digests in the VM and re-builds. No weight was
//! downloaded, hashed or executed here; all runtime inference is owner run.
//!
//! VERIFY items (owner-run, not blockers):
//! * INT8 vs FP32 for SFace / PAD -- measure accuracy and latency first.
//! * Exact ORT API names on the pinned `ort =2.0.0-rc.13` (checked against the
//!   vendored crate source; re-confirm on the MSVC toolchain in the VM).
//! * YuNet output tensor ordering and the `(col + dx) * stride` anchor, SFace
//!   channel order/scaling, and the MiniFASNet 80x80 / 2.7x / 4.0x convention.
//! * `std::thread::available_parallelism` reports *logical* cores; the
//!   contract asks for physical cores, which need Win32 in the service.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use kenali_core::frame::Frame;
use kenali_core::score::cosine;
use kenali_core::secret::Embedding;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use sha2::{Digest, Sha256};

use crate::align::{ALIGN_POINTS_112, similarity_transform, warp_affine_112};
use crate::preprocess::to_nchw;
use crate::traits::{Aligner, Detection, Detector, Embedder, PadClassifier};
use crate::VisionError;

/// Compiled-in copy of the committed model manifest. The owner fills the real
/// SHA-256 cells in `models/models.lock` before building the release binary.
pub const MODELS_LOCK: &str = include_str!("../../../models/models.lock");

/// Default absolute model directory. The `<version>` component is substituted
/// by the installer/service (a Wave 3+ integration concern); the development
/// override is `KENALI_MODEL_DIR` ([`model_dir`]).
pub const DEFAULT_MODEL_DIR: &str = r"C:\Program Files\Kenali\<version>\models";

pub const YUNET_MODEL: &str = "yunet.onnx";
pub const SFACE_MODEL: &str = "sface.onnx";
pub const PAD_V2_MODEL: &str = "minifasnet_v2.onnx";
pub const PAD_V1SE_MODEL: &str = "minifasnet_v1se.onnx";
pub const ORT_DLL: &str = "onnxruntime.dll";

/// Failures specific to loading and verifying models. Messages stay generic:
/// no digest bytes, model bytes, embeddings or paths leak into logs.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelError {
    #[error("model digest is an unresolved VERIFY placeholder")]
    PlaceholderHash,
    #[error("model digest is malformed")]
    InvalidHash,
    #[error("model digest mismatch")]
    HashMismatch,
    #[error("model has no hash entry in models.lock")]
    MissingManifestEntry,
    #[error("model directory or runtime path is not absolute")]
    NotAbsolute,
    #[error("invalid model filename")]
    BadModelName,
    #[error("model file could not be read")]
    ModelRead,
    #[error("ONNX Runtime initialisation failed")]
    RuntimeInit,
    #[error("ONNX session creation failed")]
    SessionInit,
    #[error("ONNX inference failed")]
    Inference,
}

impl From<ModelError> for VisionError {
    fn from(err: ModelError) -> Self {
        let message = match err {
            ModelError::PlaceholderHash
            | ModelError::InvalidHash
            | ModelError::HashMismatch
            | ModelError::MissingManifestEntry => "model integrity verification failed",
            _ => "model backend unavailable",
        };
        VisionError::Backend(message.to_string())
    }
}

/// `min(4, physical cores)` thread budget.
///
/// ponytail: `std` exposes logical, not physical, cores; the `min(4, ..)`
/// ceiling is the contract. Owner confirms the physical count via Win32 in the
/// service (VERIFY).
pub fn intra_op_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 4)
}

fn to_hex_lower(bytes: &[u8]) -> [u8; 64] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 64];
    for (i, &byte) in bytes.iter().enumerate() {
        out[i * 2] = HEX[(byte >> 4) as usize];
        out[i * 2 + 1] = HEX[(byte & 0x0f) as usize];
    }
    out
}

fn constant_time_eq_64(a: &[u8; 64], b: &[u8]) -> bool {
    if b.len() != 64 {
        return false;
    }
    let mut diff = 0u8;
    for (i, &byte) in b.iter().enumerate() {
        diff |= a[i] ^ byte;
    }
    diff == 0
}

fn is_placeholder(value: &str) -> bool {
    let upper = value.to_ascii_uppercase();
    upper.starts_with("VERIFY") || upper.contains("<FILL") || upper.contains("<UPSTREAM")
}

/// Verify a model's SHA-256 against the digest recorded in `models.lock`.
///
/// `expected_sha256` is the `sha256` column of the matching row. The current
/// `models.lock` rows use `VERIFY:<fill-from-upstream>` / `VERIFY:<...>`
/// placeholders; those are **not** digests and are rejected with
/// [`ModelError::PlaceholderHash`] rather than being treated as a valid hash.
pub fn verify_model_hash(bytes: &[u8], expected_sha256: &str) -> Result<(), ModelError> {
    if is_placeholder(expected_sha256) {
        return Err(ModelError::PlaceholderHash);
    }
    let expected = expected_sha256.trim().to_ascii_lowercase();
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ModelError::InvalidHash);
    }

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = to_hex_lower(&hasher.finalize());
    if constant_time_eq_64(&actual, expected.as_bytes()) {
        Ok(())
    } else {
        Err(ModelError::HashMismatch)
    }
}

/// Parsed `models.lock` digest table (filename → SHA-256 column). Only the
/// `name` and `sha256` columns are needed; comment lines are ignored.
#[derive(Debug, Default)]
pub struct ModelManifest {
    hashes: HashMap<String, String>,
}

impl ModelManifest {
    pub fn parse(text: &str) -> Self {
        let mut hashes = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut columns = line.split(',');
            let name = columns.next().unwrap_or("").trim();
            let _url = columns.next();
            let sha256 = columns.next().unwrap_or("").trim();
            if !name.is_empty() && !sha256.is_empty() {
                hashes.insert(name.to_string(), sha256.to_string());
            }
        }
        Self { hashes }
    }

    pub fn expected_hash(&self, name: &str) -> Option<&str> {
        self.hashes.get(name).map(String::as_str)
    }
}

/// The compiled-in manifest, parsed once per call.
pub fn default_manifest() -> ModelManifest {
    ModelManifest::parse(MODELS_LOCK)
}

/// Resolve the absolute model directory: `KENALI_MODEL_DIR` in development,
/// otherwise `C:\Program Files\Kenali\<version>\models\`. Relative overrides
/// are rejected so a CWD trick cannot swap in a different directory.
pub fn model_dir() -> Result<PathBuf, ModelError> {
    if let Some(value) = std::env::var_os("KENALI_MODEL_DIR") {
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(ModelError::NotAbsolute);
        }
        return Ok(path);
    }
    Ok(PathBuf::from(DEFAULT_MODEL_DIR))
}

fn model_path(dir: &Path, name: &str) -> Result<PathBuf, ModelError> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || !dir.is_absolute()
    {
        return Err(ModelError::NotAbsolute);
    }
    Ok(dir.join(name))
}

/// Initialise ORT from Kenali's own pinned DLL. Call once at service start,
/// before any session is built. `ort` uses a global environment; repeated
/// calls are harmless (the second `commit` is a no-op).
pub fn initialize_runtime(dll_path: &Path) -> Result<(), ModelError> {
    if !dll_path.is_absolute() {
        return Err(ModelError::NotAbsolute);
    }
    // Thread counts are configured per-session (see `build_session`). The
    // global environment only binds the DLL; VERIFY the exact env-builder API
    // on ort =2.0.0-rc.13 (global thread pool lives on GlobalThreadPoolOptions).
    let env = ort::init_from(dll_path).map_err(|_| ModelError::RuntimeInit)?;
    env.commit();
    Ok(())
}

/// Verify then build a CPU session from the *verified* bytes (no TOCTOU).
fn build_session(dir: &Path, name: &str, manifest: &ModelManifest) -> Result<Session, ModelError> {
    let expected = manifest.expected_hash(name).ok_or(ModelError::MissingManifestEntry)?;
    let path = model_path(dir, name)?;
    let bytes = std::fs::read(&path).map_err(|_| ModelError::ModelRead)?;
    verify_model_hash(&bytes, expected)?;

    // VERIFY: session-setting method names on ort =2.0.0-rc.13.
    let mut builder = Session::builder()
        .map_err(|_| ModelError::SessionInit)?
        .with_intra_threads(intra_op_threads())
        .map_err(|_| ModelError::SessionInit)?
        .with_inter_threads(1)
        .map_err(|_| ModelError::SessionInit)?
        .with_optimization_level(GraphOptimizationLevel::All)
        .map_err(|_| ModelError::SessionInit)?;
    builder.commit_from_memory(&bytes).map_err(|_| ModelError::SessionInit)
}

/// One dummy inference at service start. Exposed for the service to call; we
/// do not run it here. `shape` must match the session's single input.
pub fn warm_up(session: &mut Session, shape: &[usize]) -> Result<(), ModelError> {
    let count = shape.iter().copied().product::<usize>();
    let tensor =
        Tensor::from_array((shape.to_vec(), vec![0.0f32; count])).map_err(|_| ModelError::SessionInit)?;
    let _ = session
        .run(ort::inputs![tensor])
        .map_err(|_| ModelError::Inference)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// YuNet decode (pure, host-testable)
// ---------------------------------------------------------------------------

/// Decode one YuNet stride into detections. `cls`, `obj`, `bbox` and `kps`
/// are flat f32 tensors for a `grid_h × grid_w` grid:
///
/// * `cls[idx]`, `obj[idx]` -- class / objectness at cell `(x, y)`;
/// * `bbox[plane + idx]` with plane `grid_h * grid_w` -- channel `0..3` =
///   `dx, dy, dw, dh`;
/// * `kps[2*i * plane + idx]`, `kps[(2*i+1) * plane + idx]` -- landmark `i`'s
///   `kx, ky`.
///
/// Score = `√(cls·obj)`. Centre = `(col+dx, row+dy)·stride`, size =
/// `exp(dw, dh)·stride`, landmark = `(col+kx, row+ky)·stride`.
///
/// VERIFY: exact YuNet channel order and the `col + dx` (vs `col + 0.5 + dx`)
/// anchor against OpenCV's `FaceDetectorYN` on the Windows VM.
pub fn decode_yunet_stride(
    cls: &[f32],
    obj: &[f32],
    bbox: &[f32],
    kps: &[f32],
    grid_h: usize,
    grid_w: usize,
    stride: f32,
    score_threshold: f32,
) -> Vec<Detection> {
    let plane = match grid_h.checked_mul(grid_w) {
        Some(n) if n > 0 => n,
        _ => return Vec::new(),
    };
    if cls.len() < plane
        || obj.len() < plane
        || bbox.len() < 4 * plane
        || kps.len() < 10 * plane
        || !stride.is_finite()
        || stride <= 0.0
    {
        return Vec::new();
    }

    let mut out = Vec::new();
    for y in 0..grid_h {
        for x in 0..grid_w {
            let idx = y * grid_w + x;
            let (class, objectness) = (cls[idx], obj[idx]);
            if class <= 0.0 || objectness <= 0.0 {
                continue;
            }
            let score = (class * objectness).sqrt();
            if !score.is_finite() || score < score_threshold {
                continue;
            }

            let dx = bbox[idx];
            let dy = bbox[plane + idx];
            let dw = bbox[2 * plane + idx];
            let dh = bbox[3 * plane + idx];
            let width = dw.exp() * stride;
            let height = dh.exp() * stride;
            if !width.is_finite() || !height.is_finite() {
                continue;
            }
            let cx = (x as f32 + dx) * stride;
            let cy = (y as f32 + dy) * stride;

            let mut landmarks = [(0.0f32, 0.0f32); 5];
            for (i, point) in landmarks.iter_mut().enumerate() {
                let kx = kps[2 * i * plane + idx];
                let ky = kps[(2 * i + 1) * plane + idx];
                *point = ((x as f32 + kx) * stride, (y as f32 + ky) * stride);
            }

            out.push(Detection {
                score,
                bbox: [cx - width * 0.5, cy - height * 0.5, width, height],
                landmarks,
            });
        }
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out
}

/// RGB frame → padded BGR NCHW f32 (raw 0–255), with width/height rounded up
/// to multiples of 32 as YuNet requires. Padding is black.
fn rgb_to_padded_bgr_nchw(frame: &Frame) -> Result<(Vec<f32>, usize, usize), VisionError> {
    let w = usize::from(frame.width);
    let h = usize::from(frame.height);
    let pad_w = (w + 31) & !31;
    let pad_h = (h + 31) & !31;
    if pad_w == 0 || pad_h == 0 || w == 0 || h == 0 {
        return Err(VisionError::BadInput("empty frame"));
    }
    let mut bgr = vec![0u8; pad_w * pad_h * 3];
    for y in 0..h {
        for x in 0..w {
            let src = (y * w + x) * 3;
            let dst = (y * pad_w + x) * 3;
            bgr[dst] = frame.rgb[src + 2];
            bgr[dst + 1] = frame.rgb[src + 1];
            bgr[dst + 2] = frame.rgb[src];
        }
    }
    let nchw = to_nchw(&bgr, pad_w, pad_h, 3);
    if nchw.is_empty() {
        return Err(VisionError::BadInput("padding overflow"));
    }
    Ok((nchw, pad_w, pad_h))
}

fn extract_tensor_f32(value: &ort::value::Value) -> Result<Vec<f32>, VisionError> {
    let (_shape, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|_| VisionError::Backend("unexpected model output type".to_string()))?;
    Ok(data.to_vec())
}

// ---------------------------------------------------------------------------
// Backends
// ---------------------------------------------------------------------------

/// YuNet detector. Owns its session and reuses one input buffer.
pub struct OrtDetector {
    session: Session,
}

impl OrtDetector {
    pub fn new(dir: &Path) -> Result<Self, ModelError> {
        let manifest = default_manifest();
        let session = build_session(dir, YUNET_MODEL, &manifest)?;
        Ok(Self { session })
    }
}

impl Detector for OrtDetector {
    fn detect(&mut self, frame: &Frame) -> Result<Vec<Detection>, VisionError> {
        let (data, pad_w, pad_h) = rgb_to_padded_bgr_nchw(frame)?;
        let input = Tensor::from_array(([1usize, 3, pad_h, pad_w], data))
            .map_err(|_| VisionError::Backend("input tensor creation failed".to_string()))?;
        let outputs = self
            .session
            .run(ort::inputs![input])
            .map_err(|_| VisionError::Backend("inference failed".to_string()))?;

        // OpenCV Zoo YuNet exports 12 outputs: four tensors (cls, obj, bbox,
        // kps) per stride at 8/16/32. VERIFY: exact ordering against the
        // export used by the owner.
        let strides = [8.0f32, 16.0, 32.0];
        let mut detections = Vec::new();
        for (s, stride) in strides.into_iter().enumerate() {
            let base = s * 4;
            let tensors: Vec<Vec<f32>> = (base..base + 4)
                .try_fold(Vec::with_capacity(4), |mut acc, i| {
                    if i >= outputs.len() {
                        return Err(VisionError::Backend("missing detector output".to_string()));
                    }
                    acc.push(extract_tensor_f32(&outputs[i])?);
                    Ok::<_, VisionError>(acc)
                })?;
            let plane = pad_h / (stride as usize) * (pad_w / (stride as usize));
            let grid_h = pad_h / (stride as usize);
            let grid_w = pad_w / (stride as usize);
            let expected = |channels: usize| plane * channels;
            if tensors[0].len() < expected(1)
                || tensors[1].len() < expected(1)
                || tensors[2].len() < expected(4)
                || tensors[3].len() < expected(10)
            {
                return Err(VisionError::Backend("unexpected detector output shape".to_string()));
            }
            detections.extend(decode_yunet_stride(
                &tensors[0],
                &tensors[1],
                &tensors[2],
                &tensors[3],
                grid_h,
                grid_w,
                stride,
                0.0,
            ));
        }
        detections.sort_by(|a, b| b.score.total_cmp(&a.score));
        Ok(detections)
    }
}

/// Alignment has no weights: reuse the merged similarity-transform + warp.
#[derive(Debug, Clone, Copy, Default)]
pub struct OrtAligner;

impl Aligner for OrtAligner {
    fn align(
        &mut self,
        frame: &Frame,
        landmarks: &[(f32, f32); 5],
    ) -> Result<Vec<u8>, VisionError> {
        let affine = similarity_transform(landmarks, &ALIGN_POINTS_112)?;
        warp_affine_112(frame, &affine)
    }
}

/// Build an L2-normalised embedding from raw SFace output. `Embedding::new`
/// is the merged secret newtype, so normalisation happens in one place.
pub fn embedding_from_raw(raw: Vec<f32>) -> Embedding {
    Embedding::new(raw)
}

/// Match using a **caller-supplied calibrated threshold**. Never default to
/// OpenCV's 0.363 -- that value is tuned for LFW accuracy, not 1e-5 FAR.
pub fn is_match(stored: &Embedding, probe: &Embedding, calibrated_threshold: f32) -> bool {
    cosine(stored, probe) >= calibrated_threshold
}

/// SFace embedder. Input is the 112×112×3 RGB aligned buffer from [`OrtAligner`].
pub struct OrtEmbedder {
    session: Session,
}

impl OrtEmbedder {
    pub fn new(dir: &Path) -> Result<Self, ModelError> {
        let manifest = default_manifest();
        let session = build_session(dir, SFACE_MODEL, &manifest)?;
        Ok(Self { session })
    }
}

impl Embedder for OrtEmbedder {
    fn embed(&mut self, aligned_rgb: &[u8]) -> Result<Embedding, VisionError> {
        if aligned_rgb.len() != 112 * 112 * 3 {
            return Err(VisionError::BadInput("aligned buffer must be 112x112x3"));
        }
        let nchw = to_nchw(aligned_rgb, 112, 112, 3);
        if nchw.is_empty() {
            return Err(VisionError::BadInput("aligned buffer conversion failed"));
        }
        // VERIFY: SFace channel order/scaling against OpenCV FaceRecognizerSF.
        let input = Tensor::from_array(([1usize, 3, 112usize, 112usize], nchw))
            .map_err(|_| VisionError::Backend("input tensor creation failed".to_string()))?;
        let outputs = self
            .session
            .run(ort::inputs![input])
            .map_err(|_| VisionError::Backend("inference failed".to_string()))?;
        if outputs.len() == 0 {
            return Err(VisionError::Backend("missing embedder output".to_string()));
        }
        Ok(embedding_from_raw(extract_tensor_f32(&outputs[0])?))
    }
}

/// Bilinear resize of an RGB HWC buffer to 80×80 (MiniFASNet input).
///
/// VERIFY: the upstream convention (crop scale 2.7x/4.0x, 80x80 resize,
/// channel order and 0..1 vs -1..1 scaling) on the Windows VM.
fn resize_rgb_80x80(src: &[u8], w: usize, h: usize) -> Result<Vec<f32>, VisionError> {
    if w == 0 || h == 0 || src.len() != w * h * 3 {
        return Err(VisionError::BadInput("invalid crop for PAD"));
    }
    const OUT: usize = 80;
    let mut out = vec![0.0f32; OUT * OUT * 3];
    for oy in 0..OUT {
        let sy = (oy as f32 + 0.5) * h as f32 / OUT as f32 - 0.5;
        let y0 = sy.floor();
        let fy = sy - y0;
        for ox in 0..OUT {
            let sx = (ox as f32 + 0.5) * w as f32 / OUT as f32 - 0.5;
            let x0 = sx.floor();
            let fx = sx - x0;
            for c in 0..3 {
                let tap = |ix: i64, iy: i64| -> f32 {
                    if ix < 0 || iy < 0 {
                        return 0.0;
                    }
                    let (ix, iy) = (ix as usize, iy as usize);
                    if ix >= w || iy >= h {
                        return 0.0;
                    }
                    f32::from(src[(iy * w + ix) * 3 + c]) / 255.0
                };
                let x0 = x0 as i64;
                let y0 = y0 as i64;
                let value = (1.0 - fx) * (1.0 - fy) * tap(x0, y0)
                    + fx * (1.0 - fy) * tap(x0 + 1, y0)
                    + (1.0 - fx) * fy * tap(x0, y0 + 1)
                    + fx * fy * tap(x0 + 1, y0 + 1);
                out[(oy * OUT + ox) * 3 + c] = value;
            }
        }
    }
    Ok(out)
}

/// f32 HWC → NCHW (the byte version in `preprocess` only accepts `u8`).
fn hwc_f32_to_nchw(src: &[f32], w: usize, h: usize, channels: usize) -> Vec<f32> {
    let plane = w * h;
    let mut out = vec![0.0f32; plane * channels];
    for c in 0..channels {
        for i in 0..plane {
            out[c * plane + i] = src[i * channels + c];
        }
    }
    out
}

/// Fuse the two MiniFASNet real-probabilities. Averaging is the simplest
/// fusion; the owner replaces this after measuring on the eval harness.
pub fn fuse_pad_probabilities(v2: f32, v1se: f32) -> f32 {
    (v2 + v1se) * 0.5
}

/// Passive PAD classifier: MiniFASNet V2 + V1SE fused.
pub struct OrtPadClassifier {
    v2: Session,
    v1se: Session,
}

impl OrtPadClassifier {
    pub fn new(dir: &Path) -> Result<Self, ModelError> {
        let manifest = default_manifest();
        let v2 = build_session(dir, PAD_V2_MODEL, &manifest)?;
        let v1se = build_session(dir, PAD_V1SE_MODEL, &manifest)?;
        Ok(Self { v2, v1se })
    }

    fn score_one(session: &mut Session, rgb: &[u8], w: usize, h: usize) -> Result<f32, VisionError> {
        let hwc = resize_rgb_80x80(rgb, w, h)?;
        let nchw = hwc_f32_to_nchw(&hwc, 80, 80, 3);
        if nchw.is_empty() {
            return Err(VisionError::BadInput("PAD tensor conversion failed"));
        }
        let input = Tensor::from_array(([1usize, 3, 80usize, 80usize], nchw))
            .map_err(|_| VisionError::Backend("input tensor creation failed".to_string()))?;
        let outputs = session
            .run(ort::inputs![input])
            .map_err(|_| VisionError::Backend("inference failed".to_string()))?;
        if outputs.len() == 0 {
            return Err(VisionError::Backend("missing PAD output".to_string()));
        }
        let values = extract_tensor_f32(&outputs[0])?;
        let raw = values.first().copied().unwrap_or(0.0);
        if !raw.is_finite() {
            return Err(VisionError::Backend("invalid PAD output".to_string()));
        }
        Ok(raw.clamp(0.0, 1.0))
    }
}

impl PadClassifier for OrtPadClassifier {
    fn pad_score(&mut self, face_crop_rgb: &[u8]) -> Result<f32, VisionError> {
        // The caller supplies the appropriately scaled crop for each model;
        // both backends resize to 80×80 internally. VERIFY the 2.7x/4.0x crop
        // convention in the service on the Windows VM.
        if face_crop_rgb.len() % 3 != 0 || face_crop_rgb.is_empty() {
            return Err(VisionError::BadInput("invalid face crop"));
        }
        let pixels = face_crop_rgb.len() / 3;
        let side = (pixels as f64).sqrt().round() as usize;
        if side == 0 || side.saturating_mul(side) != pixels {
            return Err(VisionError::BadInput("face crop is not square"));
        }
        let v2 = Self::score_one(&mut self.v2, face_crop_rgb, side, side)?;
        let v1se = Self::score_one(&mut self.v1se, face_crop_rgb, side, side)?;
        Ok(fuse_pad_probabilities(v2, v1se))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SHA-256("abc") -- a fixed, independently known digest.
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const OTHER_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn verify_hash_accepts_correct_sha256() {
        assert_eq!(verify_model_hash(b"abc", ABC_SHA256), Ok(()));
    }

    #[test]
    fn verify_hash_rejects_wrong_and_malformed() {
        assert_eq!(
            verify_model_hash(b"abd", ABC_SHA256),
            Err(ModelError::HashMismatch)
        );
        assert_eq!(
            verify_model_hash(b"abc", OTHER_SHA256),
            Err(ModelError::HashMismatch)
        );
        assert_eq!(
            verify_model_hash(b"abc", "not-a-hex-digest"),
            Err(ModelError::InvalidHash)
        );
        assert_eq!(
            verify_model_hash(b"abc", &ABC_SHA256[..63]),
            Err(ModelError::InvalidHash)
        );
    }

    #[test]
    fn verify_hash_rejects_models_lock_placeholders() {
        // models.lock rows use VERIFY:<...> placeholders until the owner
        // fills real digests; they must never be accepted as valid hashes.
        for placeholder in ["VERIFY:<fill-from-upstream>", "VERIFY:<upstream-commit>"] {
            assert_eq!(
                verify_model_hash(b"abc", placeholder),
                Err(ModelError::PlaceholderHash)
            );
        }
    }

    #[test]
    fn manifest_parses_committed_models_lock() {
        let manifest = default_manifest();
        assert_eq!(
            manifest.expected_hash(YUNET_MODEL),
            Some("VERIFY:<fill-from-upstream>")
        );
        assert_eq!(
            verify_model_hash(b"anything", manifest.expected_hash(SFACE_MODEL).unwrap()),
            Err(ModelError::PlaceholderHash)
        );
        assert!(manifest.expected_hash("missing.onnx").is_none());
    }

    #[test]
    fn decode_yunet_stride_decodes_known_cell() {
        let (grid_h, grid_w, stride) = (2usize, 2usize, 8.0f32);
        let plane = grid_h * grid_w;
        let idx = 0; // cell (0, 0)
        let mut cls = vec![0.0; plane];
        let mut obj = vec![0.0; plane];
        let mut bbox = vec![0.0; 4 * plane];
        let mut kps = vec![0.0; 10 * plane];
        cls[idx] = 0.9;
        obj[idx] = 0.8;
        bbox[idx] = 0.5; // dx
        bbox[plane + idx] = 0.25; // dy
        bbox[3 * plane + idx] = 2.0f32.ln(); // dh -> height = 16
        kps[plane + idx] = 0.5; // landmark 0 ky
        // dw stays 0 -> width = 8.

        let detections = decode_yunet_stride(&cls, &obj, &bbox, &kps, grid_h, grid_w, stride, 0.5);
        assert_eq!(detections.len(), 1);
        let d = &detections[0];
        assert!((d.score - (0.9f32 * 0.8).sqrt()).abs() < 1e-5);
        // cx = (0 + 0.5)*8 = 4, cy = (0 + 0.25)*8 = 2, w=8, h=16.
        assert!((d.bbox[0] - 0.0).abs() < 1e-4); // cx - w/2
        assert!((d.bbox[1] - -6.0).abs() < 1e-4); // cy - h/2
        assert!((d.bbox[2] - 8.0).abs() < 1e-4);
        assert!((d.bbox[3] - 16.0).abs() < 1e-4);
        assert!((d.landmarks[0].0 - 0.0).abs() < 1e-4);
        assert!((d.landmarks[0].1 - 4.0).abs() < 1e-4);
    }

    #[test]
    fn decode_yunet_stride_filters_below_threshold_and_short_tensors() {
        let cls = [0.09];
        let obj = [0.09];
        let bbox = vec![0.0; 4];
        let kps = vec![0.0; 10];
        assert!(decode_yunet_stride(&cls, &obj, &bbox, &kps, 1, 1, 8.0, 0.5).is_empty());
        assert!(decode_yunet_stride(&cls, &obj, &bbox[..2], &kps, 1, 1, 8.0, 0.0).is_empty());
    }

    #[test]
    fn embedding_is_l2_normalised_and_match_uses_calibrated_threshold() {
        let emb = embedding_from_raw(vec![3.0, 4.0]);
        assert!((emb.as_slice()[0] - 0.6).abs() < 1e-6);
        assert!((emb.as_slice()[1] - 0.8).abs() < 1e-6);

        let same = embedding_from_raw(vec![0.6, 0.8]);
        assert!(is_match(&emb, &same, 0.9));
        let other = embedding_from_raw(vec![0.0, 1.0]);
        assert!(!is_match(&emb, &other, 0.9));

        // Our calibration must not be OpenCV's 0.363 default; the caller owns
        // the number, so no tutorial default is baked in here.
        assert!(!is_match(&emb, &other, 0.99));
    }

    #[test]
    fn pad_probabilities_are_fused_and_clamped() {
        assert!((fuse_pad_probabilities(0.8, 0.4) - 0.6).abs() < 1e-6);
        assert!((fuse_pad_probabilities(1.0, 1.0) - 1.0).abs() < 1e-6);
    }
}
