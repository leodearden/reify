#!/usr/bin/env bash
# tests/infra/run-all-member-tmpdir-lib.sh — run one command with a private
# TMPDIR that is removed when it exits, and reap the ones a killed run left
# behind (task 10372).
#
# Sourced by tests/infra/run_all.sh, which runs every member through
# member_tmpdir_run and calls member_tmpdir_reap once per run, before the first
# member starts. The root (run_all's own ${TMPDIR:-/tmp}) arrives here as an
# argument, and nothing here reads a caller global.
#
# SCOPE. Only run_all.sh's members get a private TMPDIR. A task-role verify
# runs its selected infra members from verify.sh's selective infra leaf
# (scripts/reify_verify/render.py::_infra_leaf_loop), not through run_all.sh,
# so a member killed there still leaks into /tmp.
#
# WHY. Members clean their temp dirs up on a normal exit, but a member killed
# by its budget, or a cancelled verify, dies before its cleanup runs, and no
# per-test fix survives SIGKILL. Giving every member a TMPDIR of its own and
# removing it from the outside bounds every such leak at one choke point.
#
# THE LEASE. member_tmpdir_run holds flock(LOCK_EX) on an fd of the directory
# itself for as long as the command runs. The kernel drops it when that holder
# exits, however it exits, so "lease free" means "no live run owns this dir".
# The lease lives on the directory, so no extra file appears in the member's
# view.
#
# NOT INHERITED. The command runs with the lease fd closed, so neither the
# member nor a background child it leaves behind holds it: run_all's rule that
# no member holds a lock fd the run opened.
#
# STILL IN USE. A process a member started can outlive it and still work in
# its TMPDIR: an sccache server that a member's cargo respawned runs with the
# member's environment and makes a temp dir under TMPDIR for every compile, so
# removing the dir would fail every compile routed to it. Removal therefore
# first scans /proc/<pid>/environ, and keeps the dir, with one stderr line,
# while any live process this user can read was started with TMPDIR=<dir>.
# The first reap after that process exits collects the dir. The scan matches
# TMPDIR=<dir> exactly, in the environment a process was started with: a
# process that moved TMPDIR to a subdir, or uses the dir by another name, is
# not seen.
#
# THE CREATION RACE. mktemp creates the dir a moment before the lease is
# taken. member_tmpdir_reap only considers dirs whose mtime is more than a
# minute old, which no dir is in that window.
#
# Imposes no shell option on its caller and never exits the caller's shell:
# both functions are subshells, and every command in them that can fail is
# guarded, so they behave the same with or without the caller's set -e. No
# traps: when member_tmpdir_run is killed, the next run's reap collects its dir.

# Source guard: sourcing twice is a no-op.
if [ "${_REIFY_RUN_ALL_MEMBER_TMPDIR_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_RUN_ALL_MEMBER_TMPDIR_LIB_SOURCED=1

# _member_tmpdir_in_use <dir>: whether a live process this user can read was
# started with TMPDIR=<dir>.
_member_tmpdir_in_use() {
    grep -qsxzF -e "TMPDIR=$1" -- /proc/[0-9]*/environ
}

# _member_tmpdir_remove <dir>: rm -rf <dir>, retrying once with every entry
# made writable, since a member may leave a dir it cannot write into. Status 1,
# with one stderr line, when the dir is still in use or survives both attempts.
_member_tmpdir_remove() {
    if _member_tmpdir_in_use "$1"; then
        printf 'run_all.sh: keeping member TMPDIR %s: a live process still has it as TMPDIR; a later run reaps it.\n' "$1" >&2
        return 1
    fi
    rm -rf -- "$1" 2>/dev/null && return 0
    chmod -R u+rwx -- "$1" 2>/dev/null || true
    rm -rf -- "$1" 2>/dev/null && return 0
    printf 'run_all.sh: WARNING — could not remove member TMPDIR %s; the next run reaps it.\n' "$1" >&2
    return 1
}

# member_tmpdir_run <root> <cmd> [arg...]
#   Runs <cmd> with TMPDIR set to a fresh <root>/reify-ra-tmp.XXXXXX dir it
#   holds the lease on, removes that dir when <cmd> exits unless it is still in
#   use, and exits with <cmd>'s status. When the dir cannot be created or
#   leased, it warns once on stderr naming <root> and runs <cmd> with the
#   inherited TMPDIR.
member_tmpdir_run() (
    _mt_root="$1"
    shift
    _mt_rc=0
    if ! _mt_dir="$(mktemp -d "$_mt_root/reify-ra-tmp.XXXXXX" 2>/dev/null)"; then
        printf 'run_all.sh: WARNING — cannot create a member TMPDIR under %s; running with the inherited TMPDIR.\n' "$_mt_root" >&2
        "$@" || _mt_rc=$?
        exit "$_mt_rc"
    fi
    if ! { exec {_mt_lease}<"$_mt_dir"; } 2>/dev/null || ! flock -n -x "$_mt_lease"; then
        printf 'run_all.sh: WARNING — cannot lease a member TMPDIR under %s; running with the inherited TMPDIR.\n' "$_mt_root" >&2
        _member_tmpdir_remove "$_mt_dir" || true
        "$@" || _mt_rc=$?
        exit "$_mt_rc"
    fi
    export TMPDIR="$_mt_dir"
    "$@" {_mt_lease}<&- || _mt_rc=$?
    _member_tmpdir_remove "$_mt_dir" || true
    exit "$_mt_rc"
)

# member_tmpdir_reap <root>
#   Removes every <root>/reify-ra-tmp.* that is a directory (never a symlink),
#   owned by this user, last modified more than a minute ago, not leased, and
#   not still in use. A leased dir is a live run's and is kept; a lease that
#   cannot be probed keeps the dir with one stderr warning. Says on stderr how
#   many it removed, when any. Always exits 0.
member_tmpdir_reap() (
    _mt_root="$1"
    _mt_reaped=0
    readonly _mt_leased_rc=75
    [ -d "$_mt_root" ] || exit 0
    while IFS= read -r -d '' _mt_dir; do
        if ! { exec {_mt_fd}<"$_mt_dir"; } 2>/dev/null; then
            [ ! -e "$_mt_dir" ] \
                || printf 'run_all.sh: WARNING — cannot open stale member TMPDIR %s; keeping it.\n' "$_mt_dir" >&2
            continue
        fi
        _mt_flock_rc=0
        flock -n -x -E "$_mt_leased_rc" "$_mt_fd" || _mt_flock_rc=$?
        case "$_mt_flock_rc" in
            0) _member_tmpdir_remove "$_mt_dir" && _mt_reaped=$(( _mt_reaped + 1 )) ;;
            "$_mt_leased_rc") ;;
            *) printf 'run_all.sh: WARNING — flock on member TMPDIR %s failed (rc=%s); keeping it.\n' \
                   "$_mt_dir" "$_mt_flock_rc" >&2 ;;
        esac
        exec {_mt_fd}<&-
    done < <(find "$_mt_root" -mindepth 1 -maxdepth 1 -name 'reify-ra-tmp.*' -type d \
                  -user "$(id -u)" -mmin +1 -print0 2>/dev/null)
    [ "$_mt_reaped" -eq 0 ] \
        || printf 'run_all.sh: reaped %d stale member TMPDIR(s) under %s\n' "$_mt_reaped" "$_mt_root" >&2
    exit 0
)
