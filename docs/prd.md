# PRD — Kenali: Face Unlock for the Windows Lock Screen (Rust, UI-first)

| Field | Value |
|---|---|
| Version | 0.2 — operator workflows moved from CLI to a native Rust GUI |
| Date | 2026-11-16 |
| Working name | **Kenali** (Indonesian: "to recognize"). Rename freely; crates use the `kenali-` prefix. |
| Status | Draft. Items marked **[CONFIRM]** are assumptions inferred from the interview or from the v0.2 "everything UI-based" request. |
| Companion files | `context_pack.md` · `architecture.md` · `threat_model.md` · `roadmap.md` · `test_plan.md` · `dev_safety_runbook.md` · `references.md` |

Reference tags like [R6] point to `references.md`.

> **Rev 0.2 (2026-11-16) — what changed and why.** The owner asked for *everything* to be UI-based while staying in Rust. Every operator-facing workflow — enrol, templates, settings, status, diagnostics, enable/disable, refresh credentials — now happens in **`kenali-ui.exe`**, a single-window native Rust GUI; the terminal is no longer needed for daily operation. The logon-side UI (the Credential Provider tile) was already a UI and is unchanged. A minimal **recovery CLI** is retained as an internal fallback for headless/VM/recovery use **[CONFIRM — §17 Q7]**. Developer/build tooling (`cargo xtask`) stays on the command line because it is build tooling, not an operator surface.

---

## 1. Summary

Kenali lets the owner of a Windows 11 laptop unlock the lock screen by looking at the built-in webcam, without typing a password. It is written in Rust end to end.

A very small **Credential Provider DLL** (loaded by `LogonUI.exe`) shows a "Face unlock" tile and asks a resident **SYSTEM service** to run a local pipeline: detect → quality gate → align → embed → match → liveness/attention → policy. On success the service releases a **TPM-protected** copy of the account password over a locked-down named pipe, and the DLL hands it to Windows, which performs a normal logon. Nothing leaves the machine.

All management happens in **`kenali-ui.exe`**, a native Rust GUI (no terminal required): an **enrolment wizard** with guided capture and a live camera preview, a **dashboard** (service, camera, TPM/BitLocker, policy state), plus **template**, **settings**, **security** (refresh credentials, disable) and **diagnostics** screens. The UI talks to the service only through the typed admin pipe and never sees any secret.

Priorities, in order: **1) security, 2) never locking the owner out, 3) speed, 4) footprint.**

**Honest limit:** a single RGB camera cannot match an IR/depth system such as Windows Hello or Face ID. Microsoft's own face stack requires a near-infrared camera and uses IR partly because IR does not appear in photos or on LCD screens [R1]. Kenali is therefore positioned as a *convenience factor with strong compensating controls* (layered anti-spoofing, strict policy, hardware-bound secrets, fail-safe fallback), not as an equal of Windows Hello. See §4 and `threat_model.md`.

---

## 2. Confirmed intent (restated from the interview)

You answered "as ur guess" to both interview questions, so both guesses are treated as accepted.

| | |
|---|---|
| **Outcome** | Open the laptop, look at the screen, and be signed back in within ~1–2 s. No typing. |
| **User** | You — sole owner of one personal Windows laptop. |
| **Why (inferred) [CONFIRM]** | The built-in webcam has no IR sensor, so native Windows Hello face sign-in is unavailable; you want to build a Face-ID-like unlock yourself, in Rust, with maximum security, speed and optimisation. |
| **Success** | Reliable and fast for you; resists casual spoofing (photo, phone/tablet replay); never locks you out; private (100% local). |
| **Binding constraints** | Rust. Windows lock screen. RGB webcam only. "Best possible" security/speed/optimisation within those. **Rev 0.2: every operator workflow through a native UI.** |
| **Out of scope** | See §5. Notably: Windows Hello integration, IR/depth cameras, multi-user, domain accounts, pre-boot unlock, distribution/commercial release. |

---

## 3. Assumptions to confirm

| # | Assumption | If wrong → |
|---|---|---|
| A1 | Windows 11 x64, current feature release; Windows 10 22H2 best-effort. | Re-check Credential Provider behaviour per OS (scenario reporting differs, see §14 R5). |
| A2 | Built-in camera is RGB only (no IR). Quick check: Settings → Accounts → Sign-in options → "Facial recognition (Windows Hello)" is unavailable. | If Windows Hello face works on this laptop, **use Windows Hello instead** — it is stronger. This project then becomes a learning exercise only. |
| A3 | Personal, single-user, single-laptop use. | Multi-user enrolment changes threshold calibration (Microsoft raises the threshold when several users are enrolled [R1]). |
| A4 | The account signs in with a **password** (local account or Microsoft account). A Credential Provider must submit a real password; PINs cannot be submitted [R17]. | Passwordless-only setups need a compatibility spike (P0-S6). |
| A5 | BitLocker / Device Encryption is on. | Without it, offline attackers can read the disk; TPM-wrapping still helps but the model weakens. Enable it. |
| A6 | TPM 2.0 and Secure Boot present. | Fall back to software key storage with an explicit `--allow-weak-keystore` flag and a warning. |
| A7 | Reference laptop has ≥ 4 CPU cores. A comparable project reports noticeable latency below 4 cores with CPU inference [R15]. | Re-baseline latency targets (NFR-01/02). |
| A8 | You have admin rights and can create a second local admin account. | Required by the safety runbook. |
| A9 | No distribution. Personal use only. | If you ever distribute, replace any non-permissive model weights (§12). |
| A10 | The webcam is an internal UVC/MIPI camera exposed through Media Foundation. | Record device IDs in P0; adjust camera pinning (SEC-16). |
| A11 | The management GUI uses **egui/eframe** (pure Rust, immediate mode), pinned at bootstrap. | If wrong → swap the front end for native Win32 or iced (§16); the UI is isolated behind `kenali-ipc`, so the swap is contained to one crate. |
| A12 | The management UI always runs elevated (one UAC prompt at launch); unelevated read-only status is not required. | If wrong → add a second unelevated status surface; more attack surface and more code for little gain. |
| A13 | A **live camera preview** during enrolment is wanted. | If wrong → drop FR-18; the wizard falls back to text guidance only. |

---

## 4. Background and verified facts

- **Windows Hello face needs IR.** Microsoft documents face authentication as using "a camera specially configured for near infrared (IR) imaging", head orientation within ±15°, FAR < 0.001 % (1/100,000), TPR > 95 % and FNR < 5 % for a single enrolled user, and average authentication under 2 s [R1][R2]. Since a 2025 update, Hello also requires a colour frame in addition to IR, reportedly to counter an IR-spoofing technique [R4].
- **Third-party route = Credential Provider (CP).** A CP is a set of COM objects loaded by `LogonUI.exe`; it is the supported extension point for custom sign-in methods. `windows-rs` exposes `ICredentialProvider`, `ICredentialProviderCredential` and their `_Impl` traits [R8]; an open-source Rust sample exists [R9], and a community answer explains how to export the COM entry points from a Rust DLL [R10].
- **Prior art validates the architecture and its limits.** The closest project (C++ CP + Python LocalSystem service, ONNX on CPU) runs recognition in the service, keeps the password in an LSA secret, falls back to password after 3 failed attempts, and states plainly that RGB security is far below Windows Hello [R15]. Others use the same CP approach [R16][R17]. One simple design ("any process that writes to a pipe can unlock") is a cautionary example of what **not** to do [R16].
- **Camera capture from the service works in at least one real deployment**, using DirectShow retries and a confirmed frame read; cold-boot and resume can delay camera readiness [R15]. A community report shows WinRT `MediaCapture` failing with access-denied inside a service [R35], so Kenali uses lower-level Media Foundation instead and treats this as spike P0-S2.
- **A faulty CP can lock you out.** Microsoft's guidance: a provider can be disabled with `Disabled=1` under its CLSID key; Safe Mode loads only the built-in password provider [R12]. Recovery options are in `dev_safety_runbook.md` [R6][R13].
- **Model licences matter.** YuNet (MIT) and SFace (Apache-2.0) from OpenCV Zoo are permissive [R22]. InsightFace code is MIT but its pretrained recognition models are non-commercial and require a separate licence [R25]. MiniFASNet (Silent-Face-Anti-Spoofing) is Apache-2.0 [R26], but passive RGB liveness is camera- and scene-dependent [R27].
- **Inference runtime.** `ort` (ONNX Runtime bindings) is at `2.0.0-rc.13` in the sources seen; DirectML is in "sustained engineering" with Windows ML as the forward path [R18][R20][R21]. CPU-first is the robust choice at a logon screen.
- **GUI in Rust (Rev 0.2).** egui/eframe is a mature immediate-mode GUI in pure Rust that renders in-RAM images (the camera preview) easily and needs no web stack; the exact version must be pinned at bootstrap (**VERIFY** current stable). Native Win32 via the `windows` crate is the smallest-dependency alternative (§16). No async runtime is introduced: the UI runs on the egui/winit event loop, consistent with the "no async runtime" rule.

---

## 5. Goals, non-goals, out of scope

**Goals**

- G1 Hands-free unlock of an existing session at the lock screen, typically ≤ 1.5 s warm.
- G2 Strongest feasible security for an RGB-only camera: layered presentation-attack detection, strict policy, hardware-bound secrets.
- G3 Zero lock-out risk: password/PIN always available, kill switch, rehearsed recovery.
- G4 100 % local and private.
- G5 Engineering quality: memory-safe, tested, benchmarked, reproducible, supply-chain-aware.
- G6 Everything measurable: FAR/FRR, APCER/BPCER and latency from a repeatable harness.
- G7 **UI-first operation (Rev 0.2):** every operator task is achievable from the GUI without a terminal; the recovery CLI exists only as an internal fallback **[CONFIRM]**.

**Non-goals** — matching Windows Hello/Face ID assurance; IR/depth support; hiding or replacing the default password/PIN providers.

**Out of scope for v1**

Windows Biometric Framework plug-in; Windows Hello integration; domain/Entra accounts and smart-card flows; RDP/remote sessions; UAC and CredUI prompts; password-change screen; pre-boot (BitLocker PIN) unlock; multi-user; automatic updates; cloud anything; commercial distribution; a web-based UI stack (Tauri/wry — see §16).

---

## 6. Users, scenarios and flows

**Actors:** *Owner* (enrolled user), *Bystander* (someone near the laptop), *Attacker* (see `threat_model.md`: ADV-1 … ADV-7).

| Flow | Steps |
|---|---|
| **F1 Enrol** (elevated GUI wizard) | Launch `kenali-ui.exe` (UAC) → **Enrol wizard**: prerequisite checks (TPM, BitLocker, camera, service) with green/red badges → password entry in a masked field → password validated by the service with `LogonUser` (wrong password = inline error, no retry limit but never sealed) → guided capture (straight, small left/right/up/down turns, glasses on/off if relevant) with live preview and liveness check → ≥ 12 quality-gated templates across ≥ 2 lighting conditions → secret sealed to TPM → self-test + summary screen. Cancel is available at every step. |
| **F2 Unlock** | Lid opens / display wakes → service pre-arms camera → lock screen visible → tile "Looking for you…" → pipeline → policy → submit → desktop. |
| **F3 Fallback** | Any failure, timeout, service down, or policy block → tile says why in generic terms; password/PIN tile works immediately. Never a delay. |
| **F4 Password changed** | Next face unlock returns a wrong-password status → face unlock auto-disables (no retries) → owner signs in with the password → the UI shows a "Face unlock paused — credentials changed" banner → the **Refresh credentials** wizard re-validates the password and reseals the secret. |
| **F5 Pause / uninstall** | The UI's **Security** screen (Disable/Enable), the registry kill switch, or `cargo xtask uninstall` (developer tooling). |
| **F6 Recovery** | Safe Mode or WinRE steps in `dev_safety_runbook.md`; the recovery CLI (if retained, §17 Q7) works where the GUI cannot. |

---

## 7. Functional requirements

Priority: **M**ust / **S**hould / **C**ould.

| ID | Requirement | P | Acceptance criteria |
|---|---|---|---|
| FR-01 | **Enrolment.** Elevated **UI wizard** captures ≥ 12 quality-gated templates across ≥ 2 lighting conditions, rejects spoofed enrolment, validates the password before sealing. | M | Template count ≥ 12; wrong password rejected with an inline error and never sealed; no frame written to disk; completes ≤ 90 s. |
| FR-02 | **Lock-screen unlock** (`CPUS_UNLOCK_WORKSTATION`) hands-free for the enrolled SID only. | M | Meets NFR-01 and NFR-04 on the reference laptop. Other users never see the tile. |
| FR-03 | **Always-available fallback.** Password/PIN providers are never filtered, hidden or delayed. | M | Kill the service mid-attempt → password works instantly. CP shows "Face unlock unavailable" ≤ 300 ms. |
| FR-04 | **Policy engine** in the service (table in §10). | M | Each rule has a unit test and an integration test. |
| FR-05 | **Scope switches** per scenario (unlock on by default, sign-in off by default), editable in the UI's Settings screen. | S | Setting `scope.logon=on` from the UI takes effect at next session creation. |
| FR-06 | **Presentation-attack detection:** passive CNN + temporal checks + camera pinning (M); active challenge on elevated risk (S); screen-flash challenge R&D (C). | M/S/C | Meets NFR-05 on the attack protocol in `test_plan.md`. |
| FR-07 | **Attention gating:** eyes open and face frontal within configurable yaw/pitch (default ±20°; Microsoft uses ±15° [R1]). | M | Eyes closed or head turned away does not unlock. |
| FR-08 | **Camera lifecycle:** armed on wake/lock-visible, stopped ≤ 8 s or on result; never captures outside armed windows. | M | Event Log shows capture windows; camera LED off when idle. |
| FR-09 | **Management coverage, all via the UI:** every operator action is a typed UI action — enrol, add-template, delete, status, test (dry-run, never unlocks), calibrate, bench, refresh-credential, config get/set, export-diagnostics, enable, disable. | M | The admin pipe accepts only typed commands; the service never accepts file paths (the diagnostics ZIP is returned over the pipe in chunks and the UI saves it to the user-chosen location). |
| FR-10 | **Kill switch:** `HKLM\SOFTWARE\Kenali\Disabled=1` makes the CP return `E_NOTIMPL` at next session creation. | M | Verified in VM, host test account, and from WinRE. The UI shows the kill-switch state (read-only). |
| FR-11 | **Stale-credential handling:** wrong-password result auto-disables face unlock and never retries. | M | No second submit after `STATUS_WRONG_PASSWORD`; account is never locked out by Kenali. |
| FR-12 | **Observability:** Windows Event Log events + rotating structured logs, no secrets/biometrics — including the UI process. | M | Log-hygiene test (test_plan §7) passes for service, CP and UI. |
| FR-13 | **Multi-face rejection** when a second face of comparable size is present (default on). | S | Small background faces are ignored; comparable-size second face rejects the attempt. |
| FR-14 | **Diagnostics export** (redacted ZIP), saved from the UI. | S | Contains no templates, embeddings, passwords or frames; the service returns bytes over the pipe; the UI writes the file. |
| FR-15 | **Install / uninstall** via `cargo xtask` with versioned install path and rollback (developer tooling; the UI exposes Disable/Enable but not uninstall). | M | Install, upgrade, rollback and uninstall all verified in a VM. |
| FR-16 | **Allowed-SID guard:** tile shown only for the enrolled SID (also the safe way to test on a secondary account). | M | Main account unaffected while a test account is enrolled. The UI shows the enrolled SID. |
| FR-17 | **Management GUI** (`kenali-ui.exe`): single-window native Rust GUI. Screens: Dashboard, Enrol wizard, Templates, Settings, Security, Diagnostics, Test/Bench (§11). Elevated via UAC at launch; single instance; talks only over the admin pipe. | M | Every FR-09 action is reachable and completable in the UI without a terminal; the UI's absence or breakage never affects unlock, fallback or recovery. |
| FR-18 | **Enrolment live preview:** the service streams downscaled preview frames (default 320×240 RGB) to the UI only during enrolment/calibration windows; the wizard renders them inline with pose guidance. | S | Frames exist in RAM only on both ends and are zeroized on every exit path; nothing persisted or logged; the preview channel is closed outside enrolment windows. |
| FR-19 | **UI secret hygiene:** the UI never receives the sealed secret, stored password or embeddings; password field buffers are cleared immediately after submit; no secret in UI logs, state or error messages; the UI has no access to the auth pipe. | M | Log-hygiene test extended to the UI; code review confirms no auth-pipe types are linked into the UI crate. |
| FR-20 | **Recovery CLI retained [CONFIRM]:** a minimal internal `kenali` CLI (status, export-diagnostics, disable, refresh-credential) for headless/VM/recovery use. | S | Not required for any daily operation; documented as an internal tool, not an operator surface. |

---

## 8. Non-functional requirements

Latency, compute and footprint numbers are **initial targets** to be re-baselined in Phase 0 on the reference laptop.

| ID | Area | Target |
|---|---|---|
| NFR-01 | **Latency** (lock screen visible → credential submitted) | Warm (camera pre-armed by wake event): p50 ≤ 1.0 s, p95 ≤ 1.8 s. Cold (camera must open): p50 ≤ 2.0 s, p95 ≤ 3.5 s. Hard timeout 6 s → password UI. Reference: Microsoft documents Hello face at < 2 s average [R1]. |
| NFR-02 | **Compute** | Detect + align + embed + passive PAD ≤ 40 ms per qualified frame on ≥ 4 cores. Service warm-up inference at start so the first real frame is not slow. |
| NFR-03 | **Footprint** | Service idle RSS ≤ 150 MB, ~0 % CPU idle. CP DLL ≤ 1 MB, ≤ 5 MB RSS, no ML. UI binary ≤ 25 MB; the UI runs only when opened — no background process, no tray by default [CONFIRM]. Install ≤ 100 MB with INT8 models and the UI. |
| NFR-04 | **Recognition accuracy** | Zero-effort FAR ≤ 0.001 % (1:100,000) at the operating threshold — the bar Microsoft states for Hello face [R1][R2]. TAR ≥ 95 % and FRR ≤ 5 % for one enrolled user in normal indoor lighting. Statistical power: Microsoft's formula implies ≈ 2,237 unique samples to support a 0.001 % FAR claim at 96 % confidence [R2]. |
| NFR-05 | **Presentation attacks** | APCER ≤ 1 % per attack species (printed photo, phone-screen replay, tablet/monitor replay) with ≥ 300 attempts per species (zero failures in 300 trials bounds APCER ≈ 1 % at 95 % confidence — rule of three). BPCER ≤ 5 %. |
| NFR-06 | **Reliability** | Service crash/hang never blocks password logon. CP connect timeout ≤ 200 ms; attempt timeout ≤ 6 s. 0 CP crashes in a 1,000-cycle lock/unlock soak (VM). |
| NFR-07 | **Compatibility** | Windows 11 x64 (current + previous feature release). Local and Microsoft accounts with password. |
| NFR-08 | **Privacy** | 0 outbound connections from service, CP or UI; no frames or images on disk (including the UI's preview path); no telemetry. |
| NFR-09 | **Build & supply chain** | Pinned toolchain, `--locked` builds, `cargo-deny`/`cargo-audit` clean, `unsafe` confined to named FFI crates (GUI crates included in the review), all release binaries signed, models hash-pinned. |
| NFR-10 | **Management UI** | Cold start ≤ 2 s to an interactive Dashboard. Live preview ≥ 15 fps with ≤ 5 % of one core for downscale+send (service-side; measured in P2). Every long action shows progress and a cancel; per-monitor DPI aware; keyboard navigable. |

---

## 9. Security requirements

Full rationale in `threat_model.md` (TM = threat IDs).

| ID | Requirement | Addresses |
|---|---|---|
| SEC-01 | The password is never stored or logged in plaintext. At rest: AEAD ciphertext with a DEK wrapped by a non-exportable TPM key. | TM-07 |
| SEC-02 | Only embeddings are stored (AEAD-encrypted). Raw frames live in RAM only and are zeroized — in the service **and in the UI's preview path**. | TM-05, TM-15 |
| SEC-03 | Auth pipe: SYSTEM-only DACL, first-instance flag, remote clients rejected, **peer verified in both directions**, nonce-bound one-shot secret release. The UI has no access to this pipe at all. | TM-08, TM-14 |
| SEC-04 | Admin pipe: Administrators only (elevated token), typed commands, enrolment/refresh need elevation. Clients are `kenali-ui.exe` and the recovery CLI; the service verifies the client's image path is a signed Kenali binary under `C:\Program Files\Kenali\<version>\`. | TM-22 |
| SEC-05 | Binaries and models under `C:\Program Files\Kenali\` (admin-write only), absolute-path loading, models hash-pinned, binaries Authenticode-signed (self-signed acceptable for personal use) — **including `kenali-ui.exe`**. | TM-09, TM-16, TM-23 |
| SEC-06 | The CP is minimal and memory-safe: no ML, no network, `catch_unwind` on every COM entry point, fails open to password. | TM-10 |
| SEC-07 | Policy is enforced in the service. The registry holds only a *disable* switch, which can tighten but never loosen security. The UI can change settings only as typed admin-pipe commands that the service validates; it never writes Kenali's HKLM state. | TM-13 |
| SEC-08 | PAD: passive + temporal + attention + virtual-camera rejection; challenge escalation on risk. | TM-01, TM-02, TM-18 |
| SEC-09 | Rate limiting and cooldowns; scores never shown on screen (tiles **or the UI**); generic failure messages. | TM-11 |
| SEC-10 | Never retry after a wrong-password or locked-out result. | TM-12 |
| SEC-11 | No networking crates; outbound firewall block for the service executable (and the UI binary). | TM-15 |
| SEC-12 | Memory hygiene: `zeroize`, no `String` for secrets, minimal copies, exclude secret buffers from crash dumps where feasible. GUI text fields are String-based — accepted exception: enrol/refresh password buffers are cleared immediately after submit (§14 R14). | TM-07 |
| SEC-13 | Build hardening: static CRT, CFG, CET-compat, ASLR/DEP; `forbid(unsafe_code)` outside FFI crates (UI included — its Win32 needs go through `kenali-winapi`); fuzzing for every parser; `cargo-deny`/`audit`. | TM-10, TM-16 |
| SEC-14 | Audit trail in Event Log without sensitive data. | TM-11 |
| SEC-15 | Enrolment integrity: elevation + password validation + liveness; **no template auto-update from face-only unlocks**. | TM-06 |
| SEC-16 | Camera pinning: only the enrolled physical camera; a change disables face unlock until re-enrolled. | TM-02, TM-03 |
| SEC-17 | Safe-by-default scenarios: only `CPUS_UNLOCK_WORKSTATION` on; `CPUS_LOGON` opt-in; never CredUI, change-password, PLAP or RDP. | TM-24 |
| SEC-18 | First sign-in after boot/restart always requires the password. | TM-02, TM-04 |
| SEC-19 | **UI attack surface (Rev 0.2):** the UI never touches the auth pipe, templates, embeddings or the sealed secret; it holds a plaintext password only transiently in the enrol/refresh wizards (cleared after submit); preview frames are RAM-only; all mutations are typed commands validated by the service's policy engine; the UI binary is signed and loads nothing by relative path. | TM-22, TM-23 |
| SEC-20 | **The UI is optional at logon:** unlock, password fallback, the kill switch and recovery never depend on the UI being installed or running. | R1 |

---

## 10. Default policy

Evaluated by the service. All values configurable **via the admin pipe from the UI's Settings screen**; defaults favour security. The service validates every change against the schema; the UI can only *tighten* what the schema allows.

| Condition | Default behaviour |
|---|---|
| Boot / restart / first sign-in | Password only (SEC-18). |
| Sign-out → sign-in (`CPUS_LOGON`, no live session) | Off; opt-in. |
| Lock-screen unlock, enrolled SID, live session | Face allowed. |
| ≥ 24 h since last knowledge-factor unlock (password/PIN) | Password required. The service infers this from session-unlock events with no preceding Kenali success. |
| 3 failed face attempts in one lock | Tile falls back to password for this lock. |
| 5 consecutive failures within 10 min | Face disabled for 10 min. |
| 3 PAD rejections within 10 min | Face disabled for 30 min + Event Log warning. |
| Wrong-password result from Windows | Face disabled until `refresh-credential` (UI wizard). |
| Camera device differs from the pinned device | Face disabled until re-enrolled. |
| Comparable-size second face in frame | Attempt rejected. |
| RDP / remote session | Not offered. |
| Elevated-risk signals (low PAD margin, lighting anomalies, recent failures, long idle) | Escalate to an active challenge (blink / head turn). |

---

## 11. UX spec

### 11.1 Lock-screen tile states (generic wording; no scores shown)

| State | Text |
|---|---|
| Armed | "Face unlock ready" |
| Searching | "Looking for you…" |
| Guidance | "Look at the screen" · "Move a little closer" · "Too dark" |
| Challenge | "Blink twice" · "Turn your head left" |
| Success | "Welcome" |
| Failed | "Not recognised — 2 tries left" |
| Fallback | "Use your password or PIN" |
| Disabled | "Face unlock paused" |
| Service down | "Face unlock unavailable" |

### 11.2 Management UI (`kenali-ui.exe`, elevated)

Single window, left navigation, English strings in v1 via a single string table (localisation, e.g. Indonesian, later) **[CONFIRM — §17 Q9]**.

| Screen | Contents / actions |
|---|---|
| **Dashboard** | Service state, camera status, TPM/BitLocker badges, face-unlock on/off, policy state summary (e.g. "Paused for 10 min: 5 failures", "Paused — credentials changed"), recent redacted Event Log entries, kill-switch status (read-only), enrolled SID. |
| **Enrol wizard** | Steps: prerequisite checks (green/red badges) → password (masked field, validated by the service) → guided capture (live preview [FR-18], pose guidance "straight / left / right / up / down", per-pose progress "7/12", lighting hints) → seal to TPM → self-test result → summary (template count, conditions, calibration date). Cancel at every step; cancel wipes preview buffers. |
| **Templates** | List (capture date, lighting condition, glasses flag), delete with confirm, add-template (mini capture flow reusing the wizard's capture step). |
| **Settings** | Scope toggles (unlock on/off, logon on/off), attention yaw/pitch angle, failure cooldowns, PAD thresholds shown **read-only** with calibration date, pinned camera device info. All changes sent as typed config keys; schema-validated by the service. |
| **Security** | Refresh credentials wizard (after a password change), Disable/Enable face unlock, kill-switch status + how to use it, allowed-SID guard status, link to the runbook. |
| **Diagnostics** | Export redacted ZIP (UI asks for the save location; service returns bytes; FR-14), Event Log viewer (filtered), log-file location note, service PID/version. |
| **Test / Bench** | Dry-run recognition test (never unlocks; states so explicitly on screen), latency bench summary, calibration report with threshold + date. |

**UI conventions:** generic failure wording (same vocabulary as the tiles), similarity scores are never rendered anywhere, long actions show progress + cancel, per-monitor DPI aware, dark/light follows the Windows theme, keyboard-accessible. Future (C): a system-tray quick status with Disable **[CONFIRM — §17 Q10]**.

---

## 12. Technical approach (summary — details in `architecture.md`)

- **Three programs, one DLL:** `kenali-cp.dll` (thin, in LogonUI) ↔ auth pipe ↔ `kenali-svc.exe` (SYSTEM service) ← admin pipe (+ a short-lived preview pipe during enrolment/calibration) ← `kenali-ui.exe` (management GUI, egui); minimal `kenali.exe` recovery CLI **[CONFIRM]**.
- **Vision (CPU-first, ONNX Runtime via `ort`):** YuNet (detect, MIT) → 5-point alignment → SFace (128-d embedding, Apache-2.0) → cosine match vs multiple templates → MiniFASNet ×2 (passive PAD, Apache-2.0) → temporal decision window. All behind traits so models can be swapped after measurement. Do **not** ship OpenCV's default SFace threshold (cosine 0.363, tuned for ~99.6 % LFW accuracy, not 1e-5 FAR [R23]); calibrate on data.
- **Secrets:** hybrid encryption — XChaCha20-Poly1305 data key wrapped by a non-exportable TPM RSA key (CNG, `MS_PLATFORM_CRYPTO_PROVIDER`) [R29][R30]; software-key fallback is explicit and loud.
- **Management GUI:** egui/eframe (pure Rust, immediate mode; pin the exact version at bootstrap — **VERIFY**), single window, renders preview frames straight from RAM, no web stack, no async runtime; Win32 needs (single-instance mutex, elevation relaunch, DPI awareness) live in `kenali-winapi`; the UI crate itself is `forbid(unsafe_code)`.
- **Speed:** resident service with warm models, wake-event camera pre-arm, INT8 models, no per-frame allocation in the hot path, early exit.
- **Safety:** no CP filter, kill switch, allowed-SID guard, bounded SCM recovery, VM-first test ladder; the UI is optional at logon (SEC-20), so a broken UI can never contribute to a lock-out.

---

## 13. Success metrics and v1.0 Definition of Done

v1.0 ships (for personal daily use) only when **all** are true:

- [ ] P0 spike report shows camera capture, CP hosting, TPM wrap and scenario detection work on the reference laptop.
- [ ] NFR-01…NFR-10 measured and met, or each miss has an accepted written exception.
- [ ] FAR/FRR report (≥ 2,237 impostor identities) and PAD report (≥ 300 attempts/species) committed under `docs/reports/`.
- [ ] 1,000-cycle lock/unlock soak on VM and 2-week soak on a secondary host account with 0 lock-outs.
- [ ] End-to-end **UI walkthrough recorded**: enrol via the wizard, change a setting, add/delete a template, export diagnostics, refresh credentials, disable/enable — all without opening a terminal.
- [ ] Recovery drills R-A…R-E in `dev_safety_runbook.md` each executed once successfully.
- [ ] Hardware acceptance matrix (`test_plan.md` §5) passed: sleep, hibernate, lid close, Fast Startup, camera contention, first unlock after Windows Update.
- [ ] Fuzz campaigns ≥ 1 h each on IPC and frame parsers with 0 crashes.
- [ ] `cargo deny` and `cargo audit` clean; SBOM generated; all binaries signed (service, CP, UI, CLI).

---

## 14. Risks

| ID | Risk | L | I | Mitigation | Phase |
|---|---|---|---|---|---|
| R1 | Lock-out from a CP bug | M | H | Thin CP, `catch_unwind`, no filter, kill switch, allowed-SID guard, VM-first, runbook drills, second admin account. | All |
| R2 | Camera unavailable from Session 0 at the lock screen | M | H | Spike P0-S2; Media Foundation not WinRT; retries with first-frame confirmation; helper-process fallback. | P0 |
| R3 | RGB spoofing limits | H | M | Layered PAD, attention, policy (no cold-boot face, 24 h rule), honest docs. | P2–P3 |
| R4 | Camera cold-start dominates latency | H | M | Wake-event pre-arm, discard AE-convergence frames, measure per-camera. | P2, P4 |
| R5 | Windows reports unlock as `CPUS_LOGON` (observed in prior art [R15]) | M | M | Disambiguate via WTS session state; test both scenarios. | P0 |
| R6 | Undocumented CP interfaces change (`RequestSerialization`, `SetDisplayState`) [R6] | M | L | Documented path first (`CredentialsChanged` + autologon); undocumented ones optional with feature detection. | P1 |
| R7 | CP does not load at first logon after reboot (auto sign-in feature) [R14] | M | L | Consistent with policy (password after boot); document workaround. | P1 |
| R8 | Wrong-password loops trip account lockout policy | L | H | SEC-10; one submit per attempt; stale-credential state. | P1 |
| R9 | Smart App Control / WDAC blocks unsigned binaries | M | M | Sign everything; check state in P0. | P0 |
| R10 | Microsoft-account "Hello-only sign-in" setting hides or blocks the password path | M | M | Compatibility test with setting on/off. | P0 |
| R11 | Model licence problems | L | M | Permissive defaults; weights not committed; licence table in `context_pack.md`. | P2 |
| R12 | `ort` is still a release candidate | M | L | Pin exact version; `tract` as benchmark fallback; thin wrapper crate. | P1 |
| R13 | GUI dependency tree is large (winit + glow/wgpu) and supply-chain-visible | L | M | Pin exact versions; `cargo-deny`/`cargo-geiger` scoped to `kenali-ui`; keep the UI isolated behind `kenali-ipc` so a swap is contained; software-GL fallback if GPU issues appear in VMs. | P1 |
| R14 | The UI holds the account password briefly in the enrol/refresh wizards | M | M | Cleared immediately after submit; never logged, cloned or persisted; elevated + signed process; accepted residual risk — Windows itself holds the password in memory during logon; documented in `threat_model.md`. | P2 |

---

## 15. Milestones (see `roadmap.md`)

P0 Feasibility spikes → P1 Secure skeleton (service + CP + UI shell with Dashboard) → P2 Vision v1 + calibration + enrol wizard with live preview → P3 Liveness, attention, PAD evaluation → P4 Hardening and performance (service **and UI**) → P5 Daily-driver rollout and soak. Indicative total: 11–16 part-time weeks (slightly up from v0.1's 10–14 for the UI work).

---

## 16. Alternatives considered

| Option | Verdict | Why |
|---|---|---|
| Native Windows Hello with an external IR "Hello" webcam | **Best if the goal is security, not building** | Stronger anti-spoofing by design [R1]. Out of scope here, but buy this if you want assurance over learning. |
| Windows Biometric Framework custom adapter | Rejected (not verified in research; revisit if the CP path is blocked) | To our understanding it needs driver-level packaging and signing, and Windows' face engine is Microsoft's. |
| Registry auto-logon | Rejected | Stores the password in plaintext in the registry and has no face check. |
| Replace Winlogon/GINA | Not possible | Replaced by Credential Providers since Windows Vista. |
| ML inside the CP (in-process) | Rejected | Heavy code inside `LogonUI.exe` threatens stability and enlarges the SYSTEM attack surface. |
| C++ CP shim + Rust service | **Contingency** | Proven by prior art [R15]; use only if Rust COM friction blocks P0-S1. |
| Python service (prior-art style) | Rejected | Footprint, start-up latency and supply-chain size conflict with the goals. |
| Password via LSA secret read inside the CP | Considered | Prior art does this [R15]. Kenali prefers a TPM-wrapped blob in the service so secrets are not read inside LogonUI at rest. See ADR-006. |
| **Management CLI only (v0.1 design)** | Superseded | Owner requested UI-first operation in Rev 0.2; a minimal recovery CLI is retained as an internal fallback [CONFIRM — §17 Q7]. |
| **Web-based UI (Tauri/wry)** | Rejected | Adds a web toolchain and a browser engine against footprint and supply-chain goals; the owner wants pure Rust. |
| **Native Win32 UI via the `windows` crate** | **Contingency** | Smallest dependency tree and most native look; more unsafe code and slower to build; adopt if egui friction appears (A11). |
| **iced** | Considered | Retained-mode pure Rust; heavier wgpu focus; pick only if immediate mode is unwanted. |

---

## 17. Open questions (defaults apply unless you say otherwise)

| # | Question | Default |
|---|---|---|
| Q1 | Do you ever want face to work for sign-in after reboot/sign-out (`CPUS_LOGON`)? | No. |
| Q2 | Will this become the daily driver on your main account? | Yes, only after the secondary-account soak. |
| Q3 | Microsoft account or local account? | Either, if it has a password. |
| Q4 | Any plan to share or sell it? | No (so permissive-licence models are a preference, not a hard blocker). |
| Q5 | ~~CLI only, or a small tray app later?~~ | **Answered in Rev 0.2:** UI-first (`kenali-ui.exe`); tray quick-status stays a "Could" (Q10). |
| Q6 | Which GUI framework? | **egui/eframe** (A11); alternatives in §16. |
| Q7 | Keep the minimal internal recovery CLI (`kenali`: status, export-diagnostics, disable, refresh-credential)? | Yes — useful for VM scripts and recovery where the GUI cannot run. |
| Q8 | Live camera preview during enrolment? | Yes (FR-18, Should), RAM-only. |
| Q9 | UI language? | English first via a single string table; Indonesian as a fast follow. |
| Q10 | Tray icon with quick status + Disable? | Not in v1. |
| Q11 | Elevation model: always elevated (one UAC prompt at launch)? | Yes (A12). |

---

## 18. Glossary

**CP** Credential Provider · **LogonUI** the process that draws the lock/sign-in UI · **PAD** presentation-attack detection · **APCER/BPCER** attack/bona-fide presentation classification error rates (ISO/IEC 30107-3) · **FAR/FRR/TAR** false accept / false reject / true accept rate · **DEK/KEK** data / key-encryption key · **AEAD** authenticated encryption · **ORT** ONNX Runtime · **SCM** Service Control Manager · **WinRE** Windows Recovery Environment · **UAC** User Account Control (elevation prompt) · **DPI** dots-per-inch scaling (per-monitor DPI awareness).
