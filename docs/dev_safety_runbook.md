# Developer Safety Runbook — Kenali

> **Read before touching install, registration, registry or recovery.** This runbook exists for one reason: priorities in `prd.md` §1 put *never locking the owner out* second only to security. A faulty Credential Provider (CP) is loaded by `LogonUI.exe` and can prevent sign-in. Every step here is designed so that failure is reversible in minutes.
>
> Companion sources: `context_pack.md` §2 item 2, §3.7, §10; `prd.md` §6 F5/F6, §13; references [R6][R7][R12][R13][R14].

---

## 0. The absolute rule for AI agents on a non-Windows host

> **AN AI AGENT WORKING ON A NON-WINDOWS HOST (e.g. this WSL2 Linux builder) MUST NEVER RUN INSTALL OR REGISTRATION COMMANDS.**
>
> It produces **scripts and step-by-step manual instructions only.** It never writes Windows registry state, never copies a DLL into `System32`, never calls `reg add`, `regsvr32`, `New-Service`, `sc create`, or `cargo xtask install`, and never touches a Windows machine it is not explicitly told to touch. The owner alone runs these commands, by hand, **inside the VM first**. This is non-negotiable (context_pack §2 item 2, §14 bootstrap prompt, BRIEF §4).
>
> Nothing in this document authorises a remote or automated install. The gate for an agent's work is "script written + manual steps documented", never "it ran on Windows".

---

## 1. VM-first install ladder (the core safety rule)

**Never register the CP on the dev host's main account first.** The order is fixed:

1. **VM** — a disposable Windows 11 x64 virtual machine with a checkpoint taken *before* any install. Do every first-time install, every CP registration, every upgrade here.
2. **Secondary host account** — a second **local administrator** account on the physical laptop. After the VM passes, register there and soak-test. The main account is untouched. The allowed-SID guard (FR-16) means the tile only appears for the enrolled test SID.
3. **Main account** — only after the VM and the secondary-account soak both pass (prd §13). This is the last step, never the first.

Rules that apply at every rung:

- Take/checkpoint a known-good restore point before registration.
- Keep a second admin account with a working password available at all times (assumption A8).
- The default password/PIN providers are **never** filtered, hidden, replaced or delayed (context_pack §2 items 1, non-goal in prd §5). No `ICredentialProviderFilter` in v1.
- Registration uses a **freshly generated CLSID** committed as a constant (`KENALI_CP_CLSID`), never one copied from a blog (context_pack §3.1, §12).
- Install into a versioned, admin-write-only path `C:\Program Files\Kenali\<version>\`, with absolute-path loading. Never edit a registered DLL in place (context_pack §12).
- `cargo xtask install --vm` and `--host-test-account` refuse to run unless `KENALI_ALLOW_INSTALL=1` and print this runbook's pre-flight checklist first (context_pack §10; implementation plan shared contract).

If any rung produces a lock-out or a surprise, **stop** and do not advance to the next rung. Recovery is §5–§8 below.

---

## 2. Pre-flight checklist (collect BEFORE any Windows work)

Record the reference-laptop facts below in the spike report (`docs/spikes/P0-Sx.md`, template in context_pack §11). Run these in an elevated PowerShell on the *target VM/account*, never on the agent's host:

| # | Check | Command / location | Why it matters |
|---|---|---|---|
| 1 | OS build / feature release | `winver` | CP behaviour and scenario reporting differ per build (prd A1, R5). Confirm current + previous Win11 feature release. |
| 2 | TPM present & ready | `Get-Tpm` | TPM 2.0 assumed (A6). Wrapping needs a usable TPM; unwrap latency is unknown (U6) — measure in P0-S4. |
| 3 | BitLocker / Device Encryption | `manage-bde -status` | Offline disk safety (A5). Without it, TPM wrapping is defence-in-depth only. |
| 4 | Secure Boot | `Confirm-SecureBootUEFI` | Part of the hardware trust story (A6); affects Smart App Control. |
| 5 | Camera VID/PID + formats | `Get-PnpDevice -Class Camera,Image`; Device Manager → camera → Details → *Hardware Ids* | Pin the physical camera (SEC-16); camera readiness after resume is U11. |
| 6 | CPU | `Get-CimInstance Win32_Processor` | ≥ 4 cores assumed (A7); sets `min(4, physical cores)` inference threads and NFR-02 baseline. |
| 7 | Smart App Control state | Windows Security → App & browser control → *Smart App Control* | Can block unsigned binaries (R9). Self-signed build needs a decision here. |
| 8 | Account type | Settings → Accounts → local vs. Microsoft; confirm the account **has a password** | CP must submit a real password; PIN cannot be submitted [R17] (A4). Microsoft "Hello-only" setting can hide the password path (R10). |
| 9 | GPU / GL availability | VM display adapter; run the egui `glow` renderer smoke test | GL on Windows VMs is not guaranteed (U12); `wgpu` software adapter or the native Win32 front end is the fallback. |
| 10 | Windows Hello face | Settings → Accounts → Sign-in options → *Facial recognition* | If Hello face already works, **use it instead** (A2) — it is stronger. |
| 11 | Restore point / VM checkpoint | Disk Management / hypervisor checkpoint | Mandatory rollback safety before step 1 of the ladder. |

Pre-flight is printed by the `xtask install` gate, but the owner must actually read and satisfy it. Do not proceed with any registration until every row is recorded.

---

## 3. The kill switch (fastest disable, no reboot of the OS)

The kill switch makes the CP return `E_NOTIMPL` at the **next session creation** (FR-10). It never loosens security; it only disables Kenali. Two registry values, both `REG_DWORD` = 1:

```reg
HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{CLSID}\Disabled = 1
HKLM\SOFTWARE\Kenali\Disabled = 1
```

- The first is the per-provider switch Microsoft documents: `Disabled=1` under the provider's CLSID key in `...\Authentication\Credential Providers\` takes effect at the next session creation [R12] (context_pack §3.7). This stops `LogonUI` loading our tile.
- The second is Kenali's own switch. The CP reads it and returns `E_NOTIMPL`; the UI shows it read-only. The service/policy engine never loosens because of it (SEC-07).

Manual steps (owner, elevated PowerShell / regedit on the target machine):

1. Open an elevated PowerShell.
2. Set the provider switch, substituting the committed `KENALI_CP_CLSID`:
   ```powershell
   $clsid = '<KENALI_CP_CLSID from crates/kenali-winapi/src/com.rs>'
   New-Item -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$clsid" -Force
   New-ItemProperty -Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\$clsid" -Name Disabled -PropertyType DWord -Value 1 -Force
   ```
3. Set Kenali's own switch:
   ```powershell
   New-Item -Path 'HKLM:\SOFTWARE\Kenali' -Force
   New-ItemProperty -Path 'HKLM:\SOFTWARE\Kenali' -Name Disabled -PropertyType DWord -Value 1 -Force
   ```
4. Lock or sign out and back in (no full OS reboot needed for the CP change) — the tile must be gone and the password/PIN tile must still work.

To re-enable, delete both `Disabled` values (or set them to 0) and lock again. Only the owner, on the target machine, does this.

---

## 4. Recovery drills R-A … R-E

Each drill is executed **once successfully by the owner in the VM** before v1 sign-off (prd §13). An agent never executes them. Record date, machine, and result in `docs/reports/` or a spike report.

### R-A — Normal password fallback
**Goal:** any failure or service-down state leaves the password/PIN path working instantly (FR-03).
1. In the VM, sign in normally with the account password.
2. Kill `kenali-svc.exe` (Task Manager → Details → End task) or stop the service with `sc stop KenaliSvc`.
3. Lock the screen (`Win+L`). The tile must read "Face unlock unavailable" within ≤ 300 ms.
4. Type the account password at the normal password tile → desktop reached.
5. **Pass condition:** password works immediately, no delay, no face tile blocking it.

### R-B — Kill switch
**Goal:** the registry kill switch removes the tile at next session creation (FR-10).
1. Follow §3 to set both `Disabled=1` values.
2. Lock / sign out and back in.
3. **Pass condition:** no Kenali tile; password/PIN works; UI shows kill-switch state read-only. Re-enable afterwards and confirm the tile returns.

### R-C — Safe Mode
**Goal:** Safe Mode boots with only the built-in password provider, so a broken CP cannot lock the owner out [R12][R13] (context_pack §3.7).
1. From Settings → System → Recovery → *Advanced startup → Restart now*, or hold `Shift` while choosing Restart.
2. Troubleshoot → Advanced options → Startup Settings → Restart → press **4** (or `F4`) for Safe Mode.
3. Sign in at the plain built-in username/password provider.
4. **Pass condition:** sign-in succeeds with Kenali's CP present-but-inert; the default password provider is the only one offered.
5. Return to normal boot.

### R-D — WinRE (when Safe Mode is unreachable)
**Goal:** disable the CP offline via the Windows Recovery Environment command prompt [R6][R13].
1. Boot to WinRE (Shift+Restart, or a Windows installer USB → *Repair your computer*).
2. Choose Troubleshoot → Advanced options → **Command Prompt**.
3. Identify the Windows partition (often `C:` in WinRE, but verify): `dir C:\Windows` / `bcdedit /enum`.
4. Load the offline SYSTEM and SOFTWARE hives (SYSTEM is needed because the CP key can be under the `ControlSet` tree):
   ```
   reg load HKLM\KENALI_OFFLINE_SYSTEM C:\Windows\System32\config\SYSTEM
   reg load HKLM\KENALI_OFFLINE_SOFTWARE C:\Windows\System32\config\SOFTWARE
   ```
5. Set `Disabled=1` on the provider CLSID under the offline SOFTWARE hive. Because the exact ControlSet can vary, also cover the well-known path:
   ```
   reg add "HKLM\KENALI_OFFLINE_SOFTWARE\Microsoft\Windows\CurrentVersion\Authentication\Credential Providers\{CLSID}" /v Disabled /t REG_DWORD /d 1 /f
   reg add "HKLM\KENALI_OFFLINE_SOFTWARE\Kenali" /v Disabled /t REG_DWORD /d 1 /f
   ```
6. **Alternative if hive editing is uncertain — delete/rename the DLL** (the CP simply fails to load, password provider remains):
   ```
   ren C:\Program Files\Kenali\<version>\kenali-cp.dll kenali-cp.dll.disabled
   ```
7. Unload the hives and reboot:
   ```
   reg unload HKLM\KENALI_OFFLINE_SOFTWARE
   reg unload HKLM\KENALI_OFFLINE_SYSTEM
   exit
   ```
8. **Pass condition:** normal boot presents the password provider; sign-in succeeds; Kenali tile absent.

> **VERIFY** the exact offline hive path/ControlSet number on the reference build before relying on step 5. Renaming the DLL (step 6) is the robust fallback and is sufficient to recover.

### R-E — Rollback via versioned paths
**Goal:** a bad upgrade is reversible without touching the registry by hand (FR-15, context_pack §12).
1. Installs live at `C:\Program Files\Kenali\<version>\`; the previous version's directory is kept.
2. Run the developer tool `cargo xtask rollback` (owner, in the VM; never the agent), which rewrites the `InprocServer32` default value to the previous version's absolute DLL path and restores the prior registration.
3. Lock / sign out and back in; confirm the previous version's tile loads.
4. **Pass condition:** previous version works; no in-place edit of a registered DLL was performed. If rollback fails, fall back to R-D (rename DLL / offline disable).

---

## 5. KB 4073719 and the first-logon-after-restart workaround

Microsoft **KB 4073719**: custom credential providers may fail to load at the **first logon after a restart** when Windows auto-completes sign-in [R14] (context_pack §3.7, prd R7). This is consistent with our policy that the first sign-in after boot always requires the password (SEC-18).

**Workaround — `DisableAutomaticRestartSignOn`:**
1. Open an elevated PowerShell / regedit on the target machine.
2. Set:
   ```powershell
   New-Item -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' -Force
   New-ItemProperty -Path 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System' -Name DisableAutomaticRestartSignOn -PropertyType DWord -Value 1 -Force
   ```
   (Equivalent Group Policy: *Computer Configuration → Administrative Templates → Windows Components → Windows Logon Options → "Sign-in and lock last interactive user automatically after a restart"* = Disabled.)
3. Reboot once; the first logon after restart now shows the normal sign-in screen instead of auto-completing.
4. **Expected:** the CP does not load on that first post-boot sign-in; password is required. Later unlocks load the tile normally. Document this as accepted behaviour, not a bug.

---

## 6. Residual risk: password present in the GUI process

The management UI (`kenali-ui.exe`) holds the account password **briefly** in the enrol/refresh wizards because egui text fields are `String`-based (prd R14, SEC-19, context_pack §3.8). Accepted mitigation: the buffer is moved into a typed admin-pipe message and cleared immediately after submit; never cloned, logged or persisted. This is a documented accepted residual risk — Windows itself holds the password in memory during logon. It does **not** affect logon-side recovery: the UI is optional at logon (SEC-20), so a broken UI never contributes to a lock-out.

---

## 7. Quick reference — "I am locked out"

1. Stay calm. The password/PIN providers are never filtered — try the normal password tile first (R-A).
2. Tile still present but wrong? Lock and use the §3 kill switch, then sign out/in (R-B).
3. Cannot reach the desktop? Boot to Safe Mode (R-C) — only the built-in password provider loads.
4. Cannot reach Safe Mode? Boot to WinRE and disable the CP offline or rename the DLL (R-D).
5. Bad update? `cargo xtask rollback` to the previous versioned path (R-E).
6. First logon after a restart looks wrong? Apply `DisableAutomaticRestartSignOn` (§5).
7. Never delete the second admin account, never remove the restore point/VM checkpoint.

---

## 8. Definition of done for any change touching install / registry / recovery

From context_pack §11: the runbook is updated, the affected drill (R-A…R-E) is **re-run in a VM** by the owner, and the result is recorded. No hardware/VM test is claimed by an agent — only the owner runs these. This document records the *plan and manual steps*; execution evidence lives in spike/report docs.
