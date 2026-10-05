#!/usr/bin/env bash
# scripts/land.sh — the sanctioned manual path to land a task branch onto main.
#
# This is the blessed alternative to routing a merge through the orchestrator's
# merge queue (/merge-queue) when the orchestrator is congested or down. It
# replaces the dangerous ad-hoc fallbacks (`git merge --no-verify`,
# `git update-ref`, manual `commit-tree` + `update-ref`) and closes the two traps
# that made them necessary/dangerous:
#
#   1. It REFUSES a dirty working tree. The pre-merge-commit gate verifies the
#      WHOLE working tree (a merge cannot trust `git diff --cached`), so unrelated
#      dirt produced false negatives — which is exactly why the old fallback
#      reached for --no-verify. Refusing dirt removes that pressure at the source.
#   2. It runs a REAL `git merge --no-ff` (NOT --no-verify), so the
#      hooks/pre-merge-commit gate runs the full `--scope all --profile both`
#      verification. No bypass.
#
# It also marks the main-gate sentinel (hooks/main-gate-lib.sh) so the
# hooks/reference-transaction tripwire records the resulting refs/heads/main move
# as SANCTIONED instead of flagging it; on any failure it removes that mark so a
# move that did not happen cannot falsely sanction a later one.
#
# Usage: scripts/land.sh <task-branch>

set -uo pipefail

usage() {
    echo "usage: scripts/land.sh <task-branch>" >&2
    echo "  Lands <task-branch> onto main via a verified --no-ff merge." >&2
    echo "  Requires: current branch = main, clean working tree, existing branch." >&2
}

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [ -z "$ROOT" ]; then
    echo "land.sh: ERROR — not inside a git repository." >&2
    exit 1
fi
cd "$ROOT"

BRANCH="${1:-}"
if [ -z "$BRANCH" ]; then
    echo "land.sh: ERROR — no task branch given." >&2
    usage
    exit 64
fi

# Refuse off-main (the gate, and the sentinel handshake, are main-only).
current="$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo '?')"
if [ "$current" != "main" ]; then
    echo "land.sh: ERROR — current branch is '$current', not 'main'. Check out main first." >&2
    exit 1
fi

# Refuse merging main into itself.
if [ "$BRANCH" = "main" ]; then
    echo "land.sh: ERROR — refusing to merge 'main' into itself." >&2
    exit 1
fi

# Refuse a branch that does not resolve to a commit.
if ! git rev-parse --verify --quiet "${BRANCH}^{commit}" >/dev/null 2>&1; then
    echo "land.sh: ERROR — branch '$BRANCH' does not exist (no such commit-ish)." >&2
    exit 1
fi

# Refuse a dirty working tree — see header trap (1).
if [ -n "$(git status --porcelain)" ]; then
    echo "land.sh: ERROR — working tree is dirty. The merge gate verifies the WHOLE" >&2
    echo "  working tree, so unrelated changes would cause false failures. Commit or" >&2
    echo "  clean them first, then re-run." >&2
    exit 1
fi

# Defensively re-assert the relative gate path so hooks/reference-transaction is
# live at the exact refs/heads/main move below.  Claude Code's worktree feature
# can overwrite the SHARED core.hooksPath to the inert .git/hooks samples dir;
# this is the guard for the manual-landing path (see task 4380).  The write goes
# to the per-worktree config (extensions.worktreeConfig, task 4379), which git
# reads first — so it beats any shared-config clobber.  When the per-worktree
# core.hooksPath is already 'hooks' this is a true no-op.  If worktreeConfig is
# not enabled, git config --worktree errors and we fail loudly below.
git config --worktree core.hooksPath hooks \
    || { echo 'land.sh: ERROR — could not set per-worktree core.hooksPath (extensions.worktreeConfig missing, or read-only/locked config); aborting before the main move to avoid landing with a dark gate.' >&2; exit 1; }

# Sanction the upcoming refs/heads/main move for hooks/reference-transaction.
# shellcheck source=hooks/main-gate-lib.sh
. "$ROOT/hooks/main-gate-lib.sh"
main_gate_mark

# Run the merge gate as role=merge so the held test-run slot and PSI gate exempt
# the manual-land path — without this a manual land queues behind a task slot
# (merge-starvation / livelock; PRD §5 D5). Exporting propagates the role to the
# pre-merge-commit child spawned by git merge.
#
# That same child inherits REIFY_GATE_EXCLUDE_HEAVY, which scopes the 8 heavy-filter
# members (scripts/heavy-test-filter-lib.sh) out of this local gate exactly as
# dark-factory-orchestrator.yaml:230 already does for every orchestrator-spawned
# role. Defaulting it to 1 is load-bearing, not tidiness: this path runs
# --profile both --scope all, so the debug --workspace pass runs the heavy members
# under a binding 3600s wall where their 12h per-test ceiling is unreachable and a
# hang degrades to a bare `timeout` exit 124 naming nothing. Do not remove this
# without also removing that ceiling.
#
# The decision, the coverage-vs-diagnosability trade behind it, why the hook is
# deliberately left alone, and the accepted residual are recorded ONCE in
# docs/prds/offline-deep-test-lane.md DA5 (esc-6485-3 option B, Leo 2026-08-31).
# Propagation into the gate child is pinned behaviourally, not by grep, in
# tests/infra/test_land_script.sh.
#
# `:-` rather than a bare 1, deliberately: DA5 settles the DEFAULT, not whether an
# operator may override it. `REIFY_GATE_EXCLUDE_HEAVY=0 scripts/land.sh <branch>`
# still buys full local heavy coverage, at the cost of the attribution above — the
# behaviour every local land had before task 6485.
export DF_VERIFY_ROLE=merge
export REIFY_GATE_EXCLUDE_HEAVY="${REIFY_GATE_EXCLUDE_HEAVY:-1}"
echo "land.sh: merging '$BRANCH' into main (--no-ff; pre-merge-commit runs the full gate)..." >&2
if git merge --no-ff "$BRANCH"; then
    landed="$(git rev-parse HEAD)"
    echo "land.sh: landed '$BRANCH' on main as $landed" >&2
    echo "$landed"
    exit 0
fi

rc=$?
echo "land.sh: ERROR — merge failed (exit $rc). Aborting; main is left unchanged." >&2
git merge --abort 2>/dev/null || true
# Drop our pre-mark: the sanctioned move did not happen, so the sentinel must not
# linger and falsely sanction a later, unsanctioned main move.
rm -f "$(main_gate_sentinel)" 2>/dev/null || true
exit "$rc"
