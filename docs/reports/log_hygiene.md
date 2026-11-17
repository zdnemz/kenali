# Log-Hygiene Proof — Service, CP and UI (Wave 4)

| Field | Value |
|---|---|
| Status | **Owner-run proof, pending execution.** Runtime assertions are marked **VERIFY** (static review on the Linux/WSL2 host) or **OWNER** (to be executed and filled in by the owner in the Windows VM). |
| Scope | `kenali-svc` (service), `kenali-cp` (Credential Provider), `kenali-ui` (management GUI). |
| Authority | `docs/prd.md` FR-12, FR-19, SEC-01, SEC-02, SEC-12, SEC-14; `docs/test_plan.md` §8; `docs/threat_model.md` R14, TM-05, TM-07, TM-11, TM-15, TM-19, TM-20, TM-21; `docs/context_pack.md` §2 item 19, §3.8. |

---

## 1. Boundary — read before trusting any result

**Nothing in this proof was executed on this Linux/WSL2 host.** No log was captured, no Event Log was read, no process was run, no sentinel was injected and no test was executed here. The development host is `x86_64-unknown-linux-gnu`; it cannot register a CP, install/start the service, open a camera, trigger UAC or write Windows registry state. All capture, injection, inspection and attack flows are **OWNER-run in a Windows VM**, as required by `docs/test_plan.md` §13.

- **VERIFY** = a static, code/document-level claim checked by review on this host (type guards, string tables, emitted-field review). It proves code shape and intent, **not** runtime output.
- **OWNER** = a runtime assertion the owner must execute in the VM and record. Until the sign-off table (§8) is filled, this document is a proof scaffold, not a claim that logs were captured or inspected.

Code shape was reviewed against the current tree for the mechanism names below; where an emitted message or a wipe can only be observed at runtime, the row is OWNER. There is no "we captured the logs" statement in this document, and none may be added until the owner signs.

---

## 2. The two log sinks and the universal invariant

### 2.1 Sinks (all three processes)

1. **Windows Event Log** — authored through `kenali_winapi::eventlog`. The fixed vocabularly is:
   - **1000** = `EVENT_SERVICE_START` — service started.
   - **1001** = `EVENT_PAD_BURST` — aggregate PAD/burst rejection count.
   - **1002** = `EVENT_DISABLED` — face unlock disabled / kill-switch state.

   These ids carry coarse state and counts only: no account password, no embedding, no frame, no free-text SID, no numeric score finer than 0.05, no internal reason code. The CP and UI do not invent new ids; any status they surface reuses this vocabulary with generic wording. VERIFY (message tables) / OWNER (Event Log dump).
2. **Rotating structured trace log** — `tracing` output to a size/age-rotated file in each process's own data directory, with fixed typed fields. There is no free-text interpolation of credential, biometric or frame data. VERIFY (field review) / OWNER (captured file).

For this proof the sink definition also covers every string-bearing surface named in `docs/test_plan.md` §8: `Debug`/`Display` output, `anyhow`/`thiserror` context, panic messages, error/status strings returned over pipes, captured stderr, and the redacted diagnostics ZIP.

### 2.2 Universal never-log list — `kenali-svc`, `kenali-cp`, `kenali-ui`

The following are **never logged**, never placed in an error string, and never rendered in any sink above:

- account **password** in any form (plaintext, UTF-16, wide blob), the sealed password blob, the DEK or derived key material (SEC-01);
- **embeddings** / face templates, in raw bytes or any encoding (SEC-02);
- raw frames, enrolment frames and **preview** frames in any encoding (§5);
- **face images** or any reconstructable derivative;
- **SID**s in free text — SIDs may appear only as structured typed fields where the store AAD requires them, never inside a message;
- numeric match/similarity values at resolution **finer than 0.05** — any emitted numeric telemetry is quantised to steps ≥ 0.05, and scores are never rendered on a tile or in the UI (SEC-14);
- **internal reason codes** — raw `NTSTATUS` values, policy/PAD sub-scores, codec discriminants, path details. Callers receive generic failure wording only.

> **Universal invariant: no secret reaches a log.** In all three processes, logs contain no password, no embedding, no biometric, no raw or preview frame, no free-text SID, no fine-grained score and no internal reason code. This invariant is what the three process sections, the preview section, R14 and the red-team plan in §7 defend.

### 2.3 Mechanisms (VERIFY)

| Mechanism | Where | Effect / status |
|---|---|---|
| **Redacting newtypes whose `Debug` prints `<redacted>`** | `kenali_core::secret::{Password, Embedding}` (`Password(<redacted>)`, `Embedding(<redacted>)`); `kenali_ipc::wire::{PasswordWide, SecretWide}`; `kenali_store::keys::KeyHandle`; `Frame` redacts the RGB field | Formatting a secret in any `{:?}` path emits only the marker, never bytes or length. No `Display`, no `Clone` on the core secret newtypes; password-bearing `AdminRequest` variants are not `Clone`. **VERIFY** |
| **`zeroize` / `Zeroizing`** | `Zeroizing<Vec<u16>>` / `Zeroizing<Vec<f32>>` in core secrets; `Drop`-wipe on `PasswordWide`/`SecretWide`; `Zeroizing` wire and control buffers in service/winapi; `Zeroizing<Vec<u8>>` preview slot in the service | Secret buffers are wiped on success, error, cancel and drop paths. **VERIFY** (shape) / **OWNER** (runtime debug wipe assertion) |
| **Generic failure wording** | `kenali_core::strings` / `tiles`, CP `HRESULT` strings, admin-pipe `Error{code:u16}`, UI `strings` table | One shared, score-free English table; no SID, path, secret, score or Win32 detail in user- or log-facing text. **VERIFY** |
| **No `format!` of secrets** | all secret, admin-command and preview paths | No secret is ever a formatting argument; typed messages carry bytes opaquely and redacted `Debug` is the only textual view. **VERIFY** |
| **Redactor backstop** | `kenali_service::diagnostics::redact` and `kenali_service::logging::safe_message`, used by every Event Log wrapper and the diagnostics ZIP builder | Masks `S-1-` SIDs, paths, `@`-tokens, decimal scores and long hex blobs to `[redacted]` before emission; `quantize_score` rounds numeric telemetry to 0.05 steps. **VERIFY** |
| **Dependency boundary** | `kenali-ui` depends on core/ipc/winapi only; no camera, store, vision, CP or auth-pipe types linked into the UI | The UI cannot reach the auth pipe or a stored secret, so there is none to log. **VERIFY** |

Type-level review is necessary but not sufficient: the sentinel test in §7 is the actual proof and is **OWNER**.

---

## 3. `kenali-svc` — service process

**What IS logged:**
- Event Log ids **1000 / 1001 / 1002** only — service start, aggregate PAD/burst counts, and disabled/kill-switch state. Fixed generic messages; no score, biometric or SID in the text. VERIFY / OWNER dump.
- Rotating structured trace log: lifecycle, phase/state, typed policy-outcome category, attempt counts and coarse latency spans. Numeric telemetry, where genuinely needed, is quantised to ≥ 0.05 steps; no raw float, no face material. VERIFY / OWNER capture.
- Diagnostics export: a redacted ZIP assembled from fixed status/config fields and returned as bytes on the admin pipe — no templates, embeddings, password or frames (TM-20). VERIFY / OWNER inspection.

**What is NEVER logged:** password, sealed blob, DEK (SEC-01); embeddings/templates (SEC-02); raw, enrolment or preview frames and face images (SEC-02, TM-15); SIDs in free text; match values finer than 0.05; internal policy/PAD reason codes (SEC-14). Unlock traces measure latency only — never a biometric payload.

**Mechanisms:** redacting newtypes print `<redacted>`; crypto and pipe wire buffers are `Zeroizing`; the service never calls `format!` on a secret; decoded auth/admin/preview frames are typed and size-capped before any trace field is emitted; password and preview buffers are wiped on every error path (SEC-12). All Event Log text passes `safe_message` → `redact`, and the diagnostics builder calls the same redactor. Failure paths return generic wording rather than raw `NTSTATUS` or sub-scores.

**Status:** VERIFY (static shape) / OWNER (runtime result in §8).

---

## 4. `kenali-cp` — Credential Provider process

**What IS logged:**
- The CP emits **no secret-bearing Event Log entries**. Ids 1000/1001/1002 are service-owned; CP status reaches LogonUI and the tile through generic, score-free wording from `kenali_core::tiles` (installed text only, no log payload). OWNER dump confirms no extra entries.
- If CP-side `tracing` is enabled on the VM, the rotating structured log holds lifecycle and coarse outcome only — no password, no score, no reason code. VERIFY / OWNER capture.

**What is NEVER logged:** the password submitted on a verified unlock (sent one-shot to the auth pipe, then released); embeddings or frames (no camera/vision dependency in the CP); face images; SIDs in free text; numeric matches finer than 0.05; internal reason codes or raw `NTSTATUS`. SEC-01, SEC-02, SEC-12, SEC-14 and TM-15 apply verbatim.

**Mechanisms:** every COM entry is panic-fenced and returns an `HRESULT` with **generic failure wording**; the one-shot secret is held in `Zeroizing<Vec<u16>>`, redacts as `<redacted>` under `Debug`, has no `Clone`/`Display`, is never passed to `format!`, and is explicitly `zeroize`d on credential release and on `SetUsage` reset. No ML/network/async runtime and no camera/store/vision crates, so no frame or embedding material exists in-process. VERIFY / OWNER (runtime wipe and log capture).

---

## 5. `kenali-ui` — management GUI process

**What IS logged:**
- Ids **1000 / 1001 / 1002** are emitted by the service on the UI's behalf; the UI writes no ad-hoc event ids. VERIFY / OWNER dump.
- The rotating structured trace log is diagnostic-only: screen transitions, typed admin-command names (`GetStatus`, `SetEnabled`, …), pipe connection lifecycle and generic worker `Error{code:u16}` events. No field carries a password, frame, score or free-text SID. VERIFY / OWNER capture.
- No auth-pipe types are linked (FR-19): UI source contains no `AUTH_PIPE`, `AuthPipeClient`, `kenali_ipc::auth` or `SecretWide` usage and no camera/store/vision/CP crates. Structurally the UI never receives a secret from the auth pipe, so there is nothing there to log. VERIFY.

**What is NEVER logged:** password (FR-19), sealed secret, embeddings, raw/enrolment/preview frames, face images, SIDs in free text, numeric matches finer than 0.05, internal reason codes. All UI text comes from one English string table with generic failure wording — no scores, similarities, confidence or PAD margins anywhere (SEC-14, TM-11).

**Mechanisms:** `kenali_ui::password_field::{PasswordField, submit_password}` wraps the egui `String`; on submit the characters are moved into `kenali_core::secret::Password` and the source is cleared immediately — no clone, no log, no persistence (§6, R14). `kenali_ui::pipe_worker` passes typed `PipeCommand`/`PipeEvent` over bounded channels; password variants carry `Password`, never `String`; errors use generic `Display`. Secret types print `<redacted>` under `Debug`; no `format!` takes a secret. VERIFY / OWNER.

---

## 6. Preview frames — RAM-only in service and UI

Downscaled **320×240 RGB** frames travel on the preview pipe only between `BeginPreview` and `EndPreview`, while an enrolment/calibration window is open.

- **RAM-only in both the service and the UI.** The service owns the camera and holds the current frame in a `Zeroizing<Vec<u8>>`; the UI copies it into a private zeroizing `PreviewSlot` and renders it straight from RAM. The UI never opens the camera itself (`docs/context_pack.md` §3.8).
- **Wiped on `EndPreview`, on wizard cancel and on app exit** — `clear()` overwrites the bytes and drops the buffer; error and disconnect paths clear it too. VERIFY / OWNER debug-assertion wipe check.
- **Never encoded:** raw RGB on the wire, no JPEG/PNG, no codec dependency in the preview path.
- **Never on disk and never logged:** no frame bytes reach the rotating trace log, Event Log, an error string, stderr or the diagnostics ZIP (TM-05, TM-19).
- Geometry and the ≤ 512 KB cap are validated before a frame enters the slot; a rejected frame is dropped, never logged.

**Status:** VERIFY (static shape) / OWNER (runtime wipe and sink grep).

---

## 7. R14 — transient `String` password in the UI (accepted risk)

egui/eframe text fields are **`String`-based**. During the Enrol and Refresh-credentials wizards the account password therefore exists briefly as a plaintext `String` in `kenali-ui.exe`. We do not pretend otherwise.

The accepted control:

- On submit the characters are **moved into the typed admin-pipe message** (`EnrolPassword(PasswordWide)` / `RefreshCredential(PasswordWide)`, via `PipeCommand::EnrolPassword(Password)` / `RefreshCredential(Password)`) and the source `String` is **cleared immediately after submit** — no clone, no retention in UI state.
- The field is **never cloned, logged or persisted**: it does not reach the trace log, Event Log, an error/`Display` string, stderr, the diagnostics ZIP or disk.
- The password is never sent to the auth pipe and is never made available to the UI beyond the wizard; the service validates it with `LogonUser` and seals it to the TPM before discarding it.
- Consequently, even though the widget is `String`-based, **no secret reaches a log** from the UI path: the buffer is short-lived, moved out, cleared, and structurally outside every emitter.

This is a documented, accepted residual risk (medium likelihood / medium impact): an attacker with local admin or a process-memory-read primitive could scrape the plaintext during the brief entry window. It is accepted in `docs/threat_model.md` §5 (R14), cross-referenced by `docs/prd.md` §14 R14 and SEC-12/SEC-19, `docs/context_pack.md` §2 item 19 and §3.8, and threats TM-07/TM-21/TM-22. Revisit if the GUI framework changes (prefer a secure edit control) or if privileged local memory capture enters the attacker set. The runtime memory window is out of scope for this proof. **VERIFY** (documented and code-shaped) / **OWNER** (owner sign-off).

---

## 8. Red-team test plan — sentinel canaries

The executable harness is **`tests/ci/log_hygiene`** (reference only — this task does not create it). It is owner-run in the Windows VM and implements `docs/test_plan.md` §8; the threats defended are recorded in `docs/threat_model.md` (R14, TM-05, TM-07, TM-15, TM-19, TM-20, TM-21).

**Method**

1. **Seed sentinel strings.** Generate unique greppable canaries and inject them as: the password (enrol, refresh, wrong-password), synthetic **embedding** bytes, a synthetic **raw frame** and a synthetic **preview frame**, a marker **SID**, and a marker reason/`NTSTATUS` value. Record the exact sentinels in a temp file outside the repo.
2. **Drive the flows** for `kenali-svc`, `kenali-cp` and `kenali-ui`: enrol (with preview open), refresh credential, successful unlock, failed unlock (wrong password, PAD reject, pipe reject, tampered AEAD) and diagnostics export. Sweep the error branches of `docs/test_plan.md` §8 so error strings, not only happy paths, are exercised.
3. **Collect both log sinks:** the rotated trace files of all three processes and the Windows Event Log (ids 1000/1001/1002 plus any other entries authored by those binaries). Also capture stderr and the diagnostics ZIP contents as additional sinks.
4. **Grep both sinks; assert zero hits** for every sentinel in every sink, plus structural patterns: raw frame/embedding bytes, free-text `S-1-` SIDs outside typed fields, floats finer than 0.05, and internal reason-code tokens. **0 hits per sink per process.**
5. **Score-granularity check:** every logged numeric score must be quantised to ≥ 0.05 steps; no finer float appears.
6. **Preview wipe check:** after `EndPreview`, wizard cancel and app exit, assert the slot is wiped and no frame bytes appear in any sink.

**Pass condition:** `kenali-svc`, `kenali-cp` and `kenali-ui` each show **0 sentinel hits, 0 raw-frame/embedding hits, 0 fine-grained scores and 0 free-text SIDs** in both the Event Log and the rotating trace log. Re-run in P0, P2 and P4; it is a Definition-of-Done gate for any task touching logging or secrets (`docs/test_plan.md` §8).

**Status:** OWNER — not executed on this host.

---

## 9. Coverage mapping

| Requirement | Demand | Covered by | Status |
|---|---|---|---|
| **FR-12** | Event Log + rotating structured logs with no secrets/biometrics, including the UI; log-hygiene test passes for service, CP and UI; score resolution ≥ 0.05 | §2 sinks/invariant, §3 svc, §4 cp, §5 ui, §6 preview, §8 test plan | VERIFY design / OWNER runtime |
| **FR-19** | UI never receives sealed secret, stored password or embeddings; password field cleared immediately after submit; no secret in UI logs/state/errors; no auth-pipe access | §5 ui, §7 R14, §2.3 dependency boundary, §8 (UI process) | VERIFY design / OWNER runtime |
| **SEC-14** | Audit trail in Event Log without sensitive data; ids 1000/1001/1002 carry only coarse state/counts | §2.1, §3, §4, §5 | VERIFY id/field review / OWNER Event Log dump |

Supporting clauses exercised by the same test: SEC-01, SEC-02, SEC-12 (§2.2 never-log list, `zeroize`/`Zeroizing` on every exit path, RAM-only preview).

---

## 10. Sign-off

| Check | Process / sink | Owner action | Status |
|---|---|---|---|
| Redacting newtypes print `<redacted>`; no `Clone`/`Display` on secrets | svc / cp / ui | code review | **VERIFY** |
| `zeroize` / `Zeroizing` on every exit path | svc / cp / ui | code review + debug assertion | **VERIFY** (logic) / **OWNER** (runtime wipe) |
| Generic failure wording; no `format!` of secrets | svc / cp / ui | string-table + grep review | **VERIFY** |
| Event Log ids 1000/1001/1002 carry state/counts only | svc (cp/ui via svc) | dump Event Log in VM | **OWNER** |
| Rotating trace log: 0 secret/biometric hits | `kenali-svc` | run `tests/ci/log_hygiene`, grep log dir | **OWNER** |
| Rotating trace log: 0 secret/biometric hits | `kenali-cp` | run `tests/ci/log_hygiene`, grep log dir | **OWNER** |
| Rotating trace log: 0 secret/biometric hits | `kenali-ui` | run `tests/ci/log_hygiene`, grep log dir | **OWNER** |
| Enrol / refresh / unlock / failure / diagnostics flows covered with sentinels | all | run flows, grep both sinks | **OWNER** |
| Score resolution ≥ 0.05 everywhere | all | capture calibration run, regex floats | **OWNER** |
| Preview frames: never encoded / on disk / logged; wiped on EndPreview, cancel, exit | svc + ui | inspect log + temp dirs; debug assert wipe | **OWNER** |
| R14 residual risk accepted and recorded | ui | threat-model review | **VERIFY** (documented) / **OWNER** (sign) |

Owner signature / VM build: `______________________`  Date: `__________`

Until every **OWNER** row is filled with captured evidence, this document proves the *design* of log hygiene only — no test was run and no log was produced or inspected on this host.
