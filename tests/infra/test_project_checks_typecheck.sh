#!/usr/bin/env bash
# Infrastructure tests for typecheck invocation alignment (task 1080).
# Validates that hooks/project-checks uses `npm run typecheck` (matching
# dark-factory-orchestrator.yaml lint_command) instead of raw `npx tsc --noEmit`.

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

# -- Test 2: gui tests reach vitest via 'npm test' (fires pretest=build:grammar) --
echo ""
echo "--- Test 2: test plan runs gui via 'npm test' not 'npx vitest run' ---"

# Task 3766 deliverable: the hook formerly ran `npx vitest run`, which skips the
# pretest=build:grammar lezer codegen and lets grammar drift go uncaught. The
# unified plan reaches vitest through `npm test`, which fires the pretest hook.
#
# Task 7630 moved that `npm test` ONE INDIRECTION DEEPER: the plan's gui leaf is
# now ../scripts/gui-vitest-run.sh (which adds the bounded worker-RPC retry), and
# the runner is what invokes `npm test`. 3766's guarantee is unchanged but is no
# longer visible in the plan string alone, so it is asserted across BOTH hops --
# plan -> runner, and runner -> npm test. Asserting only the first hop would let
# the runner be rewritten to `npx vitest run` with this guard still green.
assert "test plan gui block runs the vitest runner" \
    bash -c "printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'cd gui &&' && printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'gui-vitest-run.sh'"

assert "the runner reaches vitest via 'npm test' (pretest=build:grammar still fires)" \
    bash -c "grep -qE '^[[:space:]]*npm test( --)?( \"\\\$@\")?[[:space:]]*$' '$REPO_ROOT/scripts/gui-vitest-run.sh'"

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

# -- Regression guard (task 4063): clippy is the sole Rust type-error signal ----
# dark-factory-orchestrator.yaml's type_check_command is now a no-op ("true") because
# lint_command's `cargo clippy --workspace --all-targets -- -D warnings` is a
# strict superset of `cargo check`. This assertion pins that invariant: if clippy
# were ever weakened or removed from verify.sh lint, the per-task type-error check
# would silently vanish. Reuses LINT_PLAN_SEGS captured above — no extra invocation.
echo ""
echo "--- Regression guard (task 4063): lint plan includes clippy (sole Rust type-error signal) ---"
assert "lint plan runs clippy --workspace --all-targets -D warnings (now the sole Rust type-error signal; supersedes the dropped per-task cargo check)" \
    bash -c "printf '%s\n' \"\$LINT_PLAN_SEGS\" | grep -qE 'cargo clippy --workspace --all-targets.*-D warnings'"

test_summary
