#!/usr/bin/env bash
# Tests for scripts/normalize_tasks_json.py.
# Verifies: existence, executability, numeric-to-string conversion for
# top-level task IDs and subtask IDs.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
NORMALIZE="$REPO_ROOT/scripts/normalize_tasks_json.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== normalize_tasks_json.py tests ==="

# -- Setup: temp-dir fixture machinery ----------------------------------------
_tmpdir=$(mktemp -d)
trap 'rm -rf "$_tmpdir"' EXIT

# ==============================================================================
# Check 0: script exists and is executable
# ==============================================================================
echo ""
echo "--- Check 0: script existence and executability ---"

assert "scripts/normalize_tasks_json.py exists" \
    test -f "$NORMALIZE"

assert "scripts/normalize_tasks_json.py is executable" \
    test -x "$NORMALIZE"

# ==============================================================================
# Check 1: numeric top-level task id is converted to JSON string type
# ==============================================================================
echo ""
echo "--- Check 1: numeric top-level task id -> JSON string ---"

_fix1="$_tmpdir/fix_toplevel.json"
printf '{"master":{"tasks":[{"id":5,"subtasks":[]}]}}\n' > "$_fix1"

assert "normalize converts numeric top-level id to string" \
    bash -c "python3 '$NORMALIZE' '$_fix1' && [ \"\$(jq -r '.master.tasks[0].id | type' '$_fix1')\" = 'string' ]"

# ==============================================================================
# Check 2: numeric subtask id is converted to JSON string type
# ==============================================================================
echo ""
echo "--- Check 2: numeric subtask id -> JSON string ---"

_fix2="$_tmpdir/fix_subtask.json"
printf '{"master":{"tasks":[{"id":"1","subtasks":[{"id":7}]}]}}\n' > "$_fix2"

assert "normalize converts numeric subtask id to string" \
    bash -c "python3 '$NORMALIZE' '$_fix2' && [ \"\$(jq -r '.master.tasks[0].subtasks[0].id | type' '$_fix2')\" = 'string' ]"

# -- Summary ------------------------------------------------------------------
test_summary
