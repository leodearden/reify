#!/usr/bin/env bash
# Infrastructure test for task 10232 (scripts/canon_cache.py: the per-policy
# cache environment, the compile guard, the private sccache session,
# supervision and the build_cache record; canonical-build-path PRD C4-C7).
# Task 10441 split the store link out into scripts/canon_store.py, whose suite
# is test_canon_store_units.sh.
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py; without this
# file they would be silently never run, reading as coverage while asserting
# nothing.
#
# The suite runs OUTSIDE `assert` so unittest's verbose output, including any
# skipped test's named reason and the final counts, reaches the log.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_canon_cache_units.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_canon_cache_units ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
rc=0
python3 "$SCRIPT_DIR/test_canon_cache_units.py" -v || rc=$?
assert "tests/infra/test_canon_cache_units.py exits 0" test "$rc" -eq 0

test_summary
