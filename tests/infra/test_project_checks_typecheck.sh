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

HOOK="$REPO_ROOT/hooks/project-checks"
ORCH="$REPO_ROOT/orchestrator.yaml"
PKG="$REPO_ROOT/gui/package.json"

# -- Test 1: hooks/project-checks uses npm run typecheck ----------------------
echo ""
echo "--- Test 1: hooks/project-checks uses npm run typecheck ---"

assert "hooks/project-checks contains 'npm run typecheck'" \
    bash -c "grep -q 'npm run typecheck' '$HOOK'"

assert "hooks/project-checks does NOT contain 'npx tsc --noEmit'" \
    bash -c "! grep -q 'npx tsc --noEmit' '$HOOK'"

# -- Test 2: error message is aligned with new invocation ---------------------
echo ""
echo "--- Test 2: error message references npm run typecheck, not tsc --noEmit ---"

assert "hook error message does NOT mention 'tsc --noEmit'" \
    bash -c "! grep -q 'tsc --noEmit failed' '$HOOK'"

assert "hook error message mentions 'npm run typecheck failed'" \
    bash -c "grep -q 'npm run typecheck failed' '$HOOK'"

# -- Test 3: orchestrator.yaml lint_command already uses npm run typecheck ----
echo ""
echo "--- Test 3: orchestrator.yaml lint_command uses npm run typecheck ---"

assert "orchestrator.yaml lint_command contains 'npm run typecheck'" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'npm run typecheck'"

# -- Test 4: gui/package.json defines a typecheck script ----------------------
echo ""
echo "--- Test 4: gui/package.json defines a typecheck script ---"

assert "gui/package.json exists" \
    test -f "$PKG"

assert "gui/package.json defines a 'typecheck' script" \
    bash -c "grep -qE '\"typecheck\"\\s*:' '$PKG'"

test_summary
