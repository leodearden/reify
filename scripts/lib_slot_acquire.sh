#!/usr/bin/env bash
# scripts/lib_slot_acquire.sh — shared N-slot shuffle-acquire core (sourced-only).
#
# Extracted common mechanism shared by:
#   scripts/lib_test_semaphore.sh    (test-run semaphore, sourced by verify.sh)
#   scripts/cargo-test-occt-gated.sh (OCCT concurrency gate, standalone runner)
#
# FUNCTIONS (defined when sourced):
#   slot_acquire LOCK_BASE N WAIT  — shuffle-acquire loop; holds FD 9 in the
#                                    CALLER's shell on success. Returns 0 on
#                                    success (slot held), 75 (EX_TEMPFAIL) on
#                                    deadline. Sets output globals:
#                                      SLOT_ACQUIRE_SLOT    — slot number acquired
#                                      SLOT_ACQUIRE_ELAPSED — wall seconds waited
#   slot_emit_event VERB [SLOT]    — opt-in event-log append (complete no-op
#                                    when REIFY_SLOT_EVENT_LOG is unset).
#
# OUTPUT GLOBALS (set by slot_acquire):
#   SLOT_ACQUIRE_SLOT    — slot number (1..N) acquired; "" on 75 return.
#   SLOT_ACQUIRE_ELAPSED — seconds elapsed waiting; 0 on 75 return.
#   Both are always defined after slot_acquire returns (set -u safe).
#
# INVOCATION CONTRACT:
#   Always SOURCED, never executed directly (no main-guard, not chmod +x).
#   slot_acquire MUST be called in the CALLER's shell (not in a subshell /
#   command-substitution) so that `exec 9>>` mutates the caller's FD table
#   and FD 9 is held by the caller process.  Callers run child processes with
#   9<&- to prevent daemon FD inheritance (the 2026-04-20 sccache wedge class).
#
# EVENT LOG (opt-in, zero side effects when REIFY_SLOT_EVENT_LOG unset):
#   When REIFY_SLOT_EVENT_LOG is set to a writable path, each acquire/release
#   pair is appended atomically (single printf <= PIPE_BUF = 4096 B):
#     <epoch_ns> <pid> ACQUIRE slot-N   (emitted by slot_acquire on success)
#     <epoch_ns> <pid> RELEASE           (emitted by CALLER before closing FD 9)
#   $$ is the holder shell PID — stable across ACQUIRE and RELEASE since both
#   are called from the same sourced-function / script context.
#   Nanosecond timestamps (date +%s%N) give the ordering resolution needed by
#   the R-technique substrate (T2/T3 in the de-flake PRD).
#
# CAUSAL ORDERING INVARIANT:
#   RELEASE is emitted BEFORE the caller closes FD 9, so a waiting process
#   cannot win `flock -xn 9` until after RELEASE is logged.  This guarantees
#   ts(prev RELEASE) < ts(next ACQUIRE) — the R-technique proof.
#
# DRIFT-GUARD NOTE:
#   This lib is sourced TRANSITIVELY (verify.sh → lib_test_semaphore.sh →
#   lib_slot_acquire.sh) and is therefore NOT auto-derived by
#   verify-pipeline-guard.sh's live source inspection of verify.sh.  It is
#   registered in scripts/verify-pipeline-paths.txt so the merge-worker
#   config fast-path cannot ambush a Rust task on an edit to this file alone
#   (the #4618/#4624 → #4288 class; see CLAUDE.md).

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_SLOT_ACQUIRE_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_SLOT_ACQUIRE_SH_SOURCED=1

# ---------------------------------------------------------------------------
# slot_emit_event VERB [SLOT]
#   Opt-in append to REIFY_SLOT_EVENT_LOG.  Guards on the env var as the
#   VERY FIRST operation — before any subshell or syscall — so the production
#   path is byte-for-byte unchanged when REIFY_SLOT_EVENT_LOG is unset.
#
#   Format:
#     With SLOT: "<epoch_ns> <pid> <VERB> slot-<SLOT>\n"   (ACQUIRE)
#     Without:   "<epoch_ns> <pid> <VERB>\n"               (RELEASE)
#
#   The single printf is <= PIPE_BUF (4096 B) so each call is one atomic
#   write(2) — safe for concurrent callers sharing the same log path.
# ---------------------------------------------------------------------------
slot_emit_event() {
    [ -n "${REIFY_SLOT_EVENT_LOG:-}" ] || return 0
    local _verb="$1"
    local _slot="${2:-}"
    if [ -n "$_slot" ]; then
        printf '%s %s %s slot-%s\n' "$(date +%s%N)" "$$" "$_verb" "$_slot" \
            >> "$REIFY_SLOT_EVENT_LOG"
    else
        printf '%s %s %s\n' "$(date +%s%N)" "$$" "$_verb" \
            >> "$REIFY_SLOT_EVENT_LOG"
    fi
}

# ---------------------------------------------------------------------------
# slot_acquire LOCK_BASE N WAIT
#   N-slot shuffle-acquire loop.  Opens and holds FD 9 in the CALLER's shell
#   on success (sourced function — not a subshell — so `exec 9>>` mutates the
#   caller's FD table directly, preserving the single-FD-9 invariant).
#
#   Args:
#     LOCK_BASE  — base path; slot files are ${LOCK_BASE}.slot-1..N
#     N          — slot count (positive integer, caller-validated)
#     WAIT       — deadline in seconds (non-negative integer, caller-validated)
#
#   On success  — SLOT_ACQUIRE_SLOT=<N>, SLOT_ACQUIRE_ELAPSED=<secs>,
#                 FD 9 held open, slot_emit_event ACQUIRE called; returns 0.
#   On deadline — SLOT_ACQUIRE_SLOT="", SLOT_ACQUIRE_ELAPSED=0; returns 75.
#
#   FD-9 invariant: exactly one FD 9 is open at any time.  Each failed slot
#   attempt closes FD 9 (exec 9>&-) before trying the next, so no stale file
#   description leaks.  Callers MUST run child processes with 9<&- to prevent
#   daemon inheritance (the 2026-04-20 sccache wedge class).
# ---------------------------------------------------------------------------
slot_acquire() {
    local _lock_base="$1"
    local _n="$2"
    local _wait="$3"

    # Output globals — always defined after return (set -u safe on both paths).
    SLOT_ACQUIRE_SLOT=""
    SLOT_ACQUIRE_ELAPSED=0

    local _start _deadline _acq _ORDER _SLOT _SLOT_FILE
    _start="$(date +%s)"
    _deadline=$(( _start + _wait ))
    _acq=0

    while true; do
        # Fresh shuffle each retry pass (thundering-herd avoidance).
        # shuf comes from GNU coreutils; fall back to seq (ordered, still correct).
        if command -v shuf >/dev/null 2>&1; then
            _ORDER="$(shuf -i "1-${_n}")"
        else
            _ORDER="$(seq 1 "${_n}")"
        fi

        for _SLOT in $_ORDER; do
            _SLOT_FILE="${_lock_base}.slot-${_SLOT}"
            # Open slot file on FD 9 (append — no truncation of shared lock inode).
            # INVARIANT: at most one FD 9 is open at any time.  Successful
            # acquisition leaves FD 9 open (the held slot).  Failed acquisition
            # closes FD 9 immediately (exec 9>&-) so the file description is
            # fully released before the next attempt — no stale slot FD leak.
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

        # All N slots busy.  Check deadline BEFORE sleeping (not after) so the
        # caller exits within at most one retry-pass overhead (~0.5s) of the
        # deadline, regardless of N.
        local _now
        _now="$(date +%s)"
        if [ "$_now" -ge "$_deadline" ]; then
            return 75
        fi
        sleep 0.5
    done

    SLOT_ACQUIRE_SLOT="$_SLOT"
    SLOT_ACQUIRE_ELAPSED=$(( $(date +%s) - _start ))
    slot_emit_event ACQUIRE "$SLOT_ACQUIRE_SLOT"
    return 0
}
