#!/usr/bin/env bash
# tests/infra/slot_holder_handshake_lib.sh — CAUSAL holder-handshake primitives.
#
# WHY A SHARED LIB (SPOT):
# The infra suites all need the same two facts before they may run a contended
# operation: "a competing owner really holds the slot NOW", and "that owner is
# still holding while my operation runs".  Before this lib those facts were
# approximated by a fixed pause after backgrounding an owner, and the approximation
# is a TWO-SIDED race:
#   * OUTRUN — under load the background subshell may not be scheduled inside the
#     fixed window, so the slot is still FREE when the contended operation runs.
#     It then takes the uncontended fast path and the test's whole premise
#     evaporates (no blocking, no clock markers).
#   * OVERRUN — where the owner is self-timed (`( flock -x 9; sleep N )`), the
#     pause eats part of the hold, so the operation contends with only the
#     remainder — or with nothing at all.
# The primitives here close both sides: a causal barrier for the first, a
# TEST-RELEASED owner for the second.
#
# WHAT WAS CONSOLIDATED HERE, AND WHAT WAS NOT.  The same facts were open-coded
# in four divergent copies.  MIGRATED (task 6247): occt_wait_until_slot_held in
# tests/infra/occt_flock_gate_lib.sh, whose occt_*-prefixed forwarder is now
# deleted and whose call sites use holder_wait_until_held directly; an inline
# `flock -xn` probe in tests/infra/test_run_all.sh; and _wait_for_reader_lock in
# tests/infra/test_warm_lane_gc.sh, now deleted with all six of its call sites
# on holder_wait_for_marker.
#
# STILL OUTSTANDING: tests/infra/test_warm_lane_pool.sh carries an
# identically-named _wait_for_reader_lock twin, layered over a helper of its own
# and with its own unit tests (its Block RH).  That file was outside task 6247's
# lock set, so the copy stands; migrating it is follow-up work.  Until then this
# lib is the single home for its four users, NOT for every marker-poll in
# tests/infra — do not read the SPOT claim wider than that list.
#
# The two argument conventions differ where a caller was migrated: this lib
# counts POLL ITERATIONS (load-scaled), where _wait_for_reader_lock took a
# deadline in SECONDS at a 0.05s tick.  30s there is 150 iterations here.
#
# WHAT THESE FUNCTIONS ASSERT — AND WHAT THEY DO NOT:
# every barrier here returns on a CAUSAL OUTCOME (a non-blocking flock probe
# fails; a marker file exists; a process has exited).  None of them measures or
# compares a wall-clock magnitude.  Per PRD docs/prds/infra-test-wallclock-deflake.md
# decision D1, absolute wall-clock upper bounds are ABANDONED for this class
# rather than re-tuned, so a caller must never rebuild one on top of these.
#
# THE POLL BUDGETS ARE BROKEN-INFRA BACKSTOPS, NOT TIMING ASSERTIONS.
# Each bounded loop exists so a never-arriving owner cannot hang the suite; the
# bound is deliberately far larger than any legitimate wait and is scaled by
# tests/infra/load_tolerance_lib.sh so it only ever GROWS under load.  Reaching
# a bound means the infrastructure is broken, never that something was "too
# slow".  This is the rationale the retired occt_wait_until_slot_held carried
# (PRD docs/prds/merge-gate-health.md W4b, task 5258), kept verbatim in spirit
# now that its call sites read this file instead.
#
# Unit tests: tests/infra/test_slot_holder_handshake_lib.sh.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_SLOT_HOLDER_HANDSHAKE_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_SLOT_HOLDER_HANDSHAKE_LIB_SH_SOURCED=1

_reify_shh_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$_reify_shh_dir/load_tolerance_lib.sh" ] || {
    echo "ERROR: load_tolerance_lib.sh not found at $_reify_shh_dir/load_tolerance_lib.sh" >&2
    return 1 2>/dev/null || exit 1
}
# shellcheck disable=SC1090,SC1091
source "$_reify_shh_dir/load_tolerance_lib.sh"
unset _reify_shh_dir

# The barriers below are `export -f`d so a caller's `bash -c "! holder_... "`
# negative control runs the REAL helper in the child shell rather than a
# vacuous command-not-found.  An exported barrier is useless without its budget
# helper, so those travel with it — the same reason tests/infra/test_proc_reaper.sh:35
# exports the keepalive pair out of the same lib.
export -f load_tolerance_factor load_tolerant_attempts

# holder_wait_until_held SLOT_FILE [BASE_ITERS=100]
# Return 0 once some OTHER process holds SLOT_FILE's exclusive flock.
#
# Polls a NON-BLOCKING `flock -n -x 9` probe: a probe that SUCCEEDS means the
# slot is FREE (no owner yet) so we keep polling; a probe that FAILS proves an
# owner holds it, which is the causal fact the caller needs.  This replaces the
# fixed post-spawn pause and is what makes "the owner is holding" an OBSERVED
# fact rather than an assumption.
#
# `9>>"$slot"` opens for append, creating the file if absent, so a probe that
# races ahead of the owner self-heals: both converge on the same inode.  The
# probe runs as an `if` condition so a non-zero `flock -n` never trips
# `set -euo pipefail` in the caller.
#
# BASE_ITERS x 0.2s (scaled by load_tolerant_attempts) is a BROKEN-INFRA
# BACKSTOP so a never-arriving owner cannot hang the suite — it is NOT a timing
# assertion.  Returns non-zero if the budget is exhausted without ever
# observing the slot held.
holder_wait_until_held() {
    local _slot="$1"
    local _budget
    _budget="$(load_tolerant_attempts "${2:-100}")"
    local _i=0
    while [ "$_i" -lt "$_budget" ]; do
        if ! ( flock -n -x 9 ) 9>>"$_slot"; then
            return 0
        fi
        sleep 0.2
        _i=$(( _i + 1 ))
    done
    return 1
}
export -f holder_wait_until_held

# holder_wait_for_marker MARKER_FILE [BASE_ITERS=100]
# Return 0 once MARKER_FILE exists — the ready-file half of the handshake, for
# owners that announce readiness by touching a file rather than by taking a
# lock.  Same loop shape and the same BROKEN-INFRA BACKSTOP budget as
# holder_wait_until_held above; it asserts the causal fact "the marker exists",
# never a magnitude.  Returns non-zero if the budget is exhausted first.
holder_wait_for_marker() {
    local _marker="$1"
    local _budget
    _budget="$(load_tolerant_attempts "${2:-100}")"
    local _i=0
    while [ "$_i" -lt "$_budget" ]; do
        if [ -e "$_marker" ]; then
            return 0
        fi
        sleep 0.2
        _i=$(( _i + 1 ))
    done
    return 1
}
export -f holder_wait_for_marker

# holder_spawn_gated SLOT_FILE READY_FILE RELEASE_FILE [BASE_ITERS=600]
# Spawn a background owner that takes SLOT_FILE's exclusive flock, touches
# READY_FILE, and then holds until the TEST creates RELEASE_FILE.  Echoes the
# owner's PID to stdout.
#
# This is the replacement for the self-timed `( flock -x 9; sleep N )` owner.
# Because the TEST ends the hold (via holder_release), the hold interval
# strictly CONTAINS whatever the test does in between, however long that takes
# under load — the guarantee a fixed N can never give.
#
# The owner's stdout and stderr go to /dev/null so a caller capturing the PID
# with `$( ... )` does not block: a background child inheriting the command
# substitution's pipe would hold its write end open until it exited, and the
# capture would wait for exactly the owner it is trying to spawn.
#
# BASE_ITERS x 0.2s (load-scaled) is a BROKEN-INFRA BACKSTOP so a test that
# dies before releasing cannot leave an owner holding a slot forever — it is
# NOT a timing assertion, and a healthy test always ends the hold first.
holder_spawn_gated() {
    local _slot="$1" _ready="$2" _release="$3"
    local _budget
    _budget="$(load_tolerant_attempts "${4:-600}")"
    (
        flock -x 9
        touch "$_ready"
        _held=0
        while [ ! -e "$_release" ] && [ "$_held" -lt "$_budget" ]; do
            sleep 0.2
            _held=$(( _held + 1 ))
        done
    ) 9>>"$_slot" >/dev/null 2>&1 &
    echo "$!"
}
export -f holder_spawn_gated

# holder_release RELEASE_FILE PID [BASE_ITERS=600]
# End a gated owner's hold and return 0 once that owner has actually exited.
#
# `wait` alone is not enough: when the caller captured the PID with `$( ... )`
# the owner is a child of that (now-gone) subshell, so `wait` returns
# immediately without having observed anything.  Polling `kill -0` afterwards
# makes "the owner is gone" — and therefore "the slot is free" — an OBSERVED
# fact in both cases rather than one that holds only when the PID happened to
# be a direct child.
#
# Returns non-zero if the owner is still alive when the BROKEN-INFRA BACKSTOP
# budget (BASE_ITERS x 0.2s, load-scaled) is exhausted.
holder_release() {
    local _release="$1" _pid="$2"
    local _budget
    _budget="$(load_tolerant_attempts "${3:-600}")"
    touch "$_release"
    wait "$_pid" 2>/dev/null || true
    local _i=0
    while [ "$_i" -lt "$_budget" ]; do
        if ! kill -0 "$_pid" 2>/dev/null; then
            return 0
        fi
        sleep 0.2
        _i=$(( _i + 1 ))
    done
    return 1
}
export -f holder_release

# holder_max_concurrent EVENT_LOG
# R-technique predicate (PRD docs/prds/infra-test-wallclock-deflake.md §2/T3).
# Reads a slot event log (REIFY_SLOT_EVENT_LOG format from
# scripts/lib_slot_acquire.sh) and echoes the maximum number of slots held
# simultaneously across all events in the log.
#
# Log line format:
#   <epoch_ns> <pid> ACQUIRE slot-N   (emitted by slot_acquire on success)
#   <epoch_ns> <pid> RELEASE          (emitted by caller before closing FD 9)
#
# Why sort -n by the leading epoch-ns field (NOT physical line order):
#   Concurrent wrapper PIDs write via O_APPEND (atomic EoF appends), but the
#   OS may schedule competing appends in any order, so physical line order may
#   differ from nanosecond-timestamp order.  The CAUSAL ORDERING INVARIANT in
#   scripts/lib_slot_acquire.sh guarantees ts(prev RELEASE) < ts(next ACQUIRE),
#   so ns-sorted order is the canonical causal sequence.
#
# This is the discriminating replacement for the millisecond band the occt
# serialization tests used to assert: an exact expected maximum separates
# correct N=2 behaviour (2) from an over-serialization regression (1) and from
# a lost cap (3), all three of which landed inside that band undetected.
#
# Echoes an integer >= 0.  Empty log or RELEASE-only log -> 0.
holder_max_concurrent() {
    local _log="$1"
    sort -n "$_log" | awk '
        $3 == "ACQUIRE" { c++; if (c > m) m = c }
        $3 == "RELEASE" { c-- }
        END { print m+0 }
    '
}
export -f holder_max_concurrent
