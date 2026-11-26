#!/usr/bin/env bash
# Infrastructure test for task #10333 (esc-9637-1: test_verify_nextest_absent_suites.sh
# leaves an EXIT RECORD naming its status, what was in flight and the failing
# command whenever it exits non-zero, and decodes a nested suite's signal death).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which runs a copy
# of the real harness against stubbed collaborators in a temp tree.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_nextest_absent_suites_exit_record.py
#      (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_nextest_absent_suites_exit_record ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_nextest_absent_suites_exit_record.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_nextest_absent_suites_exit_record.py"

test_summary
