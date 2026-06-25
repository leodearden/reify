#!/usr/bin/env bash
# tests/infra/test_slot_event_log.sh — mechanism tests for scripts/lib_slot_acquire.sh
# and the REIFY_SLOT_EVENT_LOG opt-in event-log substrate (task T1 / #4840 foundation).
#
# Auto-discovered by tests/infra/run_all.sh (pattern test_*.sh).
#
# Asserts ordering/structure ONLY — no absolute wall-clock upper bounds —
# so tests stay green under cpu_load_fixture-induced load (section F).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

source "$SCRIPT_DIR/test_helpers.sh"

LIB="$REPO_ROOT/scripts/lib_slot_acquire.sh"
SEM="$REPO_ROOT/scripts/lib_test_semaphore.sh"
OCCT="$REPO_ROOT/scripts/cargo-test-occt-gated.sh"
FIX="$SCRIPT_DIR/cpu_load_fixture.sh"

echo "=== test_slot_event_log.sh: lib_slot_acquire.sh + REIFY_SLOT_EVENT_LOG tests ==="

# ============================================================================
# (A) Structure: lib exists, is sourceable, defines slot_acquire + slot_emit_event
# ============================================================================

echo ""
echo "--- (A) Structure ---"

assert "(A) scripts/lib_slot_acquire.sh exists" \
    test -f "$LIB"

assert "(A) lib is sourceable with no side effects" \
    bash -c 'source "$1" >/dev/null 2>&1' -- "$LIB"

assert "(A) lib defines slot_acquire after sourcing" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F slot_acquire >/dev/null' -- "$LIB"

assert "(A) lib defines slot_emit_event after sourcing" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F slot_emit_event >/dev/null' -- "$LIB"

# ============================================================================
# (B) Semaphore-path emission: one invocation → ACQUIRE + RELEASE in event-log
# ============================================================================

echo ""
echo "--- (B) Semaphore-path emission ---"

_LOCK_B="$(mktemp)"
_LOG_B="$(mktemp)"

DF_VERIFY_ROLE=task \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK_B" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    REIFY_SLOT_EVENT_LOG="$_LOG_B" \
    "$SEM" bash -c true 2>/dev/null

_LINES_B="$(wc -l < "$_LOG_B" | tr -d ' ')"

assert "(B) exactly 2 log lines emitted (ACQUIRE + RELEASE)" \
    test "$_LINES_B" -eq 2

_LINE1_B="$(sed -n '1p' "$_LOG_B")"
_LINE2_B="$(sed -n '2p' "$_LOG_B")"

assert "(B) line 1 matches format: <epoch_ns> <pid> ACQUIRE slot-<N>" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^[0-9]+ [0-9]+ ACQUIRE slot-[0-9]+$"' -- "$_LINE1_B"

assert "(B) line 2 matches format: <epoch_ns> <pid> RELEASE" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^[0-9]+ [0-9]+ RELEASE$"' -- "$_LINE2_B"

_TS_ACQ_B="$(printf '%s\n' "$_LINE1_B" | awk '{print $1}')"
_TS_REL_B="$(printf '%s\n' "$_LINE2_B" | awk '{print $1}')"
_PID_ACQ_B="$(printf '%s\n' "$_LINE1_B" | awk '{print $2}')"
_PID_REL_B="$(printf '%s\n' "$_LINE2_B" | awk '{print $2}')"

assert "(B) ts(ACQUIRE) <= ts(RELEASE) numerically" \
    bash -c 'test -n "$1" && test -n "$2" && test "$1" -le "$2"' -- "$_TS_ACQ_B" "$_TS_REL_B"

assert "(B) pid(ACQUIRE) == pid(RELEASE) — same shell process" \
    bash -c 'test -n "$1" && test -n "$2" && test "$1" -eq "$2"' -- "$_PID_ACQ_B" "$_PID_REL_B"

rm -f "$_LOCK_B" "${_LOCK_B}.slot-1" "$_LOG_B"

# ============================================================================
# (C) No-op when REIFY_SLOT_EVENT_LOG is unset — zero side effects
# ============================================================================

echo ""
echo "--- (C) No-op when REIFY_SLOT_EVENT_LOG is unset ---"

_LOCK_C="$(mktemp)"
_PROBE_C="/tmp/slot_evlog_noop_probe_$$"
_STDERR_C="$(mktemp)"
_EXIT_C=0

env -u REIFY_SLOT_EVENT_LOG \
    DF_VERIFY_ROLE=task \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK_C" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    "$SEM" bash -c true 2>"$_STDERR_C" || _EXIT_C=$?

assert "(C) exits 0 when REIFY_SLOT_EVENT_LOG unset (got $_EXIT_C)" \
    test "$_EXIT_C" -eq 0

assert "(C) no probe file created (slot_emit_event has zero side effects when unset)" \
    test ! -f "$_PROBE_C"

assert "(C) stderr contains normal 'acquired test slot' diagnostic" \
    grep -q 'acquired test slot' "$_STDERR_C"

assert "(C) stderr does NOT contain 'ACQUIRE' (event-log words absent)" \
    bash -c '! grep -q ACQUIRE "$1"' -- "$_STDERR_C"

assert "(C) stderr does NOT contain 'RELEASE' (event-log words absent)" \
    bash -c '! grep -q RELEASE "$1"' -- "$_STDERR_C"

rm -f "$_LOCK_C" "${_LOCK_C}.slot-1" "$_STDERR_C" "$_PROBE_C"

# ============================================================================
# (D) OCCT-path emission shares the SAME substrate
# ============================================================================

echo ""
echo "--- (D) OCCT-path emission ---"

_LOCK_D="$(mktemp)"
_LOG_D="$(mktemp)"

REIFY_OCCT_LOCK="$_LOCK_D" \
    REIFY_OCCT_CONCURRENCY=1 \
    REIFY_SLOT_EVENT_LOG="$_LOG_D" \
    "$OCCT" true 2>/dev/null

_LINES_D="$(wc -l < "$_LOG_D" | tr -d ' ')"

assert "(D) exactly 2 log lines emitted by OCCT path (ACQUIRE + RELEASE)" \
    test "$_LINES_D" -eq 2

_LINE1_D="$(sed -n '1p' "$_LOG_D")"
_LINE2_D="$(sed -n '2p' "$_LOG_D")"

assert "(D) line 1 matches ACQUIRE format" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^[0-9]+ [0-9]+ ACQUIRE slot-[0-9]+$"' -- "$_LINE1_D"

assert "(D) line 2 matches RELEASE format" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^[0-9]+ [0-9]+ RELEASE$"' -- "$_LINE2_D"

_TS_ACQ_D="$(printf '%s\n' "$_LINE1_D" | awk '{print $1}')"
_TS_REL_D="$(printf '%s\n' "$_LINE2_D" | awk '{print $1}')"
_PID_ACQ_D="$(printf '%s\n' "$_LINE1_D" | awk '{print $2}')"
_PID_REL_D="$(printf '%s\n' "$_LINE2_D" | awk '{print $2}')"

assert "(D) ts(ACQUIRE) <= ts(RELEASE)" \
    bash -c 'test -n "$1" && test -n "$2" && test "$1" -le "$2"' -- "$_TS_ACQ_D" "$_TS_REL_D"

assert "(D) pid(ACQUIRE) == pid(RELEASE)" \
    bash -c 'test -n "$1" && test -n "$2" && test "$1" -eq "$2"' -- "$_PID_ACQ_D" "$_PID_REL_D"

rm -f "$_LOCK_D" "${_LOCK_D}.slot-1" "$_LOG_D"

# ============================================================================
# (E) Causal serialization (the R substrate)
#     Two concurrent N=1 semaphore invocations → 4 log lines that, sorted by
#     timestamp, are ACQUIRE, RELEASE, ACQUIRE, RELEASE with
#     ts(2nd ACQUIRE) > ts(1st RELEASE) — the causal ordering T2 relies on.
# ============================================================================

echo ""
echo "--- (E) Causal serialization ---"

_LOCK_E="$(mktemp)"
_LOG_E="$(mktemp)"

DF_VERIFY_ROLE=task \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK_E" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    REIFY_SLOT_EVENT_LOG="$_LOG_E" \
    "$SEM" bash -c 'sleep 0.2' 2>/dev/null &
_PID_E1=$!

DF_VERIFY_ROLE=task \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK_E" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    REIFY_SLOT_EVENT_LOG="$_LOG_E" \
    "$SEM" bash -c 'sleep 0.2' 2>/dev/null &
_PID_E2=$!

wait "$_PID_E1" "$_PID_E2"

_LINES_E="$(wc -l < "$_LOG_E" | tr -d ' ')"
assert "(E) exactly 4 log lines (2 ACQUIRE + 2 RELEASE)" \
    test "$_LINES_E" -eq 4

# Sort by nanosecond timestamp (field 1, numeric) and extract verb sequence.
_SORTED_E="$(sort -k1,1n "$_LOG_E")"
_EV1="$(printf '%s\n' "$_SORTED_E" | awk 'NR==1{print $3}')"
_EV2="$(printf '%s\n' "$_SORTED_E" | awk 'NR==2{print $3}')"
_EV3="$(printf '%s\n' "$_SORTED_E" | awk 'NR==3{print $3}')"
_EV4="$(printf '%s\n' "$_SORTED_E" | awk 'NR==4{print $3}')"

assert "(E) sorted event 1 is ACQUIRE" test "$_EV1" = "ACQUIRE"
assert "(E) sorted event 2 is RELEASE" test "$_EV2" = "RELEASE"
assert "(E) sorted event 3 is ACQUIRE" test "$_EV3" = "ACQUIRE"
assert "(E) sorted event 4 is RELEASE" test "$_EV4" = "RELEASE"

# Causal invariant: waiter's ACQUIRE ts must be strictly AFTER prior holder's RELEASE ts.
_TS_REL1_E="$(printf '%s\n' "$_SORTED_E" | awk 'NR==2{print $1}')"
_TS_ACQ2_E="$(printf '%s\n' "$_SORTED_E" | awk 'NR==3{print $1}')"

assert "(E) ts(2nd ACQUIRE) > ts(1st RELEASE) — waiter acquired only after holder released" \
    bash -c 'test -n "$1" && test -n "$2" && test "$2" -gt "$1"' -- "$_TS_REL1_E" "$_TS_ACQ2_E"

rm -f "$_LOCK_E" "${_LOCK_E}.slot-1" "$_LOG_E"

# ============================================================================
# (F) Under-load: repeat (B) and (E) with cpu_load_fixture backgrounded.
#     No absolute wall-clock bounds — structural/ordering only.
# ============================================================================

echo ""
echo "--- (F) Under load ---"

_FIX_PID=""
"$FIX" 4 2 --label slot-evlog &
_FIX_PID=$!

# (F-B) Single semaphore invocation under load.
_LOCK_FB="$(mktemp)"
_LOG_FB="$(mktemp)"

DF_VERIFY_ROLE=task \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK_FB" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    REIFY_SLOT_EVENT_LOG="$_LOG_FB" \
    "$SEM" bash -c true 2>/dev/null

_LINES_FB="$(wc -l < "$_LOG_FB" | tr -d ' ')"
assert "(F-B) under load: 2 log lines emitted" \
    test "$_LINES_FB" -eq 2

_LINE1_FB="$(sed -n '1p' "$_LOG_FB")"
_LINE2_FB="$(sed -n '2p' "$_LOG_FB")"

assert "(F-B) under load: line 1 is ACQUIRE" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^[0-9]+ [0-9]+ ACQUIRE slot-[0-9]+$"' -- "$_LINE1_FB"

assert "(F-B) under load: line 2 is RELEASE" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^[0-9]+ [0-9]+ RELEASE$"' -- "$_LINE2_FB"

_TS_ACQ_FB="$(printf '%s\n' "$_LINE1_FB" | awk '{print $1}')"
_TS_REL_FB="$(printf '%s\n' "$_LINE2_FB" | awk '{print $1}')"

assert "(F-B) under load: ts(ACQUIRE) <= ts(RELEASE)" \
    bash -c 'test -n "$1" && test -n "$2" && test "$1" -le "$2"' -- "$_TS_ACQ_FB" "$_TS_REL_FB"

rm -f "$_LOCK_FB" "${_LOCK_FB}.slot-1" "$_LOG_FB"

# (F-E) Causal serialization under load.
_LOCK_FE="$(mktemp)"
_LOG_FE="$(mktemp)"

DF_VERIFY_ROLE=task \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK_FE" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    REIFY_SLOT_EVENT_LOG="$_LOG_FE" \
    "$SEM" bash -c 'sleep 0.2' 2>/dev/null &
_PID_FE1=$!

DF_VERIFY_ROLE=task \
    REIFY_TEST_SEMAPHORE_LOCK="$_LOCK_FE" \
    REIFY_TEST_SEMAPHORE_CONCURRENCY=1 \
    REIFY_SLOT_EVENT_LOG="$_LOG_FE" \
    "$SEM" bash -c 'sleep 0.2' 2>/dev/null &
_PID_FE2=$!

wait "$_PID_FE1" "$_PID_FE2"

_LINES_FE="$(wc -l < "$_LOG_FE" | tr -d ' ')"
assert "(F-E) under load: 4 log lines" \
    test "$_LINES_FE" -eq 4

_SORTED_FE="$(sort -k1,1n "$_LOG_FE")"
_FV1="$(printf '%s\n' "$_SORTED_FE" | awk 'NR==1{print $3}')"
_FV2="$(printf '%s\n' "$_SORTED_FE" | awk 'NR==2{print $3}')"
_FV3="$(printf '%s\n' "$_SORTED_FE" | awk 'NR==3{print $3}')"
_FV4="$(printf '%s\n' "$_SORTED_FE" | awk 'NR==4{print $3}')"

assert "(F-E) under load: sorted event 1 is ACQUIRE" test "$_FV1" = "ACQUIRE"
assert "(F-E) under load: sorted event 2 is RELEASE" test "$_FV2" = "RELEASE"
assert "(F-E) under load: sorted event 3 is ACQUIRE" test "$_FV3" = "ACQUIRE"
assert "(F-E) under load: sorted event 4 is RELEASE" test "$_FV4" = "RELEASE"

_FTS_REL1="$(printf '%s\n' "$_SORTED_FE" | awk 'NR==2{print $1}')"
_FTS_ACQ2="$(printf '%s\n' "$_SORTED_FE" | awk 'NR==3{print $1}')"

assert "(F-E) under load: ts(2nd ACQUIRE) > ts(1st RELEASE)" \
    bash -c 'test -n "$1" && test -n "$2" && test "$2" -gt "$1"' -- "$_FTS_REL1" "$_FTS_ACQ2"

rm -f "$_LOCK_FE" "${_LOCK_FE}.slot-1" "$_LOG_FE"

# Reap the load fixture.
if [ -n "$_FIX_PID" ]; then
    kill "$_FIX_PID" 2>/dev/null || true
    wait "$_FIX_PID" 2>/dev/null || true
fi

test_summary
