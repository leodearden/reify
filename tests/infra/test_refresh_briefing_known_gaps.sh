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

# ==============================================================================
# Check 9: --json mode — two mismatches across two subprojects
# ==============================================================================
echo ""
echo "--- Check 9: --json mode emits structured output ---"

_brief9="$_tmpdir/briefing9.yaml"
_tasks9="$_tmpdir/tasks9.json"

cat > "$_brief9" <<'YAML'
subprojects:
  subproject_a:
    known_gaps:
      - what: "gap in A"
        tracking: "1"
  subproject_b:
    known_gaps:
      - what: "gap in B"
        tracking: "2"
YAML

cat > "$_tasks9" <<'JSON'
{"master":{"tasks":[
  {"id":"1","title":"Task one","status":"done"},
  {"id":"2","title":"Task two","status":"done"}
]}}
JSON

_stdout9="$_tmpdir/stdout9.txt"
_stderr9="$_tmpdir/stderr9.txt"
_exit9=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief9" --tasks "$_tasks9" --json \
    >"$_stdout9" 2>"$_stderr9" || _exit9=$?

assert "Check 9: exit code is 1 (mismatches found)" \
    test "$_exit9" -eq 1

assert "Check 9: stderr contains no WARN lines (--json suppresses stderr)" \
    bash -c "! grep -q 'WARN' '$_stderr9'"

assert "Check 9: stdout is valid JSON" \
    bash -c "jq . '$_stdout9' >/dev/null"

assert "Check 9: JSON list has length 2" \
    bash -c "[ \"\$(jq 'length' '$_stdout9')\" = '2' ]"

assert "Check 9: JSON contains task_id '1'" \
    bash -c "jq -e '[.[].task_id] | contains([\"1\"])' '$_stdout9' >/dev/null"

assert "Check 9: JSON contains task_id '2'" \
    bash -c "jq -e '[.[].task_id] | contains([\"2\"])' '$_stdout9' >/dev/null"

assert "Check 9: JSON contains subproject 'subproject_a'" \
    bash -c "jq -e '[.[].subproject] | contains([\"subproject_a\"])' '$_stdout9' >/dev/null"

assert "Check 9: JSON contains subproject 'subproject_b'" \
    bash -c "jq -e '[.[].subproject] | contains([\"subproject_b\"])' '$_stdout9' >/dev/null"

# ==============================================================================
# Check 10: missing --briefing path → non-zero exit, ERROR on stderr, no Traceback
# ==============================================================================
echo ""
echo "--- Check 10: missing briefing.yaml path ---"

_tasks_valid="$_tmpdir/tasks_valid.json"
printf '{"master":{"tasks":[]}}\n' > "$_tasks_valid"

_stderr10="$_tmpdir/stderr10.txt"
_exit10=0
python3 "$REFRESH_SCRIPT" --briefing /no/such/briefing.yaml --tasks "$_tasks_valid" \
    2>"$_stderr10" || _exit10=$?

assert "Check 10: exit code is non-zero (missing briefing)" \
    test "$_exit10" -ne 0

assert "Check 10: stderr contains ERROR" \
    grep -qi "ERROR" "$_stderr10"

assert "Check 10: stderr does NOT contain 'Traceback'" \
    bash -c "! grep -q 'Traceback' '$_stderr10'"

# ==============================================================================
# Check 11: malformed YAML briefing → non-zero exit, ERROR on stderr, no Traceback
# ==============================================================================
echo ""
echo "--- Check 11: malformed YAML in briefing ---"

_brief11="$_tmpdir/briefing11_bad.yaml"
printf '{:\n' > "$_brief11"  # intentionally malformed YAML

_stderr11="$_tmpdir/stderr11.txt"
_exit11=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief11" --tasks "$_tasks_valid" \
    2>"$_stderr11" || _exit11=$?

assert "Check 11: exit code is non-zero (malformed YAML)" \
    test "$_exit11" -ne 0

assert "Check 11: stderr contains ERROR" \
    grep -qi "ERROR" "$_stderr11"

assert "Check 11: stderr does NOT contain 'Traceback'" \
    bash -c "! grep -q 'Traceback' '$_stderr11'"

# ==============================================================================
# Check 12: missing tasks.json path → non-zero exit, ERROR on stderr, no Traceback
# ==============================================================================
echo ""
echo "--- Check 12: missing tasks.json path ---"

_brief_valid="$_tmpdir/briefing_valid.yaml"
printf 'subprojects: {}\n' > "$_brief_valid"

_stderr12="$_tmpdir/stderr12.txt"
_exit12=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief_valid" --tasks /no/such/tasks.json \
    2>"$_stderr12" || _exit12=$?

assert "Check 12: exit code is non-zero (missing tasks.json)" \
    test "$_exit12" -ne 0

assert "Check 12: stderr contains ERROR" \
    grep -qi "ERROR" "$_stderr12"

assert "Check 12: stderr does NOT contain 'Traceback'" \
    bash -c "! grep -q 'Traceback' '$_stderr12'"

# ==============================================================================
# Check 13: --quiet suppresses OK message; default mode prints it on no mismatch
# ==============================================================================
echo ""
echo "--- Check 13: --quiet suppresses OK message; default mode shows it ---"

_brief13="$_tmpdir/briefing13.yaml"
_tasks13="$_tmpdir/tasks13.json"

# Gap with a tracked task that is NOT done — no mismatch, but there IS a gap
# entry to process (so we reach the OK-message code, not the early return).
cat > "$_brief13" <<'YAML'
subprojects:
  proj:
    known_gaps:
      - what: "open gap"
        tracking: "77"
YAML

cat > "$_tasks13" <<'JSON'
{"master":{"tasks":[{"id":"77","title":"Task 77","status":"in-progress"}]}}
JSON

# 13a: default mode (no --quiet) → stderr should contain OK message
_stderr13a="$_tmpdir/stderr13a.txt"
_exit13a=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief13" --tasks "$_tasks13" 2>"$_stderr13a" || _exit13a=$?

assert "Check 13a: exit code is 0 (no done task)" \
    test "$_exit13a" -eq 0

assert "Check 13a: default mode prints OK message when no mismatches" \
    grep -q "OK" "$_stderr13a"

# 13b: --quiet → stderr must be empty (OK message suppressed)
_stderr13b="$_tmpdir/stderr13b.txt"
_exit13b=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief13" --tasks "$_tasks13" --quiet 2>"$_stderr13b" || _exit13b=$?

assert "Check 13b: exit code is 0 (no done task)" \
    test "$_exit13b" -eq 0

assert "Check 13b: --quiet suppresses OK message (stderr empty)" \
    bash -c "[ ! -s '$_stderr13b' ]"

# ==============================================================================
# Check 14: done subtask with dotted tracking ID is flagged
# ==============================================================================
echo ""
echo "--- Check 14: done subtask (dotted tracking ID) is flagged ---"

_brief14="$_tmpdir/briefing14.yaml"
_tasks14="$_tmpdir/tasks14.json"

cat > "$_brief14" <<'YAML'
subprojects:
  engine:
    known_gaps:
      - what: "LSP subtask that was fixed"
        tracking: "42.1"
YAML

cat > "$_tasks14" <<'JSON'
{"master":{"tasks":[{
  "id": "42",
  "title": "Parent task",
  "status": "in-progress",
  "subtasks": [
    {"id": "1", "title": "Subtask one", "status": "done"}
  ]
}]}}
JSON

_stderr14="$_tmpdir/stderr14.txt"
_exit14=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief14" --tasks "$_tasks14" 2>"$_stderr14" || _exit14=$?

assert "Check 14: exit code is 1 (done subtask mismatch)" \
    test "$_exit14" -eq 1

assert "Check 14: stderr contains WARN" \
    grep -q "WARN" "$_stderr14"

assert "Check 14: stderr contains dotted subtask id '42.1'" \
    grep -q "42.1" "$_stderr14"

# ==============================================================================
# Check 15: non-master tag containing a done task referenced from briefing.yaml
# — the script must detect the done task and report it (exit 1 + WARN on stderr)
# ==============================================================================
echo ""
echo "--- Check 15: done task in non-master tag is detected ---"

_brief15="$_tmpdir/briefing15.yaml"
_tasks15="$_tmpdir/tasks15.json"

cat > "$_brief15" <<'YAML'
subprojects:
  engine:
    known_gaps:
      - what: "feature gap"
        tracking: "200"
YAML

cat > "$_tasks15" <<'JSON'
{"master":{"tasks":[]},"feature-branch":{"tasks":[{"id":"200","title":"Fix feature gap","status":"done"}]}}
JSON

_stderr15="$_tmpdir/stderr15.txt"
_exit15=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief15" --tasks "$_tasks15" 2>"$_stderr15" || _exit15=$?

assert "Check 15: exit code is 1 (mismatch found — done task in non-master tag)" \
    test "$_exit15" -eq 1

assert "Check 15: stderr contains WARN" \
    grep -q "WARN" "$_stderr15"

assert "Check 15: stderr contains task id 200" \
    grep -q "200" "$_stderr15"

assert "Check 15: stderr contains gap text 'feature gap'" \
    grep -q "feature gap" "$_stderr15"

# ==============================================================================
# Check 16: cross-tag collision — done-wins semantics
# task 300 is done in feature-branch; any done occurrence must trigger WARN
# master entry (in-progress) must not suppress detection of the done occurrence
# ==============================================================================
echo ""
echo "--- Check 16: cross-tag collision — done wins ---"

_brief16="$_tmpdir/briefing16.yaml"
_tasks16="$_tmpdir/tasks16.json"

cat > "$_brief16" <<'YAML'
subprojects:
  engine:
    known_gaps:
      - what: "collision gap"
        tracking: "300"
YAML

# feature-branch has status done; master has the same task as in-progress.
# done-wins semantics: the done occurrence must be detected regardless of order.
cat > "$_tasks16" <<'JSON'
{"feature-branch":{"tasks":[{"id":"300","title":"Branch version","status":"done"}]},"master":{"tasks":[{"id":"300","title":"Master version","status":"in-progress"}]}}
JSON

_stderr16="$_tmpdir/stderr16.txt"
_exit16=0
python3 "$REFRESH_SCRIPT" --briefing "$_brief16" --tasks "$_tasks16" 2>"$_stderr16" || _exit16=$?

assert "Check 16: exit code is 1 (done in any tag → mismatch)" \
    test "$_exit16" -eq 1

assert "Check 16: stderr contains WARN" \
    grep -q "WARN" "$_stderr16"

assert "Check 16: stderr contains task id 300" \
    grep -q "300" "$_stderr16"

assert "Check 16: stderr contains feature-branch task title 'Branch version'" \
    grep -q "Branch version" "$_stderr16"

# -- Summary ------------------------------------------------------------------
test_summary
