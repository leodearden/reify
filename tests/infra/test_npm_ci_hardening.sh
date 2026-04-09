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

# -- Test 2: script has only checks 1-4 (no 5-9) ----------------------------
echo ""
echo "--- Test 2: script contains only checks 1-4 ---"

SCRIPT="$REPO_ROOT/scripts/check-pm-standardization.sh"

assert "script has no grep calls referencing hooks/project-checks" \
    bash -c "! grep -qE 'grep.*hooks/project-checks|hooks/project-checks.*grep' '$SCRIPT'"

assert "script has no grep calls referencing orchestrator.yaml" \
    bash -c "! grep -qE 'grep.*orchestrator|orchestrator.*grep' '$SCRIPT'"

assert "script has exactly 4 'Check N:' echo statements" \
    bash -c "[ \"\$(grep -cE 'echo \"Check [0-9]' '$SCRIPT')\" = '4' ]"

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

# -- Test 4: script has git preflight check ----------------------------------
echo ""
echo "--- Test 4: script has 'command -v git' preflight ---"

assert "script contains 'command -v git' preflight check" \
    grep -q 'command -v git' "$SCRIPT"

# -- Test 5: Check 1 matches npm@ prefix, not just field presence -------------
echo ""
echo "--- Test 5: Check 1 grep matches npm@ prefix ---"

assert "Check 1 grep pattern includes 'npm@' prefix match" \
    bash -c "grep -qE 'grep.*npm@' '$SCRIPT'"

# -- Test 6: script has cross-file consistency check -------------------------
echo ""
echo "--- Test 6: script has cross-file packageManager consistency check ---"

assert "script contains 'sort -u' for cross-file consistency comparison" \
    grep -q 'sort -u' "$SCRIPT"

# -- Test 7: git check-ignore is NOT called inside a for loop ----------------
echo ""
echo "--- Test 7: git check-ignore is batched (not in a for loop) ---"

assert "no 'git check-ignore' calls inside for/done loops" \
    bash -c "! awk '/^for /,/^done/' '$SCRIPT' | grep -q 'git check-ignore'"

# -- Test 8: wc -l output is stripped for cross-platform portability ----------
echo ""
echo "--- Test 8: wc -l output has whitespace stripped (cross-platform) ---"

assert "script does not use bare 'wc -l)' without whitespace stripping" \
    bash -c "! grep -qE 'wc -l\)' '$SCRIPT'"

# -- Test 9: orchestrator command placement and existence guards ---------------
echo ""
echo "--- Test 9: orchestrator command placement and existence guards ---"

# S1: full-path assertion (the guard-pattern assertion below also provides full-path coverage)
assert "scripts/check-pm-standardization.sh (full path) is in lint_command" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'scripts/check-pm-standardization.sh'"

assert "check-pm-standardization.sh is NOT in test_command" \
    bash -c "! grep 'test_command:' '$ORCH' | grep -q 'check-pm-standardization.sh'"

# S2: symmetric negative assertion — test-only scripts should not be in lint_command
assert "sync_comments_test.sh is NOT in lint_command" \
    bash -c "! grep 'lint_command:' '$ORCH' | grep -q 'sync_comments_test.sh'"

assert "sync_comments_test.sh uses 'if test -f' guard in test_command" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'if test -f tests/sync_comments_test.sh'"

assert "check-pm-standardization.sh uses 'if test -f' guard in lint_command" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'if test -f scripts/check-pm-standardization.sh'"

# -- Test 10: WARNING echoes when guards trigger a skip ------------------------
echo ""
echo "--- Test 10: WARNING echoes for guard skips ---"

assert "test_command has WARNING echo for sync_comments_test.sh skip" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'WARNING.*sync_comments_test'"

assert "lint_command has WARNING echo for check-pm-standardization.sh skip" \
    bash -c "grep 'lint_command:' '$ORCH' | grep -q 'WARNING.*check-pm-standardization'"

# -- Test 11: end-to-end execution test ----------------------------------------
echo ""
echo "--- Test 11: check-pm-standardization.sh runs successfully ---"

assert "check-pm-standardization.sh runs successfully in repo context" \
    bash "$REPO_ROOT/scripts/check-pm-standardization.sh"

# -- Test 12: build artifact tracking hygiene ----------------------------------
echo ""
echo "--- Test 12: build artifact tracking hygiene ---"

# tree-sitter-reify/src/.grammar_hash.stamp is listed at .gitignore:34 but was
# previously tracked (pre-dated the gitignore entry). While tracked, every
# tree-sitter build regenerates it and dirties main's working tree, causing
# advance_main to fail with stash_failed (task 1005 blocker). The fix is
# `git rm --cached` to remove the stale index entry so the existing rule applies.
assert "tree-sitter-reify/src/.grammar_hash.stamp is NOT tracked by git" \
    bash -c "cd '$REPO_ROOT' && ! git ls-files --error-unmatch tree-sitter-reify/src/.grammar_hash.stamp >/dev/null 2>&1"

test_summary
