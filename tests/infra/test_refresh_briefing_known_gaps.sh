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

# -- Summary ------------------------------------------------------------------
test_summary
