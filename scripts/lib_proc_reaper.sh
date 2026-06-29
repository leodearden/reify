#!/usr/bin/env bash
# scripts/lib_proc_reaper.sh — Process-group teardown + host-wide orphan reaper.
#
# Designed to be sourced by callers (e.g. verify.sh) that need to run cargo
# test/compile passes in dedicated process groups AND directly executable for
# the host-wide orphan sweep (the main-guard at the end enables direct exec).
#
# FUNCTIONS (defined when sourced):
#   reaper_kill_pgroup <pgid>
#       Send SIGTERM to process group <pgid>, wait GRACE_SECS, escalate to
#       SIGKILL.  ESRCH-safe: a stale/nonexistent PGID returns 0.
#
#   reaper_run_in_pgroup <cmd-string>
#       Run <cmd-string> in its own process group (set -m; eval & ; $!-as-PGID).
#       Tracks the PGID in _REIFY_REAP_PGIDS for reaper_teardown.
#       Propagates the command's exit code (set -e safe).
#
#   reaper_teardown
#       Kill all PGIDs tracked by reaper_run_in_pgroup (TERM->grace->KILL).
#       Idempotent: no-op when no PGIDs are tracked; safe to call multiple times.
#
# KNOBS (environment variables):
#   REIFY_REAPER_GRACE_SECS    seconds between SIGTERM and SIGKILL (default 10)
#   REIFY_PROC_REAPER_DISABLE  set to 1 to disable teardown (break-glass)
#
# KNOBS for reap-orphans subcommand:
#   REIFY_REAPER_DEPS_GLOB        glob for candidate exe paths
#                                 (default: */target/debug/deps/* */target/release/deps/*)
#   REIFY_REAPER_MIN_AGE_SECS     minimum process age in seconds (default 7200)
#   REIFY_REAPER_ORPHAN_PPIDS     space-separated PPIDs considered orphan parents
#                                 (default: 1)
#   REIFY_REAPER_COMMS            space-separated comm names of orphan-parent procs
#                                 (default: systemd init)
#   REIFY_REAPER_UID              UID to filter by (default: $(id -u))
#   REIFY_REAPER_PS_TIMEOUT       max seconds for the host-wide ps scan (default 15)
#                                 On timeout: emits a WARNING and skips the sweep cycle.
#   REIFY_REAPER_PS_PPID_TIMEOUT  max seconds for per-candidate parent-comm ps (default 5)
#
# Direct-exec usage (standalone sweep):
#   REIFY_REAPER_DEPS_GLOB=... REIFY_REAPER_MIN_AGE_SECS=0 \
#     ./scripts/lib_proc_reaper.sh reap-orphans [--dry-run]

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_PROC_REAPER_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_PROC_REAPER_SH_SOURCED=1

# Array tracking PGIDs of in-flight reaper_run_in_pgroup passes.
_REIFY_REAP_PGIDS=()

# ---------------------------------------------------------------------------
# reaper_kill_pgroup <pgid>
#   TERM -> sleep GRACE -> KILL the entire process group.
#   All kill errors are swallowed (ESRCH-safe) so a stale PGID returns 0.
# ---------------------------------------------------------------------------
reaper_kill_pgroup() {
    local _pgid="$1"
    local _grace="${REIFY_REAPER_GRACE_SECS:-10}"
    kill -TERM -- -${_pgid} 2>/dev/null || true
    sleep "${_grace}" 2>/dev/null || true
    kill -KILL -- -${_pgid} 2>/dev/null || true
    return 0
}

# ---------------------------------------------------------------------------
# reaper_run_in_pgroup <cmd-string>
#   Run cmd-string in its own process group; track the PGID for teardown.
#   Uses the set -m + background + $!-as-PGID idiom from lib_portable.sh
#   portable_timeout(), inheriting its proven exit-code propagation and
#   PID-reuse-safe PGID tracking.
# ---------------------------------------------------------------------------
reaper_run_in_pgroup() {
    local _cmd="$1"
    local _rc=0

    if [ "${REIFY_PROC_REAPER_DISABLE:-}" = "1" ]; then
        eval "$_cmd" || _rc=$?
        return "$_rc"
    fi

    # Save and restore caller's monitor-mode state (mirrors portable_timeout).
    local _had_monitor=0
    case $- in *m*) _had_monitor=1 ;; esac

    set -m 2>/dev/null || true
    local _monitor_active=0
    case $- in *m*) _monitor_active=1 ;; esac
    if [ "$_monitor_active" -eq 0 ]; then
        # Monitor mode unavailable: the child shares the caller's process group.
        # reaper_teardown's kill -- -<pgid> will target a foreign group, so
        # teardown of this pass will be a no-op rather than a full group kill.
        echo "lib_proc_reaper.sh: WARNING: set -m unavailable; pgroup teardown degraded for: ${_cmd:0:80}" >&2
    fi
    eval "$_cmd" &
    local _pid=$!
    if [ "$_had_monitor" -eq 0 ]; then set +m 2>/dev/null || true; fi

    # Track the PGID (== _pid under monitor mode) for teardown.
    _REIFY_REAP_PGIDS+=("$_pid")

    # Wait for the command; propagate exit code under set -e.
    wait "$_pid" 2>/dev/null || _rc=$?

    # Remove from tracking array (command completed; PID-reuse-safe).
    local _new=()
    local _p
    for _p in "${_REIFY_REAP_PGIDS[@]+${_REIFY_REAP_PGIDS[@]}}"; do
        [ "$_p" = "$_pid" ] || _new+=("$_p")
    done
    _REIFY_REAP_PGIDS=("${_new[@]+${_new[@]}}")

    return "$_rc"
}

# ---------------------------------------------------------------------------
# reaper_teardown
#   Kill all tracked PGIDs (TERM->grace->KILL). Idempotent.
# ---------------------------------------------------------------------------
reaper_teardown() {
    if [ "${REIFY_PROC_REAPER_DISABLE:-}" = "1" ]; then
        return 0
    fi
    local _p
    for _p in "${_REIFY_REAP_PGIDS[@]+${_REIFY_REAP_PGIDS[@]}}"; do
        reaper_kill_pgroup "$_p" || true
    done
    _REIFY_REAP_PGIDS=()
}

# ---------------------------------------------------------------------------
# _reaper_bounded_ps <secs> <ps-args...>
#   Run `ps <ps-args>` under a wall-clock bound of <secs> seconds using GNU
#   `timeout` (when available on PATH); falls back to bare `ps` for portability.
#   Exit code is propagated verbatim — callers may inspect $? for 124 (timeout).
#   Mirrors the `command -v timeout` guard pattern in lib_portable.sh.
# ---------------------------------------------------------------------------
_reaper_bounded_ps() {
    local _secs="$1"
    shift
    if command -v timeout >/dev/null 2>&1; then
        timeout "$_secs" ps "$@"
    else
        ps "$@"
    fi
}

# ---------------------------------------------------------------------------
# _reaper_reap_orphans [--dry-run]
#   Scan running processes owned by REIFY_REAPER_UID whose resolved exe
#   (/proc/<pid>/exe) matches REIFY_REAPER_DEPS_GLOB, whose PPID is in
#   REIFY_REAPER_ORPHAN_PPIDS or whose parent comm is in REIFY_REAPER_COMMS,
#   and whose age exceeds REIFY_REAPER_MIN_AGE_SECS.  SIGKILL candidates
#   unless --dry-run.
# ---------------------------------------------------------------------------
_reaper_reap_orphans() {
    local _dry_run=0
    [ "${1:-}" = "--dry-run" ] && _dry_run=1

    local _deps_glob="${REIFY_REAPER_DEPS_GLOB:-*/target/debug/deps/* */target/release/deps/*}"
    local _min_age="${REIFY_REAPER_MIN_AGE_SECS:-7200}"
    local _orphan_ppids="${REIFY_REAPER_ORPHAN_PPIDS:-1}"
    local _reaper_comms="${REIFY_REAPER_COMMS:-systemd init}"
    local _uid="${REIFY_REAPER_UID:-$(id -u)}"

    # Scan all PIDs owned by the target UID in a single ps pass (pid, ppid, etimes).
    # Single invocation is far faster than N×2 per-PID ps calls on busy hosts.
    # Bounded by REIFY_REAPER_PS_TIMEOUT (default 15s): captures output to a var so
    # timeout exit code 124 is observable; on timeout, emits a WARNING and skips
    # this sweep cycle (non-silent, returns promptly — esc-4889-94 fix).
    local _ps_out _ps_rc
    _ps_rc=0
    _ps_out="$(_reaper_bounded_ps "${REIFY_REAPER_PS_TIMEOUT:-15}" \
        -u "$_uid" -o pid=,ppid=,etimes= 2>/dev/null)" || _ps_rc=$?
    if [ "${_ps_rc}" -eq 124 ] 2>/dev/null; then
        echo "lib_proc_reaper.sh: WARNING: host-wide ps scan exceeded ${REIFY_REAPER_PS_TIMEOUT:-15}s under load; skipping orphan sweep this cycle" >&2
        return 0  # deterministic skip: discard any partial ps output captured before SIGKILL
    fi
    local _pid _ppid _etimes _exe _ppid_comm _glob _matched
    while read -r _pid _ppid _etimes; do
        _pid="${_pid# }"; _pid="${_pid% }"
        _ppid="${_ppid# }"; _ppid="${_ppid% }"
        _etimes="${_etimes# }"; _etimes="${_etimes% }"
        [ -n "$_pid" ] && [ -n "$_ppid" ] && [ -n "$_etimes" ] || continue

        # Age filter first (cheap) — skip before any further work.
        [ "$_etimes" -ge "$_min_age" ] 2>/dev/null || continue

        # Resolve exe via /proc/<pid>/exe (spoof-proof, works with copied binaries).
        _exe=$(readlink "/proc/${_pid}/exe" 2>/dev/null || echo "")
        [ -n "$_exe" ] || continue

        # Deps-glob filter: check against each glob pattern.
        _matched=0
        for _glob in $_deps_glob; do
            # shellcheck disable=SC2254
            case "$_exe" in
                $_glob) _matched=1; break ;;
            esac
        done
        [ "$_matched" -eq 1 ] || continue

        # PPID filter: PPID must be in orphan set OR parent comm must be in comms set.
        _ppid_comm=$(_reaper_bounded_ps "${REIFY_REAPER_PS_PPID_TIMEOUT:-5}" -o comm= -p "$_ppid" 2>/dev/null | tr -d ' ' || echo "")
        _matched=0
        local _opid _comm
        for _opid in $_orphan_ppids; do
            [ "$_ppid" = "$_opid" ] && { _matched=1; break; }
        done
        if [ "$_matched" -eq 0 ]; then
            for _comm in $_reaper_comms; do
                [ "$_ppid_comm" = "$_comm" ] && { _matched=1; break; }
            done
        fi
        [ "$_matched" -eq 1 ] || continue

        # Candidate found.
        if [ "$_dry_run" -eq 1 ]; then
            echo "reap-orphans [dry-run]: pid=$_pid exe=$_exe age=${_etimes}s ppid=$_ppid" >&2
        else
            echo "reap-orphans: killing pid=$_pid exe=$_exe age=${_etimes}s ppid=$_ppid" >&2
            kill -9 "$_pid" 2>/dev/null || true
        fi
    done <<< "$_ps_out"
}

# ---------------------------------------------------------------------------
# Main guard — direct execution dispatches reap-orphans subcommand.
# ---------------------------------------------------------------------------
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    case "${1:-}" in
        reap-orphans)
            shift
            _reaper_reap_orphans "$@"
            ;;
        *)
            echo "Usage: $(basename "$0") reap-orphans [--dry-run]" >&2
            exit 1
            ;;
    esac
fi
