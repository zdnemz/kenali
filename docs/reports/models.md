# Model report — Kenali

> **Status: plan + pinned manifest, not a verified deployment.** No weight was
> downloaded, hashed or executed on the Linux authoring host. Everything marked
> **VERIFY** below must be confirmed by the owner in the Windows VM. The
> authoritative machine-readable pin is `models/models.lock`.

## Pinned assets

Input/output shapes are transcribed from `docs/context_pack.md` §3.5.

| Stage | File | Licence | Input → output |
|---|---|---|---|
| Detect | `yunet.onnx` (YuNet, OpenCV Zoo) | MIT | BGR, raw 0–255, NCHW, H/W padded to multiples of 32 → per-stride (8/16/32) `cls, obj, bbox, kps`; score = √(cls·obj); box centre = (col+dx, row+dy)·stride, size = exp(dw, dh)·stride, landmark = (col+kx, row+ky)·stride |
| Align | — (similarity transform, no model) | — | 5 landmarks → 112×112 template via least-squares/Umeyama to (38.29, 51.70), (73.53, 51.50), (56.03, 71.74), (41.55, 92.37), (70.73, 92.20) |
| Embed | `sface.onnx` (SFace, OpenCV Zoo) | Apache-2.0 | 112×112 aligned, RGB, raw 0–255, NCHW → 128-d; L2-normalise; cosine. FP32 ≈ 36.9 MB; INT8 variant exists |
| Passive PAD | `minifasnet_v2.onnx` + `minifasnet_v1se.onnx` (MiniFASNet V2 + V1SE, Silent-Face-Anti-Spoofing) | Apache-2.0 | Face crops at ≈ 2.7× and ≈ 4.0× box scale, resized to 80×80 (upstream convention — **VERIFY**); fuse real-probabilities. ≈ 1.2 MB each |
| Runtime | `onnxruntime.dll` (tied to `ort =2.0.0-rc.13`, wraps ORT 1.28) | MIT | Loaded by absolute path via `ort::init_from()` with `load-dynamic`; Kenali's own pinned copy, never the system DLL |

Each row in `models/models.lock` carries `name,url,sha256,licence,upstream_commit`.
The `sha256` and `upstream_commit` cells are `VERIFY:<...>` placeholders — no
real digests/commits were verifiable on this host. Canonical source URLs are
plausible but also **VERIFY** against upstream.

## Calibration note — do not ship OpenCV's default threshold

**Do NOT ship OpenCV's reference SFace threshold (cosine 0.363 / L2 1.128).**
It corresponds to ≈ 99.6 % LFW accuracy and is *lower* on harder sets
(CALFW 0.340, CPLFW 0.275, AgeDB-30 0.277) [R23]. It optimises accuracy,
**not** a 1e-5 FAR. Kenali's threshold must come from its own calibration data
(`kenali-bench` calibration), never a tutorial/upstream default (context pack
§2 item 12, §3.5, PRD §12).

## INT8 vs FP32 — VERIFY

Choice of INT8 vs FP32 for SFace (and PAD) is **VERIFY**: measure accuracy and
latency on the eval harness before deciding. FP32 SFace is ≈ 36.9 MB; an INT8
variant exists. CPU-first; no DirectML. Indicative timings (OpenCV Zoo README,
i7-12700K class, OpenCV DNN, order-of-magnitude only): YuNet @160×120 ≈ 1.45 ms,
SFace @112×112 ≈ 8.65 ms [R24].

## Licence exclusions

InsightFace weights such as `buffalo_l` are **non-commercial** [R25] and are
excluded — never bundle by default. Passive RGB liveness is one layer, never
the whole defence [R27].

## OWNER-RUN checklist (in the Windows VM)

- [ ] Confirm each canonical source URL and the exact asset revision against
      upstream (OpenCV Zoo for YuNet/SFace; Silent-Face-Anti-Spoofing for
      MiniFASNet; Microsoft ORT release for `onnxruntime.dll`).
- [ ] Fill the real `sha256` for every row in `models/models.lock`, replacing
      the `VERIFY:<fill-from-upstream>` placeholders — compute it from the
      downloaded file, do not copy a digest from a blog.
- [ ] Fill the real `upstream_commit` for every row, replacing the
      `VERIFY:<upstream-commit>` placeholders.
- [ ] Run `cargo xtask models fetch` to populate `models/`, then
      `cargo xtask models verify` — it must pass with no VERIFY placeholders
      remaining and before any ONNX session is created.
- [ ] Cross-check SFace channel order and scaling against OpenCV's
      `FaceRecognizerSF` on a few images; confirm the 80×80 PAD crop scale.
- [ ] Run `kenali-bench` calibration to choose our own SFace threshold; verify
      it is NOT 0.363. Measure INT8 vs FP32 accuracy and latency (VERIFY).
- [ ] Confirm `onnxruntime.dll` is Kenali's own pinned copy, loaded by absolute
      path; confirm no `0xc000007b` failure and no system-DLL fallback.
- [ ] Confirm the ORT session settings (intra-op threads = min(4, physical
      cores), inter-op = 1, graph optimisation = all) against docs.rs at the
      `ort =2.0.0-rc.13` pin (VERIFY).
- [ ] Confirm no `*.onnx`, `onnxruntime.dll`, dataset, certificate or face data
      is committed (`git status` clean of binaries; `find` finds no weights).
