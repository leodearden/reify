#!/usr/bin/env bash
# tests/infra/test_lane_task_status.sh
# Hermetic tests for scripts/lane-task-status.sh — the first-party backing-task
# status oracle for the warm-lane leak machinery (REIFY_LANE_LEAK_STATUS_CMD
# contract; task 5177 wiring of the 4749 seam).
#
# Fully hermetic: builds its own throwaway Taskmaster-shaped SQLite fixture in a
# mktemp dir (via python3's stdlib sqlite3) and points the oracle at it with
# REIFY_LANE_TASK_DB, so it NEVER reads the real .taskmaster store. No PATH
# stubbing needed — the oracle's own read engines (sqlite3 CLI / python3) are
# exercised against the fixture.
#
# run_oracle captures:
#   OUT — captured stdout (the printed status, whitespace-trimmed by the script)
#   RC  — exit code (the oracle's contract: ALWAYS 0)
#
# Blocks:
#   A — terminal statuses resolve verbatim (done, cancelled)
#   B — non-terminal statuses resolve verbatim (pending, in-progress, blocked,
#       deferred, infra-hold)
#   C — fail-safe: every error path => empty stdout, exit 0 (nonexistent id,
#       non-numeric id, no arg, missing DB, empty 0-byte stub DB)
#   D — tag isolation (REIFY_LANE_TASK_TAG); wrong tag => empty
#   E — output hygiene: exactly "<status>\n", no leading/trailing spaces
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; classified
# `pool` (hermetic) in tests/infra/run-all-classification.manifest.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/lane-task-status.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/lane-task-status.sh hermetic tests (task 5177) ==="

[ -x "$SCRIPT" ] || {
    echo "ERROR: $SCRIPT not found or not executable"
    exit 1
}
command -v python3 >/dev/null 2>&1 || {
    echo "ERROR: python3 required to build the hermetic fixture DB"
    exit 1
}

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do
        rm -rf "$d" 2>/dev/null || true
    done
}
trap cleanup EXIT

WORK="$(mktemp -d "${TMPDIR:-/tmp}/reify-lane-task-status.XXXXXX")"
_TMPDIRS+=("$WORK")

DB="$WORK/tasks.db"

# Build the fixture: a `tasks` table mirroring the real Taskmaster schema's
# load-bearing columns (tag, id, status + PRIMARY KEY(tag, id)). Two tags so
# Block D can prove tag isolation. Statuses cover both terminal and every
# non-terminal value the real store uses.
python3 - "$DB" <<'PY'
import sqlite3, sys
con = sqlite3.connect(sys.argv[1])
con.execute(
    "CREATE TABLE tasks (tag TEXT NOT NULL, id INTEGER NOT NULL, "
    "title TEXT, status TEXT NOT NULL, PRIMARY KEY (tag, id))"
)
rows = [
    ("master", 100, "done"),
    ("master", 101, "cancelled"),
    ("master", 102, "pending"),
    ("master", 103, "in-progress"),
    ("master", 104, "blocked"),
    ("master", 105, "deferred"),
    ("master", 106, "infra-hold"),
    # id 200 exists ONLY under the 'other' tag — Block D uses it to prove that a
    # lookup under the default 'master' tag must NOT find it.
    ("other", 200, "done"),
]
con.executemany(
    "INSERT INTO tasks (tag, id, title, status) VALUES (?, ?, 'x', ?)", rows
)
con.commit()
con.close()
PY

# ── run helper: capture stdout + exit code (stderr merged away; oracle is
#    silent on stdout except the status). REIFY_LANE_TASK_DB pins the fixture. ──
run_oracle() {
    OUT="$(REIFY_LANE_TASK_DB="${_DB_OVERRIDE:-$DB}" \
           REIFY_LANE_TASK_TAG="${_TAG_OVERRIDE:-master}" \
           "$SCRIPT" "$@" 2>/dev/null)" && RC=0 || RC=$?
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — terminal statuses resolve verbatim
# ──────────────────────────────────────────────────────────────────────────────
echo "--- Block A: terminal statuses ---"

run_oracle 100
assert "A1: done task exits 0"        test "$RC" -eq 0
assert "A1: done task prints 'done'"  test "$OUT" = "done"

run_oracle 101
assert "A2: cancelled task exits 0"          test "$RC" -eq 0
assert "A2: cancelled task prints 'cancelled'" test "$OUT" = "cancelled"

# ──────────────────────────────────────────────────────────────────────────────
# Block B — non-terminal statuses resolve verbatim (consumers treat any non
# done/cancelled string as non-terminal => preserve the lane)
# ──────────────────────────────────────────────────────────────────────────────
echo "--- Block B: non-terminal statuses ---"

run_oracle 102
assert "B1: pending prints 'pending'"          test "$OUT" = "pending"
run_oracle 103
assert "B2: in-progress prints 'in-progress'"  test "$OUT" = "in-progress"
run_oracle 104
assert "B3: blocked prints 'blocked'"          test "$OUT" = "blocked"
run_oracle 105
assert "B4: deferred prints 'deferred'"        test "$OUT" = "deferred"
run_oracle 106
assert "B5: infra-hold prints 'infra-hold'"    test "$OUT" = "infra-hold"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — fail-safe: every error path => empty stdout, exit 0. This is the
# safety property: an unresolved lookup must degrade to unknown (preserve),
# NEVER to a spurious terminal status that would let the GC reclaim a live lane.
# ──────────────────────────────────────────────────────────────────────────────
echo "--- Block C: fail-safe empty output ---"

run_oracle 99999999
assert "C1: nonexistent id exits 0"      test "$RC" -eq 0
assert "C1: nonexistent id prints empty" test -z "$OUT"

run_oracle abc
assert "C2: non-numeric id exits 0"      test "$RC" -eq 0
assert "C2: non-numeric id prints empty" test -z "$OUT"

run_oracle 12x3
assert "C3: partially-numeric id exits 0"      test "$RC" -eq 0
assert "C3: partially-numeric id prints empty" test -z "$OUT"

run_oracle
assert "C4: no arg exits 0"      test "$RC" -eq 0
assert "C4: no arg prints empty" test -z "$OUT"

_DB_OVERRIDE="$WORK/does-not-exist.db" run_oracle 100
assert "C5: missing DB exits 0"      test "$RC" -eq 0
assert "C5: missing DB prints empty" test -z "$OUT"

# Empty 0-byte stub (mirrors the top-level .taskmaster/tasks.db placeholder).
: > "$WORK/empty.db"
_DB_OVERRIDE="$WORK/empty.db" run_oracle 100
assert "C6: empty 0-byte DB exits 0"      test "$RC" -eq 0
assert "C6: empty 0-byte DB prints empty" test -z "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block D — tag isolation: id 200 lives only under tag 'other'.
# ──────────────────────────────────────────────────────────────────────────────
echo "--- Block D: tag isolation ---"

run_oracle 200
assert "D1: id under a different tag not found under 'master' (empty)" test -z "$OUT"
assert "D1: exits 0"                                                    test "$RC" -eq 0

_TAG_OVERRIDE="other" run_oracle 200
assert "D2: same id resolves under its own tag" test "$OUT" = "done"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — output hygiene: stdout is exactly "<status>\n" (one line, no
# surrounding whitespace). Consumers strip whitespace anyway, but keep it clean.
# ──────────────────────────────────────────────────────────────────────────────
echo "--- Block E: output hygiene ---"

RAW="$(REIFY_LANE_TASK_DB="$DB" "$SCRIPT" 102 2>/dev/null | od -An -c | tr -s ' ')"
assert "E1: pending output is exactly 'p e n d i n g \\n'" \
    bash -c '[ "$1" = " p e n d i n g \n" ]' _ "$RAW"

LINES="$(REIFY_LANE_TASK_DB="$DB" "$SCRIPT" 100 2>/dev/null | wc -l | tr -d ' ')"
assert "E2: single line of output" test "$LINES" = "1"

test_summary
