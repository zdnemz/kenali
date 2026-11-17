#!/usr/bin/env bash
# Local pre-push validation for Kenali.
#
# Runs the same class of checks as GitHub Actions *before* the push, so a
# failure that CI would report is caught locally instead. The NSIS compile in
# particular catches the installer-script failures that reached CI repeatedly
# (invalid macros such as ${GetFullPath}, commands used from the wrong section,
# and missing staged files).
#
# Each check reports PASS, SKIP or FAIL. SKIP is used only when the local
# environment cannot run the check (no cargo, no PyYAML, no NSIS reachable
# offline); it never blocks a push. Any FAIL aborts the push.
#
# Escape hatch: `git push --no-verify`.
set -euo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$REPO_ROOT"

PASS=0
SKIP=0
FAIL=0
FAILED_NAMES=()

pass() { PASS=$((PASS + 1)); printf 'PASS  %s\n' "$1"; }
skip() { SKIP=$((SKIP + 1)); printf 'SKIP  %s (%s)\n' "$1" "$2"; }
fail() {
  FAIL=$((FAIL + 1))
  FAILED_NAMES+=("$1")
  printf 'FAIL  %s\n' "$1"
}

# Run a check function that returns 0 (pass), 1 (fail) or 2 (skip).
report() {
  local name=$1 fn=$2 reason=$3
  local rc=0
  "$fn" || rc=$?
  case "$rc" in
    0) pass "$name" ;;
    2) skip "$name" "$reason" ;;
    *) fail "$name" ;;
  esac
}

# ---------------------------------------------------------------------------
# NSIS toolchain provisioning
#
# Third-party binaries are never committed. When no system makensis is usable,
# the Ubuntu `nsis` + `nsis-common` .debs are downloaded with `apt-get download`
# (no root needed) and unpacked with `dpkg-deb -x` into the gitignored
# `tools/nsis/` cache:
#
#   tools/nsis/bin/makensis
#   tools/nsis/share/nsis/Include
#   tools/nsis/share/nsis/Stubs
#   tools/nsis/lib/            (only if the .debs carry shared libraries)
#
# `NSISDIR` must point at the directory that contains Include/ and Stubs/.
# Offline: a populated tools/nsis/ is used as-is; if it is absent and the
# download fails, the compile is skipped with a loud warning.
# ---------------------------------------------------------------------------
provision_nsis() {
  local vendored="$REPO_ROOT/tools/nsis"
  local tmp rc=0

  command -v apt-get >/dev/null 2>&1 || return 1
  command -v dpkg-deb >/dev/null 2>&1 || return 1

  tmp=$(mktemp -d "${TMPDIR:-/tmp}/kenali-nsis-provision.XXXXXX") || return 1

  # NOTE: this function runs in a condition context (`if provision_nsis`),
  # where bash suppresses `set -e`. Every step is therefore checked explicitly
  # and the cache is only replaced after the staged toolchain proves it runs,
  # so a partial download can never be mistaken for a usable toolchain.
  (
    cd "$tmp" || exit 1
    mkdir -p debs extract || exit 1
    ( cd debs && apt-get download nsis nsis-common ) || exit 1
    for deb in debs/*.deb; do
      [ -f "$deb" ] || exit 1
      dpkg-deb -x "$deb" extract || exit 1
    done
    test -x extract/usr/bin/makensis || exit 1
    test -d extract/usr/share/nsis/Include || exit 1
    test -d extract/usr/share/nsis/Stubs || exit 1

    mkdir -p stage/bin stage/share/nsis || exit 1
    cp extract/usr/bin/makensis stage/bin/makensis || exit 1
    chmod +x stage/bin/makensis || exit 1
    cp -a extract/usr/share/nsis/Include stage/share/nsis/Include || exit 1
    cp -a extract/usr/share/nsis/Stubs stage/share/nsis/Stubs || exit 1
    if [ -d extract/usr/lib/x86_64-linux-gnu ]; then
      mkdir -p stage/lib || exit 1
      cp -a extract/usr/lib/x86_64-linux-gnu/. stage/lib/ || exit 1
    fi

    # Prove the staged toolchain executes before touching the cache.
    NSISDIR="$tmp/stage/share/nsis" \
      LD_LIBRARY_PATH="$tmp/stage/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
      "$tmp/stage/bin/makensis" -VERSION >/dev/null 2>&1 || exit 1

    rm -rf "$vendored/bin" "$vendored/share" "$vendored/lib" || exit 1
    mkdir -p "$vendored" || exit 1
    cp -a stage/bin "$vendored/bin" || exit 1
    cp -a stage/share "$vendored/share" || exit 1
    if [ -d stage/lib ]; then
      cp -a stage/lib "$vendored/lib" || exit 1
    fi
    exit 0
  ) || rc=$?

  rm -rf "$tmp"
  return "$rc"
}

# Resolve a usable makensis and its NSISDIR. Returns 2 when none is usable.
resolve_nsis() {
  local vendored="$REPO_ROOT/tools/nsis"

  if [ -x "$vendored/bin/makensis" ] \
     && [ -d "$vendored/share/nsis/Include" ] \
     && [ -d "$vendored/share/nsis/Stubs" ]; then
    NSIS_BIN="$vendored/bin/makensis"
    NSIS_DIR="$vendored/share/nsis"
    NSIS_LIB=""
    [ -d "$vendored/lib" ] && NSIS_LIB="$vendored/lib"
    return 0
  fi

  printf '      tools/nsis/ is not populated; attempting to provision it (needs network)...\n'
  if provision_nsis; then
    NSIS_BIN="$vendored/bin/makensis"
    NSIS_DIR="$vendored/share/nsis"
    NSIS_LIB=""
    [ -d "$vendored/lib" ] && NSIS_LIB="$vendored/lib"
    printf '      provisioned NSIS into tools/nsis/\n'
    return 0
  fi

  if command -v makensis >/dev/null 2>&1; then
    NSIS_BIN=$(command -v makensis)
    local cand
    for cand in \
      "$(dirname -- "$(dirname -- "$NSIS_BIN")")/share/nsis" \
      /usr/share/nsis \
      /usr/local/share/nsis; do
      if [ -d "$cand/Include" ] && [ -d "$cand/Stubs" ]; then
        NSIS_DIR="$cand"
        return 0
      fi
    done
    printf '      found %s but no NSISDIR with Include/ and Stubs/\n' "$NSIS_BIN"
  fi

  return 2
}

# NSIS compile with a temporary dummy stage (four binaries + SBOM).
# Returns 0 (pass), 1 (compile error) or 2 (toolchain unavailable).
nsis_check() {
  local tag value
  local stage version clsid log rc

  # Defines are required by installer/kenali.nsi; a stale name here must be a
  # hard failure, so verify the script's own contract before compiling.
  for tag in \
    KENALI_NSIS_INCLUDE KENALI_VERSION KENALI_CLSID KENALI_STAGE_DIR \
    KENALI_SVC_EXE KENALI_CP_DLL KENALI_UI_EXE KENALI_CLI_EXE; do
    if ! grep -q "$tag" installer/kenali.nsi; then
      printf '      installer/kenali.nsi no longer expects %s\n' "$tag"
      return 1
    fi
  done

  resolve_nsis || return 2
  printf '      makensis: %s\n' "$NSIS_BIN"

  local nsis_env=(NSISDIR="$NSIS_DIR")
  if [ -n "${NSIS_LIB:-}" ]; then
    nsis_env+=("LD_LIBRARY_PATH=$NSIS_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}")
  fi

  if ! env "${nsis_env[@]}" "$NSIS_BIN" -VERSION >/dev/null 2>&1; then
    printf '      WARNING: makensis cannot run (%s -VERSION failed); missing shared libraries?\n' "$NSIS_BIN"
    return 2
  fi

  version=$(awk -F'"' '/^\[workspace\.package\]/{f=1;next} /^\[/{f=0} f && /^version[ \t]*=/{print $2; exit}' Cargo.toml || true)
  [ -n "$version" ] || version=0.0.0-precheck
  clsid=$(sed -n 's/.*KENALI_CP_CLSID[[:space:]]*:[[:space:]]*&str[[:space:]]*=[[:space:]]*"\([0-9a-f][0-9a-f-]*\)".*/\1/p' \
            crates/kenali-winapi/src/com.rs | head -n1 || true)
  [ -n "$clsid" ] || clsid=00000000-0000-0000-0000-000000000000

  stage=$(mktemp -d "${TMPDIR:-/tmp}/kenali-nsis-stage.XXXXXX") || return 1
  log="$stage/makensis.log"

  # Dummy staged release files so the absolute File instructions resolve.
  for value in kenali-svc.exe kenali-cp.dll kenali-ui.exe kenali.exe; do
    printf 'pre-push dummy\n' > "$stage/$value"
  done
  printf '<bom/>\n' > "$stage/kenali-$version.cdx.xml"

  rc=0
  ( cd "$stage" && exec env "${nsis_env[@]}" "$NSIS_BIN" -NOCD \
      "-DKENALI_NSIS_INCLUDE=$NSIS_DIR/Include" \
      "-DKENALI_VERSION=$version" \
      "-DKENALI_CLSID=$clsid" \
      "-DKENALI_STAGE_DIR=$stage" \
      "-DKENALI_SVC_EXE=$stage/kenali-svc.exe" \
      "-DKENALI_CP_DLL=$stage/kenali-cp.dll" \
      "-DKENALI_UI_EXE=$stage/kenali-ui.exe" \
      "-DKENALI_CLI_EXE=$stage/kenali.exe" \
      "$REPO_ROOT/installer/kenali.nsi" ) >"$log" 2>&1 || rc=$?

  if [ "$rc" -eq 0 ] \
     && [ -s "$stage/kenali-$version-x86_64-pc-windows-msvc-setup.exe" ]; then
    printf '      compiled installer/kenali.nsi -> kenali-%s-%s-setup.exe\n' \
      "$version" x86_64-pc-windows-msvc
    rm -rf "$stage"
    return 0
  fi

  printf '      makensis failed (exit %s); last 40 lines:\n' "$rc"
  tail -n 40 "$log" | sed 's/^/        /'
  rm -rf "$stage"
  return 1
}

# Workflow structure assertions (mirrors the CI installer gate).
# Returns 2 when python3/PyYAML is unavailable.
yaml_check() {
  command -v python3 >/dev/null 2>&1 || return 2
  python3 -c 'import yaml' >/dev/null 2>&1 || return 2
  python3 - <<'PY'
import yaml

with open('.github/workflows/build-installer.yml', encoding='utf-8') as fh:
    wf = yaml.safe_load(fh)

trig = wf.get('on', wf.get(True))
assert 'workflow_dispatch' in trig, 'missing workflow_dispatch trigger'
assert 'main' in trig['push']['branches'], 'push.branches missing main'
assert 'v*' in trig['push']['tags'], 'push.tags missing v*'

job = wf['jobs']['build-installer']
assert job['runs-on'] == 'windows-latest', 'runs-on is not windows-latest'

steps = yaml.safe_dump(job)
for token in ('choco install nsis', 'Locate makensis', 'Locate signtool',
              '/DKENALI_NSIS_INCLUDE=', 'secrets.KENALI_SIGN_CERT_B64'):
    assert token in steps, 'workflow missing step token: ' + token

print('      workflow triggers, job and installer steps look correct')
PY
}

fmt_check() {
  command -v cargo >/dev/null 2>&1 || return 2
  cargo fmt --version >/dev/null 2>&1 || return 2
  cargo fmt --check
}

cargo_check() {
  command -v cargo >/dev/null 2>&1 || return 2
  cargo check --target x86_64-pc-windows-msvc --locked --workspace
}

gate_check() {
  bash "$1"
}

# ---------------------------------------------------------------------------
# Checks
# ---------------------------------------------------------------------------
printf 'Kenali pre-push validation\n'
printf 'repo: %s\n' "$REPO_ROOT"

printf '\n==> cargo fmt --check\n'
report 'cargo fmt --check' fmt_check \
  'cargo or rustfmt not installed (rustup component add rustfmt)'

printf '\n==> cargo check --target x86_64-pc-windows-msvc --locked --workspace\n'
report 'cargo check x86_64-pc-windows-msvc' cargo_check 'cargo not installed'

printf '\n==> repository gates\n'
for gate in tests/ci/integration_gate.sh tests/ci/goal_check.sh; do
  if ( gate_check "$gate" ); then
    pass "bash $gate"
  else
    fail "bash $gate"
  fi
done

printf '\n==> GitHub Actions workflow YAML\n'
report 'workflow YAML assertions' yaml_check \
  'python3 or PyYAML not available (pip install pyyaml)'

printf '\n==> NSIS installer compile (installer/kenali.nsi)\n'
report 'NSIS installer compile' nsis_check \
  'no usable NSIS toolchain (offline and tools/nsis/ not provisioned)'

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
printf '\n----------------------------------------\n'
printf 'pre-push summary: %d passed, %d skipped, %d failed\n' "$PASS" "$SKIP" "$FAIL"
if [ "$FAIL" -gt 0 ]; then
  printf 'failed checks:\n'
  printf '  - %s\n' "${FAILED_NAMES[@]}"
  printf 'fix the failures above, or bypass once with: git push --no-verify\n'
  exit 1
fi
printf 'all runnable checks passed\n'
