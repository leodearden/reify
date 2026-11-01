#!/usr/bin/env bash
# Infrastructure test for task 9097 (S18 β): the Python verify package's copies
# of reify-audit's ptodo, pdiag and pdoccover predicates
# (scripts/reify_verify/selection.py) select the leaves the Rust source reads
# (PRD docs/prds/v0_6/verify-structured-plan-and-role-table.md §2.3, §8, D13).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which drives
# `python3 -I -B scripts/verify.py ... --print-scope` as a subprocess against
# corpora tests/infra/reify_audit_inputs.py derives from the Rust source.
#
# CLASSIFIED `pool`: every scratch file (shared clone, private index copies)
# is private to its own tempdirs.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_selector_copies.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_selector_copies ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_selector_copies.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_selector_copies.py"

test_summary
