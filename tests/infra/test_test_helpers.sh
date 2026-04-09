#!/usr/bin/env bash
# Unit tests for tests/infra/test_helpers.sh shared test helper module.
# Uses bare bash conditionals (not the assert function being tested) to avoid
# circular dependency.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HELPER_FILE="$SCRIPT_DIR/test_helpers.sh"

T_PASS=0
T_FAIL=0

check() {
    local desc="$1"
    local ok="$2"
    if [ "$ok" = "true" ]; then
        echo "  PASS: $desc"
        T_PASS=$((T_PASS + 1))
    else
        echo "  FAIL: $desc"
        T_FAIL=$((T_FAIL + 1))
    fi
}

echo "=== test_helpers.sh unit tests ==="

# -- Test (a): test_helpers.sh exists ------------------------------------------
echo ""
echo "--- Test a: test_helpers.sh exists ---"

if [ -f "$HELPER_FILE" ]; then
    check "test_helpers.sh file exists" "true"
else
    check "test_helpers.sh file exists" "false"
fi

# -- Test (b): test_helpers.sh is sourceable -----------------------------------
echo ""
echo "--- Test b: test_helpers.sh is sourceable ---"

if bash -c "source '$HELPER_FILE'" >/dev/null 2>&1; then
    check "test_helpers.sh can be sourced without error" "true"
else
    check "test_helpers.sh can be sourced without error" "false"
fi

# -- Test (c): PASS and FAIL initialized to 0 after sourcing ------------------
echo ""
echo "--- Test c: PASS and FAIL initialized to 0 ---"

result=$(bash -c "source '$HELPER_FILE' && echo \"\$PASS:\$FAIL\"" 2>/dev/null || echo "ERROR")
if [ "$result" = "0:0" ]; then
    check "PASS=0 and FAIL=0 after sourcing" "true"
else
    check "PASS=0 and FAIL=0 after sourcing (got: $result)" "false"
fi

# -- Test (d): assert function is defined --------------------------------------
echo ""
echo "--- Test d: assert function defined ---"

if bash -c "source '$HELPER_FILE' && declare -f assert >/dev/null" 2>/dev/null; then
    check "assert function is defined after sourcing" "true"
else
    check "assert function is defined after sourcing" "false"
fi

# -- Test (e): test_summary function is defined --------------------------------
echo ""
echo "--- Test e: test_summary function defined ---"

if bash -c "source '$HELPER_FILE' && declare -f test_summary >/dev/null" 2>/dev/null; then
    check "test_summary function is defined after sourcing" "true"
else
    check "test_summary function is defined after sourcing" "false"
fi

# -- Test (f): source guard prevents double-sourcing side effects --------------
echo ""
echo "--- Test f: source guard prevents double-sourcing ---"

# Source twice: PASS counter should still be 0 (no re-init).
# Set PASS=42 between sourcing to detect re-initialization.
result=$(bash -c "
    source '$HELPER_FILE'
    PASS=42
    source '$HELPER_FILE'
    echo \"\$PASS\"
" 2>/dev/null || echo "ERROR")
if [ "$result" = "42" ]; then
    check "source guard preserves PASS on double-source" "true"
else
    check "source guard preserves PASS on double-source (got: $result)" "false"
fi

# -- Test (g): assert increments PASS on success -------------------------------
echo ""
echo "--- Test g: assert increments PASS on success ---"

result=$(bash -c "
    source '$HELPER_FILE'
    assert 'should pass' true >/dev/null
    echo \"\$PASS\"
" 2>/dev/null || echo "ERROR")
if [ "$result" = "1" ]; then
    check "assert increments PASS on success" "true"
else
    check "assert increments PASS on success (got: $result)" "false"
fi

# -- Test (h): assert increments FAIL on failure -------------------------------
echo ""
echo "--- Test h: assert increments FAIL on failure ---"

result=$(bash -c "
    source '$HELPER_FILE'
    assert 'should fail' false >/dev/null
    echo \"\$FAIL\"
" 2>/dev/null || echo "ERROR")
if [ "$result" = "1" ]; then
    check "assert increments FAIL on failure" "true"
else
    check "assert increments FAIL on failure (got: $result)" "false"
fi

# -- Test (i): assert prints PASS/FAIL prefix ----------------------------------
echo ""
echo "--- Test i: assert prints correct prefix ---"

pass_output=$(bash -c "source '$HELPER_FILE' && assert 'my test' true" 2>/dev/null || echo "")
if echo "$pass_output" | grep -q "PASS: my test"; then
    check "assert prints 'PASS: <desc>' on success" "true"
else
    check "assert prints 'PASS: <desc>' on success (got: $pass_output)" "false"
fi

fail_output=$(bash -c "source '$HELPER_FILE' && assert 'my test' false" 2>/dev/null || echo "")
if echo "$fail_output" | grep -q "FAIL: my test"; then
    check "assert prints 'FAIL: <desc>' on failure" "true"
else
    check "assert prints 'FAIL: <desc>' on failure (got: $fail_output)" "false"
fi

# -- Test (j): test_summary exits 0 when FAIL=0 -------------------------------
echo ""
echo "--- Test j: test_summary exits 0 when no failures ---"

rc=0
bash -c "source '$HELPER_FILE' && assert 'passing' true && test_summary" >/dev/null 2>&1 || rc=$?
if [ "$rc" -eq 0 ]; then
    check "test_summary exits 0 when FAIL=0" "true"
else
    check "test_summary exits 0 when FAIL=0 (got rc=$rc)" "false"
fi

# -- Test (k): test_summary exits 1 when FAIL>0 and prints results ------------
echo ""
echo "--- Test k: test_summary exits 1 when failures present ---"

rc=0
summary_output=$(bash -c "source '$HELPER_FILE' && assert 'failing' false && test_summary" 2>/dev/null || rc=$?)
# rc should be non-zero (exit 1 from test_summary)
# Note: bash -c exits with the exit code of test_summary
rc=0
summary_output=$(bash -c "source '$HELPER_FILE' && assert 'failing' false && test_summary" 2>&1) || rc=$?
if [ "$rc" -eq 1 ]; then
    check "test_summary exits 1 when FAIL>0" "true"
else
    check "test_summary exits 1 when FAIL>0 (got rc=$rc)" "false"
fi

if echo "$summary_output" | grep -q "Results:.*passed.*failed"; then
    check "test_summary prints results line" "true"
else
    check "test_summary prints results line (got: $summary_output)" "false"
fi

# ==============================================================================
# Consumer refactoring verification tests
# Each consumer file should: source test_helpers.sh, NOT define assert() locally,
# NOT init PASS=0/FAIL=0 locally, NOT have inline summary block.
# ==============================================================================

REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONSUMERS=(
    "tests/infra/test_portable_sha256.sh"
    "tests/infra/test_portable_timeout.sh"
    "scripts/test_lib.sh"
    "scripts/test_tree_sitter_generate.sh"
    "tests/sync_comments_test.sh"
    "scripts/check-pm-standardization.sh"
)

for consumer in "${CONSUMERS[@]}"; do
    cfile="$REPO_ROOT/$consumer"
    cname="$(basename "$consumer")"

    echo ""
    echo "--- Consumer: $cname ---"

    # (a) file contains 'source.*test_helpers.sh'
    if grep -qE '(source|\.)\s+.*test_helpers\.sh' "$cfile" 2>/dev/null; then
        check "$cname sources test_helpers.sh" "true"
    else
        check "$cname sources test_helpers.sh" "false"
    fi

    # (b) file does NOT contain assert() function definition
    if grep -q '^assert()' "$cfile" 2>/dev/null; then
        check "$cname does NOT define assert() locally" "false"
    else
        check "$cname does NOT define assert() locally" "true"
    fi

    # (c) file does NOT contain PASS=0 or FAIL=0 initialization
    if grep -qE '^PASS=0|^FAIL=0' "$cfile" 2>/dev/null; then
        check "$cname does NOT init PASS/FAIL locally" "false"
    else
        check "$cname does NOT init PASS/FAIL locally" "true"
    fi

    # (d) file does NOT contain inline summary block
    # Look for the echo "Results:..." pattern outside a function definition
    if grep -q 'echo "Results:.*passed.*failed"' "$cfile" 2>/dev/null; then
        check "$cname does NOT have inline summary block" "false"
    else
        check "$cname does NOT have inline summary block" "true"
    fi

    # (e) scripts/ consumers must have a comment explaining cross-directory
    #     sourcing from tests/infra/ (gated to scripts/ consumers only)
    case "$consumer" in scripts/*)
        if grep -B3 -E '(source|\.)\s+.*test_helpers\.sh' "$cfile" 2>/dev/null \
             | grep -qi 'test script.*not.*build'; then
            check "$cname has cross-directory sourcing comment" "true"
        else
            check "$cname has cross-directory sourcing comment" "false"
        fi
        ;;
    esac

    # (f) all consumers must have a pre-source existence guard for test_helpers.sh
    #     matching pattern: [ -f ... ] || or test -f ... ||
    if grep -E '\[ -f.*test_helpers\.sh.*\] \|\||test -f.*test_helpers\.sh.*\|\|' "$cfile" >/dev/null 2>&1; then
        check "$cname has pre-source existence guard" "true"
    else
        check "$cname has pre-source existence guard" "false"
    fi
done

# ==============================================================================
# sync_comments_test.sh refactoring structural checks
# Verify: DRY helper exists, defensive if-guards removed, head -1 documented.
# ==============================================================================

SYNC_FILE="$REPO_ROOT/tests/sync_comments_test.sh"

# File-local helpers so the structural checks and robustness tests share the
# same pattern source-of-truth and cannot drift independently.
_check_defines_assert_sync_ref_exists() { grep -qE '^assert_sync_ref_exists\s*\(\)' "$1" 2>/dev/null; }
_check_has_no_ref_guard() { ! grep -qE 'if \[ -n.*ref' "$1" 2>/dev/null; }

echo ""
echo "--- sync_comments_test.sh structural checks ---"

# (a) file defines assert_sync_ref_exists() helper function
if _check_defines_assert_sync_ref_exists "$SYNC_FILE"; then
    check "sync_comments_test.sh defines assert_sync_ref_exists()" "true"
else
    check "sync_comments_test.sh defines assert_sync_ref_exists()" "false"
fi

# (b) file has NO defensive if-guard referencing ref (defensive guards removed)
if _check_has_no_ref_guard "$SYNC_FILE"; then
    check "sync_comments_test.sh has no defensive if-guard referencing ref" "true"
else
    check "sync_comments_test.sh has no defensive if-guard referencing ref" "false"
fi

# (c) head -1 pipeline has adjacent comment documenting single-reference limitation
if grep -B3 'head -1' "$SYNC_FILE" 2>/dev/null | grep -qiE 'first|single|multi.?reference'; then
    check "head -1 pipeline has single-reference documentation comment" "true"
else
    check "head -1 pipeline has single-reference documentation comment" "false"
fi

# (d) assert_sync_ref_exists has an early-fail guard when ref_fn is empty
if grep -Fq '[ -z "$ref_fn" ]' "$SYNC_FILE" 2>/dev/null; then
    check "assert_sync_ref_exists has early-fail guard for empty ref_fn" "true"
else
    check "assert_sync_ref_exists has early-fail guard for empty ref_fn" "false"
fi

# (e) assert_sync_ref_exists uses a display_fn fallback variable
if grep -Fq 'display_fn' "$SYNC_FILE" 2>/dev/null; then
    check "assert_sync_ref_exists uses display_fn fallback variable" "true"
else
    check "assert_sync_ref_exists uses display_fn fallback variable" "false"
fi

# (f) extract_fn docstring uses 'naturally excluded' wording (not the misleading 'Excludes')
if grep -q 'naturally excluded' "$SYNC_FILE" 2>/dev/null; then
    check "extract_fn docstring uses 'naturally excluded' wording" "true"
else
    check "extract_fn docstring uses 'naturally excluded' wording" "false"
fi

# (g) extract_fn awk pattern is anchored with [(<] after fn_name to prevent prefix collisions
if grep -q 'fn_name.*\[(<\]' "$SYNC_FILE" 2>/dev/null; then
    check "extract_fn awk pattern is anchored with [(<] after fn_name" "true"
else
    check "extract_fn awk pattern is anchored with [(<] after fn_name" "false"
fi

# (h) extract_fn output is captured to a named variable before diffing (non-empty guard)
if grep -Fq 'expr_body' "$SYNC_FILE" 2>/dev/null; then
    check "extract_fn output captured to expr_body variable" "true"
else
    check "extract_fn output captured to expr_body variable" "false"
fi

# (i) sync_comments_test.sh has a non-empty guard for the captured expr_body variable
if grep -Fq '[ -z "$expr_body"' "$SYNC_FILE" 2>/dev/null; then
    check "extract_fn non-empty guard present for expr_body" "true"
else
    check "extract_fn non-empty guard present for expr_body" "false"
fi

# behavioral: extract_fn returns empty output for a non-existent function name,
# confirming the non-empty guard would fire when a fn is renamed or missing.
echo ""
echo "--- extract_fn non-empty guard behavioral test ---"

_fn_beh_out=$(bash -c "
    tmp=\$(mktemp)
    printf 'fn sanitize_value(\n    v: i32,\n) -> i32 {\n    v\n}\n' > \"\$tmp\"
    source '${HELPER_FILE}'
    test_summary() { :; }
    { source '${SYNC_FILE}'; } >/dev/null 2>&1
    PASS=0; FAIL=0
    extract_fn nonexistent_fn_xyz \"\$tmp\"
    rm -f \"\$tmp\"
")

if [ -z "$_fn_beh_out" ]; then
    check "extract_fn returns empty output for non-existent function name" "true"
else
    check "extract_fn returns empty output for non-existent function name (got: $_fn_beh_out)" "false"
fi

# (j) behavioral: guard fires and records FAIL when ref_fn extraction yields nothing
echo ""
echo "--- assert_sync_ref_exists empty-ref_fn guard behavioral test ---"

_beh_out=$(bash -c "
    tmp_src=\$(mktemp)
    tmp_tgt=\$(mktemp)
    echo '// SYNC: reify-bogus::missing_fn' > \"\$tmp_src\"
    echo 'pub fn other_thing() {}' > \"\$tmp_tgt\"
    source '${HELPER_FILE}'
    test_summary() { :; }
    source '${SYNC_FILE}'
    PASS=0; FAIL=0
    assert_sync_ref_exists src-crate reify-nonexistent \"\$tmp_src\" \"\$tmp_tgt\"
    rm -f \"\$tmp_src\" \"\$tmp_tgt\"
" 2>&1)

if echo "$_beh_out" | grep -q 'FAIL'; then
    check "guard fires and records FAIL when ref_fn extraction yields nothing" "true"
else
    check "guard fires and records FAIL when ref_fn extraction yields nothing (got: $_beh_out)" "false"
fi

# ==============================================================================
# Robustness tests for sync_comments_test.sh structural checks
# ==============================================================================

echo ""
echo "--- Robustness: assert_sync_ref_exists pattern tolerates whitespace ---"

fixture=$(mktemp)
printf 'assert_sync_ref_exists () {\n  : trivial body\n}\n' > "$fixture"
if _check_defines_assert_sync_ref_exists "$fixture" 2>/dev/null; then
    check "assert_sync_ref_exists pattern accepts 'fn ()' (space before parens)" "true"
else
    check "assert_sync_ref_exists pattern accepts 'fn ()' (space before parens)" "false"
fi
rm -f "$fixture"

echo ""
echo "--- Robustness: if-guard pattern catches non-underscore ref variable ---"

# Fixture with a ref guard using a non-underscore variable (e.g. ref_fn).
# The helper should detect this and return non-zero (guard IS present → check
# for "no guard" must be FALSE).
fixture_guard=$(mktemp)
printf 'if [ -n "$ref_fn" ]; then\n  echo cleanup\nfi\n' > "$fixture_guard"
if _check_has_no_ref_guard "$fixture_guard" 2>/dev/null; then
    check "if-guard pattern detects non-underscore ref variable (should FAIL)" "false"
else
    check "if-guard pattern detects non-underscore ref variable (guard present → false)" "true"
fi
rm -f "$fixture_guard"

# Clean fixture with no if-guard: helper should return 0 (no guard → true).
fixture_clean=$(mktemp)
printf '# no guards here\necho hello\n' > "$fixture_clean"
if _check_has_no_ref_guard "$fixture_clean" 2>/dev/null; then
    check "if-guard pattern returns true for clean file (no guard)" "true"
else
    check "if-guard pattern returns true for clean file (no guard)" "false"
fi
rm -f "$fixture_clean"

# ==============================================================================
# Pipeline divergence documentation check
# test_helpers.sh must document that test_tree_sitter_pipeline.sh uses its own
# richer assert API and is intentionally excluded from this shared module.
# ==============================================================================

echo ""
echo "--- Pipeline divergence documented in test_helpers.sh ---"

if grep -q 'tests/infra/test_tree_sitter_pipeline.sh' "$HELPER_FILE" 2>/dev/null; then
    check "test_helpers.sh documents pipeline divergence" "true"
else
    check "test_helpers.sh documents pipeline divergence" "false"
fi

# -- Summary -------------------------------------------------------------------
echo ""
echo "Results: $T_PASS passed, $T_FAIL failed"
if [ "$T_FAIL" -gt 0 ]; then
    exit 1
fi
