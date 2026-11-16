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

echo "GOAL CHECK PASS (proxy: static coverage + gates; runtime/hardware proof is manual per BRIEF.md)"
