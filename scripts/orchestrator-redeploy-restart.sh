#!/usr/bin/env bash
# scripts/orchestrator-redeploy-restart.sh
# Safe detached orchestrator redeploy+restart mechanism (task 4620).
#
# The orchestrator loads orchestrator.yaml ONCE at startup and refuses to
# start with uncommitted tracked changes in project_root (dirty-start guard).
# A task UNDER the orchestrator cannot do `systemctl restart` — that
# SIGTERM-kills its own agent mid-run.
#
# This script provides TWO modes:
#
#   SCHEDULE MODE (default):
#     Checks project_root is clean. If dirty, exits non-zero with an
#     actionable "commit/land first" message — schedules NOTHING.
#     If clean, best-effort pre-cleans any stale transient unit, then
#     schedules the restart as a DETACHED transient unit via:
#       systemd-run --user --on-active=<delay> --unit=<tu> --collect \
#         --setenv=ORCH_UNIT=… --setenv=ORCH_PROJECT_ROOT=… \
#         <abs-path-to-self> --exec-restart
#     The transient unit is a child of the USER systemd manager (not the
#     orchestrator), so it fires AFTER the triggering agent has exited.
#
#   EXEC MODE (--exec-restart):
#     Run by the transient unit at fire time. Re-checks project_root.
#     If clean → blocking `systemctl --user stop <unit>` THEN
#       `systemctl --user start <unit>` (NEVER `systemctl restart` —
#       the 90s graceful-stop window cancels restart's start-half).
#     If dirty → leave the old orchestrator RUNNING, log loudly, exit 0.
#
# Config (env vars with defaults):
#   ORCH_UNIT            — systemd unit name (default: orchestrator-reify.service)
#   ORCH_PROJECT_ROOT    — main checkout to guard (default: /home/leo/src/reify)
#   ORCH_RESTART_DELAY   — on-active delay for systemd-run (default: 60s)
#   ORCH_TRANSIENT_UNIT  — transient unit name (default: orch-redeploy-restart)
#
# Usage:
#   scripts/orchestrator-redeploy-restart.sh [--help]
#   scripts/orchestrator-redeploy-restart.sh --exec-restart
#
# See also: docs in CLAUDE.md §"Deploying the orchestrator (config/code changes)"

set -uo pipefail

# ── Config ────────────────────────────────────────────────────────────────────
ORCH_UNIT="${ORCH_UNIT:-orchestrator-reify.service}"
ORCH_PROJECT_ROOT="${ORCH_PROJECT_ROOT:-/home/leo/src/reify}"
ORCH_RESTART_DELAY="${ORCH_RESTART_DELAY:-60s}"
ORCH_TRANSIENT_UNIT="${ORCH_TRANSIENT_UNIT:-orch-redeploy-restart}"

# ── Helpers ───────────────────────────────────────────────────────────────────
usage() {
    cat >&2 <<'USAGE'
Usage: scripts/orchestrator-redeploy-restart.sh [--help | --exec-restart]

Modes:
  (default)      Schedule mode: check project_root is clean, then schedule a
                 detached transient unit to stop+start the orchestrator after
                 ORCH_RESTART_DELAY. Exits non-zero if project_root is dirty.
  --exec-restart Exec mode (run by the transient unit at fire time): re-check
                 clean, then blocking stop then start. If dirty, leave the
                 orchestrator running and exit 0.
  --help         Show this usage and exit 0.

Environment knobs (all have defaults):
  ORCH_UNIT             Orchestrator systemd unit (default: orchestrator-reify.service)
  ORCH_PROJECT_ROOT     Main checkout to guard   (default: /home/leo/src/reify)
  ORCH_RESTART_DELAY    on-active delay           (default: 60s)
  ORCH_TRANSIENT_UNIT   Transient unit name       (default: orch-redeploy-restart)

IMPORTANT: project_root must be clean (no uncommitted tracked changes) before
scheduling. If it is dirty, commit/land your changes first, then re-run.
USAGE
}

# is_clean ROOT
#   Returns 0 if the tree has no uncommitted tracked changes (clean).
#   Returns 1 if there are uncommitted tracked changes (dirty).
#   Returns 2 (and prints an error to stderr) if git itself fails —
#   a bad path, non-repo, or any other git error.
#
#   A git failure MUST NOT be treated as "clean": a misconfigured
#   ORCH_PROJECT_ROOT would otherwise silently pass the clean-guard and
#   schedule/exec a restart against an unverified tree, potentially
#   triggering the crash-loop outage this script exists to prevent.
#
# (--untracked-files=no mirrors the orchestrator's dirty-start-guard semantics)
is_clean() {
    local root="$1"
    local status_out git_rc=0
    status_out="$(git -C "$root" status --porcelain --untracked-files=no 2>/dev/null)" || git_rc=$?
    if [ "$git_rc" -ne 0 ]; then
        echo "orchestrator-redeploy-restart.sh: ERROR — git status failed for project_root: $root" >&2
        echo "  Ensure ORCH_PROJECT_ROOT exists and is a git repository." >&2
        return 2
    fi
    [ -z "$status_out" ]
}

# ── Arg parsing ───────────────────────────────────────────────────────────────
MODE="schedule"
for arg in "$@"; do
    case "$arg" in
        --help|-h)
            usage
            exit 0
            ;;
        --exec-restart)
            MODE="exec"
            ;;
        *)
            echo "orchestrator-redeploy-restart.sh: ERROR — unknown argument: $arg" >&2
            usage
            exit 1
            ;;
    esac
done

# ── Schedule mode ─────────────────────────────────────────────────────────────
if [ "$MODE" = "schedule" ]; then
    # Preflight: check project_root is clean before scheduling anything.
    # Three outcomes: 0=clean (proceed), 1=dirty (refuse), 2=git error (abort).
    clean_rc=0
    is_clean "$ORCH_PROJECT_ROOT" || clean_rc=$?
    if [ "$clean_rc" -eq 2 ]; then
        # Error already printed by is_clean (git failed / path not a repo).
        exit 1
    fi
    if [ "$clean_rc" -ne 0 ]; then
        echo "orchestrator-redeploy-restart.sh: ERROR — project_root is dirty." >&2
        echo "  Uncommitted tracked changes detected in: $ORCH_PROJECT_ROOT" >&2
        echo "  The orchestrator's dirty-start guard will refuse to restart with" >&2
        echo "  uncommitted changes, causing a crash-loop (StartLimitBurst=10" >&2
        echo "  then stays DOWN)." >&2
        echo "" >&2
        echo "  FIX: commit/land your changes first, then re-run this script:" >&2
        echo "    git -C '$ORCH_PROJECT_ROOT' status --short --untracked-files=no" >&2
        echo "    # commit or land the changes above, then:" >&2
        echo "    scripts/orchestrator-redeploy-restart.sh" >&2
        exit 1
    fi

    # Resolve self as absolute path so the transient unit can re-invoke us
    SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

    # Best-effort pre-clean stale transient unit (errors ignored — idempotency)
    systemctl --user stop "${ORCH_TRANSIENT_UNIT}.service" 2>/dev/null || true
    systemctl --user stop "${ORCH_TRANSIENT_UNIT}.timer"   2>/dev/null || true
    systemctl --user reset-failed "${ORCH_TRANSIENT_UNIT}.service" 2>/dev/null || true
    systemctl --user reset-failed "${ORCH_TRANSIENT_UNIT}.timer"   2>/dev/null || true

    # Schedule the detached restart as a transient user unit.
    # Check systemd-run's exit code: if scheduling fails (e.g. a stale unit the
    # pre-clean couldn't clear, or systemd --user unavailable), emit an error and
    # exit non-zero rather than printing a false "scheduled" confirmation.
    if ! systemd-run \
            --user \
            --on-active="$ORCH_RESTART_DELAY" \
            --unit="$ORCH_TRANSIENT_UNIT" \
            --collect \
            --setenv="ORCH_UNIT=$ORCH_UNIT" \
            --setenv="ORCH_PROJECT_ROOT=$ORCH_PROJECT_ROOT" \
            "$SELF" --exec-restart; then
        echo "orchestrator-redeploy-restart.sh: ERROR — systemd-run failed to schedule restart of '$ORCH_UNIT'." >&2
        echo "  Check that systemd --user is available and the transient unit is not already active:" >&2
        echo "    systemctl --user status '$ORCH_TRANSIENT_UNIT'" >&2
        exit 1
    fi

    echo "orchestrator-redeploy-restart.sh: scheduled restart of '$ORCH_UNIT'" >&2
    echo "  Transient unit: $ORCH_TRANSIENT_UNIT" >&2
    echo "  Fires in:       $ORCH_RESTART_DELAY (after the scheduling agent exits)" >&2
    echo "  project_root:   $ORCH_PROJECT_ROOT (clean at schedule time)" >&2
    exit 0
fi

# ── Exec mode (--exec-restart, run by the transient unit) ────────────────────
if [ "$MODE" = "exec" ]; then
    # Re-check clean at fire time (rare: main could have become dirty since schedule).
    # Three outcomes: 0=clean (proceed), 1=dirty (leave running), 2=git error (abort).
    clean_rc=0
    is_clean "$ORCH_PROJECT_ROOT" || clean_rc=$?
    if [ "$clean_rc" -eq 2 ]; then
        # Error already printed by is_clean (git failed / path not a repo).
        exit 1
    fi
    if [ "$clean_rc" -ne 0 ]; then
        echo "orchestrator-redeploy-restart.sh: WARNING — project_root is dirty at fire time." >&2
        echo "  project_root: $ORCH_PROJECT_ROOT" >&2
        echo "  Leaving orchestrator '$ORCH_UNIT' RUNNING to avoid a crash-loop." >&2
        echo "  (Starting dirty would crash-loop to StartLimitBurst=10 then stay DOWN.)" >&2
        echo "  Commit/land the changes and run the script again when clean." >&2
        exit 0
    fi

    echo "orchestrator-redeploy-restart.sh: stopping '$ORCH_UNIT' ..." >&2
    # Stop failure (e.g. unit already stopped after a crash) is non-fatal: warn
    # and continue so we still attempt the start.
    if ! systemctl --user stop "$ORCH_UNIT"; then
        echo "orchestrator-redeploy-restart.sh: WARNING — stop '$ORCH_UNIT' failed (unit may already be stopped)." >&2
    fi

    echo "orchestrator-redeploy-restart.sh: starting '$ORCH_UNIT' ..." >&2
    # Start failure is fatal: the orchestrator is now down.  Exit non-zero so the
    # caller (operator or monitoring) knows the restart did not succeed.
    if ! systemctl --user start "$ORCH_UNIT"; then
        echo "orchestrator-redeploy-restart.sh: ERROR — start '$ORCH_UNIT' failed." >&2
        echo "  The orchestrator may be down. Check: systemctl --user status '$ORCH_UNIT'" >&2
        exit 1
    fi

    echo "orchestrator-redeploy-restart.sh: '$ORCH_UNIT' restarted successfully." >&2
    exit 0
fi
