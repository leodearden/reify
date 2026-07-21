#!/usr/bin/env bash
# scripts/lane-task-status.sh — first-party backing-task status oracle for the
# warm-lane leak machinery (the REIFY_LANE_LEAK_STATUS_CMD contract).
#
# Contract (warm-lane-preflight.sh Check 6 / warm-lane-audit.sh --status-cmd /
# warm-lane-gc.sh --status-cmd, all byte-for-byte identical):
#   Invoked as `<cmd> <task_id>`. Prints that task's Taskmaster status
#   (e.g. done, cancelled, pending, in-progress, blocked, deferred) to stdout
#   and exits 0. A non-zero exit OR empty output is treated by every consumer
#   as UNKNOWN / non-terminal — the lane is preserved, never reclaimed. Only
#   `done` / `cancelled` are terminal (a leaked lane whose task already finished).
#
# FAIL-SAFE BY CONSTRUCTION: every error path degrades to empty output. The one
# genuinely dangerous outcome would be reporting `done`/`cancelled` for a task
# that is actually live (causing warm-lane-gc.sh Tier-3 to reclaim an in-use
# lane). That can only ever happen if the canonical task store itself records
# the task terminal — this script never fabricates a terminal status, and a
# not-found id / read error / missing DB all yield empty output (=> unknown =>
# preserve). Live tasks (pending/in-progress/blocked/deferred/infra-hold) print
# their own non-terminal status, so their lanes are never reclaimed.
#
# Source of truth: the Taskmaster SQLite store at
#   $REIFY_LANE_TASK_DB (default: /home/leo/src/reify/.taskmaster/tasks/tasks.db)
# — the same store fused-memory's get_statuses proxies (verified identical for
# every live lane, 2026-07-20). Read strictly READ-ONLY (mode=ro / -readonly):
# no write, no WAL checkpoint, no lock contention with the live writers.
#
# Usage:
#   scripts/lane-task-status.sh <task_id>
#
# Environment (env defaults shown):
#   REIFY_LANE_TASK_DB    Path to the Taskmaster tasks.db
#                         (default: /home/leo/src/reify/.taskmaster/tasks/tasks.db).
#   REIFY_LANE_TASK_TAG   Taskmaster tag namespace (default: master). Task ids
#                         are unique only within a tag (PRIMARY KEY (tag, id)).
#
# Exit status: always 0 (advisory oracle; consumers read stdout, not exit code).

# NOT `set -e`: every failure must degrade to empty-output, never abort.
set -uo pipefail

DB="${REIFY_LANE_TASK_DB:-/home/leo/src/reify/.taskmaster/tasks/tasks.db}"
TAG="${REIFY_LANE_TASK_TAG:-master}"

id="${1:-}"

# Only a purely-numeric id is resolvable; anything else => unknown (empty out).
case "$id" in
    ''|*[!0-9]*) exit 0 ;;
esac

# Missing/empty DB => unknown (fail-safe: preserve). An empty 0-byte stub (the
# top-level .taskmaster/tasks.db placeholder) also degrades to empty here.
[ -s "$DB" ] || exit 0

status=""

# Engine 1 (fast path, ~8ms): the sqlite3 CLI, read-only. Prefer the stable
# system binary at an absolute path so this never depends on which sqlite3
# happens to be first on PATH; fall back to a PATH lookup.
_sqlite_bin=""
for _c in /usr/bin/sqlite3 sqlite3; do
    if command -v "$_c" >/dev/null 2>&1; then _sqlite_bin="$_c"; break; fi
done
if [ -n "$_sqlite_bin" ]; then
    status="$("$_sqlite_bin" -readonly "$DB" \
        "SELECT status FROM tasks WHERE tag='$TAG' AND id=$id LIMIT 1;" 2>/dev/null || true)"
fi

# Engine 2 (fallback): python3's stdlib sqlite3 with a read-only URI. Guaranteed
# present in the orchestrator runtime; covers a host with no sqlite3 CLI.
if [ -z "$status" ] && command -v python3 >/dev/null 2>&1; then
    status="$(REIFY_LANE_TASK_DB="$DB" REIFY_LANE_TASK_TAG="$TAG" _RID="$id" \
        python3 - "$id" <<'PY' 2>/dev/null || true
import os, sqlite3, sys
db = os.environ["REIFY_LANE_TASK_DB"]
tag = os.environ["REIFY_LANE_TASK_TAG"]
tid = int(sys.argv[1])
try:
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=2.0)
    row = con.execute(
        "SELECT status FROM tasks WHERE tag=? AND id=? LIMIT 1", (tag, tid)
    ).fetchone()
    con.close()
    if row and row[0]:
        sys.stdout.write(str(row[0]))
except Exception:
    pass
PY
)"
fi

# Trim any surrounding whitespace; print only a non-empty status. Consumers
# strip whitespace themselves (tr -d '[:space:]'), but keep stdout clean.
status="$(printf '%s' "$status" | tr -d '[:space:]')"
[ -n "$status" ] && printf '%s\n' "$status"
exit 0
