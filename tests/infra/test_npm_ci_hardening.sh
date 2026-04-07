#!/usr/bin/env bash
# Infrastructure tests for npm ci hardening (task 816).
# Validates that check-pm-standardization.sh lives in scripts/ and that
# orchestrator.yaml uses if/then/fi guards instead of || true for npm ci.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== npm ci hardening tests ==="

# -- Test 1: check-pm-standardization.sh location ----------------------------
echo ""
echo "--- Test 1: script lives in scripts/, not tests/ ---"

assert "scripts/check-pm-standardization.sh exists" \
    test -f "$REPO_ROOT/scripts/check-pm-standardization.sh"

assert "scripts/check-pm-standardization.sh is executable" \
    test -x "$REPO_ROOT/scripts/check-pm-standardization.sh"

assert "tests/check-pm-standardization.sh does NOT exist" \
    bash -c "! test -f '$REPO_ROOT/tests/check-pm-standardization.sh'"

# -- Test 2: script has only checks 1-3 (no 4-9) ----------------------------
echo ""
echo "--- Test 2: script contains only checks 1-3 ---"

SCRIPT="$REPO_ROOT/scripts/check-pm-standardization.sh"

assert "script has no grep calls referencing hooks/project-checks" \
    bash -c "! grep -qE 'grep.*hooks/project-checks|hooks/project-checks.*grep' '$SCRIPT'"

assert "script has no grep calls referencing orchestrator.yaml" \
    bash -c "! grep -qE 'grep.*orchestrator|orchestrator.*grep' '$SCRIPT'"

assert "script has exactly 3 'Check N:' echo statements" \
    bash -c "[ \"\$(grep -cE 'echo \"Check [0-9]' '$SCRIPT')\" = '3' ]"

# -- Test 3: orchestrator.yaml uses if/then/fi guards (not || true) ----------
echo ""
echo "--- Test 3: orchestrator.yaml if/then/fi guards for npm ci ---"

ORCH="$REPO_ROOT/orchestrator.yaml"

assert "test_command has no '|| true' after npm ci" \
    bash -c "! grep 'test_command:' '$ORCH' | grep -q 'npm ci.*|| true\||| true.*npm ci'"

assert "lint_command has no '|| true' after npm ci" \
    bash -c "! grep 'lint_command:' '$ORCH' | grep -q 'npm ci.*|| true\||| true.*npm ci'"

assert "test_command uses 'if test' guard pattern for npm ci" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'if test'"

assert "lint_command uses 'if test' guard pattern for npm ci" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'if test'"

# -- Test 4: orchestrator command placement and existence guards ---------------
echo ""
echo "--- Test 4: orchestrator command placement and existence guards ---"

assert "check-pm-standardization.sh is in lint_command" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'check-pm-standardization.sh'"

assert "check-pm-standardization.sh is NOT in test_command" \
    bash -c "! grep 'test_command:' '$ORCH' | grep -q 'check-pm-standardization.sh'"

assert "sync_comments_test.sh uses 'if test -f' guard in test_command" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'if test -f tests/sync_comments_test.sh'"

assert "check-pm-standardization.sh uses 'if test -f' guard in lint_command" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'if test -f scripts/check-pm-standardization.sh'"

test_summary
