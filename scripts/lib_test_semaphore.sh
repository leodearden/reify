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

    # N-slot shuffle-acquire loop.
    # Deadline checked BEFORE sleep (copy of cargo-test-occt-gated.sh:214-223).
    local _start _deadline _acq _ORDER _SLOT _SLOT_FILE
    _start="$(date +%s)"
    _deadline=$(( _start + WAIT ))
    _acq=0

    while true; do
        # Fresh shuffle each retry pass (thundering-herd avoidance).
        if command -v shuf >/dev/null 2>&1; then
            _ORDER="$(shuf -i "1-${N}")"
        else
            _ORDER="$(seq 1 "${N}")"
        fi

        for _SLOT in $_ORDER; do
            _SLOT_FILE="${LOCK}.slot-${_SLOT}"
            # Open slot file on FD 9 (append — no truncation of shared lock inode).
            # INVARIANT: at most one FD 9 is open at any time.
            exec 9>>"$_SLOT_FILE"
            if flock -xn 9; then
                _acq=1
                break
            fi
            # Acquisition failed — close FD 9 so the file description is freed.
            exec 9>&-
        done

        if [ "$_acq" -eq 1 ]; then
            break
        fi

        # All N slots busy.  Check deadline BEFORE sleeping.
        local _now
        _now="$(date +%s)"
        if [ "$_now" -ge "$_deadline" ]; then
            echo "lib_test_semaphore.sh: failed to acquire test slot within ${WAIT}s (LOCK=${LOCK}, N=${N})" >&2
            return 75
        fi
        sleep 0.5
    done

    _REIFY_TEST_SEMAPHORE_HELD=1
    local _elapsed
    _elapsed=$(( $(date +%s) - _start ))
    echo "lib_test_semaphore.sh: acquired test slot (slot ${_SLOT}/${N}) after ${_elapsed}s (LOCK=${LOCK})" >&2
    return 0
}

# ---------------------------------------------------------------------------
# test_semaphore_release
#   Release the held slot by closing FD 9.  No-op if not held.
# ---------------------------------------------------------------------------
test_semaphore_release() {
    if [ "${_REIFY_TEST_SEMAPHORE_HELD:-0}" = "1" ]; then
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
