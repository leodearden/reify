#!/usr/bin/env bash
# Infrastructure test for task 9099 (S18 γ2): B1's failed_only retry member —
# bash scripts/verify.sh and the Python verify package print the same test and
# all --print-plan under REIFY_VERIFY_RETRY_SCOPE=failed_only: the attempt
# sidecar's tree-oid gate, each profile's exact-id subset and its refusals, and
# the @@REIFY_RETRY_SCOPE@@ marker (PRD
# docs/prds/v0_6/verify-structured-plan-and-role-table.md D8, D13). The core
# member is test_verify_plan_parity.sh.
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which drives
# `bash scripts/verify.sh test ... --print-plan` and
# `python3 -I -B scripts/verify.py test ... --print-plan` as subprocesses
# in a shared clone of the tree under test and compares them byte for byte.
#
# CLASSIFIED `pool`: every run is a --print-plan, and the shared clone, the
# scratch sidecars and filter files, and the stub cargo and HOME are private
# to their own tempdirs.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_plan_parity_retry.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_plan_parity_retry ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_plan_parity_retry.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_plan_parity_retry.py"

test_summary
