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
#   step-9  — per-branch git measurement and --task single mode
#   step-11 — the scope verdict and the declared-scope cross-check
#   step-13 — the commit-citation census and the signature column
#   step-15 — --audit fleet mode, the SWEEP: summary, and --format json
#   step-17 — the read-only and non-gating invariants (R1-R4), each with a
#             mutation-injection check proving the assertion can fail
#   step-22 — --task mode's degradation matrix: the fail-open R4 gap that
#             --audit's own non-terminal filter kept out of step-17's reach
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

# ─────────────────────────────────────────────────────────────────────────────
# Block 3 (step-9) — per-branch git measurement, --task single mode
#
# Every count is asserted against the SAME quantity computed independently in
# the test with plain git, not against a number this suite hard-codes — a
# hard-coded expectation would drift with any fixture edit and would not
# actually pin the SUT's definition of `behind`.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 3: per-branch git measurement and --task mode ---"

# _commit_main <subject> — one empty commit on main.
_commit_main() { git -C "$REPO" commit -q --allow-empty -m "$1"; }

# _git <args...> — read-only git in the fixture repo.
_git() { git -C "$REPO" "$@"; }

_mk_tasks_db
_mk_repo
G_DB="$DB"

# main: initial + five more, so a branch can be cut a KNOWN distance back.
for _n in 1 2 3 4 5; do _commit_main "main commit $_n"; done
G_MAIN_TIP="$(_git rev-parse main)"
G_MAIN_BACK3="$(_git rev-parse 'main~3')"

_add_task 9301 in-progress '{"files":["x.rs"]}'
_add_task 9302 pending     '{"files":["y.rs"]}'
_add_task 9399 pending     '{"files":["z.rs"]}'

# 9301 — cut at main's tip: behind must be 0.
_branch_at "task/9301" "$G_MAIN_TIP"
_commit_files "task/9301" "feat(9301): x" "x.rs"

# 9302 — cut three commits back: behind must be exactly 3.
_branch_at "task/9302" "$G_MAIN_BACK3"
_commit_files "task/9302" "feat(9302): y" "y.rs"
_commit_files "task/9302" "feat(9302): y again" "y.rs"

# 9399 — a non-terminal task with NO branch ref at all.

# (a) the row carries every measured field, each matching an independent
#     computation over the same fixture.
run_helper --task 9301 --db "$G_DB" --repo "$REPO"
assert "G1: --task exits 0" test "$RC" -eq 0
assert "G1: --task emits EXACTLY one row on stdout" \
    bash -c '[ "$(printf "%s\n" "$1" | grep -c .)" -eq 1 ]' _ "$OUT"
assert "G1: --task emits no SWEEP: summary (that is fleet mode's line)" \
    bash -c '! printf "%s\n" "$1" | grep -q "^SWEEP:"' _ "$OUT"
_assert_field "G1: task id"  9301 task   9301
_assert_field "G1: status"   9301 status in-progress

G1_MB="$(_git merge-base "task/9301" main)"
_assert_field "G1: merge_base is the abbreviated git merge-base" \
    9301 merge_base "$(_git rev-parse --short "$G1_MB")"
_assert_field "G1: behind == rev-list --count <merge_base>..main" \
    9301 behind "$(_git rev-list --count "$G1_MB..main")"
_assert_field "G1: commits == rev-list --count main..<branch>" \
    9301 commits "$(_git rev-list --count "main..task/9301")"
_assert_field "G1: changed == count of diff --name-only <merge_base> <branch>" \
    9301 changed "$(_git diff --name-only "$G1_MB" "task/9301" | grep -c .)"

# (b) behind is asserted as an EXACT number in both directions, not merely
#     "zero" versus "positive".
_assert_field "G2: a branch cut at main's tip reports behind=0" 9301 behind 0

run_helper --task 9302 --db "$G_DB" --repo "$REPO"
assert "G2: --task 9302 exits 0" test "$RC" -eq 0
_assert_field "G2: a branch cut three commits back reports behind=3" 9302 behind 3
_assert_field "G2: ...and commits=2 (its own two)"                   9302 commits 2
G2_MB="$(_git merge-base "task/9302" main)"
_assert_field "G2: ...and behind still equals the independent count" \
    9302 behind "$(_git rev-list --count "$G2_MB..main")"

# (c) degradation — each exits 0, reports UNKNOWN, and warns on stderr.
run_helper --task 9399 --db "$G_DB" --repo "$REPO"
assert "G3[absent branch ref]: exits 0"        test "$RC" -eq 0
assert "G3[absent branch ref]: warns on stderr" test -n "$ERR_OUT"
_assert_field "G3[absent branch ref]: scope=UNKNOWN"  9399 scope UNKNOWN
_assert_field "G3[absent branch ref]: signature='-'"  9399 signature -

run_helper --task 9301 --db "$G_DB" --repo "$REPO" --main-ref no-such-ref
assert "G3[unresolvable --main-ref]: exits 0"        test "$RC" -eq 0
assert "G3[unresolvable --main-ref]: warns on stderr" test -n "$ERR_OUT"
_assert_field "G3[unresolvable --main-ref]: scope=UNKNOWN" 9301 scope UNKNOWN
_assert_field "G3[unresolvable --main-ref]: signature='-'" 9301 signature -

G3_NOTGIT="$(mktemp -d "${TMPDIR:-/tmp}/task-branch-sweep-notgit-XXXXXX")"
_TMPDIRS+=("$G3_NOTGIT")
run_helper --task 9301 --db "$G_DB" --repo "$G3_NOTGIT"
assert "G3[non-git --repo]: exits 0"        test "$RC" -eq 0
assert "G3[non-git --repo]: warns on stderr" test -n "$ERR_OUT"
_assert_field "G3[non-git --repo]: scope=UNKNOWN" 9301 scope UNKNOWN
_assert_field "G3[non-git --repo]: signature='-'" 9301 signature -

# (d) READ-ONLY on the repo. Captured before and after a run that touches every
#     code path — both modes, both formats — and compared byte-for-byte.
G4_REFS_BEFORE="$(_git for-each-ref --format='%(objectname) %(refname)')"
G4_STATUS_BEFORE="$(_git status --porcelain --untracked-files=all)"
G4_HEAD_BEFORE="$(_git rev-parse HEAD)"

run_helper --task 9301 --db "$G_DB" --repo "$REPO"
run_helper --audit --db "$G_DB" --repo "$REPO"
run_helper --audit --db "$G_DB" --repo "$REPO" --format json

assert "G4: every ref is byte-identical after the sweep" \
    bash -c '[ "$1" = "$2" ]' _ "$G4_REFS_BEFORE" "$(_git for-each-ref --format='%(objectname) %(refname)')"
assert "G4: git status --porcelain is byte-identical after the sweep" \
    bash -c '[ "$1" = "$2" ]' _ "$G4_STATUS_BEFORE" "$(_git status --porcelain --untracked-files=all)"
assert "G4: HEAD is unmoved after the sweep" \
    bash -c '[ "$1" = "$2" ]' _ "$G4_HEAD_BEFORE" "$(_git rev-parse HEAD)"

# ─────────────────────────────────────────────────────────────────────────────
# Block 4 (step-11) — the `scope` verdict and the declared-scope cross-check
#
# One fixture store and one fixture repo shared by the whole block: the
# peer_owner map is GLOBAL over the store, so the peer-ownership cases only
# mean anything when the other declarers coexist with the task under test.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 4: the scope verdict ---"

_mk_tasks_db
_mk_repo
S_DB="$DB"
S_MAIN="$(_git rev-parse main)"

# _scope_case <id> <status> <metadata> <changed-path>...
# Registers the task, cuts its branch at main and commits the changed paths.
_scope_case() {
    local id="$1" status="$2" metadata="$3"; shift 3
    _add_task "$id" "$status" "$metadata"
    _branch_at "task/$id" "$S_MAIN"
    _commit_files "task/$id" "feat($id): touch files" "$@"
}

# (a) everything changed is declared
_scope_case 9401 pending '{"files":["p.rs","q.rs"]}' p.rs q.rs

# (b) two changed paths nobody declares
_scope_case 9402 pending '{"files":["b-own.rs"]}' b-own.rs orphan1.rs orphan2.rs

# (c) two foreign paths, each declared by a DIFFERENT non-terminal peer.
#     The peers are registered without branches — declaring a file is what
#     makes a task an owner, not having a branch.
_add_task 9404 pending    '{"files":["s.rs"]}'
_add_task 9405 blocked    '{"files":["t.rs"]}'
_scope_case 9403 pending '{"files":["r.rs"]}' r.rs s.rs t.rs

# (c') the only other declarer is TERMINAL, so this is plain OUT-OF-SCOPE
_add_task 9407 done       '{"files":["v.rs"]}'
_scope_case 9406 pending '{"files":["u.rs"]}' u.rs v.rs

# (d) an empty declaration, with FIVE files changed
_scope_case 9408 pending '{"files":[]}' e1.rs e2.rs e3.rs e4.rs e5.rs

# (e) exact string equality, three shapes
_scope_case 9410 pending '{"files":["x/a/b.rs"]}' a/b.rs
_scope_case 9411 pending '{"files":["crates/foo"]}' crates/foo/src/lib.rs
_scope_case 9412 pending '{"files":["long/prefix/name.rs"]}' long/prefix/name.rs.bak

# (f) a path this task declares is never a peer file, even though a peer
#     declares it too and it therefore sits in peer_owner
_add_task 9414 pending    '{"files":["shared.rs"]}'
_scope_case 9413 pending '{"files":["shared.rs"]}' shared.rs

# ── (a) CLEAN ─────────────────────────────────────────────────────────────────
run_helper --task 9401 --db "$S_DB" --repo "$REPO"
_assert_field "S1: every changed path declared -> scope=CLEAN" 9401 scope CLEAN
_assert_field "S1: ...foreign=0"                               9401 foreign 0
_assert_field "S1: ...peer_files=0"                            9401 peer_files 0
_assert_field "S1: ...peers='-'"                               9401 peers -
_assert_field "S1: ...changed=2 (the measurement still ran)"   9401 changed 2

# ── (b) OUT-OF-SCOPE ──────────────────────────────────────────────────────────
run_helper --task 9402 --db "$S_DB" --repo "$REPO"
_assert_field "S2: undeclared-by-anyone paths -> scope=OUT-OF-SCOPE" 9402 scope OUT-OF-SCOPE
_assert_field "S2: ...foreign=2 (the exact count, not merely non-zero)" 9402 foreign 2
_assert_field "S2: ...peer_files=0 (nobody else declares them)"      9402 peer_files 0
_assert_field "S2: ...peers='-'"                                     9402 peers -

# ── (c) PEER-FILES ────────────────────────────────────────────────────────────
run_helper --task 9403 --db "$S_DB" --repo "$REPO"
_assert_field "S3: peer-declared foreign paths -> scope=PEER-FILES" 9403 scope PEER-FILES
_assert_field "S3: ...foreign=2"                                    9403 foreign 2
_assert_field "S3: ...peer_files=2"                                 9403 peer_files 2
_assert_field "S3: ...peers lists both owners, sorted-unique"       9403 peers 9404,9405

# ── (c') a terminal declarer is not a peer ────────────────────────────────────
run_helper --task 9406 --db "$S_DB" --repo "$REPO"
_assert_field "S4: only a done/cancelled declarer -> OUT-OF-SCOPE, not PEER-FILES" \
    9406 scope OUT-OF-SCOPE
_assert_field "S4: ...foreign=1"    9406 foreign 1
_assert_field "S4: ...peer_files=0" 9406 peer_files 0
_assert_field "S4: ...peers='-'"    9406 peers -

# ── (d) UNDECLARED wins over OUT-OF-SCOPE ─────────────────────────────────────
run_helper --task 9408 --db "$S_DB" --repo "$REPO"
_assert_field "S5: an empty declaration -> scope=UNDECLARED" 9408 scope UNDECLARED
_assert_field "S5: ...even with five files changed"          9408 changed 5
assert "S5: ...and NEVER reports OUT-OF-SCOPE" \
    bash -c '! printf "%s\n" "$1" | grep -q "scope=OUT-OF-SCOPE"' _ "$(_row 9408)"

# ── (e) exact repo-relative string equality ───────────────────────────────────
run_helper --task 9410 --db "$S_DB" --repo "$REPO"
_assert_field "S6: declared 'x/a/b.rs' does NOT cover changed 'a/b.rs'" \
    9410 foreign 1
_assert_field "S6: ...so the row is OUT-OF-SCOPE" 9410 scope OUT-OF-SCOPE

run_helper --task 9411 --db "$S_DB" --repo "$REPO"
_assert_field "S6: declared 'crates/foo' does NOT cover 'crates/foo/src/lib.rs'" \
    9411 foreign 1

run_helper --task 9412 --db "$S_DB" --repo "$REPO"
_assert_field "S6: a changed path is foreign even when a declared path extends it" \
    9412 foreign 1

# ── (f) own declarations never count as peer files ────────────────────────────
run_helper --task 9413 --db "$S_DB" --repo "$REPO"
_assert_field "S7: a path this task declares is never foreign, even when a peer declares it too" \
    9413 foreign 0
_assert_field "S7: ...so peer_files=0" 9413 peer_files 0
_assert_field "S7: ...and scope=CLEAN" 9413 scope CLEAN
_assert_field "S7: ...and peers='-'"   9413 peers -

# ─────────────────────────────────────────────────────────────────────────────
# Block 5 (step-13) — the commit-citation census and `signature`
#
# This is the sharp signal: the defect the sweep exists to find is foreign
# COMMITS, so the census measures commits directly rather than inferring them
# from files.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 5: commit-citation census and signature ---"

# _commit_msg <branch> <subject> — one empty commit on <branch>, for a case
# where only the MESSAGE matters.
_commit_msg() {
    git -C "$REPO" checkout -q "$1"
    git -C "$REPO" commit -q --allow-empty -m "$2"
    git -C "$REPO" checkout -q main
}

_mk_tasks_db
_mk_repo
C_DB="$DB"

# Peers that exist in the store but need no branch of their own.
_add_task 9501 pending   '{"files":["peer1.rs"]}'
_add_task 9502 blocked   '{"files":["peer2.rs"]}'
_add_task 9503 done      '{"files":["peer3.rs"]}'
_add_task 9504 pending   '{"files":["shared2.rs"]}'

# 9510 — the full mix. It also changes a file 9504 declares, so `peers` must
# be the UNION of the file-derived and commit-derived ids.
_add_task 9510 pending   '{"files":["own.rs"]}'
_branch_at "task/9510" "$(_git rev-parse main)"
_commit_files "task/9510" "feat(9510): own work"          own.rs
_commit_files "task/9510" "feat(9510): touch a peer file" shared2.rs
_commit_msg   "task/9510" "fix: follows up on #9501"
_commit_msg   "task/9510" "Merge task/9502 into main"
_commit_msg   "task/9510" "chore: relates to #9503"
_commit_msg   "task/9510" "amend(9510): #9510 also touches #9501"

run_helper --task 9510 --db "$C_DB" --repo "$REPO"
assert "C1: the census exits 0" test "$RC" -eq 0

# (a)+(c)+(d) exact count: two of the six commits cite a live peer.
#   own work            -> no citation        (d)
#   touch a peer file   -> no citation        (d)
#   #9501               -> peer               (a)
#   Merge task/9502     -> peer               (a)+(e)
#   #9503 (done)        -> NOT a peer         (c)
#   #9510 and #9501     -> cites own id, NOT a peer  (b)
_assert_field "C1: peer_commits is the exact count of citing commits" \
    9510 peer_commits 2
_assert_field "C1: commits counts every commit on the branch" 9510 commits 6

# (e) both citation forms, and (a) the union with the file-derived id 9504
_assert_field "C2: peers is the sorted-unique UNION of commit- and file-derived ids" \
    9510 peers 9501,9502,9504

# (f) signature is set by peer_commits alone
_assert_field "C3: peer_commits>0 -> signature=SUSPECT" 9510 signature SUSPECT

# (b) isolated: a message that cites its OWN id must not flag even when it
#     also names a live peer in the same message.
_add_task 9520 pending '{"files":["o20.rs"]}'
_branch_at "task/9520" "$(_git rev-parse main)"
_commit_files "task/9520" "feat(9520): own" o20.rs
_commit_msg   "task/9520" "amend(9520): re-lands #9520, coordinated with #9501"
run_helper --task 9520 --db "$C_DB" --repo "$REPO"
_assert_field "C4: a self-citing commit naming a peer is NOT a peer commit" \
    9520 peer_commits 0
_assert_field "C4: ...so signature='-'" 9520 signature -

# (c) isolated: only a terminal citation
_add_task 9521 pending '{"files":["o21.rs"]}'
_branch_at "task/9521" "$(_git rev-parse main)"
_commit_files "task/9521" "feat(9521): own" o21.rs
_commit_msg   "task/9521" "chore: supersedes #9503"
run_helper --task 9521 --db "$C_DB" --repo "$REPO"
_assert_field "C5: a commit citing only a done task is NOT a peer commit" \
    9521 peer_commits 0
_assert_field "C5: ...so signature='-'" 9521 signature -

# (d) isolated: no citation anywhere is not an error
_add_task 9522 pending '{"files":["o22.rs"]}'
_branch_at "task/9522" "$(_git rev-parse main)"
_commit_files "task/9522" "feat: no citation at all" o22.rs
run_helper --task 9522 --db "$C_DB" --repo "$REPO"
assert "C6: a branch citing nothing exits 0"      test "$RC" -eq 0
_assert_field "C6: ...peer_commits=0"             9522 peer_commits 0
_assert_field "C6: ...signature='-'"              9522 signature -
_assert_field "C6: ...and it still measured"      9522 changed 1

# (g) `behind` NEVER contributes to `signature`. A deeply stale branch with no
#     peer commits must read '-'. Measured justification: the median live task
#     branch is 2201 commits behind main (n=351, p25 878, p90 5324), so a
#     staleness-triggered signature would fire on ~98% of the pool.
C_STALE_BASE="$(_git rev-parse main)"
for _n in $(seq 1 40); do _commit_main "stale-maker $_n"; done
_add_task 9530 pending '{"files":["o30.rs"]}'
_branch_at "task/9530" "$C_STALE_BASE"
_commit_files "task/9530" "feat(9530): own work only" o30.rs
run_helper --task 9530 --db "$C_DB" --repo "$REPO"
_assert_field "C7: a deeply stale branch reports its exact distance" 9530 behind 40
_assert_field "C7: ...peer_commits=0"                                9530 peer_commits 0
_assert_field "C7: ...and signature is STILL '-' — behind never triggers" \
    9530 signature -

# ─────────────────────────────────────────────────────────────────────────────
# Block 6 (step-15) — --audit fleet mode, the SWEEP: summary, --format json
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 6: fleet mode, summary and json ---"

# _orphan_branch <name> <file> — a branch with UNRELATED history, so it has no
# merge base with main. That is the only way to reach scope=UNKNOWN inside an
# otherwise-healthy repo, which the partition assertion needs.
_orphan_branch() {
    local name="$1" file="$2"
    git -C "$REPO" checkout -q --orphan "$name"
    git -C "$REPO" rm -rfq --cached . >/dev/null 2>&1 || true
    find "$REPO" -mindepth 1 -maxdepth 1 ! -name .git -exec rm -rf {} +
    printf 'orphan\n' > "$REPO/$file"
    git -C "$REPO" add -- "$file"
    git -C "$REPO" commit -q -m "feat: orphan root"
    git -C "$REPO" checkout -q -f main
}

# _summary_field <key> — the value of <key> on the SWEEP: line of $OUT.
_summary_field() {
    printf '%s\n' "$OUT" | grep '^SWEEP:' | tr ' ' '\n' | sed -n "s/^$1=//p"
}
_assert_summary() {
    assert "$1" bash -c '[ "$(printf "%s\n" "$1" | grep "^SWEEP:" | tr " " "\n" | sed -n "s/^$2=//p")" = "$3" ]' \
        _ "$OUT" "$2" "$3"
}

_mk_tasks_db
_mk_repo
F_DB="$DB"
# _mk_repo / _mk_tasks_db set the shared REPO / DB globals, and the F5
# empty-pool block below calls both again. Pin this fixture's paths now so the
# later assertions cannot silently run against the empty fixture — which is
# exactly what made F6 report zero branches and F7 compare two empty reports.
F_REPO="$REPO"
F_MAIN="$(_git rev-parse main)"

# A path carrying both a double quote and a backslash — the two bytes a
# hand-rolled JSON escaper gets wrong.
F_WEIRD='weird"path\x.rs'

_add_task 9610 pending '{"files":["p3.rs"]}'          # peer declarer + citee
_add_task 9601 pending '{"files":["c1.rs"]}'          # -> CLEAN
_add_task 9602 pending '{"files":["c2.rs"]}'          # -> OUT-OF-SCOPE
_add_task 9603 pending '{"files":["c3.rs"]}'          # -> PEER-FILES
_add_task 9604 pending '{"files":[]}'                 # -> UNDECLARED
_add_task 9605 pending '{"files":["c5.rs"]}'          # -> SUSPECT
_add_task 9606 pending '{"files":["c6.rs"]}'          # -> UNKNOWN (orphan)
_add_task 9607 done    '{"files":["c7.rs"]}'          # -> skipped_terminal
_add_task 9609 pending '{"files":["c9.rs"]}'          # -> no branch, no row
_add_task 9611 pending "$(printf '{"files":["weird\\"path\\\\x.rs"]}')"  # -> CLEAN

_branch_at "task/9601" "$F_MAIN"; _commit_files "task/9601" "feat(9601): own" c1.rs
_branch_at "task/9602" "$F_MAIN"; _commit_files "task/9602" "feat(9602): own" c2.rs orphan.rs
_branch_at "task/9603" "$F_MAIN"; _commit_files "task/9603" "feat(9603): own" c3.rs p3.rs
_branch_at "task/9604" "$F_MAIN"; _commit_files "task/9604" "feat(9604): own" c4.rs
_branch_at "task/9605" "$F_MAIN"; _commit_files "task/9605" "feat(9605): own" c5.rs
_commit_msg "task/9605" "chore: carries work for #9610"
_orphan_branch "task/9606" c6.rs
_branch_at "task/9607" "$F_MAIN"; _commit_files "task/9607" "feat(9607): own" c7.rs
_branch_at "task/9608-recovered" "$F_MAIN"
_branch_at "task/9611" "$F_MAIN"; _commit_files "task/9611" "feat(9611): own" "$F_WEIRD"

run_helper --audit --db "$F_DB" --repo "$REPO"
assert "F0: --audit exits 0" test "$RC" -eq 0

# (a) one row per non-terminal-backed branch, ascending task id
for _id in 9601 9602 9603 9604 9605 9606 9611; do
    assert "F1: task $_id yields exactly one row" \
        bash -c '[ "$(printf "%s\n" "$1" | grep -cE "^task=$2( |$)")" -eq 1 ]' _ "$OUT" "$_id"
done
assert "F1: rows are emitted in ascending task id order" \
    bash -c 'ids="$(printf "%s\n" "$1" | sed -nE "s/^task=([0-9]+) .*/\1/p")"; [ "$ids" = "$(printf "%s\n" "$ids" | sort -n)" ]' \
    _ "$OUT"

# (b) skipped, counted, never dropped and never an error
_assert_no_row "F2: a done-backed branch yields no row"      9607
_assert_summary "F2: ...and is counted in skipped_terminal"  skipped_terminal 1
assert "F2: a non-numeric branch suffix yields no row" \
    bash -c '! printf "%s\n" "$1" | grep -q "9608"' _ "$OUT"
_assert_summary "F2: ...and is counted in skipped_nonnumeric" skipped_nonnumeric 1

# (c) a task with no branch is not a branch
_assert_no_row "F3: a task with no branch ref yields no row" 9609
_assert_no_row "F3: a peer declarer with no branch yields no row" 9610

# (d) the summary partitions the rows
_assert_summary "F4: branches=7"     branches 7
_assert_summary "F4: suspect=1"      suspect 1
_assert_summary "F4: peer_files=1"   peer_files 1
_assert_summary "F4: out_of_scope=1" out_of_scope 1
_assert_summary "F4: undeclared=1"   undeclared 1
_assert_summary "F4: clean=2"        clean 2
_assert_summary "F4: unknown=1"      unknown 1
assert "F4: the six class counters PARTITION branches" \
    bash -c 'v() { local x; x="$(printf "%s\n" "$1" | grep "^SWEEP:" | tr " " "\n" | sed -n "s/^$2=//p")";
                   case "${x:-}" in ""|*[!0-9]*) echo "missing/non-numeric counter: $2" >&2; return 1 ;; esac
                   printf "%s" "$x"; };
             t="$(v "$1" branches)" || exit 1
             sum=0
             for k in suspect peer_files out_of_scope undeclared clean unknown; do
                 n="$(v "$1" "$k")" || exit 1
                 sum=$((sum + n))
             done
             [ "$t" -eq "$sum" ] || { echo "branches=$t but the six classes sum to $sum" >&2; exit 1; }' \
    _ "$OUT"
assert "F4: the SUSPECT row is counted ONLY under suspect (9605's scope is CLEAN)" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^task=9605 .*scope=CLEAN signature=SUSPECT$"' _ "$OUT"

# zeros are always emitted, never omitted
_mk_tasks_db
_mk_repo
F_EMPTY_DB="$DB"
run_helper --audit --db "$F_EMPTY_DB" --repo "$REPO"
for _k in branches suspect peer_files out_of_scope undeclared clean unknown \
          skipped_terminal skipped_nonnumeric; do
    _assert_summary "F5: $_k=0 is emitted, not omitted, on an empty pool" "$_k" 0
done

# (e) --format json
run_helper --audit --db "$F_DB" --repo "$F_REPO" --format json
assert "F6: --format json exits 0" test "$RC" -eq 0
assert "F6: --format json emits ONE parseable document" \
    bash -c 'printf "%s" "$1" | python3 -c "import json,sys; json.load(sys.stdin)"' _ "$OUT"
assert "F6: a path with a quote and a backslash does not corrupt the document" \
    bash -c 'printf "%s" "$1" | python3 -c "
import json,sys
d=json.load(sys.stdin)
r=[b for b in d[\"branches\"] if b[\"task\"]==9611][0]
assert r[\"changed\"]==1 and r[\"foreign\"]==0 and r[\"scope\"]==\"CLEAN\", r
"' _ "$OUT"
assert "F6: every column is its own key on every branch object" \
    bash -c 'printf "%s" "$1" | python3 -c "
import json,sys
keys={\"task\",\"status\",\"merge_base\",\"behind\",\"commits\",\"peer_commits\",
      \"changed\",\"foreign\",\"peer_files\",\"peers\",\"scope\",\"signature\"}
d=json.load(sys.stdin)
assert d[\"branches\"], \"no branches\"
for b in d[\"branches\"]:
    assert set(b)==keys, (set(b)^keys)
"' _ "$OUT"
assert "F6: summary is a sibling object holding the same counters" \
    bash -c 'printf "%s" "$1" | python3 -c "
import json,sys
d=json.load(sys.stdin)
s=d[\"summary\"]
assert s[\"branches\"]==7 and s[\"suspect\"]==1 and s[\"clean\"]==2, s
assert s[\"skipped_terminal\"]==1 and s[\"skipped_nonnumeric\"]==1, s
"' _ "$OUT"

# (f) the two formats agree
F7_JSON="$OUT"
run_helper --audit --db "$F_DB" --repo "$F_REPO"
F7_TABLE="$OUT"
assert "F7: json and table report the SAME rows and counters" \
    bash -c 'rendered="$(printf "%s" "$1" | python3 -c "
import json,sys
d=json.load(sys.stdin)
cols=[\"task\",\"status\",\"merge_base\",\"behind\",\"commits\",\"peer_commits\",
      \"changed\",\"foreign\",\"peer_files\",\"peers\",\"scope\",\"signature\"]
for b in d[\"branches\"]:
    print(\" \".join(f\"{c}={b[c]}\" for c in cols))
s=d[\"summary\"]
print(\"SWEEP: \" + \" \".join(f\"{k}={s[k]}\" for k in
      [\"branches\",\"suspect\",\"peer_files\",\"out_of_scope\",\"undeclared\",
       \"clean\",\"unknown\",\"skipped_terminal\",\"skipped_nonnumeric\"]))
")"; [ "$rendered" = "$2" ]' _ "$F7_JSON" "$F7_TABLE"

# ─────────────────────────────────────────────────────────────────────────────
# Block 7 (step-17) — the read-only and non-gating invariants
#
# An invariant test that has never been seen to FAIL is worth very little, so
# every fingerprint helper below is paired with a mutation-injection check
# (P-prefixed) that mutates a COPY and asserts the same helper reports a
# difference. That keeps the proof of sensitivity in the suite permanently
# rather than in a one-off manual check.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 7: read-only and non-gating invariants ---"

# _db_fingerprint <db> — size, mtime, content hash, and the sibling files
# sqlite would create if it opened the store for writing (-wal / -shm /
# -journal). Any of the four changing means the store was not read-only.
_db_fingerprint() {
    local db="$1"
    stat -c '%s %Y' "$db" 2>/dev/null || echo "ABSENT"
    sha256sum "$db" 2>/dev/null | cut -d' ' -f1 || echo "NOHASH"
    ls -1 "$db"-wal "$db"-shm "$db"-journal 2>/dev/null || true
}

# _repo_fingerprint <repo> — refs, per-branch reflogs, porcelain status, the
# worktree file list, and the .git file-NAME list. Names (not contents) under
# .git, because a new object or ref shows up as a new NAME while git's own
# index refresh only rewrites an existing one.
_repo_fingerprint() {
    local repo="$1" b
    git -C "$repo" for-each-ref --format='%(objectname) %(refname)' 2>/dev/null
    while IFS= read -r b; do
        printf 'reflog %s: %s\n' "$b" \
            "$(git -C "$repo" reflog show "$b" 2>/dev/null | tr '\n' '|')"
    done < <(git -C "$repo" for-each-ref --format='%(refname:short)' refs/heads 2>/dev/null)
    git -C "$repo" status --porcelain --untracked-files=all 2>/dev/null
    find "$repo" -path "$repo/.git" -prune -o -print 2>/dev/null | sort
    find "$repo/.git" -type f 2>/dev/null | sort
}

# A fixture in which EVERY branch is SUSPECT — the state a gating caller would
# most want to react to, and therefore the one where a non-zero exit would be
# most tempting.
_mk_tasks_db
_mk_repo
I_DB="$DB"
I_REPO="$REPO"
I_MAIN="$(_git rev-parse main)"

_add_task 9701 pending '{"files":["i1.rs"]}'
_add_task 9702 pending '{"files":["i2.rs"]}'
_add_task 9703 pending '{"files":["i3.rs"]}'
for _spec in "9701 i1.rs 9702" "9702 i2.rs 9703" "9703 i3.rs 9701"; do
    set -- $_spec
    _branch_at "task/$1" "$I_MAIN"
    _commit_files "task/$1" "feat($1): own" "$2"
    _commit_msg   "task/$1" "chore: carries work for #$3"
done

run_helper --audit --db "$I_DB" --repo "$I_REPO"
assert "I0: the fixture really is all-SUSPECT (guard is non-vacuous)" \
    bash -c '[ "$(printf "%s\n" "$1" | grep "^SWEEP:" | tr " " "\n" | sed -n "s/^suspect=//p")" = 3 ]' \
    _ "$OUT"

# ── (a) R3: exit 0 on every valid invocation, in both modes and both formats ──
I_DB_BEFORE="$(_db_fingerprint "$I_DB")"
I_REPO_BEFORE="$(_repo_fingerprint "$I_REPO")"

run_helper --audit --db "$I_DB" --repo "$I_REPO"
assert "I1[--audit table]: exits 0 even when every branch is SUSPECT" test "$RC" -eq 0
run_helper --audit --db "$I_DB" --repo "$I_REPO" --format json
assert "I1[--audit json]: exits 0 even when every branch is SUSPECT"  test "$RC" -eq 0
run_helper --task 9701 --db "$I_DB" --repo "$I_REPO"
assert "I1[--task table]: exits 0 on a SUSPECT branch"                test "$RC" -eq 0
run_helper --task 9701 --db "$I_DB" --repo "$I_REPO" --format json
assert "I1[--task json]: exits 0 on a SUSPECT branch"                 test "$RC" -eq 0

# --format is a global flag: it is accepted and validated in BOTH modes, so it
# must be HONOURED in both. Silently emitting table output from `--task
# --format json` would hand the per-merge advisory consult — the whole reason
# --task exists — an unparseable answer under a flag it asked for.
assert "I1[--task json]: actually emits JSON, not a table row" \
    bash -c 'printf "%s" "$1" | python3 -c "import json,sys; json.load(sys.stdin)"' _ "$OUT"
assert "I1[--task json]: the document carries exactly the one requested branch" \
    bash -c 'printf "%s" "$1" | python3 -c "
import json,sys
d=json.load(sys.stdin)
assert [b[\"task\"] for b in d[\"branches\"]]==[9701], d
"' _ "$OUT"
assert "I1[--task json]: single-branch mode carries no summary (that is fleet mode'\''s)" \
    bash -c 'printf "%s" "$1" | python3 -c "
import json,sys
assert \"summary\" not in json.load(sys.stdin)
"' _ "$OUT"

# ── (b) R1: the task store is untouched ───────────────────────────────────────
assert "I2: the task store's size, mtime and sha256 are unchanged" \
    bash -c '[ "$1" = "$2" ]' _ "$I_DB_BEFORE" "$(_db_fingerprint "$I_DB")"
assert "I2: no -wal / -shm / -journal sibling was created next to the store" \
    bash -c '! ls "$1"-wal "$1"-shm "$1"-journal >/dev/null 2>&1' _ "$I_DB"

# ── (c) R2: the git fixture is untouched ──────────────────────────────────────
assert "I3: refs, reflogs, porcelain status and the file lists are unchanged" \
    bash -c '[ "$1" = "$2" ]' _ "$I_REPO_BEFORE" "$(_repo_fingerprint "$I_REPO")"

# ── (d) R1: nothing is created at a --db path that does not exist ─────────────
I_NODB="$(mktemp -d "${TMPDIR:-/tmp}/task-branch-sweep-nocreate-XXXXXX")"
_TMPDIRS+=("$I_NODB")
run_helper --audit --db "$I_NODB/never-created.db" --repo "$I_REPO"
assert "I4: a nonexistent --db path exits 0"                 test "$RC" -eq 0
assert "I4: ...and is STILL absent afterwards"               test ! -e "$I_NODB/never-created.db"
assert "I4: ...and no sibling was created either"            bash -c '[ -z "$(ls -A "$1")" ]' _ "$I_NODB"

# ── mutation injection: prove each fingerprint can actually report a change ───
# Without these, I2 and I3 would pass just as happily against a helper that
# returns a constant.
I_DB_COPY="$I_NODB/copy.db"
cp "$I_DB" "$I_DB_COPY"
I_DBCOPY_BEFORE="$(_db_fingerprint "$I_DB_COPY")"
_sq "$I_DB_COPY" "UPDATE tasks SET status='blocked' WHERE id=9701;"
assert "P1: _db_fingerprint DETECTS a write to the store (I2 can fail)" \
    bash -c '[ "$1" != "$2" ]' _ "$I_DBCOPY_BEFORE" "$(_db_fingerprint "$I_DB_COPY")"

I_REPO_COPY="$I_NODB/repo-copy"
cp -r "$I_REPO" "$I_REPO_COPY"
I_REPOCOPY_BEFORE="$(_repo_fingerprint "$I_REPO_COPY")"
git -C "$I_REPO_COPY" branch -q task/9799 main
assert "P2: _repo_fingerprint DETECTS a new ref (I3 can fail)" \
    bash -c '[ "$1" != "$2" ]' _ "$I_REPOCOPY_BEFORE" "$(_repo_fingerprint "$I_REPO_COPY")"

I_REPOCOPY_BEFORE="$(_repo_fingerprint "$I_REPO_COPY")"
printf 'stray\n' > "$I_REPO_COPY/stray-file.txt"
assert "P3: _repo_fingerprint DETECTS a new worktree file (I3 can fail)" \
    bash -c '[ "$1" != "$2" ]' _ "$I_REPOCOPY_BEFORE" "$(_repo_fingerprint "$I_REPO_COPY")"

# ─────────────────────────────────────────────────────────────────────────────
# Block 8 (step-22) — --task mode must fail SAFE, not fail OPEN
#
# R4 says an unreadable store degrades the affected row to UNKNOWN. Block 2's
# T4 only ever exercised --audit, and there the fleet loop's own _STATUS
# membership filter drops the branch before any measurement happens — so the
# gap T4 could not see lives entirely in --task, the per-merge advisory consult
# seam, where the row IS measured and a missing store silently RESHAPES the
# verdict (an undeclared-looking, uncitable branch) instead of degrading it.
#
# ONE fixture, five ways. The good-store BASELINE is asserted FIRST and is
# load-bearing: without it every UNKNOWN assertion below would pass just as
# happily against a fixture that was benign anyway — the vacuity that step-20
# found the hard way.
#
# These carry the D (degradation) prefix rather than continuing block 2's T
# series, whose T6 is already the engine-interchangeability check.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block 8: --task mode degradation matrix ---"

_mk_tasks_db
_mk_repo
D_DB="$DB"
D_REPO="$REPO"
D_TASK=900
D_MAIN="$(git -C "$REPO" rev-parse main)"

_add_task 900 pending '{"files":["own900.rs"]}'
_add_task 901 pending '{"files":["peer901.rs"]}'

# The branch is contaminated on BOTH axes the report measures, so a degraded
# run cannot look clean by accident on either one: a foreign file that a live
# peer declares, and a commit citing that same live peer.
_branch_at "task/900" "$D_MAIN"
_commit_files "task/900" "feat(900): its own declared file" own900.rs
_commit_files "task/900" "chore: carries work for #901"     peer901.rs

# ── the positive control ──────────────────────────────────────────────────────
run_helper --task "$D_TASK" --db "$D_DB" --repo "$D_REPO"
assert "D0: the good-store baseline exits 0" test "$RC" -eq 0
_assert_field "D0: baseline reports the store's status"    900 status       pending
_assert_field "D0: baseline counts the peer commit"        900 peer_commits 1
_assert_field "D0: baseline names the peer"                900 peers        901
_assert_field "D0: baseline counts the peer file"          900 peer_files   1
_assert_field "D0: baseline verdict is PEER-FILES"         900 scope        PEER-FILES
_assert_field "D0: baseline signature is SUSPECT"          900 signature    SUSPECT

# ── _assert_degraded <label> <extra args...> ──────────────────────────────────
# The whole degraded contract for --task over the fixture above, in BOTH
# formats. Both, because the consult seam may ask for either and a degradation
# visible in only one of them is still a fail-open for whoever asked for the
# other.
_assert_degraded() {
    local label="$1"; shift
    run_helper --task "$D_TASK" --repo "$D_REPO" "$@"
    assert "D[$label]: exits 0 (R3)"                      test "$RC" -eq 0
    assert "D[$label]: warns on stderr (R4)"              test -n "$ERR_OUT"
    _assert_field "D[$label]: status degrades"            "$D_TASK" status       unknown
    _assert_field "D[$label]: scope is UNKNOWN"           "$D_TASK" scope        UNKNOWN
    _assert_field "D[$label]: signature is not a verdict" "$D_TASK" signature    "-"
    _assert_field "D[$label]: peers unmeasured"           "$D_TASK" peers        "-"
    _assert_field "D[$label]: foreign unmeasured"         "$D_TASK" foreign      "-"
    _assert_field "D[$label]: peer_files unmeasured"      "$D_TASK" peer_files   "-"
    _assert_field "D[$label]: peer_commits unmeasured"    "$D_TASK" peer_commits "-"
    # A degraded row keeps ONE shape: no partially-measured count survives to
    # be read as authoritative.
    assert "D[$label]: no half-measured column survives" \
        bash -c 'row="$1"
for k in merge_base behind commits changed; do
    v="$(printf "%s\n" "$row" | tr " " "\n" | sed -n "s/^$k=//p")"
    [ "$v" = "-" ] || { printf "%s=%s\n" "$k" "$v"; exit 1; }
done' _ "$(_row "$D_TASK")"

    run_helper --task "$D_TASK" --repo "$D_REPO" --format json "$@"
    assert "D[$label/json]: exits 0 (R3)" test "$RC" -eq 0
    assert "D[$label/json]: carries the identical degraded row" \
        bash -c 'printf "%s" "$1" | python3 -c "
import json, sys
d = json.load(sys.stdin)
b = d[\"branches\"]
assert len(b) == 1, b
b = b[0]
assert b[\"task\"] == int(sys.argv[1]), b
assert b[\"status\"] == \"unknown\", b
assert b[\"scope\"] == \"UNKNOWN\", b
for k in (\"merge_base\", \"behind\", \"commits\", \"peer_commits\", \"changed\",
          \"foreign\", \"peer_files\", \"peers\", \"signature\"):
    assert b[k] == \"-\", (k, b)
" "$2"' _ "$OUT" "$D_TASK"
}

D_SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/task-branch-sweep-degrade-XXXXXX")"
_TMPDIRS+=("$D_SCRATCH")

# (a) a --db path that does not exist — the typo'd consult.
_assert_degraded "missing db" --db "$D_SCRATCH/absent.db"

# (b) the store is present but unreadable — a permission change under a live
# seam, which changes nothing the caller can see in its own invocation.
cp "$D_DB" "$D_SCRATCH/unreadable.db"
chmod 000 "$D_SCRATCH/unreadable.db"
_assert_degraded "unreadable db" --db "$D_SCRATCH/unreadable.db"
chmod 644 "$D_SCRATCH/unreadable.db"

# (c) the store reads fine and the tag is simply wrong. _DB_READABLE is 1
# here, so this case is reachable ONLY through the id-absent half of the gate.
_assert_degraded "wrong tag" --db "$D_DB" --tag nosuchtag

# (d) the id's task is terminal. Same repo, same branch, same commits, same
# foreign file — ONLY the store row differs, so a verdict change here can come
# from nothing but the status lookup.
cp "$D_DB" "$D_SCRATCH/done.db"
_sq "$D_SCRATCH/done.db" "UPDATE tasks SET status='done' WHERE id=900;"
_assert_degraded "terminal task" --db "$D_SCRATCH/done.db"

test_summary
