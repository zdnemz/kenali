# Soak Report — VM Lock/Unlock & Secondary Account

| Field | Value |
|---|---|
| Status | **VERIFY / OWNER-RUN / NOT EXECUTED** — blank owner-run template. No soak has been claimed here; the owner fills the tables below with real results. |
| Scope | 1,000-cycle VM lock/unlock soak and 2-week secondary-account soak (NFR-06, `docs/test_plan.md` §9, `docs/prd.md` §13). |
| Owner-run scripts | `tests/ci/soak_vm.ps1`, `tests/ci/soak_secondary_account.ps1` — delivered only, never run by an agent. |
| Authority | `docs/test_plan.md` §9 (soak) and §10 (hardware matrix), `docs/dev_safety_runbook.md` §1 (VM-first ladder), `docs/prd.md` §13. |

---

## 1. Boundary — read before trusting any result

These soaks are **OWNER-RUN** inside the disposable Windows 11 x64 VM (checkpoint taken before install) and then on the **secondary local administrator account**; the main account stays untouched until both pass. The development host is WSL2 Linux and cannot register a Credential Provider, install a service, open a camera or write Windows registry state. No result below is filled in by an agent.

- **VERIFY** = static checklist review only (script wiring, redaction, abort path).
- **OWNER-RUN** = the owner must execute the soak in the VM / on the secondary account and record the observed numbers.
- **NOT EXECUTED** = current state of every row until the owner signs off.

Scripts parameterise credentials as `[PSCredential]` (no plaintext secrets), write only redacted status tokens to a log, count cycles, abort on suspected lock-out and emit a redacted summary. Raw health-probe output, usernames, passwords, SIDs and hostnames are never logged.

---

## 2. VM lock/unlock soak — 1,000 cycles (owner fills)

| Field | Owner entry |
|---|---|
| Date / window | _(blank — owner fills)_ |
| VM OS build / feature release (`winver`) | _(blank — owner fills)_ |
| Checkpoint taken before install | _(blank — owner fills: yes/no)_ |
| Cycles requested / completed | _(blank — owner fills)_ |
| CP crashes | _(blank — owner fills)_ |
| Lock-outs | _(blank — owner fills)_ |
| Password/PIN fallback available every cycle | _(blank — owner fills)_ |
| Camera LED off when idle | _(blank — owner fills)_ |
| CP RSS <= 5 MB / service idle RSS <= 150 MB | _(blank — owner fills)_ |
| Deliberate interleaved failures completed (service kill, camera replug, password change, kill switch flip) | _(blank — owner fills)_ |
| Log path (redacted) | _(blank — owner fills)_ |
| Verdict (pass/fail) | _(blank — owner fills)_ |
| Status | **NOT EXECUTED** |

---

## 3. Secondary-account soak — 2 weeks / 14 days (owner fills)

| Field | Owner entry |
|---|---|
| Date / window | _(blank — owner fills)_ |
| Secondary local admin account confirmed; main account untouched | _(blank — owner fills)_ |
| Days completed | _(blank — owner fills)_ |
| Lock-outs | _(blank — owner fills)_ |
| Allowed-SID guard holds (main account never sees the tile) | _(blank — owner fills)_ |
| Unexpected disables | _(blank — owner fills)_ |
| Refresh-credential works after password change | _(blank — owner fills)_ |
| Hardware matrix (sleep, hibernate, lid, Fast Startup, camera contention, first unlock after Windows Update) | _(blank — owner fills)_ |
| Log path (redacted) | _(blank — owner fills)_ |
| Verdict (pass/fail) | _(blank — owner fills)_ |
| Status | **NOT EXECUTED** |

---

## 4. Sign-off (owner only)

| Soak | Result | Owner initials / date |
|---|---|---|
| 1,000-cycle VM lock/unlock | _(blank — owner fills)_ | _(blank — owner fills)_ |
| 2-week secondary account | _(blank — owner fills)_ | _(blank — owner fills)_ |

> No result may be recorded in this report except by the owner after an actual soak in the VM / secondary account. Until then every row stays **VERIFY / OWNER-RUN / NOT EXECUTED**.
