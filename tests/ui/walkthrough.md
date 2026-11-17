# End-to-end UI walkthrough — `kenali-ui.exe` (no terminal)

**Status:** SCRIPT ONLY — not executed. Every runtime result below is left as `VERIFY` / `OWNER to fill`.

This document is the Phase 5 walkthrough required by `docs/prd.md` §13 and `docs/test_plan.md` §11. It is a step-by-step script for the owner to run **without a terminal**: every operator action is performed in the elevated `kenali-ui.exe` management GUI, and the lock-screen tile is observed directly. The recovery CLI (`kenali.exe`) is deliberately not used anywhere in this walkthrough (SEC-20; the UI is optional at logon, never required).

---

## 1. Boundary — what this document does and does not claim

- **Nothing here was executed on the host that authored this file.** The authoring host is WSL2 Linux; Windows crates are type-checked only. No Windows GUI, UAC prompt, service installation, registry write, camera open or CP registration has been run here.
- **Every step is owner-run on Windows**, following the mandatory VM-first ladder in `docs/dev_safety_runbook.md` §1, in this order:
  1. **VM** — disposable Windows 11 x64 VM with a checkpoint taken before any install. Run the whole walkthrough here first.
  2. **Secondary host account** — a second local administrator account on the physical laptop. Main account untouched; the allowed-SID guard (FR-16) keeps the tile off the main account.
  3. **Main account** — only after the VM and secondary-account passes both hold.
- Each step has result fields `Result: VERIFY` and `Evidence: OWNER to fill`. Record actual observed values on the target machine; "VERIFY" means "the owner must confirm this on that machine", not "confirmed".
- The UI talks only to the service over the **admin pipe** (plus a short-lived preview pipe during capture windows). It never opens the auth pipe, never touches templates/embeddings/sealed secret, and never writes Kenali's HKLM state (SEC-19, SEC-07).

---

## 2. Prerequisites (owner, on each target in the ladder)

Before opening the UI, the owner must satisfy and record, on the *target* VM/account:

1. **Windows 11 x64** at a supported feature release (current or previous). `winver`.
2. **`docs/dev_safety_runbook.md` §2 pre-flight checklist completed** and recorded in a spike report — OS build, TPM 2.0 ready (`Get-Tpm`), BitLocker/Device Encryption status (`manage-bde -status`), Secure Boot, camera VID/PID (pin target), CPU, Smart App Control decision, account type with a real password, GPU/GL (`glow`) smoke test, Windows Hello face decision, restore point / VM checkpoint. Do not proceed until every row is recorded.
3. **Elevated `kenali-ui.exe`** — the process requests elevation at launch and produces **one UAC prompt**. Accept it. Single-instance: a second launch focuses the existing window rather than opening another.
4. **Service `KenaliSvc` running** and its **admin pipe available** (Administrators-only, signed-binary peer check). The UI cannot function without it, and its absence never affects unlock/fallback/recovery.
5. **TPM / BitLocker / camera prerequisites** from the pre-flight are present. Physical camera is pinned; a different camera disables face unlock until re-enrolment (SEC-16).
6. **Restore point / VM checkpoint present** (rollback safety) before any enrolment or config change.

Record at start — `Result: VERIFY`, `Evidence: OWNER to fill`:
- Target in ladder (VM / secondary / main): `VERIFY`
- Pre-flight spike report path: `OWNER to fill`
- `KenaliSvc` state: `VERIFY`  ·  Admin pipe reachable by UI: `VERIFY`

---

## 3. Global UI conventions (apply to every STEP)

- **Expected wording** comes only from `kenali_core::strings`: "Face unlock ready", "Looking for you…", "Look at the screen", "Move a little closer", "Too dark", "Blink twice", "Turn your head left", "Welcome", "Use your password or PIN", "Face unlock paused", "Face unlock unavailable", and the `not_recognised(tries_left)` tile. Do not expect any other vocabulary.
- **No numeric match values are displayed anywhere in the UI** — not on the tiles, not in the wizard, not in Settings, not in Diagnostics or Test/Bench. No PAD margins are shown either. PAD *thresholds* may appear as read-only values with a calibration date, but there is no live numeric match readout. If any numeric match value appears, treat it as a defect and stop.
- **Password field buffers are cleared immediately after submit** (accepted residual risk R14 / SEC-12): the masked field is moved into a typed admin-pipe message and cleared; it is never cloned, logged or persisted. Correctness of this behaviour is a code-review item, not something to verify by reading on-screen text.
- **Long actions show progress + cancel.** Cancel is available at every step of the enrol/refresh wizards and wipes preview buffers on cancel, error and exit.
- **Previews are RAM-only** (FR-18): 320×240 RGB frames streamed only during capture windows, rendered inline, never persisted or logged.
- Step headers use the `STEP|Step` prefix (for example "STEP 1"); every one follows the same four-part shape: expected wording, what to observe, how to record evidence, how to cancel / roll back.
- Evidence convention for each STEP: screenshot of the relevant UI state (screenshots contain no secrets), plus a short note of the exact wording observed and any event-log entry. Store evidence outside any directory the service writes to.

---

## STEP 1 — Launch and Dashboard

**Goal:** open the single elevated window and confirm the Dashboard reflects real service/hardware state.

1. Launch `kenali-ui.exe`. Accept the single UAC elevation prompt.
2. Observe one Dashboard window. Open a second launch attempt: it must focus the existing window, not create a second one.
3. On **Dashboard**, observe and record:
   - **Service state** badge — service `KenaliSvc` running/reachable over the admin pipe.
   - **Camera status** badge — camera present and pinned.
   - **TPM** and **BitLocker** badges — both green if prerequisites are met (amber/red = fix pre-flight before enrolling).
   - **Enrolled SID** — shown read-only; matches the account you are testing in this rung of the ladder.
   - **Kill-switch status** — read-only. Observe the state the owner set for this drill; the UI must not offer a control that writes it (SEC-07, FR-10).
   - Face-unlock on/off and policy-state summary, e.g. "Face unlock paused — credentials changed", plus recent redacted Event Log entries.
   - Cold start to interactive Dashboard should feel immediate (NFR-10 target ≤ 2 s).

**Expected wording:** Dashboard mirrors tile vocabulary; when disabled it shows "Face unlock paused".
**Observe:** badge colours, SID text, read-only kill-switch, absence of any numeric match value.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (Dashboard screenshot with SID and badges).
**Cancel / roll back:** close the window normally; no state changed on launch. A failed launch (UAC declined, service down) leaves the system untouched.

---

## STEP 2 — Enrol via the wizard (F1)

**Goal:** enrol ≥ 12 templates across ≥ 2 lighting conditions, validate the password with the service, seal to TPM, self-test.

Dashboard → **Enrol wizard**. Cancel is available at every sub-step; cancelling wipes the RAM-only preview buffer.

1. **Prerequisite checks.** Green/red badges for TPM, BitLocker, camera, service. All must be green to continue; a red badge is a blocker, not a warning to bypass.
2. **Masked password field.** Type the account password. It is masked. Press Continue → the **service** validates it (LogonUser). Wrong password → inline error; nothing is sealed. There is no retry limit, but a wrong password never reaches the seal step. Confirm the field clears after submit (R14).
3. **Guided capture with live preview.** A live RAM-only preview renders inline (320×240). Follow pose guidance in order: straight, small left turn, small right turn, up, down; blink twice on challenge. If "Too dark", improve lighting. Per-pose progress counts samples (e.g. `7/12`). You must reach **≥ 12 templates across ≥ 2 lighting conditions** (change the room lighting partway through and record it). No frame is written to disk.
4. **Seal to TPM.** On completion the service seals the secret to the TPM. Observe progress; do not close the app mid-seal.
5. **Self-test.** Observe the self-test result (generic wording, no numeric match value).
6. **Summary.** Record template count, the conditions captured, and calibration date shown.

Deliberately exercise cancel once: start the wizard, reach capture, hit **Cancel**, and confirm the preview buffer is wiped (the view clears; no persisted image appears anywhere). Then re-run to completion.

**Expected wording:** "Look at the screen", "Move a little closer", "Too dark", "Blink twice", "Turn your head left"; self-test/summary use generic wording only.
**Observe:** live preview present only during capture; pose guidance; progress counter; ≥ 12 / ≥ 2 conditions; inline wrong-password error; no on-disk frames; no numeric match value.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (summary screenshot, count + conditions; note the cancel-and-wipe observation).
**Cancel / roll back:** Cancel at any step — nothing sealed, preview wiped. If sealing already happened, use Templates → Delete for the just-added templates, or Security → Disable while you re-run enrol.

---

## STEP 3 — Change a setting (Settings screen)

**Goal:** change one scope/toggle and confirm it is a typed admin command applied by the service.

Settings → change a **scope toggle** (unlock on/off, sign-in on/off), an **attention angle** (yaw/pitch), or a **failure cooldown**. The UI sends a typed config key to the service's admin pipe; the service schema-validates it. The UI never writes HKLM state (SEC-07).

1. Change one value; observe the UI reflect the applied value.
2. **PAD thresholds are read-only** — shown with a calibration date; attempt to edit one and confirm no edit control is offered.
3. Pinned camera device info is shown read-only.
4. For a scope change (e.g. `scope.logon=on`), confirm at next session creation that the service applied it.

**Expected wording:** generic settings labels; no numeric match value anywhere.
**Observe:** value persists after re-opening Settings; read-only thresholds are not editable; no registry tool was used.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (before/after Settings screenshot; note the key changed).
**Cancel / roll back:** re-open Settings and restore the original value through the same UI control (another typed admin command). No terminal needed.

---

## STEP 4 — Add a template (Templates screen)

**Goal:** add one more template via the mini capture flow.

Templates → **Add template**. The mini capture flow reuses the enrol wizard's capture step: masked password validation (R14 clearing applies), live RAM-only preview, pose guidance, quality gate.

1. Complete one guided capture; observe the new entry appear in the list with capture date, lighting condition and glasses flag.
2. Confirm the template count increased by one on the summary/Dashboard view.

**Expected wording:** "Look at the screen", "Move a little closer", "Too dark", "Blink twice", "Turn your head left".
**Observe:** new row present with metadata; preview only during the capture window; no numeric match value.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (Templates list screenshot before and after).
**Cancel / roll back:** Cancel before completion — no new template added, preview wiped. After completion, use STEP 5 to delete the added row.

---

## STEP 5 — Delete a template with confirm (Templates screen)

**Goal:** remove the template added in STEP 4, through a confirmation step.

Templates → select the target row → **Delete**.

1. A confirmation prompt must appear; dismissing it must leave the row intact.
2. Confirm the deletion; observe the row disappear from the list and the count decrease.
3. Confirm no numeric match value appears in the list or confirmation dialog.

**Expected wording:** generic confirmation text; no numeric match value.
**Observe:** confirm-cancel keeps the row; confirm-accept removes it.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (list screenshot before confirm, after cancel, after accept).
**Cancel / roll back:** use the confirmation dialog's cancel to abort — no change. Deletion is intentional; to restore, re-run STEP 4 (templates are not auto-updated from face unlocks, SEC-15).

---

## STEP 6 — Export diagnostics (Diagnostics screen)

**Goal:** export the redacted ZIP with the UI choosing the save location; the service returns bytes over the pipe.

Diagnostics → **Export redacted ZIP**.

1. The UI opens a **save-location picker**; choose a destination. The service is given **no file path** — it returns redacted bytes in chunks over the admin pipe, and the UI writes the file to the chosen location (FR-14, FR-09).
2. Observe progress for the chunked transfer; cancel is available.
3. Inspect the saved ZIP (owner, outside this no-terminal walkthrough as a verification artifact): confirm it contains **no templates, embeddings, passwords or frames** and no numeric match values.
4. Also note Diagnostics shows the filtered Event Log viewer, log-file location note, and service PID/version.

**Expected wording:** generic progress/finish wording; no numeric match value.
**Observe:** save picker appears; no path is sent to the service; ZIP content redacted.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (Diagnostics screenshot; note save location and redaction inspection result).
**Cancel / roll back:** Cancel during transfer — partial output may exist at the chosen location; delete it via the file manager. No service state changes.

---

## STEP 7 — Refresh credentials after a password change (Security screen)

**Goal:** clear the stale-credential auto-disable by re-validating and resealing (F4, R-E).

First simulate the trigger: change the account password (owner, on this test account) so the next face unlock returns a wrong-password status. Face unlock auto-disables with **no retries**; sign in with the new password; the Dashboard/Security banner shows "Face unlock paused — credentials changed".

Security → **Refresh credentials** wizard:

1. Prerequisite badges (if shown) green.
2. Enter the **new** password in the masked field; the service validates it. Wrong password → inline error, no reseal. Confirm the field clears after submit (R14).
3. Service reseals the secret to TPM.
4. Confirm the stale-credential auto-disable is cleared and face unlock returns to "Face unlock ready".

**Expected wording:** "Face unlock paused" / "Face unlock paused — credentials changed" before, "Face unlock ready" after.
**Observe:** no retry after wrong password; reseal completes; banner clears; no numeric match value.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (banner before, ready state after).
**Cancel / roll back:** Cancel before reseal — face unlock stays paused (password/PIN still works). Re-run the wizard to completion. If locked out, use the runbook R-E recovery drill (owner-only, separate from this walkthrough).

---

## STEP 8 — Disable / Enable face unlock (Security screen)

**Goal:** pause and resume face unlock from the UI; observe the read-only kill-switch state.

Security → **Disable**:

1. Observe "Face unlock paused". Lock the session and confirm it still signs in with the password; the tile uses "Face unlock paused". The password/PIN provider is never filtered or delayed (FR-03).
2. Observe the **kill-switch status** is read-only here — the UI explains how to use the switch but offers no control that writes it (SEC-07, FR-10).
3. Security → **Enable**: confirm face unlock returns and the tile reads "Face unlock ready".

**Expected wording:** "Face unlock paused", "Use your password or PIN", then "Face unlock ready".
**Observe:** password sign-in works while disabled; kill-switch status not editable; no numeric match value.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (Security screenshot disabled + enabled; lock-screen wording note).
**Cancel / roll back:** the inverse action (Enable reverses Disable) — both are UI toggles, fully reversible without a terminal.

---

## STEP 9 — Test / Bench dry-run (Test/Bench screen)

**Goal:** run a recognition dry-run that explicitly never unlocks.

Test/Bench → **Run dry-run**. The screen states explicitly, on-screen, that this test **never unlocks the session**. Run the recognition test and observe the result wording, the latency bench summary and the calibration report (threshold + date).

**Expected wording:** generic result wording; the on-screen "never unlocks" notice; no numeric match value is rendered.
**Observe:** the session remains unlocked/locked as it was — the dry-run does not submit a credential. Progress + cancel available.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (Test/Bench screenshot; note session state unchanged).
**Cancel / roll back:** Cancel the run at any time; no state change. Nothing to roll back.

---

## STEP 10 — Fallback check (lock screen; service stopped)

**Goal:** prove the password/PIN tile works instantly and the unavailable tile appears within 300 ms (FR-03, NFR-06).

1. Lock the session (Win+L). Confirm the tile appears only for the **enrolled SID** (FR-16) and shows generic wording only — no numeric match value.
2. Observe the fallback: with the service **stopped**, the tile reads "Use your password or PIN" and the password/PIN tile works **instantly** with no added delay. The CP shows "Face unlock unavailable" within **≤ 300 ms** of service-down detection (FR-03 acceptance criterion).
3. Measure/record the observed fallback behaviour and confirm there is no delay attributable to Kenali.

**Expected wording:** "Face unlock ready", "Looking for you…", "Use your password or PIN", "Face unlock unavailable".
**Observe:** password works with service down; unavailable tile ≤ 300 ms; tile only for enrolled SID; no numeric match value.
**Evidence:** `Result: VERIFY` · `Evidence: OWNER to fill` (lock-screen screenshots; timed observation note).
**Cancel / roll back:** sign in with the password to restore the session. Restarting the service (owner, out of scope for this no-terminal walkthrough) restores face unlock; until then the system is safe via password.

---

## 4. No-terminal assertion

Confirm that every action in STEP 1–10 was completed **without opening a terminal** on the target machine: no PowerShell prompt, no `kenali.exe` recovery CLI invocation, no registry editor, no service-control command. The only command-line interaction anywhere in this walkthrough would be the owner's out-of-band pre-flight (`winver`, `Get-Tpm`, etc.) and the separate recovery drills — neither is an operator action and neither is part of the UI walkthrough. If any STEP required a terminal, mark it as a failure of FR-17/FR-09 and record the anomaly.

---

## 5. Owner sign-off checklist

| # | Item | Status | Evidence / notes |
|---|---|---|---|
| 1 | Pre-flight checklist complete on this target | `VERIFY` | `OWNER to fill` |
| 2 | STEP 1 Launch + Dashboard | `VERIFY` | `OWNER to fill` |
| 3 | STEP 2 Enrol wizard (≥ 12 / ≥ 2 conditions, cancel verified) | `VERIFY` | `OWNER to fill` |
| 4 | STEP 3 Change a setting (PAD thresholds read-only) | `VERIFY` | `OWNER to fill` |
| 5 | STEP 4 Add a template | `VERIFY` | `OWNER to fill` |
| 6 | STEP 5 Delete a template with confirm | `VERIFY` | `OWNER to fill` |
| 7 | STEP 6 Export diagnostics (redacted, no paths to service) | `VERIFY` | `OWNER to fill` |
| 8 | STEP 7 Refresh credentials | `VERIFY` | `OWNER to fill` |
| 9 | STEP 8 Disable / Enable | `VERIFY` | `OWNER to fill` |
| 10 | STEP 9 Test/Bench dry-run (never unlocks) | `VERIFY` | `OWNER to fill` |
| 11 | STEP 10 Fallback (password instant, unavailable ≤ 300 ms) | `VERIFY` | `OWNER to fill` |
| 12 | No numeric match values displayed anywhere in UI | `VERIFY` | `OWNER to fill` |
| 13 | No terminal / recovery CLI used for any STEP | `VERIFY` | `OWNER to fill` |
| 14 | All evidence attached (screenshots + notes, no secrets) | `VERIFY` | `OWNER to fill` |

**Anomalies observed:** `OWNER to fill`

**Ladder rung signed off:** `VM / secondary / main — OWNER to fill`  ·  **Owner:** `OWNER to fill`  ·  **Date:** `OWNER to fill`

> Reminder for the signer: this is a plan plus step-by-step script. Execution evidence lives in spike/report docs; no runtime result is claimed by the authoring host. Walk the ladder in order and do not advance a rung until the prior one passes.
