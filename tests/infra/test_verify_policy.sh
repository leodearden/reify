#!/usr/bin/env bash
# Infrastructure test for task 9095 (S18 α): the Python verify package's CLI,
# knob registry and role/profile policy tables
# (PRD docs/prds/v0_6/verify-structured-plan-and-role-table.md D14).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which drives
# `bash scripts/verify.sh` and `python3 -I -B scripts/verify.py` as
# subprocesses and compares their observable behaviour.
#
# CLASSIFIED `pool`: every bash run is an argv error or a --print-plan, and
# every scratch file (shared clone, FIFOs, PATH farms) is private to its own
# tempdirs.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_policy.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_policy ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_policy.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_policy.py"

test_summary
