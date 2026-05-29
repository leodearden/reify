#!/usr/bin/env bash
# Infrastructure test for task 1410.
# Validates that a release-mode cargo test pass exists so tests gated on
# #[cfg(not(debug_assertions))] (and the heavy release-only tests gated behind
# #[cfg_attr(debug_assertions, ignore)]) are exercised. Release coverage now
# lives at the MERGE GATE: per-task verify (orchestrator.yaml) runs --profile
# debug for fast feedback, while hooks/pre-merge-commit runs --profile both.
# Tests 1-5 below pin the release passes that `verify.sh … --profile both`
# emits (the profile the merge gate uses); Test 8 pins that the merge gate is
# in fact where --profile both is wired.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== release-mode test_command tests ==="

# Canonical command lists from verify.sh --print-plan (the oracle the
# orchestrator calls since task 3766), not orchestrator.yaml directly. --scope
# all forces the full plan; env lines are stripped via `grep -v '^#'`. Both
# runner spellings (cargo test / cargo nextest run) are accepted — under nextest
# the UNGATED tail drops `-- --test-threads=1` (nextest isolates per test), while
# the GATED OCCT passes keep it. So Test 3 below pins single-threaded release to
# the gated pass, where the OCCT serialization actually matters.
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --print-plan | grep -v '^#')"
LINT_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --print-plan | grep -v '^#')"
export TEST_PLAN_SEGS LINT_PLAN_SEGS

# -- Test 1: release pass exists -----------------------------------------------
echo ""
echo "--- Test 1: release workspace test pass present in the plan ---"

assert "plan contains a 'cargo (test|nextest run) --workspace … --release' pass" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -qE 'cargo (test|nextest run) --workspace.*--release'"

# -- Test 2: debug pass preserved ----------------------------------------------
echo ""
echo "--- Test 2: debug (non-release) workspace test pass preserved ---"

assert "plan contains a non-release 'cargo (test|nextest run) --workspace' pass" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -E 'cargo (test|nextest run) --workspace' | grep -vq -- '--release'"

# -- Test 3: release OCCT pass uses --test-threads=1 ---------------------------
echo ""
echo "--- Test 3: gated release pass runs single-threaded (--test-threads=1) ---"

# Single-threaded release matters for the OCCT-touching crates (shared C++
# global state); that pass is the flock-gated `cargo test … --release`.
assert "plan's gated release pass uses '--release -- --test-threads=1'" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep 'cargo-test-occt-gated\.sh' | grep -- '--release' | grep -qE -- '--release -- --test-threads=1'"

# -- Test 4: ordering (release AFTER debug) ------------------------------------
echo ""
echo "--- Test 4: release pass appears after debug pass in the plan ---"

assert "ungated release pass appears after the ungated debug pass" \
    bash -c "
        DEBUG_IDX=\$(printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -nE 'cargo (test|nextest run) --workspace' | grep -v -- '--release' | head -1 | cut -d: -f1)
        RELEASE_IDX=\$(printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -nE 'cargo (test|nextest run) --workspace' | grep -- '--release' | head -1 | cut -d: -f1)
        [ -n \"\$DEBUG_IDX\" ] && [ -n \"\$RELEASE_IDX\" ] && [ \"\$RELEASE_IDX\" -gt \"\$DEBUG_IDX\" ]
    "

# -- Test 5: release pass NOT in lint plan -------------------------------------
echo ""
echo "--- Test 5: no release test pass in the lint plan ---"

assert "lint plan does NOT contain a '--release' test pass" \
    bash -c "! printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE 'cargo (test|nextest run).*--release'"

# -- Test 6: sanity check — release-only test exists in workspace --------------
echo ""
echo "--- Test 6: at least one #[cfg(not(debug_assertions))] test exists ---"

assert "at least one .rs file in workspace contains #[cfg(not(debug_assertions))]" \
    grep -rq --exclude-dir=target --exclude-dir=.git --exclude-dir=node_modules '#\[cfg(not(debug_assertions))\]' "$REPO_ROOT" --include='*.rs'

# -- Test 7: structural self-check — Test 6 must use workspace-wide grep ---------
echo ""
echo "--- Test 7: Test 6 is path-agnostic (structural self-check) ---"

THIS_FILE="${BASH_SOURCE[0]}"

assert "Test 6 grep targets REPO_ROOT as sole path (no subdirectory)" \
    bash -c "grep -qE '^\s+grep -rq.*REPO_ROOT\"[[:space:]]' \"$THIS_FILE\""

assert "Test 6 uses workspace-wide recursive grep with --include flag" \
    bash -c "grep -qE '^\s+grep -rq.*REPO_ROOT.*--include=' \"$THIS_FILE\""

# -- Test 8: release coverage is pinned to the merge gate ------------------------
echo ""
echo "--- Test 8: hooks/pre-merge-commit carries --profile both (merge-gate release) ---"

# Release (and the heavy release-only tests) run at the merge boundary, not on
# every per-task iteration. Pin --profile both to the pre-merge-commit hook so
# this location can't silently regress to debug-only.
PRE_MERGE="$REPO_ROOT/hooks/pre-merge-commit"

assert "hooks/pre-merge-commit execs verify.sh all --profile both --scope all" \
    bash -c "grep -qE 'scripts/verify\.sh\" all --profile both --scope all' '$PRE_MERGE'"

test_summary
