#!/usr/bin/env bash
# Infrastructure test for task 9098 (S18 γ1): B1, plan parity — bash
# scripts/verify.sh and the Python verify package print the same --print-plan
# (PRD docs/prds/v0_6/verify-structured-plan-and-role-table.md D8, D13). The
# lint and typecheck grids are the sibling shards
# test_verify_plan_parity_{lint,typecheck}.sh.
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which drives
# `bash scripts/verify.sh ... --print-plan` and
# `python3 -I -B scripts/verify.py ... --print-plan` as subprocesses in a shared
# clone of the tree under test and compares them byte for byte.
#
# CLASSIFIED `pool`: every run is a --print-plan or an argv error, and every
# scratch file (shared clones, scratch HOMEs, FIFOs, PATH farms) is private to
# its own tempdirs.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_plan_parity.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_plan_parity ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_plan_parity.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_plan_parity.py"

test_summary
