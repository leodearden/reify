#!/usr/bin/env bash
# tests/infra/test_test_run_semaphore.sh — mechanism tests for scripts/lib_test_semaphore.sh
#
# Tests the N-slot counting semaphore lib for the TEST-EXECUTION phase.
# Mirrors the structure of tests/infra/test_occt_flock_gate.sh.
#
# Auto-discovered by tests/infra/run_all.sh (pattern test_*.sh).
# Each invocation sets DF_VERIFY_ROLE and REIFY_TEST_SEMAPHORE_CONCURRENCY
# explicitly (hermetic; valid if defaults change), uses an isolated mktemp LOCK
# base, and cleans ${LOCK}.slot-* after each test.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB="$REPO_ROOT/scripts/lib_test_semaphore.sh"

source "$SCRIPT_DIR/test_helpers.sh"

# ===========================================================================
# FOUNDATION tests (Tests 1-5): lib structure and sourceable interface
# ===========================================================================

echo "=== test_test_run_semaphore.sh: lib_test_semaphore.sh mechanism tests ==="
echo ""
echo "--- Test 1: lib file exists ---"
assert "lib file exists at scripts/lib_test_semaphore.sh" \
    test -f "$LIB"

echo ""
echo "--- Test 2: lib is executable ---"
assert "lib is executable" \
    test -x "$LIB"

echo ""
echo "--- Test 3: lib has correct shebang ---"
assert "first line is #!/usr/bin/env bash" \
    bash -c 'head -1 "$1" | grep -q "^#!/usr/bin/env bash"' -- "$LIB"

echo ""
echo "--- Test 4: lib contains set -euo pipefail ---"
assert "lib contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$LIB"

echo ""
echo "--- Test 5: lib is sourceable without side effects and defines all three functions ---"
assert "sourceable: defines test_semaphore_acquire, test_semaphore_release, test_semaphore_run" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F test_semaphore_acquire && declare -F test_semaphore_release && declare -F test_semaphore_run' -- "$LIB"

# ===========================================================================
# SIGNAL (a): held-slot serialization + exit-code propagation (Tests 6-7)
# ===========================================================================

echo ""
echo "--- Test 6: two concurrent role=task N=1 runs serialize (elapsed >= 700ms) ---"

_LOCK6="$(mktemp)"
_START6_NS="$(date +%s%N)"

# Spawn two concurrent wrapper invocations each sleeping 0.4s.
# REIFY_TEST_SEMAPHORE_CONCURRENCY=1 pins N=1 (exclusive).
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK6" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" bash -c 'sleep 0.4' &
_PID6A=$!
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK6" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" bash -c 'sleep 0.4' &
_PID6B=$!
wait "$_PID6A" "$_PID6B"

_END6_NS="$(date +%s%N)"
_ELAPSED6_MS=$(( (_END6_NS - _START6_NS) / 1000000 ))

rm -f "$_LOCK6" "${_LOCK6}.slot-1"

# Parallel would finish in ~400ms; serialized takes >=700ms.
assert "two 0.4s sleep invocations run serially (elapsed >= 700ms, got ${_ELAPSED6_MS}ms)" \
    test "$_ELAPSED6_MS" -ge 700

echo ""
echo "--- Test 7: exit-code propagated through the wrapper ---"

_LOCK7="$(mktemp)"
_EXIT7=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK7" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" bash -c 'exit 42' || _EXIT7=$?
rm -f "$_LOCK7" "${_LOCK7}.slot-1"

assert "exit code 42 propagated through wrapper (got $_EXIT7)" \
    test "$_EXIT7" -eq 42

# ===========================================================================
# SIGNAL (b): merge-role exemption (Tests 8-9)
# ===========================================================================

echo ""
echo "--- Test 8: role=merge skips acquisition, runs fast even when slot is held ---"

_LOCK8="$(mktemp)"

# Start a background role=task holder of the only slot (holds 2s).
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK8" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" bash -c 'sleep 2' &
_HOLDER8=$!
sleep 0.3   # give the holder time to acquire slot-1

_START8_NS="$(date +%s%N)"
_EXIT8=0
DF_VERIFY_ROLE=merge REIFY_TEST_SEMAPHORE_LOCK="$_LOCK8" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" true || _EXIT8=$?
_END8_NS="$(date +%s%N)"
_ELAPSED8_MS=$(( (_END8_NS - _START8_NS) / 1000000 ))

kill "$_HOLDER8" 2>/dev/null || true
wait "$_HOLDER8" 2>/dev/null || true
rm -f "$_LOCK8" "${_LOCK8}.slot-1"

assert "Test 8: role=merge exits 0 (no acquisition, no command failure; got $_EXIT8)" \
    test "$_EXIT8" -eq 0
assert "Test 8: role=merge completes fast (<1000ms, not waiting 2s for task slot; got ${_ELAPSED8_MS}ms)" \
    test "$_ELAPSED8_MS" -lt 1000

echo ""
echo "--- Test 9: role=merge bypass noted in stderr ---"

_LOCK9="$(mktemp)"
_ERR9="$(mktemp)"
DF_VERIFY_ROLE=merge REIFY_TEST_SEMAPHORE_LOCK="$_LOCK9" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" true 2>"$_ERR9" || true
rm -f "$_LOCK9" "${_LOCK9}.slot-1"

assert "Test 9: merge bypass is noted in stderr (grep merge|exempt|bypass)" \
    grep -qiE 'merge|exempt|bypass' "$_ERR9"

rm -f "$_ERR9"

# ===========================================================================
# SIGNAL (c): deadline → exit 75 + DISABLE knob (Tests 10-12)
# ===========================================================================

echo ""
echo "--- Test 10: role=task WAIT=1 with slot held → exit 75 within budget ---"

_LOCK10="$(mktemp)"
_ERR10="$(mktemp)"

# Background holder: acquire slot-1 and hold it for 10s.
( flock -x 9; sleep 10 ) 9>>"${_LOCK10}.slot-1" &
_HOLDER10=$!
sleep 0.2   # give holder time to acquire

_T0_10="$(date +%s)"
_EXIT10=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK10" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 REIFY_TEST_SEMAPHORE_WAIT=1 \
    timeout 5 "$LIB" true 2>"$_ERR10" || _EXIT10=$?
_T1_10="$(date +%s)"
_ELAPSED10=$(( _T1_10 - _T0_10 ))

kill "$_HOLDER10" 2>/dev/null || true
wait "$_HOLDER10" 2>/dev/null || true
rm -f "$_LOCK10" "${_LOCK10}.slot-1" "$_ERR10"

assert "Test 10: exits 75 (EX_TEMPFAIL) when WAIT budget exhausted (got $_EXIT10)" \
    test "$_EXIT10" -eq 75

assert "Test 10: exits within 3s, not blocked until outer timeout 5s (elapsed=${_ELAPSED10}s)" \
    test "$_ELAPSED10" -le 3

echo ""
echo "--- Test 11: exit-75 stderr mentions 'acquire' and wait duration (1s) ---"

_LOCK11="$(mktemp)"
_ERR11="$(mktemp)"

( flock -x 9; sleep 10 ) 9>>"${_LOCK11}.slot-1" &
_HOLDER11=$!
sleep 0.2

_EXIT11=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK11" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 REIFY_TEST_SEMAPHORE_WAIT=1 \
    timeout 5 "$LIB" true 2>"$_ERR11" || _EXIT11=$?

kill "$_HOLDER11" 2>/dev/null || true
wait "$_HOLDER11" 2>/dev/null || true
rm -f "$_LOCK11" "${_LOCK11}.slot-1"

assert "Test 11: stderr mentions 'acquire' and lock-wait duration (1s)" \
    grep -qE 'acquire.*1s|1s.*acquire' "$_ERR11"

rm -f "$_ERR11"

echo ""
echo "--- Test 12: REIFY_TEST_SEMAPHORE_DISABLE=1 bypasses the slot even when slot is held ---"

_LOCK12="$(mktemp)"

# Background holder holds slot-1.
( flock -x 9; sleep 5 ) 9>>"${_LOCK12}.slot-1" &
_HOLDER12=$!
sleep 0.2

_START12_NS="$(date +%s%N)"
_EXIT12=0
_OUT12="$(DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_DISABLE=1 \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK12" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" bash -c 'echo ran' 2>/dev/null)" || _EXIT12=$?
_END12_NS="$(date +%s%N)"
_ELAPSED12_MS=$(( (_END12_NS - _START12_NS) / 1000000 ))

kill "$_HOLDER12" 2>/dev/null || true
wait "$_HOLDER12" 2>/dev/null || true
rm -f "$_LOCK12" "${_LOCK12}.slot-1"

assert "Test 12: DISABLE=1 exits 0 (got $_EXIT12)" \
    test "$_EXIT12" -eq 0
assert "Test 12: DISABLE=1 runs fast (<2000ms, not waiting for slot; got ${_ELAPSED12_MS}ms)" \
    test "$_ELAPSED12_MS" -lt 2000
assert "Test 12: DISABLE=1 still runs the command (stdout contains 'ran')" \
    bash -c "echo '$_OUT12' | grep -q 'ran'"

# ===========================================================================
# SIGNAL (d): FD non-leak to surviving daemons (Test 13)
# ===========================================================================

echo ""
echo "--- Test 13: wrapper closes fd 9 on child; surviving daemons do not leak the slot lock ---"

_LOCK13="$(mktemp)"
_DAEMON_PID_FILE13="$(mktemp)"
_EXIT13=0

# Run the wrapper on a command that spawns a detached daemon surviving the wrapper's exit.
# setsid + & + disown reproduces the sccache-style inheritance pattern.
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK13" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" bash -c '
    setsid bash -c "sleep 30" </dev/null >/dev/null 2>&1 &
    echo $! > "'"$_DAEMON_PID_FILE13"'"
    disown
    exit 0
' || _EXIT13=$?

_DAEMON_PID13="$(cat "$_DAEMON_PID_FILE13" 2>/dev/null || echo "")"

# The daemon must still be alive (otherwise test is vacuous).
assert "Test 13: detached daemon is still alive after wrapper exits (pid=$_DAEMON_PID13)" \
    bash -c "[ -n '$_DAEMON_PID13' ] && kill -0 '$_DAEMON_PID13' 2>/dev/null"

# After the wrapper returns, slot-1 must be flock-acquirable (fd 9 not inherited).
_LOCK_FREE13=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK13}.slot-1" || _LOCK_FREE13=0

assert "Test 13: slot-1 lock released after wrapper exit despite surviving daemon (fd 9 not inherited)" \
    test "$_LOCK_FREE13" -eq 1

assert "Test 13: wrapper exited 0 on successful spawn (got $_EXIT13)" \
    test "$_EXIT13" -eq 0

# Cleanup daemon.
if [ -n "$_DAEMON_PID13" ]; then
    kill "$_DAEMON_PID13" 2>/dev/null || true
fi
rm -f "$_LOCK13" "${_LOCK13}.slot-1" "$_DAEMON_PID_FILE13"

# ===========================================================================
# Summary
# ===========================================================================

test_summary
