#!/usr/bin/env bash
# Infrastructure test for task 10229 (scripts/canon_run.py with real cargo and
# real mount namespaces: a build at CANON under the `incremental` policy, and
# the real-path and refusal rows; canonical-build-path PRD boundary rows 9, 11,
# 12 and 17).
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
#   2. tests/infra/test_canon_run_host.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_canon_run_host ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
rc=0
python3 "$SCRIPT_DIR/test_canon_run_host.py" -v || rc=$?
assert "tests/infra/test_canon_run_host.py exits 0" test "$rc" -eq 0

test_summary
