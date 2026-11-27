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
# <duration_s> 0 means no time limit: each worker burns until the fixture is
# stopped with TERM, so an observer of the burn is never racing a clock. With 0,
# every process of the burn carries a parent-death signal (TERM), so the burn
# cannot outlive its launcher, SIGKILL included. A parent-death signal only fires
# for a death AFTER it is armed, so each process, once armed, checks that its
# parent is still the one that started it and refuses to burn when it is not.
# The fixture's check compares against the parent its shell saw at startup: a
# launcher already gone by then is not detected. That needs setpriv(1); without
# it duration 0 exits 69, because an unbounded burn is never run unanchored.
#
# OPTIONS:
#   --label NAME          label for progress output (optional, default "fixture")
#   --emit-cgroup FILE    write own /proc/self/cgroup rel path to FILE on startup
#                         (lets the harness locate the governing scope by reading this
#                         file after the fixture is placed in a cgroup scope by
#                         cpu-governed-exec.sh — mirrors the D1 PROBE pattern)
#   --print-usage         print "USAGE_USEC=<cpu.stat usage_usec>" to stdout before
#                         exit (for Row 4 slice-share measurements)
#   --pin-cpus LIST       pin worker i to CPU LIST[i mod n] with taskset(1). LIST is
#                         comma-separated CPU ids, digits only ("30,31"); a range,
#                         an empty entry or a non-digit exits 64, and a missing
#                         taskset exits 69 — pinning is never silently dropped.
#                         With the same worker count and LIST for two fixtures,
#                         every listed CPU hosts equally many workers of each.
#   --ready-file FILE     once every worker is forked — and, with --pin-cpus, has
#                         applied its pin — write the worker PIDs, one per line,
#                         to FILE atomically (tmp + mv). The post-fork
#                         counterpart of --emit-cgroup, which is written BEFORE
#                         the fork and so proves only that the fixture STARTED.
#
# NOTES:
#   - Dependency-free: uses only POSIX sh builtins + timeout(1); --pin-cpus
#     additionally needs taskset(1) and awk(1), --ready-file needs mv(1), and
#     duration 0 needs setpriv(1).
#   - Safe under set -euo pipefail.
#   - Workers are backgrounded and waited on with `wait`; EXIT trap reaps any
#     survivors via a per-PID `kill` over the recorded worker PIDs (harmless
#     if already exited). TERM, INT and HUP reap them the same way before the
#     fixture exits, a repeated signal included: it exits 128+n only once its
#     workers are reaped. No process-group kill: callers may background this
#     fixture from their own process group, and a group-wide kill would
#     signal the caller too.
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
    echo "Usage: $(basename "$0") <nworkers> <duration_s> [--label NAME] [--emit-cgroup FILE] [--print-usage] [--pin-cpus LIST] [--ready-file FILE]" >&2
    exit 64
fi

_FIXTURE_ARGV=("$@")
NWORKERS="$1"
DURATION_S="$2"
shift 2

LABEL="fixture"
EMIT_CGROUP_FILE=""
PRINT_USAGE=0
PIN_CPUS=""
READY_FILE=""

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
        --pin-cpus)
            shift
            PIN_CPUS="${1:-}"
            case "$PIN_CPUS" in
                ''|*[!0-9,]*|,*|*,|*,,*)
                    echo "$(basename "$0"): --pin-cpus requires comma-separated CPU ids, digits only (got '$PIN_CPUS')" >&2
                    exit 64
                    ;;
            esac
            shift
            ;;
        --ready-file)
            shift
            READY_FILE="${1:-}"
            if [ -z "$READY_FILE" ]; then
                echo "$(basename "$0"): --ready-file requires a FILE argument" >&2
                exit 64
            fi
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
        echo "$(basename "$0"): duration_s must be a non-negative integer (got '$DURATION_S')" >&2
        exit 64
        ;;
esac

# --pin-cpus: the list was validated above; refuse to run unpinned if taskset is
# missing, then split it once for the round-robin below.
_PIN_LIST=()
if [ -n "$PIN_CPUS" ]; then
    if ! command -v taskset >/dev/null 2>&1; then
        echo "$(basename "$0"): --pin-cpus needs taskset(1), which is not on PATH" >&2
        exit 69
    fi
    IFS=',' read -r -a _PIN_LIST <<< "$PIN_CPUS"
fi

# duration 0 is a burn with no clock, so only a parent-death signal can end it
# when its launcher dies: refuse to run one without setpriv(1) to set it.
if [ "$DURATION_S" -eq 0 ] && ! command -v setpriv >/dev/null 2>&1; then
    echo "$(basename "$0"): duration 0 needs setpriv(1) for its parent-death anchor, which is not on PATH" >&2
    exit 69
fi

# Self-anchor: re-exec once under a parent-death signal, so the launcher's death
# (SIGKILL included, which runs no trap) TERMs this fixture and its TERM trap
# (_on_signal) reaps the workers.  exec keeps the pid, so the caller's `$!` stays valid.  The
# marker is compared with `$$`: one inherited from another process never matches.
# The launcher may die before the signal is armed, which no signal reports, so
# the anchored pass compares its parent (read at its startup, after the arming)
# with the one the first pass saw, and exits 75 without burning on a mismatch.
if [ "$DURATION_S" -eq 0 ]; then
    if [ "${CPU_LOAD_FIXTURE_ANCHOR_PID:-}" != "$$" ]; then
        CPU_LOAD_FIXTURE_ANCHOR_PID=$$ CPU_LOAD_FIXTURE_ANCHOR_PPID=$PPID \
            exec setpriv --pdeathsig TERM -- "$BASH" "$0" "${_FIXTURE_ARGV[@]}"
    fi
    if [ "$PPID" != "${CPU_LOAD_FIXTURE_ANCHOR_PPID:-}" ]; then
        echo "$(basename "$0"): launcher ${CPU_LOAD_FIXTURE_ANCHOR_PPID:-?} died before the parent-death anchor was armed; not burning" >&2
        exit 75
    fi
fi

# ---------------------------------------------------------------------------
# EXIT trap: reap surviving workers.
# Per-PID kill over recorded _WORKER_PIDS only (best-effort; ignores errors
# if a worker already exited), followed by `wait`. Deliberately NOT a
# process-group kill (no `kill 0` / `kill -- -$$`): callers may background
# this fixture from their own process group (e.g. `fixture.sh ... &`), and a
# group-wide kill would signal the calling shell too.
# ---------------------------------------------------------------------------
_WORKER_PIDS=""
_PINNED_WORKERS=""  # "<pid>:<cpu>" pairs, one per worker, only with --pin-cpus

_cleanup() {
    # Kill all recorded worker PIDs (best-effort; ignore errors if already gone).
    if [ -n "$_WORKER_PIDS" ]; then
        for _pid in $_WORKER_PIDS; do
            kill "$_pid" 2>/dev/null || true
        done
        wait 2>/dev/null || true
        _WORKER_PIDS=""
    fi
}

# _on_signal <exit_status>: reap the workers, then exit.  Further terminating
# signals are ignored first, so a repeat can never cut the nested `wait` short,
# whichever way a given bash interleaves a trap with `wait`.  _cleanup empties
# _WORKER_PIDS, so the EXIT trap that `exit` fires is then a no-op.
_on_signal() {
    trap '' TERM INT HUP
    _cleanup
    exit "$1"
}

# With an EXIT trap alone, bash runs the trap from its own termination handler,
# and a second TERM then ends the shell on the spot, abandoning the reap.  GNU
# timeout signals its child and then the child's process group, so a fixture run
# under it is TERMed twice.  An explicit trap per signal runs as an ordinary
# trap instead.  Guarded by the FIXTURE-RESIGNAL block of
# tests/infra/test_cpu_load_governance.sh.
trap '_cleanup' EXIT
trap '_on_signal 143' TERM
trap '_on_signal 130' INT
trap '_on_signal 129' HUP

# _await_pins_applied: with --pin-cpus, block until every live pinned worker
# reports its own CPU as its whole affinity mask.  It is the forked child that
# runs taskset, so the pin lands AFTER the fork returns here; until then a worker
# still reports the mask it inherited, and a ready report made in that window
# would list workers that are not yet pinned.  A worker that is gone (taskset
# refused the CPU, or the burn ended) is not waited for — its absence is the
# reader's to report.  No deadline: a worker either applies its pin or exits, so
# the wait ends on its own (and a burn length would mean nothing at duration 0);
# the caller's readiness poll is the anti-hang.  A no-op without --pin-cpus.
_await_pins_applied() {
    local pair pid want got
    for pair in $_PINNED_WORKERS; do
        pid="${pair%%:*}"
        want="${pair##*:}"
        while :; do
            got="$(awk '/^Cpus_allowed_list:/ { print $2 }' "/proc/${pid}/status" 2>/dev/null || true)"
            if [ -z "$got" ] || [ "$got" = "$want" ]; then
                break
            fi
            sleep 0.01
        done
    done
}

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
# With --pin-cpus, worker i is launched under `taskset -c <LIST[i mod n]>`;
# with duration 0, under `setpriv --pdeathsig TERM` as well, followed by a check
# that this fixture is still the worker's parent once the signal is armed (a
# fixture killed before the arming sends no signal).  All of them exec, so $! is
# still the worker handle (timeout) that _cleanup reaps, the spinner is still its
# child and inherits the single-CPU mask.  GNU timeout treats 0 as no limit.
# ---------------------------------------------------------------------------
_LIFETIME_PREFIX=()
if [ "$DURATION_S" -eq 0 ]; then
    _LIFETIME_PREFIX=(setpriv --pdeathsig TERM
        sh -c '[ "$PPID" = "$1" ] || exit 75; shift; exec "$@"' _ "$$")
fi
_i=0
while [ "$_i" -lt "$NWORKERS" ]; do
    _pin_prefix=()
    if [ -n "$PIN_CPUS" ]; then
        _cpu="$(( 10#${_PIN_LIST[$(( _i % ${#_PIN_LIST[@]} ))]} ))"
        _pin_prefix=(taskset -c "$_cpu")
    fi
    # The subshell exits automatically when timeout terminates it.
    "${_LIFETIME_PREFIX[@]}" "${_pin_prefix[@]}" timeout "$DURATION_S" sh -c 'while true; do :; done' &
    _pid=$!
    _WORKER_PIDS="${_WORKER_PIDS}${_WORKER_PIDS:+ }${_pid}"
    if [ -n "$PIN_CPUS" ]; then
        _PINNED_WORKERS="${_PINNED_WORKERS}${_PINNED_WORKERS:+ }${_pid}:${_cpu}"
    fi
    _i=$(( _i + 1 ))
done

# ---------------------------------------------------------------------------
# --ready-file: report the worker PIDs once every worker is up — and, with
# --pin-cpus, pinned.  Written to a temp file in the same directory and renamed
# onto FILE, so a reader polling for FILE never observes a partial list.
# ---------------------------------------------------------------------------
if [ -n "$READY_FILE" ]; then
    _await_pins_applied
    for _pid in $_WORKER_PIDS; do
        printf '%s\n' "$_pid"
    done > "${READY_FILE}.tmp.$$"
    mv -f "${READY_FILE}.tmp.$$" "$READY_FILE"
fi

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
