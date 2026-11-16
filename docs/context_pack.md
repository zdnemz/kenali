# Context Pack — Kenali (brief for the coding agent)

> **Read this file first.** It is self-contained enough to start work. Depth lives in `prd.md`, `architecture.md`, `threat_model.md`, `roadmap.md`, `test_plan.md`, `dev_safety_runbook.md`. Source tags like [R6] point to `references.md`.
> Date of this pack: 2026-11-16. Anything marked **VERIFY** was not confirmed in research — confirm it (docs.rs / Microsoft Learn / a spike) before relying on it.
> **Rev 0.2:** every operator workflow is UI-first (`kenali-ui.exe`, native Rust GUI). The management CLI of v0.1 is gone; a minimal internal recovery CLI is retained **[CONFIRM]**. The logon-side CP tile is unchanged.

---

## 1. Mission

Build **Kenali**: face unlock for the Windows 11 lock screen, in Rust, using an ordinary RGB webcam, 100 % local. Priorities: **security → never lock the owner out → speed → footprint.** All operator workflows (enrol, templates, settings, status, diagnostics, enable/disable, refresh-credential) happen in a native Rust GUI; nothing in daily operation requires a terminal.

Shape of the system:

```
LogonUI.exe ── loads ──► kenali-cp.dll  (thin Credential Provider, COM, Rust)
                              │  auth pipe  \\.\pipe\kenali.auth   (SYSTEM-only DACL)
                              ▼
                        kenali-svc.exe  (SYSTEM service: camera → vision → policy → TPM-sealed secret)
                              ▲  admin pipe  \\.\pipe\kenali.admin  (Administrators, elevated)
                              ▲  preview pipe \\.\pipe\kenali.preview (only during enrolment/calibration windows)
   kenali-ui.exe  (management GUI: enrol wizard, dashboard, templates,
                   settings, security, diagnostics — talks ONLY to the admin pipe)
   kenali.exe     (minimal internal recovery CLI [CONFIRM]: status, export-diagnostics,
                   disable, refresh-credential — for VM scripts and recovery)
```

Honest limit: one RGB camera ≠ Windows Hello/Face ID. Convenience factor with strong compensating controls, not equal assurance. Do not oversell this in code comments, UI text or docs.

---

## 2. Non-negotiables

1. **Never hide, replace or filter the default password/PIN providers.** No `ICredentialProviderFilter` in v1.
2. **Never register the CP on the dev host's main account first.** Order: VM → secondary host account → main account. Follow `dev_safety_runbook.md`. As an agent, **never run install/registration commands on the machine you are working on**; produce scripts and manual steps instead.
3. **Every COM / `extern "system"` entry point is wrapped in `catch_unwind`** and returns an `HRESULT` on panic. No `unwrap`, `expect`, `panic!`, unchecked indexing or blocking waits on LogonUI callback threads.
4. **The CP has no ML, no network, no async runtime and no heavy dependencies.** Target ≤ 1 MB DLL. It is a dumb relay: it asks, waits (bounded), packs, zeroizes.
5. **Secrets never touch `String`, `format!`, `Debug`, logs or error messages.** Passwords and embeddings use redacting newtypes over `Zeroizing<…>`. Zeroize on every path, including errors.
6. **Never persist raw frames or images.** A dev-only dump exists solely behind `cfg(debug_assertions)` and an env var, and is never compiled into release.
7. **Policy lives only in the service.** The CP, the registry and the UI cannot loosen anything; the registry only carries a *disable* switch, and the UI only *reads* it.
8. **One submit per attempt.** After a wrong-password / logon-failure / locked-out / expired / must-change result: disable face unlock and never retry.
9. **Release the password only to the pipe instance and nonce that started the attempt**, within ≤ 3 s of the decision.
10. **Load DLLs and models by absolute path** from `C:\Program Files\Kenali\<version>\`. Verify each model's SHA-256 against the compiled-in `models.lock` before creating a session.
11. **Treat every IPC byte as hostile:** typed enums, strict size caps, no allocation from untrusted lengths, fuzz every decoder.
12. **Thresholds come from calibration data**, never from tutorial defaults.
13. **`unsafe` only in `kenali-winapi`, `kenali-camera`, `kenali-cp`.** Every other crate — **including `kenali-ui`** — has `#![forbid(unsafe_code)]`. Every `unsafe` block carries a `// SAFETY:` comment.
14. **Do not use** WinRT `MediaCapture` in the service, DirectML, or `nokhwa` in production paths (reasons in §14).
15. **Never commit** model weights, datasets, certificates/PFX, secrets or anyone's face data.
16. **The management UI never receives secrets** (Rev 0.2). It speaks only the typed admin protocol: no access to the auth pipe, templates, embeddings or the sealed password. The diagnostics ZIP is returned as bytes over the admin pipe; the UI saves it where the user chooses — the service still accepts no file paths.
17. **Preview and enrolment frames exist in RAM only** (service + UI), zeroized on every exit path, never written to disk, never logged. The preview pipe opens only during enrolment/calibration windows and is closed afterwards.
18. **Every UI mutation is a typed admin-pipe command** validated by the service's policy/schema engine. The UI never writes Kenali's HKLM state and never talks to the camera itself.
19. **UI hygiene:** password field buffers are cleared immediately after submit (accepted exception — egui fields are `String`-based; documented in §3.8 and threat_model R14); no secret ever reaches UI logs, UI state or error messages; the UI process is signed and loads nothing by relative path.
20. **The UI is optional at logon.** Unlock, password fallback, the kill switch and recovery never depend on the UI being installed or running.

---

## 3. Verified facts — do not re-derive

### 3.1 Credential Provider mechanics [R6][R7][R8][R10]

- A CP is an in-process COM server loaded by `LogonUI.exe`; Explorer may also load it for some UAC/CredUI prompts. Decline scenarios you do not support.
- DLL exports: `DllGetClassObject`, `DllCanUnloadNow` (module-wide ref-count; `S_OK` only at zero). Registration is done by `xtask`, not by `DllRegisterServer`.
- Registry (native x64 view). **Generate your own CLSID once and commit it as a constant — never reuse one from a blog.**
  - `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{CLSID}` — default value = display name.
  - `HKCR\CLSID\{CLSID}` — default = name.
  - `HKCR\CLSID\{CLSID}\InprocServer32` — default = **absolute** DLL path; `ThreadingModel` = `Apartment`.
  - No reboot needed: lock or sign out to test. Put the DLL in an admin-write-only folder.
- Call order seen by a provider (no filter): `DllGetClassObject` → `IClassFactory::CreateInstance(ICredentialProvider)` → `SetUsageScenario(cpus, flags)` → *(`SetSerialization` for RDP/CredUI — we decline)* → *(`SetUserArray` if `ICredentialProviderSetUserArray` is implemented)* → `Advise(events, ctx)` → `GetCredentialCount(count, default, autologon)` → `GetCredentialAt(i)` → `GetFieldDescriptorCount/At` → per-credential field callbacks → `Credential::Advise` → `SetSelected` → … → on submit `GetSerialization` → `ReportResult` → `UnAdvise`.
- Return `E_NOTIMPL` from `SetUsageScenario` to decline a scenario; the provider is then unloaded.
- **Autologon** is coordinated in two places: `GetCredentialCount` sets `*pdwDefault` = tile index and `*pbAutoLogonWithDefault = TRUE`; `SetSelected` returns TRUE. It is **not guaranteed**; on Windows 10+, auto-logon Windows considers inappropriate shows a sign-in button as a speed bump [R7].
- To act on an **asynchronous** result (face matched later), call `ICredentialProviderEvents::CredentialsChanged(advise_context)` so LogonUI re-queries `GetCredentialCount`. Optional, **undocumented**: `ICredentialProviderCredentialEvents4::RequestSerialization()` (IID `DF50EA86-B7A9-4485-8F04-930A49686E5B`) [R6] — feature-detect with `QueryInterface`; never depend on it.
- Undocumented `ICredentialProviderWithDisplayState::SetDisplayState` (IID `A09BCC29-D779-4513-BB59-B4DB5D82D2B6`): flags `CPDSF_CURTAIN`=0x4, `CPDSF_PROVIDER_VISIBLE`=0x80, `CPDSF_SHUTDOWN_STARTED`=0x1000. A tile is really visible when PROVIDER_VISIBLE is set and CURTAIN is clear [R6]. Use it to start an attempt; fall back to `SetSelected`.
- Callbacks can arrive on non-UI threads, and LogonUI can unload the provider at any moment. Do not create windows from other threads; marshal the events interface across threads correctly (Global Interface Table) [R6]. **VERIFY** in P0-S1 which thread may call `CredentialsChanged`.
- `SetDeselected`: scrub every secret held by the credential object [R6].
- Windows 10+ can report a *workstation unlock* as `CPUS_LOGON`. Disambiguate by checking whether the current WTS session already has a signed-in user [R15].
- Use the **allowed-SID guard**: show the tile only for the enrolled user's SID.

`windows` crate 0.62.x docs snapshot (0.62.2), module `Win32::UI::Shell` — trait shapes you will implement:

```rust
// ICredentialProvider_Impl
fn SetUsageScenario(&self, cpus: CREDENTIAL_PROVIDER_USAGE_SCENARIO, dwflags: u32) -> Result<()>;
fn SetSerialization(&self, pcpcs: *const CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION) -> Result<()>;
fn Advise(&self, pcpe: Ref<'_, ICredentialProviderEvents>, upadvisecontext: usize) -> Result<()>;
fn UnAdvise(&self) -> Result<()>;
fn GetFieldDescriptorCount(&self) -> Result<u32>;
fn GetFieldDescriptorAt(&self, dwindex: u32) -> Result<*mut CREDENTIAL_PROVIDER_FIELD_DESCRIPTOR>;
fn GetCredentialCount(&self, pdwcount: *mut u32, pdwdefault: *mut u32, pbautologonwithdefault: *mut BOOL) -> Result<()>;
fn GetCredentialAt(&self, dwindex: u32) -> Result<ICredentialProviderCredential>;

// ICredentialProviderCredential_Impl (17 required methods; the important ones)
fn Advise(&self, pcpce: Ref<'_, ICredentialProviderCredentialEvents>) -> Result<()>;
fn SetSelected(&self) -> Result<BOOL>;            // the BOOL is the autologon flag
fn SetDeselected(&self) -> Result<()>;
fn GetSerialization(&self, pcpgsr: *mut CREDENTIAL_PROVIDER_GET_SERIALIZATION_RESPONSE,
                    pcpcs: *mut CREDENTIAL_PROVIDER_CREDENTIAL_SERIALIZATION,
                    ppszoptionalstatustext: *mut PWSTR,
                    pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON) -> Result<()>;
fn ReportResult(&self, ntsstatus: NTSTATUS, ntssubstatus: NTSTATUS,
                ppszoptionalstatustext: *mut PWSTR,
                pcpsioptionalstatusicon: *mut CREDENTIAL_PROVIDER_STATUS_ICON) -> Result<()>;
```

Use `#[implement(ICredentialProvider, …)]`. A working Rust sample (local logon via MSV1_0) and a community walkthrough of exporting the COM entry points exist [R9][R10]. **VERIFY** exact feature names and any `ICredentialProviderCredential2` / `ICredentialProviderSetUserArray` shapes on docs.rs for the pinned version.

### 3.2 Serialization and results [R11]

- Pack a `KERB_INTERACTIVE_UNLOCK_LOGON`: `MessageType = KerbWorkstationUnlockLogon` for `CPUS_UNLOCK_WORKSTATION`, `KerbInteractiveLogon` for `CPUS_LOGON`. The `UNICODE_STRING.Buffer` members are **byte offsets** into the packed buffer, not pointers; strings are not NUL-terminated; allocate with `CoTaskMemAlloc`. Do not cast the output of `CredPackAuthenticationBuffer` (its message type differs) — copy the approach in Chromium's GCPW `auth_utils.cc` [R11].
- GCPW also protects the password with `CredProtectW` before serialization for these scenarios [R11]. Evaluate and adopt if it works with the Negotiate package.
- Authentication package: the Rust sample uses MSV1_0 [R9]; Microsoft's C++ sample uses Negotiate (from memory — **VERIFY**). Test both against a local account and a Microsoft account in P0-S6.
- `ReportResult` statuses that mean "the stored password is stale or the account is blocked" (disable face, never retry): `STATUS_LOGON_FAILURE 0xC000006D`, `STATUS_WRONG_PASSWORD 0xC000006A`, `STATUS_ACCOUNT_LOCKED_OUT 0xC0000234`, `STATUS_ACCOUNT_DISABLED 0xC0000072`, `STATUS_PASSWORD_EXPIRED 0xC0000071`, `STATUS_PASSWORD_MUST_CHANGE 0xC0000224`.

### 3.3 Camera [R15][R35]

- WinRT `MediaCapture` inside a service has been reported to throw access-denied [R35]. Use **Media Foundation** (`IMFSourceReader`), with DirectShow as the fallback if MF is blocked.
- Prior art captures from a LocalSystem service (DirectShow) with retries and a confirmed frame read; after cold boot or resume the camera can take seconds to become ready [R15]. Implement: open with backoff (≈ 0, 100, 200, 400, 800 ms … until the deadline), only call the camera "ready" after a real frame is read, re-enumerate on failure.
- Discard the first N frames (default 5) for auto-exposure/white-balance convergence; tune per camera.
- Pin the physical camera at enrolment (symbolic link + hardware ID); reject anything else (SEC-16).
- The **UI never opens the camera itself**. During enrolment/calibration the service owns the camera and streams downscaled preview frames to the UI (§3.8); the wizard warns the user to close other camera apps to avoid contention.

### 3.4 ONNX Runtime from Rust [R18][R19][R20][R36]

- `ort` newest confirmed: **`2.0.0-rc.13`** (wraps ONNX Runtime 1.28). Pin with `=2.0.0-rc.13`. Re-check crates.io at bootstrap for a stable `2.0.0`.
- Use the `load-dynamic` feature and load **Kenali's own** `onnxruntime.dll` by absolute path (`ort::init_from(path)`). Windows ships its own, older `onnxruntime.dll`; picking the wrong one produces `0xc000007b` [R36] and is also a DLL-hijack risk.
- Since rc.11 the bundled ORT build favours low-resource edge inference over throughput [R19] — a good default for us.
- **Do not use DirectML**: Microsoft lists it as "sustained engineering" with Windows ML as the forward path [R20][R21]. CPU first; hardware EPs are a later, optional upgrade.
- Session settings to set explicitly (**VERIFY** names on docs.rs): intra-op threads = `min(4, physical cores)`, inter-op = 1, graph optimisation = all, reuse sessions, pre-allocated input tensors, one dummy inference at service start.
- `tract` (pure Rust) is the benchmark fallback if ORT proves problematic.

### 3.5 Models and preprocessing [R22][R23][R24][R25][R26][R27]

| Stage | Model | Licence | Input → output |
|---|---|---|---|
| Detect | **YuNet** (OpenCV Zoo; a `…yunet_2026may.onnx` file was seen — **VERIFY** latest) | MIT | BGR, raw 0–255, NCHW, H/W padded to multiples of 32 → per-stride (8/16/32) `cls, obj, bbox, kps`. Score = √(cls·obj). Box centre = (col+dx, row+dy)·stride; size = exp(dw, dh)·stride; landmark = (col+kx, row+ky)·stride. |
| Align | — | — | Similarity transform (rotation + uniform scale + shift, least squares / Umeyama) from the 5 landmarks to the 112×112 template: **(38.29, 51.70), (73.53, 51.50), (56.03, 71.74), (41.55, 92.37), (70.73, 92.20)** = left eye, right eye, nose tip, left mouth corner, right mouth corner. |
| Embed | **SFace** (OpenCV Zoo; INT8 variant exists) | Apache-2.0 | 112×112 aligned, RGB, raw 0–255, NCHW → 128-d; L2-normalise; cosine. 36.9 MB FP32. Cross-check channel order and scaling against OpenCV's `FaceRecognizerSF` outputs on a few images. |
| Passive PAD | **MiniFASNet V2 + V1SE** (Silent-Face-Anti-Spoofing) | Apache-2.0 | Two models fed with face crops at ≈ 2.7× and ≈ 4.0× the box scale, resized to 80×80 (upstream convention — **VERIFY**); fuse real-probabilities. ~1.2 MB each. |

- OpenCV's reference SFace thresholds — cosine **0.363** / L2 1.128 — correspond to 99.6 % LFW accuracy and are *lower* on harder sets (CALFW 0.340, CPLFW 0.275, AgeDB-30 0.277) [R23]. They optimise accuracy, **not** a 1e-5 FAR. Calibrate our own threshold.
- Indicative CPU timings (OpenCV Zoo README, OpenCV DNN on an i7-12700K-class CPU): YuNet @160×120 ≈ 1.45 ms; SFace @112×112 ≈ 8.65 ms [R24]. Treat as order-of-magnitude only.
- **InsightFace pretrained weights (e.g. `buffalo_l`) are non-commercial** and need a separate licence [R25]. Do not bundle by default.
- Passive RGB liveness depends on camera model and scene [R27]; it is **one layer**, never the whole defence.
- Keep detector/embedder/PAD behind traits so models can be swapped after measuring on the eval harness.

| Component | Licence |
|---|---|
| YuNet | MIT |
| SFace, MiniFASNet | Apache-2.0 |
| ONNX Runtime | MIT |
| `ort`, `windows`, `windows-service` | MIT / Apache-2.0 |
| `egui`, `eframe`, `winit`, `glow`/`wgpu` (UI) | MIT / Apache-2.0 (**VERIFY** each at pinning) |
| InsightFace weights | non-commercial — excluded |

### 3.6 TPM and crypto [R29][R30][R31]

- CNG flow: `NCryptOpenStorageProvider(MS_PLATFORM_CRYPTO_PROVIDER)` → `NCryptCreatePersistedKey` → **`NCryptFinalizeKey`** (the key is not stored or usable before finalize) [R29]. Use `NCRYPT_MACHINE_KEY_FLAG` consistently on create/open, otherwise the key is user-scoped. Creating the key needs elevation. Mark it non-exportable.
- TPMs reliably support RSA. RSA-2048-OAEP-SHA256 encrypts at most 190 bytes, so use **hybrid encryption**: random 32-byte DEK; password and templates encrypted with XChaCha20-Poly1305; DEK wrapped by the TPM RSA key.
- Provider fallback: `MS_KEY_STORAGE_PROVIDER` (software) only behind an explicit `--allow-weak-keystore` flag and a loud warning (Chromium selects TPM vs software provider the same way [R30]). Windows VBS key protection is a possible middle tier [R31] — **VERIFY** flags.
- AEAD: `chacha20poly1305` (XChaCha20-Poly1305), random 24-byte nonce, AAD = magic ‖ version ‖ file type ‖ user SID.
- TPM unwrap latency is unknown: measure in P0-S4.
- Assume BitLocker is on; TPM wrapping is defence in depth, not a substitute.

### 3.7 Recovery behaviour [R6][R12][R13][R14]

- A provider can be disabled with a `REG_DWORD` named `Disabled` = 1 under its CLSID key in `…\Authentication\Credential Providers\`; it takes effect at the next session creation [R12].
- In Safe Mode LogonUI loads only the built-in username/password provider [R12][R13]. If you cannot reach Safe Mode, use the WinRE command prompt: edit the offline SOFTWARE hive or delete the DLL [R6][R13]. Exact steps: `dev_safety_runbook.md`.
- Microsoft KB 4073719: custom providers may not load at the first logon after a restart when Windows auto-completes sign-in; workaround is the `DisableAutomaticRestartSignOn` setting [R14]. This is consistent with our "password after boot" policy.

### 3.8 Management GUI and preview channel (Rev 0.2)

- **Framework:** egui/eframe, pinned at bootstrap (**VERIFY** the current stable on crates.io and record it in `rust-toolchain.toml`/`Cargo.lock`). Prefer the `glow` (OpenGL) backend for footprint; `wgpu` is the fallback. **VERIFY on a GPU-less VM** that rendering works (GL on Windows VMs is not guaranteed); if not, evaluate `wgpu` with a software adapter or fall back to the native Win32 front end (prd §16).
- **No async runtime:** the UI runs on egui's own winit event loop; keep `std::thread` + bounded `crossbeam-channel` inside the app (UI thread ⇄ worker thread that owns the pipe connection).
- **Elevation:** the UI is launched elevated (UAC). Implement with `ShellExecuteExW` + verb `runas` via a `kenali-winapi` helper, plus a manifest (`requireAdministrator`). An unelevated instance shows a "run as administrator" message and exits — the admin pipe's SDDL (`…(A;;GA;;;BA)`) rejects its token anyway. **VERIFY** the exact failure the client gets so the message matches reality.
- **Single instance:** named mutex `Local\KenaliUiSingleInstance` via `kenali-winapi`; a second launch focuses the existing window and exits.
- **Preview frames:** the service downscales in RAM (`fast_image_resize`, already pinned) to 320×240 RGB and streams them on `\\.\pipe\kenali.preview`, message-mode, cap ≤ 512 KB per message (320×240×3 = 230,400 B fits), ≈ 15 fps target, only while an enrolment/calibration window is open. The UI renders them straight from RAM and zeroizes its buffers when the window closes or the app exits. No encoder/codec dependency: raw RGB, never JPEG/PNG on the wire, never on disk.
- **Password fields:** egui text fields are `String`-based. Accepted exception (threat_model R14): the buffer is moved into the typed admin-pipe message and cleared immediately after submit; never cloned, logged or persisted. Say this honestly in code comments and the threat model — do not pretend the GUI keeps the password out of process memory.
- **Diagnostics ZIP:** the service returns redacted bytes in framed chunks (≤ 64 KB per chunk, total ≤ 10 MB) on the admin pipe; the UI writes them to the user-chosen path. The service still accepts no file paths (SEC-04).
- **Strings:** all UI text goes through one string table (English in v1; Indonesian planned — prd §17 Q9). Same generic failure wording as the tiles; similarity scores are never rendered.

---

## 4. Known unknowns — resolve in Phase 0 / P1, never assume

| ID | Unknown | Spike |
|---|---|---|
| U1 | Can Media Foundation capture from a Session 0 SYSTEM service at the lock screen? | P0-S2 |
| U2 | Which thread may call `CredentialsChanged`; GIT marshalling pattern | P0-S1 |
| U3 | `CPUS_LOGON` vs `CPUS_UNLOCK_WORKSTATION` reporting and the WTS check | P0-S6 |
| U4 | Negotiate vs MSV1_0 for local and Microsoft accounts | P0-S6 |
| U5 | Interaction with the Microsoft-account "Windows Hello-only sign-in" setting | P0-S6 |
| U6 | TPM unwrap latency (RSA-2048) | P0-S4 |
| U7 | Smart App Control / WDAC vs self-signed binaries | P0-S7 |
| U8 | `ort` stable 2.0 status; INT8 vs FP32 accuracy and latency | P0-S5 |
| U9 | Availability of `RequestSerialization` / `SetDisplayState` on this Windows build | P0-S1 |
| U10 | Is LocalSystem required, or does a virtual service account work for camera + TPM? | P0-S2 |
| U11 | Camera readiness after resume / hibernate / Fast Startup | P4 acceptance |
| U12 | egui/eframe pinned version; `glow` vs `wgpu` on the reference GPU and in a GPU-less VM; per-monitor DPI behaviour | P1 |
| U13 | Preview pipe throughput and CPU cost (downscale + send at ≥ 15 fps) | P2 |

---

## 5. Stack and pins

Pin every dependency at bootstrap (`Cargo.lock` committed, `--locked` everywhere). **VERIFY** all feature names on docs.rs.

| Area | Choice |
|---|---|
| Toolchain | Latest stable via `rust-toolchain.toml` (record the exact version), edition 2024, target `x86_64-pc-windows-msvc`, MSVC Build Tools + Windows SDK. |
| Win32/COM | `windows` 0.62.x (0.62.2 seen). Enable the minimal feature set per crate — likely `Win32_Foundation`, `Win32_UI_Shell`, `Win32_Security`, `Win32_Security_Authentication_Identity`, `Win32_Security_Cryptography`, `Win32_Security_Authorization`, `Win32_System_Com`, `Win32_System_Pipes`, `Win32_System_IO`, `Win32_System_Threading`, `Win32_System_Services`, `Win32_System_RemoteDesktop`, `Win32_System_Power`, `Win32_System_Registry`, `Win32_Media_MediaFoundation`, `Win32_System_EventLog`. |
| Service host | `windows-service` 0.8.x (Mullvad) [R32]; alternative `windows-services` 0.26 [R33]. Default: `windows-service`. |
| Inference | `ort =2.0.0-rc.13` with `load-dynamic` (+ `ndarray`); pinned `onnxruntime.dll`. |
| Imaging | `ndarray`, `fast_image_resize`; `image` for tooling only (not in the hot path). |
| Crypto | `chacha20poly1305`, `sha2`, `zeroize`, `secrecy`, `subtle`, `getrandom`. |
| IPC | `serde` + `postcard`. |
| **GUI (Rev 0.2)** | `egui` + `eframe` with the `glow` backend (pin exact versions at bootstrap — **VERIFY**; `wgpu` fallback). No web stack, no async runtime. The UI crate wraps Win32 needs through `kenali-winapi`. |
| Logging | `tracing`, `tracing-subscriber`; thin Event Log writer over `ReportEventW`. |
| CLI / errors | `clap` (derive) for the recovery CLI and `xtask` only; `thiserror` in libraries, `anyhow` in binaries only. |
| Concurrency | `std::thread` + bounded `crossbeam-channel`. **No async runtime anywhere** (the egui/winit event loop is not an async runtime) unless a measurement justifies it. |
| Camera | Direct Media Foundation via `windows`. `nokhwa` only for throwaway prototypes (maintenance uncertain) [R34]. |
| Quality | `proptest`, `criterion`, `cargo-nextest`, `cargo-fuzz` (nightly), `cargo-deny`, `cargo-audit`, `cargo-llvm-cov`, `cargo-geiger`, `cargo-cyclonedx`. |

---

## 6. Repository layout

```
kenali/
├─ Cargo.toml                  # workspace, resolver = "3"
├─ rust-toolchain.toml  deny.toml  clippy.toml  .cargo/config.toml  .gitignore
├─ models/
│  ├─ models.lock              # name, url, sha256, licence, upstream commit  (committed)
│  └─ *.onnx                   # NOT committed
├─ crates/
│  ├─ kenali-core/             # forbid(unsafe): types, FrameSource trait, policy engine, state machine, config schema, scoring
│  ├─ kenali-ipc/              # forbid(unsafe): wire types + codec (postcard), limits — auth, admin AND preview messages
│  ├─ kenali-store/            # forbid(unsafe): file formats, AEAD, key-hierarchy traits
│  ├─ kenali-vision/           # forbid(unsafe): preprocess, align, decode, match, PAD, decision window (ort behind traits)
│  ├─ kenali-winapi/           # UNSAFE ok: NCrypt/TPM, LSA/WTS, pipes + SDDL, registry, service helpers, Event Log,
│  │                           #   UI helpers (single-instance mutex, elevation relaunch, DPI manifest plumbing)
│  ├─ kenali-camera/           # UNSAFE ok: MfSource, FileSource (replay), SyntheticSource
│  ├─ kenali-service/          # bin KenaliSvc: orchestration
│  ├─ kenali-cp/               # cdylib: COM glue only
│  ├─ kenali-ui/               # bin kenali-ui.exe: management GUI (egui) — forbid(unsafe), talks only via kenali-ipc admin/preview
│  ├─ kenali-cli/              # bin kenali.exe: minimal internal recovery CLI [CONFIRM] — status, export-diagnostics, disable, refresh-credential
│  └─ kenali-bench/            # eval harness, calibration, latency benches
├─ xtask/                      # build | check | models | sign | install | uninstall | rollback | fuzz | bench | package
├─ fuzz/                       # cargo-fuzz targets
├─ docs/                       # these docs + docs/spikes/ + docs/reports/
└─ tests/                      # integration tests, VM scripts
```

Dependency rules: `core` ← `ipc`, `store`, `vision`. `service` may depend on everything except `cp`. `cp` depends only on `ipc`, `windows`, `zeroize`. `vision` never depends on `winapi`. `ui` depends on `ipc`, `core` (config/status types only) and `winapi` — never on `camera`, `vision` or `store`. Nothing depends on `cli` or `ui`.

---

## 7. Coding conventions

- rustfmt defaults; `clippy -D warnings` plus a curated `pedantic` subset; no `#[allow]` without a comment.
- Errors: `thiserror` types in libraries. No `unwrap()` outside tests and `main` setup; `expect` is banned in `cp`, the service hot path and the UI pipe worker.
- Secrets: `Password(Zeroizing<Vec<u16>>)` and `Embedding(Zeroizing<…>)` newtypes with **no** `Clone`, `Display`, or derived `Debug` (hand-written `Debug` prints `<redacted>`). The UI converts its (unavoidable) `String` field contents into `Password` at submit and clears the source immediately.
- Logging: `tracing` with structured fields; a `Redacted` wrapper for anything sensitive; never log scores at finer than 0.05 resolution. The UI logs to the rotating file + Event Log through the same helpers — no secrets, no frames.
- Time: `Instant` for deadlines; every blocking call has a timeout.
- No `static mut`. No global singletons in the CP beyond the module ref-count.
- IPC schema versioned by an explicit `PROTO_VERSION` constant; breaking changes bump it and the handshake rejects mismatches.
- Tests live next to code; property tests for the policy engine and codecs; `// SAFETY:` on every `unsafe`.
- Commits: Conventional Commits. Keep diffs small and reviewable.

---

## 8. Patterns to copy

**Panic firewall — use at every COM entry point** (illustrative; adapt):

```rust
use windows::core::{Result, Error};
use windows::Win32::Foundation::E_UNEXPECTED;

pub fn guard<T>(_what: &'static str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => Err(Error::from(E_UNEXPECTED)), // log `_what` only; never the panic payload
    }
}
```

**Redacting secret newtype:**

```rust
pub struct Password(zeroize::Zeroizing<Vec<u16>>);       // UTF-16; no Clone/Display
impl core::fmt::Debug for Password {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("Password(<redacted>)") }
}
```

**`FrameSource` seam (defined in `kenali-core`, implemented in `kenali-camera`):**

```rust
pub trait FrameSource: Send {
    fn open(&mut self, pin: &CameraPin, deadline: Instant) -> Result<(), CaptureError>;
    fn next_frame(&mut self, deadline: Instant) -> Result<Frame, CaptureError>;
    fn close(&mut self);
}
// MfSource (prod) · FileSource (replay video / PNG sequence — tests and VMs) · SyntheticSource (fuzz, bench)
```

**IPC messages (sketch; caps: auth/control messages ≤ 4096 B, strings ≤ 256 UTF-16 units; preview frames ≤ 512 KB on the preview pipe; diagnostics chunks ≤ 64 KB, ZIP total ≤ 10 MB):**

```rust
// auth pipe (CP ⇄ service) — unchanged from v0.1
pub enum Request  { Hello { proto: u16 },
                    BeginAttempt { scenario: Scenario, sid: Sid, nonce: [u8; 16] },
                    Cancel { attempt: u64 },
                    Report { attempt: u64, nt_status: i32 } }
pub enum Response { Hello { proto: u16, svc_pid: u32 },
                    Status { attempt: u64, phase: Phase },
                    Verified { attempt: u64, nonce: [u8; 16], user: Wide, domain: Wide, secret: SecretWide },
                    Denied { attempt: u64, reason: DenyReason },
                    Error { code: u16 } }

// admin pipe (UI / recovery CLI ⇄ service) — all typed, no file paths ever
pub enum AdminRequest  { Hello { proto: u16 },
                         GetStatus, ListTemplates, DeleteTemplate { id: TemplateId },
                         BeginEnrolment { mode: EnrolMode }, EnrolPassword(PasswordWide),
                         SubmitSample, FinishEnrolment, CancelEnrolment,
                         BeginPreview, EndPreview,                 // preview window open/close
                         BeginTest, BeginCalibration, GetConfig, SetConfig { key: ConfigKey, value: ConfigValue },
                         RefreshCredential(PasswordWide), SetEnabled(bool), ExportDiagnostics }
pub enum AdminResponse { Hello { proto: u16, svc_pid: u32 },
                         Status(StatusReport), TemplateList(Vec<TemplateMeta>),
                         Progress { stage: EnrolStage, pose: Pose, kept: u8, needed: u8 },
                         PreviewFrame { w: u16, h: u16, rgb: Bytes },   // preview pipe only
                         Config(ConfigReport), TestResult(TestReport),
                         DiagnosticsChunk { index: u32, total: u32, bytes: Bytes },
                         Error { code: u16 } }
```

**UI bootstrap (single instance + elevation) — sketch, implement in `kenali-winapi`:**

```rust
// 1. named mutex Local\KenaliUiSingleInstance → if exists, activate existing window, exit 0
// 2. if token is not elevated → ShellExecuteExW(runas) self, exit; if user declines UAC, show why and exit
// 3. connect to \\.\pipe\kenali.admin; verify server PID == SCM's PID for KenaliSvc (anti-squatting, as with the CP)
// 4. spawn one worker thread owning the pipe; UI thread ⇄ worker via bounded crossbeam-channel; PreviewFrames land in a
//    Zeroizing<Vec<u8>> slot the UI thread renders and clears
```

---

## 9. IPC and pipe rules

| | Auth pipe `\\.\pipe\kenali.auth` | Admin pipe `\\.\pipe\kenali.admin` | Preview pipe `\\.\pipe\kenali.preview` (Rev 0.2) |
|---|---|---|---|
| DACL (SDDL) | `D:P(A;;GA;;;SY)` — SYSTEM only | `D:P(A;;GA;;;SY)(A;;GA;;;BA)` — SYSTEM + elevated Administrators | same as admin pipe |
| Create flags | `FILE_FLAG_FIRST_PIPE_INSTANCE`, overlapped, message mode, `PIPE_REJECT_REMOTE_CLIENTS`, small max instances | same | same; instance exists **only during enrolment/calibration windows** |
| Server verifies client | `GetNamedPipeClientProcessId` → image path is `%SystemRoot%\System32\LogonUI.exe` in the console session | token is elevated Administrator; client image path is a signed Kenali binary in `C:\Program Files\Kenali\<version>\`; commands are typed (no file paths) | same as admin pipe |
| Client verifies server | CP compares `GetNamedPipeServerProcessId` with the PID that SCM reports for `KenaliSvc` (defeats pipe squatting) | UI/CLI do the same | same |
| Secret release | One-shot, nonce-echoed, ≤ 3 s after decision, only to the instance that began the attempt | — | — |
| Message cap | ≤ 4096 B | ≤ 4096 B (control); diagnostics ZIP in ≤ 64 KB chunks, total ≤ 10 MB | ≤ 512 KB (320×240 RGB = 230,400 B fits) |
| Lifecycle | Always listening | Always listening | Opened by `BeginPreview`, closed by `EndPreview` / wizard end / app exit |

---

## 10. Commands and environment

**User-facing operations live in `kenali-ui.exe` (Rev 0.2).** The recovery CLI (`kenali.exe` [CONFIRM]) and `cargo xtask` are internal tools, not operator surfaces.

Dev setup on Windows:

```powershell
winget install Rustlang.Rustup
rustup default stable; rustup target add x86_64-pc-windows-msvc
# Visual Studio Build Tools: "Desktop development with C++" (MSVC, Windows SDK, signtool)
cargo install cargo-deny cargo-audit cargo-nextest cargo-llvm-cov cargo-cyclonedx cargo-geiger
rustup toolchain install nightly; cargo install cargo-fuzz
```

`xtask` commands to implement (developer tooling — stays command-line by design):

| Command | Purpose |
|---|---|
| `cargo xtask check` | fmt, clippy `-D warnings`, nextest, deny, audit |
| `cargo xtask build [--release]` | per-crate flags (below), models verified |
| `cargo xtask models fetch\|verify` | download to `models/`, check SHA-256 vs `models.lock` |
| `cargo xtask sign` | `signtool` with the dev certificate (service, CP, UI, CLI) |
| `cargo xtask install --vm` / `--host-test-account` | refuses unless `KENALI_ALLOW_INSTALL=1`; prints the runbook pre-flight; versioned dir; keeps previous version |
| `cargo xtask uninstall` / `rollback` | removes registration, restores prior version |
| `cargo xtask fuzz <target> <secs>` | time-boxed fuzz run |
| `cargo xtask bench` | criterion + end-to-end latency traces |

Release build flags (set via `xtask`/`.cargo/config.toml`; **VERIFY** each on the pinned toolchain): `-C target-feature=+crt-static` (no redistributable at logon), `-C control-flow-guard=yes`, `-C link-arg=/CETCOMPAT`, `lto = "fat"`, `codegen-units = 1`, `opt-level = 3`, `panic = "unwind"` (we rely on `catch_unwind`), debug symbols in a separate PDB. `-C target-cpu=x86-64-v3` for the service only, and only if the reference CPU supports it; keep the CP on baseline. The UI binary may use `x86-64-v3` too (it never runs at logon), but must still ship a static CRT.

Reference-laptop fact sheet (collect before P0): `winver`; `Get-Tpm`; `manage-bde -status`; `Confirm-SecureBootUEFI`; `Get-PnpDevice -Class Camera,Image`; `Get-CimInstance Win32_Processor`; Smart App Control state (Windows Security → App & browser control); account type (local / Microsoft); camera formats and VID/PID; GPU/GL availability for the UI renderer (U12).

---

## 11. Working agreement

**Picking work:** next unchecked task in `roadmap.md` for the current phase. Spikes first.

**Definition of Done for any task:**
- [ ] Tests added or updated (unit/property; integration when it crosses a boundary).
- [ ] `cargo xtask check` passes.
- [ ] No new `unsafe` without `// SAFETY:`; no `unsafe` outside the three allowed crates (UI included in the forbid check).
- [ ] No secret/biometric reaches logs or errors (see `test_plan.md` §7) — service, CP **and UI**.
- [ ] Hot-path change → benchmark before/after recorded.
- [ ] UI change → walkthrough performed in the UI; no new secret in UI state; preview buffers zeroized.
- [ ] Touches install, registry or recovery → `dev_safety_runbook.md` updated and the drill re-run in a VM.
- [ ] Touches the attack surface → `threat_model.md` updated.
- [ ] Architectural change → ADR added to `architecture.md`.

**Spike report template** (`docs/spikes/P0-Sx.md`): environment (OS build, CPU, camera VID/PID, TPM) · procedure · raw results · evidence (logs/screens) · decision (go / pivot / stop) · follow-ups.

---

## 12. Things NOT to do

| Don't | Why |
|---|---|
| Add a Credential Provider Filter | Can hide the password tile → lock-out risk (R1). |
| Run ML, network or an async runtime inside the CP | LogonUI stability and SYSTEM attack surface. |
| Use WinRT `MediaCapture` in the service | Reported access-denied in services [R35]. |
| Use DirectML | Sustained engineering [R20]. |
| Copy a GUID/CLSID from any blog | Name collisions break other providers. |
| Pass the password through logs, `String`, `anyhow` context or panic messages | SEC-01/SEC-12. |
| Retry a submit after a failure status | Account-lockout loops (TM-12). |
| Let any process write to the pipe and unlock (the pattern in [R16]) | That design is the anti-example. |
| Auto-learn templates from face-only unlocks | Template poisoning (TM-06). |
| Show similarity scores on screen (tiles **or the management UI**) | Hill-climbing aid (TM-11). |
| Ship OpenCV's default threshold | Not calibrated for security. |
| Bundle InsightFace weights by default | Non-commercial licence [R25]. |
| Edit a registered DLL in place on a host | Use versioned paths and `xtask rollback`. |
| **Give the UI access to the auth pipe, templates, embeddings or any secret** | The UI is a management surface only (SEC-19/SEC-03). |
| **Persist preview or enrolment frames — in the service or the UI** | SEC-02, FR-18; frames are RAM-only and zeroized. |
| **Let the UI write policy or registry state directly** | Policy lives in the service; the registry only carries the disable switch (SEC-07). |
| **Spawn or embed the UI from the CP or the service** | The UI is user-launched and elevated by UAC; logon must never depend on it (SEC-20). |
| **Open the camera from the UI process** | The service owns the camera exclusively; UI gets downscaled preview frames (SEC-16, §3.8). |
| **Use a web-based UI stack (Tauri/wry)** | Footprint and supply-chain goals; owner wants pure Rust (prd §16). |

---

## 13. Glossary

**CP** Credential Provider · **LogonUI** lock/sign-in UI process · **PAD** presentation-attack detection · **APCER/BPCER** attack / bona-fide classification error rates · **FAR/FRR/TAR** false accept / reject / true accept rate · **DEK/KEK** data / key-encryption key · **GIT** Global Interface Table (COM cross-thread marshalling) · **WinRE** Windows Recovery Environment · **SCM** Service Control Manager · **ESS** Enhanced Sign-in Security (Hello) · **UAC** User Account Control (elevation prompt) · **DPI** dots-per-inch scaling (per-monitor DPI awareness).

---

## 14. Bootstrap prompt (paste into Claude Code in an empty repo)

```text
You are the lead engineer on Kenali, a Rust face-unlock for the Windows 11 lock screen.

Read, in order: docs/context_pack.md, docs/prd.md, docs/architecture.md,
docs/threat_model.md, docs/roadmap.md, docs/dev_safety_runbook.md.

Rules:
- Follow context_pack.md §2 (Non-negotiables) exactly — including the Rev 0.2 UI rules
  (§2 items 16–20): all operator workflows live in kenali-ui.exe, which never touches
  secrets, the auth pipe, or the camera.
- Never run any command that registers a Credential Provider or installs the service on
  this machine. Produce scripts plus step-by-step manual instructions instead, and wait
  for me to run them inside the VM.
- Mark anything you could not verify as VERIFY and list it in the spike report.
- Ask me only about decisions listed in prd.md §17 or about missing reference-laptop facts.

Step 1: create the workspace skeleton from context_pack.md §6 — including an empty
        kenali-ui shell (a window with an empty Dashboard). Every crate compiles empty,
        `cargo xtask check` passes, deny.toml/clippy.toml/rust-toolchain.toml are in place.
Step 2: implement Phase-0 spike P0-S1 from roadmap.md (a minimal Credential Provider DLL that
        loads in LogonUI inside a VM and shows a tile reading "spike", declines every scenario
        except CPUS_UNLOCK_WORKSTATION, never submits, and has a working kill switch).
        Deliver the code, an xtask script for the VM, and docs/spikes/P0-S1.md with what you
        verified offline plus the exact manual steps I must run in the VM.
Stop after Step 2 and summarise what you need from me.
```
