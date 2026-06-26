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
