#!/usr/bin/env bash
# tests/infra/cpu_load_fixture.sh — synthetic CPU-burn load generator.
# Task 4634 (cpu-load-governance ε integration gate).
#
# Usage:
#   cpu_load_fixture.sh <nworkers> <duration_s> [OPTIONS]
#
# Forks <nworkers> busy-spin children, each bounded by an internal
# `timeout <duration_s>`, then waits for all children and exits 0.
# The busy-spin loop (:; done) runs tight enough to saturate one CPU core
# per worker on any POSIX shell.
#
# OPTIONS:
#   --label NAME          label for progress output (optional, default "fixture")
#   --emit-cgroup FILE    write own /proc/self/cgroup rel path to FILE on startup
#                         (lets the harness locate the governing scope by reading this
#                         file after the fixture is placed in a cgroup scope by
#                         cpu-governed-exec.sh — mirrors the D1 PROBE pattern)
#   --print-usage         print "USAGE_USEC=<cpu.stat usage_usec>" to stdout before
#                         exit (for Row 4 slice-share measurements)
#
# NOTES:
#   - Dependency-free: uses only POSIX sh builtins + timeout(1).
#   - Safe under set -euo pipefail.
#   - Workers are backgrounded and waited on with `wait`; EXIT trap reaps any
#     survivors via `kill 0` (process-group kill, harmless if already exited).
#   - --emit-cgroup writes BEFORE workers are forked so the scope is visible
#     as soon as the fixture starts; the harness reads the file after a short
#     settle to ensure scope placement is complete.
#   - --print-usage reads /sys/fs/cgroup<rel>/cpu.stat usage_usec AFTER all
#     workers finish (captures total CPU time consumed by this fixture run).

set -euo pipefail

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
if [ "$#" -lt 2 ]; then
    echo "Usage: $(basename "$0") <nworkers> <duration_s> [--label NAME] [--emit-cgroup FILE] [--print-usage]" >&2
    exit 64
fi

NWORKERS="$1"
DURATION_S="$2"
shift 2

LABEL="fixture"
EMIT_CGROUP_FILE=""
PRINT_USAGE=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --label)
            shift
            LABEL="${1:-fixture}"
            shift
            ;;
        --emit-cgroup)
            shift
            EMIT_CGROUP_FILE="${1:-}"
            if [ -z "$EMIT_CGROUP_FILE" ]; then
                echo "$(basename "$0"): --emit-cgroup requires a FILE argument" >&2
                exit 64
            fi
            shift
            ;;
        --print-usage)
            PRINT_USAGE=1
            shift
            ;;
        *)
            echo "$(basename "$0"): unknown option '$1'" >&2
            exit 64
            ;;
    esac
done

# Validate numeric args.
case "$NWORKERS" in
    ''|*[!0-9]*)
        echo "$(basename "$0"): nworkers must be a positive integer (got '$NWORKERS')" >&2
        exit 64
        ;;
esac
if [ "$NWORKERS" -lt 1 ]; then
    echo "$(basename "$0"): nworkers must be >= 1 (got '$NWORKERS')" >&2
    exit 64
fi
case "$DURATION_S" in
    ''|*[!0-9]*)
        echo "$(basename "$0"): duration_s must be a positive integer (got '$DURATION_S')" >&2
        exit 64
        ;;
esac
if [ "$DURATION_S" -lt 1 ]; then
    echo "$(basename "$0"): duration_s must be >= 1 (got '$DURATION_S')" >&2
    exit 64
fi

# ---------------------------------------------------------------------------
# EXIT trap: reap surviving workers.
# Uses kill -- -$$  (process-group kill) if the main process is the group
# leader, else tries a per-PID kill on recorded _WORKER_PIDS.
# ---------------------------------------------------------------------------
_WORKER_PIDS=""

_cleanup() {
    # Kill all recorded worker PIDs (best-effort; ignore errors if already gone).
    if [ -n "$_WORKER_PIDS" ]; then
        for _pid in $_WORKER_PIDS; do
            kill "$_pid" 2>/dev/null || true
        done
        wait 2>/dev/null || true
    fi
}
trap '_cleanup' EXIT

# ---------------------------------------------------------------------------
# --emit-cgroup: write own cgroup relative path BEFORE forking workers.
# The rel path strips the leading "0::" prefix from /proc/self/cgroup.
# ---------------------------------------------------------------------------
if [ -n "$EMIT_CGROUP_FILE" ]; then
    _CGROUP_REL="$(sed 's/^0:://' /proc/self/cgroup 2>/dev/null || echo "")"
    printf '%s\n' "$_CGROUP_REL" > "$EMIT_CGROUP_FILE"
fi

# ---------------------------------------------------------------------------
# Fork <nworkers> busy-spin workers.
# Each worker runs: timeout <duration_s> sh -c 'while true; do :; done'
# timeout(1) is used repo-wide (test_portable_timeout.sh); POSIX sh busy-spin.
# ---------------------------------------------------------------------------
_i=0
while [ "$_i" -lt "$NWORKERS" ]; do
    # The subshell exits automatically when timeout terminates it.
    timeout "$DURATION_S" sh -c 'while true; do :; done' &
    _pid=$!
    _WORKER_PIDS="${_WORKER_PIDS}${_WORKER_PIDS:+ }${_pid}"
    _i=$(( _i + 1 ))
done

# Wait for all workers to finish (timeout expires → each exits 124 or 143;
# both are acceptable — the shell busy-spin itself returns 0 on SIGTERM).
# We `wait` without inspecting per-worker exit codes; overall fixture success
# is that all workers terminated (not their individual exit codes).
wait 2>/dev/null || true
_WORKER_PIDS=""  # Already reaped; suppress EXIT trap double-kill.

# ---------------------------------------------------------------------------
# --print-usage: read cpu.stat usage_usec for this cgroup after workers done.
# ---------------------------------------------------------------------------
if [ "$PRINT_USAGE" -eq 1 ]; then
    _CGROUP_REL="$(sed 's/^0:://' /proc/self/cgroup 2>/dev/null || echo "")"
    _CPU_STAT="/sys/fs/cgroup${_CGROUP_REL}/cpu.stat"
    _USAGE_USEC=""
    if [ -r "$_CPU_STAT" ]; then
        _USAGE_USEC="$(awk '/^usage_usec/{print $2; exit}' "$_CPU_STAT" 2>/dev/null || echo "")"
    fi
    if [ -n "$_USAGE_USEC" ]; then
        echo "USAGE_USEC=${_USAGE_USEC}"
    else
        echo "USAGE_USEC=unavailable"
    fi
fi

exit 0
