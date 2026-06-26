#!/usr/bin/env bash
# scripts/lib_clock_stop.sh — shared @@REIFY_CLOCK_*@@ emitter (sourced-only).
#
# Single-source owner of the clock-stop marker grammar — the H two-way boundary
# contract between reify and dark_factory:1916 (the clock-stop-aware verify timeout).
# Sourced by BOTH admission gates:
#   scripts/lib_slot_acquire.sh (transitive: verify.sh → lib_test_semaphore.sh → lib_slot_acquire.sh)
#   scripts/cpu-admit.sh        (direct: PSI-gate clock-stop path)
#
# FUNCTIONS (defined when sourced):
#   clock_emit_stop      REASON          — emit STOP marker to stderr (entering wait)
#   clock_emit_heartbeat REASON WAITED   — emit HEARTBEAT marker to stderr (poll liveness)
#   clock_emit_start     REASON WAITED   — emit START marker to stderr (wait over)
#
# MARKER GRAMMAR (the H two-way wire contract with dark_factory:1916):
#   @@REIFY_CLOCK_STOP@@      reason=<reason> pid=<pid>
#   @@REIFY_CLOCK_HEARTBEAT@@ reason=<reason> waited=<secs>
#   @@REIFY_CLOCK_START@@     reason=<reason> waited=<secs>
#
#   All markers are emitted to STDERR.
#
# REASON VOCABULARY (stable tokens consumed by dark_factory:1916):
#   test_slot_starvation  — held-slot semaphore wait (lib_slot_acquire.sh)
#   psi_pressure          — PSI gate wait (cpu-admit.sh requeue mode)
#
# KNOB:
#   REIFY_CLOCK_HEARTBEAT_SECS   interval between HEARTBEAT emissions (default 30).
#     The caller's poll loop must call clock_emit_heartbeat on every iteration
#     once REIFY_CLOCK_HEARTBEAT_SECS seconds have elapsed since the last emission
#     (PRD D4: emitted from INSIDE the poll loop so a wedged loop / SIGSTOP stops
#     heartbeating — a dumb wall-clock timer would mask a wedge).
#
# DESIGN NOTES:
#   - STOP is emitted ONCE on entering the wait (first failed acquire/pressure-check).
#   - START is emitted ONCE on exiting the wait (acquire/pressure-clear succeeded).
#   - STOP and START are BALANCED on the SUCCESS path: both fire only on a real wait.
#   - An uncontended acquire (first immediate try succeeds) emits NOTHING.
#   - HEARTBEAT is emitted from inside the poll loop every REIFY_CLOCK_HEARTBEAT_SECS.
#   - Markers are additively emitted to stderr; they do not replace any existing
#     diagnostic messages from the calling lib.
#
# FINITE-WAIT TIMEOUT (exit 75) — IMPLICIT SPAN CLOSE:
#   When a finite WAIT deadline expires the callee returns 75 WITHOUT emitting
#   a START marker.  The consumer (dark_factory:1916) MUST treat process-exit
#   with a dangling STOP (no matching START) as 'span closed by exit' — the
#   process exits immediately after return 75, so the wall-clock span is bounded
#   by the verified_command_timeout_secs ceiling regardless.  Callers annotate
#   the return-75 site with "# STOP emitted; exit implicitly closes the span."
#
# GATED DORMANT (PRD §5 D5):
#   Marker emission is shipped dormant until dark_factory:1916 deploys (task 4838).
#   Markers are emitted on real contended waits but are inert pre-seam: current
#   dark-factory ignores unrecognized stderr, and the @@REIFY_CLOCK_*@@ tokens +
#   reason values avoid DF's output-text _CLASSIFY_PATTERNS (compile_error /
#   test_failure / flock_error) so they cannot be misclassified.
#
# DRIFT-GUARD NOTE:
#   This lib is sourced TRANSITIVELY (verify.sh → lib_test_semaphore.sh →
#   lib_slot_acquire.sh → lib_clock_stop.sh) and is therefore NOT auto-derived by
#   verify-pipeline-guard.sh's live source inspection of verify.sh.  It is
#   registered in scripts/verify-pipeline-paths.txt so an edit to this file alone
#   cannot take the merge-worker config fast-path (the #4618/#4624→#4288 class).

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_CLOCK_STOP_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_CLOCK_STOP_SH_SOURCED=1

# ---------------------------------------------------------------------------
# clock_emit_stop REASON
#   Emit the STOP marker to stderr once, on entering the wait.
#   Format: @@REIFY_CLOCK_STOP@@ reason=<REASON> pid=<pid>
# ---------------------------------------------------------------------------
clock_emit_stop() {
    printf '@@REIFY_CLOCK_STOP@@ reason=%s pid=%s\n' "$1" "$$" >&2
}

# ---------------------------------------------------------------------------
# clock_emit_heartbeat REASON WAITED
#   Emit a HEARTBEAT marker to stderr from inside the poll loop.
#   Format: @@REIFY_CLOCK_HEARTBEAT@@ reason=<REASON> waited=<WAITED>
#   WAITED is the number of seconds elapsed since entering the wait.
# ---------------------------------------------------------------------------
clock_emit_heartbeat() {
    printf '@@REIFY_CLOCK_HEARTBEAT@@ reason=%s waited=%s\n' "$1" "$2" >&2
}

# ---------------------------------------------------------------------------
# clock_emit_start REASON WAITED
#   Emit the START marker to stderr once, on exiting the wait successfully.
#   Format: @@REIFY_CLOCK_START@@ reason=<REASON> waited=<WAITED>
#   WAITED is the total number of seconds spent in the wait.
# ---------------------------------------------------------------------------
clock_emit_start() {
    printf '@@REIFY_CLOCK_START@@ reason=%s waited=%s\n' "$1" "$2" >&2
}

# ===========================================================================
# Clock-stop ORCHESTRATION helpers — build on the leaf emit primitives above.
# These centralize the STOP-once / heartbeat-throttle / START-iff-waited
# bookkeeping that was previously duplicated in cpu-admit.sh and
# lib_slot_acquire.sh.
#
# PASS-BY-NAME CONTRACT (bash indirection):
#   Helpers that mutate caller state accept the VARIABLE NAME (not value) of
#   the caller's local.  Reads use ${!VAR}; writes use printf -v "$VAR".
#   Internal locals use collision-proof prefixes (_cmh_/_cew_/_cxw_) so an
#   indirect read like ${!_cmh_last_hb_var} never resolves to a helper local.
#   This idiom mirrors the existing codebase pattern in cpu-admit.sh (_ca_*
#   caller-contract, _cpu_admit_psi_should_pass reading caller locals).
#
# EMPTY-REASON CONVENTION:
#   All three helpers silently return 0 when REASON is empty.  This preserves
#   the uncontended / 3-arg-caller fast path where clock-stop markers are not
#   needed (e.g. cargo-test-occt-gated.sh calling slot_acquire without REASON).
#   For clock_enter_wait the WAITED_VAR is still set to 1 on every call,
#   regardless of REASON, so the caller's state machine stays correct.
# ===========================================================================

# ---------------------------------------------------------------------------
# clock_maybe_heartbeat REASON START_TS LAST_HB_VAR
#   Throttled HEARTBEAT emission.  Call once per poll-loop iteration, AFTER
#   the sleep, so a wedged loop stops heartbeating (PRD D4 liveness contract).
#
#   REASON       — clock-stop reason token; empty ⇒ silent no-op, returns 0.
#   START_TS     — epoch second when the wait began (recorded at STOP entry).
#   LAST_HB_VAR  — NAME of caller's variable holding the epoch of the last
#                  HEARTBEAT emission (or the STOP entry time used as seed).
#                  Updated in place via printf -v when a heartbeat fires.
#
#   Emits a HEARTBEAT marker iff:
#     REASON is non-empty  AND
#     (now - ${!LAST_HB_VAR}) >= REIFY_CLOCK_HEARTBEAT_SECS (default 30).
#   Non-integer/empty REIFY_CLOCK_HEARTBEAT_SECS is clamped to 30 so the
#   `-ge` comparison never prints "integer expression expected".
# ---------------------------------------------------------------------------
clock_maybe_heartbeat() {
    local _cmh_reason="$1"
    local _cmh_start_ts="$2"
    local _cmh_last_hb_var="$3"
    [ -n "$_cmh_reason" ] || return 0
    local _cmh_interval="${REIFY_CLOCK_HEARTBEAT_SECS:-30}"
    [ "$_cmh_interval" -ge 1 ] 2>/dev/null || _cmh_interval=30
    local _cmh_now
    _cmh_now=$(date +%s)
    if [ $(( _cmh_now - ${!_cmh_last_hb_var} )) -ge "$_cmh_interval" ]; then
        clock_emit_heartbeat "$_cmh_reason" "$(( _cmh_now - _cmh_start_ts ))"
        printf -v "$_cmh_last_hb_var" '%s' "$_cmh_now"
    fi
}

# ---------------------------------------------------------------------------
# clock_enter_wait REASON WAITED_VAR LAST_HB_VAR
#   STOP-once bookkeeping.  Call BEFORE sleeping, on every poll-loop iteration
#   where all admission checks fail (entering / continuing the wait).
#
#   REASON       — clock-stop reason token.  When non-empty AND WAITED_VAR==0,
#                  emits @@REIFY_CLOCK_STOP@@ and seeds LAST_HB_VAR to now.
#                  When empty, STOP is suppressed but WAITED_VAR is still set.
#   WAITED_VAR   — NAME of caller's waited flag (0=not yet entered, 1=waiting).
#                  Set to 1 unconditionally on every call (idempotent).
#   LAST_HB_VAR  — NAME of caller's last-heartbeat timestamp variable.
#                  Updated to $(date +%s) only when STOP fires (first entry).
# ---------------------------------------------------------------------------
clock_enter_wait() {
    local _cew_reason="$1"
    local _cew_waited_var="$2"
    local _cew_last_hb_var="$3"
    if [ "${!_cew_waited_var}" -eq 0 ] && [ -n "$_cew_reason" ]; then
        clock_emit_stop "$_cew_reason"
        printf -v "$_cew_last_hb_var" '%s' "$(date +%s)"
    fi
    printf -v "$_cew_waited_var" '%s' 1
}

# ---------------------------------------------------------------------------
# clock_exit_wait REASON WAITED ELAPSED
#   START-iff-waited bookkeeping.  Call AFTER a successful acquire, before
#   returning to the caller.
#
#   REASON   — clock-stop reason token; empty ⇒ silent no-op.
#   WAITED   — VALUE of caller's waited flag (0 or 1).  Pass the value
#              directly (not the variable name); the caller already knows it.
#   ELAPSED  — Total seconds spent in the wait, computed by the caller
#              (e.g. $(( $(date +%s) - _start )) or SLOT_ACQUIRE_ELAPSED).
#              Passing as a value lets slot_acquire reuse its already-computed
#              SLOT_ACQUIRE_ELAPSED byte-for-byte (no extra date fork).
#
#   Emits @@REIFY_CLOCK_START@@ iff WAITED==1 AND REASON is non-empty.
#   (STOP and START are balanced: both fire on real contention; neither fires
#   on an uncontended fast-path acquire where WAITED remains 0.)
# ---------------------------------------------------------------------------
clock_exit_wait() {
    local _cxw_reason="$1"
    local _cxw_waited="$2"
    local _cxw_elapsed="$3"
    if [ "$_cxw_waited" -eq 1 ] && [ -n "$_cxw_reason" ]; then
        clock_emit_start "$_cxw_reason" "$_cxw_elapsed"
    fi
}
