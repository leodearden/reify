#!/usr/bin/env bash
# scripts/lib_test_semaphore.sh — N-slot counting semaphore for the TEST-EXECUTION phase.
#
# Designed to be sourced by callers (e.g. verify.sh/β) that need to hold a
# test-run slot across multiple passes, AND directly executable as a wrapper
# (the main-guard at the end enables `./scripts/lib_test_semaphore.sh cmd...`).
#
# FUNCTIONS (defined when sourced):
#   test_semaphore_acquire  — open+flock a slot on FD 9; return 0 on success,
#                             75 (EX_TEMPFAIL) on deadline, 64 on bad args.
#                             All functions return (never exit) so a sourcing
#                             shell decides what to do on failure.
#   test_semaphore_release  — close FD 9 (no-op if not held).
#   test_semaphore_run CMD  — acquire → run CMD with FD 9 closed (9<&-) →
#                             release; propagates CMD's exit code.
#
# KNOBS (environment variables):
#   REIFY_TEST_SEMAPHORE_DISABLE      set to 1 for a total bypass (no slot acquired)
#   REIFY_TEST_SEMAPHORE_CONCURRENCY  N slot count (default 1, must be positive int)
#   REIFY_TEST_SEMAPHORE_LOCK         base path for slot files
#                                     default: ${TMPDIR:-/tmp}/reify-test-semaphore-$(id -u).lock
#   REIFY_TEST_SEMAPHORE_WAIT         max seconds to wait for a slot (default 1800)
#
# β-side contract (when verify.sh holds the slot across multiple passes):
#   source this lib, call test_semaphore_acquire or test_semaphore_run.
#   When holding the slot across passes via acquire/release, each pass MUST
#   run with FD 9 closed: `<cmd> 9<&-`. test_semaphore_run already does this.
#   See the 2026-04-20 wedge class in cargo-test-occt-gated.sh:43-55.
#
# Direct-exec usage (standalone / mechanism test driver):
#   REIFY_TEST_SEMAPHORE_LOCK=/tmp/my.lock \
#   REIFY_TEST_SEMAPHORE_CONCURRENCY=2      \
#     ./scripts/lib_test_semaphore.sh cargo nextest run ...

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_TEST_SEMAPHORE_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_TEST_SEMAPHORE_SH_SOURCED=1

# Source the shared slot-acquire core (mechanism-only; contains slot_acquire
# and slot_emit_event).  CWD-independent via BASH_SOURCE resolution so this
# works whether sourced by verify.sh (from REPO_ROOT) or run directly.
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib_slot_acquire.sh"

# Internal flag: set to 1 when a slot is successfully acquired.
_REIFY_TEST_SEMAPHORE_HELD=0

# ---------------------------------------------------------------------------
# test_semaphore_acquire
#   Acquire one slot from the N-slot counting semaphore.
#   On success, FD 9 is held open (exclusive flock) in the calling process.
#   All early-return paths that do NOT acquire a slot leave FD 9 closed and
#   _REIFY_TEST_SEMAPHORE_HELD=0 so test_semaphore_release is a safe no-op.
# ---------------------------------------------------------------------------
test_semaphore_acquire() {
    # (1) DISABLE bypass — total bypass: no slot acquired, no wait.
    if [ "${REIFY_TEST_SEMAPHORE_DISABLE:-}" = "1" ]; then
        echo "lib_test_semaphore.sh: disabled (REIFY_TEST_SEMAPHORE_DISABLE=1) — no slot acquired" >&2
        return 0
    fi

    # (2) Merge bypass — skip acquisition; mirrors psi_gate() in verify.sh:161-165.
    if [ "${DF_VERIFY_ROLE:-task}" = "merge" ]; then
        echo "lib_test_semaphore.sh: bypass (role=merge) — no slot acquired" >&2
        return 0
    fi

    # Resolve knobs.
    local LOCK N WAIT
    LOCK="${REIFY_TEST_SEMAPHORE_LOCK:-${TMPDIR:-/tmp}/reify-test-semaphore-$(id -u).lock}"
    N="${REIFY_TEST_SEMAPHORE_CONCURRENCY:-1}"
    WAIT="${REIFY_TEST_SEMAPHORE_WAIT:-1800}"

    # Validate N is a positive integer.
    case "$N" in
        ''|*[!0-9]*)
            echo "lib_test_semaphore.sh: REIFY_TEST_SEMAPHORE_CONCURRENCY must be a positive integer (got '${N}')" >&2
            return 64
            ;;
    esac
    if [ "$N" -lt 1 ]; then
        echo "lib_test_semaphore.sh: REIFY_TEST_SEMAPHORE_CONCURRENCY must be >= 1 (got '${N}')" >&2
        return 64
    fi

    # Validate WAIT is a non-negative integer.  A non-numeric value would
    # silently corrupt the bash arithmetic _deadline=$(( _start + WAIT )) in a
    # sourced caller that runs without set -e (exit status 1 from (( )) leaves
    # _deadline empty, then [ "$_now" -ge "$_deadline" ] throws "integer
    # expression expected" on every pass, producing a noisy spin).
    case "$WAIT" in
        ''|*[!0-9]*)
            echo "lib_test_semaphore.sh: REIFY_TEST_SEMAPHORE_WAIT must be a non-negative integer (got '${WAIT}')" >&2
            return 64
            ;;
    esac

    # Preflight: flock is required for slot acquisition.  The main-guard also
    # checks this for direct-exec usage, but the sourced path (verify.sh/β) must
    # fail fast here too — without this check a missing flock causes every
    # `flock -xn 9` to return 127 (treated as "slot busy"), spinning until
    # REIFY_TEST_SEMAPHORE_WAIT (default 1800s) elapses with no diagnostic.
    # Mirrors cargo-test-occt-gated.sh:100-104.
    if ! command -v flock >/dev/null 2>&1; then
        echo "lib_test_semaphore.sh: flock not found on PATH — cannot acquire test slot" >&2
        return 1
    fi

    # Validate lock parent directory.
    local _LOCK_PARENT
    _LOCK_PARENT="$(dirname "$LOCK")"
    if [ ! -d "$_LOCK_PARENT" ]; then
        echo "lib_test_semaphore.sh: lock parent directory '${_LOCK_PARENT}' does not exist (REIFY_TEST_SEMAPHORE_LOCK='${LOCK}')" >&2
        return 1
    fi
    if [ ! -w "$_LOCK_PARENT" ]; then
        echo "lib_test_semaphore.sh: lock parent directory '${_LOCK_PARENT}' is not writable (REIFY_TEST_SEMAPHORE_LOCK='${LOCK}')" >&2
        return 1
    fi

    # N-slot shuffle-acquire loop — delegated to scripts/lib_slot_acquire.sh.
    # slot_acquire() is the single source of truth for the shuffle/deadline/
    # FD-9 mechanism; both this lib and scripts/cargo-test-occt-gated.sh source
    # it.  Bug fixes to the acquire loop go in lib_slot_acquire.sh only.
    if slot_acquire "$LOCK" "$N" "$WAIT"; then
        _REIFY_TEST_SEMAPHORE_HELD=1
        echo "lib_test_semaphore.sh: acquired test slot (slot ${SLOT_ACQUIRE_SLOT}/${N}) after ${SLOT_ACQUIRE_ELAPSED}s (LOCK=${LOCK})" >&2
        return 0
    else
        local _rc=$?
        if [ "$_rc" -eq 75 ]; then
            echo "lib_test_semaphore.sh: failed to acquire test slot within ${WAIT}s (LOCK=${LOCK}, N=${N})" >&2
        fi
        return $_rc
    fi
}

# ---------------------------------------------------------------------------
# test_semaphore_release
#   Release the held slot by closing FD 9.  No-op if not held.
# ---------------------------------------------------------------------------
test_semaphore_release() {
    if [ "${_REIFY_TEST_SEMAPHORE_HELD:-0}" = "1" ]; then
        # Emit RELEASE BEFORE closing FD 9: a waiting process cannot win
        # flock -xn 9 until FD 9 is closed, so ts(RELEASE) < ts(next ACQUIRE)
        # is guaranteed — the causal ordering invariant for the R technique.
        slot_emit_event RELEASE
        exec 9>&-
        _REIFY_TEST_SEMAPHORE_HELD=0
    fi
}

# ---------------------------------------------------------------------------
# test_semaphore_run CMD [ARGS...]
#   Acquire a slot, run CMD with FD 9 closed (9<&- prevents daemon inheritance),
#   release the slot, return CMD's exit code.
# ---------------------------------------------------------------------------
test_semaphore_run() {
    test_semaphore_acquire || return $?
    local _rc=0
    "$@" 9<&- || _rc=$?
    test_semaphore_release
    return $_rc
}

# ---------------------------------------------------------------------------
# Main-guard: when executed directly, enable strict mode and run the wrapper.
# ---------------------------------------------------------------------------
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    set -euo pipefail

    if ! command -v flock >/dev/null 2>&1; then
        echo "ERROR: lib_test_semaphore.sh requires flock (util-linux) but it was not found on PATH." >&2
        exit 1
    fi

    if [ "$#" -eq 0 ]; then
        echo "Usage: $(basename "$0") CMD [ARGS...]" >&2
        exit 64
    fi

    test_semaphore_run "$@"
    exit $?
fi
