#!/usr/bin/env bash
# Infrastructure test for task 10228 (scripts/lib_live_refs.sh sees processes
# that reach a lane through another mount namespace; canonical-build-path PRD
# decision 16 / C11).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py; without this
# file they would be silently never run, reading as coverage while asserting
# nothing.
#
# The suite runs OUTSIDE `assert` so unittest's verbose output, including each
# skipped test's named reason and the `OK (skipped=N)` count, reaches the log.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_live_refs_namespace.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_live_refs_namespace ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
rc=0
python3 "$SCRIPT_DIR/test_live_refs_namespace.py" -v || rc=$?
assert "tests/infra/test_live_refs_namespace.py exits 0" test "$rc" -eq 0

test_summary
