#!/usr/bin/env bash
# Infrastructure test for task 9765 (the built-tree arm of
# scripts/refresh-warm-base.sh's inv.9 provenance guard: a target whose
# reify-verify-attempt.json names a tree other than --landed-commit's never
# advances the base).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py; without this
# file they would be silently never run, reading as coverage while asserting
# nothing.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_refresh_warm_base_built_tree.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_refresh_warm_base_built_tree ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_refresh_warm_base_built_tree.py exits 0" \
    python3 "$SCRIPT_DIR/test_refresh_warm_base_built_tree.py"

test_summary
