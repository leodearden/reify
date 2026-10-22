#!/usr/bin/env bash
# Infrastructure test for task 9097 (S18 β): the scope section of the Python
# verify package's `--print-scope` record — the CLI surface, the scope decision
# and the D6 fixture coupling (PRD
# docs/prds/v0_6/verify-structured-plan-and-role-table.md D5, D6, D11, B7).
# The narrowing and infra_selection sections are test_verify_narrowing_selection.sh's.
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which drives
# `python3 -I -B scripts/verify.py ... --print-scope` and
# `bash scripts/verify.sh ... --print-plan` as subprocesses and compares them.
#
# CLASSIFIED `pool`: every bash run is an argv error or a --print-plan, and
# every scratch file (shared clone, PATH farms) is private to its own tempdirs.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_scope_record.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_scope_record ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_scope_record.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_scope_record.py"

test_summary
