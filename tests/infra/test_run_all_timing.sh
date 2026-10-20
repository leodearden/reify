#!/usr/bin/env bash
# Infrastructure test for task 7424 (run_all.sh per-member / per-phase timing).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which EXECUTES
# the real tests/infra/run_all.sh on a tempdir fixture INFRA_DIR.
#
# CLASSIFIED `pool`, like test_run_all.sh, which drives run_all.sh on fixtures
# the same way (private pool lock, isolated flaky ledger).
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_run_all_timing.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_run_all_timing ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_run_all_timing.py exits 0" \
    python3 "$SCRIPT_DIR/test_run_all_timing.py"

test_summary
