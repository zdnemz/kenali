#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

REQUIRED='kenali-core kenali-ipc kenali-store kenali-vision kenali-winapi kenali-camera kenali-service kenali-cp kenali-ui kenali-cli kenali-bench xtask fuzz'

# 1. workspace metadata contains every required crate
META=$(cargo metadata --no-deps --format-version 1 --locked)
for c in $REQUIRED; do
  printf '%s' "$META" | grep -q "\"name\":\"$c\"" || { echo "FAIL missing workspace crate: $c"; exit 1; }
done

# 2. every shared-contract crate boundary resolves (declaration + path targets)
for c in $REQUIRED; do
  if [ -f "crates/$c/Cargo.toml" ]; then CFG="crates/$c/Cargo.toml"; else CFG="$c/Cargo.toml"; fi
  test -s "$CFG" || { echo "FAIL missing manifest: $CFG"; exit 1; }
done
rg -q 'kenali-core'     crates/kenali-ipc/Cargo.toml
rg -q 'kenali-core'     crates/kenali-store/Cargo.toml
rg -q 'kenali-core'     crates/kenali-vision/Cargo.toml
rg -q 'kenali-ipc'      crates/kenali-cp/Cargo.toml
rg -q 'kenali-ipc'      crates/kenali-ui/Cargo.toml
rg -q 'kenali-winapi'   crates/kenali-ui/Cargo.toml
! rg -q 'kenali-(camera|vision|store)' crates/kenali-ui/Cargo.toml
! rg -q 'kenali-winapi' crates/kenali-vision/Cargo.toml
! rg -q 'kenali-cp'     crates/kenali-service/Cargo.toml

# 3. Windows target type-checks the whole workspace
cargo check --target x86_64-pc-windows-msvc --locked --workspace

# 4. platform-agnostic suites
cargo test --locked -p kenali-core -p kenali-ipc -p kenali-store -p kenali-vision

# 5. unsafe policy: forbid everywhere except the three FFI crates; SAFETY on every unsafe block
for c in kenali-core kenali-ipc kenali-store kenali-vision kenali-ui kenali-service kenali-cli kenali-bench xtask fuzz; do
  f=""
  for cand in crates/$c/src/lib.rs crates/$c/src/main.rs $c/src/lib.rs $c/src/main.rs; do
    [ -f "$cand" ] && f="$cand" && break
  done
  [ -n "$f" ] || { echo "FAIL no crate root for $c"; exit 1; }
  rg -q '^#!\[forbid\(unsafe_code\)\]' "$f" || { echo "FAIL missing forbid(unsafe_code): $f"; exit 1; }
done
for c in kenali-winapi kenali-camera kenali-cp; do
  ! rg -q 'forbid\(unsafe_code\)' crates/$c/src/lib.rs || { echo "FAIL unexpected forbid in $c"; exit 1; }
done
UNSAFE=$( { rg -o --no-filename '\bunsafe[[:space:]]*\{' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src || true; } | wc -l)
SAFETY=$( { rg -o --no-filename '// SAFETY:' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src || true; } | wc -l)
[ "$SAFETY" -ge "$UNSAFE" ] || { echo "FAIL unsafe block without // SAFETY:"; exit 1; }

# 6. no unapproved placeholder in a user-visible position
! rg -n --glob '!**/tests/**' --glob '!*.md' --glob '!tests/ci/**' \
   -e 'TODO' -e '\[PRICE\]' -e 'lorem' -e 'unimplemented!\(\)' \
   crates xtask fuzz

# 7. every referenced doc file exists and is non-empty
for d in docs/prd.md docs/context_pack.md docs/architecture.md docs/threat_model.md \
         docs/roadmap.md docs/test_plan.md docs/dev_safety_runbook.md docs/references.md \
         docs/reports/coverage.md; do
  test -s "$d" || { echo "FAIL missing/empty doc: $d"; exit 1; }
done

echo "INTEGRATION GATE PASS"

# --- Wave 2 seams (append to tests/ci/integration_gate.sh) ---
set -euo pipefail

# CP ↔ winapi ↔ ipc names match across crate boundaries
rg -q 'kenali-ipc'      crates/kenali-cp/Cargo.toml
rg -q 'kenali-winapi'   crates/kenali-cp/Cargo.toml
rg -q 'kenali-core'     crates/kenali-cp/Cargo.toml
rg -q 'pub const KENALI_CP_CLSID' crates/kenali-winapi/src/com.rs
rg -q 'KENALI_CP_CLSID' xtask/src/install.rs
rg -q 'AUTH_PIPE_PATH'  crates/kenali-winapi/src/pipes.rs
rg -q 'pub const PROTO_VERSION' crates/kenali-ipc/src/lib.rs
rg -q 'PROTO_VERSION'   crates/kenali-winapi/src/pipes.rs
rg -q -F '\\\\.\\pipe\\kenali.auth' crates/kenali-winapi/src/pipes.rs
rg -q -F 'D:P(A;;GA;;;SY)' crates/kenali-winapi/src/pipes.rs

# every COM entry point is wrapped: exported entries + #[implement] methods
GUARDS=$( { rg -o --no-filename 'guard\(' crates/kenali-cp/src || true; } | wc -l)
[ "$GUARDS" -ge 10 ] || { echo "FAIL CP COM entry points not all guarded"; exit 1; }
rg -q 'DllGetClassObject' crates/kenali-cp/src/lib.rs
rg -q 'DllCanUnloadNow'  crates/kenali-cp/src/lib.rs
rg -q '#\[implement\(ICredentialProvider\)\]' crates/kenali-cp/src/provider.rs
rg -q '#\[implement\(ICredentialProviderCredential\)\]' crates/kenali-cp/src/credential.rs

# no unwrap/panic in CP hot path; no scores on tiles; no ML/network/async in CP
! rg -n 'unwrap\(|expect\(|panic!|todo!|unimplemented!|\[[a-z_]+\]' crates/kenali-cp/src
! rg -n -i 'score|similarity|\bpad\b' crates/kenali-cp/src/credential.rs
! rg -q 'ort|tokio|reqwest|hyper|async-std' crates/kenali-cp/Cargo.toml
rg -q 'scenario_supported' crates/kenali-cp/src/scenarios.rs
rg -q 'tile_text'          crates/kenali-cp/src/credential.rs
rg -q 'kill_switch_enabled' crates/kenali-cp/src/kill_switch.rs
rg -q 'is_allowed_sid'      crates/kenali-cp/src/allowed_sid.rs

# install script refuses without the env var (STATIC check only — never execute it here)
rg -q 'KENALI_ALLOW_INSTALL' xtask/src/install.rs
rg -q 'bail!' xtask/src/install.rs
rg -q -F 'C:\Program Files\Kenali' xtask/src/install.rs
rg -q 'CARGO_PKG_VERSION' xtask/src/install.rs
rg -qi 'rollback|previous' xtask/src/install.rs

# coverage + deliverables updated for wave 2
for id in FR-02 FR-03 FR-10 FR-16 SEC-03 SEC-06 SEC-17; do
  rg -q "^\\| $id .*type-checked \\+ owner VM" docs/reports/coverage.md \
    || { echo "FAIL wave-2 coverage row: $id"; exit 1; }
done
while IFS= read -r p; do
  [ -z "$p" ] && continue
  case "$p" in \#*) continue ;; esac
  test -s "$p" || { echo "FAIL missing/empty wave-2 deliverable: $p"; exit 1; }
done < tests/ci/deliverables.txt
test -s docs/spikes/P0-S1.md

# whole workspace MSVC type-check + all host suites (reaffirm Wave-1 steps 3-4)
cargo check --target x86_64-pc-windows-msvc --locked --workspace
cargo test --locked -p kenali-core -p kenali-ipc -p kenali-store -p kenali-vision

# unsafe confined with SAFETY comments (Wave-1 step 5 already enforces the forbid side)
U=$( { rg -o --no-filename 'unsafe[[:space:]]*\{' crates/kenali-winapi/src crates/kenali-cp/src || true; } | wc -l)
S=$( { rg -o --no-filename '// SAFETY:' crates/kenali-winapi/src crates/kenali-cp/src || true; } | wc -l)
[ "$S" -ge "$U" ] || { echo "FAIL unsafe block without // SAFETY:"; exit 1; }

# no unapproved placeholders (reaffirm Wave-1 step 6 over the new files)
! rg -n --glob '!**/tests/**' --glob '!*.md' --glob '!tests/ci/**' \
   -e 'TODO' -e 'lorem' -e 'unimplemented!\(\)' crates xtask fuzz

echo "WAVE 2 INTEGRATION GATE PASS"

# --- Wave 3 seams (append to tests/ci/integration_gate.sh) ---
set -euo pipefail

# service ↔ winapi ↔ ipc ↔ store ↔ vision ↔ camera names match across crate boundaries
rg -q 'kenali-winapi'  crates/kenali-service/Cargo.toml
rg -q 'kenali-ipc'     crates/kenali-service/Cargo.toml
rg -q 'kenali-store'   crates/kenali-service/Cargo.toml
rg -q 'kenali-vision'  crates/kenali-service/Cargo.toml
rg -q 'kenali-camera'  crates/kenali-service/Cargo.toml
rg -q 'kenali-store'   crates/kenali-winapi/Cargo.toml
rg -q 'KeyHierarchy for TpmKeyHierarchy' crates/kenali-winapi/src/tpm.rs
rg -q 'impl FrameSource for MfSource'      crates/kenali-camera/src/mf.rs
rg -q 'impl FrameSource for FileSource'    crates/kenali-camera/src/replay.rs
rg -q 'impl FrameSource for SyntheticSource' crates/kenali-camera/src/synthetic.rs
rg -q 'impl Detector for OrtDetector'      crates/kenali-vision/src/onnx.rs
rg -q 'impl Embedder for OrtEmbedder'      crates/kenali-vision/src/onnx.rs
rg -q 'pub struct VisionPipeline'          crates/kenali-service/src/pipeline.rs
rg -q 'create_server_pipe'                 crates/kenali-service/src/pipe_loop.rs
rg -q 'AttemptStateMachine'                crates/kenali-service/src/auth_attempt.rs
rg -q 'PolicyEngine'                       crates/kenali-service/src/auth_attempt.rs
rg -q 'pub const PROTO_VERSION'            crates/kenali-ipc/src/lib.rs
rg -q 'PROTO_VERSION'                      crates/kenali-service/src/pipe_loop.rs

# every pipe path / SDDL is consistent with the client and the docs
rg -q -F '\\\\.\\pipe\\kenali.auth'    crates/kenali-winapi/src/pipes.rs
rg -q -F '\\\\.\\pipe\\kenali.admin'   crates/kenali-winapi/src/pipe_server.rs
rg -q -F '\\\\.\\pipe\\kenali.preview' crates/kenali-winapi/src/pipe_server.rs
rg -q -F 'D:P(A;;GA;;;SY)'             crates/kenali-winapi/src/pipes.rs
rg -q -F 'D:P(A;;GA;;;SY)(A;;GA;;;BA)' crates/kenali-winapi/src/pipe_server.rs
rg -q 'FIRST_PIPE_INSTANCE'            crates/kenali-winapi/src/pipe_server.rs
rg -q 'pub fn verify_peer_pid'         crates/kenali-winapi/src/pipe_server.rs
rg -q 'AUTH_ATTEMPT_TIMEOUT'           crates/kenali-winapi/src/pipes.rs
rg -q '≤ 3 s|3 s' docs/spikes/P0-S3.md

# every pipe/server entry is bounded and panic-safe (no unwrap/panic in hot paths)
! rg -n 'unwrap\(|expect\(|panic!|todo!|unimplemented!' \
   crates/kenali-service/src crates/kenali-winapi/src/pipe_server.rs
rg -q 'deadline' crates/kenali-winapi/src/pipe_server.rs
rg -q 'ConnectNamedPipe' crates/kenali-winapi/src/pipe_server.rs
rg -q 'log_warning' crates/kenali-service/src/logging.rs
rg -q 'take_pad_warning' crates/kenali-service/src/auth_attempt.rs

# unsafe confined to the three FFI crates, with SAFETY on every block (incl. camera + pipes server)
for c in kenali-winapi kenali-camera kenali-cp; do
  ! rg -q 'forbid\(unsafe_code\)' crates/$c/src/lib.rs || { echo "FAIL forbid in $c"; exit 1; }
done
U=$( { rg -o --no-filename 'unsafe[[:space:]]*\{' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src || true; } | wc -l)
S=$( { rg -o --no-filename '// SAFETY:' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src || true; } | wc -l)
[ "$S" -ge "$U" ] || { echo "FAIL unsafe block without // SAFETY:"; exit 1; }

# model lock verified, no weights committed
test -s models/models.lock
for m in yunet.onnx sface.onnx minifasnet_v2.onnx minifasnet_v1se.onnx; do
  rg -q -F "$m" models/models.lock || { echo "FAIL model not pinned: $m"; exit 1; }
done
! find . -name '*.onnx' -not -path './target/*' | grep -q .
rg -q 'verify_model_hash' crates/kenali-vision/src/onnx.rs
rg -q '\*\.onnx' .gitignore

# whole workspace MSVC type-check + all host suites (reaffirm waves 1-2 steps 3-4)
cargo check --target x86_64-pc-windows-msvc --locked --workspace
cargo test --locked -p kenali-core -p kenali-ipc -p kenali-store -p kenali-vision -p kenali-bench

# Win32-only test modules are target-gated; these crates type-check for MSVC only.
# Host cargo test of winapi/cp/camera is impossible on Linux: windows-future 0.3.2 is a
# non-optional dep of windows 0.62.2 and references windows_core::imp unconditionally.
! rg -n '^[[:space:]]*#\[cfg\(test\)\]' crates/kenali-winapi/src crates/kenali-cp/src crates/kenali-camera/src

# no unapproved placeholders (reaffirm waves 1-2 step 6 over the new files)
! rg -n --glob '!**/tests/**' --glob '!*.md' --glob '!tests/ci/**' \
   -e 'TODO' -e 'lorem' -e 'unimplemented!\(\)' crates xtask fuzz

# coverage + deliverables updated for wave 3
for id in FR-08 FR-12 SEC-01 SEC-14 SEC-16 NFR-01 NFR-02 NFR-04; do
  rg -q "^\\| $id .*type-checked \\+ owner VM" docs/reports/coverage.md \
    || { echo "FAIL wave-3 coverage row: $id"; exit 1; }
done
for d in docs/spikes/P0-S2.md docs/spikes/P0-S3.md docs/spikes/P0-S4.md \
         docs/spikes/P0-S5.md docs/spikes/P0-S10.md docs/spikes/P0-S11.md; do
  test -s "$d" || { echo "FAIL wave-3 spike doc: $d"; exit 1; }
done
while IFS= read -r p; do
  [ -z "$p" ] && continue
  case "$p" in \#*) continue ;; esac
  test -s "$p" || { echo "FAIL missing/empty wave-3 deliverable: $p"; exit 1; }
done < tests/ci/deliverables.txt

echo "WAVE 3 INTEGRATION GATE PASS"

# --- Wave 4 seams (append to tests/ci/integration_gate.sh) ---
set -euo pipefail

# UI/CLI depend only on core + ipc + winapi; never camera/vision/store/cp/bench
rg -q 'kenali-core'   crates/kenali-ui/Cargo.toml
rg -q 'kenali-ipc'    crates/kenali-ui/Cargo.toml
rg -q 'kenali-winapi' crates/kenali-ui/Cargo.toml
rg -q 'kenali-core'   crates/kenali-cli/Cargo.toml
rg -q 'kenali-ipc'    crates/kenali-cli/Cargo.toml
rg -q 'kenali-winapi' crates/kenali-cli/Cargo.toml
! rg -q 'kenali-(camera|vision|store|cp|bench)' crates/kenali-ui/Cargo.toml
! rg -q 'kenali-(camera|vision|store|cp|bench)' crates/kenali-cli/Cargo.toml

# no auth-pipe types, secrets, camera, registry writes or file paths in UI/CLI source
! rg -n 'AUTH_PIPE|AUTH_PIPE_PATH|AUTH_PIPE_SDDL|AuthPipeClient|kenali_ipc::auth|SecretWide' crates/kenali-ui/src crates/kenali-cli/src
! rg -n 'HKEY|RegSetValue|HKLM|CreateFileW|kenali_camera|MfSource|FrameSource' crates/kenali-ui/src crates/kenali-cli/src

# one shared typed admin client; every mutation is a typed admin command
rg -q 'pub struct AdminPipeClient' crates/kenali-winapi/src/admin_pipe.rs
rg -q 'pub fn exchange'            crates/kenali-winapi/src/admin_pipe.rs
rg -q 'service_pid'                crates/kenali-winapi/src/admin_pipe.rs
rg -q 'PROTO_VERSION'              crates/kenali-winapi/src/admin_pipe.rs
rg -q -F '\\\\.\\pipe\\kenali.admin' crates/kenali-winapi/src/pipe_server.rs
rg -q 'AdminRequest' crates/kenali-ui/src/pipe_worker.rs
rg -q 'AdminResponse' crates/kenali-ui/src/pipe_worker.rs
rg -q 'AdminPipeClient' crates/kenali-ui/src/admin_client.rs
rg -q 'AdminRequest' crates/kenali-cli/src/main.rs
rg -q 'AdminPipeClient' crates/kenali-cli/src/main.rs
for cmd in GetStatus ListTemplates DeleteTemplate BeginEnrolment EnrolPassword SubmitSample \
           FinishEnrolment CancelEnrolment BeginPreview EndPreview BeginTest \
           BeginCalibration GetConfig SetConfig RefreshCredential SetEnabled \
           ExportDiagnostics; do
  rg -q "$cmd" crates/kenali-ui/src/pipe_worker.rs \
    || { echo "FAIL UI worker missing typed command: $cmd"; exit 1; }
done
rg -q 'enum PipeCommand' crates/kenali-ui/src/pipe_worker.rs
rg -q 'enum PipeEvent'   crates/kenali-ui/src/pipe_worker.rs
rg -q 'crossbeam_channel::bounded' crates/kenali-ui/src/pipe_worker.rs
! rg -n 'unwrap\(|expect\(|panic!|todo!|unimplemented!' crates/kenali-ui/src/pipe_worker.rs crates/kenali-cli/src/main.rs

# elevation + single instance + per-monitor DPI live in winapi, not the UI crate
rg -q -F 'Local\KenaliUiSingleInstance' crates/kenali-winapi/src/ui.rs
rg -q 'pub fn acquire_ui_instance'      crates/kenali-winapi/src/ui.rs
rg -q 'pub fn relaunch_elevated'        crates/kenali-winapi/src/ui.rs
rg -q 'pub fn focus_existing_instance'  crates/kenali-winapi/src/ui.rs
rg -q 'pub fn set_per_monitor_dpi_awareness' crates/kenali-winapi/src/ui.rs
rg -q 'acquire_ui_instance' crates/kenali-ui/src/main.rs
rg -q 'eframe::run_native'  crates/kenali-ui/src/main.rs
rg -q 'impl eframe::App'    crates/kenali-ui/src/window.rs

# seven screens, generic wording reused, no scores, dark/light + keyboard
rg -q 'enum Screen' crates/kenali-ui/src/window.rs
for s in Dashboard Enrol Templates Settings Security Diagnostics TestBench; do
  rg -q "$s" crates/kenali-ui/src/window.rs || { echo "FAIL missing screen: $s"; exit 1; }
done
rg -q 'kenali_core::strings' crates/kenali-ui/src/strings.rs
rg -q 'kenali_core::tiles'   crates/kenali-ui/src/strings.rs
rg -q 'ThemeKind' crates/kenali-ui/src/theme.rs
! rg -n -i 'score|similarity|confidence|pad margin' crates/kenali-ui/src/strings.rs

# preview frames RAM-only and zeroized; password field cleared after submit
rg -q 'Zeroizing' crates/kenali-ui/src/preview.rs
rg -q 'pub fn clear' crates/kenali-ui/src/preview.rs
rg -q 'preview_pipe::PreviewPipeClient' crates/kenali-ui/src/pipe_worker.rs
rg -q 'MAX_PREVIEW_BYTES' crates/kenali-winapi/src/preview_pipe.rs
rg -q 'Password::from_wide' crates/kenali-ui/src/password_field.rs
rg -q 'clear()' crates/kenali-ui/src/password_field.rs
! rg -n 'png|jpeg|jpg|encode_image|image::save' crates/kenali-ui/src

# unsafe confined with SAFETY comments; UI/CLI stay forbid(unsafe_code)
for c in kenali-winapi kenali-camera kenali-cp; do
  ! rg -q 'forbid(unsafe_code)' crates/$c/src/lib.rs || { echo "FAIL forbid in $c"; exit 1; }
done
U=$( { rg -o --no-filename 'unsafe[[:space:]]*\{' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src || true; } | wc -l)
S=$( { rg -o --no-filename '// SAFETY:' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src || true; } | wc -l)
[ "$S" -ge "$U" ] || { echo "FAIL unsafe block without // SAFETY:"; exit 1; }
rg -q '^#!\[forbid\(unsafe_code\)\]' crates/kenali-ui/src/lib.rs
rg -q '^#!\[forbid\(unsafe_code\)\]' crates/kenali-ui/src/main.rs
rg -q '^#!\[forbid\(unsafe_code\)\]' crates/kenali-cli/src/lib.rs
rg -q '^#!\[forbid\(unsafe_code\)\]' crates/kenali-cli/src/main.rs

# whole workspace MSVC type-check + all host suites (reaffirm waves 1-3)
cargo check --target x86_64-pc-windows-msvc --locked --workspace
cargo test --locked -p kenali-core -p kenali-ipc -p kenali-store -p kenali-vision -p kenali-bench

# Win32-only test modules stay target-gated (host cargo test of Win32 crates is impossible)
! rg -n '^[[:space:]]*#\[cfg\(test\)\]' crates/kenali-winapi/src crates/kenali-cp/src crates/kenali-camera/src crates/kenali-ui/src crates/kenali-cli/src

# no unapproved placeholders (reaffirm over the new UI/CLI/winapi files)
! rg -n --glob '!**/tests/**' --glob '!*.md' --glob '!tests/ci/**' \
   -e 'TODO' -e 'lorem' -e 'unimplemented!\(\)' crates xtask fuzz

# log hygiene + walkthrough docs exist; coverage + deliverables updated for wave 4
test -s docs/reports/log_hygiene.md
test -s tests/ui/walkthrough.md
for id in FR-01 FR-05 FR-09 FR-14 FR-17 FR-18 FR-20 NFR-10 SEC-04; do
  rg -q "^\\| $id .*type-checked \\+ owner VM" docs/reports/coverage.md \
    || { echo "FAIL wave-4 coverage row: $id"; exit 1; }
done
while IFS= read -r p; do
  [ -z "$p" ] && continue
  case "$p" in \#*) continue ;; esac
  test -s "$p" || { echo "FAIL missing/empty wave-4 deliverable: $p"; exit 1; }
done < tests/ci/deliverables.txt

echo "WAVE 4 INTEGRATION GATE PASS"

# --- Wave 5 seams (append to tests/ci/integration_gate.sh) ---
set -euo pipefail

# xtask command dispatch: full command set present; install-family guarded statically (NEVER executed here)
for c in Check Build Models Sign Install Uninstall Rollback Fuzz Bench Package Sbom; do
  rg -q "$c" xtask/src/main.rs || { echo "FAIL xtask subcommand missing: $c"; exit 1; }
done
for f in check build models sign install uninstall rollback fuzz bench package sbom; do
  test -s "xtask/src/$f.rs" || { echo "FAIL xtask module: $f"; exit 1; }
done
for f in install uninstall rollback; do
  rg -q 'KENALI_ALLOW_INSTALL' "xtask/src/$f.rs" || { echo "FAIL install guard: $f"; exit 1; }
done
rg -q 'Fetch' xtask/src/models.rs
rg -q 'Verify' xtask/src/models.rs
rg -q 'KENALI_SIGN_CERT' xtask/src/sign.rs
rg -q -F 'target/sbom/' xtask/src/sbom.rs
rg -q -F 'target/package/' xtask/src/package.rs
# pinned staging set: no cross-extension invented names (CI failure mode)
for pin in 'kenali-svc.exe' 'kenali-cp.dll' 'kenali-ui.exe' 'kenali.exe'; do
  rg -q -F -- "$pin" xtask/src/package.rs || { echo "FAIL package pin: $pin"; exit 1; }
done
! rg -q 'trim_end_matches\("\.exe"\).*trim_end_matches\("\.dll"\)' xtask/src/package.rs \
  || { echo "FAIL invented cross-extension package name"; exit 1; }
rg -q -F -- '*.pfx' .gitignore
rg -q -F -- '*.pfx.b64' .gitignore
rg -q -F -- '*.p12' .gitignore
rg -q 'ipc_decode' xtask/src/fuzz.rs
rg -q 'frame_decode' xtask/src/fuzz.rs
! rg -n 'unwrap\(|expect\(|panic!|todo!|unimplemented!' xtask/src

# fuzz harnesses are real byte-slice entry points, host-testable and MSVC-checkable
cargo test --locked -p fuzz
cargo check --target x86_64-pc-windows-msvc --locked -p fuzz
rg -q 'pub fn fuzz_ipc_decode' fuzz/src/lib.rs
rg -q 'pub fn fuzz_frame_decode' fuzz/src/lib.rs
rg -q 'decode_auth_request' fuzz/src/lib.rs
rg -q 'decode_admin_request' fuzz/src/lib.rs
rg -q 'decode_preview' fuzz/src/lib.rs
rg -q 'Frame::new' fuzz/src/lib.rs
rg -q 'to_nchw' fuzz/src/lib.rs
test -s fuzz/tests/hostile_inputs.rs

# release flags present in their pinned locations
rg -q '^\[profile\.release\]' Cargo.toml
rg -q 'lto = "fat"' Cargo.toml
rg -q 'codegen-units = 1' Cargo.toml
rg -q 'opt-level = 3' Cargo.toml
rg -q 'panic = "unwind"' Cargo.toml
rg -q -F 'target-feature=+crt-static' .cargo/config.toml
rg -q -F 'control-flow-guard=yes' .cargo/config.toml
rg -q -F 'link-arg=/CETCOMPAT' .cargo/config.toml
rg -q '^\s*xtask = ' .cargo/config.toml

# supply-chain gates that work on this host
cargo deny check -A unmaintained
cargo audit

# owner-run reports and scripts exist (never executed here)
for d in docs/reports/far_frr.md docs/reports/pad.md docs/reports/soak.md \
         docs/reports/recovery_drills.md tests/ci/soak_vm.ps1 \
         tests/ci/soak_secondary_account.ps1 tests/ci/recovery_drills.ps1; do
  test -s "$d" || { echo "FAIL wave-5 report/script: $d"; exit 1; }
done
rg -q -e '2,237' -e '2237' docs/reports/far_frr.md
rg -q '300' docs/reports/pad.md
for d in R-A R-B R-C R-D R-E; do
  rg -q -F "$d" tests/ci/recovery_drills.ps1 || { echo "FAIL drill: $d"; exit 1; }
done
! rg -qi 'executed (on|in) this host' docs/reports/far_frr.md docs/reports/pad.md \
   docs/reports/soak.md docs/reports/recovery_drills.md

# whole workspace MSVC type-check + all host suites (reaffirm waves 1-4; add fuzz)
cargo check --target x86_64-pc-windows-msvc --locked --workspace
cargo test --locked -p kenali-core -p kenali-ipc -p kenali-store -p kenali-vision -p kenali-bench -p fuzz

# unsafe confined with SAFETY comments; fuzz/xtask stay forbid(unsafe_code)
for c in kenali-winapi kenali-camera kenali-cp; do
  ! rg -q 'forbid\(unsafe_code\)' crates/$c/src/lib.rs || { echo "FAIL forbid in $c"; exit 1; }
done
U=$( { rg -o --no-filename 'unsafe[[:space:]]*\{' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src || true; } | wc -l)
S=$( { rg -o --no-filename '// SAFETY:' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src || true; } | wc -l)
[ "$S" -ge "$U" ] || { echo "FAIL unsafe block without // SAFETY:"; exit 1; }
rg -q '^#!\[forbid\(unsafe_code\)\]' fuzz/src/lib.rs
rg -q '^#!\[forbid\(unsafe_code\)\]' xtask/src/main.rs

# no unapproved placeholders (reaffirm waves 1-4 over the new xtask/fuzz files)
! rg -n --glob '!**/tests/**' --glob '!*.md' --glob '!tests/ci/**' \
   -e 'TODO' -e 'lorem' -e 'unimplemented!\(\)' crates xtask fuzz

# coverage + deliverables updated for wave 5
for id in FR-06 FR-07 FR-13 FR-15 NFR-03 NFR-05 NFR-06 NFR-07 SEC-05 SEC-08; do
  rg -q "^\\| $id .*type-checked \\+ owner VM" docs/reports/coverage.md \
    || { echo "FAIL wave-5 coverage row: $id"; exit 1; }
done
for id in NFR-08 NFR-09 SEC-11 SEC-13; do
  rg -q "^\\| $id .*host-tested" docs/reports/coverage.md \
    || { echo "FAIL wave-5 host coverage row: $id"; exit 1; }
done
while IFS= read -r p; do
  [ -z "$p" ] && continue
  case "$p" in \#*) continue ;; esac
  test -s "$p" || { echo "FAIL missing/empty wave-5 deliverable: $p"; exit 1; }
done < tests/ci/deliverables.txt

echo "WAVE 5 INTEGRATION GATE PASS"

# --- CI installer seams (append to tests/ci/integration_gate.sh) ---
set -euo pipefail

WF=.github/workflows/build-installer.yml
NSIS=installer/kenali.nsi
CIDOC=docs/ci_installer.md

# files exist and are non-empty (static only — never run makensis/signtool here)
test -s "$WF" || { echo "FAIL missing/empty workflow: $WF"; exit 1; }
test -s "$NSIS" || { echo "FAIL missing/empty NSIS script: $NSIS"; exit 1; }
test -s "$CIDOC" || { echo "FAIL missing/empty CI doc: $CIDOC"; exit 1; }

# 1. YAML parse: triggers + job id + runs-on (PyYAML; rg/test fallback)
if python3 -c 'import yaml' >/dev/null 2>&1; then
  python3 - "$WF" <<'PY'
import sys, yaml
with open(sys.argv[1], encoding='utf-8') as fh:
    d = yaml.safe_load(fh)
trig = d.get('on', d.get(True))
assert 'workflow_dispatch' in trig, 'missing workflow_dispatch trigger'
assert 'main' in trig['push']['branches'], 'push.branches missing main'
assert 'v*' in trig['push']['tags'], 'push.tags missing v*'
j = d['jobs']['build-installer']
assert j['runs-on'] == 'windows-latest', 'runs-on is not windows-latest'
PY
else
  rg -q -F -- 'workflow_dispatch:' "$WF" || { echo "FAIL workflow trigger: workflow_dispatch"; exit 1; }
  rg -q -F -- 'main' "$WF" || { echo "FAIL workflow push.branches: main"; exit 1; }
  rg -q -F -- 'v*' "$WF" || { echo "FAIL workflow push.tags: v*"; exit 1; }
  rg -q -F -- 'build-installer:' "$WF" || { echo "FAIL workflow job id: build-installer"; exit 1; }
  rg -q -F -- 'runs-on: windows-latest' "$WF" || { echo "FAIL workflow runs-on: windows-latest"; exit 1; }
fi

# 2. D1: NSIS tokens with exactly one OutFile (use -F -- for literal/-leading tokens)
test "$(rg -c '^[[:space:]]*OutFile\b' "$NSIS")" = 1 \
  || { echo "FAIL NSIS OutFile count (expected exactly one)"; exit 1; }
for t in \
  'RequestExecutionLevel admin' 'C:\Program Files\Kenali' 'KENALI_VERSION' \
  'KENALI_CLSID' 'KENALI_STAGE_DIR' 'KENALI_SVC_EXE' 'KENALI_CP_DLL' \
  'KENALI_UI_EXE' 'KENALI_CLI_EXE' 'kenali-svc.exe' 'kenali-cp.dll' \
  'kenali-ui.exe' 'kenali.exe' 'SMPROGRAMS' 'InprocServer32' \
  'Authentication\Credential Providers' 'HKLM\SOFTWARE\Kenali' \
  'PreviousVersion' 'KenaliSvc' 'sc.exe create' 'sc.exe delete' \
  'sc.exe start' 'sc.exe stop' 'Section Uninstall' '!error'; do
  rg -q -F -- "$t" "$NSIS" || { echo "FAIL NSIS token: $t"; exit 1; }
done

# 3. D2: workflow tokens + CLSID parse from KENALI_CP_CLSID
for t in \
  'windows-latest' '1.96.0' 'x86_64-pc-windows-msvc' '--locked' '--release' \
  'choco install nsis' 'secrets.KENALI_SIGN_CERT_B64' \
  'secrets.KENALI_SIGN_CERT_PASSWORD' 'kenali-signing.pfx' \
  'signtool.exe' '/fd SHA256' 'makensis.exe' 'KENALI_VERSION' \
  'KENALI_CLSID' 'KENALI_STAGE_DIR' 'KENALI_SVC_EXE' 'KENALI_CP_DLL' \
  'KENALI_UI_EXE' 'KENALI_CLI_EXE' 'Get-FileHash -Algorithm SHA256' \
  'SHA256SUMS.txt' 'actions/upload-artifact@v4' 'if: always()'; do
  rg -q -F -- "$t" "$WF" || { echo "FAIL workflow token: $t"; exit 1; }
done
rg -q -- 'KENALI_CP_CLSID' "$WF" || { echo "FAIL workflow CLSID parse: KENALI_CP_CLSID"; exit 1; }

# 3b. CP DLL staging name mapping: real Cargo output (kenali_cp) -> deliverable
# (kenali-cp). This guards the GitHub Action failure where packaging looked for
# a hyphenated source file that Cargo never emits.
rg -q -F 'kenali_cp.dll' xtask/src/package.rs \
  || { echo "FAIL package.rs missing real CP dll source name: kenali_cp.dll"; exit 1; }
rg -q -F 'kenali-cp.dll' xtask/src/package.rs \
  || { echo "FAIL package.rs missing CP dll deliverable name: kenali-cp.dll"; exit 1; }
rg -q -F 'kenali_cp.dll' "$WF" \
  || { echo "FAIL workflow fallback missing real CP dll source name: kenali_cp.dll"; exit 1; }
rg -q -F 'kenali-cp.dll' "$WF" \
  || { echo "FAIL workflow fallback missing CP dll deliverable name: kenali-cp.dll"; exit 1; }

# signtool location guard: CI locates signtool.exe under the Windows SDK and
# never relies on a bare signtool.exe from PATH for signing.
rg -q -F 'Locate signtool' "$WF" \
  || { echo "FAIL workflow missing signtool discovery step"; exit 1; }
rg -q -F 'Windows Kits' "$WF" \
  || { echo "FAIL workflow signtool search path missing Windows Kits"; exit 1; }
rg -q -F 'SIGNTOOL' "$WF" \
  || { echo "FAIL workflow missing SIGNTOOL environment token"; exit 1; }
rg -q -F '& $env:SIGNTOOL' "$WF" \
  || { echo "FAIL workflow signing invocation missing $env:SIGNTOOL"; exit 1; }
! rg -q -F '& signtool.exe' "$WF" \
  || { echo "FAIL workflow still uses bare & signtool.exe"; exit 1; }

# makensis location guard: after choco install, CI resolves makensis.exe from a
# known absolute install path (or PATH) and never relies on a bare name that may
# not be on the runner's PATH.
rg -q -F 'Locate makensis' "$WF" \
  || { echo "FAIL workflow missing makensis discovery step"; exit 1; }
rg -q -F 'MAKENSIS' "$WF" \
  || { echo "FAIL workflow missing MAKENSIS environment token"; exit 1; }
rg -q -F '& $env:MAKENSIS' "$WF" \
  || { echo "FAIL workflow installer invocation missing $env:MAKENSIS"; exit 1; }
! rg -q -F '& makensis.exe' "$WF" \
  || { echo "FAIL workflow still uses bare & makensis.exe"; exit 1; }

# NSIS Include directory guard: nsExec.nsh (and other bundled headers) live in
# the Include directory next to makensis.exe. Derive it from MAKENSIS, fail
# loudly when missing, and pass it to the NSI script as /DKENALI_NSIS_INCLUDE;
# the script registers that directory with !addincludedir. Never edit the
# NSI script's include behavior at the command line.
rg -q -F '$nsisDir = Split-Path -Parent $env:MAKENSIS' "$WF" \
  || { echo "FAIL workflow NSIS include dir not derived from MAKENSIS"; exit 1; }
rg -q -F '$includeDir = Join-Path $nsisDir' "$WF" \
  || { echo "FAIL workflow NSIS include dir not joined to 'Include'"; exit 1; }
rg -q -F "'Include'" "$WF" \
  || { echo "FAIL workflow NSIS include dir missing 'Include' literal"; exit 1; }
rg -q -F 'Test-Path -LiteralPath $includeDir -PathType Container' "$WF" \
  || { echo "FAIL workflow NSIS include dir existence check missing"; exit 1; }
rg -q -F '/DKENALI_NSIS_INCLUDE=' "$WF" \
  || { echo "FAIL workflow makensis invocation missing /DKENALI_NSIS_INCLUDE define"; exit 1; }
! rg -q -F '/I$includeDir' "$WF" \
  || { echo "FAIL workflow still uses invalid /I$includeDir makensis option"; exit 1; }
rg -q -F '!addincludedir' "$NSIS" \
  || { echo "FAIL NSIS script missing !addincludedir"; exit 1; }
rg -q -F 'KENALI_NSIS_INCLUDE' "$NSIS" \
  || { echo "FAIL NSIS script missing KENALI_NSIS_INCLUDE"; exit 1; }

# nsExec dependency removed: NSIS core ExecWait replaces the nsExec plugin.
! rg -q -F 'nsExec.nsh' "$NSIS" \
  || { echo "FAIL NSIS script still includes nsExec.nsh"; exit 1; }
! rg -q -F 'nsExec::' "$NSIS" \
  || { echo "FAIL NSIS script still references nsExec::"; exit 1; }
rg -q -F 'ExecWait' "$NSIS" \
  || { echo "FAIL NSIS script missing built-in ExecWait"; exit 1; }

# CI doc owner-run warning
rg -qi 'owner[- ]run' "$CIDOC" || { echo "FAIL CI doc owner-run warning"; exit 1; }
rg -q -F -- 'docs/dev_safety_runbook.md' "$CIDOC" \
  || { echo "FAIL CI doc safety runbook reference"; exit 1; }
rg -qi 'kill switch|lockout|lock-out|recovery' "$CIDOC" \
  || { echo "FAIL CI doc recovery note"; exit 1; }

# 4. no secret literals or secret echoes in workflow, NSIS script or CI doc
! rg -n -e 'BEGIN (RSA )?PRIVATE KEY' -e 'MII[A-Za-z0-9+/]{80,}' \
    "$WF" "$NSIS" "$CIDOC"
! rg -n -i -e 'echo[^\n]*KENALI_SIGN_CERT' \
    -e 'Write-(Host|Output)[^\n]*KENALI_SIGN_CERT' \
    "$WF" "$NSIS" "$CIDOC"

echo "CI INSTALLER INTEGRATION GATE PASS"

# --- Pre-push hook seams (append to tests/ci/integration_gate.sh) ---
set -euo pipefail

HOOK=.husky/pre-push
VALIDATOR=scripts/validate-pre-push.sh
PKG=package.json
NSISDOC=tools/nsis/README.md

# static only: never execute the hook or the validator here
test -s "$PKG"          || { echo "FAIL missing package.json"; exit 1; }
test -s "$HOOK"         || { echo "FAIL missing pre-push hook: $HOOK"; exit 1; }
test -x "$HOOK"         || { echo "FAIL pre-push hook is not executable: $HOOK"; exit 1; }
test -s "$VALIDATOR"    || { echo "FAIL missing validator: $VALIDATOR"; exit 1; }
test -s "$NSISDOC"      || { echo "FAIL missing NSIS toolchain doc: $NSISDOC"; exit 1; }

# 1. Husky wiring: devDependency + prepare, so npm install installs the hooks
rg -q '"husky"' "$PKG"          || { echo "FAIL package.json missing husky devDependency"; exit 1; }
rg -q '"prepare"[[:space:]]*:[[:space:]]*"husky"' "$PKG" \
  || { echo "FAIL package.json prepare script does not run husky"; exit 1; }
for s in lint check validate; do
  rg -q "\"$s\"[[:space:]]*:" "$PKG" \
    || { echo "FAIL package.json missing script: $s"; exit 1; }
done
rg -q 'validate-pre-push\.sh' "$PKG" \
  || { echo "FAIL package.json scripts do not invoke the validator"; exit 1; }

# 2. hook invokes the validator directly, so it works without husky installed
rg -q 'pre-push' "$HOOK"           || { echo "FAIL hook missing pre-push marker"; exit 1; }
rg -q 'validate-pre-push\.sh' "$HOOK" \
  || { echo "FAIL hook does not invoke the validator"; exit 1; }
rg -q 'exit 1' "$HOOK"             || { echo "FAIL hook has no non-zero abort path"; exit 1; }
rg -q -- '--no-verify' "$HOOK"     || { echo "FAIL hook missing --no-verify escape hatch"; exit 1; }

# 3. validator covers every required step, portably and loudly
rg -q 'set -euo pipefail' "$VALIDATOR" \
  || { echo "FAIL validator missing set -euo pipefail"; exit 1; }
rg -q 'cargo fmt --check' "$VALIDATOR" \
  || { echo "FAIL validator missing cargo fmt --check"; exit 1; }
rg -q 'cargo check --target x86_64-pc-windows-msvc --locked --workspace' "$VALIDATOR" \
  || { echo "FAIL validator missing windows-target cargo check"; exit 1; }
rg -q 'tests/ci/integration_gate\.sh' "$VALIDATOR" \
  || { echo "FAIL validator missing integration_gate.sh"; exit 1; }
rg -q 'tests/ci/goal_check\.sh' "$VALIDATOR" \
  || { echo "FAIL validator missing goal_check.sh"; exit 1; }
rg -q 'kenali\.nsi' "$VALIDATOR" \
  || { echo "FAIL validator does not compile the NSIS installer"; exit 1; }
for d in KENALI_NSIS_INCLUDE KENALI_VERSION KENALI_CLSID KENALI_STAGE_DIR \
         KENALI_SVC_EXE KENALI_CP_DLL KENALI_UI_EXE KENALI_CLI_EXE; do
  rg -q "$d" "$VALIDATOR" \
    || { echo "FAIL validator missing NSIS define: $d"; exit 1; }
done
for m in PASS SKIP FAIL; do
  rg -q "$m" "$VALIDATOR" || { echo "FAIL validator missing $m summary line"; exit 1; }
done
rg -q 'tools/nsis' "$VALIDATOR" \
  || { echo "FAIL validator missing local NSIS toolchain lookup"; exit 1; }

# 4. the hook must skip (not fail) when the toolchain is unavailable offline,
#    and must fail a real compile error. Anchor both branches statically.
rg -q 'apt-get download' "$VALIDATOR" \
  || { echo "FAIL validator does not provision NSIS without root"; exit 1; }
rg -q 'dpkg-deb -x' "$VALIDATOR" \
  || { echo "FAIL validator does not unpack the NSIS debs"; exit 1; }
rg -qi 'skip|unavailable|offline' "$VALIDATOR" \
  || { echo "FAIL validator has no offline skip path"; exit 1; }

# 5. NSIS cache is gitignored (binaries never committed) but its README is not
rg -q -F 'tools/nsis' .gitignore \
  || { echo "FAIL .gitignore does not ignore the NSIS cache"; exit 1; }
rg -q 'node_modules' .gitignore \
  || { echo "FAIL .gitignore does not ignore node_modules"; exit 1; }
test -s "$NSISDOC" \
  || { echo "FAIL NSIS provisioning doc missing"; exit 1; }
rg -q 'apt-get download' "$NSISDOC" \
  || { echo "FAIL NSIS doc does not describe rootless provisioning"; exit 1; }
rg -qi 'offline' "$NSISDOC" \
  || { echo "FAIL NSIS doc does not describe offline behaviour"; exit 1; }
git check-ignore -q tools/nsis/bin/makensis \
  || { echo "FAIL git does not ignore provisioned makensis"; exit 1; }
! git check-ignore -q tools/nsis/README.md \
  || { echo "FAIL git ignores the tracked NSIS README"; exit 1; }

# 6. the CI doc describes the hook, what it checks and the bypass
rg -qi 'pre-push' "$CIDOC" \
  || { echo "FAIL CI doc does not mention the pre-push hook"; exit 1; }
rg -q -- '--no-verify' "$CIDOC" \
  || { echo "FAIL CI doc does not document the bypass"; exit 1; }

# 7. no secrets in the new files
! rg -n -e 'BEGIN (RSA )?PRIVATE KEY' -e 'MII[A-Za-z0-9+/]{80,}' \
    -e 'KENALI_SIGN_CERT_[A-Z_]*=[^$[:space:]]' \
    "$PKG" "$HOOK" "$VALIDATOR" "$NSISDOC" "$CIDOC" \
  || { echo "FAIL secret literal in pre-push tooling"; exit 1; }

echo "PRE-PUSH HOOK INTEGRATION GATE PASS"

