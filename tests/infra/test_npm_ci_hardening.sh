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

assert "script references 'packageManager' in consistency logic" \
    grep -q 'packageManager' "$SCRIPT"

# -- Test 7: git check-ignore is NOT called inside a for loop ----------------
echo ""
echo "--- Test 7: git check-ignore is batched (not in a for loop) ---"

assert "bare git check-ignore (without -v) is not inside for/done loops" \
    bash -c "! awk '{sub(/^[[:space:]]+/,\"\")} /^for /,/^done/' '$SCRIPT' | grep 'git check-ignore' | grep -vq -- '-v'"

# -- Test 8: wc -l output is stripped for cross-platform portability ----------
echo ""
echo "--- Test 8: wc -l output has whitespace stripped (cross-platform) ---"

assert "script does not use bare 'wc -l)' without whitespace stripping" \
    bash -c "! grep -qE 'wc -l\)' '$SCRIPT'"

assert "script uses 'tr -d' to strip wc whitespace" \
    grep -q 'tr -d' "$SCRIPT"

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

# -- Test 13: script has rev-parse --is-inside-work-tree preflight -----------
echo ""
echo "--- Test 13: script has rev-parse --is-inside-work-tree preflight ---"

assert "script contains 'rev-parse --is-inside-work-tree' preflight" \
    grep -q 'rev-parse --is-inside-work-tree' "$SCRIPT"

# -- Test 14: script defines PKG_FILES with all three package.json paths ------
echo ""
echo "--- Test 14: script defines PKG_FILES with all three package.json paths ---"

assert "script defines PKG_FILES with all three package.json paths" \
    bash -c "grep -qE '^PKG_FILES=.*gui/package.json.*gui/sidecar/package.json.*tree-sitter-reify/package.json' '$SCRIPT'"

# -- Test 15: Check 1 for-loop iterates $PKG_FILES ----------------------------
echo ""
echo "--- Test 15: Check 1 for-loop iterates \$PKG_FILES ---"

assert "Check 1 for-loop iterates \$PKG_FILES" \
    bash -c "grep -qE 'for pkg in \\\$PKG_FILES' '$SCRIPT'"

# -- Test 16: Check 2 grep arguments expand $PKG_FILES ------------------------
echo ""
echo "--- Test 16: Check 2 grep arguments expand \$PKG_FILES ---"

assert "Check 2 grep arguments expand \$PKG_FILES" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q PKG_FILES"

# -- Test 17: Check 3 has git check-ignore -v diagnostic fallback -------------
echo ""
echo "--- Test 17: Check 3 has 'git check-ignore -v' diagnostic fallback ---"

assert "Check 3 has 'git check-ignore -v' diagnostic fallback" \
    grep -q 'git check-ignore -v' "$SCRIPT"

# -- Test 18: Check 2 subshell has defensive shell flags (task 1326) ---------
echo ""
echo "--- Test 18: Check 2 subshell enables set -euo pipefail ---"

# Without these flags, a missing package.json file silently produces a PASS
# because grep's non-zero exit inside the pipeline is masked by `tr -d` and
# the bash -c subshell does not inherit the outer script's `set -euo pipefail`.
assert "Check 2 subshell enables set -euo pipefail" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'set -euo pipefail'"

# -- Test 19: Check 2 has dual total==3 AND unique==1 assertion (task 1326) --
echo ""
echo "--- Test 19: Check 2 has pre-dedup and post-dedup count assertions ---"

# Without the pre-dedup total==3 assertion, a package.json missing the
# packageManager field would be silently accepted: grep emits 2 matching lines,
# but `sort -u` still collapses them to 1 unique line.
assert "Check 2 block computes a pre-dedup 'total' variable" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'total='"

assert "Check 2 block computes a post-dedup 'unique' variable" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q 'unique='"

assert "Check 2 asserts total equals 3 (catches missing packageManager field)" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q \"total.* = '3'\""

assert "Check 2 asserts unique equals 1 (catches version disagreement)" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -q \"unique.* = '1'\""

# -- Test 20: Check 2 has explicit file existence preflight (task 1326) ------
echo ""
echo "--- Test 20: Check 2 has explicit file existence preflight ---"

# Belt-and-braces guard: even if the subshell flags were lost, a preflight
# `[ -f "$f" ] || exit 1` loop ensures a missing file aborts the subshell
# before grep is invoked.
assert "Check 2 block has an explicit '[ -f ...' file existence check" \
    bash -c "awk '/Check 2:/,/Check 3:/' '$SCRIPT' | grep -qE '\\[[[:space:]]*-f'"

# -- Test 21: Check 2 logic behavioral verification with fixtures (task 1326) -
echo ""
echo "--- Test 21: Check 2 logic rejects both bug scenarios ---"

# These tests reproduce the Check 2 subshell logic exactly and run it against
# mktemp fixture directories, independent of the real repo paths. This proves
# the pattern catches both regression modes described in task 1326:
#   bug 1: file missing the packageManager field → total==2, unique==1 → fail
#   bug 2: file missing entirely → preflight / set -euo pipefail → fail
FIX_DIR="$(mktemp -d)"
trap 'rm -rf "${FIX_DIR:?}"' EXIT

CHECK2_HELPER="$FIX_DIR/check2_logic.sh"
cat > "$CHECK2_HELPER" <<'CHECK2EOF'
#!/usr/bin/env bash
# Mirror of scripts/check-pm-standardization.sh Check 2 subshell body.
set -euo pipefail
for f in "$@"; do
    [ -f "$f" ] || exit 1
done
total=$(grep -ohE '"packageManager"\s*:\s*"[^"]+"' "$@" | wc -l | tr -d ' ')
unique=$(grep -ohE '"packageManager"\s*:\s*"[^"]+"' "$@" | sort -u | wc -l | tr -d ' ')
[ "$total" = '3' ] && [ "$unique" = '1' ]
CHECK2EOF
chmod +x "$CHECK2_HELPER"

# Case A: three files present, all agree → PASS
mkdir -p "$FIX_DIR/case_a"
for i in 1 2 3; do
    printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_a/p${i}.json"
done
assert "Check 2 logic accepts three files all agreeing on packageManager" \
    "$CHECK2_HELPER" "$FIX_DIR/case_a/p1.json" "$FIX_DIR/case_a/p2.json" "$FIX_DIR/case_a/p3.json"

# Case B: one file missing the packageManager field → FAIL (bug 1)
mkdir -p "$FIX_DIR/case_b"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_b/p1.json"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_b/p2.json"
printf '{\n  "name": "no-pm-field"\n}\n' > "$FIX_DIR/case_b/p3.json"
assert "Check 2 logic rejects a file missing the packageManager field (bug 1)" \
    bash -c "! '$CHECK2_HELPER' '$FIX_DIR/case_b/p1.json' '$FIX_DIR/case_b/p2.json' '$FIX_DIR/case_b/p3.json'"

# Case C: one file does not exist on disk → FAIL (bug 2)
mkdir -p "$FIX_DIR/case_c"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_c/p1.json"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_c/p2.json"
# case_c/p3.json intentionally not created
assert "Check 2 logic rejects a missing package.json file (bug 2)" \
    bash -c "! '$CHECK2_HELPER' '$FIX_DIR/case_c/p1.json' '$FIX_DIR/case_c/p2.json' '$FIX_DIR/case_c/p3.json'"

# Case D: three files with differing packageManager versions → FAIL
mkdir -p "$FIX_DIR/case_d"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_d/p1.json"
printf '{\n  "packageManager": "npm@10.0.0"\n}\n' > "$FIX_DIR/case_d/p2.json"
printf '{\n  "packageManager": "npm@10.1.0"\n}\n' > "$FIX_DIR/case_d/p3.json"
assert "Check 2 logic rejects differing packageManager versions" \
    bash -c "! '$CHECK2_HELPER' '$FIX_DIR/case_d/p1.json' '$FIX_DIR/case_d/p2.json' '$FIX_DIR/case_d/p3.json'"

test_summary
