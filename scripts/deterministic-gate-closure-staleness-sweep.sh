#!/usr/bin/env bash
# scripts/deterministic-gate-closure-staleness-sweep.sh — Standalone,
# timer-friendly staleness sweep over stranded `blocked` / `in-progress`
# Taskmaster rows. Read-only advisory observability: it NEVER mutates task
# state (no write to tasks.db, no write to the escalation store) and never
# gates dispatch/reclaim/merge. Its only side effect on any invocation is the
# request files it drops under --emit-requests.
#
# Task #5321; generalizes the corruption-remediation runbook shipped by #5316
# (docs/notes/offline-lane-red-corruption-remediation.md, which is a docs
# runbook with no script and no timer — this is the family's first executable
# artifact). Operational digest:
# docs/notes/deterministic-gate-closure-staleness-sweep.md
#
# A task is STRANDED when the premise that blocked it has since resolved but
# nothing re-dispatched or closed it. Three trigger classes are swept in one
# pass:
#
#   gate_closure       — a `blocked` deterministic always-escalates gate task
#                        whose gating escalation was resolved/dismissed
#                        elsewhere (no live `esc-<id>-*.json` with
#                        status=pending remains) ⇒ action=close.
#   merge_verify_red   — a post-merge-verification-failed block whose recorded
#                        main_sha is an ancestor of --main-ref, where main has
#                        advanced and ≥1 recorded files_referenced path was
#                        touched in main_sha..main-ref ⇒ action=reverify.
#   unmet_dependency   — a `blocked` task with ≥1 dependency row where every
#                        depends_on has reached a terminal status
#                        (done/cancelled) ⇒ action=redispatch.
#
# Usage:
#   scripts/deterministic-gate-closure-staleness-sweep.sh \
#       [--db PATH] [--tag TAG] [--escalations DIR] [--repo DIR] \
#       [--main-ref REF] [--format table|json] \
#       [--class all|gate_closure|merge_verify_red|unmet_dependency] \
#       [--stale-heartbeat-min N] [--emit-requests DIR] [-h|--help]
#
# For each candidate row, emits: task_id · status · class · verdict · action ·
# evidence · flags. Verdict vocabulary:
#   STALE        — premise resolved; the row is a confirmed hit (the ONLY
#                  verdict that emits a re-dispatch request).
#   UNRESOLVED   — the class matched but its premise has NOT resolved yet.
#   GATED        — class A only: a live pending escalation still gates it.
#   LIVE         — the liveness guard fired (fresh heartbeat); skipped
#                  entirely, no class predicate ran.
#   CORRUPT-HOLD — an otherwise-confirmed hit carrying a #5316 corruption
#                  flag; suppressed for a human gate (action=human_gate).
#   NO-CLASS     — no trigger class matched the row at all. This is a
#                  COMPLETE adjudication with a negative result, not a failure,
#                  and it is deliberately NOT `unknown`: on the live store most
#                  blocked / in-progress rows match no class, so folding them
#                  into `unknown` would swamp the one signal that counter
#                  exists to carry.
#   unknown      — a class matched but its ORACLE could not be read (a missing
#                  escalations dir, an unresolvable SHA, a dependency id that
#                  resolves to no row); never upgraded to STALE.
# A trailing SWEEP: line (table) / summary object (json) carries the counters
# candidates, gate_closure, merge_verify_red, unmet_dependency, corrupt_hold,
# live_skipped, no_class, unknown.
#
# Options (env defaults shown):
#   --db PATH             Taskmaster SQLite store (env: REIFY_LANE_TASK_DB;
#                         default: /home/leo/src/reify/.taskmaster/tasks/tasks.db).
#                         Reused verbatim from scripts/lane-task-status.sh's
#                         contract — one task-DB plumbing contract in the repo.
#                         Opened strictly read-only (-readonly / mode=ro). A
#                         missing / 0-byte / unreadable DB degrades to a
#                         zero-candidate report, never an abort.
#   --tag TAG             Taskmaster tag namespace (env: REIFY_LANE_TASK_TAG;
#                         default: master). Task ids are unique only within a
#                         tag (PRIMARY KEY (tag, id)), so every query is
#                         tag-scoped.
#   --escalations DIR     Live escalation store holding esc-<task>-<n>.json
#                         (env: REIFY_GATE_STALENESS_ESCALATIONS_DIR; default:
#                         <repo>/data/escalations). Only the live directory is
#                         read — archive*/ subdirectories are deliberately NOT
#                         searched, because an archived escalation is by
#                         construction no longer gating.
#   --repo DIR            Git repo used for ancestor / touched-path
#                         adjudication (env: REIFY_GATE_STALENESS_REPO;
#                         default: the repo containing this script).
#   --main-ref REF        Git ref for "main" (env:
#                         REIFY_GATE_STALENESS_MAIN_REF; default: main).
#   --format table|json   Output format (default: table).
#   --class C             Restrict the sweep to one trigger class: all,
#                         gate_closure, merge_verify_red, unmet_dependency
#                         (default: all).
#   --stale-heartbeat-min N
#                         Minutes; a candidate whose heartbeat_at is newer than
#                         N minutes is LIVE and is skipped by the liveness
#                         guard (env: REIFY_GATE_STALENESS_HEARTBEAT_MIN;
#                         default: 30).
#   --emit-requests DIR   Drop one atomic, idempotent
#                         redispatch-<task_id>-<class>.json per confirmed STALE
#                         hit into DIR (env:
#                         REIFY_GATE_STALENESS_REQUESTS_DIR; default: unset —
#                         no files are written at all).
#   -h, --help            Print this message and exit.
#
# Exit codes:
#   0  — Always, on every valid invocation (advisory/observability — this
#        script must never gate anything). An unreadable DB, a missing
#        escalations dir, an unresolvable SHA, or an unwritable
#        --emit-requests dir all degrade gracefully rather than aborting.
#   2  — Usage error: unknown flag, missing flag value, invalid --format /
#        --class / --stale-heartbeat-min.
#
# Invariants:
#   L1 — the liveness guard is the FIRST predicate applied to every candidate
#        and it short-circuits: a row with a fresh heartbeat is never a hit,
#        no class predicate evaluates for it, and it never yields a
#        re-dispatch request. A NULL/empty heartbeat is NOT live (`blocked`
#        rows legitimately carry none). An UNPARSEABLE heartbeat degrades to
#        LIVE, never to eligible — the fail-safe direction for a re-dispatch
#        decision is always "do nothing".
#   L2 — an unreadable escalation oracle (missing dir, UNREADABLE/unsearchable
#        dir, unparseable file, unrecognized status)
#        degrades that row to `unknown`, never to `STALE`. A failed oracle
#        lookup must never manufacture an actionable verdict — the same
#        posture as warm-lane-audit.sh invariant A3. `unknown` means EXACTLY
#        that — a matched class whose oracle failed. A row that simply matches
#        no class is NO-CLASS, a complete adjudication with a negative result;
#        conflating the two would bury a handful of genuine oracle failures
#        under the majority of the store, which matches no class at all.
#   L3 — every candidate contributes to EXACTLY ONE class counter and appears
#        exactly once in the report. Classes are tried in the fixed precedence
#        order gate_closure > merge_verify_red > unmet_dependency; the first
#        match becomes the row's primary class and is the only one
#        adjudicated, and any further match is disclosed in `evidence` as
#        `also:<class>` without incrementing a counter. So `--class all`
#        never double-counts a multi-class row.
#   L4 — classes A (gate_closure) and C (unmet_dependency) are `blocked`-ONLY.
#        Only class B (merge_verify_red) spans `in-progress`. Measured on
#        2026-07-26, all ten live `in-progress` tasks had every dependency
#        `done` (task 5321 itself among them), so a class C that spanned
#        `in-progress` would re-dispatch actively-running agents — it would
#        destroy work rather than recover it.
#   L5 — a #5316 corruption flag SUPPRESSES auto-re-dispatch. A flagged row
#        whose verdict would otherwise be STALE is reported as CORRUPT-HOLD
#        with action=human_gate, is counted in `corrupt_hold` INSTEAD OF its
#        class counter, and never emits a re-dispatch request. Flags are
#        still computed and reported on non-stale rows, so #5316's audit
#        coverage is not lost for records that are corrupt but not yet
#        stranded. A corrupt record has an untrustworthy block premise, so
#        re-dispatching on it would act on a false premise, and #5316 §4
#        establishes that remediation there is a human git-history
#        adjudication a detector can flag but not perform.
#   L6 — READ-ONLY on all task state. Every sqlite handle is opened
#        -readonly / mode=ro, the escalation store is only ever read, and the
#        ONLY side effect of any invocation is the request files written
#        under --emit-requests. The sweep never writes tasks.db: CLAUDE.md
#        requires all task operations to go through the fused-memory MCP
#        tools, and writing the store directly would bypass the
#        reconciliation that status transitions trigger — turning an
#        advisory sweep into an unaudited mutator of the canonical task
#        store. So this script EMITS a request and the dark-factory consumer
#        performs the set_task_status / update_task write. That is the house
#        cross-repo seam verbatim: reify ships the primitive, dark-factory
#        wires the invocation.
#
# --emit-requests consumer contract:
#   One file per confirmed hit, named redispatch-<task_id>-<class>.json,
#   holding schema_version, task_id, class, verdict, action, evidence,
#   main_ref_sha and emitted_by. Written via a mktemp intermediate in the
#   SAME directory followed by mv, so a consumer polling the directory never
#   observes a partial file. The body carries NO wall-clock field, so
#   re-emission is byte-idempotent and a consumer can diff rather than
#   re-process; read the file mtime if recency is needed. Emission never
#   gates the sweep — an uncreatable or unwritable directory warns, and the
#   report on stdout is still complete and the exit code still 0.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── log helpers (all write to stderr) ─────────────────────────────────────────
# The machine report is the ONLY thing on stdout; every diagnostic goes to
# stderr (the stdout-contract convention shared with warm-lane-audit.sh).
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── usage ─────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") [--db PATH] [--tag TAG] [--escalations DIR] [--repo DIR]
                       [--main-ref REF] [--format table|json] [--class C]
                       [--stale-heartbeat-min N] [--emit-requests DIR]

  Recurring staleness sweep over stranded blocked / in-progress Taskmaster
  rows. Read-only advisory observability: never mutates task state, never
  gates anything. Emits re-dispatch REQUESTS; the consumer performs the write.

  Options:
    --db PATH             Taskmaster SQLite store (default: \$REIFY_LANE_TASK_DB).
    --tag TAG             Taskmaster tag namespace (default: \$REIFY_LANE_TASK_TAG
                          or master).
    --escalations DIR     Live escalation store (default:
                          \$REIFY_GATE_STALENESS_ESCALATIONS_DIR or
                          <repo>/data/escalations).
    --repo DIR            Git repo for ancestor/touched-path adjudication
                          (default: \$REIFY_GATE_STALENESS_REPO or this repo).
    --main-ref REF        Git ref for "main" (default:
                          \$REIFY_GATE_STALENESS_MAIN_REF or main).
    --format table|json   Output format (default: table).
    --class C             all | gate_closure | merge_verify_red |
                          unmet_dependency (default: all).
    --stale-heartbeat-min N
                          A heartbeat newer than N minutes marks the row LIVE
                          and skips it (default:
                          \$REIFY_GATE_STALENESS_HEARTBEAT_MIN or 30).
    --emit-requests DIR   Drop one redispatch-<task_id>-<class>.json per
                          confirmed STALE hit (default:
                          \$REIFY_GATE_STALENESS_REQUESTS_DIR; unset = none).
    -h, --help            Print this message and exit.

  Exit codes:
    0  — Always, on every valid invocation (advisory-only; never gates).
    2  — Usage error.
EOF
}

# ── defaults ──────────────────────────────────────────────────────────────────
# REIFY_LANE_TASK_DB / REIFY_LANE_TASK_TAG are reused VERBATIM from
# scripts/lane-task-status.sh's contract — one task-DB plumbing contract in
# the repo, not a parallel one.
DB="${REIFY_LANE_TASK_DB:-/home/leo/src/reify/.taskmaster/tasks/tasks.db}"
TAG="${REIFY_LANE_TASK_TAG:-master}"
ESCALATIONS="${REIFY_GATE_STALENESS_ESCALATIONS_DIR:-}"
REPO="${REIFY_GATE_STALENESS_REPO:-}"
MAIN_REF="${REIFY_GATE_STALENESS_MAIN_REF:-main}"
FORMAT="table"
CLASS="all"
STALE_HEARTBEAT_MIN="${REIFY_GATE_STALENESS_HEARTBEAT_MIN:-30}"
REQUESTS_DIR="${REIFY_GATE_STALENESS_REQUESTS_DIR:-}"

# ── arg parsing ───────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --db)
            [ $# -ge 2 ] || { err "--db requires a value"; exit 2; }
            DB="$2"; shift 2 ;;
        --tag)
            [ $# -ge 2 ] || { err "--tag requires a value"; exit 2; }
            TAG="$2"; shift 2 ;;
        --escalations)
            [ $# -ge 2 ] || { err "--escalations requires a value"; exit 2; }
            ESCALATIONS="$2"; shift 2 ;;
        --repo)
            [ $# -ge 2 ] || { err "--repo requires a value"; exit 2; }
            REPO="$2"; shift 2 ;;
        --main-ref)
            [ $# -ge 2 ] || { err "--main-ref requires a value"; exit 2; }
            MAIN_REF="$2"; shift 2 ;;
        --format)
            [ $# -ge 2 ] || { err "--format requires a value"; exit 2; }
            FORMAT="$2"; shift 2 ;;
        --class)
            [ $# -ge 2 ] || { err "--class requires a value"; exit 2; }
            CLASS="$2"; shift 2 ;;
        --stale-heartbeat-min)
            [ $# -ge 2 ] || { err "--stale-heartbeat-min requires a value"; exit 2; }
            STALE_HEARTBEAT_MIN="$2"; shift 2 ;;
        --emit-requests)
            [ $# -ge 2 ] || { err "--emit-requests requires a value"; exit 2; }
            REQUESTS_DIR="$2"; shift 2 ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            err "Unexpected argument: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── validate ──────────────────────────────────────────────────────────────────
case "$FORMAT" in
    table|json) : ;;
    *)
        err "Unknown --format: '$FORMAT' (expected table or json)."
        err "Run '$(basename "$0") --help' for usage."
        exit 2 ;;
esac

case "$CLASS" in
    all|gate_closure|merge_verify_red|unmet_dependency) : ;;
    *)
        err "Unknown --class: '$CLASS' (expected all, gate_closure, merge_verify_red or unmet_dependency)."
        err "Run '$(basename "$0") --help' for usage."
        exit 2 ;;
esac

if ! printf '%s\n' "$STALE_HEARTBEAT_MIN" | grep -qE '^[0-9]+$'; then
    err "--stale-heartbeat-min (or REIFY_GATE_STALENESS_HEARTBEAT_MIN) is not a valid non-negative integer: '$STALE_HEARTBEAT_MIN'."
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi

# ── resolve repo-relative defaults (after parsing, so --repo is honoured) ─────
if [ -z "$REPO" ]; then REPO="$(cd "$SCRIPT_DIR/.." && pwd)"; fi
if [ -z "$ESCALATIONS" ]; then ESCALATIONS="$REPO/data/escalations"; fi

info "gate-closure-staleness-sweep: db=$DB tag=$TAG class=$CLASS format=$FORMAT stale_heartbeat_min=$STALE_HEARTBEAT_MIN main_ref=$MAIN_REF"

# ── summary counters ──────────────────────────────────────────────────────────
N_CANDIDATES=0
N_GATE_CLOSURE=0
N_MERGE_VERIFY_RED=0
N_UNMET_DEPENDENCY=0
N_CORRUPT_HOLD=0
N_LIVE_SKIPPED=0
N_NO_CLASS=0
N_UNKNOWN=0

# ── field separator ───────────────────────────────────────────────────────────
# ASCII US (0x1f), deliberately NOT a tab. `read` treats runs of IFS
# *whitespace* — space, tab, newline — as ONE delimiter and strips leading and
# trailing ones, so with IFS=$'\t' an EMPTY field silently collapses and every
# field after it shifts left. That is not hypothetical: a proposal with no
# block_class shifted its head_sha into main_sha, and a candidate row with a
# NULL heartbeat_at but a populated claimant_run_id would have had its
# claimant read as its heartbeat. US is not IFS whitespace, so empty fields
# round-trip intact.
_FS=$'\x1f'

# ── row accumulator ───────────────────────────────────────────────────────────
# One US-separated line per candidate:
#   task_id ␟ status ␟ class ␟ verdict ␟ action ␟ evidence ␟ flags
# The separated intermediate exists so the JSON emitter can be a python3 pass
# that escapes `evidence` prose correctly instead of hand-rolling JSON quoting.
ROWS_TSV="$(mktemp "${TMPDIR:-/tmp}/gate-staleness-rows-XXXXXX")"
_NEWEST_PROPOSAL_PY="$(mktemp "${TMPDIR:-/tmp}/gate-staleness-proposal-XXXXXX.py")"
_cleanup() { rm -f "$ROWS_TSV" "$_NEWEST_PROPOSAL_PY"; }
trap _cleanup EXIT

_emit_row() {
    printf '%s\n' "$1$_FS$2$_FS$3$_FS$4$_FS$5$_FS$6$_FS$7" >> "$ROWS_TSV"
}

# ── read-only candidate enumeration ───────────────────────────────────────────
# Dual-engine, both strictly read-only, mirroring scripts/lane-task-status.sh:
# the sqlite3 CLI at an absolute path (so this never depends on which sqlite3
# is first on PATH) with a PATH fallback, then python3's stdlib with a
# `file:...?mode=ro` URI. A missing / 0-byte / unreadable DB warns and yields
# ZERO candidates rather than aborting — this script is advisory-only.
# The sqlite3 CLI, resolved ONCE: prefer the stable system binary at an
# absolute path so this never depends on which sqlite3 is first on PATH, then
# fall back to a PATH lookup (the scripts/lane-task-status.sh convention).
_SQLITE_BIN=""
for _c in /usr/bin/sqlite3 sqlite3; do
    if command -v "$_c" >/dev/null 2>&1; then _SQLITE_BIN="$_c"; break; fi
done

_CANDIDATES=""
if [ ! -s "$DB" ]; then
    warn "Task DB is missing or empty: $DB — reporting zero candidates."
else
    _query="SELECT id, status, coalesce(heartbeat_at,''), coalesce(claimant_run_id,'')
            FROM tasks WHERE tag='$TAG' AND status IN ('blocked','in-progress') ORDER BY id;"
    if [ -n "$_SQLITE_BIN" ]; then
        _CANDIDATES="$("$_SQLITE_BIN" -readonly -separator "$_FS" "$DB" "$_query" 2>/dev/null || true)"
    fi
    if [ -z "$_CANDIDATES" ] && command -v python3 >/dev/null 2>&1; then
        _CANDIDATES="$(_GS_DB="$DB" _GS_TAG="$TAG" python3 - <<'PY' 2>/dev/null || true
import os, sqlite3, sys
try:
    con = sqlite3.connect(f"file:{os.environ['_GS_DB']}?mode=ro", uri=True, timeout=5.0)
    rows = con.execute(
        "SELECT id, status, coalesce(heartbeat_at,''), coalesce(claimant_run_id,'') "
        "FROM tasks WHERE tag=? AND status IN ('blocked','in-progress') ORDER BY id",
        (os.environ["_GS_TAG"],),
    ).fetchall()
    con.close()
    for r in rows:
        sys.stdout.write("\x1f".join(str(c) for c in r) + "\n")
except Exception:
    pass
PY
)"
    fi
    if [ -z "$_CANDIDATES" ]; then
        # Genuinely-empty and unreadable are indistinguishable from here, but
        # both degrade the same way: zero candidates, exit 0.
        info "No blocked / in-progress candidates under tag='$TAG'."
    fi
fi

# ── liveness guard (invariant L1) ─────────────────────────────────────────────
# Measured live on 2026-07-26: ALL TEN in-progress tasks had every dependency
# `done` (task 5321 itself among them). A "premise resolved => re-dispatch"
# rule without this guard would have targeted ten actively-running agents, so
# the capability would destroy work rather than recover it.
_LIVENESS_WINDOW_SEC=$((STALE_HEARTBEAT_MIN * 60))
_NOW_EPOCH="$(date -u +%s)"

# _is_live <task_id> <heartbeat_at> — exit 0 when the row is LIVE.
_is_live() {
    local id="$1" hb="$2" hb_epoch
    # A NULL/empty heartbeat is NOT live: `blocked` rows legitimately carry
    # none, and treating that as live would blind the sweep to class A and C
    # entirely.
    [ -n "$hb" ] || return 1
    if ! hb_epoch="$(date -u -d "$hb" +%s 2>/dev/null)"; then
        warn "Task $id: unparseable heartbeat_at '$hb' — treating as LIVE (fail-safe: an unreadable liveness signal must never license a re-dispatch)."
        return 0
    fi
    [ $(( _NOW_EPOCH - hb_epoch )) -lt "$_LIVENESS_WINDOW_SEC" ]
}

# ── metadata accessor ─────────────────────────────────────────────────────────
# _meta <task_id> <json_path> — read one scalar out of the row's metadata via
# SQLite's json_extract, read-only. Empty output means absent/NULL/unreadable.
_meta() {
    local id="$1" path="$2"
    [ -n "$_SQLITE_BIN" ] || return 0
    "$_SQLITE_BIN" -readonly "$DB" \
        "SELECT coalesce(json_extract(metadata,'$path'),'') FROM tasks WHERE tag='$TAG' AND id=$id LIMIT 1;" \
        2>/dev/null || true
}

# ── class A: gate_closure ─────────────────────────────────────────────────────
# Escalation-state oracle. Reads ONLY the live escalation dir: an archived
# escalation is by construction no longer gating, and the live-dir absence is
# precisely the signal. Matches `esc-<id>-*.json` and never the
# `*.json.lock` sidecars the store also holds.
#
# Sets _ESC_STATE to one of:
#   gated    — at least one live escalation has status=pending
#   clear    — zero matching files, or all of them terminal
#   unknown  — the dir is missing/unreadable, or a file failed to parse
_esc_state() {
    local id="$1" f found=0 pending=0 st
    _ESC_STATE="unknown"
    # -d alone is NOT enough: stat succeeds on a mode-000 dir (it needs +x on
    # the PARENT, not on the dir), but the glob below then fails to expand for
    # want of +r, `[ -e ]` swallows the unexpanded pattern, and found=0 lands in
    # `clear` — reporting STALE for a task whose gate is still live, without
    # ever entering the loop where the terminal allowlist could defend. An
    # unreadable/unsearchable dir must therefore degrade exactly like a missing
    # one (root-owned dir swept by another uid, a stale mount, mode 000).
    if [ ! -d "$ESCALATIONS" ] || [ ! -r "$ESCALATIONS" ] || [ ! -x "$ESCALATIONS" ]; then
        warn "Task $id: escalations dir is missing or unreadable: $ESCALATIONS — reporting unknown (a failed oracle must never manufacture a STALE verdict)."
        return 0
    fi
    for f in "$ESCALATIONS"/esc-"$id"-*.json; do
        [ -e "$f" ] || continue
        found=$((found + 1))
        st="$(python3 -c 'import json,sys
try:
    sys.stdout.write(str(json.load(open(sys.argv[1])).get("status","")))
except Exception:
    sys.stdout.write("\x00")' "$f" 2>/dev/null || printf '\000')"
        # TERMINAL ALLOWLIST, not a pending-only match (L2). `pending` gates and
        # `resolved`/`dismissed` clear; EVERY other value — unrecognized, JSON
        # null (which reaches here as the literal "None"), empty, or the NUL
        # parse sentinel — is a FAILED ORACLE READ and degrades to `unknown`.
        # A pending-only match would sink all of those into `clear`, yielding
        # STALE / close / an emitted request telling the consumer to CANCEL a
        # task whose gate may still be live — failing open toward the sweep's
        # single most destructive action.
        case "$st" in
            "$(printf '\000')")
                warn "Task $id: unparseable escalation file $(basename "$f") — reporting unknown, not STALE."
                return 0 ;;
            pending) pending=$((pending + 1)) ;;
            resolved|dismissed) : ;;
            *)
                warn "Task $id: escalation file $(basename "$f") carries an unrecognized status '$st' — reporting unknown, not STALE."
                return 0 ;;
        esac
    done
    if [ "$pending" -gt 0 ]; then
        _ESC_STATE="gated"
    else
        _ESC_STATE="clear"
        _ESC_FOUND="$found"
    fi
}

# ── class B: merge_verify_red ─────────────────────────────────────────────────
# Newest-proposal extractor. Recency is keyed on investigated_at, then
# timestamp — NOT on array position: the store appends without reordering and
# a re-investigation can rewrite an earlier entry in place, so proposals[-1]
# is not reliably the newest. Emits on stdout:
#   line 1        block_reason ␟ block_class ␟ main_sha ␟ head_sha (US-separated)
#   lines 2..n+1  one files_referenced path each (absent when there are none)
# Nothing at all is emitted when the row records no usable proposal.
cat > "$_NEWEST_PROPOSAL_PY" <<'PY'
import json, sys
try:
    props = json.loads(sys.stdin.read() or "[]")
except Exception:
    props = []
if not isinstance(props, list):
    props = []
props = [p for p in props if isinstance(p, dict)]
if not props:
    sys.exit(0)
newest = max(props, key=lambda p: (str(p.get("investigated_at") or ""),
                                   str(p.get("timestamp") or "")))
files = newest.get("files_referenced") or []
if not isinstance(files, list):
    files = []


def flat(v):
    # Neutralize BOTH the field separator (US) and any newline, so a prose
    # block_reason can never forge a field boundary or a row boundary.
    return str(v if v is not None else "").replace("\x1f", " ").replace("\n", " ")


sys.stdout.write("\x1f".join(flat(newest.get(k)) for k in
                             ("block_reason", "block_class", "main_sha", "head_sha")) + "\n")
for f in files:
    f = flat(f)
    if f:
        sys.stdout.write(f + "\n")
PY

# --main-ref is constant across candidates, so resolve it ONCE rather than
# once per row. Empty means it does not resolve inside --repo (including the
# case where --repo is not a git repo at all), which degrades every class-B
# row to `unknown` — the adjudication is scoped to --repo and nothing else.
_MAIN_REF_SHA="$(git -C "$REPO" rev-parse --verify --quiet "${MAIN_REF}^{commit}" 2>/dev/null || true)"

# _classify_merge_verify_red <task_id> — sets _MVR_MATCHED plus, when matched,
# _MVR_VERDICT / _MVR_ACTION / _MVR_EVIDENCE.
# _classify_gate_closure <task_id> <status> <adjudicate:0|1> — sets
# _GC_MATCHED plus, when matched and adjudicating, _GC_VERDICT / _GC_ACTION /
# _GC_EVIDENCE. `blocked`-only (invariant L4).
_classify_gate_closure() {
    local id="$1" status="$2" adjudicate="$3" always
    _GC_MATCHED=0
    _GC_VERDICT="unknown"
    _GC_ACTION="none"
    _GC_EVIDENCE=""

    [ "$status" = "blocked" ] || return 0
    [ "$(_meta "$id" '$.task_kind')" = "deterministic" ] || return 0
    # SQLite stores the JSON `true` as 1, so accept either spelling.
    always="$(_meta "$id" '$.always_escalates')"
    case "$always" in
        1|true) _GC_MATCHED=1 ;;
        *)      return 0 ;;
    esac
    [ "$adjudicate" = 1 ] || return 0

    _esc_state "$id"
    case "$_ESC_STATE" in
        gated)
            _GC_VERDICT="GATED"
            _GC_EVIDENCE="a live status=pending escalation still gates this task" ;;
        clear)
            _GC_VERDICT="STALE"
            # `close`, not `redispatch`: per #5316 §5 the correct closure for a
            # satisfied deterministic gate is a transition to `cancelled`, not
            # a re-run.
            _GC_ACTION="close"
            _GC_EVIDENCE="no live pending escalation remains in $ESCALATIONS (found ${_ESC_FOUND:-0} terminal file(s))" ;;
        *)
            _GC_VERDICT="unknown"
            _GC_EVIDENCE="escalation state could not be read; not upgraded to STALE" ;;
    esac
}

# _classify_merge_verify_red <task_id> <adjudicate:0|1> — sets _MVR_MATCHED
# plus, when matched and adjudicating, _MVR_VERDICT / _MVR_ACTION /
# _MVR_EVIDENCE. Spans `blocked` AND `in-progress` (invariant L4).
_classify_merge_verify_red() {
    local id="$1" adjudicate="$2" raw out b_reason b_class b_main_sha b_head_sha
    local resolved touched first_path resolving
    local -a lines=()
    _MVR_MATCHED=0
    _MVR_VERDICT="unknown"
    _MVR_ACTION="none"
    _MVR_EVIDENCE=""
    _MVR_FILES=()

    raw="$(_meta "$id" '$.dry_run_proposals')"
    [ -n "$raw" ] || return 0
    out="$(printf '%s' "$raw" | python3 "$_NEWEST_PROPOSAL_PY" 2>/dev/null || true)"
    [ -n "$out" ] || return 0

    # Here-string, not a pipe — see the SIGPIPE/pipefail note in the test suite.
    mapfile -t lines <<<"$out"
    IFS="$_FS" read -r b_reason b_class b_main_sha b_head_sha <<<"${lines[0]}"
    _MVR_FILES=("${lines[@]:1}")

    # block_class is a CONFIRMING hint, not the primary key: measured live it
    # is present on only 2 of 55 dry_run_proposals entries, so keying on it
    # alone would miss nearly every real merge-verify red. The block_reason
    # prose prefix is populated on all of them.
    case "$b_class" in
        merge_verify_red) _MVR_MATCHED=1 ;;
    esac
    case "$b_reason" in
        "Post-merge verification failed"*) _MVR_MATCHED=1 ;;
    esac
    [ "$_MVR_MATCHED" = 1 ] || return 0
    # A non-primary class is disclosed but never adjudicated: skipping the git
    # work here also keeps its warnings out of the report, which would
    # otherwise be noise about a premise nobody is acting on.
    [ "$adjudicate" = 1 ] || return 0

    if [ "${#_MVR_FILES[@]}" -eq 0 ]; then
        warn "Task $id: merge_verify_red proposal records no files_referenced — no diff surface to adjudicate; reporting unknown, not STALE."
        _MVR_EVIDENCE="post-merge-verify red with no files_referenced recorded; nothing to diff"
        return 0
    fi
    if [ -z "$b_main_sha" ]; then
        warn "Task $id: merge_verify_red proposal records no main_sha — reporting unknown, not STALE."
        _MVR_EVIDENCE="post-merge-verify red with no main_sha recorded; premise not adjudicable"
        return 0
    fi
    if [ -z "$_MAIN_REF_SHA" ]; then
        warn "Task $id: --main-ref '$MAIN_REF' does not resolve in repo '$REPO' — reporting unknown, not STALE."
        _MVR_EVIDENCE="--main-ref ${MAIN_REF} does not resolve in ${REPO}; premise not adjudicable"
        return 0
    fi

    # ── premise-resolved adjudication ────────────────────────────────────────
    # REACHABILITY IS CHECKED BEFORE ANY DIFF, per #5316 §3. A plausible
    # main_sha can be a DISCARDED DUPLICATE merge: it resolves, its diff looks
    # right, and it is nonetheless unreachable from main. That is exactly how
    # #5264 was mis-cleared when only `git show --stat` was consulted — a diff
    # inspection cannot distinguish a landed commit from a discarded twin, and
    # only `git merge-base --is-ancestor` can.
    if ! resolved="$(git -C "$REPO" rev-parse --verify --quiet "${b_main_sha}^{commit}" 2>/dev/null)" \
       || [ -z "$resolved" ]; then
        warn "Task $id: recorded main_sha '$b_main_sha' does not resolve in repo '$REPO' — reporting unknown, not STALE."
        _MVR_EVIDENCE="recorded main_sha=${b_main_sha} does not resolve in ${REPO}"
        return 0
    fi
    if ! git -C "$REPO" merge-base --is-ancestor "$resolved" "$_MAIN_REF_SHA" 2>/dev/null; then
        warn "Task $id: recorded main_sha '$b_main_sha' resolves but is NOT an ancestor of ${MAIN_REF} (a discarded-duplicate / side-branch commit) — reporting unknown, not STALE."
        _MVR_EVIDENCE="main_sha=${b_main_sha} is not an ancestor of ${MAIN_REF}; premise not adjudicable"
        return 0
    fi
    if [ "$resolved" = "$_MAIN_REF_SHA" ]; then
        _MVR_VERDICT="UNRESOLVED"
        _MVR_EVIDENCE="${MAIN_REF} is still at the recorded main_sha ${b_main_sha}; main has not advanced"
        return 0
    fi

    touched="$(git -C "$REPO" diff --name-only "$resolved".."$_MAIN_REF_SHA" -- "${_MVR_FILES[@]}" 2>/dev/null || true)"
    if [ -z "$touched" ]; then
        _MVR_VERDICT="UNRESOLVED"
        _MVR_EVIDENCE="main advanced ${b_main_sha}..${MAIN_REF} but touched none of the ${#_MVR_FILES[@]} referenced path(s)"
        return 0
    fi
    first_path="${touched%%$'\n'*}"
    resolving="$(git -C "$REPO" log --format=%h -1 "$resolved".."$_MAIN_REF_SHA" -- "$first_path" 2>/dev/null || true)"
    _MVR_VERDICT="STALE"
    _MVR_ACTION="reverify"
    _MVR_EVIDENCE="premise resolved: ${first_path} was touched by ${resolving:-?} in ${b_main_sha}..${MAIN_REF}"
}

# ── class C: unmet_dependency ─────────────────────────────────────────────────
# `blocked`-ONLY, per invariant L4. This is not a stylistic scoping choice:
# measured on 2026-07-26, every one of the ten live `in-progress` tasks had
# all of its dependencies `done` (task 5321 itself among them), so a class C
# that spanned `in-progress` would emit a re-dispatch request for every
# actively-running agent.
#
# One tag-scoped LEFT JOIN, not a lookup per dependency: `tasks` is
# PRIMARY KEY (tag, id), so an unqualified `id =` would silently conflate tags
# the moment a second tag exists. An EMPTY status column means the depends_on
# resolves to no row under this tag — that degrades the row to `unknown`,
# because an unresolvable dependency must never read as a satisfied one.
#
# _classify_unmet_dependency <task_id> <status> <adjudicate:0|1>
_classify_unmet_dependency() {
    local id="$1" status="$2" adjudicate="$3"
    local rows dep_id dep_status pairs="" n=0 unresolved=0 unresolvable=0
    _UD_MATCHED=0
    _UD_VERDICT="unknown"
    _UD_ACTION="none"
    _UD_EVIDENCE=""

    [ "$status" = "blocked" ] || return 0
    [ -n "$_SQLITE_BIN" ] || return 0

    rows="$("$_SQLITE_BIN" -readonly -separator "$_FS" "$DB" \
        "SELECT d.depends_on, coalesce(dt.status,'')
           FROM dependencies d
           LEFT JOIN tasks dt ON dt.tag = d.tag AND dt.id = d.depends_on
          WHERE d.tag='$TAG' AND d.task_id=$id
          ORDER BY d.depends_on;" 2>/dev/null || true)"
    # An EMPTY dependency set is NOT "all satisfied" — it is not class C at all.
    [ -n "$rows" ] || return 0
    _UD_MATCHED=1
    [ "$adjudicate" = 1 ] || return 0

    while IFS="$_FS" read -r dep_id dep_status; do
        [ -n "${dep_id:-}" ] || continue
        n=$((n + 1))
        pairs="${pairs:+$pairs, }${dep_id}=${dep_status:-<no row>}"
        case "$dep_status" in
            done|cancelled) ;;
            "")             unresolvable=$((unresolvable + 1)) ;;
            *)              unresolved=$((unresolved + 1)) ;;
        esac
    done <<EOF
$rows
EOF

    if [ "$unresolvable" -gt 0 ]; then
        warn "Task $id: $unresolvable dependency id(s) resolve to no row under tag='$TAG' — reporting unknown, not STALE (an unresolvable dependency must never read as a satisfied one)."
        _UD_EVIDENCE="dependencies: ${pairs}; ${unresolvable} unresolvable under tag='$TAG'"
        return 0
    fi
    if [ "$unresolved" -gt 0 ]; then
        _UD_VERDICT="UNRESOLVED"
        _UD_EVIDENCE="dependencies: ${pairs}; ${unresolved} of ${n} not yet terminal"
        return 0
    fi
    _UD_VERDICT="STALE"
    _UD_ACTION="redispatch"
    _UD_EVIDENCE="all ${n} dependency(ies) terminal: ${pairs}"
}

# ── #5316 corruption signatures (invariant L5) ────────────────────────────────
# Both signatures are catalogued in docs/notes/offline-lane-red-corruption-
# remediation.md; they are lifted here from prose into executable checks and
# wired as SUPPRESSORS rather than as a fourth trigger class.
#
# Signature 1 — help-text-as-failing-tests: an auto-filed record whose
# `metadata.failing_tests` holds harness OUTPUT rather than test names. This
# marker set is the single source of truth; the docs note points at this
# variable rather than restating it.
_CORRUPT_AUTOFILE_MARKERS=('Usage:' 'Options:' 'verify.sh: ERROR')

# The matching predicate, built once from the marker set. `instr()` and NOT
# `LIKE`, because SQLite's LIKE is case-insensitive for ASCII by default and a
# lowercase 'usage:' inside a real test name must not read as harness output.
_CORRUPT_MARKER_SQL=""
for _m in "${_CORRUPT_AUTOFILE_MARKERS[@]}"; do
    _CORRUPT_MARKER_SQL="${_CORRUPT_MARKER_SQL:+$_CORRUPT_MARKER_SQL OR }instr(value,'${_m//\'/\'\'}')>0"
done
unset _m

# _compute_flags <task_id> — sets _FLAGS to a comma-separated list in a FIXED
# order (corrupt_autofile, then the provenance flag), or `-` when clean.
_compute_flags() {
    local id="$1" hits prov
    local -a flags=()

    # Signature 1. Evaluated inside SQLite (one cheap query) rather than a
    # python3 pass, so the sweep stays timer-friendly on a large store. A
    # malformed / absent / non-array failing_tests makes json_each error; that
    # is swallowed and read as "no marker found" — an unreadable field is not
    # positive evidence of harness output.
    if [ -n "$_SQLITE_BIN" ]; then
        hits="$("$_SQLITE_BIN" -readonly "$DB" \
            "SELECT count(*) FROM json_each((SELECT json_extract(metadata,'\$.failing_tests')
                                               FROM tasks WHERE tag='$TAG' AND id=$id LIMIT 1))
              WHERE $_CORRUPT_MARKER_SQL;" 2>/dev/null || true)"
        [ "${hits:-0}" -gt 0 ] 2>/dev/null && flags+=("corrupt_autofile")
    fi

    # Signature 2 — misattributed provenance. REACHABILITY, never diff
    # inspection: #5316 records that `git show --stat` alone MIS-CLEARED #5264,
    # because a discarded duplicate merge shows a perfectly plausible diff and
    # fails only the ancestor test. Reachability is therefore checked first and
    # is the only evidence that clears a recorded provenance SHA.
    #
    # The ancestry probe is handed the PRE-RESOLVED $_MAIN_REF_SHA, never the
    # raw $MAIN_REF the operator typed, and the empty case is guarded BEFORE
    # the probe runs. `merge-base --is-ancestor` exits non-zero for two
    # unrelated reasons: the commit genuinely is not an ancestor, and the
    # second argument does not resolve at all (measured: exit 128). Passing a
    # ref NAME therefore makes a MISSING ancestry oracle indistinguishable from
    # positive evidence of corruption — and `misattributed_provenance` means,
    # precisely, "resolves but is NOT an ancestor". Feeding the probe an
    # already-resolved SHA removes that second failure mode outright, so a
    # non-zero exit now means exactly one thing. Do not "simplify" the two
    # spellings back together. This matches the class-B path above, which
    # guards on an empty $_MAIN_REF_SHA on the identical premise, and invariant
    # L2: an unreadable oracle degrades, it never manufactures an actionable
    # verdict.
    #
    # Ordering: the $REPO-only rev-parse runs FIRST and the ancestry guard
    # second, because only the ancestry claim depends on --main-ref. An
    # unresolvable provenance SHA is unresolvable no matter what main is, so
    # a missing oracle must not silently clear it.
    prov="$(_meta "$id" '$.done_provenance.commit')"
    if [ -n "$prov" ]; then
        if ! git -C "$REPO" rev-parse --verify --quiet "${prov}^{commit}" >/dev/null 2>&1; then
            # Conservative: a provenance SHA we cannot resolve is held, not
            # cleared. The fail-safe direction for a re-dispatch decision is
            # always "do nothing". This probe reads $REPO alone, so it stays
            # adjudicable — and keeps flagging — even with no ancestry oracle.
            flags+=("provenance_unresolvable")
        elif [ -z "$_MAIN_REF_SHA" ]; then
            warn "Task $id: --main-ref '$MAIN_REF' does not resolve in repo '$REPO' — provenance reachability not adjudicable; no provenance flag asserted."
        elif ! git -C "$REPO" merge-base --is-ancestor "$prov" "$_MAIN_REF_SHA" >/dev/null 2>&1; then
            flags+=("misattributed_provenance")
        fi
    fi

    if [ "${#flags[@]}" -eq 0 ]; then
        _FLAGS="-"
    else
        _FLAGS="$(IFS=,; printf '%s' "${flags[*]}")"
    fi
}

# ── classify ──────────────────────────────────────────────────────────────────
while IFS="$_FS" read -r task_id status heartbeat_at claimant_run_id; do
    [ -n "${task_id:-}" ] || continue

    # L1: the liveness guard is the FIRST predicate, and it short-circuits —
    # no class predicate evaluates, no flags are computed, and no request is
    # ever emitted for a LIVE row.
    if _is_live "$task_id" "$heartbeat_at"; then
        [ "$CLASS" = "all" ] || continue
        N_CANDIDATES=$((N_CANDIDATES + 1))
        N_LIVE_SKIPPED=$((N_LIVE_SKIPPED + 1))
        _emit_row "$task_id" "$status" "-" "LIVE" "none" \
            "heartbeat_at=${heartbeat_at} within the ${STALE_HEARTBEAT_MIN}m liveness window; claimant=${claimant_run_id:--}" \
            "-"
        continue
    fi

    row_class="-"
    # NO-CLASS, not `unknown`: nothing failed here. `unknown` is reserved for a
    # class that MATCHED and whose oracle then could not be read, so that the
    # counter keeps carrying "could not tell" rather than being swamped by the
    # majority of the store, which matches no class at all.
    row_verdict="NO-CLASS"
    row_action="none"
    row_evidence="no trigger class matched this candidate"
    row_flags="-"

    # ── class-precedence dispatcher (invariant L3) ───────────────────────────
    # Fixed order: gate_closure > merge_verify_red > unmet_dependency. The
    # FIRST match becomes the row's primary class and is the only one
    # adjudicated; any further match is disclosed as `also:<class>` in
    # evidence but never adjudicated and never counted, so a multi-class row
    # increments exactly one counter and appears exactly once.
    #
    # The order is load-bearing, not alphabetical: class A's action is `close`
    # (a satisfied deterministic gate is transitioned to `cancelled`, per
    # #5316 §5) while class C's is `redispatch`. Letting C win would RE-RUN a
    # gate task that should simply be closed.
    _primary=""
    _also=""

    _classify_gate_closure "$task_id" "$status" 1
    if [ "$_GC_MATCHED" = 1 ]; then
        _primary="gate_closure"
        row_verdict="$_GC_VERDICT"; row_action="$_GC_ACTION"; row_evidence="$_GC_EVIDENCE"
    fi

    if [ -z "$_primary" ]; then _adj=1; else _adj=0; fi
    _classify_merge_verify_red "$task_id" "$_adj"
    if [ "$_MVR_MATCHED" = 1 ]; then
        if [ -z "$_primary" ]; then
            _primary="merge_verify_red"
            row_verdict="$_MVR_VERDICT"; row_action="$_MVR_ACTION"; row_evidence="$_MVR_EVIDENCE"
        else
            _also="$_also also:merge_verify_red"
        fi
    fi

    if [ -z "$_primary" ]; then _adj=1; else _adj=0; fi
    _classify_unmet_dependency "$task_id" "$status" "$_adj"
    if [ "$_UD_MATCHED" = 1 ]; then
        if [ -z "$_primary" ]; then
            _primary="unmet_dependency"
            row_verdict="$_UD_VERDICT"; row_action="$_UD_ACTION"; row_evidence="$_UD_EVIDENCE"
        else
            _also="$_also also:unmet_dependency"
        fi
    fi

    if [ -n "$_primary" ]; then row_class="$_primary"; fi
    if [ -n "$_also" ]; then row_evidence="${row_evidence}; ${_also# }"; fi

    # ── #5316 corruption suppression (invariant L5) ──────────────────────────
    # Flags are computed for EVERY non-live row, not just for hits, so the
    # sweep keeps 5316's audit coverage on records that are corrupt but not
    # yet stranded. Only a row that would otherwise be a confirmed hit is
    # demoted — a corrupt record's block premise is untrustworthy, so
    # auto-re-dispatching it would act on a false premise.
    _compute_flags "$task_id"
    row_flags="$_FLAGS"
    if [ "$row_verdict" = "STALE" ] && [ "$row_flags" != "-" ]; then
        row_verdict="CORRUPT-HOLD"
        row_action="human_gate"
        row_evidence="${row_evidence}; held for a human gate on #5316 corruption flag(s): ${row_flags}"
    fi

    # ── --class filtering ────────────────────────────────────────────────────
    # A restricted sweep emits only rows of that class; the summary counters
    # are computed over emitted rows, so a filtered report is internally
    # consistent rather than reporting counts it did not print.
    if [ "$CLASS" != "all" ] && [ "$row_class" != "$CLASS" ]; then
        continue
    fi
    N_CANDIDATES=$((N_CANDIDATES + 1))

    case "$row_verdict" in
        STALE)
            case "$row_class" in
                gate_closure)     N_GATE_CLOSURE=$((N_GATE_CLOSURE + 1)) ;;
                merge_verify_red) N_MERGE_VERIFY_RED=$((N_MERGE_VERIFY_RED + 1)) ;;
                unmet_dependency) N_UNMET_DEPENDENCY=$((N_UNMET_DEPENDENCY + 1)) ;;
            esac ;;
        CORRUPT-HOLD) N_CORRUPT_HOLD=$((N_CORRUPT_HOLD + 1)) ;;
        NO-CLASS) N_NO_CLASS=$((N_NO_CLASS + 1)) ;;
        unknown) N_UNKNOWN=$((N_UNKNOWN + 1)) ;;
    esac
    _emit_row "$task_id" "$status" "$row_class" "$row_verdict" "$row_action" "$row_evidence" "$row_flags"
done <<EOF
$_CANDIDATES
EOF

# ── emit: table (default) or json ─────────────────────────────────────────────
_SUMMARY_JSON="$(printf '{"candidates":%d,"gate_closure":%d,"merge_verify_red":%d,"unmet_dependency":%d,"corrupt_hold":%d,"live_skipped":%d,"no_class":%d,"unknown":%d}' \
    "$N_CANDIDATES" "$N_GATE_CLOSURE" "$N_MERGE_VERIFY_RED" "$N_UNMET_DEPENDENCY" \
    "$N_CORRUPT_HOLD" "$N_LIVE_SKIPPED" "$N_NO_CLASS" "$N_UNKNOWN")"

if [ "$FORMAT" = "json" ]; then
    _GS_ROWS="$ROWS_TSV" _GS_SUMMARY="$_SUMMARY_JSON" python3 - <<'PY'
import json, os, sys
rows = []
with open(os.environ["_GS_ROWS"]) as fh:
    for line in fh:
        line = line.rstrip("\n")
        if not line:
            continue
        p = line.split("\x1f")
        rows.append({
            "task_id": int(p[0]),
            "status": p[1],
            "class": p[2],
            "verdict": p[3],
            "action": p[4],
            "evidence": p[5],
            "flags": [] if p[6] == "-" else p[6].split(","),
        })
json.dump({"candidates": rows, "summary": json.loads(os.environ["_GS_SUMMARY"])}, sys.stdout)
sys.stdout.write("\n")
PY
else
    printf '%-8s %-12s %-18s %-13s %-11s %-52s %s\n' \
        task_id status class verdict action evidence flags
    while IFS="$_FS" read -r c_id c_status c_class c_verdict c_action c_evidence c_flags; do
        [ -n "${c_id:-}" ] || continue
        printf '%-8s %-12s %-18s %-13s %-11s %-52s %s\n' \
            "$c_id" "$c_status" "$c_class" "$c_verdict" "$c_action" "$c_evidence" "$c_flags"
    done < "$ROWS_TSV"
    printf 'SWEEP: candidates=%d gate_closure=%d merge_verify_red=%d unmet_dependency=%d corrupt_hold=%d live_skipped=%d no_class=%d unknown=%d\n' \
        "$N_CANDIDATES" "$N_GATE_CLOSURE" "$N_MERGE_VERIFY_RED" "$N_UNMET_DEPENDENCY" \
        "$N_CORRUPT_HOLD" "$N_LIVE_SKIPPED" "$N_NO_CLASS" "$N_UNKNOWN"
fi

# ── --emit-requests (invariant L6) ────────────────────────────────────────────
# Runs AFTER the report is fully assembled and printed, so a request-emission
# failure can never truncate the report — the sweep is advisory and emission
# is a side channel, not a gate.
_EMITTED_BY="scripts/deterministic-gate-closure-staleness-sweep.sh"

# _write_request <task_id> <class> <verdict> <action> <evidence> — render one
# request atomically: a mktemp intermediate in the SAME directory (so the mv
# is a rename within one filesystem, never a copy) followed by mv. The
# intermediate is removed on every failure path. python3 does the rendering so
# prose `evidence` is escaped correctly rather than hand-quoted.
_write_request() {
    local id="$1" cls="$2" verdict="$3" action="$4" evidence="$5"
    local dest tmp
    dest="$REQUESTS_DIR/redispatch-${id}-${cls}.json"
    if ! tmp="$(mktemp "$REQUESTS_DIR/redispatch-tmp-XXXXXX" 2>/dev/null)"; then
        warn "Task $id: could not create a temp file in $REQUESTS_DIR — no request emitted."
        return 0
    fi
    if _GS_ID="$id" _GS_CLASS="$cls" _GS_VERDICT="$verdict" _GS_ACTION="$action" \
       _GS_EVIDENCE="$evidence" _GS_MAIN_SHA="$_MAIN_REF_SHA" _GS_EMITTER="$_EMITTED_BY" \
       python3 - > "$tmp" 2>/dev/null <<'PY'
import json, os, sys
# No wall-clock field, deliberately: re-emission must be byte-idempotent so a
# consumer can diff the directory instead of re-processing it. Recency, when
# needed, is the file's mtime.
json.dump({
    "schema_version": 1,
    "task_id": int(os.environ["_GS_ID"]),
    "class": os.environ["_GS_CLASS"],
    "verdict": os.environ["_GS_VERDICT"],
    "action": os.environ["_GS_ACTION"],
    "evidence": os.environ["_GS_EVIDENCE"],
    "main_ref_sha": os.environ["_GS_MAIN_SHA"],
    "emitted_by": os.environ["_GS_EMITTER"],
}, sys.stdout, indent=2, sort_keys=True)
sys.stdout.write("\n")
PY
    then
        if ! mv -f "$tmp" "$dest" 2>/dev/null; then
            rm -f "$tmp"
            warn "Task $id: could not place its re-dispatch request at $dest — none emitted."
        fi
    else
        rm -f "$tmp"
        warn "Task $id: failed to render its re-dispatch request — none emitted."
    fi
}

if [ -n "$REQUESTS_DIR" ]; then
    _emit_ok=1
    if [ ! -d "$REQUESTS_DIR" ]; then
        # Created on demand only when its PARENT already exists: silently
        # materializing a whole missing path would hide a typo'd --emit-requests
        # as "0 requests emitted" rather than surfacing it.
        if [ -d "$(dirname "$REQUESTS_DIR")" ] && mkdir "$REQUESTS_DIR" 2>/dev/null; then
            info "Created request dir $REQUESTS_DIR."
        else
            warn "Request dir could not be created: $REQUESTS_DIR (its parent does not exist, or is not writable) — the report above is complete; no requests were emitted."
            _emit_ok=0
        fi
    fi
    if [ "$_emit_ok" = 1 ] && [ ! -w "$REQUESTS_DIR" ]; then
        warn "Request dir is not writable: $REQUESTS_DIR — the report above is complete; no requests were emitted."
        _emit_ok=0
    fi
    if [ "$_emit_ok" = 1 ]; then
        # STALE only — every other verdict is skipped, CORRUPT-HOLD included
        # (invariant L5). ROWS_TSV already holds exactly the rows the report
        # printed, so --class filtering carries through for free and a
        # filtered sweep can never emit a request it did not report.
        while IFS="$_FS" read -r r_id r_status r_class r_verdict r_action r_evidence r_flags; do
            [ -n "${r_id:-}" ] || continue
            [ "$r_verdict" = "STALE" ] || continue
            _write_request "$r_id" "$r_class" "$r_verdict" "$r_action" "$r_evidence"
        done < "$ROWS_TSV"
    fi
fi

exit 0
