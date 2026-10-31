#!/usr/bin/env bash
# Infrastructure test for task 8476: the source mode of test_verify_scope.sh.
#
# This wrapper is the member run_all.sh actually discovers (it globs
# test_*.sh only); the assertions live in the sibling .py. Guards that
# sourcing test_verify_scope.sh defines every helper and runs no scenario, and
# that running it directly still reclaims its fixtures on exit.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_scope_source_mode ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_verify_scope_source_mode.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_scope_source_mode.py"

test_summary
