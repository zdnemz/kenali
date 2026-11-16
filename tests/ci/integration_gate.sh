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
UNSAFE=$(rg -o --no-filename '\bunsafe[[:space:]]*\{' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src | wc -l)
SAFETY=$(rg -o --no-filename '// SAFETY:' crates/kenali-winapi/src crates/kenali-camera/src crates/kenali-cp/src | wc -l)
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
