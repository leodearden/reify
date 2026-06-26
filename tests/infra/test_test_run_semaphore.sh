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
# SIGNAL (b): merge-role exemption (Test 9)
# ===========================================================================

echo ""
echo "--- Test 9: role=merge bypass noted in stderr ---"

_LOCK9="$(mktemp)"
_ERR9="$(mktemp)"
_EXIT9=0
DF_VERIFY_ROLE=merge REIFY_TEST_SEMAPHORE_LOCK="$_LOCK9" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" true 2>"$_ERR9" || _EXIT9=$?
rm -f "$_LOCK9" "${_LOCK9}.slot-1"

assert "Test 9: merge bypass is noted in stderr (grep merge|exempt|bypass)" \
    grep -qiE 'merge|exempt|bypass' "$_ERR9"
assert "Test 9: role=merge exits 0 (bypass, no command failure; got $_EXIT9)" \
    test "$_EXIT9" -eq 0

rm -f "$_ERR9"

# ===========================================================================
# SIGNAL (c): deadline → exit 75 + DISABLE knob (Tests 10-12)
# ===========================================================================

echo ""
echo "--- Test 10: role=task WAIT=1 with slot held → exit 75 within budget ---"

_LOCK10="$(mktemp)"
_ERR10="$(mktemp)"

# Background holder: acquire slot-1 and hold it for 45s (exceeds outer timeout 30s).
( flock -x 9; sleep 45 ) 9>>"${_LOCK10}.slot-1" &
_HOLDER10=$!
sleep 0.2   # give holder time to acquire

_EXIT10=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK10" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 REIFY_TEST_SEMAPHORE_WAIT=1 \
    timeout 30 "$LIB" true 2>"$_ERR10" || _EXIT10=$?

kill "$_HOLDER10" 2>/dev/null || true
wait "$_HOLDER10" 2>/dev/null || true
rm -f "$_LOCK10" "${_LOCK10}.slot-1" "$_ERR10"

assert "Test 10: exits 75 (EX_TEMPFAIL) when WAIT budget exhausted (got $_EXIT10)" \
    test "$_EXIT10" -eq 75

echo ""
echo "--- Test 11: exit-75 stderr mentions 'acquire' and wait duration (1s) ---"

_LOCK11="$(mktemp)"
_ERR11="$(mktemp)"

( flock -x 9; sleep 45 ) 9>>"${_LOCK11}.slot-1" &
_HOLDER11=$!
sleep 0.2

_EXIT11=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK11" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 REIFY_TEST_SEMAPHORE_WAIT=1 \
    timeout 30 "$LIB" true 2>"$_ERR11" || _EXIT11=$?

kill "$_HOLDER11" 2>/dev/null || true
wait "$_HOLDER11" 2>/dev/null || true
rm -f "$_LOCK11" "${_LOCK11}.slot-1"

assert "Test 11: stderr mentions 'acquire' and lock-wait duration (1s)" \
    grep -qE 'acquire.*1s|1s.*acquire' "$_ERR11"
assert "Test 11: exits 75 (EX_TEMPFAIL; got $_EXIT11)" \
    test "$_EXIT11" -eq 75

rm -f "$_ERR11"

echo ""
echo "--- Test 12: REIFY_TEST_SEMAPHORE_DISABLE=1 bypasses slot acquisition (marker on stderr) ---"

_LOCK12="$(mktemp)"
_ERR12="$(mktemp)"

_EXIT12=0
_OUT12="$(DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_DISABLE=1 \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK12" REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" bash -c 'echo ran' 2>"$_ERR12")" || _EXIT12=$?

rm -f "$_LOCK12" "${_LOCK12}.slot-1"

assert "Test 12: DISABLE=1 exits 0 (got $_EXIT12)" \
    test "$_EXIT12" -eq 0
assert "Test 12: DISABLE=1 still runs the command (stdout contains 'ran')" \
    bash -c "echo '$_OUT12' | grep -q 'ran'"
assert "Test 12: DISABLE=1 emits the disabled(REIFY_TEST_SEMAPHORE_DISABLE=1) marker on stderr" \
    grep -q 'disabled (REIFY_TEST_SEMAPHORE_DISABLE=1)' "$_ERR12"

rm -f "$_ERR12"

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
# INPUT VALIDATION tests (Tests 14-17): early-return contract for bad knobs
# These paths are part of the documented interface; regression-test them so
# a refactor of test_semaphore_acquire cannot silently break them.
# ===========================================================================

echo ""
echo "--- Test 14: REIFY_TEST_SEMAPHORE_CONCURRENCY=abc exits 64 (non-integer) ---"

_LOCK14="$(mktemp)"
_EXIT14=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_CONCURRENCY=abc \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK14" \
    "$LIB" true || _EXIT14=$?
rm -f "$_LOCK14"

assert "Test 14: CONCURRENCY=abc exits 64 (got $_EXIT14)" \
    test "$_EXIT14" -eq 64

echo ""
echo "--- Test 15: REIFY_TEST_SEMAPHORE_CONCURRENCY=0 exits 64 (less than 1) ---"

_LOCK15="$(mktemp)"
_EXIT15=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_CONCURRENCY=0 \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK15" \
    "$LIB" true || _EXIT15=$?
rm -f "$_LOCK15"

assert "Test 15: CONCURRENCY=0 exits 64 (got $_EXIT15)" \
    test "$_EXIT15" -eq 64

echo ""
echo "--- Test 16: REIFY_TEST_SEMAPHORE_WAIT=x exits 64 (non-integer) ---"

_LOCK16="$(mktemp)"
_EXIT16=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_WAIT=x \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK16" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$LIB" true || _EXIT16=$?
rm -f "$_LOCK16"

assert "Test 16: WAIT=x exits 64 (got $_EXIT16)" \
    test "$_EXIT16" -eq 64

echo ""
echo "--- Test 17: non-existent LOCK parent dir exits 1 with a stderr diagnostic ---"

_LOCK17="/tmp/nonexistent_semaphore_test_parent_$$/mylock"
_ERR17="$(mktemp)"
_EXIT17=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK17" \
    "$LIB" true 2>"$_ERR17" || _EXIT17=$?

assert "Test 17: non-existent LOCK parent exits 1 (got $_EXIT17)" \
    test "$_EXIT17" -eq 1

assert "Test 17: non-existent LOCK parent emits a 'lock parent' diagnostic on stderr" \
    grep -q 'lock parent' "$_ERR17"

rm -f "$_ERR17"

# ===========================================================================
# SIGNAL (f): semaphore continuous-wait + clock-stop markers (Tests T18-T20)
# Re-express dead-branch T18/T19/T20 onto the post-4840 lib_slot_acquire.sh
# with explicit STOP/START framing and the shared lib_clock_stop.sh emitter.
# Drive via the `"$LIB" cmd` wrapper (same as existing Tests 6-17).
# RED today: "unlimited" rejected as exit 64 (non-integer); no markers emitted.
# ===========================================================================

echo ""
echo "--- Test T18: WAIT=unlimited with free slot exits 0 (not 64/75) ---"

_LOCK18="$(mktemp)"
_ERR18="$(mktemp)"
_EXIT18=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK18" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 REIFY_TEST_SEMAPHORE_WAIT=unlimited \
    timeout 30 "$LIB" true 2>"$_ERR18" || _EXIT18=$?
rm -f "$_LOCK18" "${_LOCK18}.slot-1" "$_ERR18"

assert "Test T18: WAIT=unlimited with free slot exits 0 (not 64 non-integer; got $_EXIT18)" \
    test "$_EXIT18" -eq 0

echo ""
echo "--- Test T19: WAIT=unlimited queues behind 2s holder, exits 0, emits STOP/HEARTBEAT/START ---"

_LOCK19="$(mktemp)"
_ERR19="$(mktemp)"

# Background holder: hold slot-1 for 2s then release.
( flock -x 9; sleep 2 ) 9>>"${_LOCK19}.slot-1" &
_HOLDER19=$!
sleep 0.2   # give holder time to acquire

_START19_NS="$(date +%s%N)"
_EXIT19=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK19" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 REIFY_TEST_SEMAPHORE_WAIT=unlimited \
    REIFY_CLOCK_HEARTBEAT_SECS=1 \
    timeout 30 "$LIB" true 2>"$_ERR19" || _EXIT19=$?
_END19_NS="$(date +%s%N)"
_ELAPSED19_MS=$(( (_END19_NS - _START19_NS) / 1000000 ))

kill "$_HOLDER19" 2>/dev/null || true
wait "$_HOLDER19" 2>/dev/null || true
rm -f "$_LOCK19" "${_LOCK19}.slot-1"

assert "Test T19a: WAIT=unlimited exits 0 (queued-then-ran, not 75; got $_EXIT19)" \
    test "$_EXIT19" -eq 0
assert "Test T19b: elapsed >= 1500ms (was blocked by 2s holder; got ${_ELAPSED19_MS}ms)" \
    test "$_ELAPSED19_MS" -ge 1500
assert "Test T19c: stderr contains @@REIFY_CLOCK_STOP@@ reason=test_slot_starvation" \
    grep -qE '@@REIFY_CLOCK_STOP@@ reason=test_slot_starvation' "$_ERR19"
assert "Test T19d: stderr contains @@REIFY_CLOCK_START@@" \
    grep -q '@@REIFY_CLOCK_START@@' "$_ERR19"
assert "Test T19e: stderr contains @@REIFY_CLOCK_HEARTBEAT@@ (HEARTBEAT_SECS=1 + 2s hold)" \
    grep -q '@@REIFY_CLOCK_HEARTBEAT@@' "$_ERR19"

rm -f "$_ERR19"

echo ""
echo "--- Test T20: finite WAIT=1 with held slot exits 75 AND emits @@REIFY_CLOCK_STOP@@ ---"

_LOCK20="$(mktemp)"
_ERR20="$(mktemp)"

( flock -x 9; sleep 45 ) 9>>"${_LOCK20}.slot-1" &
_HOLDER20=$!
sleep 0.2

_EXIT20=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK20" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 REIFY_TEST_SEMAPHORE_WAIT=1 \
    timeout 30 "$LIB" true 2>"$_ERR20" || _EXIT20=$?

kill "$_HOLDER20" 2>/dev/null || true
wait "$_HOLDER20" 2>/dev/null || true
rm -f "$_LOCK20" "${_LOCK20}.slot-1"

assert "Test T20a: finite WAIT=1 exits 75 (EX_TEMPFAIL; got $_EXIT20)" \
    test "$_EXIT20" -eq 75
assert "Test T20b: stderr contains @@REIFY_CLOCK_STOP@@ (entered the wait before deadline)" \
    grep -q '@@REIFY_CLOCK_STOP@@' "$_ERR20"

rm -f "$_ERR20"

echo ""
echo "--- Test T21: balance — uncontended acquire (free slot) emits NO @@REIFY_CLOCK_STOP@@ ---"

_LOCK21="$(mktemp)"
_ERR21="$(mktemp)"
_EXIT21=0
DF_VERIFY_ROLE=task REIFY_TEST_SEMAPHORE_LOCK="$_LOCK21" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 REIFY_TEST_SEMAPHORE_WAIT=unlimited \
    timeout 10 "$LIB" true 2>"$_ERR21" || _EXIT21=$?
rm -f "$_LOCK21" "${_LOCK21}.slot-1"

assert "Test T21a: uncontended WAIT=unlimited exits 0 (got $_EXIT21)" \
    test "$_EXIT21" -eq 0
assert "Test T21b: uncontended acquire emits NO @@REIFY_CLOCK_STOP@@ (fast path is silent)" \
    bash -c '! grep -q "@@REIFY_CLOCK_STOP@@" "$1"' _ "$_ERR21"

rm -f "$_ERR21"

# ===========================================================================
# SIGNAL (e): clock-stop emitter wire-grammar contract (Tests CS-1..CS-5)
# Pins the @@REIFY_CLOCK_*@@ marker grammar that dark_factory:1916 will match.
# RED today: scripts/lib_clock_stop.sh does not exist.
# ===========================================================================

echo ""
echo "--- Test CS-1: lib_clock_stop.sh exists, is sourceable, and defines all emitter functions ---"

_CLOCK_LIB="$REPO_ROOT/scripts/lib_clock_stop.sh"
assert "Test CS-1a: lib_clock_stop.sh exists at scripts/lib_clock_stop.sh" \
    test -f "$_CLOCK_LIB"
assert "Test CS-1b: lib_clock_stop.sh is sourceable and defines clock_emit_stop, clock_emit_heartbeat, clock_emit_start" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F clock_emit_stop && declare -F clock_emit_heartbeat && declare -F clock_emit_start' _ "$_CLOCK_LIB"

echo ""
echo "--- Test CS-2: clock_emit_stop emits exact grammar @@REIFY_CLOCK_STOP@@ reason=... pid=... to stderr ---"

_CS2_ERR="$(mktemp)"
bash -c 'source "$1" >/dev/null 2>&1; clock_emit_stop test_slot_starvation' _ "$_CLOCK_LIB" 2>"$_CS2_ERR" || true
assert "Test CS-2a: clock_emit_stop stderr contains @@REIFY_CLOCK_STOP@@" \
    grep -q '@@REIFY_CLOCK_STOP@@' "$_CS2_ERR"
assert "Test CS-2b: clock_emit_stop stderr contains reason=test_slot_starvation" \
    grep -q 'reason=test_slot_starvation' "$_CS2_ERR"
assert "Test CS-2c: clock_emit_stop stderr contains pid=<digits>" \
    grep -qE 'pid=[0-9]+' "$_CS2_ERR"
assert "Test CS-2d: clock_emit_stop full line matches exact grammar: @@REIFY_CLOCK_STOP@@ reason=test_slot_starvation pid=<digits>" \
    grep -qE '^@@REIFY_CLOCK_STOP@@ reason=test_slot_starvation pid=[0-9]+$' "$_CS2_ERR"
rm -f "$_CS2_ERR"

echo ""
echo "--- Test CS-3: clock_emit_heartbeat emits exact grammar @@REIFY_CLOCK_HEARTBEAT@@ reason=... waited=... to stderr ---"

_CS3_ERR="$(mktemp)"
bash -c 'source "$1" >/dev/null 2>&1; clock_emit_heartbeat test_slot_starvation 42' _ "$_CLOCK_LIB" 2>"$_CS3_ERR" || true
assert "Test CS-3a: clock_emit_heartbeat stderr contains @@REIFY_CLOCK_HEARTBEAT@@" \
    grep -q '@@REIFY_CLOCK_HEARTBEAT@@' "$_CS3_ERR"
assert "Test CS-3b: clock_emit_heartbeat stderr contains reason=test_slot_starvation" \
    grep -q 'reason=test_slot_starvation' "$_CS3_ERR"
assert "Test CS-3c: clock_emit_heartbeat stderr contains waited=42" \
    grep -q 'waited=42' "$_CS3_ERR"
assert "Test CS-3d: clock_emit_heartbeat full line matches exact grammar: @@REIFY_CLOCK_HEARTBEAT@@ reason=test_slot_starvation waited=42" \
    grep -qE '^@@REIFY_CLOCK_HEARTBEAT@@ reason=test_slot_starvation waited=42$' "$_CS3_ERR"
rm -f "$_CS3_ERR"

echo ""
echo "--- Test CS-4: clock_emit_start emits exact grammar @@REIFY_CLOCK_START@@ reason=... waited=... to stderr ---"

_CS4_ERR="$(mktemp)"
bash -c 'source "$1" >/dev/null 2>&1; clock_emit_start test_slot_starvation 7' _ "$_CLOCK_LIB" 2>"$_CS4_ERR" || true
assert "Test CS-4a: clock_emit_start stderr contains @@REIFY_CLOCK_START@@" \
    grep -q '@@REIFY_CLOCK_START@@' "$_CS4_ERR"
assert "Test CS-4b: clock_emit_start stderr contains reason=test_slot_starvation" \
    grep -q 'reason=test_slot_starvation' "$_CS4_ERR"
assert "Test CS-4c: clock_emit_start stderr contains waited=7" \
    grep -q 'waited=7' "$_CS4_ERR"
assert "Test CS-4d: clock_emit_start full line matches exact grammar: @@REIFY_CLOCK_START@@ reason=test_slot_starvation waited=7" \
    grep -qE '^@@REIFY_CLOCK_START@@ reason=test_slot_starvation waited=7$' "$_CS4_ERR"
rm -f "$_CS4_ERR"

echo ""
echo "--- Test CS-5: reason= arg is passed through to each emitter (psi_pressure variant) ---"

_CS5_ERR="$(mktemp)"
bash -c 'source "$1" >/dev/null 2>&1; clock_emit_stop psi_pressure; clock_emit_heartbeat psi_pressure 5; clock_emit_start psi_pressure 5' _ "$_CLOCK_LIB" 2>"$_CS5_ERR" || true
assert "Test CS-5a: clock_emit_stop(psi_pressure) full line matches grammar" \
    grep -qE '^@@REIFY_CLOCK_STOP@@ reason=psi_pressure pid=[0-9]+$' "$_CS5_ERR"
assert "Test CS-5b: clock_emit_heartbeat(psi_pressure) full line matches grammar" \
    grep -qE '^@@REIFY_CLOCK_HEARTBEAT@@ reason=psi_pressure waited=5$' "$_CS5_ERR"
assert "Test CS-5c: clock_emit_start(psi_pressure) full line matches grammar" \
    grep -qE '^@@REIFY_CLOCK_START@@ reason=psi_pressure waited=5$' "$_CS5_ERR"
rm -f "$_CS5_ERR"

# ===========================================================================
# SIGNAL (g): clock-stop orchestration helper contracts (Tests CD-1..CD-3)
# Unit-tests the three NEW orchestration helpers extracted into lib_clock_stop.sh:
#   clock_maybe_heartbeat REASON START_TS LAST_HB_VAR
#   clock_enter_wait      REASON WAITED_VAR LAST_HB_VAR
#   clock_exit_wait       REASON WAITED ELAPSED
# RED today: functions do not exist → declare -F and behavioral greps fail.
# ===========================================================================

echo ""
echo "--- Test CD-1: clock_maybe_heartbeat — existence, EMIT path, THROTTLE no-op, EMPTY-reason no-op ---"

assert "Test CD-1a: clock_maybe_heartbeat is defined in lib_clock_stop.sh" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F clock_maybe_heartbeat' _ "$_CLOCK_LIB"

# EMIT path: LAST_HB_VAR=0, interval=1s, start_ts 10s ago → heartbeat must fire
_CD1_ERR="$(mktemp)"
_CD1b_out=$(bash -c '
    source "$1" >/dev/null 2>&1
    my_last_hb=0
    start_ts=$(( $(date +%s) - 10 ))
    REIFY_CLOCK_HEARTBEAT_SECS=1
    clock_maybe_heartbeat "test_reason" "$start_ts" my_last_hb
    echo "$my_last_hb"
' _ "$_CLOCK_LIB" 2>"$_CD1_ERR") || true

assert "Test CD-1b: clock_maybe_heartbeat EMIT — stderr matches @@REIFY_CLOCK_HEARTBEAT@@ reason=test_reason waited=[0-9]+" \
    grep -qE '^@@REIFY_CLOCK_HEARTBEAT@@ reason=test_reason waited=[0-9]+$' "$_CD1_ERR"

assert "Test CD-1c: clock_maybe_heartbeat EMIT — LAST_HB_VAR updated to non-zero epoch" \
    bash -c '[ "${1:-0}" -gt 0 ]' _ "$_CD1b_out"

# THROTTLE no-op: LAST_HB_VAR=$(date +%s), interval=30 → no heartbeat
> "$_CD1_ERR"
bash -c '
    source "$1" >/dev/null 2>&1
    my_last_hb=$(date +%s)
    start_ts=$(date +%s)
    REIFY_CLOCK_HEARTBEAT_SECS=30
    clock_maybe_heartbeat "test_reason" "$start_ts" my_last_hb
' _ "$_CLOCK_LIB" 2>"$_CD1_ERR" || true

assert "Test CD-1d: clock_maybe_heartbeat THROTTLE no-op — no heartbeat when last_hb is recent" \
    bash -c '! grep -q "@@REIFY_CLOCK_HEARTBEAT@@" "$1"' _ "$_CD1_ERR"

# EMPTY-reason no-op: reason="" → no @@REIFY_CLOCK_*@@ output
> "$_CD1_ERR"
bash -c '
    source "$1" >/dev/null 2>&1
    my_last_hb=0
    start_ts=$(( $(date +%s) - 60 ))
    REIFY_CLOCK_HEARTBEAT_SECS=1
    clock_maybe_heartbeat "" "$start_ts" my_last_hb
' _ "$_CLOCK_LIB" 2>"$_CD1_ERR" || true

assert "Test CD-1e: clock_maybe_heartbeat EMPTY reason — no @@REIFY_CLOCK_*@@ output" \
    bash -c '! grep -q "@@REIFY_CLOCK" "$1"' _ "$_CD1_ERR"

rm -f "$_CD1_ERR"

echo ""
echo "--- Test CD-2: clock_enter_wait — existence, first-entry STOP+vars, already-waiting silent, empty-reason silent ---"

assert "Test CD-2a: clock_enter_wait is defined in lib_clock_stop.sh" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F clock_enter_wait' _ "$_CLOCK_LIB"

# First-entry (WAITED_VAR=0, reason set) → STOP emitted, LAST_HB_VAR non-zero, WAITED_VAR=1
_CD2_ERR="$(mktemp)"
_CD2bcd_out=$(bash -c '
    source "$1" >/dev/null 2>&1
    my_waited=0
    my_last_hb=0
    clock_enter_wait "test_reason" my_waited my_last_hb
    echo "$my_waited $my_last_hb"
' _ "$_CLOCK_LIB" 2>"$_CD2_ERR") || true

assert "Test CD-2b: clock_enter_wait first-entry — emits @@REIFY_CLOCK_STOP@@ reason=test_reason pid=[0-9]+" \
    grep -qE '^@@REIFY_CLOCK_STOP@@ reason=test_reason pid=[0-9]+$' "$_CD2_ERR"

_CD2_waited=$(echo "$_CD2bcd_out" | awk '{print $1}')
_CD2_last_hb=$(echo "$_CD2bcd_out" | awk '{print $2}')

assert "Test CD-2c: clock_enter_wait first-entry — WAITED_VAR set to 1 (got '${_CD2_waited:-}')" \
    test "${_CD2_waited:-0}" -eq 1

assert "Test CD-2d: clock_enter_wait first-entry — LAST_HB_VAR updated to non-zero epoch" \
    test "${_CD2_last_hb:-0}" -gt 0

# Already-waiting (WAITED_VAR=1) → NO STOP, WAITED_VAR stays 1
> "$_CD2_ERR"
_CD2e_out=$(bash -c '
    source "$1" >/dev/null 2>&1
    my_waited=1
    my_last_hb=0
    clock_enter_wait "test_reason" my_waited my_last_hb
    echo "$my_waited"
' _ "$_CLOCK_LIB" 2>"$_CD2_ERR") || true

assert "Test CD-2e-stop: clock_enter_wait already-waiting (WAITED=1) — NO STOP emitted" \
    bash -c '! grep -q "@@REIFY_CLOCK_STOP@@" "$1"' _ "$_CD2_ERR"

assert "Test CD-2e-var: clock_enter_wait already-waiting — WAITED_VAR stays 1" \
    test "${_CD2e_out:-0}" -eq 1

# Empty reason (WAITED_VAR=0) → NO STOP, WAITED_VAR still set to 1
> "$_CD2_ERR"
_CD2f_out=$(bash -c '
    source "$1" >/dev/null 2>&1
    my_waited=0
    my_last_hb=0
    clock_enter_wait "" my_waited my_last_hb
    echo "$my_waited"
' _ "$_CLOCK_LIB" 2>"$_CD2_ERR") || true

assert "Test CD-2f-stop: clock_enter_wait empty reason — NO STOP emitted" \
    bash -c '! grep -q "@@REIFY_CLOCK_STOP@@" "$1"' _ "$_CD2_ERR"

assert "Test CD-2f-var: clock_enter_wait empty reason — WAITED_VAR still set to 1" \
    test "${_CD2f_out:-0}" -eq 1

rm -f "$_CD2_ERR"

echo ""
echo "--- Test CD-3: clock_exit_wait — existence, waited=1 emits START, waited=0 silent, empty-reason silent ---"

assert "Test CD-3a: clock_exit_wait is defined in lib_clock_stop.sh" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F clock_exit_wait' _ "$_CLOCK_LIB"

# waited=1 + reason set → emits @@REIFY_CLOCK_START@@ with exact ELAPSED
_CD3_ERR="$(mktemp)"
bash -c '
    source "$1" >/dev/null 2>&1
    clock_exit_wait "test_reason" 1 42
' _ "$_CLOCK_LIB" 2>"$_CD3_ERR" || true

assert "Test CD-3b: clock_exit_wait waited=1 — emits @@REIFY_CLOCK_START@@ reason=test_reason waited=42" \
    grep -qE '^@@REIFY_CLOCK_START@@ reason=test_reason waited=42$' "$_CD3_ERR"

# waited=0 → NO START (fast path is silent)
> "$_CD3_ERR"
bash -c '
    source "$1" >/dev/null 2>&1
    clock_exit_wait "test_reason" 0 42
' _ "$_CLOCK_LIB" 2>"$_CD3_ERR" || true

assert "Test CD-3c: clock_exit_wait waited=0 — NO START emitted" \
    bash -c '! grep -q "@@REIFY_CLOCK_START@@" "$1"' _ "$_CD3_ERR"

# empty reason → NO START
> "$_CD3_ERR"
bash -c '
    source "$1" >/dev/null 2>&1
    clock_exit_wait "" 1 42
' _ "$_CLOCK_LIB" 2>"$_CD3_ERR" || true

assert "Test CD-3d: clock_exit_wait empty reason — NO START emitted" \
    bash -c '! grep -q "@@REIFY_CLOCK_START@@" "$1"' _ "$_CD3_ERR"

rm -f "$_CD3_ERR"

# ===========================================================================
# Summary
# ===========================================================================

test_summary
