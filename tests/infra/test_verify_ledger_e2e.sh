#!/usr/bin/env bash
# Infrastructure test for task 7424 (verify instrumentation ledger, end to end).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which EXECUTES the
# real scripts/verify.sh `typecheck` with stub cargo/sccache.
#
# CLASSIFIED `pool` on the same basis as test_verify_background_sweep_ledger.sh:
# the vehicle holds no test-run semaphore slot, and the only lane-shared state
# it touches is the idempotent, flock-serialised tree-sitter-generate.sh.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_ledger_e2e.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_ledger_e2e ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_ledger_e2e.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_ledger_e2e.py"

test_summary
