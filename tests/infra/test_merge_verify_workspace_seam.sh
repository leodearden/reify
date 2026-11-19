#!/usr/bin/env bash
# Infrastructure test for task 9871 (esc-9761-1): dark-factory-orchestrator.yaml's
# top-level `merge_verify_workspace: true`, which makes dark-factory's merge role
# run the yaml's global verify commands verbatim, and the premise it relies on
# (the merge-role test_command plans the whole-workspace nextest).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py; without this
# file they would be silently never run, reading as coverage while asserting
# nothing.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_merge_verify_workspace_seam.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_merge_verify_workspace_seam ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_merge_verify_workspace_seam.py exits 0" \
    python3 "$SCRIPT_DIR/test_merge_verify_workspace_seam.py"

test_summary
