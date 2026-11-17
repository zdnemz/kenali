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
