#!/usr/bin/env bash
# Tests for scripts/normalize_tasks_json.py.
# Verifies: existence, executability, numeric-to-string conversion for
# top-level task IDs and subtask IDs, idempotence, already-string no-op,
# and non-id field preservation through round-trip.
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

# ==============================================================================
# Check 3: idempotence — running the script twice produces byte-identical output
# ==============================================================================
echo ""
echo "--- Check 3: idempotence ---"

_fix3="$_tmpdir/fix_idem.json"
printf '{"master":{"tasks":[{"id":9,"subtasks":[{"id":3}]}]}}\n' > "$_fix3"

# First run: normalize numeric -> string
python3 "$NORMALIZE" "$_fix3"
# Snapshot after first run
_snap1="$_tmpdir/snap1.json"
cp "$_fix3" "$_snap1"

# Second run: should be no-op
python3 "$NORMALIZE" "$_fix3"

assert "second normalize run produces byte-identical output (idempotent)" \
    cmp -s "$_snap1" "$_fix3"

# ==============================================================================
# Check 4: already-string fixture is unchanged byte-for-byte
# ==============================================================================
echo ""
echo "--- Check 4: already-string fixture is a no-op ---"

_fix4="$_tmpdir/fix_already_string.json"
printf '{"master":{"tasks":[{"id":"7","subtasks":[{"id":"8"}]}]}}\n' > "$_fix4"
_fix4_before="$_tmpdir/fix_already_string_before.json"
cp "$_fix4" "$_fix4_before"

python3 "$NORMALIZE" "$_fix4"

assert "already-string fixture unchanged byte-for-byte after normalize" \
    cmp -s "$_fix4_before" "$_fix4"

# ==============================================================================
# Check 5: non-id fields preserved through round-trip
# ==============================================================================
echo ""
echo "--- Check 5: non-id field preservation ---"

_fix5="$_tmpdir/fix_fields.json"
cat > "$_fix5" <<'FIXTURE'
{"master":{"tasks":[{"id":42,"title":"My task","status":"pending","dependencies":[1,2],"metadata":{"modules":["crates/foo"]},"subtasks":[{"id":10,"title":"Sub","status":"done"}]}]}}
FIXTURE

python3 "$NORMALIZE" "$_fix5"

assert "title field preserved after normalize" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].title' '$_fix5')\" = 'My task' ]"

assert "status field preserved after normalize" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].status' '$_fix5')\" = 'pending' ]"

assert "dependencies field preserved after normalize" \
    bash -c "[ \"\$(jq '.master.tasks[0].dependencies | length' '$_fix5')\" = '2' ]"

assert "metadata.modules field preserved after normalize" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].metadata.modules[0]' '$_fix5')\" = 'crates/foo' ]"

assert "subtask title field preserved after normalize" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].subtasks[0].title' '$_fix5')\" = 'Sub' ]"

assert "subtask status field preserved after normalize" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].subtasks[0].status' '$_fix5')\" = 'done' ]"

assert "top-level id normalized to string in field-preservation fixture" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].id | type' '$_fix5')\" = 'string' ]"

assert "subtask id normalized to string in field-preservation fixture" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].subtasks[0].id | type' '$_fix5')\" = 'string' ]"

# -- Summary ------------------------------------------------------------------
test_summary
