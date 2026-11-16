# Test Plan — Kenali

> Companion to `prd.md` (FR/NFR/SEC), `context_pack.md` §7 and §11, and `dev_safety_runbook.md`.
> Priorities under test: **security → never lock the owner out → speed → footprint.**
> This is both the test strategy and the acceptance record. Windows/hardware items are executable only by the owner inside the VM; see the final section "Requires the owner's Windows VM".

---

## 1. Strategy and host split

| Layer | Tooling | Where it runs | Gate |
|---|---|---|---|
| Unit + property | `cargo nextest`, `proptest` | Linux host (platform-agnostic crates) | All green on `main` |
| Integration (cross-crate) | `cargo nextest`, in-memory fakes for pipes/camera/TPM | Linux host | All green; pipe contracts versioned |
| Compile gate for Windows crates | `cargo check --target x86_64-pc-windows-msvc --locked` | Linux host (cross target) | Type-checks; does not link/run |
| Fuzz | `cargo-fuzz` (nightly) + `cargo xtask fuzz <target> <secs>` | Linux host for decoders/parsers | ≥ 1 h per target, 0 crashes |
| Benchmarks | `criterion` + `kenali-bench` traces | Linux host (models) + reference laptop (end-to-end) | NFR-01/02 met or exception written |
| Recognition / PAD / attacks | `kenali-bench` eval harness, labelled corpora | Owner's Windows laptop (or VM with replay source) | FAR/FRR + APCER/BPCER reports committed |
| Soak, hardware acceptance, recovery, UI walkthrough | Manual scripts + runbook | Owner's Windows VM → secondary account → main account | Passed before daily driver |

**Non-goals of the test suite:** it never registers a CP, installs the service, or writes Windows registry state on the development host. It never commits face data, model weights, certificates or secrets.

---

## 2. Unit and property tests (host-side)

Every rule/transform gets a unit test; every invariant gets a property test. Tests live next to code. No `unwrap()` outside tests and `main` setup.

| Area | Crate | Tests | Invariants / properties |
|---|---|---|---|
| **Policy engine** | `kenali-core` | One unit test per rule in PRD §10 (boot/first sign-in password-only, ≥ 24 h knowledge-factor, 3-fail lock fallback, 5-fail/10 min, 3-PAD/10 min, wrong-password disable, camera-pin mismatch, multi-face, RDP, attention, risk→challenge). | Property: policy is **monotonic** — no configuration change can loosen a default beyond the schema's allowed tightenings; a disable switch can only tighten. Property: deterministic given (state, event, config). Property: after any `STATUS_WRONG_PASSWORD`/locked-out/expired/must-change result, no success transition exists until refresh-credential. |
| **IPC codec** | `kenali-ipc` | Round-trip for every auth/admin/preview message variant; `PROTO_VERSION` handshake accept/reject; size caps. | Property: `decode(encode(x)) == x` for all legal values; truncated/oversized/random bytes are **rejected**, never allocate from untrusted lengths; strings ≤ 256 UTF-16 units, control ≤ 4096 B, preview ≤ 512 KB, diagnostics chunks ≤ 64 KB / ZIP ≤ 10 MB. |
| **Alignment math** | `kenali-vision` | Similarity transform against the canonical 5-point template (38.29, 51.70) … (70.73, 92.20); known rotation/scale/shift fixtures. | Property: transform is invariant under small rigid perturbations of the input landmarks; output is always 112×112 finite NCHW; no NaN/Inf for valid input; determinant > 0 (no reflection). |
| **AEAD / store format** | `kenali-store` | XChaCha20-Poly1305 round-trip, AAD = magic ‖ version ‖ file type ‖ user SID; tamper each AAD field and the ciphertext. | Property: any bit flip in ciphertext or AAD fails authentication; random 24-byte nonces never repeat in a large sample; password/template blobs decrypt only under the correct SID; malformed headers rejected without panic. |
| **Scoring / decision** | `kenali-core`, `kenali-vision` | Cosine match vs multiple templates; temporal window; threshold boundary cases. | Property: score monotonic in embedding similarity; a face-only unlock never mutates the template set (SEC-15). |

---

## 3. Integration tests crossing crate boundaries

These prove the contracts, not just the units.

| Test | Crates | Fake / seam | Assertion |
|---|---|---|---|
| Auth attempt happy path | `kenali-cp` ⇄ `kenali-ipc` ⇄ `kenali-service` | In-memory pipe pair + `SyntheticSource` frames | Verified response carries the echo nonce, secret released once, within ≤ 3 s of decision, only to the originating instance. |
| One-shot secret & no-retry | `kenali-service` ⇄ `kenali-cp` | Fake Windows `ReportResult` | Wrong-password / locked-out / expired status disables face unlock; a second submit never occurs. |
| Admin typed commands | `kenali-ui` ⇄ `kenali-ipc` ⇄ `kenali-service` | In-memory admin pipe | Every FR-09 action is a typed command; service rejects file paths, unknown keys, unmapped scenario bits; config changes are schema-validated and tighten-only. |
| Preview window lifecycle | `kenali-vision` → `kenali-ipc` → `kenali-ui` | `SyntheticSource` + RAM slot | Preview frames flow only between `BeginPreview` and `EndPreview`; buffers zeroized on close/error; nothing persisted or logged. Without the preview pipe, enrolment still completes (text guidance). |
| Store ⇄ TPM key-hierarchy trait | `kenali-store` + fake key provider | Software stub behind the trait | Hybrid encryption: DEK wrapped, templates/password sealed, correct SID required, zeroize on every error path. |
| Camera pin guard | `kenali-camera` ⇄ `kenali-service` | `FileSource` with two device IDs | A device change disables face unlock until re-enrolled (SEC-16). |
| Kill switch | `kenali-cp` + fake registry | Registry stub | `Disabled=1` makes `SetUsageScenario` return `E_NOTIMPL` at next session creation; UI reads it read-only. |
| Allowed-SID guard | `kenali-cp` | Fake WTS token | Tile shown only for the enrolled SID; other users never see it. |

---

## 4. Fuzzing (≥ 1 h per target, 0 crashes)

Run with `cargo xtask fuzz <target> <secs>` (or `cargo fuzz run <target> -- -max_total_time=3600`). Any crash, hang, panic or memory-safety finding is a release blocker; reproduce, fix, re-run the full hour. Every decoder treats input as hostile: typed enums, strict size caps, no allocation from untrusted lengths.

| Target | Input corpus | What it exercises | Campaign |
|---|---|---|---|
| `auth_decode` | Random/mutated bytes on the auth pipe codec | `Request`/`Response` variants, nonce fields, secret wrapper | ≥ 1 h |
| `admin_decode` | Random/mutated bytes on the admin pipe codec | `AdminRequest`/`AdminResponse`, config keys/values, diagnostics chunks | ≥ 1 h |
| `preview_decode` | Random/mutated preview frames | `w`/`h`/`rgb` length checks, ≤ 512 KB cap, RAM-only handling | ≥ 1 h |
| `frame_parse` | Mutated camera/frame headers | Alignment/decode input validation, dimension guards, PAD crop scales | ≥ 1 h |

Coverage is checked after each campaign; seed corpus is committed under `fuzz/corpus/` (no face data, synthetic bytes only). `crash-*` artefacts are never committed.

---

## 5. Latency & compute benchmarks (NFR-01 / NFR-02)

**NFR-01** (lock screen visible → credential submitted): warm p50 ≤ 1.0 s / p95 ≤ 1.8 s; cold p50 ≤ 2.0 s / p95 ≤ 3.5 s; hard timeout 6 s → password UI.
**NFR-02** (≥ 4 cores): detect + align + embed + passive PAD ≤ 40 ms per qualified frame; warm-up inference at service start so the first real frame is not slow.

Methodology:

1. **Micro-bench** per stage with `criterion` (detect, align, embed, PAD, cosine, frame downscale), reporting p50/p95/p99; no per-frame allocation in the hot path.
2. **Warm vs cold**: warm = camera pre-armed by the wake event (P0-S11); cold = camera must open from wake/lock. Record both — cold is dominated by camera readiness (R4), not ML.
3. **End-to-end trace**: emit `tracing` spans from tile-visible → status → verified → `ReportResult`; measure over ≥ 100 locks in each of warm and cold conditions on the reference laptop; discard the first frame's auto-exposure/white-balance frames (default N = 5, tuned per camera).
4. **p95 is the acceptance number.** Report hardware facts alongside (CPU model/cores, camera VID/PID, FP32 vs INT8, `ort` pin). A miss needs an accepted written exception.
5. Re-run after any hot-path change (Definition of Done) and record before/after.

---

## 6. Recognition accuracy — FAR / FRR protocol (NFR-04)

- **Zero-effort FAR ≤ 0.001 % (1:100,000)** at the operating threshold; **TAR ≥ 95 % / FRR ≤ 5 %** for one enrolled user in normal indoor lighting.
- **Statistical power:** Microsoft's formula implies ≈ **2,237 unique impostor identities / samples** to support a 0.001 % FAR claim at 96 % confidence [R2]. The protocol therefore uses **≥ 2,237 impostor identities** (zero-effort attempts), each contributing at least one sample; the genuine set covers multiple lighting conditions, poses and glasses on/off per FR-01.
- Build the operating threshold from held-out calibration data — **never** OpenCV's 0.363 SFace default (tuned for ~99.6 % LFW accuracy, not 1e-5 FAR).
- Report: confusion counts, FAR/FRR/TAR with confidence intervals, threshold, ROC/DET curve, corpus composition, and the exact `models.lock` hashes used. Commit under `docs/reports/`.

**Procedure:** (1) enrol the owner with ≥ 12 quality-gated templates across ≥ 2 lighting conditions; (2) run genuine attempts across poses/lighting and count accepts (TAR/FRR); (3) run zero-effort impostor attempts from ≥ 2,237 distinct identities and count accepts (FAR); (4) freeze threshold and re-run on a held-out split; (5) assert NFR-04 or record an accepted exception. No face images or embeddings are committed — only aggregate metrics.

---

## 7. Presentation attacks — APCER / BPCER protocol (NFR-05)

**APCER ≤ 1 % per attack species with ≥ 300 attempts/species** (zero failures in 300 trials bounds APCER ≈ 1 % at 95 % confidence — **rule of three**); **BPCER ≤ 5 %**.

### 7.1 Attack protocol (three mandated species)

| Species | Setup | Attempts |
|---|---|---|
| **Printed photo** | High-quality colour print of the enrolled face, held at normal unlock distance/angle, varied lighting | ≥ 300 |
| **Phone-screen replay** | Photo/video of the enrolled face on a phone display, replayed at unlock distance | ≥ 300 |
| **Tablet/monitor replay** | Larger display (tablet or monitor) replaying the capture, including slight movement | ≥ 300 |

Each attempt is scored as attack-presented; APCER = attacks accepted / attempts. Bona-fide trials (≥ 300 per condition) give BPCER = bona-fide rejected / trials. Report per species and fused; include PAD margin distribution, camera/scene conditions, and the passive + temporal + attention contributions. Virtual-camera rejection and comparable-size multi-face are checked as separate attack cases.

**Procedure:** (1) build the attack set with the owner's consent and store it outside the repo; (2) replay via the `FileSource` seam where possible, then confirm on the real camera; (3) run all three species with ≥ 300 attempts each; (4) compute APCER/BPCER and rule-of-three bounds; (5) commit the aggregate report under `docs/reports/` with 0 face images. Fail = any species > 1 % APCER or BPCER > 5 % (or an accepted written exception).

---

## 8. Log-hygiene tests (PRD §7 / FR-12 / FR-19)

**No secret or biometric reaches logs or errors — for the service, the CP **and** the UI.** Similarity scores are **never logged at finer than 0.05 resolution**, and are never rendered on any tile or UI screen.

Applies to `tracing` rotating file output, Windows Event Log entries, `Debug`/`Display`/`anyhow` context, panic messages, and error strings in **all three processes**. Secrets (password, sealed blob, DEK, embeddings, raw/preview frames) must never appear. The UI holds a password transiently in its enrol/refresh field — it must still be absent from logs and error messages.

Test method:

1. **Canary injection:** seed a unique canary string as the password / as synthetic embedding bytes; drive enrolment, refresh-credential, a failed unlock and a wrong-password status in each process; grep the rotated log files, Event Log and captured stderr for the canary. Must be absent.
2. **Error-path sweep:** force every failure branch (pipe reject, bad proto, tampered AEAD, camera timeout, wrong password, locked-out, PAD reject) and assert no error/log string contains secret material or a raw frame/embedding.
3. **Score granularity:** capture logs across a calibration run; assert every logged score is quantised to steps of ≥ 0.05 (e.g. round to nearest 0.05) — no float with finer resolution appears.
4. **Type-level guard:** code review + a compile-time check that secret newtypes are `Zeroizing` with redacting `Debug` and no `Clone`/`Display`; no `String`/`format!` in secret paths; UI crate links no auth-pipe types.
5. **Zeroization:** after cancel/error/exit, secret and preview buffers compare as zeroed in a debug assertion.

Pass condition: all three processes (service, CP, UI) show 0 canary hits, 0 fine-grained scores, 0 secret types with leaky traits. This test is re-run in P0, P2 and P4 and is a Definition-of-Done gate for any task touching logging or secrets.

---

## 9. Soak tests (NFR-06 / reliability)

| Soak | Environment | Duration | Pass criteria |
|---|---|---|---|
| **Lock/unlock soak** | Windows VM (snapshot before install) | **1,000 cycles** | 0 CP crashes, 0 lock-outs, password/PIN fallback available at every cycle, camera LED off when idle, no handle/memory growth (CP RSS ≤ 5 MB, service idle RSS ≤ 150 MB). |
| **Secondary-account soak** | Secondary host account (main account untouched) | **2 weeks** | 0 lock-outs, allowed-SID guard holds (main account never sees the tile), no unexpected disables, refresh-credential works after a password change, hardware matrix (§10) events handled. |

Interleave failures deliberately: kill the service mid-attempt, unplug/replug the camera, change the password (→ stale-credential auto-disable), and flip the kill switch. After each, the account must remain sign-in-able by password.

---

## 10. Hardware acceptance matrix

Executed on the reference laptop / VM by the owner; each row is a pass/fail with evidence.

| Scenario | Check | Expectation |
|---|---|---|
| **Sleep (S3)** | Sleep → wake → lock screen → face unlock | Camera re-arms within the backoff deadline; first frame confirmed; unlock meets NFR-01 warm target. |
| **Hibernate (S4)** | Hibernate → resume → unlock | As sleep; TPM unwrap after resume works; no stale credential state. |
| **Lid close/open** | Close lid (sleep) → open → unlock | Camera readiness after open; fallback never delayed. |
| **Fast Startup** | Shut down with Fast Startup on → power on → first sign-in | First sign-in after boot is **password-only** (SEC-18); CP behaviour consistent with R7/R14; no auto-logon surprise. |
| **Camera contention** | Open another camera app, then attempt unlock | Service owns the camera; wizard warns to close other apps; attempt fails gracefully, password works, no crash. |
| **First unlock after Windows Update** | Apply an update that reboots, then unlock | CP still registered and loads; first sign-in password-only; face works thereafter; scenario reporting still disambiguated (U3). |

Related unknowns U11 (camera after resume/hibernate/Fast Startup) and R2/R4 are de-risked in P0-S11 and finally accepted here.

---

## 11. End-to-end UI walkthrough (no terminal)

Recorded in Phase 5. Every step is performed in `kenali-ui.exe` (elevated via UAC at launch); the tile is observed at the lock screen. The UI must not be required for unlock/fallback/recovery (SEC-20).

1. **Launch** — open `kenali-ui.exe`, accept UAC, confirm single Dashboard window with service/camera/TPM/BitLocker badges and enrolled SID.
2. **Enrol via wizard** — Dashboard → Enrol wizard: prerequisite badges green → enter password in masked field (inline error on wrong password, never sealed) → guided capture with live preview (straight / left / right / up / down, ≥ 12 templates across ≥ 2 lighting conditions) → seal to TPM → self-test → summary. Cancel at least once and confirm buffers wiped.
3. **Change a setting** — Settings: toggle a scope or cooldown; confirm the typed admin command is applied and reflected at next session creation; PAD thresholds are read-only.
4. **Add a template** — Templates → Add (mini capture flow); confirm it appears in the list.
5. **Delete a template** — Templates → select → Delete with confirm; confirm removal.
6. **Export diagnostics** — Diagnostics → Export redacted ZIP → choose save location; confirm the ZIP contains no templates, embeddings, passwords or frames.
7. **Refresh credentials** — from the Security screen (after a simulated password change): Refresh credentials wizard re-validates and reseals; confirm a stale-credential auto-disable is cleared.
8. **Disable/Enable** — Security → Disable, observe "Face unlock paused" and the kill-switch state (read-only); lock screen still signs in with the password; re-Enable and confirm face unlock returns.
9. **Lock-screen check** — lock the session; confirm the tile appears only for the enrolled SID, shows generic wording (no scores), and falls back to password within 300 ms when the service is stopped.
10. **No terminal** — verify every action above required no command line; the recovery CLI was never invoked.

---

## 12. Recovery drills R-A…R-E

From `dev_safety_runbook.md`; each executed once successfully before daily driver.

| Drill | Scenario | Expected recovery |
|---|---|---|
| **R-A** | CP misbehaves / lock-out feared | Set `Disabled=1` under the CP CLSID key; at next session creation the tile is gone and password sign-in works. |
| **R-B** | Cannot reach Windows normally | Boot Safe Mode — only the built-in password provider loads; sign in and remove/disable the CP. |
| **R-C** | Safe Mode unreachable | WinRE command prompt: edit the offline SOFTWARE hive or delete the DLL; reboot to password sign-in. |
| **R-D** | Service crash/hang or install failure | `cargo xtask rollback` / `uninstall` restores the previous version and registration; password works; no lock-out. |
| **R-E** | Stale credentials / password change loop | Face unlock auto-disables with no retries; sign in with the new password; Refresh credentials wizard reseals; no account lockout. |

---

## 13. Requires the owner's Windows VM (cannot run on this Linux host)

The development host is WSL2 Linux (`x86_64-unknown-linux-gnu` only). The following are **delivered as `xtask`/VM scripts plus step-by-step manual instructions and spike/report docs** — not claimed as executed here. The agent never registers a CP, installs the service, or modifies Windows registry state on this machine.

- All 13 Phase-0 spikes (P0-S1…P0-S13) on the reference laptop / VM — CP hosting, Session-0 camera capture, TPM unwrap, account/auth-package matrix, Smart App Control, renderer, preview throughput, power-transition readiness.
- End-to-end latency traces on the reference laptop (NFR-01/02 final acceptance).
- Recognition (FAR/FRR) and PAD (APCER/BPCER) data collection with the real camera and the owner's face; the three-species attack protocol.
- 1,000-cycle lock/unlock soak and 2-week secondary-account soak.
- Hardware acceptance matrix (§10): sleep, hibernate, lid, Fast Startup, camera contention, first unlock after Windows Update.
- End-to-end UI walkthrough recording (§11) and all recovery drills R-A…R-E (§12).
- Binary signing / Smart App Control / WDAC verification and SBOM generation on the Windows toolchain.

**Host-executable instead:** platform-agnostic unit/property/integration tests (§2–3), fuzz campaigns on decoders/parsers (§4), micro-benchmarks (§5), `cargo check --target x86_64-pc-windows-msvc --locked` for Windows crates, `cargo-deny`/`cargo-audit`, and the static portions of log hygiene (canary/score checks against captured log fixtures).
