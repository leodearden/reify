#!/usr/bin/env bash
# tests/infra/test_lane_x_flock.sh — mechanism tests for scripts/lib_lane_x_flock.sh
#
# Tests the Lane-X host-exclusive coarse single-slot flock primitive (H8,
# docs/prds/run-all-host-infra-partition.md). Mirrors the structure of
# tests/infra/test_test_run_semaphore.sh, which mirrors
# tests/infra/test_occt_flock_gate.sh.
#
# H8 ships this lib INERT: it is NOT wired into any tests/infra/run_all.sh
# path in this decompose (see the inert-shipping guard below). H9 (sibling
# task) is the intra-batch consumer that acquires it directly.
#
# Auto-discovered by tests/infra/run_all.sh (pattern test_*.sh).
# Each invocation uses an isolated mktemp LOCK base and cleans
# ${LOCK}.slot-1 after each test (hermetic).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB="$REPO_ROOT/scripts/lib_lane_x_flock.sh"

source "$SCRIPT_DIR/test_helpers.sh"

# ===========================================================================
# FOUNDATION tests (Tests 1-4): lib structure and sourceable interface
# ===========================================================================

echo "=== test_lane_x_flock.sh: lib_lane_x_flock.sh mechanism tests ==="
echo ""
echo "--- Test 1: lib file exists ---"
assert "lib file exists at scripts/lib_lane_x_flock.sh" \
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
echo "--- Test 4: lib is sourceable without side effects and defines acquire+release ---"
assert "sourceable: defines lane_x_flock_acquire, lane_x_flock_release" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F lane_x_flock_acquire && declare -F lane_x_flock_release' -- "$LIB"

# ===========================================================================
# INERT-SHIPPING GUARD (Test 5): the Part A/B split (PRD §6/§7/§11) depends on
# H8 being inert in Part A — not invoked from any run_all.sh path. A future
# accidental wire would silently pull a host-exclusive lock into the
# concurrent test pool; guard it structurally rather than in prose.
# ===========================================================================

echo ""
echo "--- Test 5: run_all.sh does not reference lib_lane_x_flock (inert-shipping guard) ---"
assert "run_all.sh does NOT reference lib_lane_x_flock (H8 ships inert in Part A)" \
    bash -c '! grep -q lib_lane_x_flock "$1"' -- "$REPO_ROOT/tests/infra/run_all.sh"

# ===========================================================================
# WRAPPER tests (Tests 6-11): lane_x_flock_run + direct-exec main-guard
# ===========================================================================

echo ""
echo "--- Test 6: lib is sourceable and defines lane_x_flock_run ---"
assert "sourceable: defines lane_x_flock_run" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F lane_x_flock_run' -- "$LIB"

echo ""
echo "--- Test 7: lib contains 'set -euo pipefail' (inside the main-guard) ---"
assert "lib contains 'set -euo pipefail'" \
    grep -q 'set -euo pipefail' "$LIB"

echo ""
echo "--- Test 8: direct-exec wrapper runs the command and exits 0 ---"

_LOCK8="$(mktemp)"
_EXIT8=0
_OUT8="$(REIFY_LANE_X_FLOCK_LOCK="$_LOCK8" "$LIB" bash -c 'echo ran')" || _EXIT8=$?
rm -f "$_LOCK8" "${_LOCK8}.slot-1"

assert "Test 8: direct-exec wrapper exits 0 (got $_EXIT8)" \
    test "$_EXIT8" -eq 0
assert "Test 8: direct-exec wrapper runs the command (stdout contains 'ran')" \
    bash -c 'echo "$1" | grep -q "ran"' -- "$_OUT8"

echo ""
echo "--- Test 9: exit-code propagated through the wrapper ---"

_LOCK9="$(mktemp)"
_EXIT9=0
REIFY_LANE_X_FLOCK_LOCK="$_LOCK9" "$LIB" bash -c 'exit 42' || _EXIT9=$?
rm -f "$_LOCK9" "${_LOCK9}.slot-1"

assert "Test 9: exit code 42 propagated through wrapper (got $_EXIT9)" \
    test "$_EXIT9" -eq 42

echo ""
echo "--- Test 10: lock released after wrapper exits (slot-1 re-lockable) ---"

_LOCK10="$(mktemp)"
_EXIT10=0
REIFY_LANE_X_FLOCK_LOCK="$_LOCK10" "$LIB" true || _EXIT10=$?

_LOCK_FREE10=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK10}.slot-1" || _LOCK_FREE10=0
rm -f "$_LOCK10" "${_LOCK10}.slot-1"

assert "Test 10: wrapper exits 0 (got $_EXIT10)" \
    test "$_EXIT10" -eq 0
assert "Test 10: slot-1 lock released after wrapper exit (re-lockable)" \
    test "$_LOCK_FREE10" -eq 1

echo ""
echo "--- Test 11: wrapper closes fd 9 on child; surviving daemons do not leak the slot lock ---"

_LOCK11="$(mktemp)"
_DAEMON_PID_FILE11="$(mktemp)"
_EXIT11=0

# Run the wrapper on a command that spawns a detached daemon surviving the wrapper's exit.
# setsid + & + disown reproduces the sccache-style inheritance pattern.
REIFY_LANE_X_FLOCK_LOCK="$_LOCK11" "$LIB" bash -c '
    setsid bash -c "sleep 30" </dev/null >/dev/null 2>&1 &
    echo $! > "'"$_DAEMON_PID_FILE11"'"
    disown
    exit 0
' || _EXIT11=$?

_DAEMON_PID11="$(cat "$_DAEMON_PID_FILE11" 2>/dev/null || echo "")"

# The daemon must still be alive (otherwise test is vacuous).
assert "Test 11: detached daemon is still alive after wrapper exits (pid=$_DAEMON_PID11)" \
    bash -c "[ -n '$_DAEMON_PID11' ] && kill -0 '$_DAEMON_PID11' 2>/dev/null"

# After the wrapper returns, slot-1 must be flock-acquirable (fd 9 not inherited).
_LOCK_FREE11=1
( flock -n -x 9 || exit 1 ) 9>>"${_LOCK11}.slot-1" || _LOCK_FREE11=0

assert "Test 11: slot-1 lock released after wrapper exit despite surviving daemon (fd 9 not inherited)" \
    test "$_LOCK_FREE11" -eq 1

assert "Test 11: wrapper exited 0 on successful spawn (got $_EXIT11)" \
    test "$_EXIT11" -eq 0

# Cleanup daemon.
if [ -n "$_DAEMON_PID11" ]; then
    kill "$_DAEMON_PID11" 2>/dev/null || true
fi
rm -f "$_LOCK11" "${_LOCK11}.slot-1" "$_DAEMON_PID_FILE11"

# ===========================================================================
# FAIL-FAST tests (Tests 12-13): single-flight exclusion against a held slot
# ===========================================================================

echo ""
echo "--- Test 12: WAIT=0 with slot held exits 75 fast, does not run the command, and diagnoses on stderr ---"

_LOCK12="$(mktemp)"
_ERR12="$(mktemp)"
_OUT12F="$(mktemp)"

# Background holder: acquire slot-1 and hold it for 45s (exceeds outer timeouts).
( flock -x 9; sleep 45 ) 9>>"${_LOCK12}.slot-1" &
_HOLDER12=$!
sleep 0.2   # give holder time to acquire

_EXIT12=0
REIFY_LANE_X_FLOCK_LOCK="$_LOCK12" REIFY_LANE_X_FLOCK_WAIT=0 \
    timeout 30 "$LIB" bash -c 'echo SHOULD_NOT_RUN' >"$_OUT12F" 2>"$_ERR12" || _EXIT12=$?

assert "Test 12a: WAIT=0 with slot held exits 75 well within the timeout (got $_EXIT12)" \
    test "$_EXIT12" -eq 75
assert "Test 12b: WAIT=0 did NOT run the command (stdout lacks SHOULD_NOT_RUN)" \
    bash -c '! grep -q SHOULD_NOT_RUN "$1"' -- "$_OUT12F"
assert "Test 12c: stderr contains an 'acquire' diagnostic (case-insensitive)" \
    bash -c 'grep -qi acquire "$1"' -- "$_ERR12"
assert "Test 12d: stderr contains a 'Lane-X' diagnostic (case-insensitive)" \
    bash -c 'grep -qi lane-x "$1"' -- "$_ERR12"

rm -f "$_ERR12" "$_OUT12F"

echo ""
echo "--- Test 13: default (WAIT unset) is also fail-fast (does not block) ---"

_EXIT13=0
REIFY_LANE_X_FLOCK_LOCK="$_LOCK12" \
    timeout 5 "$LIB" true || _EXIT13=$?

assert "Test 13: default WAIT is fail-fast, exits 75 under timeout 5 (got $_EXIT13)" \
    test "$_EXIT13" -eq 75

kill "$_HOLDER12" 2>/dev/null || true
wait "$_HOLDER12" 2>/dev/null || true
rm -f "$_LOCK12" "${_LOCK12}.slot-1"

# ===========================================================================
# BLOCK/SERIALIZE + WAIT-VALIDATION tests (Tests 14-16)
# ===========================================================================

echo ""
echo "--- Test 14: WAIT=unlimited serializes two concurrent invocations, both exit 0 ---"

_LOCK14="$(mktemp)"
_START14_NS="$(date +%s%N)"

REIFY_LANE_X_FLOCK_LOCK="$_LOCK14" REIFY_LANE_X_FLOCK_WAIT=unlimited \
    "$LIB" bash -c 'sleep 0.4' &
_PID14A=$!
REIFY_LANE_X_FLOCK_LOCK="$_LOCK14" REIFY_LANE_X_FLOCK_WAIT=unlimited \
    "$LIB" bash -c 'sleep 0.4' &
_PID14B=$!

_EXIT14A=0
_EXIT14B=0
wait "$_PID14A" || _EXIT14A=$?
wait "$_PID14B" || _EXIT14B=$?

_END14_NS="$(date +%s%N)"
_ELAPSED14_MS=$(( (_END14_NS - _START14_NS) / 1000000 ))

rm -f "$_LOCK14" "${_LOCK14}.slot-1"

assert "Test 14a: two WAIT=unlimited 0.4s invocations run serially (elapsed >= 700ms, got ${_ELAPSED14_MS}ms)" \
    test "$_ELAPSED14_MS" -ge 700
assert "Test 14b: first invocation exits 0 (got $_EXIT14A)" \
    test "$_EXIT14A" -eq 0
assert "Test 14c: second (queued) invocation exits 0 (got $_EXIT14B)" \
    test "$_EXIT14B" -eq 0

echo ""
echo "--- Test 15: finite WAIT=1 with slot held exits 75 after the finite budget ---"

_LOCK15="$(mktemp)"

( flock -x 9; sleep 45 ) 9>>"${_LOCK15}.slot-1" &
_HOLDER15=$!
sleep 0.2

_EXIT15=0
REIFY_LANE_X_FLOCK_LOCK="$_LOCK15" REIFY_LANE_X_FLOCK_WAIT=1 \
    timeout 30 "$LIB" true || _EXIT15=$?

kill "$_HOLDER15" 2>/dev/null || true
wait "$_HOLDER15" 2>/dev/null || true
rm -f "$_LOCK15" "${_LOCK15}.slot-1"

assert "Test 15: finite WAIT=1 with held slot exits 75 (EX_TEMPFAIL; got $_EXIT15)" \
    test "$_EXIT15" -eq 75

echo ""
echo "--- Test 16: REIFY_LANE_X_FLOCK_WAIT=abc exits 64 with a validation diagnostic (non-integer) ---"

_LOCK16="$(mktemp)"
_ERR16="$(mktemp)"
_EXIT16=0
REIFY_LANE_X_FLOCK_LOCK="$_LOCK16" REIFY_LANE_X_FLOCK_WAIT=abc \
    "$LIB" true 2>"$_ERR16" || _EXIT16=$?
rm -f "$_LOCK16" "${_LOCK16}.slot-1"

assert "Test 16a: WAIT=abc exits 64 (got $_EXIT16)" \
    test "$_EXIT16" -eq 64
assert "Test 16b: WAIT=abc emits a validation diagnostic on stderr" \
    bash -c 'grep -qi wait "$1"' -- "$_ERR16"

rm -f "$_ERR16"

# ===========================================================================
# DISABLE break-glass tests (Tests 17-18)
# ===========================================================================

echo ""
echo "--- Test 17: REIFY_LANE_X_FLOCK_DISABLE=1 bypasses lock acquisition (marker on stderr) ---"

_LOCK17="$(mktemp)"
_ERR17="$(mktemp)"
_EXIT17=0
_OUT17="$(REIFY_LANE_X_FLOCK_DISABLE=1 REIFY_LANE_X_FLOCK_LOCK="$_LOCK17" \
    "$LIB" bash -c 'echo ran' 2>"$_ERR17")" || _EXIT17=$?
rm -f "$_LOCK17" "${_LOCK17}.slot-1"

assert "Test 17a: DISABLE=1 exits 0 (got $_EXIT17)" \
    test "$_EXIT17" -eq 0
assert "Test 17b: DISABLE=1 still runs the command (stdout contains 'ran')" \
    bash -c 'echo "$1" | grep -q "ran"' -- "$_OUT17"
assert "Test 17c: DISABLE=1 emits the disabled(REIFY_LANE_X_FLOCK_DISABLE=1) marker on stderr" \
    bash -c 'grep -q "disabled (REIFY_LANE_X_FLOCK_DISABLE=1)" "$1"' -- "$_ERR17"

rm -f "$_ERR17"

echo ""
echo "--- Test 18: DISABLE=1 does not acquire a slot — two concurrent invocations do not serialize ---"

_LOCK18="$(mktemp)"
_START18_NS="$(date +%s%N)"

REIFY_LANE_X_FLOCK_DISABLE=1 REIFY_LANE_X_FLOCK_LOCK="$_LOCK18" \
    "$LIB" bash -c 'sleep 0.4' &
_PID18A=$!
REIFY_LANE_X_FLOCK_DISABLE=1 REIFY_LANE_X_FLOCK_LOCK="$_LOCK18" \
    "$LIB" bash -c 'sleep 0.4' &
_PID18B=$!

_EXIT18A=0
_EXIT18B=0
wait "$_PID18A" || _EXIT18A=$?
wait "$_PID18B" || _EXIT18B=$?

_END18_NS="$(date +%s%N)"
_ELAPSED18_MS=$(( (_END18_NS - _START18_NS) / 1000000 ))

rm -f "$_LOCK18" "${_LOCK18}.slot-1"

assert "Test 18a: two DISABLE=1 0.4s invocations run concurrently, not serially (elapsed < 700ms, got ${_ELAPSED18_MS}ms)" \
    test "$_ELAPSED18_MS" -lt 700
assert "Test 18b: first invocation exits 0 (got $_EXIT18A)" \
    test "$_EXIT18A" -eq 0
assert "Test 18c: second invocation exits 0 (got $_EXIT18B)" \
    test "$_EXIT18B" -eq 0

# ===========================================================================
# Summary
# ===========================================================================

test_summary
