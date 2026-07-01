#!/usr/bin/env bash
# scripts/lib_lane_x_flock.sh — Lane-X host-exclusive coarse single-slot flock.
#
# H8 (docs/prds/run-all-host-infra-partition.md): ONE coarse host-exclusive
# lock at a FIXED PER-UID HOST PATH, guarding the cold lane's single-flight
# run. This is the coarse single-slot variant of scripts/lib_test_semaphore.sh
# — it reuses scripts/lib_slot_acquire.sh's shuffle-acquire core with N
# pinned to 1.
#
# SHIPPED INERT (Part A): this lib is NOT wired into any tests/infra/run_all.sh
# path in this decompose (see the inert-shipping guard in
# tests/infra/test_lane_x_flock.sh). Consumers: H9 (reify-local runner,
# sibling task) acquires it directly; the dark-factory
# offline-deep-test-lane-worker.md invocation is a documented cross-project
# follow-up (PRD §6/§11), not wired here.
#
# FUNCTIONS (defined when sourced):
#   lane_x_flock_acquire  — open+flock the single slot on FD 9; return 0 on
#                           success, 75 (EX_TEMPFAIL) on deadline/contention.
#                           Returns (never exits) so a sourcing shell decides
#                           what to do on failure.
#   lane_x_flock_release  — close FD 9 (no-op if not held).
#
# KNOBS (environment variables):
#   REIFY_LANE_X_FLOCK_LOCK   base path for the slot file (slot file is
#                             ${LOCK}.slot-1).
#                             default: ${TMPDIR:-/tmp}/reify-lane-x-$(id -u).lock
#   REIFY_LANE_X_FLOCK_WAIT   max seconds to wait for the lock (default 0 —
#                             fail-fast single-flight: a second concurrent
#                             acquire while the lane is held returns 75
#                             immediately, no sleep), OR the sentinel
#                             "unlimited" (case-insensitive) for a continuous
#                             blocking wait with no deadline.
#
# No DF_VERIFY_ROLE=merge bypass (unlike lib_test_semaphore.sh): a merge
# exemption would defeat single-flight exclusion, which is this primitive's
# whole purpose.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_LANE_X_FLOCK_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_LANE_X_FLOCK_SH_SOURCED=1

# Source the shared slot-acquire core (mechanism-only; contains slot_acquire
# and slot_emit_event). CWD-independent via BASH_SOURCE resolution. Guarded
# existence check mirrors lib_slot_acquire.sh's own sourcing of
# lib_clock_stop.sh: a missing/mislocated lib surfaces a directed error, not
# a cryptic `source: No such file`.
_lxf_slot_lib="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib_slot_acquire.sh"
if [ ! -f "$_lxf_slot_lib" ]; then
    echo "lib_lane_x_flock.sh: required lib not found next to script: $_lxf_slot_lib" >&2
    exit 1
fi
source "$_lxf_slot_lib"

# Internal flag: set to 1 when the slot is successfully acquired.
_REIFY_LANE_X_FLOCK_HELD=0

# ---------------------------------------------------------------------------
# lane_x_flock_acquire
#   Acquire the single Lane-X host-exclusive slot.
#   On success, FD 9 is held open (exclusive flock) in the calling process.
# ---------------------------------------------------------------------------
lane_x_flock_acquire() {
    local LOCK WAIT
    LOCK="${REIFY_LANE_X_FLOCK_LOCK:-${TMPDIR:-/tmp}/reify-lane-x-$(id -u).lock}"
    WAIT="${REIFY_LANE_X_FLOCK_WAIT:-0}"

    # N-slot shuffle-acquire loop — delegated to scripts/lib_slot_acquire.sh,
    # called with N=1 (the coarse single-slot variant) and no REASON (3-arg;
    # no @@REIFY_CLOCK_*@@ markers — the cold-lane wait is not part of any
    # verify's clock-stop accounting).
    if slot_acquire "$LOCK" 1 "$WAIT"; then
        _REIFY_LANE_X_FLOCK_HELD=1
        return 0
    else
        local _rc=$?
        if [ "$_rc" -eq 75 ]; then
            echo "lib_lane_x_flock.sh: failed to acquire Lane-X lock within ${WAIT}s (LOCK=${LOCK})" >&2
        fi
        return $_rc
    fi
}

# ---------------------------------------------------------------------------
# lane_x_flock_release
#   Release the held slot by closing FD 9. No-op if not held.
# ---------------------------------------------------------------------------
lane_x_flock_release() {
    if [ "${_REIFY_LANE_X_FLOCK_HELD:-0}" = "1" ]; then
        # Emit RELEASE BEFORE closing FD 9 — preserves the causal-ordering
        # invariant documented in lib_slot_acquire.sh (RELEASE logged before
        # FD close), mirroring test_semaphore_release.
        slot_emit_event RELEASE
        exec 9>&-
        _REIFY_LANE_X_FLOCK_HELD=0
    fi
}

# ---------------------------------------------------------------------------
# lane_x_flock_run CMD [ARGS...]
#   Acquire the slot, run CMD with FD 9 closed (9<&- prevents daemon
#   inheritance), release the slot, return CMD's exit code.
# ---------------------------------------------------------------------------
lane_x_flock_run() {
    lane_x_flock_acquire || return $?
    local _rc=0
    "$@" 9<&- || _rc=$?
    lane_x_flock_release
    return $_rc
}

# ---------------------------------------------------------------------------
# Main-guard: when executed directly, enable strict mode and run the wrapper.
# ---------------------------------------------------------------------------
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    set -euo pipefail

    if ! command -v flock >/dev/null 2>&1; then
        echo "ERROR: lib_lane_x_flock.sh requires flock (util-linux) but it was not found on PATH." >&2
        exit 1
    fi

    if [ "$#" -eq 0 ]; then
        echo "Usage: $(basename "$0") CMD [ARGS...]" >&2
        exit 64
    fi

    lane_x_flock_run "$@"
    exit $?
fi
