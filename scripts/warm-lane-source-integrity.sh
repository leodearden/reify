#!/usr/bin/env bash
# scripts/warm-lane-source-integrity.sh — Read-only detector for tracked source
# files that have VANISHED from a warm lane's worktree. Report-only
# observability: never mutates a lane, never restores a file, and never gates
# dispatch/reclaim/merge (PRD docs/prds/warm-lane-pool-sizing-lifecycle.md
# §9.5 inv.12, the same invariant scripts/warm-lane-audit.sh carries).
#
# WHY THE ON-DISK RECHECK IS THE WHOLE SCRIPT (task 7227).
#   `git status --porcelain` prints the same ` D <path>` for two situations
#   that are not the same event:
#
#     · the path is really gone from the lane — the esc-7106-5 sighting, still
#       unattributed;
#     · git is answering about a DIFFERENT tree than the one on disk at the
#       lane, because a foreign view was inherited from the environment (a
#       GIT_DIR/GIT_WORK_TREE naming another worktree). Measured: every
#       reported path is present under the lane.
#       tests/infra/test_warm_lane_source_integrity.sh Block C builds this.
#
#   A detector that trusts git's answer fires identically on both and so
#   produces another unattributed observation — the exact outcome this script
#   exists to prevent. Every git-reported worktree deletion is therefore
#   re-stat-ed under the lane and bucketed:
#
#     deleted — absent on disk. The vanished-file signature; raises the
#               advisory exit-3 sentinel.
#     phantom — present on disk. A view artifact, not a vanished file.
#               Reported, but deliberately does NOT raise the sentinel: if it
#               did, one poisoned environment would drown the signal this
#               detector was built for.
#
#   A poisoned view is OBSERVED, not scrubbed: an inherited GIT_DIR /
#   GIT_WORK_TREE / GIT_INDEX_FILE is left exactly as the agent's own commands
#   would see it, because classifying the agent's view is the point. Scrubbing
#   it here would silently convert a sighting into "nothing to report".
#
#   THE NEIGHBOURING FAILURE, task #7106, IS A THIRD SHAPE and lands in neither
#   bucket — correctly. Its mechanism (an unscrubbed relative GIT_INDEX_FILE
#   letting an infra-test fixture overwrite the real index) is documented in
#   scripts/lib_git_env_scrub.sh, which is also where the scrubbing fix
#   correctly lives; do not re-derive it here. Measured on git 2.43.0, it
#   reaches this script two ways and the handling is right on both: an index
#   carrying a FOREIGN repo's entries makes `git status` itself fatal
#   ("unable to read <oid>") -> exit 2 with git's own error surfaced, never a
#   false all-clear; and an index merely MISSING entries yields X-column
#   (`D `) staged deletions, which the worktree-column rule below excludes as
#   the intentional removals they are.
#
# WHY IT NEVER RESTORES.
#   The missing copy IS the evidence. Attribution needs a second natural
#   sighting; a self-healing `git checkout -- <path>` would convert a
#   diagnosable incident into an invisible one. Reify ships this primitive;
#   dark-factory owns the agent-session-start invocation (the cross-repo seam
#   pattern in CLAUDE.md).
#
# Usage:
#   scripts/warm-lane-source-integrity.sh [--lane DIR]
#
#   --lane DIR is taken LITERALLY — the directory named is the lane, and is
#   never re-resolved through git. That is load-bearing: under a poisoned view
#   `git rev-parse --show-toplevel` names the FOREIGN worktree, and resolving
#   through it would stat every path against that foreign tree and report the
#   whole lane as deleted.
#
#   Because it is literal, the directory named must be the lane ROOT: porcelain
#   paths are worktree-root-relative, so a SUBDIRECTORY would be joined against
#   the wrong root and would misclassify in both directions (task 7227 measured
#   both flips). A --lane that is not a worktree root is refused with exit 2.
#   With no --lane, the lane is the nearest ancestor of $PWD carrying a .git
#   entry, found by a pure-filesystem walk-up -- so an invocation from a
#   subdirectory still classifies the repo-root-relative porcelain paths
#   correctly, and "never re-resolved through git" holds on BOTH branches.
#   `git rev-parse --show-toplevel` would reinstate here the exact hazard the
#   paragraph above rules out for --lane; task 7227 measured it doing so.
#
#   NEITHER form scrubs the git environment, and a caller that wants the LANE
#   measured rather than its own view must do so itself (the session-start
#   invocation is dark-factory's, per CLAUDE.md's cross-repo seam pattern).
#   scripts/lib_git_env_scrub.sh is the scrubber. Unscrubbed, a foreign view
#   that is itself CLEAN makes git report nothing at all, and the run is
#   reported as what it is -- see the foreign-view notice below -- rather than
#   as an all-clear for a lane it never measured.
#
# Output contract:
#   stdout — EXACTLY one machine-readable line, always, on every classified
#            run: `source-integrity: deleted=N phantom=M lane=<basename>`.
#   stderr — every diagnostic, each offending path tagged with its class.
#            Silent on a clean lane. Paths are printed VERBATIM, never in
#            git's C-quoted porcelain rendering, so they can be pasted into a
#            shell (this is why the porcelain is read in -z form).
#
# Exit codes:
#   0  — No real deletion. Includes the phantom-only case: an index artifact
#        is not the source-vanishing signature.
#   3  — At least one tracked file is reported deleted AND is absent on disk.
#        Advisory sentinel, never a requeue — the same exit-3 "condition
#        detected" convention as scripts/warm-lane-disk-guard.sh --soft and
#        scripts/fleet-load-detector.sh, so a consumer needs no new vocabulary.
#   2  — Usage or wiring error: unknown flag, missing flag value, a --lane that
#        does not exist, is not a directory, or is not a worktree ROOT, or a
#        lane git cannot report on.
#        Chosen over a false "all clear" so a mis-wired invocation is visible.
#   1  — Runtime error (this script could not do its own work at all).
#
# Invariants:
#   A1 — read-only: no write, create, touch, mkdir, stage or restore ever
#        occurs under the lane. The only filesystem writes are to a private
#        mktemp dir, removed on exit.
#   A2 — the stdout line is emitted on every classified run, zeros included,
#        so a consumer can distinguish "clean" from "did not run".

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
hint()  { err "Hint:  $*"; }

_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--lane DIR]

  Read-only detector for tracked source files that have vanished from a warm
  lane's worktree. Re-stats every deletion git reports and separates the two
  failure modes that porcelain renders identically:

    deleted — git says deleted and the path is ABSENT on disk (esc-7106-5).
    phantom — git says deleted but the path is PRESENT on disk: git is
              answering about a different tree (a foreign git view inherited
              from the environment), not a vanished file.

  Report-only. Never restores a file, never stages, never mutates lane state —
  the missing copy is the evidence a second sighting needs.

  Options:
    --lane DIR   Lane ROOT directory to inspect, taken literally and never
                 re-resolved through git; a subdirectory is refused, since
                 porcelain paths are worktree-root-relative (default: the
                 nearest ancestor of the current directory carrying a .git
                 entry, found by a pure-filesystem walk-up).
    -h, --help   Print this message and exit.

  Output:
    stdout       Exactly one line:
                 source-integrity: deleted=N phantom=M lane=<basename>
    stderr       Diagnostics, one tagged line per offending path.

  Exit codes:
    0   — No real deletion (including the phantom-only case).
    3   — At least one tracked file is deleted and absent on disk (advisory
          sentinel; same convention as warm-lane-disk-guard.sh --soft).
    2   — Usage or wiring error (bad flag, missing/!dir --lane, non-worktree).
    1   — Runtime error.
EOF
}

# ── argument parsing ──────────────────────────────────────────────────────────
LANE_ARG=""
while [ $# -gt 0 ]; do
    case "$1" in
        --lane)
            [ $# -ge 2 ] || { err "--lane requires a directory argument."; _usage; exit 2; }
            LANE_ARG="$2"; shift 2 ;;
        -h|--help)
            _usage; exit 0 ;;
        *)
            err "Unknown argument: $1"; _usage; exit 2 ;;
    esac
done

# ── lane resolution ───────────────────────────────────────────────────────────
if [ -n "$LANE_ARG" ]; then
    [ -d "$LANE_ARG" ] || {
        err "--lane is not an existing directory: $LANE_ARG"
        exit 2
    }
    LANE="$(cd "$LANE_ARG" && pwd -P)" || {
        err "cannot enter --lane directory: $LANE_ARG"
        exit 2
    }
else
    # A PURE-FILESYSTEM walk-up from $PWD to the nearest ancestor carrying a
    # .git entry -- never `git rev-parse --show-toplevel`, for the same reason
    # --lane is taken literally above. rev-parse honours an inherited
    # GIT_DIR/GIT_WORK_TREE, so under exactly the poisoned view this detector
    # exists to classify it names the FOREIGN worktree, and then every later
    # step -- the root guard, the on-disk re-stat, the `lane=` field -- measures
    # that tree under this lane's name. Measured on this host against the
    # pre-fix script, both directions: a lane whose tracked `sub/a.txt` really
    # was gone printed `deleted=0 phantom=0 lane=foreign` exit 0, and a clean
    # lane under a foreign view holding a deletion printed `deleted=1
    # lane=foreign` exit 3 -- a foreign tree's deletion attributed to a lane
    # that had none. Block I pins both.
    #
    # Only RESOLUTION is taken out of git's hands. The `git status` call below
    # keeps the inherited view deliberately, because classifying the agent's own
    # view is the point.
    #
    # The walk-up lands where show-toplevel would in an unpoisoned environment:
    # a worktree root is exactly a directory carrying a .git entry -- the same
    # predicate the root guard below applies, for the reasons stated there.
    LANE=""
    _dir="$(pwd -P)"
    while : ; do
        if [ -e "$_dir/.git" ]; then LANE="$_dir"; break; fi
        [ "$_dir" != "/" ] || break
        _dir="$(dirname "$_dir")"
    done
    [ -n "$LANE" ] || {
        err "no --lane given and the current directory is not inside a git worktree."
        hint "Pass --lane DIR, or run from within the lane."
        exit 2
    }
fi

# ── worktree-root guard ───────────────────────────────────────────────────────
# --lane must name the lane ROOT. Porcelain paths are worktree-root-relative
# even when git is invoked from a subdirectory (measured, git 2.43.0:
# `git -C <root>/sub status --porcelain` prints ` D sub/x.txt`), while --lane is
# taken literally above, so a subdirectory would be joined against the wrong
# root and misclassify in BOTH directions -- a real deletion re-stat-ed one
# level too deep can land on a colliding leaf name and be reported `phantom`
# (a false all-clear, sentinel suppressed), and a genuine phantom with no
# counterpart under the subdirectory be reported `deleted` (a false sentinel).
# Task 7227 reproduced both; tests/infra/test_warm_lane_source_integrity.sh
# Block H pins them.
#
# The predicate is PURE FILESYSTEM, and that is load-bearing. The obvious
# alternative -- comparing $LANE against `git -C "$LANE" rev-parse
# --show-toplevel` -- is wrong here: measured, under Block C's poisoned view
# show-toplevel names the FOREIGN worktree, so a root-equality check would
# reject that perfectly valid lane and collapse the phantom classification this
# script exists for. It would also break the "taken LITERALLY ... never
# re-resolved through git" property above, which exists for the same reason.
#
# -e covers both real lane shapes, verified on this host: a linked warm lane's
# .git is a regular FILE (_lane-1, _lane-2, _merge-verify) and the main
# checkout's is a DIRECTORY, so -d or -f alone would reject half the fleet.
#
# The guard is reached by both resolution branches but can only ever bite the
# --lane one: the walk-up above stops at the first ancestor carrying a .git
# entry, so the default form arrives here already satisfying the predicate
# (Block H7 and Block I pin that it does). It is left applying to both anyway --
# one predicate over the one value $LANE can hold, rather than a branch-specific
# rule that a later edit could leave half-enforced.
[ -e "$LANE/.git" ] || {
    err "--lane is not a worktree ROOT (no .git entry): $LANE"
    hint "Pass the lane directory itself; porcelain paths are worktree-root-relative"
    hint "       and would be stat-ed against the wrong root."
    exit 2
}

# ── collect git's view ────────────────────────────────────────────────────────
# The porcelain is read in -z form for two independent reasons, both of which a
# v1 text parse would get wrong: paths are emitted VERBATIM (no C-quoting of
# spaces or non-ASCII, so no unquoting parser is needed at all), and a
# rename/copy entry's two paths arrive as two separate NUL-terminated fields
# rather than an in-band ` -> ` separator that a path may itself contain.
_TMP="$(mktemp -d "${TMPDIR:-/tmp}/warm-lane-source-integrity.XXXXXX")" || {
    err "cannot create a private temp dir; nothing measured."
    exit 1
}
trap 'rm -rf "$_TMP"' EXIT

_STATUS_Z="$_TMP/status.z"
_GIT_ERR="$_TMP/git.err"
if ! git -C "$LANE" status --porcelain -z > "$_STATUS_Z" 2> "$_GIT_ERR"; then
    err "git could not report on lane: $LANE"
    sed 's/^/  git: /' < "$_GIT_ERR" >&2 || true
    hint "Not a usable git worktree. Reporting a wiring error rather than a false all-clear."
    exit 2
fi

# ── whose tree did git just answer about? ─────────────────────────────────────
# An inherited view is CLASSIFIED, not scrubbed -- but it must never pass for
# silence. When the foreign tree happens to be clean, git reports nothing at
# all, both buckets below stay empty, and the summary line reads exactly like a
# healthy lane while nothing about the lane was measured (task 7227 measured
# precisely that: a lane whose `sub/a.txt` was really gone, reported as a zero
# summary under a clean foreign view). Naming the tree git answered about is
# what keeps those two apart.
#
# This is the ONE place $LANE is compared against a git-resolved root, and it is
# deliberately not a guard: resolution above stays literal, the exit code below
# is untouched, and a foreign view stays a reported condition rather than a
# rejected one -- rejecting it would collapse the phantom classification this
# script exists for.
FOREIGN_VIEW=""
_view_top="$(git -C "$LANE" rev-parse --show-toplevel 2>/dev/null || true)"
if [ -n "$_view_top" ] && [ -d "$_view_top" ]; then
    _view_top="$(cd "$_view_top" && pwd -P)"
    [ "$_view_top" = "$LANE" ] || FOREIGN_VIEW="$_view_top"
fi

# ── classify ──────────────────────────────────────────────────────────────────
DELETED=()
PHANTOM=()
while IFS= read -r -d '' _entry; do
    # `XY<space><path>`: two status columns, a separator, then the path.
    [ ${#_entry} -ge 4 ] || continue
    _xy="${_entry:0:2}"
    _path="${_entry:3}"

    # A rename/copy carries a SECOND NUL-terminated field (the source path).
    # It is consumed unconditionally for X in R/C — including when the entry is
    # about to be skipped — because leaving it in the stream would desynchronise
    # every later entry, silently mis-bucketing the rest of the lane.
    case "${_xy:0:1}" in
        R|C) IFS= read -r -d '' _ || true ;;
    esac

    # Unmerged entries reuse XY for CONFLICT state rather than the
    # index/worktree split, so their D means something else entirely: `UD`
    # (deleted by them) prints with the file still on disk, and `DD` prints
    # with it absent — which would raise a false sentinel on any lane sitting
    # in a conflicted merge.
    case "$_xy" in
        DD|AU|UD|UA|DU|AA|UU) continue ;;
    esac

    # Y, the WORKTREE column. An X-column D is a staged removal the agent asked
    # for; only Y=D says the worktree copy is not where the index expects it.
    [ "${_xy:1:1}" = "D" ] || continue

    # The discriminator. -e alone would miss a dangling symlink, which exists as
    # a worktree entry even though it resolves to nothing.
    if [ -e "$LANE/$_path" ] || [ -L "$LANE/$_path" ]; then
        PHANTOM+=("$_path")
    else
        DELETED+=("$_path")
    fi
done < "$_STATUS_Z"

# ── report ────────────────────────────────────────────────────────────────────
_n_deleted=${#DELETED[@]}
_n_phantom=${#PHANTOM[@]}

if [ "$_n_deleted" -gt 0 ]; then
    err "$_n_deleted tracked file(s) are reported deleted by git AND are absent on disk (lane: $LANE)"
    for _p in "${DELETED[@]+"${DELETED[@]}"}"; do
        err "  deleted: $_p"
    done
    hint "Report-only by design: do NOT restore these — the missing copy is the evidence"
    hint "       a second sighting needs to attribute the mechanism (task 7227, esc-7106-5)."
    hint "       Record the lane path, its branch, and this output before touching anything."
fi

if [ "$_n_phantom" -gt 0 ]; then
    warn "$_n_phantom path(s) are reported deleted by git but ARE present on disk (lane: $LANE)"
    for _p in "${PHANTOM[@]+"${PHANTOM[@]}"}"; do
        warn "  phantom: $_p"
    done
    info "A phantom is a view artifact, not a vanished file: git is answering about a"
    info "tree other than the one on disk here — typically a foreign GIT_DIR/GIT_WORK_TREE"
    info "inherited from the environment. It does not raise the exit-3 sentinel."
fi

if [ -n "$FOREIGN_VIEW" ]; then
    warn "git answered about a DIFFERENT worktree than this lane (lane: $LANE)"
    warn "  git's worktree root here is: $FOREIGN_VIEW"
    info "An inherited GIT_DIR/GIT_WORK_TREE is observed rather than scrubbed, because"
    info "classifying the agent's own view is the point. But the counts below describe"
    info "THAT tree's answers, re-stat-ed here — they are not an all-clear for this lane,"
    info "which git was never asked about. Re-run with the git environment scrubbed"
    info "(scripts/lib_git_env_scrub.sh) to measure the lane itself."
fi

printf 'source-integrity: deleted=%d phantom=%d lane=%s\n' \
    "$_n_deleted" "$_n_phantom" "$(basename "$LANE")"

[ "$_n_deleted" -eq 0 ] || exit 3
exit 0
