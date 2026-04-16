#!/usr/bin/env bash
# Meta-test for tests/infra/test_tasks_json_id_types.sh.
# Verifies that the non-zero count guards in Check 1 and Check 2 are present
# (structural) and that they cause the script to fail correctly on synthetic
# fixtures (behavioral).  Does NOT rely on the real tasks.json.
#
# Closes the silent-pass hole documented in esc-1887-53: when subtasks[]? or
# tasks[] produces an empty stream, pipe-based grep checks pass trivially on
# empty input.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
TARGET="$SCRIPT_DIR/test_tasks_json_id_types.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
[ -f "$TARGET" ] || {
    echo "ERROR: target script not found at $TARGET"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_tasks_json_id_types.sh meta-tests ==="

# -- Setup: temp-dir fixture machinery ----------------------------------------
_meta_tmpdir=$(mktemp -d)
trap 'rm -rf "$_meta_tmpdir"' EXIT

# mk_fixture_dir: create a self-contained temp-tree that test_tasks_json_id_types.sh
# can run from without touching the real tasks.json or real tests/infra/ directory.
# The script derives TASKS_JSON from SCRIPT_DIR/../../.taskmaster/tasks/tasks.json,
# so copying it into $d/tests/infra/ redirects it to $d/.taskmaster/tasks/tasks.json.
# Returns (echoes) the path to the temp dir.
mk_fixture_dir() {
    local d
    d=$(mktemp -d -p "$_meta_tmpdir")
    mkdir -p "$d/tests/infra"
    mkdir -p "$d/.taskmaster/tasks"
    cp "$TARGET" "$d/tests/infra/test_tasks_json_id_types.sh"
    cp "$SCRIPT_DIR/test_helpers.sh" "$d/tests/infra/test_helpers.sh"
    echo "$d"
}

# ==============================================================================
# Check 2 guards: subtask count
# ==============================================================================
echo ""
echo "--- Check 2 guard: structural presence ---"

# (a) Structural: the source script must contain the subtask-count guard jq
# expression.  Fails RED until step-2 adds the guard (esc-1887-53).
assert "test_tasks_json_id_types.sh contains subtask-count jq expression" \
    grep -qF '[.master.tasks[].subtasks[]?] | length' "$TARGET"

echo ""
echo "--- Check 2 guard: behavioral (empty subtasks -> rc != 0) ---"

# (b) Behavioral: a tasks.json with all-empty subtask arrays must cause the
# script to fail (rc != 0) and emit a FAIL line for the subtask-count guard.
# Fails RED until step-2 because the script currently exits 0 on this fixture
# (subtasks[]? suppresses iteration over empty arrays, pipe-based greps pass on
# empty input — the silent-pass hole described in esc-1887-53).
_chk2_dir=$(mk_fixture_dir)
printf '{"master":{"tasks":[{"id":"1","subtasks":[]}]}}\n' \
    > "$_chk2_dir/.taskmaster/tasks/tasks.json"
_chk2_rc=0
_chk2_out="$_meta_tmpdir/chk2_empty.out"
bash "$_chk2_dir/tests/infra/test_tasks_json_id_types.sh" > "$_chk2_out" 2>&1 || _chk2_rc=$?

assert "empty subtasks fixture: script exits non-zero" \
    test "$_chk2_rc" -ne 0

assert "empty subtasks fixture: FAIL line for subtask-count guard" \
    grep -q 'FAIL: tasks.json has at least one subtask' "$_chk2_out"

echo ""
echo "--- Check 2 guard: happy-path regression (valid fixture -> rc == 0) ---"

# (c) Happy-path regression: a valid tasks.json with digit-string IDs and
# non-empty subtask arrays must still pass all checks.  After step-2 the guard
# emits a PASS line; both the exit-code and the PASS line are pinned so the
# guard cannot regress.
_chk2_happy_dir=$(mk_fixture_dir)
printf '{"master":{"tasks":[{"id":"1","subtasks":[{"id":"101"}]},{"id":"2","subtasks":[{"id":"201"},{"id":"202"}]}]}}\n' \
    > "$_chk2_happy_dir/.taskmaster/tasks/tasks.json"
_chk2_happy_rc=0
_chk2_happy_out="$_meta_tmpdir/chk2_happy.out"
bash "$_chk2_happy_dir/tests/infra/test_tasks_json_id_types.sh" > "$_chk2_happy_out" 2>&1 || _chk2_happy_rc=$?

assert "valid fixture: script exits zero" \
    test "$_chk2_happy_rc" -eq 0

assert "valid fixture: PASS line for subtask-count guard" \
    grep -q 'PASS: tasks.json has at least one subtask' "$_chk2_happy_out"

# ==============================================================================
# Check 1 guards: top-level task count
# ==============================================================================
echo ""
echo "--- Check 1 guard: structural presence ---"

# (a) Structural: the source script must contain the top-level-task-count guard
# jq expression.  Fails RED until step-4 adds the symmetric guard.
assert "test_tasks_json_id_types.sh contains top-level-task-count jq expression" \
    grep -qF '[.master.tasks[]] | length' "$TARGET"

echo ""
echo "--- Check 1 guard: behavioral (empty tasks -> rc != 0) ---"

# (b) Behavioral: a tasks.json with an empty top-level tasks array must cause
# the script to fail (rc != 0) and emit a FAIL line for the top-level-task-count
# guard.  The rc check will pass incidentally after step-2 (Check 2's subtask
# guard fires first on zero tasks), but the FAIL-line check remains RED until
# step-4 inserts the Check 1 guard and its assertion description appears.
_chk1_dir=$(mk_fixture_dir)
printf '{"master":{"tasks":[]}}\n' \
    > "$_chk1_dir/.taskmaster/tasks/tasks.json"
_chk1_rc=0
_chk1_out="$_meta_tmpdir/chk1_empty.out"
bash "$_chk1_dir/tests/infra/test_tasks_json_id_types.sh" > "$_chk1_out" 2>&1 || _chk1_rc=$?

assert "empty tasks fixture: script exits non-zero" \
    test "$_chk1_rc" -ne 0

assert "empty tasks fixture: FAIL line for top-level-task-count guard" \
    grep -q 'FAIL: tasks.json has at least one top-level task' "$_chk1_out"

# -- Summary ------------------------------------------------------------------
test_summary
