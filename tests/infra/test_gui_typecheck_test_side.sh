#!/usr/bin/env bash
# Infrastructure test for Fix 1 (main-gate-hardening): the GUI typecheck
# (npm run typecheck = tsc --noEmit) now runs on the verify.sh TEST side, not
# only the lint side.
#
# Why this matters: the orchestrator's inner TDD loop runs
# `verify.sh test --scope branch`, whose GUI step was `npm test` (vitest) only —
# never tsc. A type-only break that renders fine at runtime (e.g. a solid-js
# <Show> function-child rejected by the non-keyed overload, TS2769) therefore
# stayed invisible through an entire task and only surfaced at lint/merge time.
# And because verify.sh forces RUN_GUI=1 for ANY Rust change, that one inherited
# type error then blocked every task's branch verify. Putting tsc on the test
# side catches the class in the cheap inner loop.
#
# Oracle: verify.sh --print-plan (a faithful dry run that executes nothing). We
# assert WHAT the plan contains for the test / lint / all / typecheck actions.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== GUI typecheck runs on the verify.sh test side (Fix 1) ==="

# --scope all forces the full plan; strip env comment lines (start with '#').
TEST_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" test --scope all --print-plan | grep -v '^#')"
LINT_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --print-plan | grep -v '^#')"
ALL_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" all --scope all --print-plan | grep -v '^#')"
TYPECHECK_PLAN="$(bash "$REPO_ROOT/scripts/verify.sh" typecheck --scope all --print-plan | grep -v '^#')"
export TEST_PLAN LINT_PLAN ALL_PLAN TYPECHECK_PLAN

# -- The headline fix: the test side now type-checks the GUI -------------------
echo ""
echo "--- test plan runs 'npm run typecheck' on the gui block (Fix 1 deliverable) ---"
assert "test plan gui block runs both typecheck and vitest" \
    bash -c "printf '%s\n' \"\$TEST_PLAN\" | grep -q 'cd gui &&' && printf '%s\n' \"\$TEST_PLAN\" | grep -q 'npm run typecheck'"
assert "test plan gui block chains 'npm ci && npm run typecheck && npm test'" \
    bash -c "printf '%s\n' \"\$TEST_PLAN\" | grep -q 'npm ci && npm run typecheck && npm test'"

# -- The typecheck must still appear on the lint side and for action=all -------
echo ""
echo "--- typecheck still present for lint and all ---"
assert "lint plan still contains 'npm run typecheck'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN\" | grep -q 'npm run typecheck'"
assert "all plan contains 'npm run typecheck'" \
    bash -c "printf '%s\n' \"\$ALL_PLAN\" | grep -q 'npm run typecheck'"

# -- No double-run: the GUI block is built once, so action=all runs it once ----
echo ""
echo "--- action=all runs the gui typecheck exactly once (no double-run) ---"
assert "all plan has exactly one gui 'npm ci && npm run typecheck && npm test' line" \
    bash -c "[ \"\$(printf '%s\n' \"\$ALL_PLAN\" | grep -c 'npm ci && npm run typecheck && npm test')\" = '1' ]"

# -- The sidecar typecheck now runs on the test side too (was lint-only) -------
echo ""
echo "--- sidecar typecheck runs whenever the GUI block runs (test side included) ---"
assert "test plan contains sidecar 'npm run typecheck && npm run typecheck:test'" \
    bash -c "printf '%s\n' \"\$TEST_PLAN\" | grep -q 'npm run typecheck && npm run typecheck:test'"
assert "lint plan still contains sidecar 'npm run typecheck:test'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN\" | grep -q 'npm run typecheck:test'"

# -- The verify.sh 'typecheck' ACTION (cargo-check only) still has no GUI block -
# That action is DO_TEST=0 DO_LINT=0, so the GUI block (gated on test||lint) is
# correctly skipped — the GUI has no `cargo check` analogue. Pinning this keeps
# Fix 1 from accidentally wiring an npm pass into the cargo-only typecheck action.
echo ""
echo "--- verify.sh 'typecheck' action has no GUI npm block (unchanged) ---"
assert "typecheck action plan has NO 'npm run typecheck' (GUI block gated on test||lint)" \
    bash -c "! printf '%s\n' \"\$TYPECHECK_PLAN\" | grep -q 'npm run typecheck'"

test_summary
