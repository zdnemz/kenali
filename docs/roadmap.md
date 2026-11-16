# Roadmap — Kenali

> Companion to `prd.md` §15 (milestones) and `context_pack.md` §4 (known unknowns) / §11 (Definition of Done).
> Phase order: **P0 Feasibility spikes → P1 Secure skeleton → P2 Vision v1 + enrol wizard → P3 Liveness/attention/PAD → P4 Hardening & performance → P5 Daily-driver rollout & soak.**
> Indicative total: **11–16 part-time weeks** (Rev 0.2 UI work included).
> Priorities: **security → never lock the owner out → speed → footprint.**
>
> **Host status.** This repository is currently developed on a WSL2 Linux host (`x86_64-unknown-linux-gnu` only). No Phase-0 spike has been run: every spike below is `NOT STARTED` and must be executed by the owner inside the Windows VM. The agent produces code, `xtask`/VM scripts and spike-report scaffolds only — it never registers a Credential Provider, installs the service or touches Windows registry state on the development host. See `dev_safety_runbook.md` and the "Requires the owner's Windows VM" section of `test_plan.md`.

---

## 1. Orchestrator wave gating

Work is released in waves; a phase may not start until its gating wave deliverable is merged.

| Wave | Theme | Feeds phase(s) | Gate to enter next wave |
|---|---|---|---|
| **Wave 2** | CP feasibility spike | P0 (P0-S1), P1 | CP loads in LogonUI in the VM, shows a tile, declines all scenarios but `CPUS_UNLOCK_WORKSTATION`, never submits, kill switch verified; spike report `docs/spikes/P0-S1.md` written. |
| **Wave 3** | Service + vision | P1, P2, P3 | Empty crates compile for `x86_64-pc-windows-msvc`; host-side unit/property/integration tests for `kenali-core`/`kenali-ipc`/`kenali-store`/`kenali-vision` pass; camera + TPM + ort spikes decided go/pivot. |
| **Wave 4** | UI + CLI | P2 (enrol wizard, live preview), P1 (UI shell) | `kenali-ui.exe` Dashboard shell and the typed admin/preview pipe contracts compile; recovery CLI present; no terminal needed for any FR-09 action. |
| **Wave 5** | Hardening & performance | P4, P5 | Fuzz ≥ 1 h with 0 crashes, NFR-01/02 measured, log hygiene green for service/CP/UI, soak and recovery drills passed before daily-driver rollout. |

---

## 2. Phase 0 — Feasibility spikes

**Intent (PRD §15):** resolve every "VERIFY"/"known unknown" on the *reference laptop / VM* before committing to the architecture. Nothing is assumed. Each spike ends with a report using the template in `context_pack.md` §11: environment (OS build, CPU, camera VID/PID, TPM) · procedure · raw results · evidence (logs/screens) · decision (go / pivot / stop) · follow-ups. All spikes are **`NOT STARTED`**.

### 2.1 U1–U13 → spike map

| Unknown (context_pack §4) | Unknown statement | Spike(s) |
|---|---|---|
| U1 | Media Foundation capture from a Session 0 SYSTEM service at the lock screen | P0-S2 |
| U2 | Which thread may call `CredentialsChanged`; GIT marshalling pattern | P0-S1 |
| U3 | `CPUS_LOGON` vs `CPUS_UNLOCK_WORKSTATION` reporting and the WTS check | P0-S6 |
| U4 | Negotiate vs MSV1_0 for local and Microsoft accounts | P0-S6 |
| U5 | Interaction with the Microsoft-account "Windows Hello-only sign-in" setting | P0-S6 |
| U6 | TPM unwrap latency (RSA-2048) | P0-S4 |
| U7 | Smart App Control / WDAC vs self-signed binaries | P0-S7 |
| U8 | `ort` stable 2.0 status; INT8 vs FP32 accuracy and latency | P0-S5 |
| U9 | Availability of `RequestSerialization` / `SetDisplayState` on this Windows build | P0-S1 |
| U10 | Is LocalSystem required, or does a virtual service account work for camera + TPM? | P0-S2 |
| U11 | Camera readiness after resume / hibernate / Fast Startup | P0-S11 (de-risked in P0; final acceptance in P4) |
| U12 | egui/eframe pinned version; `glow` vs `wgpu` on the reference GPU and in a GPU-less VM; per-monitor DPI | P0-S9 (de-risked in P0; final acceptance in P1) |
| U13 | Preview pipe throughput and CPU cost (downscale + send at ≥ 15 fps) | P0-S10 (de-risked in P0; final acceptance in P2) |

> Notes: U3–U5 share P0-S6 because scenario detection, auth package and Microsoft-account interaction are tested in the same VM session. U9 shares P0-S1 with U2 because both are CP-hosting/COM-thread questions. U11–U13 are pulled forward into Phase 0 as de-risking spikes; their pass criteria are re-verified in the named later phase.

### 2.2 Spike checklist

| ID | Status | Spike / decision | Report |
|---|---|---|---|
| P0-S1 | NOT STARTED | Credential Provider hosting + GIT thread question: can a Rust `cdylib` CP load in `LogonUI.exe`, show a tile reading "spike", decline every scenario except `CPUS_UNLOCK_WORKSTATION`, never submit, and honour the kill switch? Determine which thread may call `CredentialsChanged`, the GIT marshalling pattern (U2), and whether `RequestSerialization` / `SetDisplayState` exist on this build (U9). | [docs/spikes/P0-S1.md](docs/spikes/P0-S1.md) |
| P0-S2 | NOT STARTED | Media Foundation capture from Session 0: open the built-in RGB camera from a SYSTEM service at the lock screen via `IMFSourceReader` with backoff + first-frame confirmation; decide LocalSystem vs virtual service account for camera + TPM (U10). Do **not** use WinRT `MediaCapture`. | [docs/spikes/P0-S2.md](docs/spikes/P0-S2.md) |
| P0-S3 | NOT STARTED | Auth-pipe contract: SYSTEM-only DACL (`D:P(A;;GA;;;SY)`), `FIRST_PIPE_INSTANCE`, remote-client rejection, server-PID anti-squatting check against SCM, nonce-bound one-shot secret release; probe the CP↔service handshake end to end. | [docs/spikes/P0-S3.md](docs/spikes/P0-S3.md) |
| P0-S4 | NOT STARTED | TPM unwrap latency: CNG `MS_PLATFORM_CRYPTO_PROVIDER`, non-exportable RSA-2048 key, `NCryptFinalizeKey`, RSA-OAEP-SHA256 unwrap of the DEK; measure p50/p95 against NFR-01 (U6). | [docs/spikes/P0-S4.md](docs/spikes/P0-S4.md) |
| P0-S5 | NOT STARTED | ONNX Runtime stack: confirm `ort` stable 2.0 status / pin exact version, `load-dynamic` with our own absolute-path `onnxruntime.dll`, INT8 vs FP32 accuracy and latency on YuNet/SFace (U8). `tract` as benchmark fallback. | [docs/spikes/P0-S5.md](docs/spikes/P0-S5.md) |
| P0-S6 | NOT STARTED | Account types, auth package & scenario detection: local vs Microsoft account with a password; Negotiate vs MSV1_0; Microsoft "Hello-only sign-in" setting on/off (U5); `CPUS_LOGON` vs `CPUS_UNLOCK_WORKSTATION` reporting with the WTS session-state disambiguation (U3, U4). | [docs/spikes/P0-S6.md](docs/spikes/P0-S6.md) |
| P0-S7 | NOT STARTED | Smart App Control / WDAC vs self-signed binaries: confirm self-signed service/CP/UI/CLI load on the reference build or document the signing/enrolment path (U7, R9). | [docs/spikes/P0-S7.md](docs/spikes/P0-S7.md) |
| P0-S8 | NOT STARTED | Recovery & kill-switch mechanics in the VM: `Disabled=1` under the CP CLSID key takes effect at next session creation; Safe Mode and WinRE fallback reachable; second admin account confirmed (R1, R5). | [docs/spikes/P0-S8.md](docs/spikes/P0-S8.md) |
| P0-S9 | NOT STARTED | GUI framework pin & renderer: pin egui/eframe version, `glow` vs `wgpu` on the reference GPU and in a GPU-less VM, per-monitor DPI awareness, UI cold start ≤ 2 s (U12, NFR-10). | [docs/spikes/P0-S9.md](docs/spikes/P0-S9.md) |
| P0-S10 | NOT STARTED | Preview pipe throughput: downscale to 320×240 RGB in RAM, stream over `\\.\pipe\kenali.preview` at ≥ 15 fps within NFR-02 budget (≤ 5 % of one core); RAM-only, zeroized, no disk (U13, FR-18). | [docs/spikes/P0-S10.md](docs/spikes/P0-S10.md) |
| P0-S11 | NOT STARTED | Camera readiness after resume / hibernate / Fast Startup: measure open-to-first-frame delay after power transitions and tune the backoff/re-enumeration policy (U11, R2, R4). | [docs/spikes/P0-S11.md](docs/spikes/P0-S11.md) |
| P0-S12 | NOT STARTED | Reference-laptop fact sheet collection: `winver`, `Get-Tpm`, `manage-bde -status`, `Confirm-SecureBootUEFI`, camera VID/PID + formats, `Win32_Processor`, Smart App Control state, account type, GPU/GL availability (context_pack §10). | [docs/spikes/P0-S12.md](docs/spikes/P0-S12.md) |
| P0-S13 | NOT STARTED | End-to-end unlock smoke in the VM: minimal CP → auth pipe → service → policy → password submit → desktop, with password fallback intact; integrates S1, S2, S3, S4, S6. | [docs/spikes/P0-S13.md](docs/spikes/P0-S13.md) |

### Phase 0 deliverables (gated by wave 2 CP spike)

- [ ] VM created with a second local admin account; kill switch and Safe Mode/WinRE recovery verified (P0-S8).
- [ ] Reference-laptop fact sheet recorded (P0-S12).
- [ ] Minimal Rust CP spike loads in LogonUI, shows a tile, declines unsupported scenarios, never submits (P0-S1).
- [ ] Camera captured from Session 0 with confirmed first frame; LocalSystem decision recorded (P0-S2).
- [ ] Auth-pipe DACL, anti-squatting and one-shot release prototyped (P0-S3).
- [ ] TPM wrap/unwrap latency measured (P0-S4); `ort` version/pin and INT8 accuracy measured (P0-S5).
- [ ] Account/auth-package/scenario matrix recorded (P0-S6); Smart App Control decision recorded (P0-S7).
- [ ] GUI renderer, preview throughput and power-transition readiness de-risked (P0-S9, P0-S10, P0-S11).
- [ ] All 13 spike reports written under `docs/spikes/` with a go / pivot / stop decision.

---

## 3. Phase 1 — Secure skeleton (service + CP + UI shell)

**Intent (PRD §15):** stand up the empty-but-compiling workspace and the security skeleton — service host, thin CP with panic firewall/allowed-SID guard/kill switch, and a `kenali-ui.exe` shell with an empty Dashboard. Lock down pipes, secrets and build config before adding vision.

- [ ] Workspace skeleton from `context_pack.md` §6; every crate compiles empty; `cargo xtask check` passes for `x86_64-pc-windows-msvc`.
- [ ] Pinned toolchain, `Cargo.lock` (`--locked`), `deny.toml`, `clippy.toml`, release flags; `cargo-deny` / `cargo-audit` clean.
- [ ] `kenali-core`: config schema, state machine, policy engine skeleton, redacting secret newtypes; `forbid(unsafe_code)`.
- [ ] `kenali-ipc`: `PROTO_VERSION` handshake; auth, admin and preview typed messages with size caps; postcard codec; `forbid(unsafe_code)`.
- [ ] `kenali-store`: AEAD file format + key-hierarchy traits; hash-pinned `models.lock`; `forbid(unsafe_code)`.
- [ ] `kenali-winapi`: NCrypt/TPM, LSA/WTS, pipes + SDDL, registry, service helpers, Event Log, UI helpers; every `unsafe` has `// SAFETY:`.
- [ ] `kenali-service`: SYSTEM host, three pipes, orchestration skeleton, warm-up hook.
- [ ] `kenali-cp`: COM glue only, `catch_unwind` on every entry point, allowed-SID guard, `E_NOTIMPL` for unsupported scenarios, kill switch read.
- [ ] `kenali-ui` shell: single-window egui Dashboard (empty), single-instance mutex, elevation relaunch, DPI awareness; `forbid(unsafe_code)`.
- [ ] `kenali-cli` recovery CLI skeleton; `cargo xtask` commands (check/build/models/sign/install/uninstall/rollback/fuzz/bench).
- [ ] Event Log + rotating structured logs with `tracing`; log hygiene green for service, CP and UI (`test_plan.md` §7).
- [ ] Integration tests crossing crate boundaries (core↔ipc, store↔winapi, service↔cp over the pipes).

**Wave gating:** wave 2 (CP spike decided) → wave 3 (service + vision).

---

## 4. Phase 2 — Vision v1 + calibration + enrol wizard

**Intent (PRD §15):** implement detect → align → embed → match on CPU, calibrate the operating threshold from data (never tutorial defaults), and deliver the full enrolment wizard with RAM-only live preview.

- [ ] `kenali-vision`: YuNet detect → 5-point similarity-transform align → SFace 128-d embedding → cosine vs multiple templates; models behind traits.
- [ ] Calibration harness (`kenali-bench`): collect ≥ 12 templates across ≥ 2 lighting conditions; derive threshold from FAR/FRR data, not the 0.363 OpenCV default.
- [ ] FR-01 enrolment wizard: prerequisite badges, masked password validated by `LogonUser`, guided capture with live preview, seal to TPM, self-test, summary; cancel at every step.
- [ ] FR-18 preview: service streams 320×240 RGB over the preview pipe only during enrolment/calibration; RAM-only and zeroized in service **and** UI; no encoder, no disk.
- [ ] Templates screen: list, add (mini capture), delete with confirm.
- [ ] Settings screen: scope toggles, attention angles, cooldowns, read-only PAD thresholds.
- [ ] Admin pipe: all FR-09 typed commands; diagnostics ZIP returned as bytes; no file paths accepted by the service.
- [ ] NFR-01/NFR-02 latency & compute measurement on the reference laptop (see `test_plan.md`).
- [ ] Log hygiene re-verified for the new UI paths (preview, enrol password buffer).

**Wave gating:** wave 3 (service + vision) → wave 4 (UI + CLI) for the wizard.

---

## 5. Phase 3 — Liveness, attention, PAD evaluation

**Intent (PRD §15):** add the presentation-attack layers and prove them with the FAR/FRR and PAD reports.

- [ ] MiniFASNet V2 + V1SE passive PAD behind traits; fused real-probability; camera crop scales verified.
- [ ] Temporal decision window; attention gating (eyes open, frontal within default ±20°).
- [ ] Multi-face rejection (comparable-size second face), virtual-camera rejection.
- [ ] Active challenge escalation on elevated risk (blink / head turn); screen-flash R&D as Could.
- [ ] Recognition report: zero-effort FAR ≤ 0.001 % at ≥ 2,237 impostor identities; TAR ≥ 95 % / FRR ≤ 5 %; committed under `docs/reports/`.
- [ ] PAD report: APCER ≤ 1 % per species with ≥ 300 attempts/species (rule of three); BPCER ≤ 5 %; committed under `docs/reports/`.
- [ ] Attack protocol executed: printed photo, phone-screen replay, tablet/monitor replay (`test_plan.md`).
- [ ] No template auto-update from face-only unlocks (SEC-15).

**Wave gating:** wave 3 (service + vision); PAD thresholds consumed by wave 4 UI read-only display.

---

## 6. Phase 4 — Hardening & performance (service **and** UI)

**Intent (PRD §15):** meet every NFR, finish the security boundaries, and exercise the fuzz/soak/acceptance gates before daily use.

- [ ] NFR-01…NFR-10 measured and met, or each miss has an accepted written exception.
- [ ] Fuzz campaigns ≥ 1 h each on auth/admin/preview decoders and frame parsers with 0 crashes (`test_plan.md`).
- [ ] Security: rate limits/cooldowns, one-submit rule, stale-credential auto-disable, camera pinning, allowed-SID, first-sign-in-after-boot password rule.
- [ ] UI hardening: no secret in UI state, preview buffers zeroized on every exit path, signed binary, loads nothing by relative path, keyboard navigable, per-monitor DPI.
- [ ] Log hygiene final pass for service, CP and UI (no secret/biometric in logs or errors; scores never logged finer than 0.05).
- [ ] 1,000-cycle lock/unlock soak on VM with 0 CP crashes (NFR-06).
- [ ] Hardware acceptance matrix passed: sleep, hibernate, lid, Fast Startup, camera contention, first unlock after Windows Update (`test_plan.md` §5).
- [ ] 2-week secondary-account soak started (completed in P5).
- [ ] `cargo-deny` / `cargo-audit` clean; SBOM generated; all binaries signed (service, CP, UI, CLI).

**Wave gating:** wave 5 (hardening).

---

## 7. Phase 5 — Daily-driver rollout & soak

**Intent (PRD §15):** promote to the main account only after the secondary-account soak, with rehearsed recovery and a recorded UI walkthrough.

- [ ] End-to-end **UI walkthrough recorded**: enrol via wizard, change a setting, add/delete a template, export diagnostics, refresh credentials, disable/enable — no terminal.
- [ ] 2-week secondary-account soak completed with 0 lock-outs.
- [ ] Recovery drills R-A…R-E each executed once successfully (`dev_safety_runbook.md`).
- [ ] Main-account enrolment only after the soak passes; password/PIN fallback verified after every install/upgrade.
- [ ] v1.0 Definition of Done (`context_pack.md` §11 / PRD §13) fully checked.

**Wave gating:** wave 5 exit.

---

## 8. Definition of Done (every task — `context_pack.md` §11)

- [ ] Tests added or updated (unit/property; integration when it crosses a boundary).
- [ ] `cargo xtask check` passes.
- [ ] No new `unsafe` without `// SAFETY:`; no `unsafe` outside the three allowed crates (`kenali-winapi`, `kenali-camera`, `kenali-cp`) — UI included in the forbid check.
- [ ] No secret/biometric reaches logs or errors (see `test_plan.md` §7) — service, CP **and** UI.
- [ ] Hot-path change → benchmark before/after recorded.
- [ ] UI change → walkthrough performed in the UI; no new secret in UI state; preview buffers zeroized.
- [ ] Touches install, registry or recovery → `dev_safety_runbook.md` updated and the drill re-run in a VM.
- [ ] Touches the attack surface → `threat_model.md` updated.
- [ ] Architectural change → ADR added to `architecture.md`.

## 9. VM-first test ladder

Never register the CP on the main account first. Order is mandatory; follow `dev_safety_runbook.md` before each step.

| Order | Environment | What is verified | Gate to advance |
|---|---|---|---|
| 1 | **Windows VM** (snapshot before install) | CP hosting, camera capture, TPM wrap, scenario detection, kill switch, Safe Mode/WinRE recovery, 1,000-cycle soak, fuzz, all spikes. `xtask install --vm` requires `KENALI_ALLOW_INSTALL=1`. | All P0 spikes go/pivot decided; 0 lock-outs; recovery drills R-A…R-E pass. |
| 2 | **Secondary host account** (separate local admin) | Allowed-SID guard (main account unaffected), 2-week soak, hardware acceptance matrix, first-unlock-after-Update. Main account must remain usable throughout. | 2-week soak with 0 lock-outs; main account never sees the tile. |
| 3 | **Main account** (daily driver) | Final enrolment, UI-first operation, refresh-credential after a password change, fast fallback. | v1.0 Definition of Done fully checked; owner sign-off. |

> The development host is Linux: **no install/registration command is ever run here.** The agent produces `xtask`/VM scripts and step-by-step manual instructions; the owner executes them in the VM. See `test_plan.md` §"Requires the owner's Windows VM".
