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
#   unknown      — an oracle could not be read; never upgraded to STALE.
# A trailing SWEEP: line (table) / summary object (json) carries the counters
# candidates, gate_closure, merge_verify_red, unmet_dependency, corrupt_hold,
# live_skipped, unknown.
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
#   (Further invariants are recorded as each classifier lands.)

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
N_UNKNOWN=0

# ── row accumulator ───────────────────────────────────────────────────────────
# One tab-separated line per candidate:
#   task_id \t status \t class \t verdict \t action \t evidence \t flags
# The TSV intermediate exists so the JSON emitter can be a python3 pass that
# escapes `evidence` prose correctly instead of hand-rolling JSON quoting.
ROWS_TSV="$(mktemp "${TMPDIR:-/tmp}/gate-staleness-rows-XXXXXX")"
_cleanup() { rm -f "$ROWS_TSV"; }
trap _cleanup EXIT

_emit_row() {
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" "$6" "$7" >> "$ROWS_TSV"
}

# ── read-only candidate enumeration ───────────────────────────────────────────
# Dual-engine, both strictly read-only, mirroring scripts/lane-task-status.sh:
# the sqlite3 CLI at an absolute path (so this never depends on which sqlite3
# is first on PATH) with a PATH fallback, then python3's stdlib with a
# `file:...?mode=ro` URI. A missing / 0-byte / unreadable DB warns and yields
# ZERO candidates rather than aborting — this script is advisory-only.
_CANDIDATES=""
if [ ! -s "$DB" ]; then
    warn "Task DB is missing or empty: $DB — reporting zero candidates."
else
    _sqlite_bin=""
    for _c in /usr/bin/sqlite3 sqlite3; do
        if command -v "$_c" >/dev/null 2>&1; then _sqlite_bin="$_c"; break; fi
    done
    _query="SELECT id, status, coalesce(heartbeat_at,''), coalesce(claimant_run_id,'')
            FROM tasks WHERE tag='$TAG' AND status IN ('blocked','in-progress') ORDER BY id;"
    if [ -n "$_sqlite_bin" ]; then
        _CANDIDATES="$("$_sqlite_bin" -readonly -separator "$(printf '\t')" "$DB" "$_query" 2>/dev/null || true)"
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
        sys.stdout.write("\t".join(str(c) for c in r) + "\n")
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

# ── classify ──────────────────────────────────────────────────────────────────
# (Class predicates land in later steps. Until then every non-LIVE candidate
# is reported `unknown` — the fail-safe verdict, which never emits a request.)
while IFS="$(printf '\t')" read -r task_id status heartbeat_at claimant_run_id; do
    [ -n "${task_id:-}" ] || continue
    N_CANDIDATES=$((N_CANDIDATES + 1))

    # L1: the liveness guard is the FIRST predicate, and it short-circuits —
    # no class predicate evaluates, no flags are computed, and no request is
    # ever emitted for a LIVE row.
    if _is_live "$task_id" "$heartbeat_at"; then
        N_LIVE_SKIPPED=$((N_LIVE_SKIPPED + 1))
        _emit_row "$task_id" "$status" "-" "LIVE" "none" \
            "heartbeat_at=${heartbeat_at} within the ${STALE_HEARTBEAT_MIN}m liveness window; claimant=${claimant_run_id:--}" \
            "-"
        continue
    fi

    row_class="-"
    row_verdict="unknown"
    row_action="none"
    row_evidence="no classifier has evaluated this row yet"
    row_flags="-"

    N_UNKNOWN=$((N_UNKNOWN + 1))
    _emit_row "$task_id" "$status" "$row_class" "$row_verdict" "$row_action" "$row_evidence" "$row_flags"
done <<EOF
$_CANDIDATES
EOF

# ── emit: table (default) or json ─────────────────────────────────────────────
_SUMMARY_JSON="$(printf '{"candidates":%d,"gate_closure":%d,"merge_verify_red":%d,"unmet_dependency":%d,"corrupt_hold":%d,"live_skipped":%d,"unknown":%d}' \
    "$N_CANDIDATES" "$N_GATE_CLOSURE" "$N_MERGE_VERIFY_RED" "$N_UNMET_DEPENDENCY" \
    "$N_CORRUPT_HOLD" "$N_LIVE_SKIPPED" "$N_UNKNOWN")"

if [ "$FORMAT" = "json" ]; then
    _GS_ROWS="$ROWS_TSV" _GS_SUMMARY="$_SUMMARY_JSON" python3 - <<'PY'
import json, os, sys
rows = []
with open(os.environ["_GS_ROWS"]) as fh:
    for line in fh:
        line = line.rstrip("\n")
        if not line:
            continue
        p = line.split("\t")
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
    while IFS="$(printf '\t')" read -r c_id c_status c_class c_verdict c_action c_evidence c_flags; do
        [ -n "${c_id:-}" ] || continue
        printf '%-8s %-12s %-18s %-13s %-11s %-52s %s\n' \
            "$c_id" "$c_status" "$c_class" "$c_verdict" "$c_action" "$c_evidence" "$c_flags"
    done < "$ROWS_TSV"
    printf 'SWEEP: candidates=%d gate_closure=%d merge_verify_red=%d unmet_dependency=%d corrupt_hold=%d live_skipped=%d unknown=%d\n' \
        "$N_CANDIDATES" "$N_GATE_CLOSURE" "$N_MERGE_VERIFY_RED" "$N_UNMET_DEPENDENCY" \
        "$N_CORRUPT_HOLD" "$N_LIVE_SKIPPED" "$N_UNKNOWN"
fi

exit 0
