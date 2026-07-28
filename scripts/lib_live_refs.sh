#!/usr/bin/env bash
# scripts/lib_live_refs.sh — Shared live-reference /proc scanner for the warm-lane pool.
#
# Sourced by (both consumers, one definition — G7 no-lockstep-duplication):
#   scripts/warm-lane-gc.sh        — the ε reclaim PRIMITIVE. Calls
#       live_ref_present per lane (Pass 1) and per orphan (Pass 2), immediately
#       before the reset/remove, while that entry's flock is already held. Every
#       caller of gc.sh therefore inherits the guard — including dark-factory's
#       ε path (git_ops.py _run_warm_lane_gc_reclaim), which invokes
#       `warm-lane-gc.sh reclaim --mount <base>` and never passes
#       --extra-protect-glob (task 5572).
#   scripts/warm-lane-gc-sweep.sh  — the δ periodic backstop. Calls
#       live_referenced_paths ONCE per sweep for the whole batch of dead-PID
#       .reseed-trash candidates (_reap_stale_trash, task 5326).
#
# FUNCTIONS (defined when sourced):
#   live_referenced_paths RP...
#       Batch predicate: print the subset of the given resolved realpaths that
#       ANY live process references (cwd / open fd / mmap). ONE /proc walk for
#       the WHOLE set.
#   live_ref_present DIR
#       Single-candidate predicate: return 0 iff DIR (or a descendant) is
#       referenced by a live process; 1 otherwise.
#
# Neither function can distinguish "scanned, found nothing" from "scan failed":
# every stage is `|| true`-guarded because find/grep over /proc legitimately
# return non-zero on other users' permission-denied entries and on SIGPIPE from
# awk's early exit, so under `set -o pipefail` the exit status is unreliable
# (see the per-function note below). Callers get the fail-OPEN behaviour the
# sweep has always shipped: a failed scan reads as "no reference".

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_LIVE_REFS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_LIVE_REFS_SH_SOURCED=1

# live_referenced_paths RP... — given one or more resolved realpaths (trash
# dirs for the reaper, OR pool-lane / orphan-worktree dirs for the reclaim
# live-consumer gate — task 5378, task 5572), print (one per line,
# deduplicated) the subset that ANY live process references via its cwd
# symlink, an open fd symlink, or a mmap'd region in /proc/<pid>/maps.
# Performs exactly ONE /proc symlink walk and ONE /proc/*/maps grep for the
# WHOLE candidate set — O(/proc) once, NOT once per entry: with N stranded
# entries the older per-entry scan was O(N × /proc) (a single /proc walk
# already cost ~84s on a ~1300-process host, and the ENOSPC strand that
# motivates the reaper can leave many entries at once).
#
# The verdict is read from captured OUTPUT, never a pipeline exit status:
# find/grep over /proc return non-zero on the unavoidable permission-denied
# entries of other users' processes (and SIGPIPE on an early awk exit), so
# under `set -o pipefail` the exit status is an unreliable signal. A vanishing
# /proc entry only produces suppressed stderr; every scan is `|| true`-guarded
# so it never aborts under set -e.
live_referenced_paths() {
    [ $# -gt 0 ] || return 0

    # cwd + open-fd references: one find pass prints every process's cwd and
    # open-fd symlink TARGETS (%l = the kernel-canonical target — no per-fd
    # fork). awk loads the candidate realpaths as input records (the NR==FNR
    # two-file trick — avoids `-v`'s backslash-escape processing on data), then
    # for each target prints any candidate it equals or is a "$cand/" descendant
    # of (string compare, so arbitrary path characters are safe), each candidate
    # at most once, exiting early once every candidate is accounted for.
    # /proc/<pid>/task is pruned to skip the per-thread symlink duplicates.
    find /proc -maxdepth 3 -path '*/task' -prune -o -type l \
            \( -name cwd -o -path '*/fd/*' \) -printf '%l\n' 2>/dev/null \
        | awk '
            NR==FNR { if ($0 != "") cand[++n] = $0; next }
            {
                for (i = 1; i <= n; i++)
                    if (!(i in seen) && ($0 == cand[i] || index($0, cand[i] "/") == 1)) {
                        print cand[i]; seen[i] = 1; nfound++
                    }
                if (n > 0 && nfound >= n) exit
            }
        ' <(printf '%s\n' "$@") - 2>/dev/null || true

    # mmap'd regions: a single grep over every process's maps for a path mapped
    # from INSIDE any candidate dir. Each pattern carries a trailing "/" so the
    # fixed-string match is path-BOUNDARY-aware: ".../_lane-1/" matches a maps
    # line ".../_lane-1/target/x" but NOT the longer sibling ".../_lane-10/..."
    # whose basename merely has "_lane-1" as a name-prefix. A bare-substring
    # match (the original) spuriously mapped an "_lane-10" build's maps line back
    # to a free "_lane-1" whenever the longer path was not itself a live
    # candidate — e.g. a lane torn down mid-build, whose "(deleted)" mapping
    # lingers — perpetually shielding low-numbered lanes' divergent target/ from
    # reclaim (esc-5378 review). This mirrors the cwd/fd pass's cand"/"
    # descendant test above; a mmap'd file always lives UNDER the dir, so
    # requiring "/" drops no real reference. The trailing "/" is then stripped so
    # each emitted line equals the input candidate realpath EXACTLY — callers
    # key on it (the reaper's `referenced` set). (The reaper's trash inputs
    # already carried a ".<pid>" delimiter, so this only tightens — never
    # loosens — its matching.)
    grep -hoFf <(printf '%s/\n' "$@") /proc/[0-9]*/maps 2>/dev/null \
        | sed 's:/$::' || true
}

# live_ref_present DIR — single-candidate predicate over live_referenced_paths.
# Returns 0 iff DIR itself, or anything under it, is referenced by a live
# process (cwd / open fd / mmap); 1 otherwise.
#
# DIR is resolved with `readlink -f` FIRST, and that is mandatory rather than
# defensive: /proc cwd/fd symlink targets and /proc/*/maps paths are
# kernel-canonical, so matching a literal "$WORKTREES_DIR/<name>" would
# silently never match under a symlinked mount — a guard that quietly never
# fires, i.e. the same class of failure the per-lane gate exists to remove.
#
# An unresolvable DIR returns 1 (not live): the path cannot be matched against
# kernel-canonical /proc targets at all, so there is nothing to compare. This
# mirrors the batch callers' per-entry `[ -n "$rp" ] || continue` skip.
live_ref_present() {
    local dir="${1:-}"
    [ -n "$dir" ] || return 1

    local rp
    rp="$(readlink -f "$dir" 2>/dev/null)" || rp=""
    [ -n "$rp" ] || return 1

    local hits
    hits="$(live_referenced_paths "$rp")" || hits=""
    [ -n "$hits" ]
}
