#!/usr/bin/env bash
# Portable shell helpers for reify build scripts and infrastructure tests.
# Designed to be sourced, not executed directly.
#
# Usage:  source "$(dirname "${BASH_SOURCE[0]}")/lib_portable.sh"
#   or:   source "$REPO_ROOT/scripts/lib_portable.sh"

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_PORTABLE_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_PORTABLE_SH_SOURCED=1

# Portable SHA-256: prefer sha256sum (GNU coreutils), fall back to shasum (macOS).
portable_sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1"
    else
        echo "ERROR: neither sha256sum nor shasum found on PATH." >&2
        return 1
    fi
}

# Portable timeout: run a command with a wall-clock time limit.
#
# Usage: portable_timeout <seconds> <cmd> [args...]
#
# 3-tier strategy:
#   1. GNU timeout (Linux coreutils)
#   2. gtimeout (Homebrew coreutils on macOS)
#   3. background + sleep + kill fallback (POSIX-portable)
#
# Returns the command's exit code, or 124 if the command was killed
# due to exceeding the time limit (matches GNU timeout convention).
#
# Sets the global _PORTABLE_TIMEOUT_TIMED_OUT variable:
#   true  — the command was killed by the timeout mechanism
#   false — the command exited on its own (any exit code, including 124)
#
# Ambiguity note (GNU timeout / gtimeout paths):
#   When using GNU timeout or gtimeout, _PORTABLE_TIMEOUT_TIMED_OUT is set
#   to true whenever the exit code is 124.  However, exit 124 can also be
#   returned by the wrapped command itself (e.g. `bash -c 'exit 124'`).
#   There is no way to distinguish these two cases on the GNU path.
#   _PORTABLE_TIMEOUT_TIMED_OUT is only fully reliable on the POSIX
#   flag-file fallback path, where the flag is created by the timer process
#   and the command's exit code is additionally required to be 143 (SIGTERM).
portable_timeout() {
    local seconds="$1"
    shift

    _PORTABLE_TIMEOUT_TIMED_OUT=false
    local cmd_exit=0
    if command -v timeout >/dev/null 2>&1; then
        timeout "$seconds" "$@" || cmd_exit=$?
        # Ambiguity: exit 124 may mean the timeout fired OR the command
        # naturally exited 124.  See doc comment above for details.
        if [ "$cmd_exit" -eq 124 ]; then
            _PORTABLE_TIMEOUT_TIMED_OUT=true
        fi
    elif command -v gtimeout >/dev/null 2>&1; then
        gtimeout "$seconds" "$@" || cmd_exit=$?
        # Same exit-124 ambiguity as the GNU timeout branch above.
        if [ "$cmd_exit" -eq 124 ]; then
            _PORTABLE_TIMEOUT_TIMED_OUT=true
        fi
    else
        # POSIX fallback: background the command, sleep, kill if still running.
        # Use a temp file flag so we can distinguish the timer's kill from
        # a coincidental exit code 143 or 124.
        local timeout_flag
        timeout_flag=$(mktemp "${TMPDIR:-/tmp}/portable_timeout.XXXXXX" 2>/dev/null) || timeout_flag=""

        "$@" &
        local cmd_pid=$!

        # Save caller's monitor-mode state so we can restore it after temporarily
        # enabling job control (set -m) to place the timer subshell in its own
        # process group.  Without this save/restore, portable_timeout silently
        # disables the caller's job control when they had set -m active.
        local _pt_had_monitor=0
        case $- in *m*) _pt_had_monitor=1 ;; esac

        if [ -n "$timeout_flag" ]; then
            # Normal path: use flag file for precise timeout detection.
            rm -f "$timeout_flag"  # Remove so its presence signals timeout fired.
            # Trap INT/TERM so that if the caller's shell is interrupted during the
            # wait window the temp flag path is cleaned up.  Save and restore any
            # existing handler so we don't clobber the caller's traps.
            local _pt_old_int _pt_old_term
            _pt_old_int=$(trap -p INT 2>/dev/null || true)
            _pt_old_term=$(trap -p TERM 2>/dev/null || true)
            trap 'rm -f "$timeout_flag" 2>/dev/null' INT TERM
            # Enable job control temporarily so the timer subshell is placed in its
            # own process group, making 'kill -- -$timer_pid' safe and effective.
            set -m 2>/dev/null || true
            ( sleep "$seconds" && {
                touch "$timeout_flag" 2>/dev/null
                kill "$cmd_pid" 2>/dev/null
                sleep 2
                kill -0 "$cmd_pid" 2>/dev/null && kill -9 "$cmd_pid" 2>/dev/null || true
              } ) &
            [ "$_pt_had_monitor" -eq 0 ] && set +m 2>/dev/null || true
        else
            # Degraded path: mktemp failed, fall back to old 143-detection.
            echo "WARNING: mktemp failed, timeout detection degraded" >&2
            set -m 2>/dev/null || true
            ( sleep "$seconds" && {
                kill "$cmd_pid" 2>/dev/null
                sleep 2
                kill -0 "$cmd_pid" 2>/dev/null && kill -9 "$cmd_pid" 2>/dev/null || true
              } ) &
            [ "$_pt_had_monitor" -eq 0 ] && set +m 2>/dev/null || true
        fi
        local timer_pid=$!
        wait "$cmd_pid" 2>/dev/null || cmd_exit=$?
        # Clean up timer — if command finished before timeout, kill the timer subshell
        # and its inner sleep child.  Process-group kill ('kill -- -$pid') terminates
        # all children of the subshell atomically; fall back to plain kill if unsupported.
        kill -- -$timer_pid 2>/dev/null || kill "$timer_pid" 2>/dev/null || true
        wait "$timer_pid" 2>/dev/null || true

        if [ -n "$timeout_flag" ] && [ -f "$timeout_flag" ] && [ "$cmd_exit" -eq 143 ]; then
            # Timer fired and killed the process — genuine timeout (timer touched
            # the flag, then killed the process with SIGTERM → exit 143).
            # Requiring exit 143 here closes the false-positive race: if the
            # command exits naturally while the timer is between touch and kill,
            # the flag exists but cmd_exit ≠ 143 so we do not misreport a timeout.
            _PORTABLE_TIMEOUT_TIMED_OUT=true
            cmd_exit=124
            rm -f "$timeout_flag"
        elif [ -n "$timeout_flag" ]; then
            # Flag path: timer may have touched the flag but the command exited
            # naturally (not SIGTERM).  Clean up the stale flag file.
            rm -f "$timeout_flag"
        fi
        # Restore caller's INT/TERM traps (only set in the flag-file path above).
        if [ -n "$timeout_flag" ]; then
            if [ -n "$_pt_old_int" ]; then eval "$_pt_old_int"; else trap - INT; fi
            if [ -n "$_pt_old_term" ]; then eval "$_pt_old_term"; else trap - TERM; fi
        fi
        if [ -z "$timeout_flag" ] && [ "$cmd_exit" -eq 143 ]; then
            # Degraded mode: 143 (SIGTERM) likely means our timer killed it.
            _PORTABLE_TIMEOUT_TIMED_OUT=true
            cmd_exit=124
        fi
    fi

    return "$cmd_exit"
}
