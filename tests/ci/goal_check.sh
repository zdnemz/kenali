#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

bash tests/ci/integration_gate.sh

# every planned deliverable (wave 1 seeded; later waves append) is present
while IFS= read -r p; do
  [ -z "$p" ] && continue
  case "$p" in \#*) continue ;; esac
  test -e "$p" || { echo "FAIL missing deliverable: $p"; exit 1; }
done < tests/ci/deliverables.txt

# every PRD requirement id is tracked in the coverage manifest with a file/test reference
for id in $(rg -o '(FR|NFR|SEC)-[0-9]{2}' docs/prd.md | sort -u); do
  rg -q "^\\| *$id *\\|" docs/reports/coverage.md || { echo "FAIL untracked requirement: $id"; exit 1; }
done

# requirement ids tagged implementable-here in .crew/PRD.md must have a test on main
for tag in FR-04 FR-11 FR-13 SEC-02 SEC-07 SEC-09 SEC-10 SEC-11 SEC-12 SEC-15 SEC-18 SEC-19 SEC-20; do
  rg -q "$tag" docs/reports/coverage.md || { echo "FAIL no host-testable coverage for $tag"; exit 1; }
done

# --- Wave 2 additions to the goal-check proxy (append to tests/ci/goal_check.sh) ---

# wave-2 deliverables must exist on main, not just be planned
for p in \
  crates/kenali-cp/src/guard.rs \
  crates/kenali-cp/src/provider.rs \
  crates/kenali-cp/src/credential.rs \
  crates/kenali-cp/src/scenarios.rs \
  crates/kenali-cp/src/kill_switch.rs \
  crates/kenali-cp/src/allowed_sid.rs \
  crates/kenali-winapi/src/pipes.rs \
  crates/kenali-winapi/src/registry.rs \
  crates/kenali-winapi/src/service.rs \
  xtask/src/install.rs \
  docs/spikes/P0-S1.md; do
  test -s "$p" || { echo "FAIL missing wave-2 deliverable: $p"; exit 1; }
done

# wave-2 coverage rows updated to the pinned status token
for id in FR-02 FR-03 FR-10 FR-16 SEC-03 SEC-06 SEC-17; do
  rg -q "^\\| $id .*type-checked \\+ owner VM" docs/reports/coverage.md \
    || { echo "FAIL wave-2 coverage row not updated: $id"; exit 1; }
done

# --- Wave 3 additions to the goal-check proxy (append to tests/ci/goal_check.sh) ---

# wave-3 deliverables must exist on main, not just be planned
for p in \
  crates/kenali-winapi/src/pipe_server.rs \
  crates/kenali-winapi/src/tpm.rs \
  crates/kenali-winapi/src/eventlog.rs \
  crates/kenali-winapi/src/service_host.rs \
  crates/kenali-camera/src/mf.rs \
  crates/kenali-camera/src/replay.rs \
  crates/kenali-camera/src/synthetic.rs \
  crates/kenali-camera/src/pin.rs \
  crates/kenali-vision/src/onnx.rs \
  crates/kenali-service/src/pipe_loop.rs \
  crates/kenali-service/src/auth_attempt.rs \
  crates/kenali-service/src/template_store.rs \
  crates/kenali-service/src/pipeline.rs \
  crates/kenali-bench/src/latency.rs \
  crates/kenali-bench/src/calibration.rs \
  models/models.lock \
  docs/spikes/P0-S2.md \
  docs/spikes/P0-S3.md \
  docs/spikes/P0-S4.md \
  docs/spikes/P0-S5.md \
  docs/spikes/P0-S10.md \
  docs/spikes/P0-S11.md; do
  test -s "$p" || { echo "FAIL missing wave-3 deliverable: $p"; exit 1; }
done

# wave-3 coverage rows updated to the pinned status token
for id in FR-08 FR-12 SEC-01 SEC-14 SEC-16 NFR-01 NFR-02 NFR-04; do
  rg -q "^\\| $id .*type-checked \\+ owner VM" docs/reports/coverage.md \
    || { echo "FAIL wave-3 coverage row not updated: $id"; exit 1; }
done

echo "GOAL CHECK PASS (proxy: Wave 1 + Wave 2 + Wave 3 static coverage and gates; runtime/hardware proof is manual per BRIEF.md)"
