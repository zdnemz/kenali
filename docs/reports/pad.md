# PAD / APCER / BPCER report template — Kenali (NFR-05)

> **STATUS: VERIFY / OWNER-RUN / NOT EXECUTED.** This is a **blank** report
> template. No presentation-attack evaluation was run in the Linux authoring
> environment; all numbers below are placeholders to be filled by the owner in
> the Windows VM with the real camera. **No fabricated metrics, no face images
> and no embeddings are committed** — only aggregate counts and statistics may
> be recorded here. See `docs/test_plan.md` §7, `.crew/PRD.md` NFR-05 and §13,
> and `docs/context_pack.md` §3.5.

## 1. Scope and requirement

- **Requirement (NFR-05):** **APCER ≤ 1 % per attack species** with
  **≥ 300 attempts per species**, and **BPCER ≤ 5 %**. Protocol per
  `docs/test_plan.md` §7.
- **Three mandated attack species:** printed photo, phone-screen replay,
  tablet/monitor replay — each with **≥ 300 attempts**.
- **Environment of record:** owner's Windows 11 x64 VM / reference laptop with
  the pinned camera. This template is **OWNER-RUN**; nothing in it has been
  measured on the authoring host.
- **Definition of done:** every placeholder replaced with a measured value or
  an explicitly accepted written exception; no VERIFY placeholders remain.

## 2. Metric definitions and rule-of-three bounds

| Metric | Definition | Target |
|---|---|---|
| APCER (Attack Presentation Classification Error Rate) | Attacks accepted / attempts, computed **per species** and fused. | **≤ 1 % per species** |
| BPCER (Bona-fide Presentation Classification Error Rate) | Bona-fide trials rejected / bona-fide trials. | **≤ 5 %** |

**Rule-of-three bounds:** with **zero failures in 300 trials**, the one-sided
95 % upper bound on the true error rate is 3/300 = **1 %** — exactly the
APCER target. Therefore:

- 0 accepts in ≥ 300 attempts/species → APCER point estimate 0 %, upper bound
  ≈ **1 %** at 95 % confidence (rule of three).
- Any accept in 300 attempts → point estimate > 0 % and upper bound > 1 %;
  the species **fails** unless an accepted written exception is recorded.
- Report the exact attempt count N per species so the bound 3/N can be
  recomputed (3/300 ≈ 1 %; more attempts tighten the bound).
- Also report confidence intervals for APCER and BPCER (Wilson or
  Clopper–Pearson), stating the confidence level (default 95 %).

## 3. Attack species and corpus composition (to fill)

| Species | Setup | Attempts | Accepted | APCER | 95 % bound |
|---|---|---|---|---|---|
| **Printed photo** | High-quality colour print of enrolled face, normal unlock distance/angle, varied lighting | ____ (**≥ 300**) | ____ | ____ % | ____ |
| **Phone-screen replay** | Photo/video of enrolled face on phone display, replayed at unlock distance | ____ (**≥ 300**) | ____ | ____ % | ____ |
| **Tablet/monitor replay** | Larger display (tablet or monitor) replaying capture, including slight movement | ____ (**≥ 300**) | ____ | ____ % | ____ |
| Fused (all species) | Aggregate across species | ____ | ____ | ____ % | ____ |

Bona-fide trials: ____ per condition (**≥ 300** required); rejected: ____;
BPCER: ____ %; 95 % bound: ____.

Describe the attack corpus (built with the owner's consent, stored outside
the repository), camera/scene conditions, display models, print resolution
and distances. No face images or embeddings are committed.

## 4. Passive PAD model configuration (from `models.lock` — VERIFY)

Passive MiniFASNet **V2 + V1SE** fusion with attention gating (context pack
§3.5):

- Two models fed face crops at ≈ 2.7× and ≈ 4.0× the box scale, resized to
  80×80 (upstream convention — **VERIFY**); fuse real-probabilities.
- Passive RGB liveness is one layer, never the whole defence [R27]; record
  the passive + temporal + attention contributions in §6.

| File | Role | SHA-256 in `models.lock` | VERIFY status |
|---|---|---|---|
| `minifasnet_v2.onnx` | Passive PAD (V2) | `VERIFY:<fill-from-upstream>` | VERIFY |
| `minifasnet_v1se.onnx` | Passive PAD (V1SE) | `VERIFY:<fill-from-upstream>` | VERIFY |
| `yunet.onnx` | Detect (crop source) | `VERIFY:<fill-from-upstream>` | VERIFY |
| `sface.onnx` | Embed (recognition path) | `VERIFY:<fill-from-upstream>` | VERIFY |
| `onnxruntime.dll` | ORT runtime (`ort =2.0.0-rc.13`) | `VERIFY:<fill-from-upstream>` | VERIFY |

Record the fusion rule (e.g. mean of real-probabilities), the attention gating
mechanism, crop scale factors and the PAD decision threshold: ______________
__________________________________________________________________________.
`models.lock` digests are placeholders; the owner must fill real values and
run `cargo xtask models verify` before any session is created.

## 5. Environment (to fill)

| Field | Value |
|---|---|
| OS and build | ____ (Windows 11 x64; feature release: ____) |
| Host / VM | ____ (reference laptop or owner VM) |
| Camera make/model, VID/PID | ____ (pinned symbolic link/hardware ID) |
| Display device used for phone-screen and tablet/monitor replay | ____ |
| Printer / print resolution for printed photo | ____ |
| CPU model, physical cores | ____ |
| ONNX Runtime version / `ort` pin | ____ (target: `ort =2.0.0-rc.13`, ORT 1.28) |
| Model precision (FP32/INT8) | ____ (VERIFY) |
| Build commit / toolchain | ____ |
| Capture distance and lighting range | ____ |

## 6. Procedure (owner-run, in the Windows VM)

1. Fill real digests in `models/models.lock`; run `cargo xtask models fetch`
   and `cargo xtask models verify` before any ONNX session opens.
2. Build the three-species attack set with the owner's consent; store it
   outside the repository.
3. Replay attacks via the `FileSource` seam where possible, then confirm on
   the real camera.
4. Run **≥ 300 attempts for each species**: printed photo, phone-screen
   replay, tablet/monitor replay. Score each attempt as attack-presented;
   count accepts → APCER per species and fused.
5. Run **≥ 300 bona-fide trials per condition**; count rejects → BPCER.
6. Compute rule-of-three upper bounds (0 failures / 300 → ≈ 1 % at 95 %
   confidence) and confidence intervals for every rate.
7. Check **virtual-camera rejection** and **comparable-size multi-face**
   cases as separate attack cases (§7); record outcomes.
8. Record the passive MiniFASNet V2 + V1SE fusion, temporal contribution and
   attention gating behaviour; include the PAD margin distribution.
9. Assert NFR-05 or record an accepted written exception.

## 7. Separate attack cases (to fill)

| Case | Method | Attempts | Accepted / rejected as expected | Outcome |
|---|---|---|---|---|
| Virtual-camera rejection | Virtual camera presenting enrolled face | ____ | ____ | PASS / FAIL |
| Comparable-size second face | Second face at comparable size in frame | ____ | ____ (expected: reject) | PASS / FAIL |

## 8. Results table (blank — fill from the evaluation)

| Quantity | Value | Confidence interval (state level) | Notes |
|---|---|---|---|
| Printed photo attempts / accepts / APCER | ____ / ____ / ____ % | ____ | ≥ 300 attempts |
| Phone-screen replay attempts / accepts / APCER | ____ / ____ / ____ % | ____ | ≥ 300 attempts |
| Tablet/monitor replay attempts / accepts / APCER | ____ / ____ / ____ % | ____ | ≥ 300 attempts |
| Fused APCER | ____ % | ____ | Across all species |
| Rule-of-three bound (3/N) per species | ____ | — | 3/300 ≈ 1 % |
| Bona-fide trials / rejects / BPCER | ____ / ____ / ____ % | ____ | ≥ 300 per condition |
| PAD margin distribution | ____ (attach summary stats) | — | No images/embeddings |
| Passive + temporal + attention contribution | ____ | — | Describe in §4/§6 |
| Virtual-camera rejection | ____ | — | §7 |
| Comparable-size second face | ____ | — | §7 |

## 9. Decision

- [ ] **NFR-05 PASS** — APCER ≤ 1 % per species (≥ 300 attempts each),
      BPCER ≤ 5 %, rule-of-three bounds and CIs reported, virtual-camera and
      comparable-size cases handled.
- [ ] **NFR-05 FAIL** — any species > 1 % APCER or BPCER > 5 %.
- [ ] **Accepted written exception** (attach reference / approver / date).

Decision by: ______________  Date: ______________  Camera used: __________

## 10. Follow-ups

- [ ] If any species fails: review print quality / display brightness /
       crop-scale convention (VERIFY the 2.7× and 4.0× crops), increase
       attempts above 300, or record an exception.
- [ ] If BPCER fails: review bona-fide capture conditions and PAD threshold;
       re-run after any change.
- [ ] Confirm no face images or embeddings were committed; only aggregate
       metrics and rule-of-three bounds are retained.
- [ ] Re-run this protocol after any model, fusion or threshold change and
       link the new report here.
