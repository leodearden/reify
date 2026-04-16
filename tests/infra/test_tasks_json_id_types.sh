#!/usr/bin/env bash
# Regression test: tasks.json ID types — every task and subtask `id` field
# must be a JSON string matching ^[0-9]+$.
#
# Task 1887: normalize subtask IDs to digit-strings.
# TOP-LEVEL IDs:  already digit-strings — Check 1 passes.
# SUBTASK IDs:    were integers before this PR; Check 2 was RED then and is
#                 GREEN now that all 8 subtask ids were normalized to digit-strings.

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

# Verify jq can actually parse the file; this fails loudly so subsequent
# pipe-based assertions (which use `! jq ... | grep`) can't silently pass
# on an empty stream if jq were to error out on a corrupt/missing file.
assert "tasks.json is valid JSON (jq empty)" \
    jq empty "$TASKS_JSON"

# -- Check 1: top-level task ids are digit-strings ----------------------------
echo ""
echo "--- Check 1: top-level task ids are digit-strings ---"

# Symmetric hardening to the Check 2 guard (esc-1887-53 — same silent-pass
# class): if tasks[] produces an empty stream, the pipe-based type and pattern
# checks trivially pass on empty input.  Assert a non-zero count first.
assert "tasks.json has at least one top-level task" \
    bash -c "[ \"\$(jq '[.master.tasks[]] | length' '$TASKS_JSON')\" -gt 0 ]"

# Verify no top-level id has JSON type "number".
assert "all top-level task ids have JSON type string (not number)" \
    bash -c "! jq -r '.master.tasks[].id | type' '$TASKS_JSON' | grep -q 'number'"

# Verify every top-level id value matches ^[0-9]+$ (guards against non-digit strings).
assert "all top-level task ids match ^[0-9]+\$" \
    bash -c "! jq -r '.master.tasks[].id' '$TASKS_JSON' | grep -vqE '^[0-9]+\$'"

# -- Check 2: subtask ids are digit-strings -----------------------------------
echo ""
echo "--- Check 2: subtask ids are digit-strings ---"

# Guard against the silent-pass hole (esc-1887-53): if subtasks[]? produces an
# empty stream (all subtask arrays are missing/empty), the pipe-based type and
# pattern checks trivially pass on empty input.  Assert a non-zero count first
# so any such degenerate state fails loudly.
assert "tasks.json has at least one subtask" \
    bash -c "[ \"\$(jq '[.master.tasks[].subtasks[]?] | length' '$TASKS_JSON')\" -gt 0 ]"

# Verify no subtask id has JSON type "number".
assert "all subtask ids have JSON type string (not number)" \
    bash -c "! jq -r '.master.tasks[].subtasks[]?.id | type' '$TASKS_JSON' | grep -q 'number'"

# Verify every subtask id value matches ^[0-9]+$ (guards against non-digit strings).
assert "all subtask ids match ^[0-9]+\$" \
    bash -c "! jq -r '.master.tasks[].subtasks[]?.id' '$TASKS_JSON' | grep -vqE '^[0-9]+\$'"

# -- Summary ------------------------------------------------------------------
test_summary
