#!/usr/bin/env bash
# Infrastructure tests for typecheck invocation alignment (task 1080).
# Validates that hooks/project-checks uses `npm run typecheck` (matching
# orchestrator.yaml lint_command) instead of raw `npx tsc --noEmit`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== typecheck invocation alignment tests ==="

PKG="$REPO_ROOT/gui/package.json"

# Since task 3766 both the hook and the orchestrator run scripts/verify.sh, so
# the typecheck invocation is asserted against verify.sh --print-plan (the
# single source), not the hook/orchestrator literals. These assertions are
# invariant across the hook/orchestrator flip — they reference only verify.sh.
# --scope all forces the full plan; env lines stripped via `grep -v '^#'`.
LINT_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" lint --scope all --include-infra --print-plan | grep -v '^#')"
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile debug --scope all --include-infra --print-plan | grep -v '^#')"
export LINT_PLAN_SEGS TEST_PLAN_SEGS

# -- Test 1: typecheck uses 'npm run typecheck', not raw 'npx tsc --noEmit' ----
echo ""
echo "--- Test 1: lint plan uses npm run typecheck (not npx tsc --noEmit) ---"

assert "lint plan contains 'npm run typecheck'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'npm run typecheck'"

assert "lint plan does NOT contain raw 'npx tsc --noEmit'" \
    bash -c "! printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'npx tsc --noEmit'"

# -- Test 2: gui tests run via 'npm test' (fires pretest=build:grammar) --------
echo ""
echo "--- Test 2: test plan runs gui via 'npm test' not 'npx vitest run' ---"

# Task 3766 deliverable: the hook formerly ran `npx vitest run`, which skips the
# pretest=build:grammar lezer codegen and lets grammar drift go uncaught. The
# unified plan runs `npm test`, which fires the pretest hook.
assert "test plan gui block runs 'npm test'" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'cd gui &&' && printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'npm test'"

assert "test plan does NOT run 'npx vitest run' (the pretest bypass)" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'npx vitest'"

# -- Test 3: sidecar typecheck:test preserved in the lint plan -----------------
echo ""
echo "--- Test 3: lint plan keeps sidecar 'npm run typecheck:test' ---"

assert "lint plan contains sidecar 'npm run typecheck:test'" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -q 'npm run typecheck:test'"

# -- Test 4: gui/package.json defines a typecheck script ----------------------
echo ""
echo "--- Test 4: gui/package.json defines a typecheck script ---"

assert "gui/package.json exists" \
    test -f "$PKG"

assert "gui/package.json defines a 'typecheck' script" \
    bash -c "grep -qE '\"typecheck\"\\s*:' '$PKG'"

assert "gui/package.json scripts.typecheck contains tsc --noEmit" \
    bash -c "node -e 'process.exit(/tsc --noEmit/.test(require(\"$PKG\").scripts.typecheck) ? 0 : 1)'"

test_summary
