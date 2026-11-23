#!/usr/bin/env bash
# Infrastructure test for task 10232 (the canonical-build-path primitive's cache
# half end to end: scripts/canon_run.py with scripts/canon_cache.py, real cargo
# and a real sccache at CANON in mount namespaces, against a private
# `rclone serve webdav` store; canonical-build-path PRD boundary rows 10 and
# 13-17).
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
#   2. tests/infra/test_canon_cache_host.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_canon_cache_host ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
rc=0
python3 "$SCRIPT_DIR/test_canon_cache_host.py" -v || rc=$?
assert "tests/infra/test_canon_cache_host.py exits 0" test "$rc" -eq 0

test_summary
