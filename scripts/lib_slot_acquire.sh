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
#   pair is appended via O_APPEND (>>) to a regular file (atomic EoF writes):
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

# Source the shared clock-stop emitter (clock_emit_stop/heartbeat/start).
# CWD-independent via BASH_SOURCE resolution — mirrors the lib_test_semaphore.sh
# sourcing idiom for lib_slot_acquire.sh itself.
source "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib_clock_stop.sh"

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
#   O_APPEND (the >> redirection) positions each write atomically at
#   end-of-file; concurrent callers sharing the same log never interleave.
# ---------------------------------------------------------------------------
slot_emit_event() {
    [ -n "${REIFY_SLOT_EVENT_LOG:-}" ] || return 0
    local _verb="$1"
    local _slot="${2:-}"
    if [ -n "$_slot" ]; then
        printf '%s %s %s slot-%s\n' "$(date +%s%N)" "$$" "$_verb" "$_slot" \
            >> "$REIFY_SLOT_EVENT_LOG" || return 0
    else
        printf '%s %s %s\n' "$(date +%s%N)" "$$" "$_verb" \
            >> "$REIFY_SLOT_EVENT_LOG" || return 0
    fi
}

# ---------------------------------------------------------------------------
# slot_acquire LOCK_BASE N WAIT [REASON]
#   N-slot shuffle-acquire loop.  Opens and holds FD 9 in the CALLER's shell
#   on success (sourced function — not a subshell — so `exec 9>>` mutates the
#   caller's FD table directly, preserving the single-FD-9 invariant).
#
#   Args:
#     LOCK_BASE  — base path; slot files are ${LOCK_BASE}.slot-1..N
#     N          — slot count (positive integer, caller-validated)
#     WAIT       — deadline in seconds (non-negative integer, caller-validated),
#                  OR the sentinel "unlimited" (case-insensitive) to poll forever
#                  without a deadline (continuous clock-stop wait, PRD §3 option c).
#     REASON     — OPTIONAL 4th arg.  When non-empty and an actual wait occurs
#                  (first immediate acquire fails), emits @@REIFY_CLOCK_*@@
#                  markers to stderr via lib_clock_stop.sh:
#                    clock_emit_stop REASON     — once, on entering the wait
#                    clock_emit_heartbeat ...   — every REIFY_CLOCK_HEARTBEAT_SECS
#                    clock_emit_start REASON E  — once, on successful acquire
#                  When empty (default), no markers are emitted — the existing
#                  3-arg callers (cargo-test-occt-gated.sh) are byte-for-byte
#                  unchanged.
#
#   On success  — SLOT_ACQUIRE_SLOT=<N>, SLOT_ACQUIRE_ELAPSED=<secs>,
#                 FD 9 held open, slot_emit_event ACQUIRE called; returns 0.
#   On deadline — SLOT_ACQUIRE_SLOT="", SLOT_ACQUIRE_ELAPSED=0; returns 75.
#                 (Never returns 75 when WAIT=="unlimited".)
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
    local _reason="${4:-}"   # OPTIONAL: non-empty → emit @@REIFY_CLOCK_*@@ markers

    # Output globals — always defined after return (set -u safe on both paths).
    SLOT_ACQUIRE_SLOT=""
    SLOT_ACQUIRE_ELAPSED=0

    # Detect unlimited mode BEFORE arithmetic so the sentinel "unlimited"
    # never corrupts _deadline via integer overflow/error.
    local _unlimited=0
    case "${_wait}" in
        [Uu][Nn][Ll][Ii][Mm][Ii][Tt][Ee][Dd]) _unlimited=1 ;;
    esac

    local _start _deadline _acq _ORDER _SLOT _SLOT_FILE
    _start="$(date +%s)"
    if [ "$_unlimited" -eq 0 ]; then
        _deadline=$(( _start + _wait ))
    else
        _deadline=0   # unused in unlimited mode; set for set -u safety
    fi
    _acq=0

    # Clock-stop state: _sa_waited tracks whether we've entered a wait
    # (i.e. the first immediate acquire attempt failed).
    local _sa_waited=0
    # Track last heartbeat timestamp for interval throttling.
    local _sa_last_hb=0

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

        # All N slots busy.  Emit STOP marker the first time we enter a wait
        # (so markers fire only on actual contention, not on an uncontended acquire).
        if [ "$_sa_waited" -eq 0 ] && [ -n "$_reason" ]; then
            clock_emit_stop "$_reason"
            _sa_last_hb="$(date +%s)"
        fi
        _sa_waited=1

        # Deadline check (finite mode only): BEFORE sleeping so the caller
        # exits within at most one retry-pass overhead (~0.5s) of the deadline.
        if [ "$_unlimited" -eq 0 ]; then
            local _now
            _now="$(date +%s)"
            if [ "$_now" -ge "$_deadline" ]; then
                return 75
            fi
        fi

        sleep 0.5

        # Heartbeat: emit from INSIDE the poll loop (PRD D4 — liveness signal;
        # a wedged loop / SIGSTOP stops heartbeating; a dumb wall-clock timer
        # would mask a wedge).  Throttle to REIFY_CLOCK_HEARTBEAT_SECS.
        if [ -n "$_reason" ]; then
            local _hb_interval="${REIFY_CLOCK_HEARTBEAT_SECS:-30}"
            local _now_hb
            _now_hb="$(date +%s)"
            if [ $(( _now_hb - _sa_last_hb )) -ge "$_hb_interval" ]; then
                local _waited_so_far=$(( _now_hb - _start ))
                clock_emit_heartbeat "$_reason" "$_waited_so_far"
                _sa_last_hb="$_now_hb"
            fi
        fi
    done

    SLOT_ACQUIRE_SLOT="$_SLOT"
    SLOT_ACQUIRE_ELAPSED=$(( $(date +%s) - _start ))

    # Emit START marker iff we actually waited (STOP/START are balanced: both
    # fire on real contention, neither on an uncontended fast-path acquire).
    if [ "$_sa_waited" -eq 1 ] && [ -n "$_reason" ]; then
        clock_emit_start "$_reason" "$SLOT_ACQUIRE_ELAPSED"
    fi

    slot_emit_event ACQUIRE "$SLOT_ACQUIRE_SLOT"
    return 0
}
