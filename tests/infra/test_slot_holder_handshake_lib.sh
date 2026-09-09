#!/usr/bin/env bash
# tests/infra/test_slot_holder_handshake_lib.sh
#
# Deterministic unit tests for tests/infra/slot_holder_handshake_lib.sh — the
# SPOT home for the CAUSAL holder-handshake primitives that replace the fixed
# `sleep 0.2` holder grace across the infra suites.
#
# Context: PRD docs/prds/infra-test-wallclock-deflake.md §2 (the S/R/T/C
# toolkit) and its D1 decision that absolute wall-clock upper bounds are
# ABANDONED for this class rather than re-tuned.  The precedent these
# primitives generalise is occt_wait_until_slot_held (PRD
# docs/prds/merge-gate-health.md W4b, task 5258).
#
# WHY NOTHING HERE CAN FLAKE UNDER LOAD:
# every assertion checks a CAUSAL OUTCOME — a non-blocking flock probe fails
# (some process holds the slot) or succeeds (the slot is free), a marker file
# exists, a running max over a SYNTHETIC event log equals an exact integer, a
# poll counter grows with the injected load factor.  None compares a measured
# wall-clock magnitude against a literal ceiling, so host load changes how long
# a case takes but never whether it passes.
#
# TWO-WAY BOUNDARY TESTS: every case ships a negative control, so a broken or
# absent mechanism goes RED instead of passing vacuously.  The negative
# controls use the `bash -c "! helper ..."` form (the
# test_occt_flock_gate_bounds.sh:156 idiom) so the EXPORTED helper really runs
# in the child shell — a non-exported helper would make the negation a vacuous
# command-not-found.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

# Loud missing-lib guard (the tests/infra/test_verify_pipeline_guard.sh:81
# idiom): the whole point of this file is the lib, so its absence must be a
# named hard failure rather than a cascade of confusing not-found errors.
[ -f "$SCRIPT_DIR/slot_holder_handshake_lib.sh" ] || {
    echo "ERROR: slot_holder_handshake_lib.sh not found at $SCRIPT_DIR/slot_holder_handshake_lib.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/slot_holder_handshake_lib.sh"

_TMPD="$(mktemp -d "${TMPDIR:-/tmp}/reify-slot-handshake.XXXXXX")"
_SPAWNED_PIDS=()
cleanup() {
    local _p
    for _p in "${_SPAWNED_PIDS[@]+${_SPAWNED_PIDS[@]}}"; do
        kill "$_p" 2>/dev/null || true
    done
    rm -rf "$_TMPD"
}
trap cleanup EXIT

# Non-blocking flock probe, the observable both barriers are built on.
# Probe SUCCEEDS (rc 0) => the slot is FREE.  Probe FAILS (rc non-zero) => some
# other process HOLDS the slot's exclusive flock.  `9>>` creates the file if
# absent so a probe racing ahead of a holder converges on the same inode.
slot_probe_free() { ( flock -n -x 9 ) 9>>"$1"; }
slot_probe_held() { ! slot_probe_free "$1"; }

echo "=== slot_holder_handshake_lib.sh causal holder-handshake unit tests ==="

# ============================================================================
# (a) holder_wait_until_held — causal flock-probe barrier
# ============================================================================
echo ""
echo "--- holder_wait_until_held: causal flock-probe barrier ---"

# POSITIVE: a real background flock owner is spawned with NO grace pause at all
# — the barrier IS the wait.  This is the property the retired fixed pause
# cannot offer: under load the background subshell may not be scheduled inside
# any fixed window, leaving the slot free when the contended operation runs.
_SLOT_A="$_TMPD/a.slot"
( flock -x 9; sleep 5 ) 9>>"$_SLOT_A" &
_PID_A=$!
_SPAWNED_PIDS+=("$_PID_A")

assert "holder_wait_until_held: confirms a live owner with no grace pause (returns 0)" \
    holder_wait_until_held "$_SLOT_A"

assert "holder_wait_until_held: the probe agrees the slot is really taken" \
    slot_probe_held "$_SLOT_A"

# Pins the `export -f`: run in a CHILD shell against the still-held slot, where
# an unexported helper would be command-not-found and fail.  Without this the
# `bash -c "! ..."` negative controls below could pass vacuously for exactly
# that reason.
assert "holder_wait_until_held: the exported helper really runs in a child shell" \
    bash -c "holder_wait_until_held '$_SLOT_A'"

kill "$_PID_A" 2>/dev/null || true
wait "$_PID_A" 2>/dev/null || true

# NEGATIVE CONTROL: a fresh FREE slot is never taken, so the barrier must
# exhaust its (tiny) backstop budget and return non-zero.  Without this the
# positive case above could pass by always returning 0.
_SLOT_A_FREE="$_TMPD/a-free.slot"
assert "holder_wait_until_held: free slot exhausts its backstop budget (non-zero)" \
    bash -c "! holder_wait_until_held '$_SLOT_A_FREE' 2"

# ============================================================================
# (b) holder_wait_for_marker — causal ready-file barrier
# ============================================================================
echo ""
echo "--- holder_wait_for_marker: causal ready-file barrier ---"

_MARK_B="$_TMPD/b.ready"
( sleep 0.3; touch "$_MARK_B" ) &
_PID_B=$!
_SPAWNED_PIDS+=("$_PID_B")

assert "holder_wait_for_marker: returns 0 once a background writer creates the marker" \
    holder_wait_for_marker "$_MARK_B"

assert "holder_wait_for_marker: the marker really exists on return" \
    test -e "$_MARK_B"

# Same export pin as (a): a child shell must see the real helper, so the
# never-created negative control below cannot pass as command-not-found.
assert "holder_wait_for_marker: the exported helper really runs in a child shell" \
    bash -c "holder_wait_for_marker '$_MARK_B'"

wait "$_PID_B" 2>/dev/null || true

# NEGATIVE CONTROL: a marker nobody ever creates must exhaust the budget.
assert "holder_wait_for_marker: marker never created exhausts its budget (non-zero)" \
    bash -c "! holder_wait_for_marker '$_TMPD/b-never.ready' 2"

# ============================================================================
# (c) holder_spawn_gated — a TEST-RELEASED owner
#
# The defect this closes is the second half of the fixed-pause race: a
# self-timed `( flock -x 9; sleep N )` owner can be OUTLIVED by the operation
# it is supposed to contend with, so the contention silently disappears.  A
# gated owner holds until the TEST releases it, so the hold strictly contains
# whatever the test does in between.
# ============================================================================
echo ""
echo "--- holder_spawn_gated: test-released gated owner ---"

_SLOT_C="$_TMPD/c.slot"
_READY_C="$_TMPD/c.ready"
_RELEASE_C="$_TMPD/c.release"
_PID_C="$(holder_spawn_gated "$_SLOT_C" "$_READY_C" "$_RELEASE_C")"
_SPAWNED_PIDS+=("$_PID_C")

assert "holder_spawn_gated: echoes a live PID" \
    kill -0 "$_PID_C"

assert "holder_spawn_gated: signals readiness through its ready marker" \
    holder_wait_for_marker "$_READY_C"

assert "holder_spawn_gated: the slot is provably taken once ready" \
    slot_probe_held "$_SLOT_C"

# The property a self-timed owner cannot offer: the hold survives an
# interleaved unrelated operation of arbitrary cost.  Real work, not a pause,
# so this case stays deterministic.
_SYNTH_C="$_TMPD/c.interleave.log"
printf '100 1111 ACQUIRE slot-1\n200 1111 RELEASE\n' > "$_SYNTH_C"
_INTERLEAVE_N=0
while [ "$_INTERLEAVE_N" -lt 40 ]; do
    holder_max_concurrent "$_SYNTH_C" >/dev/null
    _INTERLEAVE_N=$(( _INTERLEAVE_N + 1 ))
done

assert "holder_spawn_gated: the hold survives an interleaved unrelated operation" \
    slot_probe_held "$_SLOT_C"

# NEGATIVE CONTROL: the gated owner is bound to ITS slot only — an unrelated
# slot stays free, so `slot_probe_held` is not a constant-true predicate.
assert "holder_spawn_gated: an unrelated slot is NOT reported taken" \
    slot_probe_free "$_TMPD/c-unrelated.slot"

# ============================================================================
# (d) holder_release — the test ends the hold
# ============================================================================
echo ""
echo "--- holder_release: test-driven end of hold ---"

assert "holder_release: returns 0 for the gated owner spawned above" \
    holder_release "$_RELEASE_C" "$_PID_C"

assert "holder_release: the owner process is gone" \
    bash -c "! kill -0 '$_PID_C' 2>/dev/null"

assert "holder_release: the slot is free again" \
    slot_probe_free "$_SLOT_C"

# NEGATIVE CONTROL: releasing without ever touching the release file leaves the
# owner holding — proving the release file, not the mere call, ends the hold.
_SLOT_D="$_TMPD/d.slot"
_READY_D="$_TMPD/d.ready"
_RELEASE_D="$_TMPD/d.release"
_PID_D="$(holder_spawn_gated "$_SLOT_D" "$_READY_D" "$_RELEASE_D")"
_SPAWNED_PIDS+=("$_PID_D")
holder_wait_for_marker "$_READY_D"

assert "holder_release: an unreleased owner still holds its slot (release file is the cause)" \
    slot_probe_held "$_SLOT_D"

holder_release "$_RELEASE_D" "$_PID_D" || true

# ============================================================================
# (e) holder_max_concurrent — R-technique event-log predicate
#
# Purely synthetic log inputs: no real invocations, no pauses, cannot flake.
# Log format (scripts/lib_slot_acquire.sh REIFY_SLOT_EVENT_LOG contract):
#   <epoch_ns> <pid> ACQUIRE slot-N
#   <epoch_ns> <pid> RELEASE
# ============================================================================
echo ""
echo "--- holder_max_concurrent: R-technique event-log predicate ---"

_LOG_PAR="$_TMPD/e-parallel.log"
printf '100 1111 ACQUIRE slot-1\n200 2222 ACQUIRE slot-2\n300 1111 RELEASE\n400 2222 RELEASE\n' \
    > "$_LOG_PAR"
assert "holder_max_concurrent: PARALLEL log (A/A/R/R) -> 2" \
    test "$(holder_max_concurrent "$_LOG_PAR")" -eq 2

# The N->1 non-vacuity control: an over-serialization regression must be
# DISTINGUISHABLE from correct N=2 behaviour, which the retired millisecond
# band could not do (both landed inside it).
_LOG_SER="$_TMPD/e-serial.log"
printf '100 1111 ACQUIRE slot-1\n200 1111 RELEASE\n300 2222 ACQUIRE slot-1\n400 2222 RELEASE\n' \
    > "$_LOG_SER"
assert "holder_max_concurrent: SERIALIZED log (A/R/A/R) -> 1 (an N->1 regression is visible)" \
    test "$(holder_max_concurrent "$_LOG_SER")" -eq 1

_LOG_3INV="$_TMPD/e-three.log"
printf '100 1111 ACQUIRE slot-1\n200 2222 ACQUIRE slot-2\n300 1111 RELEASE\n400 2222 RELEASE\n500 3333 ACQUIRE slot-1\n600 3333 RELEASE\n' \
    > "$_LOG_3INV"
assert "holder_max_concurrent: THREE invocations at N=2 -> 2 (cap honored, never 3)" \
    test "$(holder_max_concurrent "$_LOG_3INV")" -eq 2

# Proves the predicate orders by the epoch-ns field, not by physical append
# order: concurrent O_APPEND writers may land lines in any order.
#
# The physical order below is deliberately one that DISAGREES with the ns
# order: read as written it is R/A/A/R, whose running max is 1, while the
# ns-sorted sequence is A/A/R/R, whose running max is 2.  A fixture whose two
# readings happen to agree (any pure A/A/R/R shuffle) cannot tell a sorting
# predicate from a non-sorting one and would pass either way.
_LOG_SCR="$_TMPD/e-scrambled.log"
printf '300 1111 RELEASE\n100 1111 ACQUIRE slot-1\n200 2222 ACQUIRE slot-2\n400 2222 RELEASE\n' \
    > "$_LOG_SCR"
assert "holder_max_concurrent: SCRAMBLED lines whose ns field reorders to A/A/R/R -> 2 (physical order alone would give 1)" \
    test "$(holder_max_concurrent "$_LOG_SCR")" -eq 2

_LOG_EMPTY="$_TMPD/e-empty.log"
: > "$_LOG_EMPTY"
assert "holder_max_concurrent: EMPTY log -> 0" \
    test "$(holder_max_concurrent "$_LOG_EMPTY")" -eq 0

# ============================================================================
# (f) Backstop budget is load-scaled, and is counted in POLLS, never measured
#
# The budget exists only so a never-arriving owner cannot hang the suite.  It
# is a BROKEN-INFRA BACKSTOP, not a timing assertion, so what is asserted here
# is the number of poll ITERATIONS the barrier performs — an exact integer that
# is identical on an idle and a loaded host.  `sleep` is shadowed by a
# zero-cost counter stub, which removes real time from the loop entirely.
# ============================================================================
echo ""
echo "--- backstop budget: poll-count scaling (counted, never measured) ---"

_holder_poll_iterations() {
    local _factor="$1" _slot="$2"
    local _countf="$_TMPD/pollcount.$_factor"
    : > "$_countf"
    sleep() { printf 'tick\n' >> "$_countf"; }
    REIFY_LOAD_TOLERANCE_FACTOR="$_factor" holder_wait_until_held "$_slot" 1 || true
    unset -f sleep
    wc -l < "$_countf" | tr -d ' '
}

_SLOT_F="$_TMPD/f-free.slot"
_POLLS_AT_1="$(_holder_poll_iterations 1 "$_SLOT_F")"
_POLLS_AT_4="$(_holder_poll_iterations 4 "$_SLOT_F")"

assert "backstop budget: factor 1 polls the unscaled base exactly once for BASE=1" \
    test "$_POLLS_AT_1" -eq 1

assert "backstop budget: factor 4 polls four times for BASE=1" \
    test "$_POLLS_AT_4" -eq 4

# NEGATIVE CONTROL: a budget that ignored the load factor would give equal
# counts, so this comparison goes RED if the scaling is ever dropped.
assert "backstop budget: factor 4 polls strictly more often than factor 1" \
    test "$_POLLS_AT_4" -gt "$_POLLS_AT_1"

test_summary
