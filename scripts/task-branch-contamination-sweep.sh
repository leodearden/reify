#!/usr/bin/env bash
# scripts/task-branch-contamination-sweep.sh — Read-only task-branch
# provenance audit: which in-flight branches carry work that is not theirs.
#
# The defect this exists to surface (esc-6205-4): a task branch is cut with
# `git worktree add -b task/N <lane> <base>`, and <base> is occasionally not a
# commit on main but ANOTHER task's branch tip. task/N then silently contains
# every one of that peer's commits, and merging it lands the peer's unreviewed
# work under this task's name.
#
# Cross-repo seam primitive: reify ships this classifier; dark-factory wires
# the invocations — a pre-merge advisory consult (`--task <id>`) and a
# timer-driven pool sweep (`--audit`). Same two-mode shape as
# scripts/warm-lane-degenerate-ref-check.sh, deliberately, so both seams are
# one artifact.
#
# ── Usage — two mutually exclusive modes, exactly one required ────────────────
#
#   1) Single branch (the per-merge advisory consult):
#        scripts/task-branch-contamination-sweep.sh --task <id> [OPTIONS]
#
#   2) Fleet sweep:
#        scripts/task-branch-contamination-sweep.sh --audit [OPTIONS]
#
# Options (each with an env default):
#   --task <id>            Audit refs/heads/<prefix><id> only (numeric).
#                          Mutually exclusive with --audit.
#   --audit                Sweep every refs/heads/<prefix>* ref whose backing
#                          task is non-terminal. Mutually exclusive with --task.
#   --db PATH              Taskmaster store. $REIFY_LANE_TASK_DB, default
#                          /home/leo/src/reify/.taskmaster/tasks/tasks.db.
#                          Same plumbing contract as
#                          scripts/lane-task-status.sh, so the repo has ONE
#                          task-DB default rather than two that can drift.
#   --tag TAG              Taskmaster tag namespace. $REIFY_LANE_TASK_TAG,
#                          default master.
#   --repo DIR, -C DIR     Repo/worktree to read (default: CWD).
#   --main-ref REF         Ref the branches are compared against (default: main).
#   --branch-prefix PFX    Branch-name prefix (default: "task/").
#   --format table|json    Output format (default: table).
#   -h, --help             Print usage to stderr and exit 0.
#
# There is deliberately NO staleness-threshold knob. See invariant R5.
#
# ── Report ───────────────────────────────────────────────────────────────────
# One row per audited branch on stdout, `key=value` pairs in this field order:
#
#   task status merge_base behind commits peer_commits changed foreign
#   peer_files peers scope signature
#
#   task         the task id (the branch's numeric suffix)
#   status       its Taskmaster status (non-terminal by construction)
#   merge_base   abbreviated `git merge-base <branch> <main-ref>`
#   behind       commits on <main-ref> since merge_base — CONTEXT ONLY (R5)
#   commits      commits in <main-ref>..<branch>
#   peer_commits how many of those cite a non-terminal task that is NOT this one
#   changed      files in `git diff --name-only <merge_base> <branch>`
#   foreign      changed files absent from this task's metadata.files
#   peer_files   foreign files declared by a non-terminal task that is NOT this one
#   peers        the union of peer task ids implicated, sorted-unique, or "-"
#   scope        CLEAN | OUT-OF-SCOPE | PEER-FILES | UNDECLARED | UNKNOWN
#   signature    SUSPECT | -
#
# Fleet mode adds a trailing `SWEEP:` line whose first six counters PARTITION
# the row count:
#   branches = suspect + peer_files + out_of_scope + undeclared + clean + unknown
# plus the cross-cutting skip counters skipped_terminal / skipped_nonnumeric.
#
# `--format json` emits one document: a `branches` array of objects carrying
# the same keys, and a sibling `summary` object with the same counters.
#
# ── Invariants ───────────────────────────────────────────────────────────────
#   R1  Read-only on the task store. Opened strictly -readonly / mode=ro, and
#       never created — pointing --db at a nonexistent path leaves it absent.
#   R2  Read-only on the repo. No ref, worktree, index or config write.
#   R3  Non-gating. Exit 0 on EVERY valid invocation in BOTH modes; 2 only for
#       a usage error. This is a deliberate divergence from
#       warm-lane-degenerate-ref-check.sh, which uses exit codes as a
#       classification channel: a non-zero exit is exactly what a merge worker
#       would gate on, and v1 is report-only. Stdout is the only result channel.
#   R4  Fail-safe degradation. An unreadable store, an unresolvable ref, a
#       failed diff or a failed SQL engine degrades the affected row (or the
#       whole report) to UNKNOWN with a stderr warning — never an abort, never
#       a changed exit code.
#   R5  `behind` is CONTEXT, never a trigger. Measured over the 351 live task
#       branches: median 2201 commits behind main (p25 878, p75 3781, p90
#       5324), and 345 of 351 are >= 50 behind. A staleness-triggered verdict
#       would therefore fire on ~98% of the pool and discriminate nothing, so
#       no threshold knob exists to be mis-tuned.
#
# See: docs/notes/task-branch-contamination-sweep.md

set -euo pipefail

# ── the citation grammar (sourced, never re-inlined) ─────────────────────────
# scripts/lib_task_citation.sh is the SINGLE copy of the "cites task N"
# grammar, shared with scripts/warm-lane-degenerate-ref-check.sh. Do not
# re-inline either ERE here; tests/infra/test_lib_task_citation.sh fails if
# this file grows its own copy.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/lib_task_citation.sh
source "$SCRIPT_DIR/lib_task_citation.sh"

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") --task <id> [OPTIONS]
   or: $(basename "$0") --audit [OPTIONS]

  Read-only task-branch provenance audit: which in-flight branches carry
  commits or files that belong to another task. See the script header for the
  full contract.

  Options:
    --task <id>            Audit refs/heads/<prefix><id> only (numeric).
    --audit                Sweep every non-terminal-backed refs/heads/<prefix>*.
    --db PATH              Taskmaster store (\$REIFY_LANE_TASK_DB).
    --tag TAG              Taskmaster tag namespace (\$REIFY_LANE_TASK_TAG).
    --repo DIR, -C DIR     Repo/worktree to read (default: CWD).
    --main-ref REF         Ref to compare against (default: main).
    --branch-prefix PFX    Branch-name prefix (default: "task/").
    --format table|json    Output format (default: table).
    -h, --help             Print this message and exit 0.

  Exit codes: 0 = report produced (ALWAYS, in both modes); 2 = usage error.
  This script never gates: stdout is its only result channel.

  See: docs/notes/task-branch-contamination-sweep.md
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
TASK_ID=""
TASK_MODE=0
AUDIT_MODE=0
DB="${REIFY_LANE_TASK_DB:-/home/leo/src/reify/.taskmaster/tasks/tasks.db}"
TAG="${REIFY_LANE_TASK_TAG:-master}"
REPO_DIR=""
MAIN_REF="main"
BRANCH_PREFIX="task/"
FORMAT="table"

# TASK_MODE is tracked separately from TASK_ID because `--task ''` must be a
# usage error, not an absent mode: testing -n "$TASK_ID" alone would report
# "neither --task nor --audit" and hide the real fault.
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --task)
            [ $# -ge 2 ] || { err "--task requires a value"; exit 2; }
            TASK_ID="$2"; TASK_MODE=1; shift 2 ;;
        --audit)
            AUDIT_MODE=1; shift ;;
        --db)
            [ $# -ge 2 ] || { err "--db requires a value"; exit 2; }
            DB="$2"; shift 2 ;;
        --tag)
            [ $# -ge 2 ] || { err "--tag requires a value"; exit 2; }
            TAG="$2"; shift 2 ;;
        --repo|-C)
            [ $# -ge 2 ] || { err "--repo requires a value"; exit 2; }
            REPO_DIR="$2"; shift 2 ;;
        --main-ref)
            [ $# -ge 2 ] || { err "--main-ref requires a value"; exit 2; }
            MAIN_REF="$2"; shift 2 ;;
        --branch-prefix)
            [ $# -ge 2 ] || { err "--branch-prefix requires a value"; exit 2; }
            BRANCH_PREFIX="$2"; shift 2 ;;
        --format)
            [ $# -ge 2 ] || { err "--format requires a value"; exit 2; }
            FORMAT="$2"; shift 2 ;;
        -*)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
        *)
            err "Unexpected positional argument: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── validation ────────────────────────────────────────────────────────────────
if [ "$TASK_MODE" -eq 1 ] && [ "$AUDIT_MODE" -eq 1 ]; then
    err "--task and --audit are mutually exclusive"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi
if [ "$TASK_MODE" -ne 1 ] && [ "$AUDIT_MODE" -ne 1 ]; then
    err "Exactly one of --task <id> or --audit is required"
    _usage
    exit 2
fi
if [ "$TASK_MODE" -eq 1 ] && ! printf '%s\n' "$TASK_ID" | grep -qE '^[0-9]+$'; then
    err "--task must be a positive integer (got: '$TASK_ID')"
    exit 2
fi
case "$FORMAT" in
    table|json) ;;
    *)
        err "--format must be 'table' or 'json' (got: '$FORMAT')"
        exit 2 ;;
esac

[ -n "$REPO_DIR" ] || REPO_DIR="."

# ── field separators ──────────────────────────────────────────────────────────
# ASCII US (0x1f) between columns, deliberately NOT a tab: `read` treats runs of
# IFS *whitespace* as ONE delimiter and strips leading/trailing ones, so with a
# tab an EMPTY column silently collapses and every later column shifts left. A
# task with no declared files is exactly that empty column, and it is the
# common case (186 of 1345 non-terminal tasks), so this is not hypothetical.
# TAB separates the declared paths within their one column — a repo-relative
# path cannot contain a tab, but it CAN contain a space or a '#'.
_FS=$'\x1f'
_LS=$'\t'

# ── SQL engine resolution ─────────────────────────────────────────────────────
# Dual-engine, both strictly read-only, mirroring scripts/lane-task-status.sh
# and scripts/deterministic-gate-closure-staleness-sweep.sh: the sqlite3 CLI at
# an absolute path (so this never depends on which sqlite3 is first on PATH)
# with a PATH fallback, then python3's stdlib with a `file:...?mode=ro` URI.
#
# REIFY_TASK_BRANCH_SWEEP_SQLITE_BIN overrides the probe entirely. Because the
# probe uses an ABSOLUTE path, stripping PATH cannot reach it, so this is the
# only way to exercise (or break-glass onto) the python3 engine. An explicitly
# EMPTY value is meaningful — it forces python3 — so "set" is distinguished
# from "non-empty".
if [ -n "${REIFY_TASK_BRANCH_SWEEP_SQLITE_BIN+set}" ]; then
    _SQLITE_BIN="$REIFY_TASK_BRANCH_SWEEP_SQLITE_BIN"
else
    _SQLITE_BIN=""
    for _c in /usr/bin/sqlite3 sqlite3; do
        if command -v "$_c" >/dev/null 2>&1; then _SQLITE_BIN="$_c"; break; fi
    done
fi
_PYTHON_BIN=""
if command -v python3 >/dev/null 2>&1; then _PYTHON_BIN="python3"; fi

if [ -z "$_SQLITE_BIN" ] && [ -z "$_PYTHON_BIN" ]; then
    warn "Neither sqlite3 nor python3 is on PATH — the task store cannot be read at all; reporting zero branches."
fi

# ── the enumeration query ─────────────────────────────────────────────────────
# ONE tag-scoped query per INVOCATION, not one per branch. Measured on the live
# store: 1345 rows in 0.38s, versus 1m43s for the per-task-oracle shape over
# 1095 refs. Every branch therefore costs zero further store opens.
#
# Three things the query must not do:
#   * `json_each` RAISES on malformed JSON, and a raise in a whole-table query
#     kills EVERY row, not one. metadata is read through a `json_valid` guard
#     that substitutes '{}', so a corrupt blob yields an empty declaration and
#     the rest of the sweep still reports.
#   * a declared path is free-form text and could in principle carry a newline
#     or one of the separators. Every such value is flattened, so no stored
#     value can forge a field or a row boundary in the stream below.
#   * every clause is tag-scoped: `tasks` is PRIMARY KEY (tag, id), so an
#     unqualified lookup silently conflates tags the moment a second exists.
#
# Terminal statuses are excluded HERE rather than filtered later, because
# "non-terminal" is exactly what both derived maps mean: a peer that has
# already landed or been cancelled does not own a file or a commit any more.
_TAG_SQL="${TAG//\'/\'\'}"
_MD_SQL="CASE WHEN json_valid(t.metadata) THEN t.metadata ELSE '{}' END"
_ENUM_SQL="SELECT
    t.id,
    coalesce(t.status,''),
    coalesce((SELECT group_concat(
                replace(replace(replace(value,char(10),' '),char(31),' '),char(9),' '),
                char(9))
                FROM json_each($_MD_SQL,'\$.files')),'')
  FROM tasks t
 WHERE t.tag='$_TAG_SQL' AND t.status NOT IN ('done','cancelled')
 ORDER BY t.id;"

# _DB_READABLE distinguishes "the query ran and returned nothing" from "the
# query never ran". Both produce the same report — zero branches — but only the
# former is a statement about the pool. The engines' EXIT STATUS is the oracle,
# so this costs no extra process.
_TASK_ROWS=""
_DB_READABLE=0
if [ ! -s "$DB" ]; then
    warn "Task store is missing or empty: $DB — reporting zero branches."
else
    if [ -n "$_SQLITE_BIN" ]; then
        if _TASK_ROWS="$("$_SQLITE_BIN" -readonly -separator "$_FS" "$DB" "$_ENUM_SQL" 2>/dev/null)"; then
            _DB_READABLE=1
        else
            _TASK_ROWS=""
        fi
    fi
    if [ "$_DB_READABLE" = 0 ] && [ -n "$_PYTHON_BIN" ]; then
        # The SAME SQL string, verbatim, through the other engine — that is what
        # makes the two interchangeable rather than one a stub.
        if _TASK_ROWS="$(_TB_DB="$DB" _TB_SQL="$_ENUM_SQL" python3 - <<'PY' 2>/dev/null
import os, sqlite3, sys
try:
    con = sqlite3.connect(f"file:{os.environ['_TB_DB']}?mode=ro", uri=True, timeout=5.0)
    rows = con.execute(os.environ["_TB_SQL"]).fetchall()
    con.close()
    for r in rows:
        sys.stdout.write("\x1f".join("" if c is None else str(c) for c in r) + "\n")
except Exception:
    # Exit non-zero rather than swallowing: the caller must be able to tell a
    # FAILED read from an empty one.
    sys.exit(1)
PY
)"; then
            _DB_READABLE=1
        else
            _TASK_ROWS=""
        fi
    fi
    if [ "$_DB_READABLE" = 0 ]; then
        warn "Task store could not be read: $DB — reporting zero branches."
    fi
fi

# ── the two derived maps ──────────────────────────────────────────────────────
# Bash associative arrays, both keyed by a value that cannot collide:
#   _STATUS[id]        -> the task's non-terminal status
#   _DECLARED[id]      -> its declared paths, TAB-separated (empty = undeclared)
#   _PEER_OWNER[path]  -> space-separated non-terminal ids declaring <path>
# _PEER_OWNER is derived from non-terminal rows ONLY, because "owned by a task
# whose status is still non-terminal" is precisely what the peer_files flag
# means — a done/cancelled declarer is not a peer.
declare -A _STATUS=()
declare -A _DECLARED=()
declare -A _PEER_OWNER=()

while IFS="$_FS" read -r _id _st _files; do
    [ -n "${_id:-}" ] || continue
    _STATUS["$_id"]="$_st"
    _DECLARED["$_id"]="${_files:-}"
    [ -n "${_files:-}" ] || continue
    while IFS= read -r _path; do
        [ -n "$_path" ] || continue
        _PEER_OWNER["$_path"]="${_PEER_OWNER["$_path"]:+${_PEER_OWNER["$_path"]} }$_id"
    done <<< "${_files//$_LS/$'\n'}"
done <<< "$_TASK_ROWS"
unset _id _st _files _path

# ── repo preflight ────────────────────────────────────────────────────────────
# A non-git --repo or an unresolvable --main-ref is NOT fatal (R3/R4): every
# branch degrades to UNKNOWN and the report is still produced. Resolved once
# here rather than per branch.
_MAIN_SHA=""
if ! git -C "$REPO_DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    warn "Not inside a git work tree: $REPO_DIR — every branch will report scope=UNKNOWN."
else
    _MAIN_SHA="$(git -C "$REPO_DIR" rev-parse --verify "$MAIN_REF" 2>/dev/null || true)"
    [ -n "$_MAIN_SHA" ] || \
        warn "Cannot resolve --main-ref '$MAIN_REF' in $REPO_DIR — every branch will report scope=UNKNOWN."
fi

_BRANCH_PREFIX_RE="$(task_citation_regex_escape "$BRANCH_PREFIX")"

# ── per-branch measurement ────────────────────────────────────────────────────
# Row fields, in the order the header documents. Set by _measure_branch and
# consumed by the emitters; declared here so the field list has ONE definition.
R_TASK=""; R_STATUS=""; R_MERGE_BASE=""; R_BEHIND=""; R_COMMITS=""
R_PEER_COMMITS=""; R_CHANGED=""; R_FOREIGN=""; R_PEER_FILES=""; R_PEERS=""
R_SCOPE=""; R_SIGNATURE=""
# The changed-path list behind R_CHANGED. Not a report column (the report
# carries counts), but the input the scope verdict consumes.
_CHANGED_FILES=""

# _row_unknown <id> — the degraded row. Every failure path funnels here, so
# "we could not measure this branch" has exactly one shape (R4).
#
# _measure_branch calls this FIRST and only overwrites on success, which is
# what makes every early return safe. That also makes resetting _CHANGED_FILES
# here load-bearing in fleet mode: without it a degraded branch would inherit
# the previous branch's changed set.
_row_unknown() {
    R_TASK="$1"
    R_STATUS="${_STATUS["$1"]:-unknown}"
    R_MERGE_BASE="-"; R_BEHIND="-"; R_COMMITS="-"; R_PEER_COMMITS="-"
    R_CHANGED="-"; R_FOREIGN="-"; R_PEER_FILES="-"; R_PEERS="-"
    R_SCOPE="UNKNOWN"; R_SIGNATURE="-"
    _CHANGED_FILES=""
}

# _measure_branch <id>
# Populates the R_* fields for refs/heads/<prefix><id>. Returns 0 always: an
# unmeasurable branch is a reported outcome, not a script error.
#
# Exactly four git invocations per branch, all read-only. `git diff` takes the
# two-dot form against the ALREADY-RESOLVED merge base rather than the
# three-dot form against main, so the merge base is computed once, not twice.
_measure_branch() {
    local id="$1" ref tip
    _row_unknown "$id"

    [ -n "$_MAIN_SHA" ] || return 0

    ref="refs/heads/${BRANCH_PREFIX}${id}"
    tip="$(git -C "$REPO_DIR" rev-parse --verify "$ref" 2>/dev/null || true)"
    if [ -z "$tip" ]; then
        warn "No such branch: $ref — reporting scope=UNKNOWN for task $id."
        return 0
    fi

    local mb behind commits changed
    mb="$(git -C "$REPO_DIR" merge-base "$tip" "$_MAIN_SHA" 2>/dev/null || true)"
    if [ -z "$mb" ]; then
        warn "No merge base between $ref and '$MAIN_REF' — reporting scope=UNKNOWN for task $id."
        return 0
    fi
    behind="$(git -C "$REPO_DIR" rev-list --count "${mb}..${_MAIN_SHA}" 2>/dev/null || true)"
    commits="$(git -C "$REPO_DIR" rev-list --count "${_MAIN_SHA}..${tip}" 2>/dev/null || true)"
    if ! changed="$(git -C "$REPO_DIR" diff --name-only "$mb" "$tip" 2>/dev/null)"; then
        warn "Cannot diff ${mb}..${tip} — reporting scope=UNKNOWN for task $id."
        return 0
    fi
    if [ -z "$behind" ] || [ -z "$commits" ]; then
        warn "Cannot count revisions for $ref — reporting scope=UNKNOWN for task $id."
        return 0
    fi

    R_MERGE_BASE="$(git -C "$REPO_DIR" rev-parse --short "$mb" 2>/dev/null || printf '%s' "$mb")"
    R_BEHIND="$behind"
    R_COMMITS="$commits"
    R_CHANGED="$(printf '%s' "$changed" | grep -c . || true)"
    # Filled by the scope verdict (step-12) and the citation census (step-14).
    R_PEER_COMMITS=0; R_FOREIGN=0; R_PEER_FILES=0; R_PEERS="-"; R_SCOPE="CLEAN"
    R_SIGNATURE="-"
    _CHANGED_FILES="$changed"
    return 0
}

# ── emit ──────────────────────────────────────────────────────────────────────
# ONE definition of the row's field order, shared by both output formats, so
# the two cannot drift apart.
_emit_row_table() {
    printf 'task=%s status=%s merge_base=%s behind=%s commits=%s peer_commits=%s changed=%s foreign=%s peer_files=%s peers=%s scope=%s signature=%s\n' \
        "$R_TASK" "$R_STATUS" "$R_MERGE_BASE" "$R_BEHIND" "$R_COMMITS" \
        "$R_PEER_COMMITS" "$R_CHANGED" "$R_FOREIGN" "$R_PEER_FILES" \
        "$R_PEERS" "$R_SCOPE" "$R_SIGNATURE"
}

# ── single-branch mode ────────────────────────────────────────────────────────
# --task names one branch explicitly, so it always gets a row — including the
# degraded one. That is the deliberate asymmetry with fleet mode, which drops a
# branchless task silently: here the caller asked about this branch by name and
# is owed an answer.
if [ "$TASK_MODE" -eq 1 ]; then
    _measure_branch "$TASK_ID"
    _emit_row_table
    exit 0
fi
