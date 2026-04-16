#!/usr/bin/env bash
# Regression test: tasks.json ID types — every task and subtask `id` field
# must be a JSON string matching ^[0-9]+$.
#
# Task 1887: normalize subtask IDs to digit-strings.
# TOP-LEVEL IDs:  already digit-strings — Check 1 passes immediately.
# SUBTASK IDs:    integers prior to fix — Check 2 fails until step-2 impl.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TASKS_JSON="$REPO_ROOT/.taskmaster/tasks/tasks.json"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== tasks.json ID type tests ==="

# -- Check 0: prerequisites ---------------------------------------------------
echo ""
echo "--- Check 0: prerequisites ---"

assert "tasks.json exists at .taskmaster/tasks/tasks.json" \
    test -f "$TASKS_JSON"

assert "jq is available on PATH" \
    bash -c "command -v jq >/dev/null 2>&1"

# -- Check 1: top-level task ids are digit-strings ----------------------------
echo ""
echo "--- Check 1: top-level task ids are digit-strings ---"

# Verify no top-level id has JSON type "number".
assert "all top-level task ids have JSON type string (not number)" \
    bash -c "! jq -r '.master.tasks[].id | type' '$TASKS_JSON' | grep -q 'number'"

# Verify every top-level id value matches ^[0-9]+$ (guards against non-digit strings).
assert "all top-level task ids match ^[0-9]+\$" \
    bash -c "! jq -r '.master.tasks[].id' '$TASKS_JSON' | grep -vqE '^[0-9]+\$'"

# -- Check 2: subtask ids are digit-strings -----------------------------------
echo ""
echo "--- Check 2: subtask ids are digit-strings ---"

# Verify no subtask id has JSON type "number".
# FAILS (RED) until step-2 converts the 8 integer subtask ids to strings.
assert "all subtask ids have JSON type string (not number)" \
    bash -c "! jq -r '.master.tasks[].subtasks[]?.id | type' '$TASKS_JSON' | grep -q 'number'"

# Verify every subtask id value matches ^[0-9]+$ (guards against non-digit strings).
assert "all subtask ids match ^[0-9]+\$" \
    bash -c "! jq -r '.master.tasks[].subtasks[]?.id' '$TASKS_JSON' | grep -vqE '^[0-9]+\$'"

# -- Summary ------------------------------------------------------------------
test_summary
