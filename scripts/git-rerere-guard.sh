#!/usr/bin/env bash
# scripts/git-rerere-guard.sh — Keep git rerere disabled repo-wide, and detect
# the stuck-lane signature it leaves behind when it is not.
#
# WHY: `.git/rr-cache` is a git COMMON path — `git rev-parse --git-path rr-cache`
# resolves to the COMMON git dir from every linked worktree, while `MERGE_RR` and
# `index` resolve per-worktree.  Reify's ~238 warm lanes therefore share ONE
# rerere resolution cache, and git takes its only rerere lock on the PER-WORKTREE
# MERGE_RR — so that lock provides zero cross-worktree mutual exclusion over the
# shared payload directory.  Git exposes no knob to relocate rr-cache, so per-lane
# cache isolation is impossible by construction; the only sound fix is to disable
# rerere entirely.  Two hazards follow from leaving it on:
#
#   1. Silent cross-lane resolution bleed.  Lane A (task X) resolves a conflict
#      its way; lane B (unrelated task Y) later merges and git prints
#      `Staged '<f>' using previous resolution.` — lane B's tree now holds task
#      X's resolution, ALREADY STAGED under rerere.autoupdate=true, with no
#      conflict markers and a clean `git status`.
#   2. A failed rr-cache preimage write leaves a stale zero-byte MERGE_RR.lock,
#      after which every `git commit` in that lane exits 128 — while the commit
#      object is still written and the ref still moves.
#
# See docs/notes/git-rerere-shared-worktree-hazard.md (task 5870, esc-5785-5).
#
# Usage:
#   scripts/git-rerere-guard.sh <subcommand> [target_dir]
#
#   check       Report whether rerere is effectively ARMED for the target store.
#               Read-only: never writes config anywhere.
#               Exit 0 = safe, 1 = armed (the machine-readable signal).
#   arm         Idempotently write rerere.enabled=false and rerere.autoupdate=false
#               to the SHARED local config, then re-verify via `check`.
#               NEVER deletes or prunes rr-cache — see below.
#   scan-locks  Read-only census of stale MERGE_RR.lock files across the store's
#               linked worktrees.  Prints the remediation command; never runs it.
#               Exit 0 = clean, 1 = at least one lock found.
#
#   target_dir  Optional path to a git work tree inside the store to operate on.
#               Defaults to the repo root (one level up from this script).
#               Any worktree of the store resolves to the same shared config and
#               the same rr-cache, so the main checkout and any linked lane are
#               interchangeable here.
#
# Idempotent.  All diagnostics go to stderr; nothing is written to stdout.
#
# THE GOTCHA that makes a one-time `git config` write insufficient: git's default
# for `rerere.enabled` is -1, meaning "enabled iff rr-cache/ exists".  With the
# key UNSET and the residual rr-cache/ still on disk, rerere is silently ON for
# the whole fleet.  So the explicit `false` must be present, not merely absent —
# `git config --unset rerere.enabled` is a silent RE-ARM, not a no-op.  That is
# why this ships as a re-runnable guard with a `check` mode rather than a
# one-shot write, and why `arm` never prunes rr-cache: the explicit false
# neutralises the residual cache in place, while deleting an rr-cache entry that
# another live worktree still holds is precisely the operation that reproduces
# the segfault + stale-lock signature.

set -euo pipefail

_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
    cat >&2 <<'USAGE'
Usage: git-rerere-guard.sh <subcommand> [target_dir]

Subcommands:
  check       Report whether git rerere is effectively armed for the target
              store (read-only).  Exit 0 = safe, 1 = armed.
  arm         Idempotently disable rerere in the shared local config, then
              re-verify.  Never deletes rr-cache.
  scan-locks  Read-only census of stale MERGE_RR.lock files across the store's
              linked worktrees.  Exit 0 = clean, 1 = lock(s) found.

  target_dir  Optional git work tree inside the store; defaults to the repo
              root (one level up from this script).

All diagnostics go to stderr.  See
docs/notes/git-rerere-shared-worktree-hazard.md for the mechanism and the
recovery procedure.
USAGE
}

# ── argument parsing ──────────────────────────────────────────────────────────

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    usage
    exit 0
fi

SUBCOMMAND="${1:-}"

if [ -z "$SUBCOMMAND" ]; then
    echo "ERROR: no subcommand given." >&2
    usage
    exit 1
fi

case "$SUBCOMMAND" in
    check|arm|scan-locks) ;;
    *)
        echo "ERROR: unknown subcommand: $SUBCOMMAND" >&2
        usage
        exit 1
        ;;
esac

shift

if [ $# -gt 1 ]; then
    echo "ERROR: too many arguments." >&2
    usage
    exit 1
fi

TARGET="${1:-"$(cd "$_SCRIPT_DIR/.." && pwd)"}"

if [ ! -d "$TARGET" ]; then
    echo "ERROR: target directory does not exist: $TARGET" >&2
    exit 1
fi

if ! git -C "$TARGET" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "ERROR: not a git work tree: $TARGET" >&2
    exit 1
fi

# ── resolve the SHARED store ──────────────────────────────────────────────────
# --git-common-dir, not --git-dir: from a linked worktree --git-dir points at
# .git/worktrees/<name>/ (which holds the per-worktree MERGE_RR), while the
# shared config and rr-cache both live in the COMMON dir.  Resolving through the
# common dir is what makes this script behave identically whether it is invoked
# from the main checkout or from any of the ~238 lanes.

COMMON_DIR="$(git -C "$TARGET" rev-parse --git-common-dir)"
case "$COMMON_DIR" in
    /*) ;;
    *)  COMMON_DIR="$(cd "$TARGET" && cd "$COMMON_DIR" && pwd)" ;;
esac

RR_CACHE="$COMMON_DIR/rr-cache"
WORKTREES_DIR="$COMMON_DIR/worktrees"

# ── subcommand implementations ────────────────────────────────────────────────

cmd_check() {
    echo "ERROR: check is not implemented yet" >&2
    return 1
}

cmd_arm() {
    echo "ERROR: arm is not implemented yet" >&2
    return 1
}

cmd_scan_locks() {
    echo "ERROR: scan-locks is not implemented yet" >&2
    return 1
}

case "$SUBCOMMAND" in
    check)      cmd_check ;;
    arm)        cmd_arm ;;
    scan-locks) cmd_scan_locks ;;
esac
