# CI installer build (`build-installer.yml`)

The `.github/workflows/build-installer.yml` workflow builds the Windows release binaries, stages them with `cargo xtask package`, signs them with Authenticode, produces the NSIS setup executable, and uploads the package, installer and checksums as GitHub Actions artifacts.

All Windows build/sign/installer steps run on a GitHub-hosted `windows-latest` runner. This repository never runs NSIS, `signtool.exe`, or any install/registration command on the Linux development host.

## Triggers

- `workflow_dispatch` — manual run from the Actions tab (defaults to the workspace version from `Cargo.toml`).
- `push` to `main` — uses `[workspace.package].version` from `Cargo.toml`.
- `push` of a tag matching `v*` — strips the leading `v` and uses the remainder as the version (for example, tag `v0.2.0` produces version `0.2.0`).

The version is written to `$env:GITHUB_ENV` and appears in artifact names and the installer filename: `kenali-${VERSION}-x86_64-pc-windows-msvc-setup.exe`.

## Required secrets

Configure these as repository (or environment) secrets before running the workflow:

| Secret | Purpose |
|---|---|
| `KENALI_SIGN_CERT_B64` | Base64-encoded code-signing PFX. The workflow decodes it to `$env:RUNNER_TEMP\kenali-signing.pfx` at run time. |
| `KENALI_SIGN_CERT_PASSWORD` | Password for the PFX; passed only as the `signtool.exe /p` argument. |

Signing is performed **in CI with secrets**: the four staged binaries (`kenali-svc.exe`, `kenali-cp.dll`, `kenali-ui.exe`, `kenali.exe`) are signed after staging, and the installer is signed after `makensis.exe` produces it. Secrets are never echoed, written into the repository, or committed. The temporary PFX is deleted by an `if: always()` cleanup step even when an earlier step fails, so no private key material is left on the runner. The credential provider CLSID is parsed from `KENALI_CP_CLSID` in `crates/kenali-winapi/src/com.rs` at build time and passed to NSIS via `/DKENALI_CLSID=...`; it is never hardcoded in the workflow.

If either secret is missing the build fails before signing; no unsigned installer is uploaded. Request a certificate from the repository owner to enable signing — do not commit a PFX, password or base64 blob to the repository.

## Downloading artifacts

After the run finishes, open the workflow run page, scroll to **Artifacts**, and download:

- `kenali-package-${VERSION}-x86_64-pc-windows-msvc` — the staged package ZIP (signed binaries plus support files).
- `kenali-installer-${VERSION}-x86_64-pc-windows-msvc` — the signed NSIS setup executable.
- `kenali-checksums-${VERSION}-x86_64-pc-windows-msvc` — `SHA256SUMS.txt` with hashes for the ZIP and installer.

Verify the SHA-256 hashes in `SHA256SUMS.txt` before running anything (for example, `Get-FileHash -Algorithm SHA256 <file>` in PowerShell) and confirm the installer's Authenticode signature in file properties. Artifacts expire according to your GitHub plan's retention policy; re-run the workflow if they are no longer available.

## Owner-run install warning

> **The installer registers the Credential Provider and the `KenaliSvc` service automatically and writes HKLM registry state. This is an owner-run step only.**

Do **not** run the installer on the account you rely on to sign in. Test an installer build in a disposable Windows 11 VM with a checkpoint taken first, then on a secondary local-admin account, and only last on the main account. Never install over an existing version in place: the installer uses a versioned `C:\Program Files\Kenali\<version>\` directory and records the previous version for rollback. Before installing, read `docs/dev_safety_runbook.md` for the pre-flight checklist, the **kill switch** (`Disabled=1` under the provider CLSID and `HKLM\SOFTWARE\Kenali`), Safe Mode / WinRE recovery drills and rollback steps. Keep a working password and a second administrator account available at all times — an unhealthy credential provider can otherwise cause a lock-out.

CI only builds and signs; it does not install or register anything on any machine. Verification of the installer, registration and recovery remains a manual owner-run task on Windows.

## Pre-push hook (`husky` + `scripts/validate-pre-push.sh`)

Eight consecutive `build-installer.yml` failures were all detectable without GitHub. To stop that class of failure from reaching CI, the repository ships a pre-push hook that runs the same validations locally before the objects leave the machine.

The hook is wired with [Husky](https://typicode.github.io/husky/). `package.json` declares the `husky` devDependency and a `prepare` script, so a single `npm install` enables the hooks for that clone. `.husky/pre-push` is committed and executable, and it calls `scripts/validate-pre-push.sh` **directly** — no Husky runtime is needed for the hook itself to work, so an existing clone keeps validating even if `npm install` has not been run.

`npm run lint`, `npm run check` and `npm run validate` all invoke the same validator, so the checks can be run on demand without pushing.

### What it checks

| Check | Behaviour when the environment cannot run it |
|---|---|
| `cargo fmt --check` | Skipped with a warning if `cargo` or `rustfmt` is missing. |
| `cargo check --target x86_64-pc-windows-msvc --locked --workspace` | Skipped with a warning if `cargo` is missing. |
| `bash tests/ci/integration_gate.sh` | Always runs. |
| `bash tests/ci/goal_check.sh` | Always runs. |
| GitHub Actions workflow YAML (parse + triggers, job, installer-step tokens) | Skipped with a warning if `python3` or PyYAML is missing. |
| **Compile of `installer/kenali.nsi` with `makensis`** | Skipped with a warning when no NSIS toolchain is reachable offline. |

The NSIS compile is the important one. The validator creates a temporary staging directory, writes dummy versions of the four release binaries plus the SBOM, and compiles the real script with every `/DKENALI_*` define the workflow passes (`KENALI_NSIS_INCLUDE`, `KENALI_VERSION`, `KENALI_CLSID`, `KENALI_STAGE_DIR`, `KENALI_SVC_EXE`, `KENALI_CP_DLL`, `KENALI_UI_EXE`, `KENALI_CLI_EXE`). Output and dummies stay in the temporary directory, so the working tree is never modified.

This catches the failures that actually broke CI, including `Invalid command: "${GetFullPath}"` — `${Name}` only expands for `!define`d names, so referencing a non-existent macro aborts the compile. It also catches commands used from the wrong section and `File` instructions pointing at files that are not staged.

### NSIS toolchain without root

`makensis` is not installed on the Linux host. On first use the validator provisions it automatically by running `apt-get download nsis nsis-common` (no root required), unpacking the `.deb` files with `dpkg-deb -x`, and caching `bin/makensis` plus `share/nsis/{Include,Stubs}` under the gitignored `tools/nsis/`. Later runs reuse the cache with no network access. See `tools/nsis/README.md` for the manual procedure and the cache layout.

The rule is deliberately asymmetric:

- **Toolchain present** (cached, freshly provisioned, or `makensis` on `PATH`) → the compile always runs, and a compile error **fails the push**.
- **Toolchain unavailable and offline** → the check is **skipped with a loud warning**; the push proceeds.

So a missing toolchain never blocks work, but a broken installer script is never waved through.

### Summary output and bypass

Every run ends with an explicit tally:

```
pre-push summary: 5 passed, 1 skipped, 1 failed
FAIL  NSIS installer compile
```

Any `FAIL` exits non-zero, which aborts the push before anything is transferred. `SKIP` never fails the push.

To bypass the hook once — for example when validating on a machine that cannot run Rust at all — push with:

```sh
git push --no-verify
```

Use it as an escape hatch, not a habit: CI will still apply the same installer checks, and a failure there costs a full CI round trip. There is no way to bypass the hook for every push without editing `.husky/pre-push`, which the static gate in `tests/ci/integration_gate.sh` detects.

