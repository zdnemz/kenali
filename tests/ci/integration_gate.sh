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
