#!/usr/bin/env bash
# Infrastructure test for task 6059 (scripts/hooks-armed-guard.sh, the warm-lane
# stash-guard liveness detector).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py; without this
# file they would be silently never run, reading as coverage while asserting
# nothing.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_hooks_armed_guard ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_hooks_armed_guard.py exits 0" \
    python3 "$SCRIPT_DIR/test_hooks_armed_guard.py"

test_summary
