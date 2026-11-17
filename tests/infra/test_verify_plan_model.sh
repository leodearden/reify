#!/usr/bin/env bash
# Infrastructure test for tasks 9098 and 9099 (S18 γ1, γ2): the Python verify
# package's plan model, in process — the records' invariants, build_steps'
# gate-path declarations, PlanInputs' test-region invariant,
# nextest.nextest_config_path's three arms and bashcall.source_delta (PRD
# docs/prds/v0_6/verify-structured-plan-and-role-table.md D2, D8, D12).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which imports
# scripts/reify_verify and drives its functions directly.
#
# CLASSIFIED `pool`: every scratch $HOME and TMPDIR is private to its own
# tempdir.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_plan_model.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_plan_model ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_plan_model.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_plan_model.py"

test_summary
