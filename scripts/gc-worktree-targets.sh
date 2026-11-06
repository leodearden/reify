#!/usr/bin/env bash
# gc-worktree-targets.sh — Remove the target/ dir of every worktree under the
# sweep root that has not been built in the last MIN_AGE_HOURS.
#
# Usage: gc-worktree-targets.sh [MIN_AGE_HOURS]   (default 6)
#   DRY_RUN=1                  report what would be removed; remove nothing.
#   REIFY_GC_WORKTREE_DIR=DIR  sweep root (default /home/leo/src/reify/.worktrees).
#
# Designed to run via cron while the orchestrator is active. A target is kept
# when any of these holds:
#   - recent: target/'s own mtime is within MIN_AGE_HOURS. A rebuild that only
#     rewrites files below target/debug/ leaves that mtime alone, so this is a
#     weak signal of an active build;
#   - locked: <worktree>.lock is held. That is dark_factory's lane lock (its
#     verify_cancel.lane_lock_path convention), which covers a lane's reseed
#     and verify spans only, never an implement-phase build
#     (docs/prds/warm-lane-pool-space-safety.md §8.4). The recency check is
#     repeated once the lock is taken, so a verify that finished between the
#     first check and the lock keeps its target;
#   - _merge-verify: exempt by name. reify-merge-verify-trim.timer trims its
#     persistent target in place (scripts/prune-unconsulted-target.sh), and
#     wiping it whole forces a ~2h cold build.
#
# Self-contained: dark_factory's install_laptop_merge_verify_trim.sh copies
# this file alone onto leo-laptop.

set -euo pipefail

WORKTREE_DIR="${REIFY_GC_WORKTREE_DIR:-/home/leo/src/reify/.worktrees}"
MIN_AGE_HOURS="${1:-6}"
DRY_RUN="${DRY_RUN:-}"
readonly LANE_LOCK_HELD_RC=75

built_recently() {
  [ -n "$(find "$1" -maxdepth 0 -mmin "-$((MIN_AGE_HOURS * 60))" -print -quit)" ]
}

# Removes worktree $1's target/ while holding its lane lock, adding the MB
# freed to total_freed. Exits the script on a lock or removal failure.
remove_target_under_lane_lock() {
  local target="$1/target" lock="$1.lock" lock_fd rc=0 size
  if ! exec {lock_fd}>>"$lock"; then
    echo "ERROR: cannot open lane lock $lock; $target left in place" >&2
    exit 1
  fi
  flock -n -x -E "$LANE_LOCK_HELD_RC" "$lock_fd" || rc=$?
  case "$rc" in
    0)
      if built_recently "$target"; then
        echo "built since the idle check, skipped $target"
      else
        size=$(du -sm "$target" 2>/dev/null | cut -f1)
        if ! rm -rf -- "$target"; then
          echo "ERROR: removing $target failed" >&2
          exit 1
        fi
        echo "removed $target (${size}MB)"
        total_freed=$((total_freed + size))
      fi ;;
    "$LANE_LOCK_HELD_RC")
      echo "lane lock held, skipped $target" ;;
    *)
      echo "ERROR: locking $lock failed (flock rc=$rc); $target left in place" >&2
      exit 1 ;;
  esac
  exec {lock_fd}>&-
}

if [ ! -d "$WORKTREE_DIR" ]; then
  echo "No worktree dir at $WORKTREE_DIR"
  exit 0
fi

total_freed=0

for wt in "$WORKTREE_DIR"/*/; do
  [ -d "$wt" ] || continue
  wt="${wt%/}"
  target="$wt/target"
  [ -d "$target" ] || continue

  if [ "$(basename "$wt")" = "_merge-verify" ]; then
    echo "skipped $target: trimmed in place by reify-merge-verify-trim.timer"
    continue
  fi

  if built_recently "$target"; then
    continue
  fi

  if [ -n "$DRY_RUN" ]; then
    size=$(du -sm "$target" 2>/dev/null | cut -f1)
    echo "[dry-run] would remove $target (${size}MB)"
    total_freed=$((total_freed + size))
    continue
  fi

  remove_target_under_lane_lock "$wt"
done

echo "total freed: ${total_freed}MB"
