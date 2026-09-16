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
#   changed      files in `git diff -z --name-only <merge_base> <branch>`
#   foreign      changed files absent from this task's metadata.files
#   peer_files   foreign files declared by a non-terminal task that is NOT this one
#   peers        the union of peer task ids implicated, sorted-unique, or "-"
#   scope        CLEAN | OUT-OF-SCOPE | PEER-FILES | UNDECLARED | UNKNOWN
#   signature    SUSPECT | -
#
# Fleet mode adds a trailing `SWEEP:` line whose first six counters PARTITION
# the row count:
#   branches = suspect + peer_files + out_of_scope + undeclared + clean + unknown
# plus three skip counters for the refs that yielded no row at all, each naming
# the REASON the store gave rather than a guess at it:
#   skipped_terminal    the store shows the backing task done or cancelled
#   skipped_no_task     the store was read and does not carry that id under
#                       --tag at all (a four-digit count here is the signature
#                       of a mistyped --tag, not of a finished pool)
#   skipped_nonnumeric  the ref's suffix is not a task id
# A ref is never skipped for want of an answer: when the store could not be
# read at all, EVERY ref is measured and reported scope=UNKNOWN instead (R4).
#
# One last counter is not a count but a flag, and it is the only case where a
# fleet report has no rows to degrade:
#   repo_unusable       1 when --repo is not a git work tree. There is then no
#                       ref list to enumerate, so `branches=0` means the row
#                       set is UNDEFINED, not empty. Zero on every healthy run,
#                       so an all-zero summary still reads as a clean pool.
#
# `--format json` emits one document: a `branches` array of objects carrying
# the same keys, and a sibling `summary` object with the same counters.
#
# ── Invariants ───────────────────────────────────────────────────────────────
#   R1  Read-only on the task store. Opened strictly -readonly / mode=ro, and
#       never created — pointing --db at a nonexistent path leaves it absent.
#   R2  Read-only on the repo. Every git call goes through ONE wrapper that
#       sets GIT_OPTIONAL_LOCKS=0, so no read can take a lock or refresh the
#       index as a side effect: no ref, worktree, index or config write, and no
#       file created anywhere beneath it. The only files written at all are two
#       mktemp temporaries under $TMPDIR, removed by the EXIT trap.
#   R3  Non-gating. Exit 0 on EVERY valid invocation in BOTH modes, whatever it
#       finds — no classification ever reaches the exit status. The only
#       non-zero exit is 2: a usage error, or `--format json` on a host with no
#       python3, which is refused UP FRONT (before the store read and before
#       any measurement) so a caller never receives a partial report. This is a
#       deliberate divergence from warm-lane-degenerate-ref-check.sh, which
#       uses exit codes as a classification channel: a non-zero exit is exactly
#       what a merge worker would gate on, and v1 is report-only. Stdout is the
#       only result channel, and --format is binding in BOTH modes — a --task
#       consult that asks for json gets json, never a table row.
#   R4  Fail-safe degradation. An unreadable store, an id absent from the
#       tag's non-terminal set, an unresolvable ref, a failed diff or a failed
#       SQL engine degrades the affected row (or the whole report) to UNKNOWN
#       with a stderr warning — never an abort, never a changed exit code.
#       Degradation is decided BEFORE measurement, so a degraded row carries
#       "-" in every column and can never be read as a benign verdict.
#       Fleet mode's "emit no row at all" is NOT a degradation channel for any
#       ref it can see: a ref is dropped only on the store's POSITIVE evidence
#       about it (terminal, absent, non-numeric — each with its own counter),
#       and a store that answered nothing yields a full report of UNKNOWN rows
#       rather than a short one that reads as an all-clean pool.
#       The ONE degradation that cannot take that shape is a non-git --repo:
#       with no work tree there is no ref list, so there is no row to degrade
#       and inventing one would be inventing data. That case is therefore
#       reported as the whole-report `repo_unusable=1` token rather than as a
#       row — still on stdout, still never an exit code, so R3 is untouched and
#       `branches=0` is never mistakable for a measured empty pool.
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

# ── the git read wrapper (R2) ─────────────────────────────────────────────────
# EVERY git invocation in this script goes through here, which is what makes R2
# structural rather than incidental. GIT_OPTIONAL_LOCKS=0 forbids git from
# taking a lock or refreshing the index as a side effect of a read, so no call
# can write .git/index even if a future one is added that otherwise would, and
# a concurrent agent in the same worktree is never blocked by this audit.
_git() { GIT_OPTIONAL_LOCKS=0 git -C "$REPO_DIR" "$@"; }

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
    warn "Neither sqlite3 nor python3 is on PATH — the task store cannot be read at all; every branch will report scope=UNKNOWN."
fi

# A json report is rendered by a python3 pass rather than a hand-rolled
# escaper, so on a host without python3 that request cannot be honoured at all.
# Refused HERE — before the store read and before any measurement — so a caller
# never receives a partial report, and never a silently-downgraded table one
# under a flag that asked for json. This and a usage error are the ONLY
# non-zero exits the script can produce (R3); no classification ever reaches
# the exit status.
if [ "$FORMAT" = "json" ] && [ -z "$_PYTHON_BIN" ]; then
    err "--format json needs python3, which is not on PATH."
    exit 2
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
# EVERY task in the tag is enumerated, not just the non-terminal ones, because
# the fleet loop has three outcomes to tell apart and only the store can: an id
# it shows as live (audit the branch), an id it shows as done/cancelled
# (skipped_terminal), and an id it does not know AT ALL (skipped_no_task — the
# signature of a mistyped --tag, which a non-terminal-only enumeration would
# launder into a benign-looking all-terminal sweep).
#
# The two derived maps below stay non-terminal-only, because "non-terminal" is
# exactly what they mean: a peer that has already landed or been cancelled does
# not own a file or a commit any more. So the group_concat — the only expensive
# clause — is still computed for live rows ONLY, and a terminal row costs one
# id and one status.
_TAG_SQL="${TAG//\'/\'\'}"
_MD_SQL="CASE WHEN json_valid(t.metadata) THEN t.metadata ELSE '{}' END"
_ENUM_SQL="SELECT
    t.id,
    coalesce(t.status,''),
    CASE WHEN t.status IN ('done','cancelled') THEN ''
         ELSE coalesce((SELECT group_concat(
                replace(replace(replace(value,char(10),' '),char(31),' '),char(9),' '),
                char(9))
                FROM json_each($_MD_SQL,'\$.files')),'') END
  FROM tasks t
 WHERE t.tag='$_TAG_SQL'
 ORDER BY t.id;"

# _DB_READABLE distinguishes "the query ran and returned nothing" from "the
# query never ran". Both produce the same report — zero branches — but only the
# former is a statement about the pool. The engines' EXIT STATUS is the oracle,
# so this costs no extra process.
_TASK_ROWS=""
_DB_READABLE=0
if [ ! -s "$DB" ]; then
    warn "Task store is missing or empty: $DB — every branch will report scope=UNKNOWN."
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
        if _TASK_ROWS="$(_TB_DB="$DB" _TB_SQL="$_ENUM_SQL" "$_PYTHON_BIN" - <<'PY' 2>/dev/null
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
        warn "Task store could not be read: $DB — every branch will report scope=UNKNOWN."
    fi
fi

# ── the two derived maps ──────────────────────────────────────────────────────
# Bash associative arrays, both keyed by a value that cannot collide:
#   _STATUS[id]        -> the task's non-terminal status
#   _TERMINAL[id]      -> set for an id the store shows as done/cancelled
#   _DECLARED[id]      -> its declared paths, TAB-separated (empty = undeclared)
#   _PEER_OWNER[path]  -> space-separated non-terminal ids declaring <path>
# _PEER_OWNER is derived from non-terminal rows ONLY, because "owned by a task
# whose status is still non-terminal" is precisely what the peer_files flag
# means — a done/cancelled declarer is not a peer. _TERMINAL is the store's
# POSITIVE evidence of termination, and it is what keeps "the store says this
# branch's task is finished" distinct from "the store has never heard of this
# id": absence from _STATUS alone cannot tell those apart.
declare -A _STATUS=()
declare -A _TERMINAL=()
declare -A _DECLARED=()
declare -A _PEER_OWNER=()

while IFS="$_FS" read -r _id _st _files; do
    [ -n "${_id:-}" ] || continue
    case "$_st" in
        done|cancelled) _TERMINAL["$_id"]=1; continue ;;
    esac
    _STATUS["$_id"]="$_st"
    _DECLARED["$_id"]="${_files:-}"
    [ -n "${_files:-}" ] || continue
    while IFS= read -r _path; do
        [ -n "$_path" ] || continue
        _PEER_OWNER["$_path"]="${_PEER_OWNER["$_path"]:+${_PEER_OWNER["$_path"]} }$_id"
    done <<< "${_files//$_LS/$'\n'}"
done <<< "$_TASK_ROWS"
unset _id _st _files _path

# _is_live <id> — is <id> in the store's non-terminal set? MEMBERSHIP, not
# truthiness: a task row whose status column is empty is still a live task, and
# all three sites that ask this question (the fleet loop's triage, the
# per-branch gate, the peer-citation filter) must answer it identically.
_is_live() { [ -n "${_STATUS["$1"]+set}" ]; }

# ── repo preflight ────────────────────────────────────────────────────────────
# Neither a non-git --repo nor an unresolvable --main-ref is fatal (R3/R4), but
# they degrade DIFFERENTLY, and conflating them is what let the non-git case
# report a benign-looking empty fleet:
#
#   unresolvable --main-ref  the refs still enumerate, so every branch gets its
#                            own UNKNOWN row. Nothing further is needed.
#   non-git --repo           there is no ref list to enumerate, so fleet mode
#                            has no row to degrade — an honest report cannot
#                            invent one. `branches=0` would then be
#                            indistinguishable from a pool with no task
#                            branches at all, so the summary carries
#                            repo_unusable=1 instead: the ONE stdout token that
#                            says the row set is undefined rather than empty.
#
# Resolved once here rather than per branch.
_MAIN_SHA=""
_REPO_UNUSABLE=0
if ! _git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    _REPO_UNUSABLE=1
    warn "Not inside a git work tree: $REPO_DIR — no branch can be measured here."
else
    _MAIN_SHA="$(_git rev-parse --verify "$MAIN_REF" 2>/dev/null || true)"
    [ -n "$_MAIN_SHA" ] || \
        warn "Cannot resolve --main-ref '$MAIN_REF' in $REPO_DIR — every branch will report scope=UNKNOWN."
fi

_BRANCH_PREFIX_RE="$(task_citation_regex_escape "$BRANCH_PREFIX")"

# ── per-branch measurement ────────────────────────────────────────────────────
# Row fields, in the order the header documents. Set by _measure_branch and
# consumed by the emitters; declared here so the field list has ONE definition.
_DB_WARNED=0
R_TASK=""; R_STATUS=""; R_MERGE_BASE=""; R_BEHIND=""; R_COMMITS=""
R_PEER_COMMITS=""; R_CHANGED=""; R_FOREIGN=""; R_PEER_FILES=""; R_PEERS=""
R_SCOPE=""; R_SIGNATURE=""

# The changed-path list behind R_CHANGED — not a report column (the report
# carries counts), but the input the scope verdict consumes.
#
# It lives in a FILE, NUL-separated, rather than in a variable, because the
# only faithful way to read paths out of git is `diff -z`: plain
# `--name-only` C-QUOTES any path containing a double quote, a backslash, a
# control character or a non-ASCII byte, wrapping it in quotes and escaping
# the contents. The task store holds the RAW path, so a quoted path never
# compares equal and every UTF-8 filename would be reported foreign. A bash
# variable cannot hold the NUL bytes `-z` emits, hence the file.
_CHANGED_FILE="$(mktemp "${TMPDIR:-/tmp}/task-branch-sweep-changed-XXXXXX")"
_ROWS=""
_cleanup() { rm -f "$_CHANGED_FILE" ${_ROWS:+"$_ROWS"}; }
trap _cleanup EXIT

# _row_unknown <id> — the degraded row. Every failure path funnels here, so
# "we could not measure this branch" has exactly one shape (R4).
#
# _measure_branch calls this FIRST and only overwrites on success, which is
# what makes every early return safe. That also makes truncating
# _CHANGED_FILE here load-bearing in fleet mode: without it a degraded branch
# would inherit the previous branch's changed set.
_row_unknown() {
    R_TASK="$1"
    R_STATUS="${_STATUS["$1"]:-unknown}"
    R_MERGE_BASE="-"; R_BEHIND="-"; R_COMMITS="-"; R_PEER_COMMITS="-"
    R_CHANGED="-"; R_FOREIGN="-"; R_PEER_FILES="-"; R_PEERS="-"
    R_SCOPE="UNKNOWN"; R_SIGNATURE="-"
    : > "$_CHANGED_FILE"
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

    # R4's store half, returning BEFORE any measurement. Without it a --task
    # consult whose store never loaded still measures the branch and reports a
    # POSITIVELY BENIGN verdict — _classify_scope finds no declaration and says
    # UNDECLARED, _census_commits rejects every citation for want of a peer
    # status — so a typo'd --db or a permission change turns a SUSPECT branch
    # clean on stdout, the caller's only result channel (R3).
    #
    # Sited at this ONE funnel rather than on the --task branch, so "a failure
    # leaves the row exactly as _row_unknown shaped it" stays a property of the
    # single entry point both modes call — fleet mode reaches it too, for every
    # ref it cannot resolve to a status, which is the whole pool when the store
    # never loaded. The warning is therefore emitted ONCE per run rather than
    # once per branch: the condition is a property of the invocation, not of
    # the branch, and 1095 copies of it would bury the rest of stderr.
    if [ "$_DB_READABLE" = 0 ]; then
        if [ "$_DB_WARNED" -eq 0 ]; then
            _DB_WARNED=1
            warn "Task store was not read ($DB) — reporting scope=UNKNOWN for every branch."
        fi
        return 0
    fi
    if ! _is_live "$id"; then
        warn "Task $id is not in the non-terminal set for tag '$TAG' — reporting scope=UNKNOWN."
        return 0
    fi

    [ -n "$_MAIN_SHA" ] || return 0

    ref="refs/heads/${BRANCH_PREFIX}${id}"
    tip="$(_git rev-parse --verify "$ref" 2>/dev/null || true)"
    if [ -z "$tip" ]; then
        warn "No such branch: $ref — reporting scope=UNKNOWN for task $id."
        return 0
    fi

    local mb behind commits
    mb="$(_git merge-base "$tip" "$_MAIN_SHA" 2>/dev/null || true)"
    if [ -z "$mb" ]; then
        warn "No merge base between $ref and '$MAIN_REF' — reporting scope=UNKNOWN for task $id."
        return 0
    fi
    behind="$(_git rev-list --count "${mb}..${_MAIN_SHA}" 2>/dev/null || true)"
    commits="$(_git rev-list --count "${_MAIN_SHA}..${tip}" 2>/dev/null || true)"
    # -z: NUL-separated and NEVER quoted, so a path compares byte-for-byte
    # against the store's raw value. See _CHANGED_FILE's note.
    if ! _git diff -z --name-only "$mb" "$tip" > "$_CHANGED_FILE" 2>/dev/null; then
        warn "Cannot diff ${mb}..${tip} — reporting scope=UNKNOWN for task $id."
        : > "$_CHANGED_FILE"
        return 0
    fi
    if [ -z "$behind" ] || [ -z "$commits" ]; then
        warn "Cannot count revisions for $ref — reporting scope=UNKNOWN for task $id."
        return 0
    fi

    R_MERGE_BASE="$(_git rev-parse --short "$mb" 2>/dev/null || printf '%s' "$mb")"
    R_BEHIND="$behind"
    R_COMMITS="$commits"
    R_CHANGED="$(tr -cd '\0' < "$_CHANGED_FILE" | wc -c | tr -d '[:space:]')"
    _classify_scope "$id"
    _census_commits "$id" "$tip"
    return 0
}

# ── the commit-citation census ────────────────────────────────────────────────
# The SHARP signal. The mechanism esc-6205-4 describes is foreign COMMITS on a
# branch, so this measures commits directly rather than inferring them from
# files — closer to the fault and far less noisy than the file cross-check.
#
# A commit counts as a peer commit iff its message cites at least one id that
# is (a) non-terminal in the store, (b) not this task's own, and (c) the
# message does not ALSO cite this task's own id. Condition (c) is checked
# first and skips the whole commit: an amend that says "re-lands #N,
# coordinated with #M" is this task's own work referencing a sibling, not
# somebody else's commit riding along.
#
# ONE `git log` per branch, not one `git log -1` per commit: the pool has
# branches with tens of commits and the sweep runs over hundreds of branches.
# Messages are record-separated with US so a multi-line body cannot be read as
# several commits.
#
# `behind` is deliberately absent from this function. It is CONTEXT ONLY and
# never a trigger (R5): over the 351 live task branches the median is 2201
# commits behind main (p25 878, p75 3781, p90 5324) and 345 of 351 are >= 50
# behind, so any staleness threshold would fire on ~98% of the pool and
# separate nothing.
_census_commits() {
    local id="$1" tip="$2" log msg peer_id
    R_PEER_COMMITS=0

    log="$(_git log --format="%B%x1f" "${_MAIN_SHA}..${tip}" 2>/dev/null || true)"
    [ -n "$log" ] || return 0

    local commit_peers="" found
    while IFS= read -r -d $'\x1f' msg; do
        [ -n "${msg//[[:space:]]/}" ] || continue
        # (c) its own id anywhere in the message exempts the whole commit.
        if task_citation_message_cites "$msg" "$id" "$_BRANCH_PREFIX_RE"; then
            continue
        fi
        found=0
        while IFS= read -r peer_id; do
            [ -n "$peer_id" ] || continue
            [ "$peer_id" != "$id" ] || continue
            # (a) only a task that is still non-terminal is a peer.
            _is_live "$peer_id" || continue
            found=1
            commit_peers="$commit_peers$peer_id"$'\n'
        done < <(task_citation_peer_ids "$msg" "$_BRANCH_PREFIX_RE")
        [ "$found" -eq 0 ] || R_PEER_COMMITS=$((R_PEER_COMMITS + 1))
    done <<< "$log"

    [ "$R_PEER_COMMITS" -eq 0 ] || R_SIGNATURE="SUSPECT"

    # Merge the commit-derived ids into `peers` alongside the file-derived
    # ones, sorted-unique. Both halves feed ONE column because both answer the
    # same question: which other tasks are implicated in this branch.
    if [ -n "$commit_peers" ]; then
        [ "$R_PEERS" = "-" ] || commit_peers="$commit_peers${R_PEERS//,/$'\n'}"$'\n'
        R_PEERS="$(printf '%s' "$commit_peers" | sort -nu | paste -sd, -)"
    fi
    return 0
}

# ── the scope verdict ─────────────────────────────────────────────────────────
# Resolution order IS the invariant, and it is total — every branch lands in
# exactly one bucket:
#
#   1. UNKNOWN       the git measurement failed. Decided in _measure_branch,
#                    which returns before ever reaching here.
#   2. UNDECLARED    this task declares no files. NEVER downgraded to
#                    OUT-OF-SCOPE: 186 of the store's 1345 non-terminal tasks
#                    declare none (the documented defer-to-architect value), so
#                    treating "declares nothing" as "everything is foreign"
#                    would manufacture false positives at that scale.
#   3. PEER-FILES    some foreign path is declared by a NON-TERMINAL task that
#                    is not this one.
#   4. OUT-OF-SCOPE  the foreign set is non-empty but nobody non-terminal owns
#                    any of it.
#   5. CLEAN         the foreign set is empty.
#
# "Foreign" is the changed set minus the declared set under EXACT
# repo-relative string equality — no prefix or glob matching. That is a
# measurement, not a simplification: across all 1345 non-terminal tasks exactly
# one declared path is not a file-with-extension (`hooks/reference-transaction`,
# an extensionless FILE in lock-charter-guard's allowlist), so directory
# declarations do not exist to be handled and prefix machinery would be an
# unused dimension of variability.
_classify_scope() {
    local id="$1" declared path owner peers_found=""
    R_FOREIGN=0; R_PEER_FILES=0; R_PEERS="-"

    declared="${_DECLARED["$id"]:-}"
    if [ -z "$declared" ]; then
        R_SCOPE="UNDECLARED"
        return 0
    fi

    # An associative array keyed by the declared path gives exact-equality
    # membership directly; a substring or prefix test is what would wrongly
    # let a declared `x/a/b.rs` cover a changed `a/b.rs`.
    local -A own=()
    while IFS= read -r path; do
        [ -n "$path" ] || continue
        own["$path"]=1
    done <<< "${declared//$_LS/$'\n'}"

    while IFS= read -r -d '' path; do
        [ -n "$path" ] || continue
        [ -z "${own["$path"]:-}" ] || continue
        R_FOREIGN=$((R_FOREIGN + 1))
        # A path this task declares never reaches here, so its own id can
        # never enter peers — but a peer list still has to exclude it
        # defensively, because _PEER_OWNER holds every declarer.
        local claimed=0
        for owner in ${_PEER_OWNER["$path"]:-}; do
            [ "$owner" != "$id" ] || continue
            claimed=1
            peers_found="$peers_found$owner"$'\n'
        done
        [ "$claimed" -eq 0 ] || R_PEER_FILES=$((R_PEER_FILES + 1))
    done < "$_CHANGED_FILE"

    if [ -n "$peers_found" ]; then
        R_PEERS="$(printf '%s' "$peers_found" | sort -nu | paste -sd, -)"
        R_SCOPE="PEER-FILES"
    elif [ "$R_FOREIGN" -gt 0 ]; then
        R_SCOPE="OUT-OF-SCOPE"
    else
        R_SCOPE="CLEAN"
    fi
    return 0
}

# ── emit ──────────────────────────────────────────────────────────────────────
# BOTH modes render through ONE path: measurement appends rows to $_ROWS, and
# the report is rendered once at the end in the requested format. That is what
# makes --format binding in BOTH modes — the per-merge advisory consult asks for
# json too, and answering it with a table row under `--format json` would make
# the flag a lie — and it keeps each format's field order defined exactly once
# rather than once per mode.
#
# The accumulator is a FILE of US-separated records so the JSON emitter can be a
# python3 pass that escapes values correctly (a path may carry a quote or a
# backslash) rather than a hand-rolled escaper, and so both formats render from
# the SAME rows rather than from two traversals that could disagree.
_ROWS="$(mktemp "${TMPDIR:-/tmp}/task-branch-sweep-rows-XXXXXX")"

# _append_row — the ONE definition of the row's field order on the wire.
_append_row() {
    printf '%s\n' "$R_TASK$_FS$R_STATUS$_FS$R_MERGE_BASE$_FS$R_BEHIND$_FS$R_COMMITS$_FS$R_PEER_COMMITS$_FS$R_CHANGED$_FS$R_FOREIGN$_FS$R_PEER_FILES$_FS$R_PEERS$_FS$R_SCOPE$_FS$R_SIGNATURE" >> "$_ROWS"
}

# _render_report [summary]
# Renders every accumulated row in $FORMAT, then the summary if there is one.
# <summary> is the SWEEP counter string, and it is EMPTY in single-branch mode:
# one branch the caller named by id is not a fleet and has no partition to
# summarise, so neither format invents one there.
_render_report() {
    local summary="${1:-}"
    if [ "$FORMAT" = "json" ]; then
        _TB_ROWS="$_ROWS" _TB_SUMMARY="$summary" "$_PYTHON_BIN" - <<'PY'
import json, os, sys

COLS = ("task", "status", "merge_base", "behind", "commits", "peer_commits",
        "changed", "foreign", "peer_files", "peers", "scope", "signature")
# The counted columns are emitted as JSON numbers when they hold a count, and
# as the "-" placeholder string when the branch could not be measured. A
# consumer therefore never has to parse "-" out of an integer field.
NUMERIC = {"task", "behind", "commits", "peer_commits", "changed", "foreign",
           "peer_files"}

branches = []
with open(os.environ["_TB_ROWS"]) as fh:
    for line in fh:
        line = line.rstrip("\n")
        if not line:
            continue
        parts = line.split("\x1f")
        row = dict(zip(COLS, parts))
        for k in NUMERIC:
            if row[k].isdigit():
                row[k] = int(row[k])
        branches.append(row)

doc = {"branches": branches}
# An empty _TB_SUMMARY means single-branch mode: omit the key entirely rather
# than emitting a zeroed object, which would read as a measured fleet of none.
summary = os.environ.get("_TB_SUMMARY", "")
if summary:
    doc["summary"] = {k: int(v) for k, _, v in
                      (pair.partition("=") for pair in summary.split())}

json.dump(doc, sys.stdout)
sys.stdout.write("\n")
PY
        return 0
    fi
    while IFS="$_FS" read -r c_task c_status c_mb c_behind c_commits c_peer_commits \
                             c_changed c_foreign c_peer_files c_peers c_scope c_signature; do
        [ -n "${c_task:-}" ] || continue
        printf 'task=%s status=%s merge_base=%s behind=%s commits=%s peer_commits=%s changed=%s foreign=%s peer_files=%s peers=%s scope=%s signature=%s\n' \
            "$c_task" "$c_status" "$c_mb" "$c_behind" "$c_commits" "$c_peer_commits" \
            "$c_changed" "$c_foreign" "$c_peer_files" "$c_peers" "$c_scope" "$c_signature"
    done < "$_ROWS"
    [ -z "$summary" ] || printf 'SWEEP: %s\n' "$summary"
    return 0
}

# ── single-branch mode ────────────────────────────────────────────────────────
# --task names one branch explicitly, so it always gets a row — including the
# degraded one. That is the deliberate asymmetry with fleet mode, which drops a
# branchless task silently: here the caller asked about this branch by name and
# is owed an answer.
if [ "$TASK_MODE" -eq 1 ]; then
    _measure_branch "$TASK_ID"
    _append_row
    _render_report
    exit 0
fi

# ── fleet mode ────────────────────────────────────────────────────────────────
# Measured candidates are the INTERSECTION of the non-terminal ids from the
# store read and the refs that actually exist, so a terminal-backed branch
# costs no git work at all beyond the one for-each-ref, and a task with no
# branch costs none. Every ref outside that intersection is still ACCOUNTED
# for — by a named skip counter, or by an UNKNOWN row when the store itself is
# what could not be consulted. Rows are emitted in ascending task id.
N_BRANCHES=0; N_SUSPECT=0; N_PEER_FILES=0; N_OUT_OF_SCOPE=0
N_UNDECLARED=0; N_CLEAN=0; N_UNKNOWN=0
N_SKIPPED_TERMINAL=0; N_SKIPPED_NONNUMERIC=0; N_SKIPPED_NO_TASK=0

# _tally — classify the current row into exactly one counter.
#
# This is the ONE classify-and-tally site, and the order below is what makes
# the counters a PARTITION: a SUSPECT row is counted under `suspect` and
# nowhere else, so `branches` equals the six class counters summed. Both output
# formats consume these accumulated values rather than recomputing them, so the
# identity cannot hold in one format and not the other.
_tally() {
    N_BRANCHES=$((N_BRANCHES + 1))
    if [ "$R_SIGNATURE" = "SUSPECT" ]; then
        N_SUSPECT=$((N_SUSPECT + 1))
        return 0
    fi
    case "$R_SCOPE" in
        PEER-FILES)   N_PEER_FILES=$((N_PEER_FILES + 1)) ;;
        OUT-OF-SCOPE) N_OUT_OF_SCOPE=$((N_OUT_OF_SCOPE + 1)) ;;
        UNDECLARED)   N_UNDECLARED=$((N_UNDECLARED + 1)) ;;
        CLEAN)        N_CLEAN=$((N_CLEAN + 1)) ;;
        *)            N_UNKNOWN=$((N_UNKNOWN + 1)) ;;
    esac
}

while IFS= read -r _ref; do
    [ -n "$_ref" ] || continue
    _id="${_ref#"${BRANCH_PREFIX}"}"
    case "$_id" in
        ''|*[!0-9]*)
            # Never silently dropped and never an error: 48 of the live pool's
            # 1095 task/* refs have non-numeric suffixes (task/1741-recovered,
            # task/208-merge, task/2962-20260530T173412Z).
            warn "Skipping non-numeric branch: $_ref"
            N_SKIPPED_NONNUMERIC=$((N_SKIPPED_NONNUMERIC + 1))
            continue ;;
    esac
    # A ref leaves the report WITHOUT a row only when the store both answered
    # and accounted for it — hence the `$_DB_READABLE` conjunct, which is the
    # whole fix: an unconsulted ref used to fall into skipped_terminal, a
    # counter whose documented meaning is "the backing task is done or
    # cancelled", so a typo'd --db reported an all-clean fleet forever with the
    # SUSPECT branch invisible on stdout, the caller's only result channel
    # (R3). Now a store that answered nothing yields a measured-nothing
    # UNKNOWN row per ref instead, via the same funnel every other degradation
    # takes (R4).
    #
    # The two skip reasons are counted apart because they need opposite
    # responses: skipped_terminal is a retired branch, skipped_no_task is the
    # store not carrying that id under this --tag at all — which at four digits
    # means the tag is wrong, not that the pool is finished.
    if ! _is_live "$_id" && [ "$_DB_READABLE" = 1 ]; then
        if [ -n "${_TERMINAL["$_id"]:-}" ]; then
            N_SKIPPED_TERMINAL=$((N_SKIPPED_TERMINAL + 1))
        else
            N_SKIPPED_NO_TASK=$((N_SKIPPED_NO_TASK + 1))
        fi
        continue
    fi
    _measure_branch "$_id"
    _tally
    _append_row
done < <(_git for-each-ref --format='%(refname:short)' \
             "refs/heads/${BRANCH_PREFIX}*" 2>/dev/null | sort -t/ -k2 -n)
unset _ref _id

# The two aggregate stderr diagnostics, emitted once and only when they apply.
# A pool of refs whose tag holds NO live task at all is never a legitimate
# steady state, so it is called out in its own right rather than left to be
# inferred from a counter.
if [ "$N_SKIPPED_NO_TASK" -gt 0 ]; then
    if [ "${#_STATUS[@]}" -eq 0 ]; then
        warn "Tag '$TAG' has no non-terminal tasks at all, yet $N_SKIPPED_NO_TASK ${BRANCH_PREFIX}* refs exist — is --tag correct? Nothing was audited."
    else
        warn "$N_SKIPPED_NO_TASK ${BRANCH_PREFIX}* refs name an id absent from tag '$TAG' — counted as skipped_no_task, not audited."
    fi
fi

_render_report "branches=$N_BRANCHES suspect=$N_SUSPECT peer_files=$N_PEER_FILES out_of_scope=$N_OUT_OF_SCOPE undeclared=$N_UNDECLARED clean=$N_CLEAN unknown=$N_UNKNOWN skipped_terminal=$N_SKIPPED_TERMINAL skipped_nonnumeric=$N_SKIPPED_NONNUMERIC skipped_no_task=$N_SKIPPED_NO_TASK repo_unusable=$_REPO_UNUSABLE"
exit 0
