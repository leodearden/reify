#!/usr/bin/env bash
# Tests for scripts/refresh_briefing_known_gaps.py.
# Verifies: existence, executability, YAML/JSON loading, cross-reference logic,
# --json mode, error handling, and edge cases (orphan IDs, missing tracking field).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REFRESH_SCRIPT="$REPO_ROOT/scripts/refresh_briefing_known_gaps.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== refresh_briefing_known_gaps.py tests ==="

# -- Setup: temp-dir fixture machinery ----------------------------------------
_tmpdir=$(mktemp -d)
trap 'rm -rf "$_tmpdir"' EXIT

# ==============================================================================
# Check 0: script exists and is executable, --help exits 0
# ==============================================================================
echo ""
echo "--- Check 0: script existence, executability, and --help ---"

assert "scripts/refresh_briefing_known_gaps.py exists" \
    test -f "$REFRESH_SCRIPT"

assert "scripts/refresh_briefing_known_gaps.py is executable" \
    test -x "$REFRESH_SCRIPT"

assert "scripts/refresh_briefing_known_gaps.py --help exits 0" \
    bash -c "python3 '$REFRESH_SCRIPT' --help >/dev/null 2>&1"

# ==============================================================================
# Check 1: briefing with known_gaps: [] and a done task — exit 0, empty stderr
# ==============================================================================
echo ""
echo "--- Check 1: empty known_gaps list — no mismatches ---"

_brief1="$_tmpdir/briefing1.yaml"
_tasks1="$_tmpdir/tasks1.json"

cat > "$_brief1" <<'YAML'
subprojects:
  myproject:
    known_gaps: []
YAML

cat > "$_tasks1" <<'JSON'
{"master":{"tasks":[{"id":"10","title":"Done task","status":"done"}]}}
JSON

_stderr1="$_tmpdir/stderr1.txt"
_exit1=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief1" --tasks "$_tasks1" --quiet 2>"$_stderr1" || _exit1=$?

assert "Check 1: exit code is 0 (no mismatches)" \
    test "$_exit1" -eq 0

assert "Check 1: stderr is empty" \
    bash -c "[ ! -s '$_stderr1' ]"

# ==============================================================================
# Check 2: briefing with no known_gaps key at all — exit 0, empty stderr
# ==============================================================================
echo ""
echo "--- Check 2: subproject with no known_gaps key — no mismatches ---"

_brief2="$_tmpdir/briefing2.yaml"
_tasks2="$_tmpdir/tasks2.json"

cat > "$_brief2" <<'YAML'
subprojects:
  aproject:
    purpose: "something"
YAML

cat > "$_tasks2" <<'JSON'
{"master":{"tasks":[{"id":"20","title":"Another done task","status":"done"}]}}
JSON

_stderr2="$_tmpdir/stderr2.txt"
_exit2=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief2" --tasks "$_tasks2" --quiet 2>"$_stderr2" || _exit2=$?

assert "Check 2: exit code is 0 (no mismatches)" \
    test "$_exit2" -eq 0

assert "Check 2: stderr is empty" \
    bash -c "[ ! -s '$_stderr2' ]"

# ==============================================================================
# Check 2b: --briefing/--tasks paths are honored — non-existent path raises non-zero
# (default mode, no --quiet so ERROR line goes to stderr)
# ==============================================================================
echo ""
echo "--- Check 2b: non-existent --briefing path raises non-zero ---"

_exit2b=0
python3 "$REFRESH_SCRIPT" --briefing /no/such/briefing.yaml --tasks "$_tasks2" \
    2>/dev/null || _exit2b=$?

assert "Check 2b: non-existent --briefing path raises non-zero exit" \
    test "$_exit2b" -ne 0

# ==============================================================================
# Check 3: done-task tracking ID → exit 1, stderr contains WARN + task id + gap text
# ==============================================================================
echo ""
echo "--- Check 3: known_gap with tracking ID for done task raises mismatch ---"

_brief3="$_tmpdir/briefing3.yaml"
_tasks3="$_tmpdir/tasks3.json"

cat > "$_brief3" <<'YAML'
subprojects:
  myproject:
    known_gaps:
      - what: "thing X"
        why: "some reason"
        tracking: "42"
YAML

cat > "$_tasks3" <<'JSON'
{"master":{"tasks":[{"id":"42","title":"Fix thing X","status":"done"}]}}
JSON

_stderr3="$_tmpdir/stderr3.txt"
_exit3=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief3" --tasks "$_tasks3" 2>"$_stderr3" || _exit3=$?

assert "Check 3: exit code is 1 (mismatch found)" \
    test "$_exit3" -eq 1

assert "Check 3: stderr contains WARN" \
    grep -q "WARN" "$_stderr3"

assert "Check 3: stderr contains task id 42" \
    grep -q "42" "$_stderr3"

assert "Check 3: stderr contains gap text 'thing X'" \
    grep -q "thing X" "$_stderr3"

# ==============================================================================
# Checks 4–6: non-done statuses (in-progress, pending, blocked) → exit 0, no WARN
# ==============================================================================
echo ""
echo "--- Checks 4-6: non-done statuses are not flagged as mismatches ---"

for _status in "in-progress" "pending" "blocked"; do
    _brief_s="$_tmpdir/briefing_${_status}.yaml"
    _tasks_s="$_tmpdir/tasks_${_status}.json"

    cat > "$_brief_s" <<YAML
subprojects:
  proj:
    known_gaps:
      - what: "some gap"
        tracking: "100"
YAML

    printf '{"master":{"tasks":[{"id":"100","title":"Task 100","status":"%s"}]}}\n' "$_status" > "$_tasks_s"

    _stderr_s="$_tmpdir/stderr_${_status}.txt"
    _exit_s=0
    python3 "$REFRESH_SCRIPT" --briefing "$_brief_s" --tasks "$_tasks_s" 2>"$_stderr_s" || _exit_s=$?

    assert "status=$_status: exit code is 0 (not done — no mismatch)" \
        test "$_exit_s" -eq 0

    assert "status=$_status: stderr contains no WARN" \
        bash -c "! grep -q 'WARN' '$_stderr_s'"
done

# ==============================================================================
# Check 7: known_gap entry with no tracking: field is silently skipped
# ==============================================================================
echo ""
echo "--- Check 7: legacy gap with no tracking field is silently skipped ---"

_brief7="$_tmpdir/briefing7.yaml"
_tasks7="$_tmpdir/tasks7.json"

cat > "$_brief7" <<'YAML'
subprojects:
  proj:
    known_gaps:
      - what: "legacy gap"
        why: "was there before tracking field was added"
YAML

cat > "$_tasks7" <<'JSON'
{"master":{"tasks":[{"id":"99","title":"Unrelated","status":"done"}]}}
JSON

_stderr7="$_tmpdir/stderr7.txt"
_exit7=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief7" --tasks "$_tasks7" 2>"$_stderr7" || _exit7=$?

assert "Check 7: exit code is 0 (legacy gap silently skipped)" \
    test "$_exit7" -eq 0

assert "Check 7: stderr contains no WARN about 'legacy gap'" \
    bash -c "! grep -q 'legacy gap' '$_stderr7'"

# ==============================================================================
# Check 8: orphan tracking ID (not in tasks.json) is gracefully skipped
# ==============================================================================
echo ""
echo "--- Check 8: orphan tracking ID skipped gracefully ---"

_brief8="$_tmpdir/briefing8.yaml"
_tasks8="$_tmpdir/tasks8.json"

cat > "$_brief8" <<'YAML'
subprojects:
  proj:
    known_gaps:
      - what: "some gap"
        tracking: "9999"
YAML

cat > "$_tasks8" <<'JSON'
{"master":{"tasks":[{"id":"1","title":"Unrelated","status":"done"}]}}
JSON

_stderr8="$_tmpdir/stderr8.txt"
_exit8=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief8" --tasks "$_tasks8" 2>"$_stderr8" || _exit8=$?

assert "Check 8: exit code is 0 (orphan ID gracefully skipped)" \
    test "$_exit8" -eq 0

assert "Check 8: no WARN emitted for orphan tracking ID" \
    bash -c "! grep -q 'WARN' '$_stderr8'"

# -- Summary ------------------------------------------------------------------
test_summary
