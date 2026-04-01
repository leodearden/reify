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
portable_timeout() {
    local seconds="$1"
    shift

    local cmd_exit=0
    if command -v timeout >/dev/null 2>&1; then
        timeout "$seconds" "$@" || cmd_exit=$?
    elif command -v gtimeout >/dev/null 2>&1; then
        gtimeout "$seconds" "$@" || cmd_exit=$?
    else
        # POSIX fallback: background the command, sleep, kill if still running.
        "$@" &
        local cmd_pid=$!
        ( sleep "$seconds" && kill "$cmd_pid" 2>/dev/null ) &
        local timer_pid=$!
        wait "$cmd_pid" 2>/dev/null || cmd_exit=$?
        # Clean up timer — if command finished before timeout, kill the sleep+kill subshell.
        kill "$timer_pid" 2>/dev/null || true
        wait "$timer_pid" 2>/dev/null || true
        # Exit code 143 = 128+15 (SIGTERM from kill) — treat as timeout, same as 124.
        if [ "$cmd_exit" -eq 143 ]; then
            cmd_exit=124
        fi
    fi

    return "$cmd_exit"
}
