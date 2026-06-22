#!/usr/bin/env bash
# scripts/warm-lane-gc.sh — Task-side GC for the warm-lane CoW pool.
# Reclaims divergent FREE lanes (reset via α seed primitive) and removes
# orphan cold worktrees; respects inv.preserve (dirty/unlanded/live-consumer).
#
# Part of PRD docs/prds/warm-lane-pool-space-safety.md §8.4, §10 δ.
# Consumer: dark-factory ε (invokes reclaim on the disk-pressure path).
#
# Usage:
#   scripts/warm-lane-gc.sh reclaim \
#       --worktrees-dir DIR \
#       --base-target SYMLINK \
#       [--main-ref REF] \
#       [--lane-glob GLOB] \
#       [--protect-glob GLOB] \
#       [--seed-script PATH]
#
# Subcommands:
#   reclaim   Scan --worktrees-dir; reset reclaimable lanes via α seed primitive;
#             remove reclaimable orphan worktrees; preserve dirty/unlanded/live lanes.
#
# Options:
#   --worktrees-dir DIR   Directory holding lane/worktree entries (required).
#   --base-target SYMLINK Symlink at <base>/target → <base>/target.gen.N (required).
#                         Resolved to its concrete .gen.N dir before invoking α.
#   --main-ref REF        Git ref for "main" branch (default: main).
#   --lane-glob GLOB      Glob matching pool-lane entries (default: _lane-*,_spec-*).
#                         Matched entries are reset via α, not removed.
#   --protect-glob GLOB   Glob matching entries to never touch (default: _merge-*).
#                         Matched entries are skipped entirely.
#   --seed-script PATH    Path to the α seed primitive (default: sibling seed-warm-lane.sh).
#                         Overridable for hermetic testing.
#   -h, --help            Print this message and exit.
#
# Exit codes:
#   0  — Completed sweep (best-effort; per-candidate failures warn + continue).
#   1  — Runtime error: could not resolve required argument (e.g. base-target symlink).
#   2  — Usage error: unknown flag, missing subcommand, missing required option.
#
# Env knobs (all overridable by flags):
#   REIFY_WARM_LANE_GC_WORKTREES_DIR   — default --worktrees-dir
#   REIFY_WARM_LANE_GC_BASE_TARGET     — default --base-target
#   REIFY_WARM_LANE_GC_MAIN_REF        — default --main-ref (default: main)
#   REIFY_WARM_LANE_GC_LANE_GLOB       — default --lane-glob
#   REIFY_WARM_LANE_GC_PROTECT_GLOB    — default --protect-glob
#   REIFY_WARM_LANE_GC_SEED_SCRIPT     — default --seed-script
#
# Design notes:
#   - Reclaimability is computed purely from filesystem + git + flock; dark-factory
#     FREE/ASSIGNED state is NOT consulted. "FREE/idle" ≈ no live consumer holding
#     the lane flock (mirroring refresh-warm-base.sh reader-refcount GC).
#   - inv.preserve shared predicate (_is_reclaimable): skip on dirty tracked changes
#     (git status --porcelain), unlanded ahead-of-main (merge-base --is-ancestor),
#     or live consumer (flock -n -x <dir>.lock fails).
#   - α reuse: resolve base symlink → concrete gen, hold flock -s during α call
#     (D8 reader-refcount seam; same contract as the acquire path).
#   - Safety-ranked order: reset lanes first (cheap), then remove orphans (destructive).
#   - Stdout: machine-readable summary line only.
#     Stderr: all diagnostics (info/ok/warn/err).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") reclaim --worktrees-dir DIR --base-target SYMLINK [OPTIONS]

  Task-side GC for the warm-lane CoW pool.
  Reclaims divergent FREE lanes (reset via α seed primitive) and removes
  orphan cold worktrees; respects inv.preserve (dirty/unlanded/live-consumer).

  Subcommands:
    reclaim   Scan worktrees-dir; reset reclaimable lanes; remove orphan worktrees.

  Required options:
    --worktrees-dir DIR   Directory holding lane/worktree entries.
    --base-target SYMLINK Symlink <base>/target → <base>/target.gen.N.

  Optional options:
    --main-ref REF        Git ref for 'main' (default: main).
    --lane-glob GLOB      Glob for pool-lane entries (default: _lane-*,_spec-*).
    --protect-glob GLOB   Glob for protected entries (default: _merge-*).
    --seed-script PATH    Path to α seed primitive (default: sibling seed-warm-lane.sh).
    -h, --help            Print this message and exit.

  Exit codes:
    0  — Completed sweep (per-candidate failures warn + continue).
    1  — Runtime error (e.g. base-target symlink unresolvable).
    2  — Usage error.

  Output:
    stdout: machine-readable summary: reclaim: reset=N removed=M preserved=K
    stderr: all diagnostics.
EOF
}

# ── defaults ───────────────────────────────────────────────────────────────────
WORKTREES_DIR="${REIFY_WARM_LANE_GC_WORKTREES_DIR:-}"
BASE_TARGET="${REIFY_WARM_LANE_GC_BASE_TARGET:-}"
MAIN_REF="${REIFY_WARM_LANE_GC_MAIN_REF:-main}"
LANE_GLOB="${REIFY_WARM_LANE_GC_LANE_GLOB:-}"
PROTECT_GLOB="${REIFY_WARM_LANE_GC_PROTECT_GLOB:-}"
SEED_SCRIPT="${REIFY_WARM_LANE_GC_SEED_SCRIPT:-}"

# ── arg parsing ────────────────────────────────────────────────────────────────
SUBCOMMAND=""

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --worktrees-dir)
            [ $# -ge 2 ] || { err "--worktrees-dir requires a value"; exit 2; }
            WORKTREES_DIR="$2"; shift 2 ;;
        --base-target)
            [ $# -ge 2 ] || { err "--base-target requires a value"; exit 2; }
            BASE_TARGET="$2"; shift 2 ;;
        --main-ref)
            [ $# -ge 2 ] || { err "--main-ref requires a value"; exit 2; }
            MAIN_REF="$2"; shift 2 ;;
        --lane-glob)
            [ $# -ge 2 ] || { err "--lane-glob requires a value"; exit 2; }
            LANE_GLOB="$2"; shift 2 ;;
        --protect-glob)
            [ $# -ge 2 ] || { err "--protect-glob requires a value"; exit 2; }
            PROTECT_GLOB="$2"; shift 2 ;;
        --seed-script)
            [ $# -ge 2 ] || { err "--seed-script requires a value"; exit 2; }
            SEED_SCRIPT="$2"; shift 2 ;;
        reclaim)
            SUBCOMMAND="reclaim"; shift ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            err "Unknown subcommand: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── validate subcommand ────────────────────────────────────────────────────────
if [ -z "$SUBCOMMAND" ]; then
    err "Missing subcommand. Expected: reclaim"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# ── validate required options ──────────────────────────────────────────────────
if [ -z "$WORKTREES_DIR" ]; then
    err "Missing required option: --worktrees-dir DIR"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi
if [ -z "$BASE_TARGET" ]; then
    err "Missing required option: --base-target SYMLINK"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# ── apply defaults for optional globs and seed-script ─────────────────────────
[ -n "$LANE_GLOB" ]    || LANE_GLOB="_lane-*,_spec-*"
[ -n "$PROTECT_GLOB" ] || PROTECT_GLOB="_merge-*"
if [ -z "$SEED_SCRIPT" ]; then
    SEED_SCRIPT="$SCRIPT_DIR/seed-warm-lane.sh"
fi

# ── helper: name matches a glob pattern ───────────────────────────────────────
# _matches_glob <name> <comma-separated-globs>
_matches_glob() {
    local name="$1"
    local globs="$2"
    local g
    # Split comma-separated globs
    local IFS=","
    for g in $globs; do
        # shellcheck disable=SC2254
        case "$name" in
            $g) return 0 ;;
        esac
    done
    return 1
}

# ── helper: is this dir a git worktree? ───────────────────────────────────────
_is_git_worktree() {
    local dir="$1"
    [ -d "$dir" ] || return 1
    git -C "$dir" rev-parse --git-dir >/dev/null 2>&1 || return 1
}

# ── shared reclaimability predicate ───────────────────────────────────────────
# _is_reclaimable <dir>
# Returns 0 (reclaimable) or 1 (preserve) with diagnostic to stderr.
# Note: does NOT acquire the flock — that is the caller's responsibility.
_is_reclaimable() {
    local dir="$1"
    local name
    name="$(basename "$dir")"

    # (a) dirty tracked changes
    local dirty
    # --untracked-files=no excludes '??' lines (untracked artifacts like target/)
    # so only uncommitted changes to TRACKED files are flagged as dirty.
    dirty="$(git -C "$dir" status --porcelain --untracked-files=no 2>/dev/null)" || {
        warn "preserving $name: git status failed — treating as dirty"
        return 1
    }
    if [ -n "$dirty" ]; then
        warn "preserving $name: dirty WIP (uncommitted tracked changes)"
        return 1
    fi

    # (b) unlanded ahead-of-main commits
    if ! git -C "$dir" merge-base --is-ancestor HEAD "$MAIN_REF" 2>/dev/null; then
        warn "preserving $name: unlanded ahead-of-main commits"
        return 1
    fi

    return 0
}

# ── reclaim subcommand ─────────────────────────────────────────────────────────
_do_reclaim() {
    local reset_count=0
    local removed_count=0
    local preserved_count=0

    info "warm-lane-gc.sh reclaim: worktrees_dir=$WORKTREES_DIR  base_target=$BASE_TARGET  main_ref=$MAIN_REF"

    # Resolve the base-target symlink to its concrete gen dir (D8 seam).
    # α requires the concrete path — cp -a copies the symlink otherwise.
    local resolved_gen
    if ! resolved_gen="$(readlink -f "$BASE_TARGET" 2>/dev/null)"; then
        err "Cannot resolve base-target symlink: $BASE_TARGET"
        return 1  # runtime error; exit 2 is reserved for usage/wiring errors
    fi
    local gen_lock="${resolved_gen}.lock"
    touch "$gen_lock" 2>/dev/null || true
    info "  resolved_gen=$resolved_gen  gen_lock=$gen_lock"

    # Enumerate all immediate subdirs in the worktrees-dir.
    # We collect entries first so we can do safety-ranked two-pass order:
    #   pass 1: reset reclaimable lanes (cheap, source-tree-preserving)
    #   pass 2: remove reclaimable orphans (destructive)

    local -a lane_candidates=()
    local -a orphan_candidates=()

    local entry name
    for entry in "$WORKTREES_DIR"/*/; do
        # Strip trailing slash
        entry="${entry%/}"
        [ -d "$entry" ] || continue
        name="$(basename "$entry")"

        # Skip protected entries entirely — count them as preserved in the summary
        # (they are not reclaimed, which is the user-visible meaning of "preserved").
        if _matches_glob "$name" "$PROTECT_GLOB"; then
            info "  skipping protected: $name"
            preserved_count=$((preserved_count + 1))
            continue
        fi

        # Only process git worktrees
        if ! _is_git_worktree "$entry"; then
            info "  skipping non-git-worktree: $name"
            continue
        fi

        if _matches_glob "$name" "$LANE_GLOB"; then
            lane_candidates+=("$entry")
        else
            orphan_candidates+=("$entry")
        fi
    done

    # ── Pass 1: reset reclaimable lanes ───────────────────────────────────────
    local lane
    for lane in "${lane_candidates[@]+${lane_candidates[@]}}"; do
        name="$(basename "$lane")"
        local lane_lock="${WORKTREES_DIR}/${name}.lock"

        # Acquire the lane lock NON-BLOCKING in the PARENT shell so the same
        # file description (and advisory lock) spans the reclaimability check
        # AND the seed-script call — no check→act race window.
        # Mirror: refresh-warm-base.sh §GC (flock held across the rm).
        exec 8>"$lane_lock"
        if ! flock -n 8; then
            exec 8>&-
            warn "preserving $name: live consumer (flock held)"
            preserved_count=$((preserved_count + 1))
            continue
        fi

        # Reclaimability check (under the lock).
        if ! _is_reclaimable "$lane"; then
            exec 8>&-
            preserved_count=$((preserved_count + 1))
            continue
        fi

        # Invoke α while the lane lock is held in the parent shell.
        # The action subshell inherits FD 8; the parent still owns the lock.
        # Also hold flock -s on the gen lock (D8 reader-refcount seam).
        info "  resetting lane: $name"
        if (
            exec 9>"$gen_lock"
            flock -s 9
            "$SEED_SCRIPT" "$resolved_gen" "$lane" --fresh-checkout
        ) 2>&1 | while IFS= read -r line; do warn "  [seed] $line"; done; then
            ok "  reset lane: $name"
            reset_count=$((reset_count + 1))
        else
            warn "  reset failed for $name (seed-script error); continuing"
            preserved_count=$((preserved_count + 1))
        fi
        exec 8>&-  # release lane lock; NOT removed — persists as per-lane mutex
    done

    # ── Pass 2: remove reclaimable orphans ────────────────────────────────────
    local orphan
    for orphan in "${orphan_candidates[@]+${orphan_candidates[@]}}"; do
        name="$(basename "$orphan")"
        local orphan_lock="${WORKTREES_DIR}/${name}.lock"

        # Same single-acquisition pattern as Pass 1: non-blocking exclusive
        # acquire in the parent shell held across reclaimability check + remove.
        exec 8>"$orphan_lock"
        if ! flock -n 8; then
            exec 8>&-
            warn "preserving $name: live consumer (flock held)"
            preserved_count=$((preserved_count + 1))
            continue
        fi

        # Reclaimability check (under the lock).
        if ! _is_reclaimable "$orphan"; then
            exec 8>&-
            preserved_count=$((preserved_count + 1))
            continue
        fi

        # Determine the primary worktree to run git worktree remove from.
        # Use awk (not grep|head|cut) to avoid SIGPIPE under set -o pipefail:
        # head -n1 closes the pipe early, which can deliver SIGPIPE (141) to
        # git/grep and propagate a spurious non-zero status via pipefail.
        local primary
        primary="$(git -C "$orphan" worktree list --porcelain 2>/dev/null \
            | awk '/^worktree /{print substr($0,10); exit}')" || {
            exec 8>&-
            warn "  cannot determine primary worktree for $name; skipping"
            preserved_count=$((preserved_count + 1))
            continue
        }

        info "  removing orphan worktree: $name (primary=$primary)"
        if (
            git -C "$primary" worktree remove --force "$orphan"
        ); then
            ok "  removed orphan: $name"
            removed_count=$((removed_count + 1))
            # Orphan lock file is cleaned up on success: once the worktree slot
            # no longer exists, the lock file has no future coordination role.
            # Lane lock files (Pass 1) intentionally persist across sweeps as
            # permanent per-lane mutexes for consumer coordination (see inv.2).
            rm -f "$orphan_lock" 2>/dev/null || true
        else
            warn "  remove failed for $name; continuing"
            preserved_count=$((preserved_count + 1))
        fi
        exec 8>&-  # release orphan lock
    done

    # ── Summary ───────────────────────────────────────────────────────────────
    printf 'reclaim: reset=%d removed=%d preserved=%d\n' \
        "$reset_count" "$removed_count" "$preserved_count"
    ok "reclaim complete: reset=$reset_count removed=$removed_count preserved=$preserved_count"
}

# ── dispatch ───────────────────────────────────────────────────────────────────
case "$SUBCOMMAND" in
    reclaim)
        _do_reclaim
        ;;
esac
