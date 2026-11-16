# Architecture — Kenali

| Field | Value |
|---|---|
| Version | 0.2 (Rev 0.2: UI-first operation) |
| Date | 2026-11-17 |
| Status | Design contract. Companion to `prd.md` and `threat_model.md`. |
| Source of truth | `context_pack.md` §1, §3.5, §3.6, §3.8, §6, §9; `prd.md` §12, §9, §14 |

> **Honest limit, restated.** Kenali uses an ordinary RGB webcam, not an IR/depth sensor. It is a convenience factor with strong compensating controls, not a peer of Windows Hello/Face ID. No component in this document is designed to change that fact. Do not oversell it in code comments, UI text or docs.

---

## 1. System context — three programs, one DLL

Kenali is deliberately split so that the process that touches Windows authentication (LogonUI) is as small and dumb as possible, and all recognition, policy and secret handling lives in a resident SYSTEM service. The operator GUI is a third, fully optional process.

```
                 ┌──────────────────────────────────────────────────────────┐
  SESSION 1      │  LogonUI.exe  (Microsoft, loads our DLL in-process)      │
  (lock screen)  │  ┌────────────────────────────────────────────────────┐  │
                 │  │ kenali-cp.dll   thin COM Credential Provider       │  │
                 │  │  - panic firewall on every COM entry point        │  │
                 │  │  - allowed-SID guard, kill-switch read             │  │
                 │  │  - no ML, no network, no async, ≤1 MB               │  │
                 │  └───────────────┬────────────────────────────────────┘  │
                 └──────────────────┼─────────────────────────────────────────┘
                                    │ \\.\pipe\kenali.auth   (auth pipe)
                                    │ SYSTEM-only DACL · one-shot secret release
                                    ▼
                 ┌──────────────────────────────────────────────────────────┐
  SESSION 0      │  kenali-svc.exe  SYSTEM service (resident, warm models) │
  (service)      │  camera → vision → policy → TPM-unwrapped secret        │
                 │  - owns the camera exclusively                          │
                 │  - enforces ALL policy; registry holds only a disable    │
                 │  - Model Foundation capture, ORT CPU inference          │
                 │  ▲                    ▲                                 │
                 └──┼────────────────────┼─────────────────────────────────┘
                    │ admin pipe         │ preview pipe (only during enrol/
                    │ \\.\pipe\kenali.admin   calibration windows)
                    │ SYSTEM + elevated  │ same DACL as admin pipe
                    │ Administrators     │ 320×240 RGB frames, RAM-only
      ┌─────────────┴────────┐    ┌──────┴──────────────────┐
      │ kenali-ui.exe        │    │ (same UI process renders │
      │ management GUI       │◄───┤ preview frames inline)   │
      │ egui/eframe, native  │    └─────────────────────────┘
      │ elevated via UAC     │
      │ Talks ONLY to admin + preview pipes.                  │
      │ No auth pipe, no templates, no embeddings, no secret. │
      └──────────────────────┘
      ┌──────────────────────┐
      │ kenali.exe           │  minimal internal recovery CLI [CONFIRM]
      │ status, export-diagnostics, disable, refresh-credential
      │ admin pipe only; internal tool, not an operator surface
      └──────────────────────┘
```

**Programs:** `kenali-cp.dll`, `kenali-svc.exe`, `kenali-ui.exe`, plus the minimal `kenali.exe` recovery CLI **[CONFIRM — prd §17 Q7]**. The phrase "three programs, one DLL" counts the CP DLL plus the service and UI executables; the recovery CLI is a fourth, internal-only binary.

**The UI is optional at logon (SEC-20).** Unlock, password/PIN fallback, the registry kill switch and recovery never depend on the UI being installed or running. The CP and service never spawn or embed the UI. The UI's absence or breakage can never contribute to a lock-out.

---

## 2. Trust boundaries

| ID | Boundary | Crossing | Why it matters |
|---|---|---|---|
| TB-1 | LogonUI (Microsoft, Session 1) ↔ CP DLL | in-process COM | The CP runs at the most sensitive moment. Every COM entry point is behind `catch_unwind`; a panic returns `HRESULT`, never unwinds into LogonUI. |
| TB-2 | CP ↔ service | auth pipe | Crosses from Session 1 to Session 0 and from a medium-integrity UI host to SYSTEM. Both sides verify the peer; secret release is one-shot and nonce-bound. |
| TB-3 | Service ↔ camera | kernel/UVC, Media Foundation | The camera is untrusted input. Frames are hostile bytes; camera pinning binds the physical device. |
| TB-4 | Service ↔ TPM/CNG | `MS_PLATFORM_CRYPTO_PROVIDER` | Hardware trust root. Key is non-exportable, machine-scoped; software fallback is loud and explicit. |
| TB-5 | UI/CLI (user, elevated) ↔ service | admin + preview pipes | The GUI is a management surface, not a privileged one. Every mutation is a typed, schema-validated command; the UI never writes HKLM state. |
| TB-6 | Service ↔ filesystem / registry | `C:\Program Files\Kenali\<version>\`, HKLM | Install path is admin-write-only; models hash-pinned (`models.lock`); registry carries only the disable switch. |
| TB-7 | Build/signing ↔ runtime | `xtask sign`, Authenticode | The security of binaries and models depends on the build chain; see ADR-004 and threat model ADV-7. |

**Principle:** the service is the single policy enforcement point (SEC-07). The CP, the registry and the UI cannot loosen security; the registry can only *tighten* (disable). The UI talks **only** to the admin and preview pipes — never to auth, templates, embeddings, secrets or the camera (Rev 0.2 rule, SEC-19).

---

## 3. Crate boundaries and allowed dependency directions

Workspace layout per `context_pack.md` §6. Exactly 11 crates:

| Crate | Kind | `unsafe` | Role |
|---|---|---|---|
| `kenali-core` | lib | `forbid` | Types, `FrameSource` trait, policy engine, state machine, config schema, scoring. |
| `kenali-ipc` | lib | `forbid` | Wire types + `postcard` codec, limits, `PROTO_VERSION` — auth, admin **and** preview messages. |
| `kenali-store` | lib | `forbid` | File formats, AEAD, key-hierarchy traits, sealed secret/templates. |
| `kenali-vision` | lib | `forbid` | Preprocess, align, decode, match, PAD, decision window; `ort` behind traits. |
| `kenali-winapi` | lib | allowed | NCrypt/TPM, LSA/WTS, pipes + SDDL, registry, service helpers, Event Log, UI helpers (single-instance mutex, elevation relaunch, DPI manifest plumbing). |
| `kenali-camera` | lib | allowed | `MfSource` (prod), `FileSource` (replay), `SyntheticSource` (fuzz/bench). |
| `kenali-service` | bin (`KenaliSvc`) | — | Orchestration: camera → vision → policy → secret; hosts the three pipes. |
| `kenali-cp` | `cdylib` | allowed | COM glue only; panic firewall, allowed-SID guard, kill switch. |
| `kenali-ui` | bin (`kenali-ui.exe`) | `forbid` | Management GUI (egui/eframe). Depends only on `ipc`, `core` (config/status), `winapi`. |
| `kenali-cli` | bin (`kenali.exe`) | — | Minimal internal recovery CLI. |
| `kenali-bench` | bin | — | Eval harness, calibration, latency benches. |

`xtask` and `fuzz/` are build/test tooling, not part of the 11 crates.

### Allowed dependency directions

```
              ┌─────────────────────────────────────────────┐
              │  kenali-core   (no deps on other crates)    │
              └───▲──────────────▲──────────────▲───────────┘
                  │              │              │
            kenali-ipc     kenali-store    kenali-vision
                  │              │              │
                  └──────────┬───┴──────────────┘
                             ▼
                      kenali-service ──────► (everything except kenali-cp)

   kenali-cp    ──► ipc, windows, zeroize ONLY
   kenali-ui    ──► ipc, core (config/status types only), winapi
                      NEVER camera, vision, store
   kenali-cli   ──► ipc, core, winapi  (admin pipe only)
   kenali-bench ──► core, ipc, camera, vision, store

   NOTHING depends on kenali-cli or kenali-ui.
   kenali-vision NEVER depends on kenali-winapi.
```

Hard rules (from `context_pack.md` §6 / §2):
- `core` ← `ipc`, `store`, `vision` (these three depend on `core`, never the reverse).
- `service` may depend on everything **except** `cp`.
- `cp` depends only on `ipc`, `windows`, `zeroize`. No ML, no network, no async.
- `vision` never depends on `winapi`.
- `ui` depends on `ipc`, `core` (config/status types only) and `winapi` — **never** on `camera`, `vision` or `store`. This makes a front-end swap (A D R-002 contingency) contained to one crate.
- Nothing depends on `cli` or `ui`.
- `unsafe` only in `kenali-winapi`, `kenali-camera`, `kenali-cp`. Everywhere else — **including `kenali-ui`** — is `#![forbid(unsafe_code)]`; every `unsafe` block carries a `// SAFETY:` comment.

---

## 4. IPC topology — the three pipes

Full wire contract and caps live in `context_pack.md` §8/§9. Summary:

| | Auth pipe `\\.\pipe\kenali.auth` | Admin pipe `\\.\pipe\kenali.admin` | Preview pipe `\\.\pipe\kenali.preview` (Rev 0.2) |
|---|---|---|---|
| **Purpose** | CP asks service to run unlock; service returns the sealed password on success. | UI/CLI send typed management commands (enrol, templates, settings, status, diagnostics, enable/disable, refresh credential). | Service streams downscaled live preview frames to the UI during enrolment/calibration only. |
| **DACL (SDDL)** | `D:P(A;;GA;;;SY)` — SYSTEM only | `D:P(A;;GA;;;SY)(A;;GA;;;BA)` — SYSTEM + elevated Administrators | same as admin pipe |
| **Create flags** | `FILE_FLAG_FIRST_PIPE_INSTANCE`, overlapped, message mode, `PIPE_REJECT_REMOTE_CLIENTS`, small max instances | same | same; instance exists **only during enrolment/calibration windows** (opened by `BeginPreview`, closed by `EndPreview` / wizard end / app exit) |
| **Server verifies client** | `GetNamedPipeClientProcessId` → image path is `%SystemRoot%\System32\LogonUI.exe` in the console session | token is elevated Administrator; client image path is a signed Kenali binary under `C:\Program Files\Kenali\<version>\`; commands typed, never file paths | same as admin pipe |
| **Client verifies server** | CP compares `GetNamedPipeServerProcessId` with the PID SCM reports for `KenaliSvc` (defeats pipe squatting) | UI/CLI do the same | same |
| **Secret release** | one-shot, nonce-echoed, ≤ 3 s after decision, only to the instance that began the attempt | — | — |
| **Message cap** | ≤ 4096 B | ≤ 4096 B control; diagnostics ZIP in ≤ 64 KB chunks, total ≤ 10 MB | ≤ 512 KB (320×240 RGB = 230,400 B fits) |
| **Lifecycle** | always listening | always listening | opened/closed per enrolment window |

**Rev 0.2 rule:** the UI talks **only** to the admin and preview pipes. It has no access to the auth pipe, templates, embeddings or the sealed secret (SEC-03, SEC-19). The recovery CLI uses the admin pipe only. The service accepts no file paths over any pipe — the diagnostics ZIP is returned as bytes and the UI chooses where to save it (FR-09, FR-14). All IPC bytes are treated as hostile: typed enums, strict size caps, no allocation from untrusted lengths, fuzzed decoders, `PROTO_VERSION` handshake.

---

## 5. Secret / key hierarchy

From `context_pack.md` §3.6. Hybrid encryption, because TPM RSA-2048-OAEP-SHA256 can wrap at most 190 bytes.

```
   TPM (non-exportable RSA-2048 key, CNG MS_PLATFORM_CRYPTO_PROVIDER)
        │  machine-scoped (NCRYPT_MACHINE_KEY_FLAG), NCryptFinalizeKey required
        │  RSA-2048-OAEP-SHA256  ── wraps ──►  32-byte random DEK
        ▼
   ┌─────────────────────────────────────────────┐
   │ DEK (Zeroizing, never persisted in clear)   │
   └──────────────┬──────────────────────────────┘
                  │ XChaCha20-Poly1305, 24-byte random nonce
                  │ AAD = magic ‖ version ‖ file type ‖ user SID
        ┌─────────┴──────────┐
        ▼                    ▼
  password blob        template blobs
  (account password)   (face embeddings)
```

Flow: `NCryptOpenStorageProvider(MS_PLATFORM_CRYPTO_PROVIDER)` → `NCryptCreatePersistedKey` → **`NCryptFinalizeKey`** (the key is not usable before finalize) → wrap the DEK. Create with elevation, mark non-exportable, use `NCRYPT_MACHINE_KEY_FLAG` consistently on create/open or the key becomes user-scoped.

- **AEAD:** `chacha20poly1305` (XChaCha20-Poly1305), random 24-byte nonce, AAD = `magic ‖ version ‖ file type ‖ user SID`.
- **Fallback:** `MS_KEY_STORAGE_PROVIDER` (software) only behind an explicit `--allow-weak-keystore` flag and a loud warning. Windows VBS key protection is a possible middle tier (**VERIFY** flags).
- **BitLocker:** assumed on; TPM wrapping is defence in depth, not a substitute for full-disk encryption.
- **Secrets in memory:** `Password(Zeroizing<Vec<u16>>)` and `Embedding(Zeroizing<…>)` newtypes with no `Clone`, `Display` or derived `Debug` (hand-written `Debug` prints `<redacted>`). No `String`, `format!`, logs or error messages for secrets (SEC-01/SEC-12). Exception: the UI's `String` password fields — see `threat_model.md` R14.
- **At rest:** only AEAD ciphertext on disk; raw frames never persisted, in the service or the UI (SEC-02).

TPM unwrap latency is unknown — measure in spike P0-S4.

---

## 6. Vision pipeline (order matters)

From `context_pack.md` §3.5 and `prd.md` §12. CPU-first ONNX Runtime; all stages behind traits so models can be swapped after measurement.

```
 Frame (Media Foundation, BGR)
    │
    ▼
 1. Detect — YuNet (MIT)
    │  BGR raw 0–255, NCHW, H/W padded to multiples of 32
    │  per-stride (8/16/32) cls, obj, bbox, kps; score = √(cls·obj)
    ▼
 2. Quality gate — reject blur / bad pose / bad lighting / multi-face
    │  (comparable-size second face rejects the attempt; FR-13)
    ▼
 3. Align — similarity transform (Umeyama) from 5 landmarks to 112×112
    │  targets: (38.29,51.70) (73.53,51.50) (56.03,71.74)
    │           (41.55,92.37) (70.73,92.20)
    ▼
 4. Embed — SFace (Apache-2.0)
    │  112×112 aligned, RGB raw 0–255, NCHW → 128-d, L2-normalise, cosine
    ▼
 5. Match — cosine vs multiple stored templates, CALIBRATED threshold
    │  do NOT ship OpenCV's default 0.363 (tuned for LFW accuracy, not 1e-5 FAR)
    ▼
 6. Passive PAD — MiniFASNet V2 + V1SE (Apache-2.0)
    │  face crops at ≈2.7× and ≈4.0× box scale → 80×80; fuse real-probabilities
    ▼
 7. Temporal decision window + attention gating (eyes open, frontal within ±20°)
    ▼
 8. Policy engine in the service — cooldowns, scenario, camera pin, SID, 24 h rule
    ▼
 9. On success only: unwrap DEK, decrypt password, one-shot release on auth pipe
```

Notes:
- `ort =2.0.0-rc.13` with `load-dynamic`, loading **Kenali's own** `onnxruntime.dll` by absolute path (`ort::init_from(path)`) — Windows ships an older one and picking the wrong DLL is both a crash (`0xc000007b`) and a DLL-hijack risk.
- Intra-op threads = `min(4, physical cores)`, inter-op = 1, graph optimisation = all, reused sessions, pre-allocated input tensors, one dummy inference at service start.
- No per-frame allocation in the hot path; early exit when a gate fails.
- Passive RGB liveness is camera- and scene-dependent — one layer, never the whole defence (ADV-1/ADV-2).

---

## 7. Runtime model

- **No async runtime anywhere.** The UI runs on egui's own winit event loop; the service and CP use `std::thread` + bounded `crossbeam-channel`. The event loop is not an async runtime. Introduce one only if a measurement justifies it.
- **No CP filter.** `ICredentialProviderFilter` is out of scope: it can hide the password tile and cause lock-out (R1). Password/PIN providers are never hidden, replaced, filtered or delayed (SEC-20).
- **Safe-by-default scenarios:** only `CPUS_UNLOCK_WORKSTATION` on; `CPUS_LOGON` opt-in; never CredUI, change-password, PLAP or RDP (SEC-17). Return `E_NOTIMPL` for unsupported scenarios.
- **Panic firewall:** every COM / `extern "system"` entry point is wrapped in `catch_unwind` and returns an `HRESULT` on panic. No `unwrap`, `expect`, `panic!`, unchecked indexing or blocking waits on LogonUI callback threads.
- **Kill switch:** `HKLM\SOFTWARE\Kenali\Disabled=1` makes the CP return `E_NOTIMPL` at next session creation; the UI reads but never writes it.

---

## 8. Architecture decision records

### ADR-001 — Credential Provider, not Windows Biometric Framework

- **Context.** Windows exposes two extension points for custom sign-in: a Credential Provider (COM objects loaded by LogonUI, supported for custom tiles on the lock screen) and the Windows Biometric Framework (a driver-level plug-in whose face engine is Microsoft's). Research verified the CP extension point and Rust bindings (`windows-rs` `ICredentialProvider`, `_Impl` traits, a working Rust sample); the WBF route was not verified and appears to require driver-level packaging/signing. Requirements: work on the Windows 11 lock screen, pure Rust, personal use, no driver signing. `context_pack.md` §3.1; `prd.md` §16.
- **Decision.** Implement a thin COM Credential Provider DLL (`kenali-cp.dll`) loaded by `LogonUI.exe`, declaring only `CPUS_UNLOCK_WORKSTATION`. Do not build a WBF adapter or integrate with Windows Hello. The CP is a dumb relay: ask the service, wait (bounded), pack credentials, zeroize. No ML, no network, no async.
- **Consequences.** (+) Uses a documented, Rust-tractable extension point; no driver signing or kernel code. (+) Keeps heavy code out of LogonUI. (−) A faulty CP can lock the owner out — mitigated by minimal code, panic firewall, no filter, kill switch, allowed-SID guard, VM-first rollout and recovery runbook (R1; threat TM-10). (−) Must handle undocumented behaviour (`RequestSerialization`, `SetDisplayState`) via feature detection, never dependency. (−) Does not gain Hello's IR-based assurance (honest RGB limit).

### ADR-002 — Native egui GUI over a web stack

- **Context.** Rev 0.2 requires every operator workflow (enrol, templates, settings, status, diagnostics, enable/disable, refresh credential) to be reachable without a terminal, in pure Rust, with a small footprint and a large supply chain already dominated by vision dependencies. Options: egui/eframe (pure Rust immediate mode, renders in-RAM preview frames easily), native Win32 via `windows` (smallest dependency tree, more unsafe), iced (retained mode, wgpu-heavy), Tauri/wry (web engine — rejected against footprint/supply-chain goals and the owner's pure-Rust constraint). `prd.md` §16; `context_pack.md` §3.8, A11.
- **Decision.** Build `kenali-ui.exe` with egui/eframe (pinned exact version at bootstrap), `glow` backend preferred with `wgpu` as fallback, single window, elevated via UAC, no web stack, no async runtime. Win32 needs (single-instance mutex, elevation relaunch, DPI plumbing) live in `kenali-winapi`; `kenali-ui` itself is `#![forbid(unsafe_code)]`. The UI depends only on `kenali-ipc` and `kenali-core` config/status types; it never opens the camera and never touches the auth pipe, templates, embeddings or secrets.
- **Consequences.** (+) In-RAM preview rendering with no codec dependency; contained dependency surface via `kenali-ipc`. (+) Front-end swap stays inside one crate if egui friction appears (contingency: native Win32). (−) egui/eframe plus winit + GL adds a visible supply-chain and binary-size cost (R13); mitigate with exact pins, `cargo-deny`/`cargo-audit`/`cargo-geiger` scoped to the UI, and a software-GL fallback for VMs. (−) GUI text fields are `String`-based: R14 accepted exception. (−) The UI is strictly optional at logon (SEC-20), so a broken GUI can never cause a lock-out.

### ADR-003 — CPU-first ONNX Runtime, no DirectML

- **Context.** Inference must run at the logon screen on the reference laptop, inside a SYSTEM service, with no GPU assumptions. `ort` (ONNX Runtime bindings) is at `2.0.0-rc.13` in the sources seen. Microsoft lists DirectML as "sustained engineering" with Windows ML as the forward path. Windows ships its own older `onnxruntime.dll`. `context_pack.md` §3.4; `prd.md` §4.
- **Decision.** Run all models on CPU via `ort` pinned with `load-dynamic`, loading Kenali's own hash-pinned `onnxruntime.dll` by absolute path. Do not use DirectML or any hardware execution provider in v1. Use INT8 models, warm the session at service start, reuse sessions and pre-allocated tensors. Keep `tract` as a benchmark fallback if ORT proves problematic.
- **Consequences.** (+) Predictable behaviour at the lock screen with no GPU/driver dependency; avoids a deprecated execution path. (+) Loading our own DLL by absolute path avoids both the `0xc000007b` mismatch and a DLL-hijack vector. (−) Latency depends on CPU cores — the ≥ 4-core assumption (A7) must be validated on the reference laptop; re-baseline NFR-01/02 if wrong. (−) Hardware acceleration is deferred; a later, optional upgrade, not a v1 concern.

### ADR-004 — TPM-wrapped blob in the service, not an LSA secret read in the CP

- **Context.** Prior art stores the account password in an LSA secret and reads it from inside the Credential Provider at unlock time. That places secret access inside `LogonUI.exe` and makes the password available at rest to anything that can read LSA under the right privileges. Requirements: hardware-bound secret at rest, minimal CP, no secret material read in LogonUI, TPM 2.0 present with BitLocker assumed on. `context_pack.md` §3.6; `prd.md` §16 (where this decision is cross-referenced as "ADR-006"; this document canonicalizes it as ADR-004).
- **Decision.** Keep the password encrypted at rest in the service's store: XChaCha20-Poly1305 with a random DEK, the DEK wrapped by a non-exportable, machine-scoped TPM RSA-2048 key via CNG `MS_PLATFORM_CRYPTO_PROVIDER` (`NCryptFinalizeKey`, `NCRYPT_MACHINE_KEY_FLAG`). The service unwrapps and releases the plaintext only on a successful, policy-approved unlock, one-shot and nonce-bound over the auth pipe, ≤ 3 s after the decision. The CP never holds a password at rest and never reads LSA secrets. Software-key fallback only behind `--allow-weak-keystore` with a loud warning.
- **Consequences.** (+) Secrets are hardware-bound and never resident in the CP at rest; unauthorised disk access yields only ciphertext. (+) Compromising LogonUI/CP alone does not expose stored credentials. (−) Adds TPM/CNG complexity and a dependency on TPM availability; measure unwrap latency in P0-S4 and provide the explicit weak-keystore escape hatch. (−) The plaintext necessarily exists briefly in the service and the CP at the moment of logon — the same fundamental exposure Windows itself has; accepted and documented (threat model R14/TM-07). (−) TPM wrapping is defence in depth, not a substitute for BitLocker.

---

## 9. Cross-reference to threats

Every architectural surface above has a corresponding threat entry in `threat_model.md`: pipes → TM-08/TM-14/TM-22, UI surface → TM-19/TM-21/TM-22/TM-23, camera → TM-02/TM-03, policy → TM-13/TM-24, secret hierarchy → TM-07, build/TPM → TM-09/TM-16. Architectural changes require an ADR here and a threat-model update (working agreement, `context_pack.md` §11).
