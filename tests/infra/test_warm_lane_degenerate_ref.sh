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

# ─────────────────────────────────────────────────────────────────────────────
# step-1 — arg-parsing / usage taxonomy
#
# U1 — no mode (neither --task nor --audit)   -> exit 2, usage on stderr
# U2 — --help                                  -> exit 0, usage on stderr
# U3 — -h                                      -> exit 0, usage on stderr
# U4 — unknown flag                            -> exit 2
# U5 — --task with no value                    -> exit 2
# U6 — --task N and --audit together           -> exit 2 (mutually exclusive)
# U7 — --task with a non-numeric id            -> exit 2
# All error cases (U1, U4-U7): empty stdout.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- step-1: arg-parsing / usage taxonomy ---"

run_helper
assert "U1: no mode exits 2" test "$RC" -eq 2
assert "U1: stderr carries usage" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"
assert "U1: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

run_helper --help
assert "U2: --help exits 0" test "$RC" -eq 0
assert "U2: stderr carries usage" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"
assert "U2: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

run_helper -h
assert "U3: -h exits 0" test "$RC" -eq 0
assert "U3: stderr carries usage" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"
assert "U3: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

run_helper --bogus-flag
assert "U4: unknown flag exits 2" test "$RC" -eq 2
assert "U4: stderr is non-empty" bash -c '[ -n "$1" ]' _ "$ERR_OUT"
assert "U4: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

run_helper --task
assert "U5: --task with no value exits 2" test "$RC" -eq 2
assert "U5: stderr is non-empty" bash -c '[ -n "$1" ]' _ "$ERR_OUT"
assert "U5: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

run_helper --task 123 --audit
assert "U6: --task and --audit together exits 2" test "$RC" -eq 2
assert "U6: stderr is non-empty" bash -c '[ -n "$1" ]' _ "$ERR_OUT"
assert "U6: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

run_helper --task abc
assert "U7: --task with non-numeric id exits 2" test "$RC" -eq 2
assert "U7: stderr is non-empty" bash -c '[ -n "$1" ]' _ "$ERR_OUT"
assert "U7: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

# ─────────────────────────────────────────────────────────────────────────────
# step-3 — structural-error taxonomy (exit 3)
#
# S1 — --repo is NOT inside a git work tree -> exit 3, empty stdout
# S2 — unresolvable --main-ref against a valid repo -> exit 3, empty stdout
# Exit 3 is distinct from usage (2) and from any classification code.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- step-3: structural-error taxonomy (exit 3) ---"

S3_TMP="$(mktemp -d /tmp/test-warm-lane-degen-ref-s3-XXXXXX)"
_TMPDIRS+=("$S3_TMP")

# S1: --repo is NOT inside a git work tree -> exit 3
S3_NOT_A_REPO="$S3_TMP/not-a-repo"
mkdir -p "$S3_NOT_A_REPO"
run_helper --task 1 --repo "$S3_NOT_A_REPO"
assert "S1: not-a-git-work-tree exits 3" test "$RC" -eq 3
assert "S1: exit code 3 is distinct from usage (2)" bash -c '[ "$1" -ne 2 ]' _ "$RC"
assert "S1: stderr names the work-tree/provisioning condition" \
    bash -c 'printf "%s\n" "$1" | grep -qi "work tree\|worktree\|git repo\|not a git"' _ "$ERR_OUT"
assert "S1: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

# S2: unresolvable --main-ref against a valid repo -> exit 3
S3_REPO="$S3_TMP/repo"
build_fixture "$S3_REPO"
run_helper --task 1 --repo "$S3_REPO" --main-ref does/not/exist
assert "S2: unresolvable --main-ref exits 3" test "$RC" -eq 3
assert "S2: exit code 3 is distinct from usage (2)" bash -c '[ "$1" -ne 2 ]' _ "$RC"
assert "S2: stderr names the main-ref condition" \
    bash -c 'printf "%s\n" "$1" | grep -qi "main-ref\|main ref\|does/not/exist"' _ "$ERR_OUT"
assert "S2: stdout is empty" bash -c '[ -z "$1" ]' _ "$OUT"

# Further behavioral assertion blocks land in subsequent TDD steps (step-5
# onward): single-ref classification taxonomy, fleet-audit mode, and the
# audit status-oracle.

test_summary
