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

if [ -f "$HELPER_FILE" ]; then ok=true; else ok=false; fi
check "test_helpers.sh file exists" "$ok"

# -- Test (b): test_helpers.sh is sourceable -----------------------------------
echo ""
echo "--- Test b: test_helpers.sh is sourceable ---"

if bash -c "source '$HELPER_FILE'" >/dev/null 2>&1; then ok=true; else ok=false; fi
check "test_helpers.sh can be sourced without error" "$ok"

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

if bash -c "source '$HELPER_FILE' && declare -f assert >/dev/null" 2>/dev/null; then ok=true; else ok=false; fi
check "assert function is defined after sourcing" "$ok"

# -- Test (e): test_summary function is defined --------------------------------
echo ""
echo "--- Test e: test_summary function defined ---"

if bash -c "source '$HELPER_FILE' && declare -f test_summary >/dev/null" 2>/dev/null; then ok=true; else ok=false; fi
check "test_summary function is defined after sourcing" "$ok"

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
    "tests/infra/sync_ref_helpers.sh"
)

for consumer in "${CONSUMERS[@]}"; do
    cfile="$REPO_ROOT/$consumer"
    cname="$(basename "$consumer")"

    echo ""
    echo "--- Consumer: $cname ---"

    # (a) file contains 'source.*test_helpers.sh'
    if grep -qE '(source|\.)\s+.*test_helpers\.sh' "$cfile" 2>/dev/null; then ok=true; else ok=false; fi
    check "$cname sources test_helpers.sh" "$ok"

    # (b) file does NOT contain assert() function definition
    if ! grep -q '^assert()' "$cfile" 2>/dev/null; then ok=true; else ok=false; fi
    check "$cname does NOT define assert() locally" "$ok"

    # (c) file does NOT contain PASS=0 or FAIL=0 initialization
    if ! grep -qE '^PASS=0|^FAIL=0' "$cfile" 2>/dev/null; then ok=true; else ok=false; fi
    check "$cname does NOT init PASS/FAIL locally" "$ok"

    # (d) file does NOT contain inline summary block
    # Look for the echo "Results:..." pattern outside a function definition
    if ! grep -q 'echo "Results:.*passed.*failed"' "$cfile" 2>/dev/null; then ok=true; else ok=false; fi
    check "$cname does NOT have inline summary block" "$ok"

    # (e) scripts/ consumers must have a comment explaining cross-directory
    #     sourcing from tests/infra/ (gated to scripts/ consumers only)
    case "$consumer" in scripts/*)
        if grep -B3 -E '(source|\.)\s+.*test_helpers\.sh' "$cfile" 2>/dev/null \
             | grep -qi 'test script.*not.*build'; then ok=true; else ok=false; fi
        check "$cname has cross-directory sourcing comment" "$ok"
        ;;
    esac

    # (f) all consumers must have a pre-source existence guard for test_helpers.sh
    #     matching pattern: [ -f ... ] || or test -f ... ||
    if grep -E '\[ -f.*test_helpers\.sh.*\] \|\||test -f.*test_helpers\.sh.*\|\|' "$cfile" >/dev/null 2>&1; then ok=true; else ok=false; fi
    check "$cname has pre-source existence guard" "$ok"
done

# ==============================================================================
# sync_comments_test.sh refactoring structural checks
# Verify: DRY helper exists, defensive if-guards removed, head -1 documented.
# ==============================================================================

SYNC_FILE="$REPO_ROOT/tests/sync_comments_test.sh"
SYNC_REF_HELPERS_FILE="$REPO_ROOT/tests/infra/sync_ref_helpers.sh"

# File-local helpers so the structural checks and robustness tests share the
# same pattern source-of-truth and cannot drift independently.
# _has_if_n_guard detects defensive non-empty guards in all supported forms:
#   bracket variants:  [ -n ... ]  [[ -n ... ]]  test -n ...
#   negated-zero form: [ ! -z ... ]  [[ ! -z ... ]]  test ! -z ...
#   trigger keywords:  if / && / ||
# Comment lines (leading #) are stripped before matching to avoid false
# positives from explanatory comments. Split-line variants (newline between
# `if` and `[`) are not handled (grep is line-oriented; see design decisions).
# Variable names are not constrained — $marker, $fn_name, $ref_fn,
# $_expr_ref_fn, etc. all count as prohibited defensive guards.
_has_assert_sync_ref_exists() { grep -qE '^assert_sync_ref_exists\s*\(\)' "$1" 2>/dev/null; }
_has_if_n_guard() { grep -v '^[[:space:]]*#' "$1" 2>/dev/null | grep -qE '(if|&&|\|\|)[[:space:]]*(\[\[?|test)[[:space:]]+(-n|![[:space:]]+-z)'; }

echo ""
echo "--- sync_comments_test.sh structural checks ---"

# (a) file has NO defensive non-empty guard (defensive guards removed)
if ! _has_if_n_guard "$SYNC_FILE"; then ok=true; else ok=false; fi
check "sync_comments_test.sh has no defensive non-empty guard" "$ok"

# (b) extract_fn comment references actual awk pattern /^[^/]*fn/ (task-1310: 'naturally excluded' replaced)
if grep '^#' "$SYNC_FILE" 2>/dev/null | grep -qF '^[^/]*fn'; then ok=true; else ok=false; fi
check "extract_fn comment references actual awk pattern /^[^/]*fn/" "$ok"

# (c) extract_fn awk pattern is anchored with [(<] after fn_name to prevent prefix collisions
if grep -q 'fn_name.*\[(<\]' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "extract_fn awk pattern is anchored with [(<] after fn_name" "$ok"

# (d) extract_fn output is captured to a named variable before diffing (non-empty guard)
if grep -Fq 'expr_body' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "extract_fn output captured to expr_body variable" "$ok"

# (e) sync_comments_test.sh has a non-empty guard for the captured expr_body variable
if grep -Fq '[ -z "$expr_body"' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "extract_fn non-empty guard present for expr_body" "$ok"

# (f) sync_comments_test.sh sources sync_ref_helpers.sh (function moved out)
if grep -qE '(source|\.)\s+.*sync_ref_helpers\.sh' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "sync_comments_test.sh sources sync_ref_helpers.sh" "$ok"

# (g) sync_comments_test.sh does NOT define assert_sync_ref_exists() locally
if ! _has_assert_sync_ref_exists "$SYNC_FILE"; then ok=true; else ok=false; fi
check "sync_comments_test.sh does NOT define assert_sync_ref_exists() locally" "$ok"

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

# short-circuit behavioral test: when extract_fn returns empty for both bodies,
# execution should not reach the diff assertion (which would produce a spurious PASS).
echo ""
echo "--- extract_fn non-empty guard short-circuit behavioral test ---"

_sc_beh_out=$(bash -c "
    tmpdir=\$(mktemp -d)
    trap 'rm -rf \"\$tmpdir\"' EXIT
    mkdir -p \"\$tmpdir/crates/reify-expr/src\"
    mkdir -p \"\$tmpdir/crates/reify-stdlib/src\"
    mkdir -p \"\$tmpdir/tests/infra\"
    printf '// SYNC: reify-stdlib::sanitize_value\nfn renamed_function(v: i32) -> i32 {\n    v\n}\n' \
        > \"\$tmpdir/crates/reify-expr/src/sanitize.rs\"
    printf '// SYNC: reify-expr::sanitize_value\nfn renamed_function(v: i32) -> i32 {\n    v\n}\n' \
        > \"\$tmpdir/crates/reify-stdlib/src/helpers.rs\"
    cp '${HELPER_FILE}' \"\$tmpdir/tests/infra/test_helpers.sh\"
    cp '${SYNC_FILE}' \"\$tmpdir/tests/sync_comments_test.sh\"
    bash \"\$tmpdir/tests/sync_comments_test.sh\" 2>&1 || true
" 2>&1)

if ! echo "$_sc_beh_out" | grep -q 'PASS:.*body is identical'; then
    check "extract_fn non-empty guard short-circuits before spurious PASS on diff" "true"
else
    check "extract_fn non-empty guard short-circuits before spurious PASS on diff (spurious PASS found)" "false"
fi

# ==============================================================================
# sync_ref_helpers.sh structural checks
# Verify: helper file exists, defines assert_sync_ref_exists, sources
# test_helpers.sh, has source guard, head -1 documented, early-fail guard,
# display_fn fallback.
# ==============================================================================

echo ""
echo "--- sync_ref_helpers.sh structural checks ---"

# (a) file exists
if [ -f "$SYNC_REF_HELPERS_FILE" ]; then ok=true; else ok=false; fi
check "sync_ref_helpers.sh file exists" "$ok"

# (b) file defines assert_sync_ref_exists() helper function
if _has_assert_sync_ref_exists "$SYNC_REF_HELPERS_FILE"; then ok=true; else ok=false; fi
check "sync_ref_helpers.sh defines assert_sync_ref_exists()" "$ok"

# (c) file sources test_helpers.sh
if grep -qE '(source|\.)\s+.*test_helpers\.sh' "$SYNC_REF_HELPERS_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "sync_ref_helpers.sh sources test_helpers.sh" "$ok"

# (d) file has source guard (_REIFY_SYNC_REF_HELPERS_SH_SOURCED)
if grep -q '_REIFY_SYNC_REF_HELPERS_SH_SOURCED' "$SYNC_REF_HELPERS_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "sync_ref_helpers.sh has source guard (_REIFY_SYNC_REF_HELPERS_SH_SOURCED)" "$ok"

# (e) head -1 pipeline has adjacent comment documenting single-reference limitation
if grep -B3 'head -1' "$SYNC_REF_HELPERS_FILE" 2>/dev/null | grep -qiE 'first|single|multi.?reference'; then ok=true; else ok=false; fi
check "sync_ref_helpers.sh head -1 pipeline has single-reference documentation comment" "$ok"

# (f) assert_sync_ref_exists has an early-fail guard when ref_fn is empty
if grep -Fq '[ -z "$ref_fn" ]' "$SYNC_REF_HELPERS_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "sync_ref_helpers.sh has early-fail guard for empty ref_fn" "$ok"

# (g) assert_sync_ref_exists uses a display_fn fallback variable
if grep -Fq 'display_fn' "$SYNC_REF_HELPERS_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "sync_ref_helpers.sh uses display_fn fallback variable" "$ok"

# ==============================================================================
# assert_sync_ref_exists behavioral test (sourceable helper)
# Sources sync_ref_helpers.sh directly — no sed text extraction.
# S5 hardening applied from inception: rc -eq 0 AND anchored ^  FAIL: grep.
# ==============================================================================

echo ""
echo "--- assert_sync_ref_exists behavioral test (sourceable helper) ---"

_src_beh_rc=0
_src_beh_out=$(bash -c "
    tmp_src=\$(mktemp)
    tmp_tgt=\$(mktemp)
    trap 'rm -f \"\$tmp_src\" \"\$tmp_tgt\"' EXIT
    echo '// SYNC: reify-bogus::missing_fn' > \"\$tmp_src\"
    echo 'pub fn other_thing() {}' > \"\$tmp_tgt\"
    source '${SYNC_REF_HELPERS_FILE}'
    PASS=0; FAIL=0
    assert_sync_ref_exists src-crate reify-nonexistent \"\$tmp_src\" \"\$tmp_tgt\"
" 2>&1) || _src_beh_rc=$?

if [ "$_src_beh_rc" -eq 0 ]; then
    check "behavioral subshell exits cleanly (rc=0)" "true"
else
    check "behavioral subshell exits cleanly (rc=0, got rc=$_src_beh_rc)" "false"
fi

if echo "$_src_beh_out" | grep -q '^  FAIL:'; then
    check "guard fires: assert records anchored FAIL when ref_fn extraction yields nothing" "true"
else
    check "guard fires: assert records anchored FAIL when ref_fn extraction yields nothing (got: $_src_beh_out)" "false"
fi


# ==============================================================================
# Robustness tests for sync_comments_test.sh structural checks
# ==============================================================================

_robust_tmpdir=$(mktemp -d)
cleanup_robust() { rm -rf "$_robust_tmpdir"; }
trap cleanup_robust EXIT
mk_fixture() { mktemp -p "$_robust_tmpdir"; }

echo ""
echo "--- Robustness: assert_sync_ref_exists pattern tolerates whitespace ---"

for ws in '' ' ' '  ' $'\t'; do
    fixture=$(mk_fixture)
    printf 'assert_sync_ref_exists%s() {\n  : trivial body\n}\n' "$ws" > "$fixture"
    case "$ws" in
        '')     _ws_label='(empty)'   ;;
        ' ')    _ws_label='(1 space)' ;;
        '  ')   _ws_label='(2 spaces)' ;;
        $'\t')  _ws_label='(tab)'    ;;
        *)      _ws_label="(${#ws} chars)" ;;
    esac
    if _has_assert_sync_ref_exists "$fixture" 2>/dev/null; then ok=true; else ok=false; fi
    check "_has_assert_sync_ref_exists accepts whitespace variant: ${_ws_label}" "$ok"
done

echo ""
echo "--- Robustness: if-guard pattern catches defensive non-empty guards ---"

# Fixture with a guard using $ref_fn (non-underscore).
# The helper should detect this and return non-zero (guard IS present → check
# for "no guard" must be FALSE).
fixture_guard=$(mk_fixture)
printf 'if [ -n "$ref_fn" ]; then\n  echo cleanup\nfi\n' > "$fixture_guard"
if _has_if_n_guard "$fixture_guard" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects non-underscore ref variable" "$ok"

# Clean fixture with no if-guard: helper should return 0 (no guard → true).
fixture_clean=$(mk_fixture)
printf '# no guards here\necho hello\n' > "$fixture_clean"
if ! _has_if_n_guard "$fixture_clean" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard reports no-guard for clean file (no false positive)" "$ok"

# Fixture with a non-ref-named guard variable ($marker): the broadened regex
# 'if \[ -n' matches regardless of the variable name, so this guard is
# correctly detected and the helper returns non-zero.
fixture_marker=$(mk_fixture)
printf 'if [ -n "$marker" ]; then echo skip; fi\n' > "$fixture_marker"
if _has_if_n_guard "$fixture_marker" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects non-ref-named variable \$marker" "$ok"

# Historical regression pin: this fixture reproduces the exact guard that was
# removed from tests/sync_comments_test.sh in commit ff0880bfe
# ('if [ -n "$_expr_ref_fn" ]').  If a future change tightens the regex back
# to something narrower (e.g. requiring 'ref' in the variable name), this
# fixture will fail while the broader $marker test still passes, making the
# regression visible rather than silent.
fixture_historical=$(mk_fixture)
printf 'if [ -n "$_expr_ref_fn" ]; then echo skip; fi\n' > "$fixture_historical"
if _has_if_n_guard "$fixture_historical" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects historical \$_expr_ref_fn (ff0880bfe regression pin)" "$ok"

# Positive-direction mirror of the historical pin above: a legitimate early-fail
# guard using `-z` (not `-n`) must NOT be detected.  The regex uses alternation
# `(-n|![[:space:]]+-z)` specifically to allow bare `-z` guards while banning
# `-n` (and `! -z`) guards.  Without this pin a future change like `\[ -[nz]`
# would ban legitimate production guards silently while the negative-pin tests
# above all passed.
# Protected production sites:
#   - tests/infra/sync_ref_helpers.sh:31   `[ -z "$ref_fn" ]`
#   - tests/sync_comments_test.sh:63-64    `[ -z "$expr_body" ]`
fixture_z=$(mk_fixture)
printf 'if [ -z "$ref_fn" ]; then echo fail; fi\n' > "$fixture_z"
if ! _has_if_n_guard "$fixture_z" 2>/dev/null; then ok=true; else ok=false; fi
check "if-guard pattern tolerates legitimate -z early-fail guard (\$ref_fn)" "$ok"

# Double-bracket form: `if [[ -n "$var" ]]`
# Requires regex to match `[[` as well as `[`.
fixture_double_bracket=$(mk_fixture)
printf 'if [[ -n "$var" ]]; then echo guard; fi\n' > "$fixture_double_bracket"
if _has_if_n_guard "$fixture_double_bracket" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects double-bracket form: if [[ -n" "$ok"

# `test` keyword form: `if test -n "$var"`
fixture_test_keyword=$(mk_fixture)
printf 'if test -n "$var"; then echo guard; fi\n' > "$fixture_test_keyword"
if _has_if_n_guard "$fixture_test_keyword" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects test-keyword form: if test -n" "$ok"

# Negated zero-length form: `if [ ! -z "$var" ]`
# Requires regex to match `! -z` as an alternate to `-n`.
fixture_not_z=$(mk_fixture)
printf 'if [ ! -z "$var" ]; then echo guard; fi\n' > "$fixture_not_z"
if _has_if_n_guard "$fixture_not_z" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects negated zero-length form: if [ ! -z" "$ok"

# Double-bracket + negated zero-length: `if [[ ! -z "$var" ]]`
# Verifies that `[[` and `! -z` work together (combination of steps 2+5).
fixture_double_not_z=$(mk_fixture)
printf 'if [[ ! -z "$var" ]]; then echo guard; fi\n' > "$fixture_double_not_z"
if _has_if_n_guard "$fixture_double_not_z" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects double-bracket + ! -z: if [[ ! -z" "$ok"

# Comment-only file: guard pattern appears ONLY inside a comment.
# _has_if_n_guard must NOT fire on commented-out guards (false positive).
fixture_comment=$(mk_fixture)
printf '# if [ -n "$x" ]; then echo guard; fi\n' > "$fixture_comment"
if ! _has_if_n_guard "$fixture_comment" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard ignores guard pattern inside comment line" "$ok"

# Compound guard chained with &&: `something && [[ -n "$var" ]]`
# The (if|&&|\|\|) alternation must cover non-`if` trigger forms.
fixture_compound_and=$(mk_fixture)
printf 'something && [[ -n "$var" ]] && do_work\n' > "$fixture_compound_and"
if _has_if_n_guard "$fixture_compound_and" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects compound && guard: && [[ -n" "$ok"

# Self-check: file-local helpers use symmetric positive _has_ naming.
if grep -qE '^_has_assert_sync_ref_exists\(\)' "${BASH_SOURCE[0]}" \
    && grep -qE '^_has_if_n_guard\(\)' "${BASH_SOURCE[0]}" \
    && ! grep -qE '^_check_(defines|has)' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "file-local helpers use symmetric positive _has_ naming" "$ok"

# Self-check: no check() calls use 'should FAIL' in descriptions (grep-ambiguous).
if ! grep -qE 'check "[^"]*should FAIL' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "robustness check descriptions avoid ambiguous should-FAIL phrasing" "$ok"

# Self-check: no check() call descriptions are duplicated across lines.
# Duplicate descriptions (same string on the true and false branches of an
# if/else) are ambiguous in CI output.  The unified ok=true/false form
# (check "desc" "$ok") ensures each description appears exactly once.
dup_count=$(grep -E '^[[:space:]]*check "' "${BASH_SOURCE[0]}" \
    | grep -oE 'check "[^"]+"' \
    | sort | uniq -d | wc -l)
if [ "$dup_count" -eq 0 ]; then ok=true; else ok=false; fi
check "no duplicate check descriptions in this file" "$ok"

# Self-check: robustness section registers trap-based fixture cleanup.
if grep -q 'trap cleanup_robust EXIT' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "robustness section registers trap-based fixture cleanup" "$ok"

# Self-check: robustness section defines mk_fixture helper.
if grep -qE '^mk_fixture\(\)' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "robustness section defines mk_fixture helper" "$ok"

# Self-check: mk_fixture is subshell-safe.
# Appending to _robust_fixtures inside mk_fixture is silently lost when called
# via command substitution ($(...)) because that runs in a subshell.
# The temp-directory approach (mktemp -p) avoids parent-shell state mutation.
if ! grep -qE 'mk_fixture\(\).*_robust_fixtures\+=' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "mk_fixture is subshell-safe (no array append lost in command substitution)" "$ok"

# Self-check: _ws_label uses a comprehensive case statement with readable labels.
# Grep for the literal case-arm assignment; this string is absent until step-2 adds it.
if grep -q "_ws_label='(1 space)'" "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "_ws_label case statement maps single-space to readable label" "$ok"

# Self-check: defensive trap comment warns about the single main-shell EXIT trap.
# Grep for the comment marker; this string is absent until step-3 adds it.
if grep -q '# only main-shell EXIT trap' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "trap line has defensive comment about single main-shell EXIT trap invariant" "$ok"

# ==============================================================================
# Pipeline divergence documentation check
# test_helpers.sh must document that test_tree_sitter_pipeline.sh uses its own
# richer assert API and is intentionally excluded from this shared module.
# ==============================================================================

echo ""
echo "--- Pipeline divergence documented in test_helpers.sh ---"

if grep -q 'tests/infra/test_tree_sitter_pipeline.sh' "$HELPER_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "test_helpers.sh documents pipeline divergence" "$ok"

# -- Summary -------------------------------------------------------------------
echo ""
echo "Results: $T_PASS passed, $T_FAIL failed"
if [ "$T_FAIL" -gt 0 ]; then
    exit 1
fi
