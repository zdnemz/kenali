# Threat Model — Kenali

| Field | Value |
|---|---|
| Version | 0.2 (Rev 0.2: UI-first operation) |
| Date | 2026-11-17 |
| Status | Living document. Companion to `prd.md` (SEC-01…SEC-20) and `architecture.md`. |
| Method | STRIDE per component; attacker model ADV-1…ADV-7; threats TM-01…TM-24. |

> **Honest limit, stated plainly.** Kenali uses a single RGB webcam. It cannot match the assurance of Windows Hello or Face ID: Microsoft's face stack uses near-infrared partly because IR does not appear in photographs or on LCD screens [R1]. Kenali is a **convenience factor with strong compensating controls** — layered presentation-attack detection (PAD), strict policy, hardware-bound secrets and fail-safe fallback — not an equal of IR/depth systems. Passive RGB liveness is camera- and scene-dependent. A determined attacker with a high-fidelity replay may defeat it; the design goal is to resist *casual and opportunistic* spoofing (printed photo, phone/tablet replay) while never locking out the owner. This limit applies to every "spoofing" threat below and must not be papered over in code, UI text or docs.

---

## 1. Assets and security goals

| Asset | Goal |
|---|---|
| Account password (plaintext, transient) | Never stored/logged in clear; released only on a verified, policy-approved unlock (SEC-01, SEC-12). |
| Face templates / embeddings | Stored only as AEAD ciphertext; never auto-updated from face-only unlocks (SEC-02, SEC-15). |
| Raw frames / preview frames | RAM only, zeroized on every exit path, never persisted or logged — service **and** UI (SEC-02, FR-18). |
| Auth pipe secret release | One-shot, nonce-bound, peer-verified, ≤ 3 s, only to the initiating instance (SEC-03). |
| Policy decision | Owned solely by the service; CP, registry and UI cannot loosen it (SEC-07, SEC-19). |
| Availability of password/PIN fallback | Absolute; no CP filter, kill switch always works, UI optional at logon (SEC-20). |
| Binary/model integrity | Signed, absolute-path loaded, hash-pinned models (SEC-05, SEC-13). |

Priority from `prd.md` §1: **security → never lock the owner out → speed → footprint.** When a mitigation trades availability against security, availability of the password fallback wins.

---

## 2. Attacker model

| ID | Attacker | Capabilities | Out of scope for them |
|---|---|---|---|
| **ADV-1** | Casual bystander / opportunistic thief | Printed photo, phone showing a photo, shoulder-surfing; no account, no admin. | Cannot install software or read the disk offline. |
| **ADV-2** | Targeted knows-the-owner attacker | High-quality photo or live video replay on phone/tablet/monitor; may know the owner's appearance and schedule; brief physical proximity. | No local admin, no Windows credentials. |
| **ADV-3** | Unprivileged local process / malware under the owner's account | Runs as the owner, reads user files, can attach to pipes whose DACL allows the owner (none do), may try to impersonate the UI/CLI. | Cannot write HKLM / `C:\Program Files\Kenali\`, cannot read SYSTEM-only pipes. |
| **ADV-4** | Elevated local attacker / admin-level malware | Admin token; can write HKLM and Program Files, install drivers, swap camera hardware, sign nothing but can place files. | Cannot extract the TPM key (non-exportable); BitLocker assumed on. |
| **ADV-5** | Physical attacker with brief unattended access | Plug in/swap USB camera, cover or shine light on the camera, reboot, reach Safe Mode/WinRE with time. | No password; no persistent local presence. |
| **ADV-6** | Remote network attacker | RDP/remote session attempts, network scanning for the named pipes, exfiltration attempts. | No local console access; `PIPE_REJECT_REMOTE_CLIENTS` and no network code in our binaries. |
| **ADV-7** | Supply-chain / build-time attacker | Can tamper with models, binaries or install scripts before signing; controls a dependency or the build host. | Cannot forge the owner's Authenticode signature (self-signed but under the owner's control). |

Trust assumption: Windows, the TPM and LogonUI are trusted; BitLocker/Device Encryption is on (A5); TPM 2.0 + Secure Boot present (A6). A fully compromised SYSTEM is game over and is not defended against — the goal is to prevent *escalation to SYSTEM* and *theft of the password at rest* from the attackers above.

---

## 3. Threat register (TM-01 … TM-24)

Each row gives STRIDE category, affected component, attacker, mitigation mapped to `prd.md` SEC IDs, and residual risk. Detailed notes follow for the threats called out in the contract.

| ID | STRIDE | Component | Attack | Attacker | Mitigation (SEC) | Residual risk |
|---|---|---|---|---|---|---|
| **TM-01** | Spoofing | Service vision / PAD | Printed photo of the owner held to the camera | ADV-1, ADV-2 | SEC-08 (passive CNN + temporal + attention), SEC-09 | RGB passive PAD is imperfect; target APCER ≤ 1 %, never 0. Determined attacker with a high-quality print may pass. Honest limit. |
| **TM-02** | Spoofing | Camera + PAD + policy | Phone/tablet/monitor video replay; virtual camera feed | ADV-2, ADV-5 | SEC-08, SEC-16, SEC-18 | High-fidelity replay, masked or deepfaked faces remain a real risk; PAD camera/scene-dependent. Cold-boot face blocked by SEC-18. |
| **TM-03** | Tampering / Spoofing | Camera capture | Swap the physical camera or inject a virtual camera device | ADV-4, ADV-5 | SEC-16 (pin symbolic link + hardware ID at enrol; reject others) | Hardware/firmware-level replacement is out of scope; a change disables face unlock until re-enrolment. |
| **TM-04** | Spoofing | Service policy | Attempt face unlock at first sign-in after boot/restart | ADV-2, ADV-5 | SEC-18 (first sign-in always password) | Relies on correct session-state detection; WTS misreporting (R5) could misclassify — tested, generic fail-closed. |
| **TM-05** | Information disclosure | Camera + UI preview | Recover raw or preview frames from disk, logs or memory | ADV-3, ADV-4 | SEC-02, SEC-11 | No frames persisted or logged; RAM-only, zeroized. Admin with OS-level memory capture (ADV-4) out of scope. |
| **TM-06** | Tampering | Store / vision | Poison templates by auto-learning from a successful face-only unlock | ADV-2, ADV-3 | SEC-15 (no auto-update; enrol requires elevation + password + liveness) | Templates are AEAD-encrypted; attacker also needing the DEK makes this impractical without TPM compromise. |
| **TM-07** | Information disclosure | Service / store / UI | Recover the account password from plaintext storage, logs, errors or UI memory | ADV-3, ADV-4 | SEC-01, SEC-12, SEC-19 | Redacting newtypes, zeroize; **R14**：UI holds the password transiently in `String` during enrol/refresh. Windows itself holds the password during logon — accepted. |
| **TM-08** | Spoofing / Elevation of privilege | Auth pipe | Unauthorised process opens the auth pipe to request a secret | ADV-3, ADV-4, ADV-6 | SEC-03 (SYSTEM-only DACL, first-instance, reject remote, bidirectional peer verification, nonce-bound one-shot release) | Only SYSTEM can connect; a fully compromised SYSTEM is out of scope. |
| **TM-09** | Tampering / Elevation of privilege | Install path + model loading | Substitute a malicious DLL or model via relative-path/DLL hijack | ADV-4, ADV-7 | SEC-05, SEC-13 (admin-write-only path, absolute loading, `models.lock` SHA-256, signed binaries incl. UI) | ADV-7 before signing; self-signed certificate trust model — verify Smart App Control/WDAC state (R9, U7). |
| **TM-10** | Denial of service | Credential Provider | Crash/panic inside `LogonUI.exe` → lock-out | ADV-1–ADV-5 (trigger), accidental | SEC-06, SEC-13 (panic firewall on every COM entry, no ML/network/async, fail open) | Windows can still unload a CP; residual mitigated by kill switch, allowed-SID guard, VM-first rollout and recovery runbook (R1). |
| **TM-11** | Information disclosure | Tile + UI | Hill-climb similarity scores via on-screen numbers or verbose feedback | ADV-1, ADV-2, ADV-3 | SEC-09, SEC-14 (scores never rendered anywhere, generic messages, rate limiting/cooldowns, redacted audit) | Side channels (timing, camera LED, attempt counts) may leak coarse feedback; no scores exposed by design. |
| **TM-12** | Denial of service | CP + policy | Stale password / wrong-password / locked-out result loops account lockout | ADV-2, ADV-5 (trigger) | SEC-10 (one submit per attempt; never retry after wrong-password/locked-out/expired/must-change) | Depends on correct `NTSTATUS` detection; unknown statuses default to no-retry and disable. |
| **TM-13** | Elevation of privilege / Tampering | Admin pipe + registry | Policy bypass via UI or by writing registry state directly | ADV-3, ADV-4 | SEC-07, SEC-19 (typed schema-validated admin commands; registry only a disable switch; UI never writes HKLM) | An elevated admin can toggle the kill switch physically by design; policy engine must validate every typed command. |
| **TM-14** | Spoofing | Pipe clients (CP, UI, CLI) | Pipe squatting — fake service instance intercepts client | ADV-3, ADV-4 | SEC-03, SEC-04 (client compares server PID with SCM's PID for `KenaliSvc`; `FILE_FLAG_FIRST_PIPE_INSTANCE`) | Small race before the service starts; clients fail closed and surface "unavailable". |
| **TM-15** | Information disclosure / Spoofing | All programs | Network exfiltration or remote attack against our binaries | ADV-6 | SEC-11, SEC-02 (no networking crates; outbound firewall block for service and UI; `PIPE_REJECT_REMOTE_CLIENTS`) | No network code in our processes; OS/other-process traffic out of scope. |
| **TM-16** | Tampering / Elevation of privilege | All crates | Memory-safety bug / unsafe outside FFI / unsigned build | ADV-3, ADV-4, ADV-7 | SEC-13, SEC-05 (`forbid(unsafe_code)` outside 3 crates incl. UI, static CRT/CFG/CET, fuzzing, `deny`/`audit`, signed releases) | 0-day in a dependency; supply-chain compromise before pinning. |
| **TM-17** | Spoofing | Vision + policy | Coercion — attacker forces the enrolled owner to look at the camera | ADV-2, ADV-5 | SEC-08, SEC-18 (attention gating, policy cooldowns, no cold-boot face) | Cannot be fully prevented: the face is genuine. Documented as an inherent limit, not a bug. |
| **TM-18** | Spoofing | PAD / decision window | Bypass active challenge escalation on elevated risk | ADV-2 | SEC-08 (challenge escalation on low PAD margin, lighting anomalies, recent failures, long idle) | Threshold tuning; passive RGB signal may be weak. |
| **TM-19** | Information disclosure | UI preview path | Preview/enrolment frames persisted to disk, logs or crash dumps | ADV-3, ADV-4 | SEC-02, SEC-19, FR-18 (RAM-only `Zeroizing` buffers, raw RGB never encoded, cleared on wizard end/app exit, preview pipe closed outside windows) | OS paging / crash dumps of RAM; exclude secret/frame buffers from dumps where feasible. |
| **TM-20** | Information disclosure | Diagnostics export | Diagnostics ZIP leaks templates, embeddings, frames or password | ADV-3, ADV-4 | SEC-04, SEC-19 (redacted ZIP built in service, returned as bytes over admin pipe, service accepts no file paths, UI saves) | Redaction gaps if a new log field is added; log-hygiene test covers service, CP and UI. |
| **TM-21** | Information disclosure / Tampering | UI process | Secrets or policy state leak via UI logs, errors or process memory | ADV-3, ADV-4 | SEC-19, SEC-12 (no secret in UI logs/state/errors; no auth-pipe types linked into UI crate; signed process, no relative-path loads) | **R14** — transient `String` password fields; cleared after submit. |
| **TM-22** | Spoofing / Elevation of privilege | UI (Rev 0.2 attack surface) | Malicious or compromised UI reaches secrets/auth pipe, or issues unauthorised mutations | ADV-3, ADV-4 | SEC-19, SEC-04, SEC-20 (UI has no auth-pipe access; typed commands validated by service; signed binary path check; UI optional at logon) | An elevated attacker can always run their own signed-or-not binary, but the admin pipe verifies image path/signature and the UI still never receives secrets. R14. |
| **TM-23** | Spoofing | Admin pipe client verification | Attacker impersonates `kenali-ui.exe` to send admin commands | ADV-3, ADV-4, ADV-7 | SEC-04, SEC-05, SEC-19 (elevated token + client image path under versioned install dir + Authenticode check; same DACL as admin pipe) | Self-signed cert trust (R9); if the signing key is stolen, path check must still hold. |
| **TM-24** | Elevation of privilege | CP scenario handling | Provider offered in unintended scenario (RDP, CredUI, change-password, PLAP) | ADV-6, accidental | SEC-17 (only `CPUS_UNLOCK_WORKSTATION` on by default; `CPUS_LOGON` opt-in; decline others with `E_NOTIMPL`) | `CPUS_LOGON` misreporting on Windows 10+ (R5); WTS session-state disambiguation tested. |

---

## 4. Detailed notes for contract-required threats

**TM-01 / TM-02 — Spoofing (photo and replay).** The primary attack class. Mitigations: passive MiniFASNet PAD fused with temporal checks, attention gating (eyes open, frontal within ±20°), multi-face rejection, challenge escalation, and policy rules (password after boot, 24 h knowledge-factor rule, cooldowns). Mapped to SEC-08/SEC-09. Residual: a single RGB camera sees a 2-D projection; the honest RGB-vs-IR limit above is the governing statement, and PAD APCER ≤ 1 % is a target, not a guarantee.

**TM-03 — Camera swap.** Enrolment pins the camera's symbolic link + hardware ID (SEC-16). Any different device disables face unlock until re-enrolment. Virtual cameras are rejected. Residual: a hardware/firmware-level replacement that presents the same identifiers is out of scope for personal use.

**TM-08 / TM-14 — Pipe squatting and unauthorised clients.** Auth pipe DACL is SYSTEM-only; admin/preview pipes are SYSTEM + elevated Administrators with client image-path verification (SEC-03/SEC-04). Both CP and UI/CLI compare the pipe server PID with the PID SCM reports for `KenaliSvc`. `FILE_FLAG_FIRST_PIPE_INSTANCE` blocks a second listener. Clients fail closed. Residual: a pre-start race yields "unavailable", never a secret.

**TM-12 — Stale password / logon failure.** After `STATUS_WRONG_PASSWORD`, `STATUS_LOGON_FAILURE`, `STATUS_ACCOUNT_LOCKED_OUT`, `STATUS_ACCOUNT_DISABLED`, `STATUS_PASSWORD_EXPIRED` or `STATUS_PASSWORD_MUST_CHANGE`, face unlock disables and never retries (SEC-10). The UI shows "Paused — credentials changed" and offers the Refresh credentials wizard. Kenali itself can never trip account lockout.

**TM-11 — Score hill-climbing.** No similarity score is rendered on the tile **or anywhere in the UI**; failure wording is generic; rate limits and cooldowns cap attempts (SEC-09/SEC-14). Residual: timing and camera-LED side channels leak only coarse success/failure, which the tile already conveys.

**TM-22 — UI attack surface (SEC-19).** Rev 0.2 moved every operator workflow into `kenali-ui.exe`, which enlarged the interactive attack surface. Compensating controls: the UI has no auth-pipe handle, no templates, no embeddings, no sealed secret; all mutations are typed admin-pipe commands validated by the service's policy/schema engine; the UI never writes HKLM state; preview frames are RAM-only; the binary is signed and loads nothing by relative path; the UI is optional at logon (SEC-20). Residual: R14 (transient password in `String` fields). See also TM-19/TM-21/TM-23.


**TM-19 — Preview-frame persistence.** Downscaled 320×240 RGB frames travel over the preview pipe only while an enrolment/calibration window is open. They exist in `Zeroizing<Vec<u8>>` slots in both the service and the UI, are never encoded (no JPEG/PNG on the wire) and never written to disk or logs. The preview pipe is closed on `EndPreview`, wizard end or app exit; buffers are zeroized on every exit path (SEC-02, SEC-19, FR-18). Residual: OS paging and crash dumps can capture RAM; exclude frame buffers from dumps where feasible.

**TM-13 — Policy bypass via UI.** All UI mutations are typed admin-pipe commands; the service validates every one against the config schema and the UI never writes HKLM state (SEC-07, SEC-19). The registry holds only a disable switch that can tighten but never loosen. Residual: an elevated admin (ADV-4) can always toggle the kill switch by hand — by design; the policy engine still cannot be weakened through the UI.

**TM-10 — CP crash / lock-out.** Every COM entry point is wrapped in `catch_unwind` and returns an `HRESULT`; the CP has no ML, network or async runtime (SEC-06, SEC-13). It fails open to the password/PIN providers, which are never filtered, hidden or delayed (SEC-20). Residual: Windows may still unload the CP; mitigated by the kill switch, allowed-SID guard, VM-first rollout and rehearsed recovery drills (R1).

---

## 5. Risk R14 — UI holds the password briefly in `String` fields (accepted)

**Statement.** egui text fields are `String`-based. During the **Enrol** and **Refresh credentials** wizards the account password necessarily exists in the `kenali-ui.exe` process as a plaintext `String` for the short interval between the user typing it and submitting it. We do not pretend otherwise.

**Documented mitigation.**
- On submit, the `String` buffer is **moved into the typed admin-pipe message** (`EnrolPassword(PasswordWide)` / `RefreshCredential(PasswordWide)`) and the source buffer is **cleared immediately** — no clone, no retention in UI state.
- The password is **never logged**, never placed in an error message, never included in a `Debug`/`Display` output, and never persisted to disk or in the diagnostics ZIP. Log-hygiene tests cover the UI process as well as the service and CP (FR-12, FR-19).
- The password is never sent to the auth pipe and is never made available to the UI beyond the wizard; the service validates it with `LogonUser` and seals it to the TPM before discarding it.
- The UI is an **elevated, signed process** loaded from the admin-write-only `C:\Program Files\Kenali\<version>\` path, with no relative-path loads (SEC-05, SEC-19).

**Why accepted.** This is the same fundamental exposure Windows itself accepts: Windows holds the account password in memory during logon (and LSA holds secrets for the session). Eliminating the `String` would require replacing egui's text field with a custom secure-edit control — a large, risky change for a single-user personal tool. The residual window is small, local, and requires an attacker already able to read process memory of an elevated, signed process (ADV-4) — a capability that already defeats most application-level defences.

**Residual risk.** Medium likelihood / medium impact (R14). An attacker with local admin or a memory-read primitive could scrape the plaintext during the brief entry window. Accepted. Revisit only if the GUI framework changes (then prefer a secure edit control) or if the threat model's attacker set expands to include privileged local memory capture as in scope.

Cross-references: `prd.md` §14 R14, §9 SEC-12/SEC-19, FR-19; `context_pack.md` §2 item 19, §3.8; threats TM-07, TM-21, TM-22.

---

## 6. Honest RGB-vs-IR limit

A single RGB camera cannot match an IR/depth system such as Windows Hello or Face ID. Microsoft's face stack uses a near-infrared camera partly because IR does not appear in photographs or on LCD screens [R1], and since 2025 also requires a colour frame to counter an IR-spoofing technique [R4]. Kenali has no IR sensor, no depth map and no structured light. Its PAD is passive RGB, which is camera- and scene-dependent, plus temporal and policy checks.

Consequently:
- The FAR target (≤ 0.001 %) and PAD targets (APCER ≤ 1 % per species) are **targets to be measured and reported**, not achieved guarantees on unknown hardware.
- Kenali resists **casual** spoofing (printed photo, phone/tablet replay) with layered compensating controls; it is not a high-assurance biometric.
- No code comment, UI string or document may claim parity with Windows Hello. The UI's test/bench screen states that dry runs never unlock and shows no scores.

This limit is a property of the hardware, not a defect that can be fixed in software; every spoofing threat (TM-01, TM-02, TM-17, TM-18) carries it as residual risk.

---

## 7. Traceability summary

| Area | Threats | Primary SEC controls |
|---|---|---|
| Presentation attacks | TM-01, TM-02, TM-17, TM-18 | SEC-08, SEC-09, SEC-16, SEC-18 |
| Camera integrity | TM-02, TM-03 | SEC-16 |
| Secrets at rest / in memory | TM-05, TM-06, TM-07, TM-19, TM-20, TM-21 | SEC-01, SEC-02, SEC-11, SEC-12, SEC-15 |
| IPC / pipes | TM-08, TM-14, TM-23 | SEC-03, SEC-04, SEC-05 |
| Availability / lock-out | TM-10, TM-12 | SEC-06, SEC-10, SEC-13, SEC-20 |
| Policy / UI attack surface (SEC-19) | TM-13, TM-22 | SEC-07, SEC-19, SEC-20 |
| Build / supply chain | TM-09, TM-16 | SEC-05, SEC-13 |
| Scenario confusion | TM-24 | SEC-17 |
| Network | TM-15 | SEC-11 |

Any change to the attack surface must update this file; any architectural change must add an ADR in `architecture.md` (working agreement, `context_pack.md` §11).
