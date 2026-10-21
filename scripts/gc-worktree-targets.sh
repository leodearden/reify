#!/usr/bin/env bash
# gc-worktree-targets.sh — Remove the target/ dir of every worktree under the
# sweep root that has not been built in the last MIN_AGE_HOURS.
#
# Usage: gc-worktree-targets.sh [MIN_AGE_HOURS]   (default 6)
#   DRY_RUN=1                  report what would be removed; remove nothing.
#   REIFY_GC_WORKTREE_DIR=DIR  sweep root (default /home/leo/src/reify/.worktrees).
#
# Designed to run via cron while the orchestrator is active, so it only
# removes what no build is using:
#   - idle: a target whose own mtime is within MIN_AGE_HOURS is left alone;
#   - lane lock: each removal runs under `flock -n` on <worktree>.lock, the
#     lane lock dark_factory holds across a lane's build and verify (its
#     verify_cancel.lane_lock_path convention); a held lock skips that target
#     until the next run;
#   - _merge-verify is exempt: reify-merge-verify-trim.timer trims its
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

  # Skip if target was modified recently (active build)
  if find "$target" -maxdepth 0 -mmin "-$((MIN_AGE_HOURS * 60))" -print -quit | grep -q .; then
    continue
  fi

  size=$(du -sm "$target" 2>/dev/null | cut -f1)

  if [ -n "$DRY_RUN" ]; then
    echo "[dry-run] would remove $target (${size}MB)"
    total_freed=$((total_freed + size))
    continue
  fi

  rc=0
  flock -n -E "$LANE_LOCK_HELD_RC" "$wt.lock" rm -rf -- "$target" || rc=$?
  case "$rc" in
    0)
      echo "removed $target (${size}MB)"
      total_freed=$((total_freed + size)) ;;
    "$LANE_LOCK_HELD_RC")
      echo "lane lock held, skipped $target" ;;
    *)
      echo "ERROR: removing $target failed (rc=$rc)" >&2
      exit 1 ;;
  esac
done

echo "total freed: ${total_freed}MB"
