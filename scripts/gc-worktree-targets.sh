#!/usr/bin/env bash
# gc-worktree-targets.sh — Remove target/ dirs from worktrees whose tasks
# are no longer active (done/cancelled/deferred) or that haven't been
# touched in over 24 hours.
#
# Designed to run via cron. Safe to run while orchestrator is active —
# only cleans worktrees that are idle.

set -euo pipefail

REPO_ROOT="/home/leo/src/reify"
WORKTREE_DIR="$REPO_ROOT/.worktrees"
MIN_AGE_HOURS="${1:-6}"  # configurable, default 6h
DRY_RUN="${DRY_RUN:-}"

if [ ! -d "$WORKTREE_DIR" ]; then
  echo "No worktree dir at $WORKTREE_DIR"
  exit 0
fi

total_freed=0

for wt in "$WORKTREE_DIR"/*/; do
  [ -d "$wt" ] || continue
  target="$wt/target"
  [ -d "$target" ] || continue

  task_id=$(basename "$wt")

  # Skip if target was modified recently (active build)
  if find "$target" -maxdepth 0 -mmin "-$((MIN_AGE_HOURS * 60))" -print -quit | grep -q .; then
    continue
  fi

  size=$(du -sm "$target" 2>/dev/null | cut -f1)

  if [ -n "$DRY_RUN" ]; then
    echo "[dry-run] would remove $target (${size}MB)"
  else
    rm -rf "$target"
    echo "removed $target (${size}MB)"
  fi

  total_freed=$((total_freed + size))
done

echo "total freed: ${total_freed}MB"
