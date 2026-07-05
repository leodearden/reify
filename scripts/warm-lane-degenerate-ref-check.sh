#!/usr/bin/env bash
# scripts/warm-lane-degenerate-ref-check.sh — Read-only warm-lane degenerate
# task-branch-pointer classifier + fleet-audit primitive.
#
# Cross-repo seam primitive: reify ships this classifier; dark-factory wires
# BOTH fix angles that consume it (see
# docs/design/warm-lane-degenerate-ref-seam.md for full root-cause + wiring
# documentation):
#   angle B — DF's citation-missing / phantom-done reconciliation sweep skips
#             a ref this script classifies `degenerate` (never a landed
#             phantom; re-firing on it is the observed escalation-storm bug).
#   angle A — DF's re-block path deletes a ref this script classifies
#             `degenerate` (DF owns the delete; this script never mutates a
#             ref, mirroring the warm-lane-ref-check.sh / #4855 precedent).
#
# Root cause addressed (task #5006):
#   refs/heads/task/N is created by dark-factory's acquire path
#   (`git worktree add -b task/N <lane> <base>`), where <base> is a recent
#   main commit that is frequently ANOTHER task's no-ff merge commit ("Merge
#   task/<other> into main"). When the task faults/re-blocks before
#   producing its first commit, the ref is left parked on that foreign
#   main-ancestor with ZERO of its own commits — indistinguishable from a
#   landed-but-uncited phantom-done branch to a naive `is_ancestor(task/N,
#   main)` check. DF's citation-missing sweep re-fires on this ref every
#   reconciliation pass while the task stays blocked.
#
# Discriminant (see the seam doc for the full rationale):
#   degenerate <=> rev-list --count <main>..task/N == 0
#                  AND the branch tip does NOT cite task N
#   count==0 is mathematically equivalent to is_ancestor(task/N, main), so
#   the only thing distinguishing "degenerate, parked on a foreign ancestor"
#   from "genuinely landed" is whether the tip commit cites ITS OWN task id.
#
# The citation predicate mirrors dark-factory orchestrator/git_ops.py's
# citation regex byte-for-byte: a merge-commit subject `^Merge <prefix><id>
# into ` OR a `#<id>` reference, both with digit-boundary safety (task/1 does
# not match "Merge task/10 into main"; #45 does not match #4588).
#
# Usage — two mutually exclusive modes:
#
#   1) Single-ref classify:
#        scripts/warm-lane-degenerate-ref-check.sh --task <id> \
#            [--main-ref <ref>] [--branch-prefix <pfx>] [--repo <dir>|-C <dir>]
#      Resolves refs/heads/<prefix><id> and prints `<class> <tip_sha>` on
#      stdout (or `absent -` if the ref does not exist).
#
#   2) Fleet audit:
#        scripts/warm-lane-degenerate-ref-check.sh --audit \
#            [--main-ref <ref>] [--branch-prefix <pfx>] [--repo <dir>|-C <dir>] \
#            [--status-cmd <cmd>]
#      Enumerates every refs/heads/<prefix>* ref, classifies each, and prints
#      one machine-readable row per ref plus a summary line.
#
# Options:
#   --task <id>             Task id to classify (numeric only). Mutually
#                           exclusive with --audit.
#   --audit                 Fleet-audit mode. Mutually exclusive with --task.
#   --main-ref <ref>        Ref to diff/ancestor-check against (default: main).
#   --branch-prefix <pfx>   Branch-name prefix (default: "task/").
#   --repo <dir>, -C <dir>  Repo/worktree to operate in (default: CWD).
#   --status-cmd <cmd>      (--audit only) Optional advisory status oracle:
#                           invoked as `<cmd> <task_id>`, expected to print a
#                           task status (e.g. done/cancelled/blocked) to
#                           stdout. Empty output or non-zero exit is treated
#                           as `unknown` (non-terminal). Also settable via
#                           REIFY_DEGENERATE_REF_STATUS_CMD. Mirrors
#                           warm-lane-preflight.sh Check 6's
#                           REIFY_LANE_LEAK_STATUS_CMD contract.
#   -h, --help              Print this message and exit 0.
#
# Stdout contract:
#   Single-ref mode: exactly one line, `<class> <tip_sha>` (or `absent -`).
#   Audit mode: one `<task_id> <class> <tip_sha> [status]` row per ref,
#     followed by one `audit: degenerate=.. live=.. landed=.. absent=..
#     total=.. flagged=..` summary line.
#   All diagnostics go to stderr. This script NEVER creates, moves, or
#   deletes a ref (read-only diagnostic).
#
# Exit codes (single-ref mode; audit mode always exits 0 on a completed sweep):
#   0  — degenerate  (count==0, tip does not cite N — skip-sweep / prune-safe)
#   1  — live        (count>0 — own commits ahead of main)
#   2  — usage error
#   3  — structural  (not a git work tree, or --main-ref unresolvable)
#   4  — landed      (count==0, tip DOES cite N — genuinely merged)
#   5  — absent      (no such ref)
#
# See: docs/design/warm-lane-degenerate-ref-seam.md

set -euo pipefail

# ── log helpers (all write to stderr) ─────────────────────────────────────────
info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*" >&2; }
ok()    { printf '\033[1;32m[ok]\033[0m    %s\n' "$*" >&2; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*" >&2; }
err()   { printf '\033[1;31m[error]\033[0m %s\n' "$*" >&2; }
hint()  { printf '\033[1;33m[hint]\033[0m  %s\n' "$*" >&2; }

# ── usage ──────────────────────────────────────────────────────────────────────
_usage() {
    cat >&2 <<EOF
Usage: $(basename "$0") --task <id> [OPTIONS]
   or: $(basename "$0") --audit [OPTIONS]

  Read-only warm-lane degenerate task-branch-pointer classifier + fleet-audit
  primitive. See the script header for the full contract.

  Options:
    --task <id>             Classify refs/heads/<prefix><id> (numeric only).
    --audit                  Fleet-audit every refs/heads/<prefix>* ref.
    --main-ref <ref>         Ref to compare against (default: main).
    --branch-prefix <pfx>   Branch-name prefix (default: "task/").
    --repo <dir>, -C <dir>   Repo/worktree to operate in (default: CWD).
    --status-cmd <cmd>       (--audit only) Advisory status oracle.
    -h, --help               Print this message and exit 0.

  Exit codes: 0=degenerate 1=live 2=usage 3=structural 4=landed 5=absent

  See: docs/design/warm-lane-degenerate-ref-seam.md
EOF
}

# ── arg parsing ────────────────────────────────────────────────────────────────
TASK_ID=""
AUDIT_MODE=0
MAIN_REF="main"
BRANCH_PREFIX="task/"
REPO_DIR=""
STATUS_CMD="${REIFY_DEGENERATE_REF_STATUS_CMD:-}"

while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            _usage; exit 0 ;;
        --task)
            [ $# -ge 2 ] || { err "--task requires a value"; exit 2; }
            TASK_ID="$2"; shift 2 ;;
        --audit)
            AUDIT_MODE=1; shift ;;
        --main-ref)
            [ $# -ge 2 ] || { err "--main-ref requires a value"; exit 2; }
            MAIN_REF="$2"; shift 2 ;;
        --branch-prefix)
            [ $# -ge 2 ] || { err "--branch-prefix requires a value"; exit 2; }
            BRANCH_PREFIX="$2"; shift 2 ;;
        --repo|-C)
            [ $# -ge 2 ] || { err "--repo requires a value"; exit 2; }
            REPO_DIR="$2"; shift 2 ;;
        --status-cmd)
            [ $# -ge 2 ] || { err "--status-cmd requires a value"; exit 2; }
            STATUS_CMD="$2"; shift 2 ;;
        *)
            err "Unknown flag: $1"
            err "Run '$(basename "$0") --help' for usage."
            exit 2 ;;
    esac
done

# ── mode validation: exactly one of --task / --audit ──────────────────────────
if [ -n "$TASK_ID" ] && [ "$AUDIT_MODE" -eq 1 ]; then
    err "--task and --audit are mutually exclusive"
    err "Run '$(basename "$0") --help' for usage."
    exit 2
fi
if [ -z "$TASK_ID" ] && [ "$AUDIT_MODE" -ne 1 ]; then
    err "Exactly one of --task <id> or --audit is required"
    _usage
    exit 2
fi

# ── numeric --task validation ─────────────────────────────────────────────────
if [ -n "$TASK_ID" ] && ! printf '%s\n' "$TASK_ID" | grep -qE '^[0-9]+$'; then
    err "--task must be a positive integer (got: '$TASK_ID')"
    exit 2
fi

# Structural preflight and classify/audit mode dispatch land in subsequent
# TDD steps of task #5006's plan.
exit 0
