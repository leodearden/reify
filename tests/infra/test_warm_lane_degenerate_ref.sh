#!/usr/bin/env bash
# tests/infra/test_warm_lane_degenerate_ref.sh
# Hermetic tests for scripts/warm-lane-degenerate-ref-check.sh (task #5006).
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# build_fixture inits a hermetic `git init -b main` repo (no linked worktree
# needed — the classifier only reads refs + commit ancestry/messages, never a
# working tree) and the fixture_* helpers below add task branches at the
# three tip shapes the classifier must distinguish:
#   fixture_branch_at_foreign_merge — tip cites a DIFFERENT task id
#                                      (the degenerate-ref shape)
#   fixture_branch_at_self_merge    — tip cites its OWN task id (landed)
#   fixture_branch_own_commit       — one commit of its own ahead of main (live)
#
# Blocks (added incrementally across task #5006's TDD steps):
#   step-1  — arg-parsing / usage taxonomy
#   step-3  — structural-error taxonomy (exit 3)
#   step-5  — single-ref DEGENERATE classification + read-only invariant
#   step-7  — single-ref LIVE/LANDED/ABSENT + substring-safety
#   step-9  — fleet-audit mode (no status oracle)
#   step-11 — fleet-audit status-oracle (advisory filter)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-degenerate-ref-check.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-degenerate-ref-check.sh hermetic tests (task 5006) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-warm-lane-degen-ref-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ────────────────────────────────────────────────────────────────
# Invokes the script under test with no PATH stub.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ─────────────────────────────────────────────────────────────────────────────
# Fixture builders
# ─────────────────────────────────────────────────────────────────────────────

# build_fixture <dir>
# Inits a hermetic main-checkout git repo at <dir>: `git init -b main`, test
# user config, and one initial commit. Task branches are added directly to
# this repo's refs/heads/ namespace by the fixture_* helpers below.
build_fixture() {
    local dir="$1"
    git init -q -b main "$dir"
    git -C "$dir" config user.email "test@test.local"
    git -C "$dir" config user.name "Test"
    git -C "$dir" commit -q --allow-empty -m "initial"
}

# fixture_merge_commit <repo> <cite_id> [prefix]
# Creates a throwaway side branch, commits once, merges it into main with an
# explicit "Merge <prefix><cite_id> into main" message, then deletes the side
# branch. Leaves `main` checked out. Prints the resulting merge-commit SHA on
# stdout — ONLY the SHA; all git chatter is redirected away so callers can
# capture it via command substitution.
fixture_merge_commit() {
    local repo="$1" cite_id="$2" prefix="${3:-task/}"
    local side="_fixture-side-${cite_id}-$$-${RANDOM}"
    git -C "$repo" checkout -q -b "$side" main >/dev/null
    git -C "$repo" commit -q --allow-empty -m "work for ${prefix}${cite_id}" >/dev/null
    git -C "$repo" checkout -q main >/dev/null
    git -C "$repo" merge -q --no-ff -m "Merge ${prefix}${cite_id} into main" "$side" >/dev/null
    git -C "$repo" branch -D "$side" >/dev/null
    git -C "$repo" rev-parse HEAD
}

# fixture_branch_at_foreign_merge <repo> <branch_task_id> <cited_task_id> [prefix]
# Points refs/heads/<prefix><branch_task_id> at a NEW merge commit that cites
# a DIFFERENT id — the degenerate-ref shape: parked on a foreign
# main-ancestor, zero of its own commits, tip does not cite its own id.
fixture_branch_at_foreign_merge() {
    local repo="$1" branch_id="$2" cited_id="$3" prefix="${4:-task/}"
    local sha
    sha="$(fixture_merge_commit "$repo" "$cited_id" "$prefix")"
    git -C "$repo" branch -q "${prefix}${branch_id}" "$sha"
}

# fixture_branch_at_self_merge <repo> <task_id> [prefix]
# Points refs/heads/<prefix><task_id> at a NEW merge commit that cites its
# OWN id — the landed shape.
fixture_branch_at_self_merge() {
    local repo="$1" task_id="$2" prefix="${3:-task/}"
    local sha
    sha="$(fixture_merge_commit "$repo" "$task_id" "$prefix")"
    git -C "$repo" branch -q "${prefix}${task_id}" "$sha"
}

# fixture_branch_own_commit <repo> <task_id> [prefix]
# Creates refs/heads/<prefix><task_id> with ONE commit of its own ahead of
# main (count>0) — the live shape. Leaves `main` checked out.
fixture_branch_own_commit() {
    local repo="$1" task_id="$2" prefix="${3:-task/}"
    git -C "$repo" checkout -q -b "${prefix}${task_id}" main >/dev/null
    git -C "$repo" commit -q --allow-empty -m "own work for ${prefix}${task_id}" >/dev/null
    git -C "$repo" checkout -q main >/dev/null
}

# Behavioral assertion blocks land in subsequent TDD steps (step-1 onward).

test_summary
