#!/usr/bin/env bash
# Infrastructure test for task 10282 (helper .py scripts live in a private dir).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py. Rationale and
# scope: tests/infra/README.md "Python helper scripts live in a private
# directory".

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_no_shared_tmp_python_helpers ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests + real-tree scan ───────────────────────────────────────────
assert "tests/infra/test_no_shared_tmp_python_helpers.py exits 0" \
    python3 "$SCRIPT_DIR/test_no_shared_tmp_python_helpers.py"

test_summary
