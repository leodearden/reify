#!/usr/bin/env bash
# Infrastructure test for task 10442 (scripts/drop-canon-invocation-files.py,
# which removes canonical-build-path invocation files from a warm-lane copy,
# called by refresh-warm-base.sh Step 3a and by seed-warm-lane.sh after its
# clone).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py; without this
# file they would be silently never run, reading as coverage while asserting
# nothing.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_drop_canon_invocation_files.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_drop_canon_invocation_files ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_drop_canon_invocation_files.py exits 0" \
    python3 "$SCRIPT_DIR/test_drop_canon_invocation_files.py"

test_summary
