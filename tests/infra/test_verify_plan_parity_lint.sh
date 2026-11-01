#!/usr/bin/env bash
# Infrastructure test for task 9098 (S18 γ1): B1's lint grid shard — bash
# scripts/verify.sh and the Python verify package print the same lint
# --print-plan in every role × profile × scope cell (PRD
# docs/prds/v0_6/verify-structured-plan-and-role-table.md D8, D13). The core
# member is test_verify_plan_parity.sh.
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which drives
# `bash scripts/verify.sh lint ... --print-plan` and
# `python3 -I -B scripts/verify.py lint ... --print-plan` as subprocesses
# in a shared clone of the tree under test and compares them byte for byte.
#
# CLASSIFIED `pool`: every run is a --print-plan, and the shared clone is
# private to its own tempdir.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_plan_parity_lint.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_plan_parity_lint ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_plan_parity_lint.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_plan_parity_lint.py"

test_summary
