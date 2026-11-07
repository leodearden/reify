#!/usr/bin/env bash
# Adoption ratchet (PRD docs/prds/v0_6/run-all-runner-infra-harness.md §3 D9).
# This wrapper is the member run_all.sh discovers (classification_discovered_set
# globs test_*.sh only); the assertions live in the sibling .py.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_infra_adoption_ratchet ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_infra_adoption_ratchet.py exits 0" \
    python3 "$SCRIPT_DIR/test_infra_adoption_ratchet.py"

test_summary
