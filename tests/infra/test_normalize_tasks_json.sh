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

# ==============================================================================
# Check 6: multi-tag — non-master tag namespaces are also normalized
# ==============================================================================
echo ""
echo "--- Check 6: non-master tag namespace normalization ---"

_fix6="$_tmpdir/fix_multitag.json"
cat > "$_fix6" <<'FIXTURE'
{"master":{"tasks":[{"id":"1","subtasks":[]}]},"feature-x":{"tasks":[{"id":2,"subtasks":[{"id":3}]}]}}
FIXTURE

python3 "$NORMALIZE" "$_fix6"

assert "non-master tag: numeric task id is normalized to string" \
    bash -c "[ \"\$(jq -r '.\"feature-x\".tasks[0].id | type' '$_fix6')\" = 'string' ]"

assert "non-master tag: numeric subtask id is normalized to string" \
    bash -c "[ \"\$(jq -r '.\"feature-x\".tasks[0].subtasks[0].id | type' '$_fix6')\" = 'string' ]"

assert "master tag: unchanged by multi-tag run" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].id' '$_fix6')\" = '1' ]"

# ==============================================================================
# Check 7: malformed-schema robustness — non-conforming shapes are skipped
# ==============================================================================
echo ""
echo "--- Check 7: malformed-schema robustness ---"

# (a) Tag value is a string (not a dict) — must not crash, must be a no-op.
_fix7a="$_tmpdir/fix_tag_string.json"
printf '{"bad_tag":"not-a-dict","master":{"tasks":[{"id":"1","subtasks":[]}]}}\n' > "$_fix7a"
_fix7a_before="$_tmpdir/fix_tag_string_before.json"
cp "$_fix7a" "$_fix7a_before"

_fix7a_stderr="$_tmpdir/fix7a_stderr.txt"
assert "malformed (a): tag value is string — exits 0" \
    bash -c "python3 '$NORMALIZE' '$_fix7a' 2>'$_fix7a_stderr'"
assert "malformed (a): tag value is string — no Traceback on stderr" \
    bash -c "! grep -q 'Traceback' '$_fix7a_stderr'"
assert "malformed (a): tag value is string — file is byte-identical to input" \
    cmp -s "$_fix7a_before" "$_fix7a"

# (b) tasks field is a dict (not a list) — must not crash, must be a no-op.
_fix7b="$_tmpdir/fix_tasks_dict.json"
printf '{"master":{"tasks":{"0":{"id":1}}}}\n' > "$_fix7b"
_fix7b_before="$_tmpdir/fix_tasks_dict_before.json"
cp "$_fix7b" "$_fix7b_before"

_fix7b_stderr="$_tmpdir/fix7b_stderr.txt"
assert "malformed (b): tasks is a dict — exits 0" \
    bash -c "python3 '$NORMALIZE' '$_fix7b' 2>'$_fix7b_stderr'"
assert "malformed (b): tasks is a dict — no Traceback on stderr" \
    bash -c "! grep -q 'Traceback' '$_fix7b_stderr'"
assert "malformed (b): tasks is a dict — file is byte-identical to input" \
    cmp -s "$_fix7b_before" "$_fix7b"

# (c) subtasks field is a dict (not a list) — must not crash, must be a no-op.
_fix7c="$_tmpdir/fix_subtasks_dict.json"
printf '{"master":{"tasks":[{"id":"1","subtasks":{"0":{"id":99}}}]}}\n' > "$_fix7c"
_fix7c_before="$_tmpdir/fix_subtasks_dict_before.json"
cp "$_fix7c" "$_fix7c_before"

_fix7c_stderr="$_tmpdir/fix7c_stderr.txt"
assert "malformed (c): subtasks is a dict — exits 0" \
    bash -c "python3 '$NORMALIZE' '$_fix7c' 2>'$_fix7c_stderr'"
assert "malformed (c): subtasks is a dict — no Traceback on stderr" \
    bash -c "! grep -q 'Traceback' '$_fix7c_stderr'"
assert "malformed (c): subtasks is a dict — file is byte-identical to input" \
    cmp -s "$_fix7c_before" "$_fix7c"

# (d) A task entry is a string (not a dict) — must not crash, must be a no-op.
_fix7d="$_tmpdir/fix_task_string.json"
printf '{"master":{"tasks":["not-a-dict",{"id":"2","subtasks":[]}]}}\n' > "$_fix7d"
_fix7d_before="$_tmpdir/fix_task_string_before.json"
cp "$_fix7d" "$_fix7d_before"

_fix7d_stderr="$_tmpdir/fix7d_stderr.txt"
assert "malformed (d): task entry is string — exits 0" \
    bash -c "python3 '$NORMALIZE' '$_fix7d' 2>'$_fix7d_stderr'"
assert "malformed (d): task entry is string — no Traceback on stderr" \
    bash -c "! grep -q 'Traceback' '$_fix7d_stderr'"
assert "malformed (d): task entry is string — file is byte-identical to input" \
    cmp -s "$_fix7d_before" "$_fix7d"

# (e) Mixed fixture: a well-formed namespace alongside a malformed one.
# The malformed namespace is skipped; the well-formed namespace is still normalized.
_fix7e="$_tmpdir/fix_mixed.json"
cat > "$_fix7e" <<'FIXTURE'
{"master":{"tasks":[{"id":5,"subtasks":[{"id":6}]}]},"bad_ns":{"tasks":{"0":"not-a-dict"}}}
FIXTURE

assert "malformed (e): mixed — exits 0" \
    bash -c "python3 '$NORMALIZE' '$_fix7e' 2>/dev/null"
assert "malformed (e): mixed — well-formed task id is normalized to string" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].id | type' '$_fix7e')\" = 'string' ]"
assert "malformed (e): mixed — well-formed subtask id is normalized to string" \
    bash -c "[ \"\$(jq -r '.master.tasks[0].subtasks[0].id | type' '$_fix7e')\" = 'string' ]"
assert "malformed (e): mixed — malformed tasks dict is preserved byte-for-byte in JSON output" \
    bash -c "[ \"\$(jq -r '.bad_ns.tasks | type' '$_fix7e')\" = 'object' ]"

# -- Summary ------------------------------------------------------------------
test_summary
