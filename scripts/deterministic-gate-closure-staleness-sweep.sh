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
#   L2 — an unreadable escalation oracle (missing dir, unparseable file)
#        degrades that row to `unknown`, never to `STALE`. A failed oracle
#        lookup must never manufacture an actionable verdict — the same
#        posture as warm-lane-audit.sh invariant A3.
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
    if [ ! -d "$ESCALATIONS" ]; then
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
        case "$st" in
            "$(printf '\000')"|"")
                warn "Task $id: unparseable escalation file $(basename "$f") — reporting unknown, not STALE."
                return 0 ;;
            pending) pending=$((pending + 1)) ;;
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
_classify_merge_verify_red() {
    local id="$1" raw out b_reason b_class b_main_sha b_head_sha
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

# ── classify ──────────────────────────────────────────────────────────────────
# (Class predicates land in later steps. Until then every non-LIVE candidate
# is reported `unknown` — the fail-safe verdict, which never emits a request.)
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
    row_verdict="unknown"
    row_action="none"
    row_evidence="no classifier has evaluated this row yet"
    row_flags="-"

    # ── class A: gate_closure ────────────────────────────────────────────────
    # blocked ∧ task_kind=deterministic ∧ always_escalates truthy. SQLite
    # stores the JSON `true` as 1, so test for either spelling.
    if [ "$status" = "blocked" ] \
       && [ "$(_meta "$task_id" '$.task_kind')" = "deterministic" ]; then
        _always="$(_meta "$task_id" '$.always_escalates')"
        case "$_always" in
            1|true)
                row_class="gate_closure"
                _esc_state "$task_id"
                case "$_ESC_STATE" in
                    gated)
                        row_verdict="GATED"
                        row_action="none"
                        row_evidence="a live status=pending escalation still gates this task" ;;
                    clear)
                        row_verdict="STALE"
                        # `close`, not `redispatch`: per #5316 §5 the correct
                        # closure for a satisfied deterministic gate is a
                        # transition to `cancelled`, not a re-run.
                        row_action="close"
                        row_evidence="no live pending escalation remains in $ESCALATIONS (found ${_ESC_FOUND:-0} terminal file(s))" ;;
                    *)
                        row_verdict="unknown"
                        row_action="none"
                        row_evidence="escalation state could not be read; not upgraded to STALE" ;;
                esac ;;
        esac
    fi

    # ── class B: merge_verify_red ────────────────────────────────────────────
    # Unlike classes A and C this one spans `blocked` AND `in-progress`: a
    # merge-verify red can strand a row in either state. It is still gated by
    # the L1 liveness guard above. Evaluated only when no earlier class
    # matched (the precedence chain is formalized in a later step).
    if [ "$row_class" = "-" ]; then
        _classify_merge_verify_red "$task_id"
        if [ "$_MVR_MATCHED" = 1 ]; then
            row_class="merge_verify_red"
            row_verdict="$_MVR_VERDICT"
            row_action="$_MVR_ACTION"
            row_evidence="$_MVR_EVIDENCE"
        fi
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
        unknown) N_UNKNOWN=$((N_UNKNOWN + 1)) ;;
    esac
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
    printf 'SWEEP: candidates=%d gate_closure=%d merge_verify_red=%d unmet_dependency=%d corrupt_hold=%d live_skipped=%d unknown=%d\n' \
        "$N_CANDIDATES" "$N_GATE_CLOSURE" "$N_MERGE_VERIFY_RED" "$N_UNMET_DEPENDENCY" \
        "$N_CORRUPT_HOLD" "$N_LIVE_SKIPPED" "$N_UNKNOWN"
fi

exit 0
