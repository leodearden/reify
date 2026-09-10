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
