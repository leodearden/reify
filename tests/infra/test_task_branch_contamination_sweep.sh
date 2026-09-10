#!/usr/bin/env bash
# tests/infra/test_task_branch_contamination_sweep.sh
# Hermetic tests for scripts/task-branch-contamination-sweep.sh (task #7244).
#
# The SUT is a read-only, non-gating audit primitive with two modes:
#   --task <id>   single branch (the per-merge advisory consult seam)
#   --audit       fleet sweep
# It reads a Taskmaster store and a git repo and prints a report. It must
# never write to either, and must exit 0 on every valid invocation in both
# modes — 2 is reserved for usage errors, so no caller can gate on its status
# by accident.
#
# run_helper captures STDOUT, STDERR and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks (added incrementally across task #7244's TDD steps):
#   step-5  — arg-parsing / usage taxonomy
#   step-7  — the batched task-store read (enumeration, declarations, tag
#             scoping, engine interchangeability, degraded-store fallback)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/task-branch-contamination-sweep.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/task-branch-contamination-sweep.sh hermetic tests (task 7244) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp "${TMPDIR:-/tmp}/test-task-branch-sweep-err-XXXXXX")"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ────────────────────────────────────────────────────────────────
# Invokes the script under test with no PATH stub.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# A missing SUT cannot degrade into a partial report: every assertion below
# invokes it. Report the absence as one FAIL line (so run_all.sh still gets a
# parseable Results line) and stop.
if [ ! -f "$SCRIPT" ]; then
    assert "scripts/task-branch-contamination-sweep.sh exists" test -f "$SCRIPT"
    test_summary
    exit 1
fi

# ── usage-error assertion bundle ──────────────────────────────────────────────
# Every usage error must satisfy all three halves of the contract at once:
# exit 2, a diagnostic on stderr, and NOTHING on stdout. Bundling them means a
# new usage case cannot accidentally assert only the exit code — the stdout
# half is the one that matters most here, because stdout is the SUT's only
# result channel and a caller parsing it must never see a half-written report.
assert_usage_error() {
    local desc="$1"; shift
    run_helper "$@"
    assert "U[$desc]: exits 2" test "$RC" -eq 2
    assert "U[$desc]: writes a diagnostic to stderr" test -n "$ERR_OUT"
    assert "U[$desc]: writes NOTHING to stdout" test -z "$OUT"
}

# ─────────────────────────────────────────────────────────────────────────────
# Block 1 (step-5) — arg-parsing / usage taxonomy
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 1: arg parsing and usage taxonomy ---"

assert_usage_error "unknown flag" --audit --no-such-flag
assert_usage_error "unknown short flag" --audit -Z

# Every value-taking flag, checked for the "given as the last argument with no
# value" shape. Enumerated one by one rather than looped over, so a flag that
# silently stops taking a value shows up as its own named FAIL.
assert_usage_error "--db without a value"            --audit --db
assert_usage_error "--tag without a value"           --audit --tag
assert_usage_error "--repo without a value"          --audit --repo
assert_usage_error "-C without a value"              --audit -C
assert_usage_error "--main-ref without a value"      --audit --main-ref
assert_usage_error "--branch-prefix without a value" --audit --branch-prefix
assert_usage_error "--format without a value"        --audit --format
assert_usage_error "--task without a value"          --task

assert_usage_error "invalid --format value"   --audit --format yaml
assert_usage_error "empty --format value"     --audit --format ''

assert_usage_error "both --task and --audit"  --task 1 --audit
assert_usage_error "neither --task nor --audit"

assert_usage_error "non-numeric --task"       --task abc
assert_usage_error "negative --task"          --task -1
assert_usage_error "mixed alnum --task"       --task 12x
assert_usage_error "empty --task"             --task ''

assert_usage_error "unexpected positional argument" --audit extra
assert_usage_error "bare positional argument"       12345

# ── help ──────────────────────────────────────────────────────────────────────
# Usage goes to STDERR and exits 0, matching the
# warm-lane-degenerate-ref-check.sh precedent: stdout is reserved for the
# report, so even --help must not put a byte on it.
for _h in -h --help; do
    run_helper "$_h"
    assert "H[$_h]: exits 0" test "$RC" -eq 0
    assert "H[$_h]: prints usage to stderr" \
        bash -c 'printf "%s\n" "$1" | grep -qi "usage:"' _ "$ERR_OUT"
    assert "H[$_h]: prints NOTHING to stdout" test -z "$OUT"
done

# --help wins over an otherwise-invalid invocation, so a confused caller gets
# the usage text rather than a bare exit 2.
run_helper --help --no-such-flag
assert "H[--help beats a later bad flag]: exits 0" test "$RC" -eq 0

# ─────────────────────────────────────────────────────────────────────────────
# Fixture builders
#
# CONVENTION (copied from tests/infra/test_deterministic_gate_closure_
# staleness_sweep.sh): _mk_tasks_db and _mk_repo SET A GLOBAL (DB / REPO)
# rather than echoing a path, so they must be called directly — never in a
# command substitution, which would run them in a subshell and discard the
# assignment.
# ─────────────────────────────────────────────────────────────────────────────

# _sq <args...> — the sqlite3 CLI with LD_LIBRARY_PATH cleared.
#
# EVERY sqlite3 invocation in this file must go through this wrapper. Under the
# merge gate, verify.sh's apply_env() exports LD_LIBRARY_PATH=/opt/reify-deps/
# lib for OCCT, and that directory ships a conda libsqlite3 NEWER than the one
# /usr/bin/sqlite3 was linked against, so the CLI aborts with "SQLite header
# and source version mismatch" and takes the whole suite down under `set -e`.
# Same hazard and same fix as that suite's wrapper — see its header for the
# full history.
_sq() { LD_LIBRARY_PATH="" sqlite3 "$@"; }

# _mk_tasks_db — build a fresh temp Taskmaster store; sets global DB.
# DDL is the production schema verbatim: tag-scoped with PRIMARY KEY (tag, id),
# which is what makes the SUT's tag-scoping discipline testable (T5).
_mk_tasks_db() {
    local d
    d="$(mktemp -d "${TMPDIR:-/tmp}/task-branch-sweep-db-XXXXXX")"
    _TMPDIRS+=("$d")
    DB="$d/tasks.db"
    _sq "$DB" "
CREATE TABLE IF NOT EXISTS \"tasks\" (
    tag           TEXT NOT NULL DEFAULT 'master',
    id            INTEGER NOT NULL,
    title         TEXT NOT NULL,
    description   TEXT,
    details       TEXT,
    test_strategy TEXT,
    status        TEXT NOT NULL,
    priority      TEXT,
    metadata      TEXT,
    updated_at    TEXT NOT NULL, claimant_run_id TEXT, heartbeat_at TEXT, candidate_key TEXT,
    PRIMARY KEY (tag, id)
);"
}

# _sq_quote <s> — single-quote a value for direct SQL interpolation.
_sq_quote() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/''/g")"; }

# _add_task <id> <status> <metadata_json> [tag]
# An EMPTY <metadata_json> inserts SQL NULL — the shape a row carries when the
# store has never recorded metadata for it, which must not be confused with
# the JSON literal 'null' or with a malformed blob.
_add_task() {
    local id="$1" status="$2" metadata="${3:-}" tag="${4:-master}" meta_sql
    if [ -n "$metadata" ]; then meta_sql="$(_sq_quote "$metadata")"; else meta_sql="NULL"; fi
    _sq "$DB" "INSERT INTO tasks (tag,id,title,status,metadata,updated_at)
        VALUES ($(_sq_quote "$tag"),$id,'fixture task $id',$(_sq_quote "$status"),$meta_sql,
                '2026-09-10T00:00:00Z');"
}

# _mk_repo — hermetic `git init -b main` repo with one commit; sets global REPO.
_mk_repo() {
    local d
    d="$(mktemp -d "${TMPDIR:-/tmp}/task-branch-sweep-repo-XXXXXX")"
    _TMPDIRS+=("$d")
    REPO="$d/repo"
    git init -q -b main "$REPO"
    git -C "$REPO" config user.email "test@test.local"
    git -C "$REPO" config user.name "Test"
    git -C "$REPO" commit -q --allow-empty -m "initial"
}

# _branch_at <name> <start-point> — create a branch without checking it out.
_branch_at() { git -C "$REPO" branch -q "$1" "$2"; }

# _commit_files <branch> <subject> <path>... — check out <branch>, create or
# touch each <path> (parent dirs included), and commit them under <subject>.
_commit_files() {
    local branch="$1" subject="$2"; shift 2
    local f
    git -C "$REPO" checkout -q "$branch"
    for f in "$@"; do
        mkdir -p "$REPO/$(dirname "$f")"
        printf 'content for %s\n' "$f" >> "$REPO/$f"
        git -C "$REPO" add -- "$f"
    done
    git -C "$REPO" commit -q -m "$subject"
    git -C "$REPO" checkout -q main
}

# ── report accessors ──────────────────────────────────────────────────────────
# Rows are space-separated `key=value` pairs, so a field is extractable without
# a parser. No emitted value contains a space (paths appear only as COUNTS, and
# `peers` is comma-separated), which is what keeps that true.

# _row <task_id> — print the row for <task_id> from $OUT, or nothing.
_row() { printf '%s\n' "$OUT" | grep -E "^task=$1( |$)" || true; }

# _field <row> <key> — print <key>'s value from a row.
_field() { printf '%s\n' "$1" | tr ' ' '\n' | sed -n "s/^$2=//p"; }

# _assert_field <desc> <task_id> <key> <expected>
_assert_field() {
    local desc="$1" id="$2" key="$3" expected="$4"
    assert "$desc" bash -c '[ "$(printf "%s\n" "$1" | tr " " "\n" | sed -n "s/^$2=//p")" = "$3" ]' \
        _ "$(_row "$id")" "$key" "$expected"
}

# _assert_no_row <desc> <task_id>
_assert_no_row() {
    assert "$1" bash -c '[ -z "$1" ]' _ "$(_row "$2")"
}

# _has_summary — true iff $OUT carries a SWEEP: summary line.
_has_summary() { printf '%s\n' "$OUT" | grep -q '^SWEEP:'; }

# ─────────────────────────────────────────────────────────────────────────────
# Block 2 (step-7) — the batched task-store read
#
# ONE tag-scoped query per invocation supplies every row's status and declared
# file list. These assertions pin what that query must and must not return;
# the columns they read through (scope/foreign) are the report's own, so no
# introspection seam is needed.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 2: batched task-store read ---"

_mk_tasks_db
_mk_repo
T2_DB="$DB"

# Five non-terminal statuses and both terminal ones, each with a branch that
# carries one commit touching exactly its declared file.
_add_task 9101 pending      '{"files":["a.rs"]}'
_add_task 9102 in-progress  '{"files":["b.rs"]}'
_add_task 9103 blocked      '{"files":["c.rs"]}'
_add_task 9104 deferred     '{"files":["d.rs"]}'
_add_task 9105 infra-hold   '{"files":["e.rs"]}'
_add_task 9106 done         '{"files":["f.rs"]}'
_add_task 9107 cancelled    '{"files":["g.rs"]}'
# Awkward but legal repo-relative paths: one with a space, one with a '#'.
_add_task 9108 pending      '{"files":["dir with space/x.rs","hash#name.rs"]}'
# The three degraded-metadata shapes, which must all read as NO declaration.
_add_task 9109 pending      '{"files":[  not json'
_add_task 9110 pending      ''
_add_task 9111 pending      '{"other_key":1}'

MAIN_TIP="$(git -C "$REPO" rev-parse main)"
_i=0
for _spec in "9101 a.rs" "9102 b.rs" "9103 c.rs" "9104 d.rs" "9105 e.rs" \
             "9106 f.rs" "9107 g.rs" "9109 i.rs" "9110 j.rs" "9111 k.rs"; do
    set -- $_spec
    _branch_at "task/$1" "$MAIN_TIP"
    _commit_files "task/$1" "feat($1): touch $2" "$2"
done
_branch_at "task/9108" "$MAIN_TIP"
_commit_files "task/9108" "feat(9108): awkward paths" "dir with space/x.rs" "hash#name.rs"

run_helper --audit --db "$T2_DB" --repo "$REPO"
assert "T0: a valid --audit invocation exits 0" test "$RC" -eq 0
assert "T0: a valid --audit invocation emits a SWEEP: summary" _has_summary

# (a) non-terminal enumerated, terminal excluded
for _spec in "9101 pending" "9102 in-progress" "9103 blocked" \
             "9104 deferred" "9105 infra-hold"; do
    set -- $_spec
    _assert_field "T1[$2]: task $1 is enumerated with its status" "$1" status "$2"
done
_assert_no_row "T1: a 'done'-backed branch is NOT enumerated"      9106
_assert_no_row "T1: a 'cancelled'-backed branch is NOT enumerated" 9107

# (b) exact declared paths, including a space and a '#'
_assert_field "T2: a declared path containing a space matches exactly (foreign=0)" \
    9108 foreign 0
_assert_field "T2: a declared path containing a '#' matches exactly (scope=CLEAN)" \
    9108 scope CLEAN

# (c) every degraded-metadata shape reads as NO declaration, and does not abort
_assert_field "T3: malformed-JSON metadata degrades to no declaration" \
    9109 scope UNDECLARED
_assert_field "T3: SQL NULL metadata degrades to no declaration" \
    9110 scope UNDECLARED
_assert_field "T3: metadata without a 'files' key degrades to no declaration" \
    9111 scope UNDECLARED
_assert_field "T3: a degraded row does not stop the others reporting" \
    9101 scope CLEAN

# (e) tag scoping, in BOTH directions
_mk_tasks_db
_mk_repo
T5_DB="$DB"
_add_task 9201 pending '{"files":["m.rs"]}' master
_add_task 9201 pending '{"files":["m.rs"]}' other
_add_task 9202 pending '{"files":["n.rs"]}' other
_branch_at "task/9201" "$(git -C "$REPO" rev-parse main)"
_commit_files "task/9201" "feat(9201): m" "m.rs"
_branch_at "task/9202" "$(git -C "$REPO" rev-parse main)"
_commit_files "task/9202" "feat(9202): n" "n.rs"

run_helper --audit --db "$T5_DB" --repo "$REPO" --tag master
assert "T5: --tag master sees its own row" test -n "$(_row 9201)"
_assert_no_row "T5: --tag master does NOT see a row that exists only under 'other'" 9202

run_helper --audit --db "$T5_DB" --repo "$REPO" --tag other
assert "T5: --tag other sees the row that exists only under 'other'" test -n "$(_row 9202)"
assert "T5: --tag other also sees the id present under both tags" test -n "$(_row 9201)"

# (d) degraded stores: zero branches, summary still emitted, exit 0
T4_MISSING="$(mktemp -d "${TMPDIR:-/tmp}/task-branch-sweep-nodb-XXXXXX")"
_TMPDIRS+=("$T4_MISSING")

run_helper --audit --db "$T4_MISSING/absent.db" --repo "$REPO"
assert "T4[missing db]: exits 0"              test "$RC" -eq 0
assert "T4[missing db]: still emits a summary" _has_summary
assert "T4[missing db]: emits no branch rows"  bash -c '! printf "%s\n" "$1" | grep -q "^task="' _ "$OUT"
assert "T4[missing db]: warns on stderr"       test -n "$ERR_OUT"

: > "$T4_MISSING/empty.db"
run_helper --audit --db "$T4_MISSING/empty.db" --repo "$REPO"
assert "T4[0-byte db]: exits 0"               test "$RC" -eq 0
assert "T4[0-byte db]: still emits a summary"  _has_summary
assert "T4[0-byte db]: emits no branch rows"   bash -c '! printf "%s\n" "$1" | grep -q "^task="' _ "$OUT"

cp "$T5_DB" "$T4_MISSING/unreadable.db"
chmod 000 "$T4_MISSING/unreadable.db"
run_helper --audit --db "$T4_MISSING/unreadable.db" --repo "$REPO"
assert "T4[unreadable db]: exits 0"              test "$RC" -eq 0
assert "T4[unreadable db]: still emits a summary" _has_summary
assert "T4[unreadable db]: emits no branch rows"  bash -c '! printf "%s\n" "$1" | grep -q "^task="' _ "$OUT"
chmod 644 "$T4_MISSING/unreadable.db"

# (f) the two SQL engines are interchangeable, not one a stub.
# An explicitly EMPTY REIFY_TASK_BRANCH_SWEEP_SQLITE_BIN forces python3.
run_helper --audit --db "$T2_DB" --repo "$REPO"
T6_SQLITE_OUT="$OUT"
T6_ERR_FILE="$ERR_FILE"
T6_PY_OUT="$(REIFY_TASK_BRANCH_SWEEP_SQLITE_BIN="" bash "$SCRIPT" \
    --audit --db "$T2_DB" --repo "$REPO" 2>"$T6_ERR_FILE")" || T6_PY_OUT="<failed rc=$?>"
assert "T6: the python3 engine produces byte-identical output to the sqlite3 CLI" \
    bash -c '[ "$1" = "$2" ] && [ -n "$1" ]' _ "$T6_SQLITE_OUT" "$T6_PY_OUT"

test_summary
