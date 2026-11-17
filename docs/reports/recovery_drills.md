# Recovery Drill Report — R-A … R-E

| Field | Value |
|---|---|
| Status | **VERIFY / OWNER-RUN / NOT EXECUTED** — blank owner-run template. No drill result is claimed here; the owner fills every table after performing each drill in the VM. |
| Scope | R-A normal password fallback, R-B kill switch, R-C Safe Mode, R-D WinRE, R-E versioned rollback (`docs/dev_safety_runbook.md` §4, `docs/test_plan.md` §12, `docs/prd.md` §13). |
| Owner-run script | `tests/ci/recovery_drills.ps1` — functions `Drill-RA` … `Drill-RE` print exact manual steps; they never register a CP or edit the live registry. Delivered only, never run by an agent. |
| Authority | `docs/dev_safety_runbook.md` §4 remains the authoritative drill wording. |

---

## 1. Boundary — read before trusting any result

Each drill is **OWNER-RUN once successfully in the VM** before v1 sign-off (`docs/prd.md` §13). An agent never executes a drill. The development host is WSL2 Linux; it cannot register a Credential Provider, write Windows registry state, install a service, boot into Safe Mode or WinRE, or perform a rollback. The script only prints manual steps and a redacted checklist token.

- **VERIFY** = static review of the documented manual steps against the runbook.
- **OWNER-RUN** = the owner performed the drill by hand in the VM and records the observed result.
- **NOT EXECUTED** = current state of every drill below.

> **VERIFY the exact offline hive path / ControlSet number on the reference build before relying on R-D step 5.** Renaming the DLL (R-D step 6) is the robust fallback.

---

## 2. Drill results (owner fills)

### R-A — Normal password fallback

| Field | Owner entry |
|---|---|
| Date / VM OS build | _(blank — owner fills)_ |
| Manual steps followed (runbook §4 R-A) | _(blank — owner fills)_ |
| "Face unlock unavailable" within <= 300 ms | _(blank — owner fills)_ |
| Password sign-in succeeded immediately | _(blank — owner fills)_ |
| Pass / fail | _(blank — owner fills)_ |
| Status | **NOT EXECUTED** |

### R-B — Kill switch

| Field | Owner entry |
|---|---|
| Date / VM OS build | _(blank — owner fills)_ |
| Both `Disabled=1` values set per runbook §3 | _(blank — owner fills)_ |
| No tile at next session; password/PIN works | _(blank — owner fills)_ |
| Re-enable confirmed tile returns | _(blank — owner fills)_ |
| Pass / fail | _(blank — owner fills)_ |
| Status | **NOT EXECUTED** |

### R-C — Safe Mode

| Field | Owner entry |
|---|---|
| Date / VM OS build | _(blank — owner fills)_ |
| Booted via Startup Settings -> **Safe Mode** (4 / F4) | _(blank — owner fills)_ |
| Built-in password provider only; sign-in succeeded | _(blank — owner fills)_ |
| Returned to normal boot | _(blank — owner fills)_ |
| Pass / fail | _(blank — owner fills)_ |
| Status | **NOT EXECUTED** |

### R-D — WinRE

| Field | Owner entry |
|---|---|
| Date / VM OS build | _(blank — owner fills)_ |
| Booted to **WinRE** command prompt | _(blank — owner fills)_ |
| Offline hive path / ControlSet verified | _(blank — owner fills)_ |
| Disable via offline registry OR DLL rename used | _(blank — owner fills)_ |
| Normal boot presents password provider; sign-in succeeded | _(blank — owner fills)_ |
| Pass / fail | _(blank — owner fills)_ |
| Status | **NOT EXECUTED** |

### R-E — Versioned rollback

| Field | Owner entry |
|---|---|
| Date / VM OS build | _(blank — owner fills)_ |
| `cargo xtask rollback` run by owner in VM | _(blank — owner fills)_ |
| Previous version tile loads; no in-place DLL edit | _(blank — owner fills)_ |
| Fallback to R-D needed | _(blank — owner fills)_ |
| Pass / fail | _(blank — owner fills)_ |
| Status | **NOT EXECUTED** |

---

## 3. Sign-off (owner only)

| Drill | Result | Owner initials / date |
|---|---|---|
| R-A | _(blank — owner fills)_ | _(blank — owner fills)_ |
| R-B | _(blank — owner fills)_ | _(blank — owner fills)_ |
| R-C | _(blank — owner fills)_ | _(blank — owner fills)_ |
| R-D | _(blank — owner fills)_ | _(blank — owner fills)_ |
| R-E | _(blank — owner fills)_ | _(blank — owner fills)_ |

> No result may be recorded in this report except by the owner after actually performing the drill in the VM. Until then every drill stays **VERIFY / OWNER-RUN / NOT EXECUTED**.
