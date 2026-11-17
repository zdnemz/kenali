# FAR / FRR / TAR report template — Kenali (NFR-04)

> **STATUS: VERIFY / OWNER-RUN / NOT EXECUTED.** This is a **blank** report
> template. No recognition benchmark was run in the Linux authoring
> environment; all numbers below are placeholders to be filled by the owner in
> the Windows VM with the real camera and the owner's face. **No fabricated
> metrics, no face images and no embeddings are committed** — only aggregate
> counts and statistics may be recorded here. See `docs/test_plan.md` §6,
> `.crew/PRD.md` NFR-04 and §13, and `docs/context_pack.md` §3.5.

## 1. Scope and requirement

- **Requirement (NFR-04):** zero-effort **FAR ≤ 0.001 %** (1:100,000),
  **TAR ≥ 95 %** and **FRR ≤ 5 %** at the calibrated operating threshold,
  supported by **≥ 2,237 unique impostor identities** (Microsoft's rule-of-thumb
  for a 1e-5 FAR claim at ≈ 96 % confidence [R2]).
- **Environment of record:** owner's Windows 11 x64 VM / reference laptop with
  the pinned camera and face pipeline. This template is **OWNER-RUN**; nothing
  in it has been measured on the authoring host.
- **Definition of done:** every placeholder replaced with a measured value or
  an explicitly accepted written exception; no VERIFY placeholders remain.

## 2. Metric definitions and targets

| Metric | Definition | Target |
|---|---|---|
| FAR (False Accept Rate) | Zero-effort impostor attempts accepted / total impostor attempts, at the frozen threshold. One accept = an impostor identity matched to the enrolled identity. | **≤ 0.001 %** (1 in 100,000) |
| TAR (True Accept Rate) | Genuine attempts accepted / total genuine attempts, at the frozen threshold. | **≥ 95 %** |
| FRR (False Reject Rate) | Genuine attempts rejected / total genuine attempts; FRR = 1 − TAR. | **≤ 5 %** |
| Impostor identities | Distinct zero-effort individuals contributing ≥ 1 attempt each. | **≥ 2,237** unique identities |

Confidence interval requirement: report two-sided confidence intervals for
FAR, FRR and TAR (Wilson or Clopper–Pearson for the binomial rate; state the
confidence level, default 95 %). For a zero-accept FAR over N impostor
attempts, also state the rule-of-three upper bound (3/N) and the ≈ 2,237
identity power rationale. Every CI must be reported alongside its point
estimate; a point estimate alone is not an acceptance result.

## 3. Corpus composition (to fill)

| Set | Count | Composition notes |
|---|---|---|
| Enrolled user quality-gated templates | ____ (≥ 12 required) | ≥ 2 lighting conditions; FR-01 coverage |
| Genuine attempts | ____ | Poses, lighting conditions, glasses on/off per FR-01 |
| Unique zero-effort impostor identities | ____ (**must be ≥ 2,237**) | Each contributing ≥ 1 attempt |
| Impostor attempts (total) | ____ | Zero-effort: impostor simply presents their own face |
| Calibration split (threshold selection) | ____ | Held out from evaluation |
| Evaluation split (frozen threshold) | ____ | Never used to choose the threshold |

Describe cohort demographics, capture distances, indoor lighting ranges and
how identities were confirmed distinct (no duplicate subjects). Store the
source corpus outside the repository; commit aggregate metrics only.

## 4. Model hashes from `models.lock` (to fill from the pinned manifest)

`models.lock` currently carries `VERIFY:<...>` placeholders — the owner must
fill real digests before any ONNX session is created (`cargo xtask models
verify`). Record the exact SHA-256 used for this run:

| File | Role | SHA-256 recorded in `models.lock` | VERIFY status |
|---|---|---|---|
| `yunet.onnx` | Detect | `VERIFY:<fill-from-upstream>` | VERIFY |
| `sface.onnx` | Embed | `VERIFY:<fill-from-upstream>` | VERIFY |
| `minifasnet_v2.onnx` | Passive PAD | `VERIFY:<fill-from-upstream>` | VERIFY |
| `minifasnet_v1se.onnx` | Passive PAD | `VERIFY:<fill-from-upstream>` | VERIFY |
| `onnxruntime.dll` | ORT runtime (`ort =2.0.0-rc.13`) | `VERIFY:<fill-from-upstream>` | VERIFY |

Additional fields to fill: model precision (FP32 / INT8 — **VERIFY**), actual
file size, and confirmation that every loaded file passed `models verify`
before a session was opened. No weights are committed to this repository.

## 5. Operating threshold (explicitly NOT OpenCV's default)

- **Do NOT use OpenCV's SFace default cosine 0.363 / L2 1.128.** That default
  targets ≈ 99.6 % LFW accuracy and is *lower* on harder sets (CALFW 0.340,
  CPLFW 0.275, AgeDB-30 0.277) [R23]; it is not tuned for 1e-5 FAR
  (context_pack.md §3.5, models.md).
- Threshold selected from held-out calibration data: ____________________
  (cosine; state similarity metric and any preprocessing details).
- Calibration method (e.g. target-FAR operating point on calibration split):
  ______________________________________________________________________
- Confirmation the frozen threshold is different from 0.363: [ ] yes.
- Threshold frozen before evaluation split: [ ] yes. Any post-hoc retuning
  invalidates this report and requires a new held-out evaluation.

## 6. Environment (to fill)

| Field | Value |
|---|---|
| OS and build | ____ (Windows 11 x64; feature release: ____) |
| Host / VM | ____ (reference laptop or owner VM) |
| Camera make/model, VID/PID | ____ (pinned symbolic link/hardware ID) |
| CPU model, physical cores | ____ (NFR-02 reference: ≥ 4 cores) |
| ONNX Runtime version / `ort` pin | ____ (target: `ort =2.0.0-rc.13`, ORT 1.28) |
| Model precision (FP32/INT8) | ____ (VERIFY) |
| Build commit / toolchain | ____ |
| Capture distance and lighting | ____ |

## 7. Procedure (owner-run, in the Windows VM)

1. Fill real SHA-256 values in `models/models.lock` from upstream; run
   `cargo xtask models fetch` and `cargo xtask models verify` — all rows must
   verify before any ONNX session opens.
2. Enrol the owner with ≥ 12 quality-gated templates spanning ≥ 2 lighting
   conditions; record the template count and conditions in §3.
3. Run genuine attempts across poses, lighting conditions and glasses on/off;
   count accepts and rejects (TAR / FRR).
4. Run zero-effort attempts from **≥ 2,237 distinct impostor identities**, each
   contributing ≥ 1 sample; count accepts (FAR).
5. Freeze the operating threshold from the held-out calibration split
   (§5 — never OpenCV's 0.363), then re-run on the evaluation split.
6. Produce ROC (TPR vs FPR) and DET (FRR vs FAR) curves from the evaluation
   split; plot the frozen threshold's operating point on both. Save curve
   data/plots outside the repository if they contain sensitive information;
   commit only aggregate curve coordinates with no images or embeddings.
7. Compute FAR/FRR/TAR point estimates, confidence intervals and ROC/DET
   curves; assert NFR-04 or record an accepted written exception.

## 8. Results table (blank — fill from the evaluation split)

| Quantity | Value | Confidence interval (state level) | Notes |
|---|---|---|---|
| Unique impostor identities | ____ (≥ 2,237) | — | Distinct subjects confirmed |
| Total impostor attempts | ____ | — | Zero-effort |
| Impostor accepts | ____ | — | |
| FAR | ____ % | ____ | Target ≤ 0.001 % |
| Rule-of-three FAR upper bound (3/N) | ____ | — | N = impostor attempts |
| Total genuine attempts | ____ | — | |
| Genuine accepts | ____ | — | |
| TAR | ____ % | ____ | Target ≥ 95 % |
| Genuine rejects | ____ | — | |
| FRR | ____ % | ____ | Target ≤ 5 % |
| Frozen threshold (cosine) | ____ | — | NOT 0.363 |
| ROC curve | ____ (attach coords / reference) | — | TPR vs FPR |
| DET curve | ____ (attach coords / reference) | — | FRR vs FAR |

## 9. Decision

- [ ] **NFR-04 PASS** — FAR ≤ 0.001 %, TAR ≥ 95 %, FRR ≤ 5 %, ≥ 2,237 unique
      impostor identities, all CIs and curves reported.
- [ ] **NFR-04 FAIL** — one or more targets missed.
- [ ] **Accepted written exception** (attach reference / approver / date).

Decision by: ______________  Date: ______________  Threshold used: __________

## 10. Follow-ups

- [ ] If FAR target is missed: re-calibrate threshold on held-out data,
      increase impostor identity count above 2,237, or record an exception.
- [ ] If TAR/FRR target is missed: review enrolment quality gates (≥ 12
      templates, ≥ 2 lighting conditions) and capture conditions.
- [ ] Confirm ROC/DET plots contain no face images or embeddings; only
      aggregate curve coordinates are retained.
- [ ] Re-run this protocol after any hot-path, model or threshold change
      (Definition of Done) and link the new report here.
- [ ] Confirm FP32 vs INT8 choice on the eval harness (VERIFY).
