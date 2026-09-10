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
#   With no --lane, the lane is the worktree root of the current directory --
#   derived, not assumed -- so an invocation from a subdirectory still
#   classifies the repo-root-relative porcelain paths correctly.
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
                 worktree root of the current directory).
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
    LANE="$(git rev-parse --show-toplevel 2>/dev/null)" || LANE=""
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
# Both resolution branches are covered deliberately. show-toplevel returns a
# genuine worktree root, which always carries a .git entry, so the default form
# is unaffected (Block H7 pins that); the only way it can trip this guard is a
# resolved toplevel with no .git entry, which is an inherited-GIT_WORK_TREE
# wiring error and belongs in exit 2 by the contract above.
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

printf 'source-integrity: deleted=%d phantom=%d lane=%s\n' \
    "$_n_deleted" "$_n_phantom" "$(basename "$LANE")"

[ "$_n_deleted" -eq 0 ] || exit 3
exit 0
