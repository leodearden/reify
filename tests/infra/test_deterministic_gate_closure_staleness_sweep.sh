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
#   S — scaffolding + fixture-builder self-checks
#   A — CLI contract + empty-input degradation
#   B — the liveness guard
#   C — trigger class A (gate_closure)
#   D — trigger class B (merge_verify_red)
#   E — trigger class C (unmet_dependency)
#   F — the #5316 corruption suppressors
#   G — --emit-requests + the read-only proof
#   R — request retraction (the only block that mutates its fixture between runs)
#   T — tag scoping
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

# _sq <args...> — the sqlite3 CLI with LD_LIBRARY_PATH cleared.
#
# EVERY sqlite3 invocation in this file must go through this wrapper. verify.sh's
# apply_env() exports LD_LIBRARY_PATH=/opt/reify-deps/lib globally (for OCCT), and
# that directory also ships a conda libsqlite3 NEWER (3.53.1) than the one
# /usr/bin/sqlite3 was linked against (3.45.1). Under the full verify environment
# the loader hands the CLI the newer lib and it aborts with "SQLite header and
# source version mismatch", so the first fixture build here died and took the whole
# suite down under `set -e` — while passing in any dev shell whose PATH resolves
# sqlite3 to a build that tolerates the swap. That dual condition (hostile
# LD_LIBRARY_PATH *and* PATH->/usr/bin/sqlite3) is why it only ever reproduced
# under the merge gate. Same hazard and same fix as the fixture builders in
# tests/infra/test_reify_audit_ptodo.sh (esc-4581-87).
_sq() { LD_LIBRARY_PATH="" sqlite3 "$@"; }

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
    _sq "$DB" "INSERT INTO tasks (tag,id,title,status,metadata,updated_at,heartbeat_at,claimant_run_id)
        VALUES ($(_sq_quote "$tag"),$id,'fixture task $id',$(_sq_quote "$status"),$meta_sql,
                $(_sq_quote "$(_now_iso -3600)"),$hb_sql,$claimant_sql);"
}

# _add_dep <task_id> <depends_on> [tag]
_add_dep() {
    local task_id="$1" depends_on="$2" tag="${3:-master}"
    _sq "$DB" "INSERT INTO dependencies (tag,task_id,depends_on)
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
_sweep_line() { grep '^SWEEP:' <<<"$OUT" | tail -1; }
_sweep_line_is() { test "$(_sweep_line)" = "$1"; }

# _out_json_check <expr> — _json_check against the captured stdout.
_out_json_check() { python3 "$_JSON_CHECK_PY" - "$1" <<<"$OUT"; }
# _json_is is the same predicate, named for readability at the assert site.
_json_is() { _out_json_check "$1"; }

# _no_request_for <dir> <task_id> — no request file was emitted for that task.
_no_request_for() {
    local dir="$1" id="$2" hits
    hits="$(find "$dir" -maxdepth 1 -name "redispatch-${id}-*.json" 2>/dev/null)"
    [ -z "$hits" ] || { echo "unexpected request file(s): $hits"; return 1; }
}

# _json_check <json_file|-> <python_expr> — parse the JSON and evaluate
# <python_expr> with `d` bound to the parsed document; exit 0 iff truthy. A
# standalone .py checker (rather than an inline `python3 -c`) keeps JSON
# assertions free of nested-quoting hazards inside assert "..." invocations.
_JSON_CHECK_PY="$(mktemp "${TMPDIR:-/tmp}/gate-staleness-jsoncheck-XXXXXX.py")"
_TMPDIRS+=("$_JSON_CHECK_PY")
# Bound names inside <python_expr>:
#   d — the parsed document
#   t — {task_id: candidate} index (report documents only)
#   s — the summary object    (report documents only)
cat > "$_JSON_CHECK_PY" <<'PY'
import json, sys
src = sys.stdin if sys.argv[1] == "-" else open(sys.argv[1])
d = json.load(src)
t, s = {}, {}
if isinstance(d, dict):
    t = {c["task_id"]: c for c in d.get("candidates", [])}
    s = d.get("summary", {})
sys.exit(0 if eval(sys.argv[2]) else 1)
PY
_json_check() { python3 "$_JSON_CHECK_PY" "$1" "$2"; }

# Assert-arg helpers. assert() runs its command via "$@", so a shell keyword
# (`!`) or a redirect (`<<<`) cannot appear in an assert argument list — these
# wrap those forms as real commands.
_not() { ! "$@"; }
# NOTE: a here-string, deliberately NOT `printf ... | grep -q`. Under
# `set -o pipefail`, grep -q exits on the FIRST match and the upstream printf
# then dies of SIGPIPE (141), which pipefail propagates as the pipeline's
# status — so a successful match intermittently reads as a failure. The race
# is load-dependent, which made it a genuine heisenflake here.
_matches() { grep -qE "$1" <<<"$2"; }

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
_s1_tables="$(_sq "$DB" "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name;" | tr '\n' ',')"
assert "S1b: fixture DB has both production tables (got: $_s1_tables)" \
    test "$_s1_tables" = "dependencies,tasks,"

# S2 — _add_task / _add_dep round-trip, including the NULL-heartbeat blocked
# shape and the live in-progress claimant shape.
_add_task 9001 blocked '{"task_kind":"deterministic","always_escalates":true}'
_add_task 9002 in-progress '{}' "$(_now_iso -60)" "run-7c8e838c39e7/9002-c514828d/pid=3551081"
_add_dep 9001 9002
assert "S2a: blocked fixture row round-trips with a NULL heartbeat" \
    test "$(_sq "$DB" "SELECT status||'|'||coalesce(heartbeat_at,'NULL') FROM tasks WHERE tag='master' AND id=9001;")" \
       = "blocked|NULL"
assert "S2b: in-progress fixture row carries the live claimant shape" \
    test "$(_sq "$DB" "SELECT claimant_run_id FROM tasks WHERE tag='master' AND id=9002;")" \
       = "run-7c8e838c39e7/9002-c514828d/pid=3551081"
assert "S2c: metadata round-trips as queryable JSON" \
    test "$(_sq "$DB" "SELECT json_extract(metadata,'\$.task_kind') FROM tasks WHERE tag='master' AND id=9001;")" \
       = "deterministic"
assert "S2d: dependency row round-trips tag-scoped" \
    test "$(_sq "$DB" "SELECT depends_on FROM dependencies WHERE tag='master' AND task_id=9001;")" = "9002"

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
ZERO_SWEEP="SWEEP: candidates=0 gate_closure=0 merge_verify_red=0 unmet_dependency=0 corrupt_hold=0 live_skipped=0 no_class=0 unknown=0"

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
        # Here-string, not a pipe — see the _matches SIGPIPE/pipefail note.
        grep -qF -- "$f" <<<"$ERR_OUT" || missing+=("$f")
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
assert "A6: trailing SWEEP: line carries all eight counters at zero" _sweep_line_is "$ZERO_SWEEP"

# --- A7: empty fixture, json format -----------------------------------------
run_sweep --db "$EMPTY_DB" --escalations "$EMPTY_ESC" --repo "$EMPTY_REPO" --format json
assert "A7: --format json exits 0" _rc_is 0
assert "A7: stdout is a single valid JSON object" _out_json_check 'isinstance(d, dict)'
assert "A7: candidates == []" _out_json_check 'd["candidates"] == []'
assert "A7: summary keys exactly match the A6 counter set" _out_json_check \
    'sorted(d["summary"]) == ["candidates","corrupt_hold","gate_closure","live_skipped","merge_verify_red","no_class","unknown","unmet_dependency"]'
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

# ──────────────────────────────────────────────────────────────────────────────
# Block B — the liveness guard
#
# The suite's highest-consequence safety property: a false hit here would
# re-dispatch a task an agent is actively running. Measured live on
# 2026-07-26, ALL TEN in-progress tasks had every dependency `done` (task 5321
# itself among them), so a naive "premise resolved => re-dispatch" rule would
# have targeted ten live agents. heartbeat_at/claimant_run_id discriminate,
# and the fail-safe direction for a re-dispatch decision is always "do
# nothing" — hence an UNPARSEABLE heartbeat degrades to LIVE (B7), never to
# eligible.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: the liveness guard ---"

_mk_tasks_db
B_DB="$DB"
_mk_esc_dir
B_ESC="$ESC_DIR"
_mk_repo
B_REPO="$REPO_DIR"

# The live claimant shape, copied from the rows measured on 2026-07-26:
#   run-<runid>/<taskid>-<hash>/pid=<pid>
B_CLAIMANT="run-7c8e838c39e7/9101-c514828d/pid=3551081"

# The liveness window every Block-B sweep runs with (minutes). Named once
# so the fixture offsets and the flag can never drift apart.
B_WINDOW_MIN=1440

# A merge_verify_red proposal + a satisfied dependency on 9101, so the row
# would fire TWO classes if the liveness guard were not applied first.
B_PROPOSAL='{"dry_run_proposals":[{"block_reason":"Post-merge verification failed: cargo test",
"main_sha":"deadbeef","files_referenced":["tests/infra/test_warm_lane_pool_config.sh"],
"investigated_at":"2026-07-26T08:00:00Z","timestamp":"2026-07-26T08:00:00Z"}]}'

_add_task 9199 done '{}'
# 9101 — fresh heartbeat (60s) + live claimant => LIVE, despite both premises.
_add_task 9101 in-progress "$B_PROPOSAL" "$(_now_iso -60)" "$B_CLAIMANT"
_add_dep 9101 9199
# 9102 — the 5196 shape: a stale heartbeat with NO claimant => eligible.
_add_task 9102 in-progress '{}' "$(_now_iso -90000)" ""
# 9103 — blocked rows carry no heartbeat at all => eligible.
_add_task 9103 blocked '{}'
# 9104 / 9105 — the boundary, from both sides.
#
# DRIFT-PROOFING: heartbeat offsets are frozen when the fixture is BUILT, but
# the sweeps that assert on them run later, so a boundary row drifts toward
# the window edge by however long the suite takes. A tight 29m/31m pair around
# a 30m window flipped 9104 from LIVE to eligible under host load — a real
# race in the fixture, not in the SUT. The relation under test is just
# `now - heartbeat < window`, so it is asserted with a 1h margin on each side
# of a 24h window (B_WINDOW_MIN), which no plausible suite runtime can cross.
_add_task 9104 in-progress '{}' "$(_now_iso -82800)" "$B_CLAIMANT"
_add_task 9105 in-progress '{}' "$(_now_iso -90000)" "$B_CLAIMANT"
# 9106 — an unparseable heartbeat must fail SAFE (LIVE), never eligible.
_add_task 9106 in-progress '{}' "not-a-timestamp" "$B_CLAIMANT"
# 9107 — a CLAIMED in-progress row that has not yet written (or has lost) its
# heartbeat: claimant populated, heartbeat_at NULL. It carries $B_PROPOSAL so
# it WOULD fire class B, mirroring 9101 — the guard is what suppresses it,
# rather than the fixture being inert. Re-dispatching a claimed, actively-
# running agent is the single most destructive thing this sweep can do, so the
# fail-safe direction is the same one L1 already takes for an UNPARSEABLE
# heartbeat: degrade to LIVE.
_add_task 9107 in-progress "$B_PROPOSAL" "" "$B_CLAIMANT"
# 9108 — the SCOPING guard for that rule. A `blocked` row with a claimant and
# no heartbeat must stay ELIGIBLE: classes A and C are blocked-only per L4, so
# extending the claimant rule to `blocked` would blind them wholesale, and the
# live store's blocked rows legitimately carry no heartbeat.
_add_task 9108 blocked '{}' "" "$B_CLAIMANT"

run_sweep --db "$B_DB" --escalations "$B_ESC" --repo "$B_REPO" \
    --stale-heartbeat-min "$B_WINDOW_MIN" --format json

assert "B0: the liveness fixture sweep exits 0" _rc_is 0

# --- B1: a fresh heartbeat is LIVE even with two premises resolved -----------
assert "B1: a 60s-old heartbeat with a live claimant is LIVE" \
    _json_is 't[9101]["verdict"] == "LIVE"'
assert "B1: a LIVE row is not classified (class=-, action=none)" \
    _json_is 't[9101]["class"] == "-" and t[9101]["action"] == "none"'
assert "B1: a LIVE row is counted in live_skipped, not in any class counter" \
    _json_is 's["live_skipped"] == 4 and s["merge_verify_red"] == 0 and s["unmet_dependency"] == 0'

# --- B1c/B1d: a CLAIMED in-progress row with no heartbeat is LIVE ------------
# `merge_verify_red == 0` above is the sharpest signal for these: 9107 carries
# $B_PROPOSAL, so without the claimant rule it is eligible, class B fires, and
# that counter reads 1.
assert "B1c: an in-progress row with a claimant and NO heartbeat is LIVE" \
    _json_is 't[9107]["verdict"] == "LIVE"'
assert "B1c: ... and is therefore not classified (class=-, action=none)" \
    _json_is 't[9107]["class"] == "-" and t[9107]["action"] == "none"'
# The evidence must name the claimant as the liveness signal. It must NOT claim
# a heartbeat fell inside the window: the row has no heartbeat at all, so the
# generic LIVE evidence string would render a misleading bare `heartbeat_at=`.
assert "B1d: claimant-based LIVE evidence names the claimant" \
    _json_is "'$B_CLAIMANT' in t[9107]['evidence']"
assert "B1d: ... and does not claim a heartbeat matched the window" \
    _json_is '"within the" not in t[9107]["evidence"]'

# --- B2: a LIVE row never yields a request file ------------------------------
B_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-req-XXXXXX")"
_TMPDIRS+=("$B_REQ")
run_sweep --db "$B_DB" --escalations "$B_ESC" --repo "$B_REPO" \
    --stale-heartbeat-min "$B_WINDOW_MIN" --emit-requests "$B_REQ"
assert "B2: a LIVE row emits no re-dispatch request" _no_request_for "$B_REQ" 9101
assert "B2: a claimant-based LIVE row emits no re-dispatch request either" \
    _no_request_for "$B_REQ" 9107

run_sweep --db "$B_DB" --escalations "$B_ESC" --repo "$B_REPO" \
    --stale-heartbeat-min "$B_WINDOW_MIN" --format json

# --- B3/B4: genuinely stranded rows stay eligible ----------------------------
assert "B3: a stale heartbeat with NO claimant (the 5196 shape) is eligible" \
    _json_is 't[9102]["verdict"] != "LIVE"'
assert "B4: a blocked row with heartbeat_at NULL is eligible" \
    _json_is 't[9103]["verdict"] != "LIVE"'
# The scoping half of B4: the claimant rule is `in-progress`-only, so adding a
# claimant to a heartbeat-less BLOCKED row must not make it LIVE. Were it to,
# classes A and C — blocked-only per L4 — would be blinded wholesale.
assert "B4: ... and stays eligible even when it carries a claimant" \
    _json_is 't[9108]["verdict"] != "LIVE"'

# --- B5: the boundary, from both sides, with no sleep ------------------------
assert "B5a: inside the window (23h < 24h) => LIVE" _json_is 't[9104]["verdict"] == "LIVE"'
assert "B5b: outside the window (25h > 24h) => eligible" _json_is 't[9105]["verdict"] != "LIVE"'

# --- B6: flag <-> env parity for the window ---------------------------------
_SWEEP_ENV=(REIFY_GATE_STALENESS_HEARTBEAT_MIN=0)
run_sweep --db "$B_DB" --escalations "$B_ESC" --repo "$B_REPO" --format json
_SWEEP_ENV=()
assert "B6a: REIFY_GATE_STALENESS_HEARTBEAT_MIN=0 makes a 60s heartbeat eligible" \
    _json_is 't[9101]["verdict"] != "LIVE"'

_SWEEP_ENV=(REIFY_GATE_STALENESS_HEARTBEAT_MIN=0)
run_sweep --db "$B_DB" --escalations "$B_ESC" --repo "$B_REPO" \
    --stale-heartbeat-min "$B_WINDOW_MIN" --format json
_SWEEP_ENV=()
assert "B6b: an explicit --stale-heartbeat-min overrides the env value" \
    _json_is 't[9101]["verdict"] == "LIVE"'

# --- B7: an unparseable heartbeat fails SAFE --------------------------------
run_sweep --db "$B_DB" --escalations "$B_ESC" --repo "$B_REPO" \
    --stale-heartbeat-min "$B_WINDOW_MIN" --format json
assert "B7: an unparseable heartbeat degrades to LIVE, never to eligible" \
    _json_is 't[9106]["verdict"] == "LIVE"'
assert "B7: an unparseable heartbeat is counted in live_skipped, not unknown" \
    _json_is 's["live_skipped"] == 4 and s["unknown"] == 0'
# The four eligible rows here (9102/9103/9105/9108) carry no class-matching
# metadata at all, so they land in no_class — NOT in unknown, which is reserved
# for a class that matched and whose oracle then failed. Pinned as an exact
# count so a regression that folded the two back together is caught here as
# well as at G3.
assert "B7: the eligible-but-class-less rows are counted in no_class, not unknown" \
    _json_is 's["no_class"] == 4'
assert "B7: an unparseable heartbeat warns on stderr" \
    _err_has '\[warn\].*9106'

# ──────────────────────────────────────────────────────────────────────────────
# Block C — trigger class A: gate_closure
#
# This task's original scope: a deterministic always-escalates gate task left
# `blocked` after its gating escalation was resolved or dismissed elsewhere.
# The staleness signal is the ABSENCE of a live `status=pending`
# esc-<id>-*.json — validated against the live store on 2026-07-26, where
# 5537/5549/5559 were blocked with always_escalates=true and zero live
# escalation files (theirs archived `dismissed`), while every other blocked
# gate task still had a live pending file.
#
# The emitted action is `close`, not `redispatch`: per #5316 §5 the correct
# closure for a satisfied deterministic gate is a transition to `cancelled`,
# not a re-run.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: trigger class A (gate_closure) ---"

_mk_tasks_db
C_DB="$DB"
_mk_esc_dir
C_ESC="$ESC_DIR"
_mk_repo
C_REPO="$REPO_DIR"

# The class-A metadata predicate, as recorded on the live blocked gate tasks.
C_GATE_META='{"task_kind":"deterministic","always_escalates":true,"gate_escalated_at":"2026-07-26T08:00:00Z"}'
# The 5372 shape: deterministic, but with no always_escalates key at all.
C_NO_ALWAYS_META='{"task_kind":"deterministic","gate_escalated_at":"2026-07-26T08:00:00Z"}'

_add_task 9201 blocked "$C_GATE_META"                         # no esc files    => STALE
_add_task 9202 blocked "$C_GATE_META"                         # pending         => GATED
_add_task 9203 blocked "$C_GATE_META"                         # dismissed       => STALE
_add_task 9204 blocked "$C_GATE_META"                         # resolved        => STALE
_add_task 9205 blocked "$C_GATE_META"                         # dismissed+pending => GATED
_add_task 9206 blocked "$C_NO_ALWAYS_META"                    # not class A
_add_task 9207 in-progress "$C_GATE_META" "$(_now_iso -10800)" ""   # blocked-only => not class A
_add_task 9208 blocked "$C_GATE_META"                         # malformed esc   => unknown
_add_task 9209 blocked "$C_GATE_META"                         # unrecognized st => unknown
_add_task 9210 blocked "$C_GATE_META"                         # null status     => unknown
_add_task 9211 blocked "$C_GATE_META"                         # absent status   => unknown

_add_esc 9202 1 pending
_add_esc 9203 1 dismissed
_add_esc 9204 1 resolved
_add_esc 9205 1 dismissed
_add_esc 9205 2 pending
printf 'this is not json\n' > "$C_ESC/esc-9208-1.json"
# A well-formed record carrying a status OUTSIDE the store's `pending /
# resolved / dismissed` vocabulary (models.py:100). A schema addition on the
# escalation side produces exactly this shape.
printf '{"id":"esc-9209-1","task_id":"9209","status":"in_triage"}\n' > "$C_ESC/esc-9209-1.json"
# A well-formed record whose status is JSON null — the live store holds one
# such file today (data/escalations/b3-state.json), which escapes the sweep's
# glob only by name, not by shape.
printf '{"id":"esc-9210-1","task_id":"9210","status":null}\n' > "$C_ESC/esc-9210-1.json"
# A well-formed record with NO `status` key AT ALL — the mid-write shape, and
# the one case whose ROUTE through _esc_state changes when the parse sentinel
# stops being a NUL. `.get("status","")` yields the empty string, which today
# collides with the NUL sentinel (bash strips the NUL from BOTH the capture and
# the `case` pattern, so the arm degenerates to ""-matches-"") and so lands on
# the parse-error arm; once the sentinel is a literal token it will land on the
# `*)` unrecognized-status arm instead. The VERDICT is `unknown` either way —
# that equivalence is what C10g/C10h pin, so the sentinel change is provably
# behaviour-preserving rather than merely asserted to be.
printf '{"id":"esc-9211-1","task_id":"9211"}\n' > "$C_ESC/esc-9211-1.json"
# A .json.lock sidecar (the live store holds these next to the real files):
# the glob must match *.json only, never *.json.lock, or 9201 would read as
# gated by a lock file.
printf 'lock\n' > "$C_ESC/esc-9201-1.json.lock"

run_sweep --db "$C_DB" --escalations "$C_ESC" --repo "$C_REPO" --format json
assert "C0: the class-A fixture sweep exits 0" _rc_is 0

# --- C1: zero live escalation files => STALE + close -------------------------
assert "C1: a blocked always-escalates gate task with no live escalation is a class-A hit" \
    _json_is 't[9201]["class"] == "gate_closure" and t[9201]["verdict"] == "STALE"'
assert "C1: the emitted action is close (not redispatch) per #5316 §5" \
    _json_is 't[9201]["action"] == "close"'
assert "C1: evidence names the absence of a live pending escalation" \
    _json_is '"no live pending escalation" in t[9201]["evidence"]'
assert "C1: a *.json.lock sidecar does not read as a gating escalation" \
    _json_is 't[9201]["verdict"] == "STALE"'

# --- C2/C3/C4: the escalation-state oracle ----------------------------------
assert "C2: a live status=pending escalation still gates the task" \
    _json_is 't[9202]["verdict"] == "GATED"'
assert "C3a: a dismissed escalation is stale (the live 5537/5549/5559 shape)" \
    _json_is 't[9203]["verdict"] == "STALE"'
assert "C3b: a resolved escalation is stale" \
    _json_is 't[9204]["verdict"] == "STALE"'
assert "C4: any single pending escalation gates, even alongside a dismissed one" \
    _json_is 't[9205]["verdict"] == "GATED"'
assert "C2/C4: a GATED row is not counted as a hit" \
    _json_is 's["gate_closure"] == 3'

# --- C5/C6: the class-A predicate is narrow ---------------------------------
assert "C5: no always_escalates key (the 5372 shape) is not class A" \
    _json_is 't[9206]["class"] != "gate_closure"'
assert "C6: an in-progress row matching A's metadata is not class A (blocked-only)" \
    _json_is 't[9207]["class"] != "gate_closure"'

# --- C7/C8: a failed oracle degrades to unknown, never to STALE -------------
C_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-creq-XXXXXX")"
_TMPDIRS+=("$C_REQ")
run_sweep --db "$C_DB" --escalations "$C_REPO/no-such-escalations-dir" --repo "$C_REPO" \
    --emit-requests "$C_REQ" --format json
assert "C7: a nonexistent --escalations dir still exits 0" _rc_is 0
assert "C7: a missing oracle degrades every class-A candidate to unknown, never STALE" \
    _json_is 't[9201]["verdict"] == "unknown" and s["gate_closure"] == 0'
assert "C7: a missing oracle is counted in unknown" _json_is 's["unknown"] >= 1'
assert "C7: a missing oracle warns on stderr" _err_has '\[warn\]'
assert "C7: a missing oracle emits no re-dispatch request" _no_request_for "$C_REQ" 9201

C_REQ2="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-creq2-XXXXXX")"
_TMPDIRS+=("$C_REQ2")
run_sweep --db "$C_DB" --escalations "$C_ESC" --repo "$C_REPO" \
    --emit-requests "$C_REQ2" --format json
assert "C8: a malformed escalation file degrades to unknown, not STALE" \
    _json_is 't[9208]["verdict"] == "unknown"'
assert "C8: a malformed escalation file warns on stderr" _err_has '\[warn\].*9208'
# Deliberately adjacent to the assert above so it reads the SAME run's ERR_OUT.
# The parse sentinel must survive command substitution: bash strips a NUL from
# `$(...)` and warns while doing it, and that warning lands on the SUT's OWN
# stderr — the very channel `warn()` writes to and every `_err_has` assert here
# reads. Diagnostics a consumer cannot distinguish from the tool's own output
# are a defect in the tool, not noise.
assert "C8: the parse sentinel leaks no bash NUL warning onto the SUT's own stderr" \
    _not _err_has 'ignored null byte'

# --- C10: the status predicate is a TERMINAL allowlist, not a pending one ----
# `pending` gates and `resolved`/`dismissed` clear; EVERY other value —
# unrecognized, null, empty — is a failed oracle read and must degrade to
# `unknown`. A pending-allowlist would sink all of these into `clear`, which
# yields STALE / close / an emitted request telling the consumer to CANCEL the
# task — failing open toward the sweep's single most destructive action, and
# inverting invariant L2 ("a failed oracle lookup must never manufacture an
# actionable verdict"). Both shapes below are realistic: a schema addition on
# the escalation side, and a mid-write/partially-populated record.
assert "C10a: an unrecognized escalation status degrades to unknown, not STALE" \
    _json_is 't[9209]["verdict"] == "unknown"'
assert "C10b: an unrecognized status warns, naming the task and the status" \
    _err_has '\[warn\].*9209.*in_triage'
assert "C10c: an unrecognized status emits no re-dispatch request" \
    _no_request_for "$C_REQ2" 9209
assert "C10d: a null escalation status degrades to unknown, not STALE" \
    _json_is 't[9210]["verdict"] == "unknown"'
assert "C10e: a null status emits no re-dispatch request" \
    _no_request_for "$C_REQ2" 9210
assert "C10g: an ABSENT status key degrades to unknown, not STALE" \
    _json_is 't[9211]["verdict"] == "unknown"'
assert "C10h: an absent status key emits no re-dispatch request" \
    _no_request_for "$C_REQ2" 9211
assert "C10f: none of these shapes is counted as a class-A hit" \
    _json_is 's["gate_closure"] == 3'

# --- C11: an UNREADABLE escalations dir degrades exactly like a missing one --
# C7 covers a NONEXISTENT dir. The dir that exists but cannot be enumerated
# (mode 000, root-owned dir swept by another uid, a stale mount) is the more
# dangerous shape, because it defeats the terminal allowlist WITHOUT ever
# entering the loop: `[ -d ]` succeeds (stat needs +x on the PARENT, not on the
# dir), the glob fails to expand for want of +r, `[ -e "$f" ] || continue`
# swallows the unexpanded pattern, and found=0/pending=0 falls straight into
# the `clear` branch. Task 9202 carries a live status=pending escalation, so
# reporting it STALE / close is a fail-open on a task whose gate IS still live
# — inverting L2 at the one point the allowlist cannot defend.
if [ "$(id -u)" != 0 ]; then
    C_REQ3="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-creq3-XXXXXX")"
    _TMPDIRS+=("$C_REQ3")
    chmod 000 "$C_ESC"
    run_sweep --db "$C_DB" --escalations "$C_ESC" --repo "$C_REPO" \
        --emit-requests "$C_REQ3" --format json
    chmod 700 "$C_ESC"
    assert "C11: an unreadable --escalations dir still exits 0" _rc_is 0
    assert "C11: a task with a LIVE pending escalation degrades to unknown, never STALE" \
        _json_is 't[9202]["verdict"] == "unknown"'
    assert "C11: an unreadable oracle yields no class-A hit at all" \
        _json_is 's["gate_closure"] == 0'
    assert "C11: an unreadable oracle is counted in unknown" _json_is 's["unknown"] >= 1'
    assert "C11: an unreadable oracle warns on stderr" _err_has '\[warn\].*9202'
    assert "C11: an unreadable oracle emits no re-dispatch request" \
        _no_request_for "$C_REQ3" 9202
    assert "C11: ... and none for the genuinely-stale row either" \
        _no_request_for "$C_REQ3" 9201
else
    echo "  SKIP: C11 unreadable-dir asserts (running as uid 0; mode bits do not apply)"
fi

# --- C9: --class filtering ---------------------------------------------------
run_sweep --db "$C_DB" --escalations "$C_ESC" --repo "$C_REPO" \
    --class gate_closure --format json
assert "C9a: --class gate_closure emits only class-A rows" \
    _json_is 'all(c["class"] == "gate_closure" for c in d["candidates"]) and len(d["candidates"]) > 0'
run_sweep --db "$C_DB" --escalations "$C_ESC" --repo "$C_REPO" \
    --class merge_verify_red --format json
assert "C9b: --class merge_verify_red suppresses the class-A hit" \
    _json_is '9201 not in t and s["gate_closure"] == 0'

# ──────────────────────────────────────────────────────────────────────────────
# Block D — trigger class B: merge_verify_red
#
# A post-merge-verification-failed block whose premise has since resolved on
# main. Unlike classes A and C this one spans `blocked` AND `in-progress`
# (D9), because a merge-verify red can strand a row in either state.
#
# "Premise resolved" is adjudicated by a REAL git diff, never by prose: the
# proposal records `main_sha` (where main was when the gate went red) and
# `files_referenced` (what the red implicated), so the question "has main
# since moved in a way that could change this verdict?" reduces to
# `git diff <main_sha>..<main-ref> -- <files_referenced>`. This reproduces
# 5236's recorded shape verbatim (main_sha=40bbe0b6e4, files_referenced
# naming tests/infra/test_warm_lane_pool_config.sh, premise later resolved by
# #5369 touching that very file).
#
# REACHABILITY IS CHECKED BEFORE THE DIFF (D8), per #5316 §3: a
# plausible-looking main_sha can be a DISCARDED DUPLICATE merge that passes a
# diff test and fails the ancestor test. That ordering is exactly what #5264
# was mis-cleared on when only `git show --stat` was consulted.
#
# Classification keys primarily off the `block_reason` prose prefix, with
# `block_class` as a confirming hint (D2) — measured live, `block_class` is
# present on only 2 of 55 dry_run_proposals entries, so keying on it alone
# would miss nearly every real merge_verify_red block.
#
# Every SHA below is COMPUTED from the fixture repo; none is frozen.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: trigger class B (merge_verify_red) ---"

_mk_tasks_db
D_DB="$DB"
_mk_esc_dir
D_ESC="$ESC_DIR"

# Fixture main history (linear):
#   c0 root
#   c1 docs/unrelated-a.md                          <- the recorded main_sha
#   c2 tests/infra/test_warm_lane_pool_config.sh    <- the RESOLVING commit
#   c3 docs/unrelated-b.md                          <- main tip
# plus a side-branch commit that is NOT an ancestor of main (the #5316
# Signature-2 / discarded-duplicate shape, reused here for D8).
_mk_repo
D_REPO="$REPO_DIR"
D_C1="$(_commit_touching docs/unrelated-a.md)"
D_C2="$(_commit_touching tests/infra/test_warm_lane_pool_config.sh)"
D_C3="$(_commit_touching docs/unrelated-b.md)"
D_SIDE="$(_commit_on_side_branch discarded-dup-d dark-factory-orchestrator.yaml)"

# A 40-hex SHA that resolves in no repo (D8's unresolvable case).
D_FAKE_SHA="3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f"

# files_referenced payloads. dark-factory-orchestrator.yaml exists ONLY on the
# side branch, so it is never touched anywhere in c1..c3 — which is what makes
# D4 a true negative rather than an accident of fixture ordering.
D_FILES_BOTH='["tests/infra/test_warm_lane_pool_config.sh","dark-factory-orchestrator.yaml"]'
D_FILES_TEST='["tests/infra/test_warm_lane_pool_config.sh"]'
D_FILES_YAML='["dark-factory-orchestrator.yaml"]'

# The class-B block_reason prose, in the live recorded shape.
D_REASON_B='Post-merge verification failed: cargo test --workspace returned 101'
# Non-class-B reasons, drawn verbatim from the live distinct-value set (D5).
D_REASON_ITER='Execution iterations exhausted'
D_REASON_STEWARD='Steward re-escalated to human'
D_REASON_THRASH='Repeated merge-phase thrash (counter=2)'
D_REASON_NOPLAN='Architect filed L0 without plan: no plan.json at handoff'

# _d_prop <block_reason> <main_sha> <files_json|-> <investigated_at> [block_class]
# One proposal object in the live dry_run_proposals shape. Pass `-` for
# files_json to OMIT the files_referenced key entirely (D7's absent-key case).
_d_prop() {
    local reason="$1" main_sha="$2" files="$3" at="$4" bc="${5:-}"
    local bc_frag="" files_frag=""
    if [ -n "$bc" ]; then bc_frag="\"block_class\":\"$bc\","; fi
    if [ "$files" != "-" ]; then files_frag="\"files_referenced\":$files,"; fi
    printf '{"proposal_text":"fixture proposal","risk_label":"medium",%s"block_reason":"%s","head_sha":"0123456789abcdef0123456789abcdef01234567","main_sha":"%s",%s"investigated_at":"%s","timestamp":"%s"}' \
        "$bc_frag" "$reason" "$main_sha" "$files_frag" "$at" "$at"
}

# _d_meta <proposal>... — wrap proposals into a metadata document, preserving
# the given ARRAY ORDER (D6 depends on array order and recency disagreeing).
_d_meta() {
    local out="" p
    for p in "$@"; do
        if [ -z "$out" ]; then out="$p"; else out="$out,$p"; fi
    done
    printf '{"dry_run_proposals":[%s]}' "$out"
}

# D1 — the 5236 shape: recorded at c1, resolved by c2 touching the named test.
_add_task 9301 blocked "$(_d_meta "$(_d_prop "$D_REASON_B" "$D_C1" "$D_FILES_BOTH" 2026-07-24T10:00:00Z)")"
# D2 — block_class hint present, block_reason does NOT match the prose prefix.
_add_task 9302 blocked "$(_d_meta "$(_d_prop 'Merge worker reported a red gate' "$D_C1" "$D_FILES_TEST" 2026-07-24T10:00:00Z merge_verify_red)")"
# D3 — recorded main_sha IS the current tip: main has not advanced.
_add_task 9303 blocked "$(_d_meta "$(_d_prop "$D_REASON_B" "$D_C3" "$D_FILES_TEST" 2026-07-24T10:00:00Z)")"
# D4 — main advanced (c2..c3) but no files_referenced path was touched in it.
_add_task 9304 blocked "$(_d_meta "$(_d_prop "$D_REASON_B" "$D_C2" "$D_FILES_YAML" 2026-07-24T10:00:00Z)")"
# D5 — a resolvable premise in every case; ONLY the block_reason differs.
_add_task 9305 blocked "$(_d_meta "$(_d_prop "$D_REASON_ITER"    "$D_C1" "$D_FILES_TEST" 2026-07-24T10:00:00Z)")"
_add_task 9306 blocked "$(_d_meta "$(_d_prop "$D_REASON_STEWARD" "$D_C1" "$D_FILES_TEST" 2026-07-24T10:00:00Z)")"
_add_task 9307 blocked "$(_d_meta "$(_d_prop "$D_REASON_THRASH"  "$D_C1" "$D_FILES_TEST" 2026-07-24T10:00:00Z)")"
_add_task 9308 blocked "$(_d_meta "$(_d_prop "$D_REASON_NOPLAN"  "$D_C1" "$D_FILES_TEST" 2026-07-24T10:00:00Z)")"
# D6 — array order and recency DISAGREE, in both directions, so neither
# proposals[0] nor proposals[-1] can satisfy both rows: only real
# investigated_at keying does.
#   9309: the newest entry is FIRST in the array and is not class B.
_add_task 9309 blocked "$(_d_meta \
    "$(_d_prop "$D_REASON_ITER" "$D_C1" "$D_FILES_TEST" 2026-07-25T10:00:00Z)" \
    "$(_d_prop "$D_REASON_B"    "$D_C1" "$D_FILES_TEST" 2026-07-23T10:00:00Z)" \
    "$(_d_prop "$D_REASON_B"    "$D_C1" "$D_FILES_TEST" 2026-07-21T10:00:00Z)")"
#   9310: the newest entry is LAST in the array and is a resolved class-B red.
_add_task 9310 blocked "$(_d_meta \
    "$(_d_prop "$D_REASON_ITER" "$D_C1" "$D_FILES_TEST" 2026-07-21T10:00:00Z)" \
    "$(_d_prop "$D_REASON_B"    "$D_C1" "$D_FILES_TEST" 2026-07-25T10:00:00Z)")"
# D7 — no diff surface to adjudicate: empty list, then the key absent entirely.
_add_task 9311 blocked "$(_d_meta "$(_d_prop "$D_REASON_B" "$D_C1" '[]' 2026-07-24T10:00:00Z)")"
_add_task 9312 blocked "$(_d_meta "$(_d_prop "$D_REASON_B" "$D_C1" '-'  2026-07-24T10:00:00Z)")"
# D8 — reachability failures: a SHA in no repo, and a real-but-side-branch SHA.
_add_task 9313 blocked "$(_d_meta "$(_d_prop "$D_REASON_B" "$D_FAKE_SHA" "$D_FILES_TEST" 2026-07-24T10:00:00Z)")"
_add_task 9314 blocked "$(_d_meta "$(_d_prop "$D_REASON_B" "$D_SIDE"     "$D_FILES_TEST" 2026-07-24T10:00:00Z)")"
# D9 — class B spans in-progress (with a 25h-stale heartbeat, well outside the
# 30m default window, so the fixture cannot drift into the liveness guard).
_add_task 9315 in-progress "$(_d_meta "$(_d_prop "$D_REASON_B" "$D_C1" "$D_FILES_TEST" 2026-07-24T10:00:00Z)")" \
    "$(_now_iso -90000)" ""

D_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-dreq-XXXXXX")"
_TMPDIRS+=("$D_REQ")

run_sweep --db "$D_DB" --escalations "$D_ESC" --repo "$D_REPO" \
    --emit-requests "$D_REQ" --format json
assert "D0: the class-B fixture sweep exits 0" _rc_is 0

# --- D1: the 5236 shape — premise resolved by a later main commit ------------
assert "D1: a post-merge-verify red whose referenced path was touched later is class B" \
    _json_is 't[9301]["class"] == "merge_verify_red" and t[9301]["verdict"] == "STALE"'
assert "D1: the emitted action is reverify" _json_is 't[9301]["action"] == "reverify"'
assert "D1: evidence names the resolving commit SHA and the touched path" \
    _json_is "'${D_C2:0:7}' in t[9301]['evidence'] and 'test_warm_lane_pool_config.sh' in t[9301]['evidence']"

# --- D2: block_class is a confirming hint, not the primary key ---------------
assert "D2: block_class=merge_verify_red classifies even without the prose prefix" \
    _json_is 't[9302]["class"] == "merge_verify_red" and t[9302]["verdict"] == "STALE"'

# --- D3/D4: the premise has NOT resolved ------------------------------------
assert "D3: main_sha == the --main-ref tip => main has not advanced => UNRESOLVED" \
    _json_is 't[9303]["class"] == "merge_verify_red" and t[9303]["verdict"] == "UNRESOLVED"'
assert "D4: main advanced but no files_referenced path was touched => UNRESOLVED" \
    _json_is 't[9304]["class"] == "merge_verify_red" and t[9304]["verdict"] == "UNRESOLVED"'

# --- D5: a different block class is not class B ------------------------------
assert "D5: none of the four live non-merge-verify block reasons classify as class B" \
    _json_is 'all(t[i]["class"] != "merge_verify_red" for i in (9305, 9306, 9307, 9308))'

# --- D6: only the NEWEST proposal is classified ------------------------------
assert "D6a: a non-class-B newest proposal wins over older class-B ones" \
    _json_is 't[9309]["class"] != "merge_verify_red"'
assert "D6b: a class-B newest proposal wins over an older non-class-B one" \
    _json_is 't[9310]["class"] == "merge_verify_red" and t[9310]["verdict"] == "STALE"'

# --- D7: no diff surface => unknown, never STALE -----------------------------
assert "D7a: an empty files_referenced degrades to unknown" \
    _json_is 't[9311]["verdict"] == "unknown"'
assert "D7b: an absent files_referenced key degrades to unknown" \
    _json_is 't[9312]["verdict"] == "unknown"'
assert "D7: a row with no diff surface warns on stderr" _err_has '\[warn\].*931[12]'
assert "D7: a row with no diff surface emits no re-dispatch request" \
    _no_request_for "$D_REQ" 9311

# --- D8: reachability is checked BEFORE the diff (#5316 §3) ------------------
assert "D8a: an unresolvable main_sha degrades to unknown, never STALE" \
    _json_is 't[9313]["verdict"] == "unknown"'
assert "D8a: an unresolvable main_sha warns on stderr" _err_has '\[warn\].*9313'
assert "D8b: a main_sha that is NOT an ancestor of --main-ref degrades to unknown" \
    _json_is 't[9314]["verdict"] == "unknown"'
assert "D8b: the discarded-duplicate (side-branch) main_sha warns on stderr" \
    _err_has '\[warn\].*9314'
assert "D8: neither reachability failure emits a re-dispatch request" \
    _no_request_for "$D_REQ" 9313

# --- D9: class B spans in-progress ------------------------------------------
assert "D9: an in-progress row with a stale heartbeat and a resolved premise fires" \
    _json_is 't[9315]["class"] == "merge_verify_red" and t[9315]["verdict"] == "STALE"'
assert "D9: the in-progress hit is not counted as live_skipped" \
    _json_is 's["live_skipped"] == 0'

# --- the class-B counter counts exactly the confirmed hits -------------------
assert "D: merge_verify_red counts exactly the four STALE class-B rows" \
    _json_is 's["merge_verify_red"] == 4'

# --- D10: --repo / --main-ref are honoured, and the real repo is never read --
run_sweep --db "$D_DB" --escalations "$D_ESC" --repo "$D_REPO" \
    --main-ref "$D_C1" --format json
assert "D10a: --main-ref is honoured (pinning main-ref at main_sha => UNRESOLVED)" \
    _json_is 't[9301]["verdict"] == "UNRESOLVED" and s["merge_verify_red"] == 0'
_D10_FLAG_OUT="$OUT"

_SWEEP_ENV=(REIFY_GATE_STALENESS_MAIN_REF="$D_C1" REIFY_GATE_STALENESS_REPO="$D_REPO")
run_sweep --db "$D_DB" --escalations "$D_ESC" --format json
_SWEEP_ENV=()
assert "D10b: REIFY_GATE_STALENESS_MAIN_REF/_REPO match the --main-ref/--repo flags" \
    test "$OUT" = "$_D10_FLAG_OUT"

# Pointing --repo at a non-git directory must degrade every class-B row to
# unknown: the fixture SHAs exist ONLY in the fixture repo, so an adjudication
# that ran anywhere else can never resolve them. This pins that the git
# adjudication is scoped to --repo and to nothing else — the suite must never
# reach the real repo.
D_NOGIT="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-nogit-XXXXXX")"
_TMPDIRS+=("$D_NOGIT")
run_sweep --db "$D_DB" --escalations "$D_ESC" --repo "$D_NOGIT" --format json
assert "D10c: --repo is honoured — a non-git --repo degrades class B to unknown" \
    _json_is 't[9301]["verdict"] == "unknown" and s["merge_verify_red"] == 0'

# ──────────────────────────────────────────────────────────────────────────────
# Block E — trigger class C: unmet_dependency
#
# A `blocked` task whose every dependency has since reached a terminal status.
# Reproduces 5372's live shape (blocked, its one dependency 5271 `done`).
#
# E5 IS THE LOAD-BEARING ASSERT OF THIS ENTIRE SUITE. Measured live on
# 2026-07-26, ALL TEN in-progress tasks had every dependency `done` — task
# 5321 itself among them. A "dependency premise resolved => re-dispatch" rule
# that spanned in-progress would therefore have emitted re-dispatch requests
# for ten actively-running agents: the capability would DESTROY work rather
# than recover it. Class C is `blocked`-only for exactly that reason, and E5
# pins it independently of the heartbeat guard — a stale heartbeat plus fully
# satisfied dependencies still must not fire.
#
# The two fail-safe directions are pinned too: an empty dependency set must
# not read as "all satisfied" (E4), and a depends_on id that resolves to no
# row must not read as satisfied (E6) — including the tag-scoped variant,
# where the row exists only under a DIFFERENT tag (E7). `tasks` is
# PRIMARY KEY (tag, id), so an unqualified lookup would silently conflate
# tags the moment a second one exists.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: trigger class C (unmet_dependency) ---"

_mk_tasks_db
E_DB="$DB"
_mk_esc_dir
E_ESC="$ESC_DIR"
_mk_repo
E_REPO="$REPO_DIR"

# Dependency targets, one per terminal / non-terminal status.
_add_task 9490 done      '{}'
_add_task 9491 cancelled '{}'
_add_task 9492 pending   '{}'
_add_task 9493 in-progress '{}' "$(_now_iso -90000)" ""
_add_task 9494 blocked   '{}'
_add_task 9495 deferred  '{}'
# 9496 exists ONLY under a different tag (E7). It is `done` there, so a
# tag-blind lookup would wrongly read task 9410's dependency as satisfied.
_add_task 9496 done      '{}' "" "" other

# E1 — the 5372 shape: one dependency, and it is done.
_add_task 9401 blocked '{}'; _add_dep 9401 9490
# E2 — several dependencies, all terminal, done and cancelled mixed.
_add_task 9402 blocked '{}'; _add_dep 9402 9490; _add_dep 9402 9491
# E3 — one non-terminal dependency is enough to keep the premise unresolved.
_add_task 9403 blocked '{}'; _add_dep 9403 9490; _add_dep 9403 9492
_add_task 9404 blocked '{}'; _add_dep 9404 9493
_add_task 9405 blocked '{}'; _add_dep 9405 9494
_add_task 9406 blocked '{}'; _add_dep 9406 9495
# E4 — zero dependency rows: an empty set is NOT "all satisfied".
_add_task 9407 blocked '{}'
# E5 — the flood guard: in-progress, stale heartbeat, every dependency done.
_add_task 9408 in-progress '{}' "$(_now_iso -90000)" ""; _add_dep 9408 9490
# E6 — a depends_on id with no row in tasks at all.
_add_task 9409 blocked '{}'; _add_dep 9409 9999
# E7 — a depends_on whose row exists only under tag='other'.
_add_task 9410 blocked '{}'; _add_dep 9410 9496
# E8 — matches class A AND class C: A must win, and the row must be counted
# exactly once. Class A's action is `close` (a satisfied deterministic gate is
# cancelled, per #5316 §5); silently downgrading it to class C's `redispatch`
# would re-run a gate task that should simply be closed.
_add_task 9411 blocked "$C_GATE_META"; _add_dep 9411 9490

E_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-ereq-XXXXXX")"
_TMPDIRS+=("$E_REQ")

run_sweep --db "$E_DB" --escalations "$E_ESC" --repo "$E_REPO" \
    --emit-requests "$E_REQ" --format json
assert "E0: the class-C fixture sweep exits 0" _rc_is 0

# --- E1/E2: every dependency terminal => STALE + redispatch ------------------
assert "E1: a blocked task whose only dependency is done is a class-C hit (the 5372 shape)" \
    _json_is 't[9401]["class"] == "unmet_dependency" and t[9401]["verdict"] == "STALE"'
assert "E1: the emitted action is redispatch" _json_is 't[9401]["action"] == "redispatch"'
assert "E1: evidence names the satisfied dependency id and its status" \
    _json_is '"9490=done" in t[9401]["evidence"]'
assert "E2: done and cancelled are both terminal" \
    _json_is 't[9402]["verdict"] == "STALE" and "9491=cancelled" in t[9402]["evidence"]'

# --- E3: any non-terminal dependency keeps the premise unresolved ------------
assert "E3: pending / in-progress / blocked / deferred dependencies all block the hit" \
    _json_is 'all(t[i]["verdict"] == "UNRESOLVED" for i in (9403, 9404, 9405, 9406))'
assert "E3: an UNRESOLVED class-C row is still reported as class C" \
    _json_is 't[9403]["class"] == "unmet_dependency"'

# --- E4: an empty dependency set is not "all satisfied" ----------------------
assert "E4: a blocked task with zero dependency rows is not class C" \
    _json_is 't[9407]["class"] != "unmet_dependency"'

# --- E5: THE FLOOD GUARD ------------------------------------------------------
assert "E5: an in-progress task with a stale heartbeat and all deps done is NOT class C" \
    _json_is 't[9408]["class"] != "unmet_dependency"'
assert "E5: the flood-guard row is not counted in unmet_dependency" \
    _json_is 's["unmet_dependency"] == 2'
assert "E5: the flood-guard row emits no re-dispatch request" _no_request_for "$E_REQ" 9408

# --- E6/E7: an unresolvable dependency is not a satisfied one ----------------
assert "E6: a depends_on with no row in tasks degrades to unknown, never STALE" \
    _json_is 't[9409]["verdict"] == "unknown"'
assert "E6: an unresolvable dependency warns on stderr" _err_has '\[warn\].*9409'
assert "E7: dependency lookup is tag-scoped — a done row under another tag does not satisfy" \
    _json_is 't[9410]["verdict"] == "unknown"'

# --- E8: class precedence — gate_closure > merge_verify_red > unmet_dependency
assert "E8: a row matching both A and C reports gate_closure as its primary class" \
    _json_is 't[9411]["class"] == "gate_closure" and t[9411]["verdict"] == "STALE"'
assert "E8: A's close action is not downgraded to C's redispatch" \
    _json_is 't[9411]["action"] == "close"'
assert "E8: the secondary class is still disclosed in evidence" \
    _json_is '"also:unmet_dependency" in t[9411]["evidence"]'
assert "E8: a multi-class row increments exactly one class counter" \
    _json_is 's["gate_closure"] == 1 and s["unmet_dependency"] == 2'
assert "E8: a multi-class row appears exactly once in the report" \
    _json_is 'len([c for c in d["candidates"] if c["task_id"] == 9411]) == 1'

# ──────────────────────────────────────────────────────────────────────────────
# Block F — the #5316 corruption signatures, wired in as SUPPRESSORS
#
# #5316 (docs/notes/offline-lane-red-corruption-remediation.md) catalogued two
# task-record corruption signatures. This task's brief requires preserving that
# coverage "as one input heuristic feeding the unified sweep — do not regress
# it", so both are lifted from prose into executable checks here:
#
#   Signature 1 — help-text-as-failing-tests: an auto-filed record whose
#     `metadata.failing_tests` holds harness OUTPUT rather than test names
#     (a bare `Usage:` / `Options:` line, or a `verify.sh: ERROR` line).
#   Signature 2 — misattributed provenance: a `metadata.done_provenance.commit`
#     that is NOT an ancestor of --main-ref. #5316 warns explicitly that
#     `git show --stat` alone MIS-CLEARED #5264 here, because the discarded
#     duplicate merge shows a plausible diff and only fails the REACHABILITY
#     test — so F3 pins the check as reachability, never diff inspection.
#
# They are suppressors, not a fourth class: a corrupt record has an
# untrustworthy block premise, so auto-re-dispatching it would act on a false
# premise, and #5316 §4 establishes that remediation there is a human
# git-history adjudication a detector "could flag but not perform". Hence F6:
# a flagged hit becomes CORRUPT-HOLD / human_gate and emits no request. F7
# pins the other half — flags are still computed on NON-stale rows, so 5316's
# audit coverage is not lost for records that are corrupt but not yet
# stranded.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: the #5316 corruption suppressors ---"

_mk_tasks_db
F_DB="$DB"
# No escalation files are ever written into F_ESC: the class-A rows in this
# block must reach STALE so F6's suppression is observable on a real hit.
_mk_esc_dir
F_ESC="$ESC_DIR"

# Fixture history:
#   c0 root
#   F_C1 docs/f-premise.md    <- the class-B recorded main_sha
#   F_SIDE (branch)           <- resolves, but NOT an ancestor of main
#   F_C2 docs/f-resolver.md   <- main tip; resolves the class-B premise
_mk_repo
F_REPO="$REPO_DIR"
F_C1="$(_commit_touching docs/f-premise.md)"
F_SIDE="$(_commit_on_side_branch discarded-dup-f docs/f-discarded.md)"
F_C2="$(_commit_touching docs/f-resolver.md)"
# A 40-hex SHA that resolves in no repo (F5).
F_FAKE_SHA="7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e"

# --- Signature-1 fixtures, in the shapes #5316 recorded on its real victims --
F_SIG1_USAGE='{"failing_tests":["Usage:"]}'
F_SIG1_OPTIONS='{"failing_tests":["Options:"]}'
F_SIG1_VERIFY="{\"failing_tests\":[\"verify.sh: ERROR — unknown argument '--test-threads=1'\"]}"
# #5295's clean comparison sample, plus a path-shaped entry (F2).
F_CLEAN_TESTS='{"failing_tests":["test_cpu_load_governance_deflake.sh","tests/infra/test_warm_lane_audit.sh"]}'

# --- Signature-2 fixtures ----------------------------------------------------
F_PROV_SIDE="{\"done_provenance\":{\"commit\":\"$F_SIDE\"}}"
F_PROV_ANCESTOR="{\"done_provenance\":{\"commit\":\"$F_C1\"}}"
F_PROV_FAKE="{\"done_provenance\":{\"commit\":\"$F_FAKE_SHA\"}}"

# Dependency targets for the class-C rows below.
_add_task 9690 done    '{}'
_add_task 9691 pending '{}'

# F1 — each Signature-1 marker on its own row.
_add_task 9601 blocked "$F_SIG1_USAGE"
_add_task 9602 blocked "$F_SIG1_OPTIONS"
_add_task 9603 blocked "$F_SIG1_VERIFY"
# F2 — real test names / paths are NOT corruption.
_add_task 9604 blocked "$F_CLEAN_TESTS"
# F3/F4/F5 — the three Signature-2 outcomes plus the absent-key control.
_add_task 9605 blocked "$F_PROV_SIDE"
_add_task 9606 blocked "$F_PROV_ANCESTOR"
_add_task 9607 blocked '{}'
_add_task 9608 blocked "$F_PROV_FAKE"

# F6 — one confirmed STALE hit per trigger class, each carrying a corruption
# flag, so suppression is pinned on all three classes rather than just one.
F_GATE_FIELDS='"task_kind":"deterministic","always_escalates":true,"gate_escalated_at":"2026-07-26T08:00:00Z"'
_add_task 9610 blocked "{$F_GATE_FIELDS,\"failing_tests\":[\"Usage:\"]}"
F_PROP_B="$(_d_prop 'Post-merge verification failed: cargo test --workspace returned 101' \
    "$F_C1" '["docs/f-resolver.md"]' 2026-07-24T10:00:00Z)"
_add_task 9611 blocked "{\"dry_run_proposals\":[$F_PROP_B],\"done_provenance\":{\"commit\":\"$F_SIDE\"}}"
_add_task 9612 blocked "$F_SIG1_USAGE"; _add_dep 9612 9690
# The unflagged control: the SAME class-A shape with a clean record must stay
# STALE in the same sweep, so F6 measures suppression and not a blanket
# downgrade of every hit.
_add_task 9613 blocked "{$F_GATE_FIELDS}"
# F5's suppression half: an UNRESOLVABLE provenance SHA is treated
# conservatively as corrupt, so an otherwise-confirmed hit is still held.
_add_task 9616 blocked "{$F_GATE_FIELDS,\"done_provenance\":{\"commit\":\"$F_FAKE_SHA\"}}"

# F7 — corrupt but NOT stale: one dependency still pending, so the row is
# UNRESOLVED. The flag must still be computed and reported.
_add_task 9614 blocked "$F_SIG1_USAGE"; _add_dep 9614 9691
# F8 — both signatures on one row.
_add_task 9615 blocked "{\"failing_tests\":[\"Options:\"],\"done_provenance\":{\"commit\":\"$F_SIDE\"}}"

F_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-freq-XXXXXX")"
_TMPDIRS+=("$F_REQ")

run_sweep --db "$F_DB" --escalations "$F_ESC" --repo "$F_REPO" \
    --emit-requests "$F_REQ" --format json
assert "F0: the corruption-suppressor fixture sweep exits 0" _rc_is 0

# --- F1/F2: Signature 1, help-text-as-failing-tests --------------------------
assert "F1a: a bare 'Usage:' failing_tests entry flags corrupt_autofile" \
    _json_is '"corrupt_autofile" in t[9601]["flags"]'
assert "F1b: a bare 'Options:' failing_tests entry flags corrupt_autofile" \
    _json_is '"corrupt_autofile" in t[9602]["flags"]'
assert "F1c: a 'verify.sh: ERROR' failing_tests entry flags corrupt_autofile" \
    _json_is '"corrupt_autofile" in t[9603]["flags"]'
assert "F2: real test names and tests/infra paths are not corruption" \
    _json_is 't[9604]["flags"] == []'

# --- F3/F4/F5: Signature 2, misattributed provenance -------------------------
# Reachability, NOT diff inspection: the side-branch commit has a perfectly
# plausible diff, which is exactly how #5316's first pass mis-cleared #5264.
assert "F3: a done_provenance.commit reachable only from a side branch flags misattributed_provenance" \
    _json_is '"misattributed_provenance" in t[9605]["flags"]'
assert "F4a: a done_provenance.commit that IS an ancestor of --main-ref is clean" \
    _json_is 't[9606]["flags"] == []'
assert "F4b: an absent done_provenance is clean" \
    _json_is 't[9607]["flags"] == []'
assert "F5: an unresolvable done_provenance.commit flags provenance_unresolvable" \
    _json_is '"provenance_unresolvable" in t[9608]["flags"]'

# --- F6: THE SUPPRESSION COUPLING --------------------------------------------
assert "F6a: a flagged class-A hit is held as CORRUPT-HOLD / human_gate" \
    _json_is 't[9610]["verdict"] == "CORRUPT-HOLD" and t[9610]["action"] == "human_gate"'
assert "F6b: a flagged class-B hit is held as CORRUPT-HOLD / human_gate" \
    _json_is 't[9611]["verdict"] == "CORRUPT-HOLD" and t[9611]["action"] == "human_gate"'
assert "F6c: a flagged class-C hit is held as CORRUPT-HOLD / human_gate" \
    _json_is 't[9612]["verdict"] == "CORRUPT-HOLD" and t[9612]["action"] == "human_gate"'
assert "F5/F6: an unresolvable provenance SHA holds the hit conservatively too" \
    _json_is 't[9616]["verdict"] == "CORRUPT-HOLD"'
assert "F6: a held row still reports the primary class it matched" \
    _json_is 't[9610]["class"] == "gate_closure" and t[9611]["class"] == "merge_verify_red" and t[9612]["class"] == "unmet_dependency"'
assert "F6: held rows are counted in corrupt_hold, never in a class counter" \
    _json_is 's["corrupt_hold"] == 4 and s["merge_verify_red"] == 0 and s["unmet_dependency"] == 0'
assert "F6: an unflagged hit in the same sweep is still STALE" \
    _json_is 't[9613]["verdict"] == "STALE" and s["gate_closure"] == 1'
assert "F6: a held class-A row emits no re-dispatch request" _no_request_for "$F_REQ" 9610
assert "F6: a held class-B row emits no re-dispatch request" _no_request_for "$F_REQ" 9611
assert "F6: a held class-C row emits no re-dispatch request" _no_request_for "$F_REQ" 9612
assert "F5/F6: the conservatively-held row emits no re-dispatch request" _no_request_for "$F_REQ" 9616

# --- F7: audit coverage is not lost on non-stale rows ------------------------
assert "F7: a corrupt but UNRESOLVED row still reports its flag" \
    _json_is 't[9614]["verdict"] == "UNRESOLVED" and "corrupt_autofile" in t[9614]["flags"]'
assert "F7: a flagged non-stale row is not counted in corrupt_hold" \
    _json_is 'len([c for c in d["candidates"] if c["verdict"] == "CORRUPT-HOLD"]) == s["corrupt_hold"]'

# --- F8: multiple flags, stable order ----------------------------------------
assert "F8: two signatures on one row are reported in a deterministic order" \
    _json_is 't[9615]["flags"] == ["corrupt_autofile", "misattributed_provenance"]'
run_sweep --db "$F_DB" --escalations "$F_ESC" --repo "$F_REPO" --format table
assert "F8: the table report renders flags as a comma-separated list" \
    _out_has 'corrupt_autofile,misattributed_provenance'

# --- F9: an unresolvable --main-ref must not manufacture a corruption claim ---
#
# `git merge-base --is-ancestor <sha> <ref>` exits NON-ZERO for two entirely
# different reasons: the commit genuinely is not an ancestor, and the second
# argument does not resolve at all (measured empirically: exit 128, `fatal: Not
# a valid object name`). Passing a raw ref NAME to that probe therefore makes a
# MISSING ancestry oracle indistinguishable from positive evidence of
# corruption — and `misattributed_provenance` is defined, in the docs note's
# suppressor table, as "resolves but is NOT an ancestor". That is a confident
# claim the sweep is not entitled to make when it has no oracle at all.
#
# The consequence is not cosmetic. Invariant L5 suppresses any STALE hit that
# carries a flag, so one spurious flag silently converts a real, actionable hit
# into a human-gate hold and drops its re-dispatch request (F9f/F9g pin both
# halves on the unflagged control). Every other unresolvable-ref path in the
# sweep warns and degrades — the class-B path checks the pre-resolved SHA for
# emptiness FIRST, for this identical premise — so F9e pins the missing warn.
#
# --main-ref is invocation-scoped (the sweep resolves it exactly once per run),
# so F9 needs its own run_sweep. It reuses the Block F fixtures unchanged and
# adds no new builders.
F9_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-f9req-XXXXXX")"
_TMPDIRS+=("$F9_REQ")

run_sweep --db "$F_DB" --escalations "$F_ESC" --repo "$F_REPO" \
    --main-ref no-such-ref --emit-requests "$F9_REQ" --format json
assert "F9a: an unresolvable --main-ref degrades rather than aborting" _rc_is 0

# The core regression: 9606's done_provenance IS reachable from the fixture's
# main (F4a asserts it clean under a resolvable ref), so no ancestry oracle can
# license ANY provenance flag on it.
assert "F9b: a reachable provenance SHA is not flagged when --main-ref does not resolve" \
    _json_is '"misattributed_provenance" not in t[9606]["flags"] and "provenance_unresolvable" not in t[9606]["flags"]'
# The other direction: the fix degrades, it does not merely invert. With no
# ancestry oracle, non-ancestry is not adjudicable for the side-branch row
# either — even though that row IS genuinely misattributed (F3).
assert "F9c: without an ancestry oracle, a genuinely side-branch provenance is not adjudicable either" \
    _json_is '"misattributed_provenance" not in t[9605]["flags"]'
# rev-parse runs against --repo alone, so this check is independent of the ref.
assert "F9d: provenance_unresolvable is a --repo check and is unaffected by --main-ref" \
    _json_is '"provenance_unresolvable" in t[9608]["flags"]'
assert "F9e: the missing ancestry oracle warns, naming the ref and the un-adjudicated check" \
    _err_has '\[warn\].*no-such-ref.*provenance reachability not adjudicable'
# F9f/F9g — the L5 consequence, and the reason this is blocking rather than
# cosmetic: a spurious flag would demote a confirmed hit to a human-gate hold
# and silently drop its actionable output.
assert "F9f: an unflagged class-A hit is still STALE / close and still counted" \
    _json_is 't[9613]["verdict"] == "STALE" and t[9613]["action"] == "close" and t[9613]["class"] == "gate_closure" and s["gate_closure"] == 1'
assert "F9f: ... and is not rewritten to a CORRUPT-HOLD / human_gate row" \
    _json_is 't[9613]["verdict"] != "CORRUPT-HOLD" and t[9613]["action"] != "human_gate"'
assert "F9g: ... so its re-dispatch request is still emitted" \
    test -f "$F9_REQ/redispatch-9613-gate_closure.json"

# ──────────────────────────────────────────────────────────────────────────────
# Block G — --emit-requests and the read-only invariant
#
# The sweep EMITS a re-dispatch request; it never performs the task-state
# write. CLAUDE.md is categorical that all task operations go through the
# fused-memory MCP tools, and a bash script cannot call MCP — writing tasks.db
# directly would bypass the reconciliation that status transitions trigger,
# turning an advisory sweep into an unaudited mutator of the canonical task
# store. This is the house cross-repo seam verbatim: reify ships the
# primitive, dark-factory wires the invocation that performs the
# set_task_status / update_task write.
#
# G3 is the emission-boundary re-pinning of B2 / C2 / E5 / F6: STALE is the
# ONLY verdict that emits, asserted once per non-emitting verdict so a
# regression in any one of them cannot hide behind the others. G8 is the
# read-only proof — a byte-level before/after comparison of the fixture store
# plus a source-level check that no mutating SQL and no non-read-only sqlite
# handle exists in the SUT at all.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: --emit-requests + the read-only proof ---"

# Block-local helpers.
# _request_count_is <dir> <n> — exactly n files in dir (any name).
_request_count_is() {
    local dir="$1" want="$2" got
    got="$(find "$dir" -maxdepth 1 -type f 2>/dev/null | wc -l)"
    [ "$got" = "$want" ] || { echo "expected $want file(s) in $dir, found $got"; return 1; }
}
# _only_request_files <dir> — nothing but redispatch-*.json survives, so a
# leaked mktemp intermediate (which by construction cannot end in `.json`) is
# caught by name rather than only by timing.
_only_request_files() {
    local dir="$1" f base bad=""
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        base="$(basename "$f")"
        case "$base" in
            redispatch-*.json) ;;
            *) bad="${bad:+$bad }$base" ;;
        esac
    done < <(find "$dir" -maxdepth 1 -mindepth 1 2>/dev/null)
    [ -z "$bad" ] || { echo "unexpected non-request entries: $bad"; return 1; }
}
# _request_snapshot <dir> — name+sha256 of every file, sorted; the idempotence
# oracle.
_request_snapshot() {
    local dir="$1" f
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        printf '%s %s\n' "$(basename "$f")" "$(sha256sum <"$f" | awk '{print $1}')"
    done < <(find "$dir" -maxdepth 1 -type f 2>/dev/null | LC_ALL=C sort)
}
# _all_requests_parse <dir> — every emitted file is valid JSON.
_all_requests_parse() {
    local dir="$1" f
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        python3 -c 'import json,sys; json.load(open(sys.argv[1]))' "$f" || {
            echo "unparseable request file: $f"; return 1; }
    done < <(find "$dir" -maxdepth 1 -type f 2>/dev/null)
}
_mk_tasks_db
G_DB="$DB"
_mk_esc_dir
G_ESC="$ESC_DIR"
_mk_repo
G_REPO="$REPO_DIR"
G_C1="$(_commit_touching docs/g-premise.md)"
G_TIP="$(_commit_touching docs/g-resolver.md)"

G_GATE='"task_kind":"deterministic","always_escalates":true,"gate_escalated_at":"2026-07-26T08:00:00Z"'
G_PROP_B="$(_d_prop 'Post-merge verification failed: cargo test --workspace returned 101' \
    "$G_C1" '["docs/g-resolver.md"]' 2026-07-24T10:00:00Z)"

_add_task 9790 done    '{}'
_add_task 9791 pending '{}'

# One confirmed hit per trigger class — the three files G1 expects.
_add_task 9701 blocked "{$G_GATE}"
_add_task 9702 blocked "{\"dry_run_proposals\":[$G_PROP_B]}"
_add_task 9703 blocked '{}'; _add_dep 9703 9790
# One row per NON-emitting verdict. 9704 carries a premise that WOULD fire, so
# the liveness guard is what suppresses it and not an inert fixture.
_add_task 9704 in-progress "{\"dry_run_proposals\":[$G_PROP_B]}" \
    "$(_now_iso -60)" "run-7c8e838c39e7/9704-abc123/pid=3551081"
_add_task 9705 blocked "{$G_GATE}"; _add_esc 9705 1 pending
_add_task 9706 blocked '{}'; _add_dep 9706 9791
_add_task 9707 blocked "{$G_GATE,\"failing_tests\":[\"Usage:\"]}"
# 9708 is a genuine `unknown`: class C MATCHES (it has a dependency row) and
# its oracle then fails, because 9999 resolves to no row under this tag.
_add_task 9708 blocked '{}'; _add_dep 9708 9999
# 9709 is NO-CLASS: no class matched it at all. It is a DISTINCT verdict from
# 9708's — a complete adjudication with a negative result, not a failed one.
_add_task 9709 blocked '{}'

G_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-greq-XXXXXX")"
_TMPDIRS+=("$G_REQ")

# --- G8 read-only proof: snapshot the store BEFORE the emitting run ----------
G_BEFORE="$(_snapshot_readonly "$G_DB" "$G_ESC")"

run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
    --emit-requests "$G_REQ" --format json
assert "G0: the emission fixture sweep exits 0" _rc_is 0

G_AFTER="$(_snapshot_readonly "$G_DB" "$G_ESC")"

# --- G1: one file per confirmed hit, named for the task and its class -------
assert "G1: the class-A hit emits redispatch-9701-gate_closure.json" \
    test -f "$G_REQ/redispatch-9701-gate_closure.json"
assert "G1: the class-B hit emits redispatch-9702-merge_verify_red.json" \
    test -f "$G_REQ/redispatch-9702-merge_verify_red.json"
assert "G1: the class-C hit emits redispatch-9703-unmet_dependency.json" \
    test -f "$G_REQ/redispatch-9703-unmet_dependency.json"
assert "G1: exactly three files — one per hit, and nothing else" \
    _request_count_is "$G_REQ" 3

# --- G2: the consumer contract ----------------------------------------------
assert "G2: the request carries the full consumer field set" \
    _json_check "$G_REQ/redispatch-9701-gate_closure.json" \
    'set(["schema_version","task_id","class","action","verdict","evidence","main_ref_sha","emitted_by"]) <= set(d)'
assert "G2: the class-A request records task_id, class, verdict and the close action" \
    _json_check "$G_REQ/redispatch-9701-gate_closure.json" \
    'd["task_id"] == 9701 and d["class"] == "gate_closure" and d["verdict"] == "STALE" and d["action"] == "close"'
assert "G2: the class-B request's action is reverify" \
    _json_check "$G_REQ/redispatch-9702-merge_verify_red.json" 'd["action"] == "reverify"'
assert "G2: the class-C request's action is redispatch" \
    _json_check "$G_REQ/redispatch-9703-unmet_dependency.json" 'd["action"] == "redispatch"'
assert "G2: evidence is carried through verbatim from the row" \
    _json_check "$G_REQ/redispatch-9703-unmet_dependency.json" '"9790=done" in d["evidence"]'
assert "G2: main_ref_sha is the resolved --main-ref tip, not the recorded premise sha" \
    _json_check "$G_REQ/redispatch-9702-merge_verify_red.json" "d['main_ref_sha'] == '$G_TIP'"
assert "G2: emitted_by names this script" \
    _json_check "$G_REQ/redispatch-9701-gate_closure.json" \
    '"deterministic-gate-closure-staleness-sweep" in d["emitted_by"]'

# --- G3: STALE is the ONLY verdict that emits -------------------------------
# Pinned against the fixture's own verdicts first, so none of the five
# no-request asserts below can pass vacuously against a mis-built row.
assert "G3: the fixture really does produce one row of each non-emitting verdict" \
    _json_is '[t[i]["verdict"] for i in (9704, 9705, 9706, 9707, 9708, 9709)] == ["LIVE", "GATED", "UNRESOLVED", "CORRUPT-HOLD", "unknown", "NO-CLASS"]'
# unknown vs NO-CLASS are counted SEPARATELY. `unknown` exists so that "no
# hits" stays distinguishable from "could not tell"; on the live store the
# great majority of blocked / in-progress rows match no class, so folding them
# into `unknown` would swamp exactly the signal the counter carries. 9708 (a
# matched class with a failed oracle) and 9709 (no class at all) are otherwise
# identical blocked rows, so this pins the split and nothing else.
assert "G3: a class-less row increments no_class and NOT unknown" \
    _json_is 's["unknown"] == 1 and s["no_class"] == 1'
assert "G3: a LIVE row emits no request (re-pins B2 at the emission boundary)" \
    _no_request_for "$G_REQ" 9704
assert "G3: a GATED row emits no request (re-pins C2)" _no_request_for "$G_REQ" 9705
assert "G3: an UNRESOLVED row emits no request" _no_request_for "$G_REQ" 9706
assert "G3: a CORRUPT-HOLD row emits no request (re-pins F6)" _no_request_for "$G_REQ" 9707
assert "G3: an unknown row emits no request" _no_request_for "$G_REQ" 9708
assert "G3: a NO-CLASS row emits no request" _no_request_for "$G_REQ" 9709

# --- G4/G5: idempotence and atomicity ---------------------------------------
G_SNAP1="$(_request_snapshot "$G_REQ")"
assert "G5: every emitted request file parses as JSON" _all_requests_parse "$G_REQ"
assert "G5: no mktemp intermediate survives the first run" _only_request_files "$G_REQ"

run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
    --emit-requests "$G_REQ" --format json
assert "G4: a second sweep still exits 0" _rc_is 0
assert "G4: re-emission leaves the request set byte-identical" \
    test "$G_SNAP1" = "$(_request_snapshot "$G_REQ")"
assert "G4: re-emission does not duplicate files" _request_count_is "$G_REQ" 3
assert "G5: no mktemp intermediate survives the second run either" _only_request_files "$G_REQ"

# --- G6: request emission never gates the sweep ------------------------------
G_PARENT="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-gparent-XXXXXX")"
_TMPDIRS+=("$G_PARENT")
run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
    --emit-requests "$G_PARENT/created-on-demand" --format json
assert "G6: a nonexistent DIR whose parent exists is created" \
    test -d "$G_PARENT/created-on-demand"
assert "G6: and it receives the same three requests" \
    _request_count_is "$G_PARENT/created-on-demand" 3

run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
    --emit-requests "$G_PARENT/no/such/parent" --format json
assert "G6: a DIR whose parent does not exist still exits 0" _rc_is 0
assert "G6: ... warns on stderr" _err_has '\[warn\]'
assert "G6: ... and still prints a complete report on stdout" \
    _json_is 's["gate_closure"] == 1 and s["merge_verify_red"] == 1 and s["unmet_dependency"] == 1'

if [ "$(id -u)" != 0 ]; then
    G_RO="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-gro-XXXXXX")"
    _TMPDIRS+=("$G_RO")
    chmod 500 "$G_RO"
    run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
        --emit-requests "$G_RO" --format json
    assert "G6: a non-writable DIR still exits 0" _rc_is 0
    assert "G6: a non-writable DIR warns on stderr" _err_has '\[warn\]'
    assert "G6: a non-writable DIR still prints a complete report" \
        _json_is 's["gate_closure"] == 1'
    assert "G6: nothing was written into the non-writable DIR" _request_count_is "$G_RO" 0
    chmod 700 "$G_RO"
else
    echo "  SKIP: G6 non-writable-DIR asserts (running as uid 0; mode bits do not apply)"
fi

# --- G7: --emit-requests honours --class ------------------------------------
G_REQ_A="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-greqa-XXXXXX")"
_TMPDIRS+=("$G_REQ_A")
run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
    --class gate_closure --emit-requests "$G_REQ_A" --format json
assert "G7: a --class-restricted sweep emits only that class's request" \
    _request_count_is "$G_REQ_A" 1
assert "G7: and it is the class-A request" \
    test -f "$G_REQ_A/redispatch-9701-gate_closure.json"

# --- G8: the read-only proof, BEHAVIOURAL ONLY -------------------------------
#
# Deliberately not a source grep. Two earlier asserts here tested the SUT's
# source TEXT and were removed for being unsound in both directions:
#   - `grep -qiE '\b(UPDATE|INSERT +INTO|DELETE +FROM)\b' "$SCRIPT"` scans
#     comment PROSE as well as code (the header already says `update_task` /
#     `set_task_status`, and survives only because `_` is a word character), so
#     a future comment reading "update the task record" would fail the suite
#     for a wording edit — a false positive on a docs change.
#   - a grep keyed on ONE spelling of the invocation (`"$_SQLITE_BIN"`) asserts
#     that the matching lines carry -readonly, so it passes VACUOUSLY the moment
#     a regression uses another spelling (unquoted, a renamed variable, a
#     wrapper function) — it can silently pass on exactly the regression it
#     exists to catch.
# The three asserts below are behavioural and cannot be fooled by either.
assert "G8a: the fixture tasks.db and escalation store are byte-identical after an emitting sweep" \
    test "$G_BEFORE" = "$G_AFTER"
assert "G8b: no -wal / -shm sidecar was created alongside the fixture DB" \
    _not test -e "$G_DB-wal"

# G8c — the strongest form: make the store PHYSICALLY read-only (the DB file
# mode 400 and its directory mode 500, so neither the file itself nor a
# -wal/-journal sidecar beside it can be written) and require a COMPLETE report
# out of it anyway. A sweep that opened any handle read-write fails here, where
# a source grep cannot see it; and unlike a grep this cannot be satisfied by
# renaming a variable.
if [ "$(id -u)" != 0 ]; then
    G_RO_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-groreq-XXXXXX")"
    _TMPDIRS+=("$G_RO_REQ")
    G_DB_DIR="$(dirname "$G_DB")"
    chmod 400 "$G_DB"
    chmod 500 "$G_DB_DIR"
    G_RO_BEFORE="$(_snapshot_readonly "$G_DB" "$G_ESC")"
    run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
        --emit-requests "$G_RO_REQ" --format json
    G_RO_AFTER="$(_snapshot_readonly "$G_DB" "$G_ESC")"
    chmod 700 "$G_DB_DIR"
    chmod 600 "$G_DB"
    assert "G8c: a physically read-only store (db 400, dir 500) still exits 0" _rc_is 0
    assert "G8c: ... and still yields the complete three-hit report" \
        _json_is 's["gate_closure"] == 1 and s["merge_verify_red"] == 1 and s["unmet_dependency"] == 1'
    assert "G8c: ... and still emits all three requests" _request_count_is "$G_RO_REQ" 3
    assert "G8c: ... leaving the store byte-identical (no handle was opened read-write)" \
        test "$G_RO_BEFORE" = "$G_RO_AFTER"
else
    echo "  SKIP: G8c physically-read-only asserts (running as uid 0; mode bits do not apply)"
fi

# --- G9: flag<->env parity, and the default writes nothing -------------------
G_REQ_ENV="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-greqenv-XXXXXX")"
_TMPDIRS+=("$G_REQ_ENV")
_SWEEP_ENV=(REIFY_GATE_STALENESS_REQUESTS_DIR="$G_REQ_ENV")
run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" --format json
_SWEEP_ENV=()
assert "G9: REIFY_GATE_STALENESS_REQUESTS_DIR emits the same request set as the flag" \
    test "$G_SNAP1" = "$(_request_snapshot "$G_REQ_ENV")"

G_NONE="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-gnone-XXXXXX")"
_TMPDIRS+=("$G_NONE")
run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" --format json
assert "G9: with neither the flag nor its env knob set, the sweep writes nothing" \
    _request_count_is "$G_NONE" 0
assert "G9: ... and the previously-emitted request set is left untouched" \
    test "$G_SNAP1" = "$(_request_snapshot "$G_REQ")"

# --- G10: the two DB engines are interchangeable, not one-and-a-stub ---------
#
# The python3 task-DB fallback used to be INERT: it backed candidate
# enumeration only, and every downstream reader short-circuited on an empty
# sqlite3 binary. On a host with no sqlite3 the sweep therefore enumerated every
# candidate and then reported all of them as "no trigger class matched", with no
# diagnostic saying classification had been disabled wholesale — a fallback
# advertising a resilience it did not deliver. Both engines now run the
# IDENTICAL enumeration SQL, which is what makes them interchangeable; this
# block is the only proof of that, so it compares the WHOLE observable surface
# (stdout and the emitted request set) rather than a counter.
#
# The engine probe resolves /usr/bin/sqlite3 by ABSOLUTE path, so hiding
# sqlite3 from PATH cannot reach it. REIFY_GATE_STALENESS_SQLITE_BIN is the
# seam; an explicitly empty value forces the python3 engine.
G_REQ_CLI="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-gcli-XXXXXX")"
_TMPDIRS+=("$G_REQ_CLI")
G_REQ_PY="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-gpy-XXXXXX")"
_TMPDIRS+=("$G_REQ_PY")

# The CLI arm has to actually run the CLI. The SUT's probe is `command -v`, which
# proves a binary EXISTS, not that it RUNS — so under verify.sh's OCCT
# LD_LIBRARY_PATH the chosen /usr/bin/sqlite3 aborts on the libsqlite3 version
# mismatch (esc-4581-87) and the SUT quietly falls through to python3. This block
# would then compare python3 against python3: green, and proving nothing, in
# exactly the environment the merge gate runs it in. Clearing LD_LIBRARY_PATH for
# this arm restores a working CLI, and the precondition below fails loudly rather
# than degrading silently if it is ever broken again.
_G_CLI_BIN=""
for _c in /usr/bin/sqlite3 sqlite3; do
    if command -v "$_c" >/dev/null 2>&1; then _G_CLI_BIN="$_c"; break; fi
done
assert "G10: the CLI arm's sqlite3 actually runs (else this block is vacuous)" \
    env LD_LIBRARY_PATH= "$_G_CLI_BIN" "$G_DB" "SELECT 1;"

_SWEEP_ENV=(LD_LIBRARY_PATH= REIFY_GATE_STALENESS_SQLITE_BIN="$_G_CLI_BIN")
run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
    --emit-requests "$G_REQ_CLI" --format json
_SWEEP_ENV=()
G_OUT_CLI="$OUT"

_SWEEP_ENV=(REIFY_GATE_STALENESS_SQLITE_BIN=)
run_sweep --db "$G_DB" --escalations "$G_ESC" --repo "$G_REPO" \
    --emit-requests "$G_REQ_PY" --format json
_SWEEP_ENV=()
assert "G10: the python3 engine alone still exits 0" _rc_is 0
assert "G10: ... and produces byte-identical stdout to the sqlite3 CLI" \
    test "$OUT" = "$G_OUT_CLI"
assert "G10: ... and an identical emitted request set" \
    test "$(_request_snapshot "$G_REQ_CLI")" = "$(_request_snapshot "$G_REQ_PY")"
# The regression this exists to catch head-on: with no sqlite3, every class
# predicate used to fail closed and the whole report collapsed to no-class rows.
assert "G10: ... with all three classes still adjudicated, not collapsed to no_class" \
    _json_is 's["gate_closure"] == 1 and s["merge_verify_red"] == 1 and s["unmet_dependency"] == 1'
assert "G10: ... and the corruption suppressor still firing on the python3 engine" \
    _json_is 's["corrupt_hold"] == 1 and t[9707]["flags"] == ["corrupt_autofile"]'

# ──────────────────────────────────────────────────────────────────────────────
# Block R — request retraction: the directory is a SNAPSHOT, not a log
#
# Emission alone only ADDS. Two consequences a consumer polling the directory
# cannot untangle:
#   (a) once a hit is remediated the task drops out of the candidate set, but
#       its request file survives forever — so a consumer following the
#       documented "diff the directory" contract keeps seeing an actionable
#       request for an already-closed task;
#   (b) if a row's PRIMARY class changes between runs (its gating escalation
#       reappears, so gate_closure stops winning and unmet_dependency takes
#       over) the directory ends up holding redispatch-<id>-gate_closure.json
#       (action=close) AND redispatch-<id>-unmet_dependency.json
#       (action=redispatch) at the same time — two contradictory instructions
#       with no ordering hint, since the bodies deliberately carry NO
#       wall-clock field (G4's idempotence property).
#
# Each run therefore retracts what is no longer a hit before it emits. This is
# the one block whose fixture DB is MUTATED between runs — that is the point:
# every other block asserts one sweep over a frozen store, and neither defect
# above is observable in a single run.
#
# The three scoping rules matter as much as the retraction: R4 pins that a
# --class-restricted run cannot delete another class's requests, R5 that a
# DEGRADED READ retracts nothing (an unreadable DB reports zero candidates
# too, and wiping on that would be the sweep destroying its own output on a
# transient fault), and R7 that a consumer's own files are never touched.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block R: request retraction ---"

_mk_tasks_db
R_DB="$DB"
_mk_esc_dir
R_ESC="$ESC_DIR"
_mk_repo
R_REPO="$REPO_DIR"
R_C1="$(_commit_touching docs/r-premise.md)"
R_TIP="$(_commit_touching docs/r-resolver.md)"

R_GATE='"task_kind":"deterministic","always_escalates":true,"gate_escalated_at":"2026-07-26T08:00:00Z"'
R_PROP_B="$(_d_prop 'Post-merge verification failed: cargo test --workspace returned 101' \
    "$R_C1" '["docs/r-resolver.md"]' 2026-07-24T10:00:00Z)"

_add_task 9990 done '{}'
# One confirmed hit per class.
_add_task 9901 blocked "{$R_GATE}"
_add_task 9902 blocked "{\"dry_run_proposals\":[$R_PROP_B]}"
_add_task 9903 blocked '{}'; _add_dep 9903 9990

R_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-rreq-XXXXXX")"
_TMPDIRS+=("$R_REQ")

run_sweep --db "$R_DB" --escalations "$R_ESC" --repo "$R_REPO" \
    --emit-requests "$R_REQ" --format json
assert "R0: the retraction fixture sweep exits 0" _rc_is 0
assert "R0: one request per class is emitted" _request_count_is "$R_REQ" 3
R_SNAP_B="$(sha256sum <"$R_REQ/redispatch-9902-merge_verify_red.json" | awk '{print $1}')"

# --- R1: a remediated hit's request is retracted -----------------------------
# 9901 is closed out of band (exactly what a consumer acting on the request
# does), so it is no longer enumerated at all.
_sq "$R_DB" "UPDATE tasks SET status='done' WHERE tag='master' AND id=9901;"
run_sweep --db "$R_DB" --escalations "$R_ESC" --repo "$R_REPO" \
    --emit-requests "$R_REQ" --format json
assert "R1: the second sweep still exits 0" _rc_is 0
assert "R1: the remediated task's request is retracted" _no_request_for "$R_REQ" 9901
assert "R1: the retraction is announced on stderr" _err_has 'Retracted superseded request.*9901'
assert "R1: the still-live hits are untouched" \
    test -f "$R_REQ/redispatch-9902-merge_verify_red.json" -a -f "$R_REQ/redispatch-9903-unmet_dependency.json"
assert "R1: a surviving request is still byte-identical (retraction is not rewrite)" \
    test "$R_SNAP_B" = "$(sha256sum <"$R_REQ/redispatch-9902-merge_verify_red.json" | awk '{print $1}')"
assert "R1: exactly the two surviving requests remain" _request_count_is "$R_REQ" 2

# --- R2/R3: a class change never leaves two contradictory instructions -------
# 9903 keeps its satisfied dependency but gains the class-A gate shape, so
# gate_closure now wins on precedence and its class-C request is superseded.
_sq "$R_DB" "UPDATE tasks SET metadata='{$R_GATE}' WHERE tag='master' AND id=9903;"
run_sweep --db "$R_DB" --escalations "$R_ESC" --repo "$R_REPO" \
    --emit-requests "$R_REQ" --format json
assert "R2: the reclassified task's OLD class request is retracted" \
    _no_request_for "$R_REQ" 9903-unmet_dependency
assert "R2: ... and its new class request is emitted" \
    test -f "$R_REQ/redispatch-9903-gate_closure.json"
assert "R3: the task maps to EXACTLY ONE request — no contradictory pair" \
    test "$(find "$R_REQ" -maxdepth 1 -name 'redispatch-9903-*.json' | wc -l)" = "1"
assert "R3: and that one carries the new class's action" \
    _json_check "$R_REQ/redispatch-9903-gate_closure.json" 'd["action"] == "close"'

# --- R4: a --class-restricted run retracts only its own class ----------------
# 9902 stops being a hit, but a gate_closure-only sweep did not adjudicate
# class B at all, so it has no standing to retract that request.
_sq "$R_DB" "UPDATE tasks SET status='done' WHERE tag='master' AND id=9902;"
run_sweep --db "$R_DB" --escalations "$R_ESC" --repo "$R_REPO" \
    --class gate_closure --emit-requests "$R_REQ" --format json
assert "R4: a --class-restricted sweep exits 0" _rc_is 0
assert "R4: it does NOT retract a request of a class it never adjudicated" \
    test -f "$R_REQ/redispatch-9902-merge_verify_red.json"
assert "R4: and it keeps its own class's live request" \
    test -f "$R_REQ/redispatch-9903-gate_closure.json"

# --- R5: a degraded DB read retracts nothing ---------------------------------
run_sweep --db "$R_REPO/definitely-not-here.db" --escalations "$R_ESC" --repo "$R_REPO" \
    --emit-requests "$R_REQ" --format json
assert "R5: an unreadable --db still exits 0" _rc_is 0
assert "R5: an unreadable --db retracts NOTHING (zero candidates is not evidence)" \
    _request_count_is "$R_REQ" 2
assert "R5: ... and says so on stderr" _err_has '\[warn\].*no superseded request was retracted'

# --- R6: a full sweep then does retract the now-stale class-B request --------
run_sweep --db "$R_DB" --escalations "$R_ESC" --repo "$R_REPO" \
    --emit-requests "$R_REQ" --format json
assert "R6: a full sweep retracts the class-B request R4 was not entitled to" \
    _no_request_for "$R_REQ" 9902
assert "R6: only the one live hit remains" _request_count_is "$R_REQ" 1

# --- R7: a consumer's own files in the directory are never touched -----------
# Retraction is scoped to redispatch-<digits>-<known class>.json, so neither a
# consumer's bookkeeping nor a foreign redispatch-shaped name is removed.
: > "$R_REQ/consumer-bookkeeping.txt"
: > "$R_REQ/redispatch-notanid-gate_closure.json"
: > "$R_REQ/redispatch-9999-some_other_class.json"
run_sweep --db "$R_DB" --escalations "$R_ESC" --repo "$R_REPO" \
    --emit-requests "$R_REQ" --format json
assert "R7: a consumer's own non-request file survives" \
    test -f "$R_REQ/consumer-bookkeeping.txt"
assert "R7: a redispatch-shaped file with a non-numeric id survives" \
    test -f "$R_REQ/redispatch-notanid-gate_closure.json"
assert "R7: a redispatch-shaped file naming an unknown class survives" \
    test -f "$R_REQ/redispatch-9999-some_other_class.json"
assert "R7: and the live hit is still emitted alongside them" \
    test -f "$R_REQ/redispatch-9903-gate_closure.json"

# ──────────────────────────────────────────────────────────────────────────────
# Block T — tag scoping
#
# `tasks` and `dependencies` are both PRIMARY KEY (tag, ...), so a task id is
# unique only WITHIN a tag. Every query the sweep issues must therefore carry
# `tag = <the swept tag>`; a regression that dropped that predicate from any one
# of them would ship silently, because the live store has exactly one tag today
# and every query returns the same answer with or without it.
#
# The failure it would cause is not cosmetic. With a second tag present, a
# tag-blind ENUMERATION returns the same id twice; a tag-blind metadata read
# (`... WHERE id=<id> LIMIT 1`) answers for whichever tag the (tag, id) index
# reaches first; and the sweep would then emit `redispatch-<id>-<class>.json`
# adjudicated from one tag's row while naming a task in another — telling the
# consumer to CANCEL the wrong task.
#
# ROW-ORDER ROBUSTNESS: a scalar read of the shape `... WHERE id=<id> LIMIT 1`
# cannot use the (tag, id) primary-key index (id is not its leading column), so
# a tag-blind one degrades to a table scan and answers with whichever row has
# the lower ROWID — i.e. whichever tag was INSERTED FIRST. Verified by
# mutation: with the swept tag's row inserted first, dropping `tag='$TAG'` from
# the metadata and flags queries changed no output at all. So the decoy is
# planted in BOTH orders — 9801's decoy row is inserted before the swept row,
# 9805's after — and both are asserted clean. Whichever way a tag-blind scan
# resolves, one of the two is adjudicated from the wrong tag and fails.
#
# Each decoy row is shaped to trip every tag-scoped query at once: it carries a
# satisfied dependency (the class-C LEFT JOIN), a Signature-1 corruption marker
# (the flags query), and no `task_kind` at all (the class-A metadata reads).
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block T: tag scoping ---"

_mk_tasks_db
T_DB="$DB"
_mk_esc_dir
T_ESC="$ESC_DIR"
_mk_repo
T_REPO="$REPO_DIR"

T_GATE='"task_kind":"deterministic","always_escalates":true,"gate_escalated_at":"2026-07-26T08:00:00Z"'
# The decoy shape: NOT class A (no task_kind), corrupt per Signature 1, and
# carrying a dependency — divergent from the swept tag's row in every dimension
# the sweep reads.
T_DECOY='{"failing_tests":["Usage:"]}'

_add_task 9890 done '{}' "" "" alt

# 9801 — DECOY FIRST, then the swept row.
_add_task 9801 blocked "$T_DECOY" "" "" alt
_add_dep  9801 9890 alt
_add_task 9801 blocked "{$T_GATE}"
# 9805 — SWEPT ROW FIRST, then the decoy.
_add_task 9805 blocked "{$T_GATE}"
_add_task 9805 blocked "$T_DECOY" "" "" alt
_add_dep  9805 9890 alt
# 9803 — present only under master. The control that keeps T7/T9 from passing
# merely because the report came back empty.
_add_task 9803 blocked "{$T_GATE}"
# 9802 — exists ONLY under alt, and is a class-A hit THERE. It must never
# appear in a master sweep, and must appear in an alt one.
_add_task 9802 blocked "{$T_GATE}" "" "" alt

T_REQ="$(mktemp -d "${TMPDIR:-/tmp}/gate-staleness-treq-XXXXXX")"
_TMPDIRS+=("$T_REQ")

run_sweep --db "$T_DB" --escalations "$T_ESC" --repo "$T_REPO" \
    --tag master --emit-requests "$T_REQ" --format json
assert "T0: the two-tag fixture sweep exits 0" _rc_is 0

# --- T1/T2: candidate ENUMERATION is tag-scoped ------------------------------
assert "T1: a task present under two tags is enumerated exactly once, in both row orders" \
    _json_is 'len([c for c in d["candidates"] if c["task_id"] in (9801, 9805)]) == 2'
assert "T2: a task that exists only under another tag is never enumerated" \
    _json_is '9802 not in t'
assert "T2: the swept tag's own rows are all enumerated" \
    _json_is 'sorted(t) == [9801, 9803, 9805]'

# --- T3: the metadata reads are tag-scoped -----------------------------------
# The decoy carries no task_kind, so a tag-blind read drops the row out of
# class A entirely. Asserted for BOTH row orders (see the header note).
assert "T3: both dual-tag rows are adjudicated from the MASTER metadata (class A, STALE, close)" \
    _json_is 'all(t[i]["class"] == "gate_closure" and t[i]["verdict"] == "STALE" and t[i]["action"] == "close" for i in (9801, 9805))'

# --- T4: the dependency LEFT JOIN is tag-scoped (re-pins E7 from the other side)
# E7 pins that a dependency TARGET under another tag does not satisfy. This
# pins the complementary direction: another tag's dependency ROWS are not read
# for this task at all.
assert "T4: the other tag's dependency rows are not attributed to these tasks" \
    _json_is 'all("also:unmet_dependency" not in t[i]["evidence"] for i in (9801, 9805))'

# --- T5: the Signature-1 flags query is tag-scoped ---------------------------
assert "T5: the other tag's corruption marker does not flag these tasks" \
    _json_is 'all(t[i]["flags"] == [] for i in (9801, 9805))'
assert "T5: ... so neither hit is spuriously demoted to CORRUPT-HOLD" \
    _json_is 's["gate_closure"] == 3 and s["corrupt_hold"] == 0'

# --- T6: emission follows the swept tag --------------------------------------
assert "T6: the emitted requests are the master rows' close requests" \
    test -f "$T_REQ/redispatch-9801-gate_closure.json"
assert "T6: exactly the three master hits emit, and nothing from the other tag" \
    _request_count_is "$T_REQ" 3
assert "T6: no request is emitted for the other tag's task" _no_request_for "$T_REQ" 9802

# --- T7: --tag really does select the namespace ------------------------------
run_sweep --db "$T_DB" --escalations "$T_ESC" --repo "$T_REPO" --tag alt --format json
assert "T7: --tag alt sweeps the other namespace (its rows appear, master's do not)" \
    _json_is '9802 in t and 9803 not in t'
assert "T7: the dual-tag ids now adjudicate from the ALT rows — class C, flagged, held" \
    _json_is 'all(t[i]["class"] == "unmet_dependency" and t[i]["verdict"] == "CORRUPT-HOLD" for i in (9801, 9805))'
_T7_ALT_OUT="$OUT"

# --- T8/T9: flag <-> env parity for the tag knob (mirrors A8a/A8b) -----------
_SWEEP_ENV=(REIFY_LANE_TASK_TAG=alt)
run_sweep --db "$T_DB" --escalations "$T_ESC" --repo "$T_REPO" --format json
_SWEEP_ENV=()
assert "T8: REIFY_LANE_TASK_TAG produces byte-identical stdout to --tag" \
    test "$OUT" = "$_T7_ALT_OUT"

_SWEEP_ENV=(REIFY_LANE_TASK_TAG=alt)
run_sweep --db "$T_DB" --escalations "$T_ESC" --repo "$T_REPO" --tag master --format json
_SWEEP_ENV=()
assert "T9: an explicit --tag overrides REIFY_LANE_TASK_TAG" \
    _json_is 'sorted(t) == [9801, 9803, 9805] and t[9801]["class"] == "gate_closure"'

test_summary
