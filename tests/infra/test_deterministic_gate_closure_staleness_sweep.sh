#!/usr/bin/env bash
# tests/infra/test_deterministic_gate_closure_staleness_sweep.sh
# Hermetic tests for scripts/deterministic-gate-closure-staleness-sweep.sh.
#
# Task #5321. Every fixture is synthetic: a temp tasks.db built from the
# production DDL, a temp escalation dir, and a temp git repo. The suite NEVER
# reads the live .taskmaster/tasks/tasks.db, the live data/escalations/, or
# the real repo — both because those mutate continuously under the
# orchestrator (flaky, and lock-contending under a `pool` classification) and
# because the four seed instances this design was derived from (5236, 5271,
# 5316, 5373) have already been redispatched/closed, so a live assertion on
# them would be a doomed RED that could never be GREENed. Their recorded
# shapes are encoded as frozen fixtures instead.
#
# run_sweep captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   S — scaffolding
#   (additional blocks land in subsequent commits as the script grows: the
#    CLI contract + empty-input degradation, the liveness guard, trigger
#    classes A/B/C, the #5316 corruption suppressors, and --emit-requests +
#    the read-only proof.)
#
# The suite is free of `sleep` / wall-clock upper bounds by construction
# (offset-timestamp fixtures instead), so
# tests/infra/test_no_new_wallclock_upper_bounds.sh stays green.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/deterministic-gate-closure-staleness-sweep.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/deterministic-gate-closure-staleness-sweep.sh hermetic tests (task 5321) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp "${TMPDIR:-/tmp}/test-gate-staleness-err-XXXXXX")"
_TMPDIRS+=("$ERR_FILE")

# ──────────────────────────────────────────────────────────────────────────────
# Fixture builders
#
# CONVENTION: the three _mk_* builders SET A GLOBAL (DB / ESC_DIR / REPO_DIR)
# rather than echoing their path, so they must be called directly — never in a
# command substitution, which would run them in a subshell and discard the
# assignment. The _add_* / _commit_* helpers then operate on the current
# global. A block that needs two DBs calls _mk_tasks_db twice, saving the
# first "$DB" into a local first.
# ──────────────────────────────────────────────────────────────────────────────

# _mk_tasks_db — build a fresh temp Taskmaster store; sets global DB.
# The DDL is the production schema copied VERBATIM (verified against
# .taskmaster/tasks/tasks.db's `.schema` on 2026-07-26): both tables are
# tag-scoped with PRIMARY KEY (tag, ...), which is what makes the SUT's
# tag-scoping discipline testable (E7).
_mk_tasks_db() {
    local d
    d="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-db-XXXXXX")"
    _TMPDIRS+=("$d")
    DB="$d/tasks.db"
    sqlite3 "$DB" "
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
);
CREATE TABLE IF NOT EXISTS \"dependencies\" (
    tag        TEXT NOT NULL DEFAULT 'master',
    task_id    INTEGER NOT NULL,
    depends_on INTEGER NOT NULL,
    PRIMARY KEY (tag, task_id, depends_on)
);"
}

# _sq_quote <s> — single-quote a value for direct SQL interpolation.
_sq_quote() { printf "'%s'" "$(printf '%s' "$1" | sed "s/'/''/g")"; }

# _add_task <id> <status> <metadata_json> [heartbeat_at] [claimant_run_id] [tag]
# An empty heartbeat_at / claimant_run_id inserts SQL NULL (the production
# shape for a `blocked` row, which carries no heartbeat).
_add_task() {
    local id="$1" status="$2" metadata="${3:-}" hb="${4:-}" claimant="${5:-}" tag="${6:-master}"
    local hb_sql claimant_sql meta_sql
    if [ -n "$hb" ]; then hb_sql="$(_sq_quote "$hb")"; else hb_sql="NULL"; fi
    if [ -n "$claimant" ]; then claimant_sql="$(_sq_quote "$claimant")"; else claimant_sql="NULL"; fi
    if [ -n "$metadata" ]; then meta_sql="$(_sq_quote "$metadata")"; else meta_sql="NULL"; fi
    sqlite3 "$DB" "INSERT INTO tasks (tag,id,title,status,metadata,updated_at,heartbeat_at,claimant_run_id)
        VALUES ($(_sq_quote "$tag"),$id,'fixture task $id',$(_sq_quote "$status"),$meta_sql,
                $(_sq_quote "$(_now_iso -3600)"),$hb_sql,$claimant_sql);"
}

# _add_dep <task_id> <depends_on> [tag]
_add_dep() {
    local task_id="$1" depends_on="$2" tag="${3:-master}"
    sqlite3 "$DB" "INSERT INTO dependencies (tag,task_id,depends_on)
        VALUES ($(_sq_quote "$tag"),$task_id,$depends_on);"
}

# _mk_esc_dir — build a fresh temp escalation store; sets global ESC_DIR.
_mk_esc_dir() {
    ESC_DIR="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-esc-XXXXXX")"
    _TMPDIRS+=("$ESC_DIR")
}

# _add_esc <task_id> <n> <status> — write esc-<task_id>-<n>.json carrying the
# real field names observed in the live store (id, task_id, status, level,
# resolved_at, category, summary), so a pending vs dismissed/resolved fixture
# is byte-shaped like production. NOTE task_id is a STRING in the live store.
_add_esc() {
    local task_id="$1" n="$2" status="$3"
    local resolved_at="null"
    case "$status" in
        pending) : ;;
        *) resolved_at="\"$(_now_iso -7200)\"" ;;
    esac
    cat > "$ESC_DIR/esc-${task_id}-${n}.json" <<JSON
{
  "id": "esc-${task_id}-${n}",
  "task_id": "${task_id}",
  "status": "${status}",
  "level": 2,
  "resolved_at": ${resolved_at},
  "category": "task_failure",
  "summary": "fixture escalation ${n} for task ${task_id}"
}
JSON
}

# _mk_repo — build a temp git repo with an initial commit on `main`; sets
# global REPO_DIR. Local user.name/user.email so the suite never depends on
# (or perturbs) the host's git identity.
_mk_repo() {
    REPO_DIR="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-repo-XXXXXX")"
    _TMPDIRS+=("$REPO_DIR")
    git -C "$REPO_DIR" init -q -b main
    git -C "$REPO_DIR" config user.email "fixture@example.invalid"
    git -C "$REPO_DIR" config user.name "Fixture"
    git -C "$REPO_DIR" commit -q --allow-empty -m "root"
}

# _commit_touching <path>... — create/append the named repo-relative paths and
# commit them on the CURRENT branch; echoes the resulting SHA. Callers capture
# the SHA into a local so evidence assertions compare against COMPUTED values,
# never frozen constants.
_commit_touching() {
    local p
    for p in "$@"; do
        mkdir -p "$REPO_DIR/$(dirname "$p")"
        printf 'touched at %s\n' "$RANDOM" >> "$REPO_DIR/$p"
        git -C "$REPO_DIR" add -- "$p"
    done
    git -C "$REPO_DIR" commit -q -m "touch $*"
    git -C "$REPO_DIR" rev-parse HEAD
}

# _commit_on_side_branch <branch> <path>... — commit on a side branch that is
# NOT reachable from main, then return to main; echoes the side commit's SHA.
# This is the Signature-2 discarded-duplicate-merge fixture: a SHA that
# resolves but fails `git merge-base --is-ancestor <sha> main`.
_commit_on_side_branch() {
    local branch="$1"; shift
    local sha
    git -C "$REPO_DIR" checkout -q -b "$branch"
    sha="$(_commit_touching "$@")"
    git -C "$REPO_DIR" checkout -q main
    printf '%s\n' "$sha"
}

# _now_iso <offset_seconds> — UTC timestamp offset from now, in the production
# heartbeat_at shape (microseconds + +00:00, e.g.
# 2026-07-26T12:29:54.042540+00:00). A NEGATIVE offset is in the past. Used to
# build both fresh and stale heartbeats with no wall-clock sleep, so the suite
# introduces no wall-clock upper bound.
_now_iso() {
    local offset="${1:-0}"
    date -u -d "@$(( $(date +%s) + offset ))" +"%Y-%m-%dT%H:%M:%S.000000+00:00"
}

# run_sweep <args>... — invoke the SUT, capturing OUT (stdout), ERR_OUT
# (stderr) and RC. Never `set -e`-aborts on a non-zero exit. Set the
# _SWEEP_ENV array (e.g. _SWEEP_ENV=(REIFY_LANE_TASK_DB="$DB")) to inject env
# knobs for a flag<->env parity assert; reset it to () afterwards.
_SWEEP_ENV=()
run_sweep() {
    local rc=0
    : > "$ERR_FILE"
    OUT="$(env "${_SWEEP_ENV[@]+${_SWEEP_ENV[@]}}" "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC="$rc"
    return 0
}

# Report accessors, usable directly as assert commands.
_rc_is()    { test "$RC" = "$1"; }
_out_has()  { _matches "$1" "$OUT"; }
_err_has()  { _matches "$1" "$ERR_OUT"; }
_out_empty() { test -z "$OUT"; }

# _sweep_line — the trailing SWEEP: summary line from the table report.
_sweep_line() { printf '%s' "$OUT" | grep '^SWEEP:' | tail -1; }
_sweep_line_is() { test "$(_sweep_line)" = "$1"; }

# _out_json_check <expr> — _json_check against the captured stdout.
_out_json_check() { printf '%s' "$OUT" | python3 "$_JSON_CHECK_PY" - "$1"; }

# _json_check <json_file|-> <python_expr> — parse the JSON and evaluate
# <python_expr> with `d` bound to the parsed document; exit 0 iff truthy. A
# standalone .py checker (rather than an inline `python3 -c`) keeps JSON
# assertions free of nested-quoting hazards inside assert "..." invocations.
_JSON_CHECK_PY="$(mktemp "${TMPDIR:-/tmp}/gate-staleness-jsoncheck-XXXXXX.py")"
_TMPDIRS+=("$_JSON_CHECK_PY")
cat > "$_JSON_CHECK_PY" <<'PY'
import json, sys
src = sys.stdin if sys.argv[1] == "-" else open(sys.argv[1])
d = json.load(src)
sys.exit(0 if eval(sys.argv[2]) else 1)
PY
_json_check() { python3 "$_JSON_CHECK_PY" "$1" "$2"; }

# Assert-arg helpers. assert() runs its command via "$@", so a shell keyword
# (`!`) or a redirect (`<<<`) cannot appear in an assert argument list — these
# wrap those forms as real commands.
_not() { ! "$@"; }
_matches() { printf '%s' "$2" | grep -qE "$1"; }

# _snapshot_readonly <db> <esc_dir> — emit a sha256 of the DB plus a sorted
# listing (path/size/mtime) of the escalations dir AND of the DB's directory,
# so the read-only proof also catches a stray -wal/-shm sidecar.
_snapshot_readonly() {
    local db="$1" esc_dir="$2"
    sha256sum "$db" 2>/dev/null | awk '{print $1}'
    find "$(dirname "$db")" "$esc_dir" -mindepth 0 -printf '%p %s %T@\n' 2>/dev/null | LC_ALL=C sort
}

# ──────────────────────────────────────────────────────────────────────────────
# Block S — scaffolding + fixture-builder self-checks
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block S: scaffolding + fixture-builder self-checks ---"

assert "S0: SUT is present and executable" test -x "$SCRIPT"

# S1 — _mk_tasks_db produces a non-empty, queryable store with both tables.
_mk_tasks_db
assert "S1a: fixture tasks.db is non-empty" test -s "$DB"
_s1_tables="$(sqlite3 "$DB" "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name;" | tr '\n' ',')"
assert "S1b: fixture DB has both production tables (got: $_s1_tables)" \
    test "$_s1_tables" = "dependencies,tasks,"

# S2 — _add_task / _add_dep round-trip, including the NULL-heartbeat blocked
# shape and the live in-progress claimant shape.
_add_task 9001 blocked '{"task_kind":"deterministic","always_escalates":true}'
_add_task 9002 in-progress '{}' "$(_now_iso -60)" "run-7c8e838c39e7/9002-c514828d/pid=3551081"
_add_dep 9001 9002
assert "S2a: blocked fixture row round-trips with a NULL heartbeat" \
    test "$(sqlite3 "$DB" "SELECT status||'|'||coalesce(heartbeat_at,'NULL') FROM tasks WHERE tag='master' AND id=9001;")" \
       = "blocked|NULL"
assert "S2b: in-progress fixture row carries the live claimant shape" \
    test "$(sqlite3 "$DB" "SELECT claimant_run_id FROM tasks WHERE tag='master' AND id=9002;")" \
       = "run-7c8e838c39e7/9002-c514828d/pid=3551081"
assert "S2c: metadata round-trips as queryable JSON" \
    test "$(sqlite3 "$DB" "SELECT json_extract(metadata,'\$.task_kind') FROM tasks WHERE tag='master' AND id=9001;")" \
       = "deterministic"
assert "S2d: dependency row round-trips tag-scoped" \
    test "$(sqlite3 "$DB" "SELECT depends_on FROM dependencies WHERE tag='master' AND task_id=9001;")" = "9002"

# S3 — _mk_esc_dir / _add_esc produce valid JSON with the live field names.
_mk_esc_dir
_add_esc 9001 1 pending
_add_esc 9001 2 dismissed
assert "S3a: pending escalation file is valid JSON with status=pending" \
    _json_check "$ESC_DIR/esc-9001-1.json" 'd["status"] == "pending" and d["task_id"] == "9001"'
assert "S3b: dismissed escalation carries a resolved_at timestamp" \
    _json_check "$ESC_DIR/esc-9001-2.json" 'd["status"] == "dismissed" and isinstance(d["resolved_at"], str)'

# S4 — _mk_repo / _commit_touching / _commit_on_side_branch.
_mk_repo
_s4_base="$(git -C "$REPO_DIR" rev-parse HEAD)"
_s4_touch="$(_commit_touching tests/infra/test_warm_lane_pool_config.sh)"
assert "S4a: fixture repo HEAD resolves" test -n "$_s4_base"
assert "S4b: _commit_touching advances main and echoes the new SHA" \
    test "$_s4_touch" = "$(git -C "$REPO_DIR" rev-parse main)"
assert "S4c: the touched path is visible in base..main" \
    test "$(git -C "$REPO_DIR" diff --name-only "$_s4_base".."$_s4_touch" -- tests/infra/test_warm_lane_pool_config.sh)" \
       = "tests/infra/test_warm_lane_pool_config.sh"
_s4_side="$(_commit_on_side_branch discarded-dup side.txt)"
assert "S4d: the side-branch SHA resolves as a commit" \
    git -C "$REPO_DIR" rev-parse --verify --quiet "$_s4_side^{commit}"
assert "S4e: the side-branch SHA is NOT an ancestor of main (Sig-2 shape)" \
    _not git -C "$REPO_DIR" merge-base --is-ancestor "$_s4_side" main
assert "S4f: _commit_on_side_branch leaves the repo back on main" \
    test "$(git -C "$REPO_DIR" rev-parse --abbrev-ref HEAD)" = "main"

# S5 — _now_iso shape and ordering (no sleep: offsets only).
assert "S5a: _now_iso emits the production heartbeat shape" \
    _matches '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}\+00:00$' "$(_now_iso 0)"
assert "S5b: a negative offset is in the past relative to now" \
    test "$(date -u -d "$(_now_iso -3600)" +%s)" -lt "$(date -u -d "$(_now_iso 0)" +%s)"
assert "S5c: date -d parses the emitted timestamp (the SUT's liveness path)" \
    date -u -d "$(_now_iso -120)" +%s

# S6 — run_sweep captures OUT/ERR_OUT/RC without aborting under set -e.
run_sweep --help
assert "S6a: run_sweep populates RC without aborting" test -n "${RC:-}"

# S7 — _snapshot_readonly is stable across two reads of an unmodified fixture.
_s7_before="$(_snapshot_readonly "$DB" "$ESC_DIR")"
assert "S7: _snapshot_readonly is stable when nothing mutates" \
    test "$_s7_before" = "$(_snapshot_readonly "$DB" "$ESC_DIR")"

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI contract + empty-input degradation
#
# The advisory-only posture is the load-bearing property here: EVERY valid
# invocation exits 0 (a sweep must never gate anything), and exit 2 is
# reserved exclusively for usage errors. A missing/unreadable input degrades
# to a zero-candidate report + a [warn], never an abort.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI contract + empty-input degradation ---"

# An empty fixture: production schema present, zero rows.
_mk_tasks_db
EMPTY_DB="$DB"
_mk_esc_dir
EMPTY_ESC="$ESC_DIR"
_mk_repo
EMPTY_REPO="$REPO_DIR"

# The canonical all-zero summary, asserted as one exact string so a counter
# rename or reordering is caught rather than silently tolerated.
ZERO_SWEEP="SWEEP: candidates=0 gate_closure=0 merge_verify_red=0 unmet_dependency=0 corrupt_hold=0 live_skipped=0 unknown=0"

# Every value-taking flag, in one place: the A1 usage-completeness check and
# the A3 missing-value sweep both iterate this list, so a future flag cannot
# be added to the parser without both checks noticing.
VALUE_FLAGS=(--db --tag --escalations --repo --main-ref --format --class --stale-heartbeat-min --emit-requests)

# _usage_names_all_flags — every VALUE_FLAGS entry appears in the usage text.
# One assert over the whole set (rather than one per flag) keeps the fork
# count of this `pool`-classified suite low; the failure message names the
# specific flags that were missing.
_usage_names_all_flags() {
    local f missing=()
    for f in "${VALUE_FLAGS[@]}"; do
        printf '%s' "$ERR_OUT" | grep -qF -- "$f" || missing+=("$f")
    done
    [ "${#missing[@]}" -eq 0 ] || { echo "usage text is missing: ${missing[*]}"; return 1; }
}

# --- A1: -h / --help -> usage on STDERR, stdout empty, exit 0 -----------------
for _h in -h --help; do
    run_sweep "$_h"
    assert "A1[$_h]: exits 0" _rc_is 0
    assert "A1[$_h]: stdout stays empty (usage goes to stderr)" _out_empty
    assert "A1[$_h]: usage block is printed to stderr" _err_has '^Usage: '
    assert "A1[$_h]: usage names every value-taking flag" _usage_names_all_flags
done

# --- A2: unknown flag -> exit 2, [error] naming the flag ----------------------
run_sweep --bogus
assert "A2: unknown flag exits 2" _rc_is 2
assert "A2: unknown flag produces an [error] line naming the flag" _err_has '\[error\].*--bogus'

# --- A3: every value-taking flag requires a value ----------------------------
# One assert over the whole set, for the same fork-count reason as A1; the
# failure message names the flags that did NOT exit 2.
_all_value_flags_require_a_value() {
    local f bad=()
    for f in "${VALUE_FLAGS[@]}"; do
        run_sweep "$f"
        [ "$RC" = 2 ] || bad+=("$f(rc=$RC)")
    done
    [ "${#bad[@]}" -eq 0 ] || { echo "flags that did not exit 2 on a missing value: ${bad[*]}"; return 1; }
}
assert "A3: every value-taking flag exits 2 when its value is missing" \
    _all_value_flags_require_a_value

# --- A4: enum / numeric validation ------------------------------------------
run_sweep --db "$EMPTY_DB" --format bogus
assert "A4a: --format bogus exits 2" _rc_is 2
run_sweep --db "$EMPTY_DB" --class bogus
assert "A4b: --class bogus exits 2" _rc_is 2
run_sweep --db "$EMPTY_DB" --stale-heartbeat-min abc
assert "A4c: --stale-heartbeat-min abc exits 2" _rc_is 2
run_sweep --db "$EMPTY_DB" --stale-heartbeat-min -1
assert "A4d: --stale-heartbeat-min -1 exits 2" _rc_is 2

# --- A5: unreadable DB degrades, never aborts -------------------------------
run_sweep --db "$EMPTY_REPO/definitely-not-here.db" --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO"
assert "A5a: a nonexistent --db still exits 0 (advisory-only)" _rc_is 0
assert "A5a: a nonexistent --db reports zero candidates" _sweep_line_is "$ZERO_SWEEP"
assert "A5a: a nonexistent --db warns on stderr" _err_has '\[warn\]'

_A5_STUB="$(mktemp "${TMPDIR:-/tmp}/gate-staleness-stub-XXXXXX.db")"
_TMPDIRS+=("$_A5_STUB")
: > "$_A5_STUB"
run_sweep --db "$_A5_STUB" --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO"
assert "A5b: a 0-byte DB stub still exits 0" _rc_is 0
assert "A5b: a 0-byte DB stub reports zero candidates" _sweep_line_is "$ZERO_SWEEP"
assert "A5b: a 0-byte DB stub warns on stderr" _err_has '\[warn\]'

# --- A6: empty fixture, table format ----------------------------------------
run_sweep --db "$EMPTY_DB" --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO" --format table
assert "A6: empty fixture exits 0" _rc_is 0
assert "A6: trailing SWEEP: line carries all seven counters at zero" _sweep_line_is "$ZERO_SWEEP"

# --- A7: empty fixture, json format -----------------------------------------
run_sweep --db "$EMPTY_DB" --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO" --format json
assert "A7: --format json exits 0" _rc_is 0
assert "A7: stdout is a single valid JSON object" _out_json_check 'isinstance(d, dict)'
assert "A7: candidates == []" _out_json_check 'd["candidates"] == []'
assert "A7: summary keys exactly match the A6 counter set" _out_json_check \
    'sorted(d["summary"]) == ["candidates","corrupt_hold","gate_closure","live_skipped","merge_verify_red","unknown","unmet_dependency"]'
assert "A7: every summary counter is 0" _out_json_check 'all(v == 0 for v in d["summary"].values())'

# --- A8: flag <-> env parity for the task-DB knob ---------------------------
run_sweep --db "$EMPTY_DB" --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO"
_A8_FLAG_OUT="$OUT"
_SWEEP_ENV=(REIFY_LANE_TASK_DB="$EMPTY_DB")
run_sweep --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO"
_SWEEP_ENV=()
assert "A8a: REIFY_LANE_TASK_DB produces byte-identical stdout to --db" \
    test "$OUT" = "$_A8_FLAG_OUT"

# An explicit --db must WIN over the env value: point the env at a
# nonexistent path and the flag at the good fixture — no [warn] must fire.
_SWEEP_ENV=(REIFY_LANE_TASK_DB="$EMPTY_REPO/env-not-here.db")
run_sweep --db "$EMPTY_DB" --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO"
_SWEEP_ENV=()
assert "A8b: an explicit --db overrides REIFY_LANE_TASK_DB" _rc_is 0
assert "A8b: the overridden (bad) env path never warns" _not _err_has '\[warn\].*env-not-here'

# --- A9: every --class value is accepted -------------------------------------
for _c in all gate_closure merge_verify_red unmet_dependency; do
    run_sweep --db "$EMPTY_DB" --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO" --class "$_c"
    assert "A9[$_c]: accepted on the empty fixture (exit 0)" _rc_is 0
done

test_summary
