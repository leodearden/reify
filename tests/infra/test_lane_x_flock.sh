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
# Summary
# ===========================================================================

test_summary
