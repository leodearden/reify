#!/usr/bin/env bash
# scripts/lib_slot_acquire.sh — shared N-slot shuffle-acquire core (sourced-only).
#
# Extracted common mechanism shared by:
#   scripts/lib_test_semaphore.sh    (test-run semaphore, sourced by verify.sh)
#   scripts/cargo-test-occt-gated.sh (OCCT concurrency gate, standalone runner)
#
# FUNCTIONS (defined when sourced):
#   slot_acquire LOCK_BASE N WAIT [REASON] [TIMEOUT_REASON] [TIMEOUT_DISPOSITION]
#                                  — shuffle-acquire loop; holds FD 9 in the
#                                    CALLER's shell on success. Returns 0 on
#                                    success (slot held), 75 (EX_TEMPFAIL) on
#                                    deadline. Sets output globals:
#                                      SLOT_ACQUIRE_SLOT    — slot number acquired
#                                      SLOT_ACQUIRE_ELAPSED — wall seconds waited
#   slot_emit_event VERB [SLOT]    — opt-in event-log append (complete no-op
#                                    when REIFY_SLOT_EVENT_LOG is unset).
#   slot_emit_timeout R S W D L    — the slot-timeout sentinel (see below);
#                                    called from the deadline site only.
#
# SLOT-TIMEOUT MARKER:
#   On the finite-WAIT deadline (the rc=75 path) slot_acquire emits ONE line to
#   stderr, first token at COLUMN 0:
#     @@REIFY_SLOT_TIMEOUT@@ reason=<R> slots=<N> waited=<secs> disposition=<D> lock=<LOCK_BASE>
#   It is strictly ADDITIVE alongside the human-readable deadline messages the
#   three wrapper callers print (dark-factory's other grounded anchors).
#   Emission contract -- why column 0, why lock= is last, why it is non-fatal:
#   slot_emit_timeout's header below.  Consumer, reason vocabulary and how this
#   family relates to @@REIFY_CLOCK_*@@: docs/notes/verify-pipeline-knobs.md.
#   Guard: tests/infra/test_slot_timeout_marker.sh.
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
# Guarded existence check mirrors verify.sh's lib_test_semaphore.sh sourcing: a
# missing/mislocated lib surfaces a directed error, not a cryptic `source: No such file`.
_sa_clock_lib="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/lib_clock_stop.sh"
if [ ! -f "$_sa_clock_lib" ]; then
    echo "lib_slot_acquire.sh: required lib not found next to script: $_sa_clock_lib" >&2
    exit 1
fi
source "$_sa_clock_lib"

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
# slot_emit_timeout REASON SLOTS WAITED DISPOSITION LOCK_BASE
#   Emit the slot-acquisition-timeout sentinel to stderr, ONCE, from the
#   finite-WAIT deadline site below.  Format:
#     @@REIFY_SLOT_TIMEOUT@@ reason=<REASON> slots=<SLOTS> waited=<WAITED> disposition=<DISPOSITION> lock=<LOCK_BASE>
#
#   Byte-identical in SHAPE to clock_emit_stop (lib_clock_stop.sh:140-142): a
#   single printf whose format string starts the marker token at COLUMN 0, a
#   literal trailing \n, and a >&2 redirect.  That is not stylistic -- it is the
#   emission contract dark-factory's SEMAPHORE_TIMEOUT classifier keys on, whose
#   matcher is line-anchored on the token as the FIRST token of its own line.
#   Never append this mid-line to another message.
#
#   FIELD ORDER IS LOAD-BEARING -- lock= is emitted LAST.  Every other field is
#   a repo-controlled token from a closed vocabulary, but LOCK_BASE is
#   OPERATOR-controlled on every path (REIFY_OCCT_LOCK / REIFY_LANE_X_FLOCK_LOCK
#   / REIFY_RUN_ALL_POOL_LOCK / REIFY_TEST_SEMAPHORE_LOCK, and the
#   ${TMPDIR:-/tmp}-derived defaults), so it is the one value that can contain
#   whitespace.  Terminal placement means a space-bearing path can only extend
#   the line, never shift reason=/slots=/waited=/disposition= out from under a
#   field-position parser -- and it also lets a consumer read lock= as
#   "everything after the marker", losslessly.  Pinned by the space-bearing
#   lock-base case in tests/infra/test_slot_timeout_marker.sh (A9).
#
#   NON-FATAL BY CONSTRUCTION (`|| return 0`), matching slot_emit_event's
#   established idiom.  This runs immediately before `return 75` in a function
#   whose EX_TEMPFAIL contract every wrapper keys on, and whose run_all.sh pool
#   caller must proceed to run its member unslotted.  A failed write to fd 2
#   (EPIPE on a dead log reader, ENOSPC on a redirected stderr file) must never
#   be able to substitute an ERR-exit for that contract: diagnostics never
#   change control flow.
#
#   A DISTINCT marker family from @@REIFY_CLOCK_*@@, and outside run_all.sh's
#   sanitizer prefix by design -- stated once, in docs/notes/verify-pipeline-knobs.md.
# ---------------------------------------------------------------------------
slot_emit_timeout() {
    printf '@@REIFY_SLOT_TIMEOUT@@ reason=%s slots=%s waited=%s disposition=%s lock=%s\n' \
        "$1" "$2" "$3" "$4" "$5" >&2 || return 0
}

# ---------------------------------------------------------------------------
# slot_acquire LOCK_BASE N WAIT [REASON] [TIMEOUT_REASON] [TIMEOUT_DISPOSITION]
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
#     TIMEOUT_REASON — OPTIONAL 5th arg.  Reason token carried by the
#                  @@REIFY_SLOT_TIMEOUT@@ sentinel on the rc=75 path only.
#                  Defaults to ${REASON:-slot_acquire}, so existing 3-arg and
#                  4-arg callers keep working and always put a non-empty reason
#                  on the wire.
#                  SEPARATE FROM REASON ON PURPOSE: REASON is load-bearing for
#                  clock-stop accounting, and callers that deliberately pass
#                  none (cargo-test-occt-gated.sh, lib_lane_x_flock.sh — whose
#                  waits are not part of any verify's clock-stop budget) would
#                  have that budget silently changed if naming their timeout
#                  required supplying one.  Pass "" as REASON alongside a
#                  TIMEOUT_REASON to say "no clock markers, but name my
#                  deadline".  Vocabulary: docs/notes/verify-pipeline-knobs.md.
#     TIMEOUT_DISPOSITION — OPTIONAL 6th arg, `fatal` (default) or `soft`.
#                  What the CALLER does with the rc=75, carried on the sentinel
#                  so a consumer can tell the two apart.  `fatal`: the caller
#                  aborts and propagates 75 (all three wrapper paths).  `soft`:
#                  the caller proceeds unslotted and the work still runs to
#                  completion (tests/infra/run_all.sh's pool worker, whose
#                  soft-admission contract is "never skip a test, never hang").
#                  Defaulting to `fatal` keeps every pre-existing caller's wire
#                  shape meaning exactly what it did before this field existed.
#                  NOTE (cross-repo): dark-factory's SEMAPHORE_TIMEOUT detector
#                  reads this field as of dark-factory task 4212 (closed
#                  2026-08-19): a `soft` deadline is advisory and falls through
#                  to per-tool dispatch instead of being classified as a
#                  starvation verdict; a sentinel with no `disposition=` field
#                  keeps DF's pre-4212 classify behavior.
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
    # OPTIONAL 5th: reason token carried by the deadline sentinel only, SEPARATE
    # from _reason -- see the TIMEOUT_REASON arg doc above for why.  The
    # ${_reason:-slot_acquire} fallback keeps every existing 3-/4-arg caller
    # working, set -u safe, and guarantees a non-empty reason field on the wire.
    # Both fallback branches are exercised directly by A7/A8 in
    # tests/infra/test_slot_timeout_marker.sh -- the literal `slot_acquire` token
    # is a value a consumer can observe, so it is pinned like any other.
    local _timeout_reason="${5:-${_reason:-slot_acquire}}"
    # OPTIONAL 6th: what the caller DOES with the 75 -- `fatal` (abort and
    # propagate, the default and what all three wrapper paths do) or `soft`
    # (proceed unslotted; run_all.sh's pool worker). Defaulting to fatal keeps
    # every pre-existing caller's wire shape meaning what it already did.
    local _timeout_disposition="${6:-fatal}"

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
    clock_now_epoch _start
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
        # Fork-free fast path at N==1 ONLY (task 4930, narrowed post-review):
        # shuffling a single-element order can never change try-order, so at
        # N=1 (the test-semaphore hot path) `shuf` forking every 0.5s
        # iteration bought nothing. N>=2 KEEPS the shuf/seq herd-avoidance
        # shuffle: slot count does not bound the number of waiters, so a
        # fixed "1 2 .. N" order would let 3+ concurrent waiters always race
        # slot-1 first every pass — the exact thundering-herd the shuffle
        # exists to avoid. (Correctness would still hold even with a static
        # order — non-blocking flock -xn means only one waiter wins per
        # pass — but this is a fairness/efficiency regression worth avoiding
        # for the N=2+ OCCT multi-slot case, which isn't the N=1 hot path
        # this hardening targets.)
        if [ "$_n" -eq 1 ]; then
            _ORDER="1"
        elif command -v shuf >/dev/null 2>&1; then
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

        # All N slots busy; enter/continue the wait (STOP fires on first entry).
        clock_enter_wait "$_reason" _sa_waited _sa_last_hb

        # Deadline check (finite mode only): BEFORE sleeping so the caller
        # exits within at most one retry-pass overhead (~0.5s) of the deadline.
        if [ "$_unlimited" -eq 0 ]; then
            local _now
            clock_now_epoch _now
            if [ "$_now" -ge "$_deadline" ]; then
                # STOP may have been emitted above (_sa_waited=1) but START is
                # intentionally NOT emitted here — exit-75 implicitly closes
                # the STOP span (see lib_clock_stop.sh FINITE-WAIT TIMEOUT note).
                #
                # The slot-timeout sentinel is emitted HERE and nowhere else:
                # this is the single deadline site every finite-WAIT caller
                # shares, and it sits INSIDE the `_unlimited -eq 0` branch, so
                # "never emitted under WAIT=unlimited" and "never emitted on an
                # uncontended acquire" are structural rather than conditional.
                # `_now` was just read by the deadline check above, so the
                # waited= value costs no extra clock read.  SLOT_ACQUIRE_ELAPSED
                # deliberately stays 0 on this path (the documented output-globals
                # contract); the elapsed value travels in waited= only.
                # The emitter is non-fatal by construction (see its header), so
                # a failed write to fd 2 can never pre-empt the `return 75`
                # below -- the EX_TEMPFAIL contract every caller keys on.
                slot_emit_timeout "$_timeout_reason" "$_n" "$(( _now - _start ))" \
                    "$_timeout_disposition" "$_lock_base"
                return 75
            fi
        fi

        sleep 0.5

        # Heartbeat: throttled emission from INSIDE the poll loop (PRD D4 liveness).
        clock_maybe_heartbeat "$_reason" "$_start" _sa_last_hb
    done

    SLOT_ACQUIRE_SLOT="$_SLOT"
    local _end
    clock_now_epoch _end
    SLOT_ACQUIRE_ELAPSED=$(( _end - _start ))

    # Emit START marker iff we actually waited (STOP/START balanced).
    clock_exit_wait "$_reason" "$_sa_waited" "$SLOT_ACQUIRE_ELAPSED"

    slot_emit_event ACQUIRE "$SLOT_ACQUIRE_SLOT"
    return 0
}
