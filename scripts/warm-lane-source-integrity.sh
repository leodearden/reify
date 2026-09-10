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
#               advisory exit-3 sentinel, but ONLY when git answered about this
#               lane's own repository and worktree (see `view=` below): a
#               foreign index's paths are absent here for the mundane reason
#               that this lane never tracked them, and the on-disk
#               discriminator cannot tell that apart from a real vanish.
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
#   reaches this script THREE ways -- an earlier revision of this comment
#   claimed two, and the third was a live false sentinel for two review rounds:
#   an index carrying a foreign repo's entries whose OBJECTS the lane's store
#   lacks makes `git status` itself fatal ("unable to read <oid>") -> exit 2
#   with git's own error surfaced, never a false all-clear; an index merely
#   MISSING entries yields X-column (`D `) staged deletions, which the
#   worktree-column rule below excludes as the intentional removals they are;
#   and an index from a SIBLING LANE OF THE SAME SHARED STORE resolves every
#   OID, so nothing is fatal and its paths reach the classifier as ordinary
#   worktree deletions. Only the third is a false-sentinel risk, and it is the
#   index arm of the identity check below -- not this bucketing -- that closes
#   it.
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
#   scripts/lib_git_env_scrub.sh is the scrubber (and its variable list is the
#   authority on which variables can redirect a view; the identity check below
#   covers the three that change git's ANSWER about this lane). Unscrubbed, every run is
#   reported as what it is -- `view=foreign`, sentinel withheld -- rather than
#   as either an all-clear or a sighting for a lane it never measured.
#
# Output contract:
#   stdout — EXACTLY one machine-readable line, always, on every classified
#            run: `source-integrity: deleted=N phantom=M lane=<basename>
#            view=<lane|foreign>`.
#            `view` is the field a consumer must read alongside the exit code:
#            `foreign` means git answered about another repository or worktree,
#            so the counts describe THAT view's index re-stat-ed here and the
#            lane itself was never measured. It is carried on stdout, not left
#            to stderr prose, because suppressing the sentinel (below) without a
#            machine-readable reason would trade a loud false alarm for a quiet
#            one.
#   stderr — every diagnostic, each offending path tagged with its class.
#            Silent on a clean lane. Paths are printed VERBATIM, never in
#            git's C-quoted porcelain rendering, so they can be pasted into a
#            shell (this is why the porcelain is read in -z form).
#
# Exit codes:
#   0  — Nothing attributable to THIS lane. Three distinguishable cases, and
#        `view=` separates them: a clean lane; the phantom-only case (an index
#        artifact is not the source-vanishing signature); and any run under a
#        foreign view, whose counts are not lane evidence at all. Exit 0 with
#        `view=foreign` is NOT an all-clear — it means the lane was not
#        measured.
#   3  — At least one tracked file is reported deleted AND is absent on disk,
#        measured through this lane's OWN repository and worktree. Advisory
#        sentinel, never a requeue — the same exit-3 "condition detected"
#        convention as scripts/warm-lane-disk-guard.sh --soft and
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
#   A3 — the exit-3 sentinel is a claim about THIS lane, so it is raised only
#        from entries measured through this lane's own repository AND worktree
#        (`view=lane`). One rule, no per-shape exceptions: a foreign index's
#        paths are absent here for the mundane reason that this lane never
#        tracked them, and the on-disk re-stat -- the whole discriminator --
#        cannot tell that apart from a real vanish.

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
                 entry, found by a pure-filesystem walk-up). An empty value is
                 refused rather than falling back to that default.
    -h, --help   Print this message and exit.

  Output:
    stdout       Exactly one line:
                 source-integrity: deleted=N phantom=M lane=<basename> view=V
                 view=foreign means git answered about another repository,
                 worktree or index, so the lane itself was NOT measured and the
                 counts are not evidence about it.
    stderr       Diagnostics, one tagged line per offending path.

  Exit codes:
    0   — Nothing attributable to this lane: a clean lane, the phantom-only
          case, or any view=foreign run (which is NOT an all-clear).
    3   — At least one tracked file is deleted and absent on disk, measured
          through this lane's OWN repository, worktree and index (advisory
          sentinel; same convention as warm-lane-disk-guard.sh --soft).
    2   — Usage or wiring error (bad flag, missing/empty/!dir --lane,
          non-worktree).
    1   — Runtime error.
EOF
}

# ── argument parsing ──────────────────────────────────────────────────────────
LANE_ARG=""
while [ $# -gt 0 ]; do
    case "$1" in
        --lane)
            # The EMPTINESS check is not cosmetic. `--lane ""` is what an unset
            # or empty variable expands to at the deployed call site (a
            # dark-factory session-start invocation passing a lane path), and
            # without it the value falls through to the walk-up branch below --
            # silently classifying whatever repository the CALLER's cwd happens
            # to sit in, and stamping it `view=lane`, which positively asserts
            # that lane WAS measured. Task 7227 measured exactly that: exit 3
            # naming a directory nobody asked about. Refusing it here is what
            # keeps the documented exit-2 contract and invariant A3 true.
            [ $# -ge 2 ] && [ -n "$2" ] || {
                err "--lane requires a non-empty directory argument."
                _usage
                exit 2
            }
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
# --no-optional-locks carries A1, so keep it: a bare `git status` rewrites
# .git/index and takes .git/index.lock, which against the deployed target -- a
# LIVE lane -- can fail that agent's own concurrent `git add`/`git commit`. It
# suppresses the write and changes no reported entry. Pinned by G4-G6.
# -uno stays off on purpose: it would make Block E's untracked fixture vacuous.
if ! git --no-optional-locks -C "$LANE" status --porcelain -z \
        > "$_STATUS_Z" 2> "$_GIT_ERR"; then
    err "git could not report on lane: $LANE"
    sed 's/^/  git: /' < "$_GIT_ERR" >&2 || true
    hint "Not a usable git worktree. Reporting a wiring error rather than a false all-clear."
    exit 2
fi

# ── is git answering about THIS lane's repository AND worktree? ───────────────
# An inherited view is CLASSIFIED, not scrubbed -- but it must never be mistaken
# for evidence ABOUT THIS LANE. Git resolves "which repository", "which
# worktree" and "which index" from THREE independent variables, so a view can
# be foreign in any one axis alone. Measured on this host, git 2.43.0, with
# `git -C <lane> rev-parse --absolute-git-dir --show-toplevel
# --path-format=absolute --git-path index` under each shape:
#
#   GIT_DIR + GIT_WORK_TREE  gitdir=poison  toplevel=poison  index=poison
#   GIT_DIR only             gitdir=poison  toplevel=LANE    index=poison
#   GIT_WORK_TREE only       gitdir=LANE    toplevel=poison  index=LANE
#   GIT_INDEX_FILE only      gitdir=LANE    toplevel=LANE    index=poison
#
# So NO equality test subsumes another and all three arms are load-bearing:
# the toplevel arm alone misses the GIT_DIR-only shape (the one git itself
# hands to a hook's descendants), the gitdir arm alone misses the
# GIT_WORK_TREE-only shape, and NEITHER sees GIT_INDEX_FILE. Block J pins each
# arm against a shape the others cannot see.
#
# GIT_INDEX_FILE is the highest-consequence axis in reify's topology, and the
# one this script got wrong for two rounds. `git status` compares the EFFECTIVE
# INDEX against the lane's files, so a sibling lane's index reports every path
# that lane tracks and this one does not as ` D`, which then re-stats as
# genuinely absent here. Whether that reaches the classifier at all depends on
# the OBJECT STORE, which is why the obvious two-independent-repos fixture is
# misleading: there, `git status`'s rename detection reads a blob the lane's
# store does not have and dies ("unable to read <oid>") -> exit 2, and the bug
# hides. Across two lanes of ONE SHARED store -- reify's actual arrangement,
# ~253 linked worktrees over /home/leo/src/reify/.git -- every OID resolves,
# nothing is fatal, and the run produced `deleted=2 ... view=lane` exit 3 under
# the "do NOT restore" hint, naming plausible reify source paths. Block J4
# builds the shared-store shape deliberately for that reason.
#
# The GIT_DIR-only shape is the dangerous one and is why the sentinel is
# suppressed below rather than merely annotated: git compares a FOREIGN index
# against the LANE's files, so every path the foreign repo tracks and this lane
# does not is reported ` D` and then re-stats as genuinely absent here. The
# on-disk discriminator cannot help -- those paths really are missing -- so the
# run produced an exit-3 sentinel naming the lane, listing files that were
# never the lane's, scaling with the foreign repo's tracked-file count. An
# inherited GIT_DIR naming another lane of the SHARED .git store would render
# that as a plausible-looking set of reify source paths.
#
# The lane's own gitdir is resolved PURELY FROM THE FILESYSTEM, for the reason
# the resolution and root guard above are: asking git for it would ask the
# poisoned view to describe itself. Both on-disk shapes are handled -- a
# directory in the main checkout, a `gitdir: <path>` pointer file in a linked
# warm lane (measured here: `/home/leo/src/reify/.git/worktrees/_lane-4`, which
# --absolute-git-dir reports identically, so an unpoisoned lane compares equal).
FOREIGN_VIEW=""

_lane_gitdir=""
if [ -d "$LANE/.git" ]; then
    _lane_gitdir="$LANE/.git"
elif [ -f "$LANE/.git" ]; then
    # `gitdir: <path>`; git writes it absolute, but a relative pointer is legal
    # and resolves against the worktree root.
    _lane_gitdir="$(sed -n 's/^gitdir: *//p' "$LANE/.git" | head -n 1)"
    case "$_lane_gitdir" in
        ""|/*) ;;
        *) _lane_gitdir="$LANE/$_lane_gitdir" ;;
    esac
fi
if [ -n "$_lane_gitdir" ] && [ -d "$_lane_gitdir" ]; then
    _lane_gitdir="$(cd "$_lane_gitdir" && pwd -P)"
fi

_view_gitdir="$(git -C "$LANE" rev-parse --absolute-git-dir 2>/dev/null || true)"
if [ -n "$_view_gitdir" ] && [ -d "$_view_gitdir" ]; then
    _view_gitdir="$(cd "$_view_gitdir" && pwd -P)"
fi

_view_top="$(git -C "$LANE" rev-parse --show-toplevel 2>/dev/null || true)"
if [ -n "$_view_top" ] && [ -d "$_view_top" ]; then
    _view_top="$(cd "$_view_top" && pwd -P)"
fi

# --path-format=absolute is required: under `-C`, the bare `--git-path index`
# form returns a path relative to the -C directory, which would compare unequal
# on every healthy lane. The index is a FILE and may not exist yet, so it is
# normalised via its PARENT rather than by cd-ing to it.
_view_index="$(git -C "$LANE" rev-parse --path-format=absolute --git-path index 2>/dev/null || true)"
if [ -n "$_view_index" ] && [ -d "$(dirname "$_view_index")" ]; then
    _view_index="$(cd "$(dirname "$_view_index")" && pwd -P)/$(basename "$_view_index")"
fi

# Only a POSITIVE mismatch marks the view foreign. An unresolvable side is left
# alone deliberately: `git status` already succeeded, so a blank here means
# rev-parse could not name the thing, not that it named something else, and
# inventing a mismatch from silence would fire this on every healthy lane.
if [ -n "$_view_gitdir" ] && [ -n "$_lane_gitdir" ] && [ "$_view_gitdir" != "$_lane_gitdir" ]; then
    FOREIGN_VIEW="repository $_view_gitdir"
elif [ -n "$_view_top" ] && [ "$_view_top" != "$LANE" ]; then
    FOREIGN_VIEW="worktree $_view_top"
elif [ -n "$_view_index" ] && [ -n "$_lane_gitdir" ] && [ "$_view_index" != "$_lane_gitdir/index" ]; then
    FOREIGN_VIEW="index $_view_index"
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

# The foreign-view notice comes FIRST, because it changes what every count
# below means. Stating it after the per-path listing would let an operator read
# "deleted: crates/reify-kernel/src/lib.rs — do NOT restore, this is evidence"
# about a file that was never this lane's.
if [ -n "$FOREIGN_VIEW" ]; then
    warn "git answered about a different $FOREIGN_VIEW — NOT this lane ($LANE)"
    info "An inherited GIT_DIR / GIT_WORK_TREE / GIT_INDEX_FILE is observed rather than"
    info "scrubbed, because classifying the agent's own view is the point. But nothing"
    info "below is evidence about this lane: the paths come from that view, re-stat-ed"
    info "here, so a path 'missing' here may simply be one this lane never tracked."
    info "This is not an all-clear either — the lane itself was never measured. Re-run"
    info "with the git environment scrubbed (scripts/lib_git_env_scrub.sh) to measure it."
fi

if [ "$_n_deleted" -gt 0 ] && [ -z "$FOREIGN_VIEW" ]; then
    err "$_n_deleted tracked file(s) are reported deleted by git AND are absent on disk (lane: $LANE)"
    for _p in "${DELETED[@]+"${DELETED[@]}"}"; do
        err "  deleted: $_p"
    done
    hint "Report-only by design: do NOT restore these — the missing copy is the evidence"
    hint "       a second sighting needs to attribute the mechanism (task 7227, esc-7106-5)."
    hint "       Record the lane path, its branch, and this output before touching anything."
elif [ "$_n_deleted" -gt 0 ]; then
    warn "$_n_deleted path(s) from that view's index are absent under this lane:"
    for _p in "${DELETED[@]+"${DELETED[@]}"}"; do
        warn "  unattributable: $_p"
    done
    info "Counted, but they raise no sentinel: a foreign index's paths are not this"
    info "lane's vanished files, and this run cannot tell the two apart."
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

printf 'source-integrity: deleted=%d phantom=%d lane=%s view=%s\n' \
    "$_n_deleted" "$_n_phantom" "$(basename "$LANE")" \
    "$([ -n "$FOREIGN_VIEW" ] && printf foreign || printf lane)"

# THE SENTINEL IS A CLAIM ABOUT THIS LANE, so it may only be raised from
# evidence measured through this lane's own repository and worktree. One rule,
# no per-shape exceptions -- the GIT_DIR-only false sentinel above and the
# already-shipped mixed case (a foreign-index path absent here, counted
# `deleted` and exited 3 while the lane itself was intact) are the same defect
# and are closed by the same line.
[ -z "$FOREIGN_VIEW" ] || exit 0
[ "$_n_deleted" -eq 0 ] || exit 3
exit 0
