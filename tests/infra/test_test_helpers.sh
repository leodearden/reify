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
# Test: assert dumps captured output on FAIL (evidence preservation)
# esc-4959-57: assert() historically discarded the asserted command's
# stdout/stderr entirely (`>/dev/null 2>&1`), so a failing infra-test left
# zero diagnostic evidence in the archived verify log (task 4965's quoting
# bug burned ~20h for exactly this reason). These checks pin the fix: on
# FAIL, the captured output must be dumped after the byte-identical
# "  FAIL: <desc>" line; on PASS, output stays byte-identical (no dump,
# proving captured output is swallowed rather than printed); and the
# no-subshell invariant (a checker's parent-shell global mutation survives
# assert) must hold throughout, since asserted checker functions run in the
# parent shell and some mutate parent-shell globals (e.g. the offline
# suite's _OFFLINE_PLAN_CACHE memoization).
# ==============================================================================

echo ""
echo "--- Test: assert dumps captured output on FAIL (evidence preservation) ---"

# Sub-checks 1-3: a failing assert must dump the asserted command's captured
# stdout+stderr instead of silently discarding it, while the "  FAIL: <desc>"
# line itself stays byte-identical (parsed by test_run_all.sh / dark-factory's
# cause_hint). RED on base -- assert currently redirects to /dev/null.
_eb_fail_out=$(bash -c "
    source '$HELPER_FILE'
    _eb_boom() { printf 'EVIDENCE_NEEDLE_%s\n' ABC >&2; return 1; }
    assert 'boom desc' _eb_boom
" 2>&1 || true)

if printf '%s\n' "$_eb_fail_out" | grep -qF '  FAIL: boom desc'; then
    check "FAIL line stays byte-identical when a dump follows" "true"
else
    check "FAIL line stays byte-identical when a dump follows (got: $_eb_fail_out)" "false"
fi

if printf '%s\n' "$_eb_fail_out" | grep -qF 'EVIDENCE_NEEDLE_ABC'; then
    check "assert dumps the failing command's captured stderr needle on FAIL" "true"
else
    check "assert dumps the failing command's captured stderr needle on FAIL (got: $_eb_fail_out)" "false"
fi

if printf '%s\n' "$_eb_fail_out" | grep -qF 'assert: captured output'; then
    check "assert FAIL dump has a stable dump marker (assert: captured output)" "true"
else
    check "assert FAIL dump has a stable dump marker (assert: captured output) (got: $_eb_fail_out)" "false"
fi

# Sub-check 4: byte-identical-green guard -- a PASSING assert's output must
# stay EXACTLY "  PASS: <desc>" with no dump, even though the passing command
# also wrote to stderr (proving the captured output is swallowed into the
# tmpfile, not printed, on the PASS branch).
_eb_pass_out=$(bash -c "
    source '$HELPER_FILE'
    _eb_ok() { printf X >&2; return 0; }
    assert 'ok desc' _eb_ok
" 2>&1 || true)

if [ "$_eb_pass_out" = "  PASS: ok desc" ]; then
    check "PASS output is byte-identical to '  PASS: <desc>' (no dump on PASS)" "true"
else
    check "PASS output is byte-identical to '  PASS: <desc>' (no dump on PASS) (got: $_eb_pass_out)" "false"
fi

# Sub-check 5: no-subshell invariant -- a checker fn that mutates a
# parent-shell global and returns 1 must leave that mutation visible after
# assert returns, proving assert executes "$@" in-shell (redirect only) and
# never wraps it in a command-substitution subshell (which would fork and
# discard the mutation).
_eb_subshell_result=$(bash -c "
    source '$HELPER_FILE'
    _eb_mut_global=0
    _eb_mutator() { _eb_mut_global=1; return 1; }
    assert 'mutator desc' _eb_mutator >/dev/null 2>&1
    echo \"\$_eb_mut_global\"
" 2>/dev/null || echo "ERROR")

if [ "$_eb_subshell_result" = "1" ]; then
    check "no-subshell invariant: checker's parent-shell global mutation survives assert" "true"
else
    check "no-subshell invariant: checker's parent-shell global mutation survives assert (got: $_eb_subshell_result)" "false"
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
    "scripts/test_pm_standardization.sh"
    "tests/infra/sync_ref_helpers.sh"
)

for consumer in "${CONSUMERS[@]}"; do
    cfile="$REPO_ROOT/$consumer"
    cname="$(basename "$consumer")"

    echo ""
    echo "--- Consumer: $cname ---"

    # (a) file contains 'source.*test_helpers.sh'
    if grep -qE '(source|\.)[[:space:]]+.*test_helpers\.sh' "$cfile" 2>/dev/null; then ok=true; else ok=false; fi
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
        if grep -B3 -E '(source|\.)[[:space:]]+.*test_helpers\.sh' "$cfile" 2>/dev/null \
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
# _has_expr_body_empty_guard_short_circuit checks that the empty-guard for
# expr_body short-circuits via test_summary on the same line. NOTE: if the
# guard is ever reformatted to span multiple lines, this per-line grep will
# need to be replaced with an awk-based multiline matcher.
_has_assert_sync_ref_exists() { grep -qE '^assert_sync_ref_exists[[:space:]]*\(\)' "$1" 2>/dev/null; }
_has_if_n_guard() { grep -v '^[[:space:]]*#' "$1" 2>/dev/null | grep -qE '(if|&&|\|\|)[[:space:]]*(\[\[?|test)[[:space:]]+(-n|![[:space:]]+-z)'; }
_has_expr_body_empty_guard_short_circuit() { grep -qE '\[ -z "\$expr_body".*test_summary' "$1" 2>/dev/null; }

# Meta-helper: extract every `^_has_[a-z_]+()` definition from $1 and print
# the names of any that have no call site (i.e., the name appears on only
# the definition line).  Uses word-boundary matching
# `(^|[^[:alnum:]_])NAME([^[:alnum:]_]|$)` so that prefix-overlapping names
# (e.g., `_has_foo` vs `_has_foo_bar`) are not counted as callers of each
# other.  Counting is done with `grep -c` (matching lines); each definition
# contributes exactly 1 line, so `< 2` means "no call site".
#
# Named `_unused_has_helpers` — NOT `_has_*` — because it is a computation
# over helper definitions, not a structural content-checker.  Reserving the
# `_has_*` prefix for content checkers keeps the dynamic self-check's
# enumeration well-defined (it operates on content checkers, not on itself).
_unused_has_helpers() {
    local file="$1"
    local names name count
    names=$(grep -oE '^_has_[a-z_]+\(\)' "$file" 2>/dev/null | sed 's/()$//')
    [ -z "$names" ] && return 0
    for name in $names; do
        count=$(grep -cE "(^|[^[:alnum:]_])${name}([^[:alnum:]_]|\$)" "$file" 2>/dev/null || echo 0)
        if [ "$count" -lt 2 ]; then
            printf '%s\n' "$name"
        fi
    done
}

echo ""
echo "--- sync_comments_test.sh structural checks ---"

# (a) file has NO defensive non-empty guard (defensive guards removed)
if ! _has_if_n_guard "$SYNC_FILE"; then ok=true; else ok=false; fi
check "sync_comments_test.sh has no defensive non-empty guard" "$ok"

# (b) extract_fn comment describes the actual broad awk pattern modifier prefixes
#     (task-1309: broadened from /^[^/]*fn/ to mirror assert_sync_ref_exists regex)
if grep '^#' "$SYNC_FILE" 2>/dev/null | grep -qF 'Allowed prefixes'; then ok=true; else ok=false; fi
check "extract_fn comment describes allowed prefixes for broad awk pattern" "$ok"

# (c) extract_fn awk pattern is anchored with [[:space:](<] after fn_name to prevent prefix collisions
if grep -q 'fn_name.*\[\[:space:\](<\]' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "extract_fn awk pattern is anchored with [[:space:](<] after fn_name" "$ok"

# (d) extract_fn output is captured to a named variable before diffing (non-empty guard)
if grep -Fq 'expr_body' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "extract_fn output captured to expr_body variable" "$ok"

# (e) sync_comments_test.sh has a non-empty guard for the captured expr_body variable
if grep -Fq '[ -z "$expr_body"' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "extract_fn non-empty guard present for expr_body" "$ok"

# (e2) sync_comments_test.sh empty-guard short-circuits via test_summary before diff
# WHY: check (e) only confirms the guard exists; it does NOT confirm the guard
# short-circuits.  Without test_summary; inside the guard's braces, a failed
# assert still records a FAIL but execution falls through to the diff assertion.
# On empty expr_body, diff <(printf '') <(printf '') returns rc=0, masking the
# regression with a spurious PASS.  This structural check is the fast pre-flight
# counterpart to the expensive behavioral test at the
# "extract_fn non-empty guard short-circuit behavioral test" section below.
if _has_expr_body_empty_guard_short_circuit "$SYNC_FILE"; then ok=true; else ok=false; fi
check "extract_fn empty-guard short-circuits via test_summary for expr_body" "$ok"

# (f) sync_comments_test.sh sources sync_ref_helpers.sh (function moved out)
if grep -qE '(source|\.)[[:space:]]+.*sync_ref_helpers\.sh' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "sync_comments_test.sh sources sync_ref_helpers.sh" "$ok"

# (g) sync_comments_test.sh does NOT define assert_sync_ref_exists() locally
if ! _has_assert_sync_ref_exists "$SYNC_FILE"; then ok=true; else ok=false; fi
check "sync_comments_test.sh does NOT define assert_sync_ref_exists() locally" "$ok"

# (h) source call for test_helpers.sh has || error-handler attached
if grep -Fq 'source "$REPO_ROOT/tests/infra/test_helpers.sh" || { echo "ERROR: failed to source test_helpers.sh"; exit 1; }' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "source test_helpers.sh has || error-handler attached" "$ok"

# (i) source call for sync_ref_helpers.sh has || error-handler attached
if grep -Fq 'source "$REPO_ROOT/tests/infra/sync_ref_helpers.sh" || { echo "ERROR: failed to source sync_ref_helpers.sh"; exit 1; }' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "source sync_ref_helpers.sh has || error-handler attached" "$ok"

# (j) EXPR_FILE existence guard present before assert calls
if grep -Fq '[ -f "$EXPR_FILE" ] || { echo "ERROR: $EXPR_FILE not found"; exit 1; }' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "sync_comments_test.sh has EXPR_FILE existence guard" "$ok"

# (k) STDLIB_FILE existence guard present before assert calls
if grep -Fq '[ -f "$STDLIB_FILE" ] || { echo "ERROR: $STDLIB_FILE not found"; exit 1; }' "$SYNC_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "sync_comments_test.sh has STDLIB_FILE existence guard" "$ok"

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
# test_helpers.sh, has source guard, head -1 documented, early-fail guard.
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
if grep -qE '(source|\.)[[:space:]]+.*test_helpers\.sh' "$SYNC_REF_HELPERS_FILE" 2>/dev/null; then ok=true; else ok=false; fi
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

# ==============================================================================
# assert_sync_ref_exists behavioral test (sourceable helper)
# Sources sync_ref_helpers.sh directly — no sed text extraction.
# S3+S5 hardening: bash -eu catches unset-var/missing-cmd regressions via rc;
# anchored PASS/FAIL greps verify assertion output.
# ==============================================================================

echo ""
echo "--- assert_sync_ref_exists behavioral test (sourceable helper) ---"

_src_beh_rc=0
_src_beh_out=$(bash -eu -c "
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

# happy-path: SYNC comment references a fn that exists in target file → PASS
_src_beh_happy_rc=0
_src_beh_happy_out=$(bash -eu -c "
    tmp_src=\$(mktemp)
    tmp_tgt=\$(mktemp)
    trap 'rm -f \"\$tmp_src\" \"\$tmp_tgt\"' EXIT
    echo '// SYNC: mirror of reify-bogus::some_fn' > \"\$tmp_src\"
    echo 'pub fn some_fn() {}' > \"\$tmp_tgt\"
    source '${SYNC_REF_HELPERS_FILE}'
    PASS=0; FAIL=0
    assert_sync_ref_exists src-crate reify-bogus \"\$tmp_src\" \"\$tmp_tgt\"
" 2>&1) || _src_beh_happy_rc=$?

if [ "$_src_beh_happy_rc" -eq 0 ]; then
    check "happy-path subshell exits cleanly (rc=0)" "true"
else
    check "happy-path subshell exits cleanly (rc=0, got rc=$_src_beh_happy_rc)" "false"
fi

if echo "$_src_beh_happy_out" | grep -q '^  PASS:'; then
    check "happy-path: assert records anchored PASS when referenced fn exists in target" "true"
else
    check "happy-path: assert records anchored PASS when referenced fn exists in target (got: $_src_beh_happy_out)" "false"
fi

# mismatch-path: SYNC comment references a fn that does NOT exist in target → FAIL
_src_beh_mismatch_rc=0
_src_beh_mismatch_out=$(bash -eu -c "
    tmp_src=\$(mktemp)
    tmp_tgt=\$(mktemp)
    trap 'rm -f \"\$tmp_src\" \"\$tmp_tgt\"' EXIT
    echo '// SYNC: mirror of reify-bogus::expected_fn' > \"\$tmp_src\"
    echo 'pub fn different_fn() {}' > \"\$tmp_tgt\"
    source '${SYNC_REF_HELPERS_FILE}'
    PASS=0; FAIL=0
    assert_sync_ref_exists src-crate reify-bogus \"\$tmp_src\" \"\$tmp_tgt\"
" 2>&1) || _src_beh_mismatch_rc=$?

if [ "$_src_beh_mismatch_rc" -eq 0 ]; then
    check "mismatch-path subshell exits cleanly (rc=0)" "true"
else
    check "mismatch-path subshell exits cleanly (rc=0, got rc=$_src_beh_mismatch_rc)" "false"
fi

if echo "$_src_beh_mismatch_out" | grep -q '^  FAIL:'; then
    check "mismatch-path: assert records anchored FAIL when referenced fn absent from target" "true"
else
    check "mismatch-path: assert records anchored FAIL when referenced fn absent from target (got: $_src_beh_mismatch_out)" "false"
fi

if echo "$_src_beh_mismatch_out" | grep '^  FAIL:' | grep -q 'expected_fn'; then
    check "mismatch-path FAIL message names the missing fn (fn-existence path, not guard path)" "true"
else
    check "mismatch-path FAIL message names the missing fn (fn-existence path, not guard path) (got: $_src_beh_mismatch_out)" "false"
fi

# ==============================================================================
# sync_ref_helpers.sh sourceable-failure test (S5)
# Verify: sourcing the helper when test_helpers.sh is absent does NOT kill the
# caller's shell (i.e., uses return 1 rather than exit 1 on failure).
# ==============================================================================

echo ""
echo "--- sync_ref_helpers.sh sourceable-failure test (S5) ---"

_s5_tmp_dir=$(mktemp -d)
cp "$SYNC_REF_HELPERS_FILE" "$_s5_tmp_dir/sync_ref_helpers.sh"
# Deliberately do NOT copy test_helpers.sh — we want the helper to hit the
# "ERROR: test_helpers.sh not found" branch.
_s5_out=$(bash -c "source '$_s5_tmp_dir/sync_ref_helpers.sh' 2>&1; echo CALLER_SURVIVED" 2>&1) || true
rm -rf "$_s5_tmp_dir"

# Use bash-native substring matching (`[[ == *substr* ]]`) rather than
# `echo "$_s5_out" | grep -q`: the pipe-to-grep form forks a subshell and a
# grep that read from a pipe, and under heavy concurrent test load that grep
# can transiently fail (broken pipe / EINTR) and return non-zero EVEN WHEN the
# content matches — which silently flips this check to its else branch and
# produces a spurious FAIL (observed in esc-4574-42: the got: output plainly
# contained the expected string yet the grep "missed" it). Native matching
# does no fork and no pipe, so the assertion is purely a function of $_s5_out.
if [[ "$_s5_out" == *CALLER_SURVIVED* ]]; then
    check "S5: caller shell survives source-time failure (return 1 not exit 1)" "true"
else
    check "S5: caller shell survives source-time failure (return 1 not exit 1) (got: $_s5_out)" "false"
fi

if [[ "$_s5_out" == *"ERROR: test_helpers.sh not found"* ]]; then
    check "S5: error diagnostic still emitted when test_helpers.sh is absent" "true"
else
    check "S5: error diagnostic still emitted when test_helpers.sh is absent (got: $_s5_out)" "false"
fi

# ==============================================================================
# Robustness tests for sync_comments_test.sh structural checks
# ==============================================================================

_robust_tmpdir=$(mktemp -d)
cleanup_robust() { rm -rf "$_robust_tmpdir"; }
# only main-shell EXIT trap in this file — earlier EXIT traps are inside
# `bash -c` subshells of the "extract_fn non-empty guard short-circuit
# behavioral test" and "assert_sync_ref_exists behavioral test (sourceable
# helper)" sections, and do not affect this scope.  If you need a second
# main-shell trap, use `trap -p EXIT` stacking instead of replacing this.
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

echo ""
echo "--- Robustness: empty-guard short-circuit pattern ---"

# Negative fixture: guard WITHOUT test_summary; — helper must return non-zero.
# This reproduces the exact regression the new check is designed to catch:
# the guard is present but does not short-circuit, so execution falls through
# to the diff assertion, producing a spurious PASS on empty expr_body.
fixture_no_summary=$(mk_fixture)
printf '[ -z "$expr_body" ] && { assert "extract_fn sanitize_value found in reify-expr" false; }\n' \
    > "$fixture_no_summary"
if ! _has_expr_body_empty_guard_short_circuit "$fixture_no_summary" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_expr_body_empty_guard_short_circuit rejects guard without test_summary" "$ok"

# Positive fixture: guard WITH test_summary; — helper must return zero.
# Confirms the helper does not false-positive on a correctly written guard.
fixture_with_summary=$(mk_fixture)
printf '[ -z "$expr_body" ] && { assert "extract_fn sanitize_value found in reify-expr" false; test_summary; }\n' \
    > "$fixture_with_summary"
if _has_expr_body_empty_guard_short_circuit "$fixture_with_summary" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_expr_body_empty_guard_short_circuit accepts guard with test_summary" "$ok"

# Positive-direction mirror of the historical pin above: a legitimate early-fail
# guard using `-z` (not `-n`) must NOT be detected.  The regex uses alternation
# `(-n|![[:space:]]+-z)` specifically to allow bare `-z` guards while banning
# `-n` (and `! -z`) guards.  Without this pin a future change like `\[ -[nz]`
# would ban legitimate production guards silently while the negative-pin tests
# above all passed.
# Protected production sites — two independent mechanisms:
#   Protected by -z alternation (trigger keyword present, bare -z tolerated):
#     - tests/infra/sync_ref_helpers.sh:31  `if [ -z "$ref_fn" ]; then ...; fi`
#       Has `if` trigger, so regex fires; only the (-n|! -z) alternation saves it.
#   Protected by trigger-keyword constraint (no trigger before `[`):
#     - tests/sync_comments_test.sh:75-76   `[ -z "$expr_body" ] && { ...; }`
#       Starts with `[`, no preceding if/&&/||, so regex never matches regardless
#       of -z vs -n.  (Line 63 in that file is an unrelated body comment.)
fixture_z=$(mk_fixture)
printf 'if [ -z "$ref_fn" ]; then echo fail; fi\n' > "$fixture_z"
if ! _has_if_n_guard "$fixture_z" 2>/dev/null; then ok=true; else ok=false; fi
check "if-guard pattern tolerates legitimate -z early-fail guard (\$ref_fn)" "$ok"

# Positive-direction -z pin: double-bracket form `if [[ -z "$var" ]]`.
# If the regex were tightened to `\[\[?[[:space:]]+-[nz]`, it would ban this
# legitimate guard too.  This pin ensures double-bracket -z stays tolerated.
fixture_z_double=$(mk_fixture)
printf 'if [[ -z "$var" ]]; then echo fail; fi\n' > "$fixture_z_double"
if ! _has_if_n_guard "$fixture_z_double" 2>/dev/null; then ok=true; else ok=false; fi
check "if-guard pattern tolerates -z in double-bracket form: if [[ -z" "$ok"

# Positive-direction -z pin: test-keyword form `if test -z "$var"`.
# If the regex were tightened to `test[[:space:]]+-[nz]`, it would ban this
# legitimate guard.  This pin ensures test-keyword -z stays tolerated.
fixture_z_test=$(mk_fixture)
printf 'if test -z "$var"; then echo fail; fi\n' > "$fixture_z_test"
if ! _has_if_n_guard "$fixture_z_test" 2>/dev/null; then ok=true; else ok=false; fi
check "if-guard pattern tolerates -z in test-keyword form: if test -z" "$ok"

# Positive-direction -z pin: compound && single-bracket form
# `something && [ -z "$var" ] && do_work`.
# The (if|&&|\|\|) trigger comes BEFORE the bracket here, but the -z
# alternation still protects this guard.  Pin ensures compound-&& -z
# stays tolerated.
fixture_z_and=$(mk_fixture)
printf 'something && [ -z "$var" ] && do_work\n' > "$fixture_z_and"
if ! _has_if_n_guard "$fixture_z_and" 2>/dev/null; then ok=true; else ok=false; fi
check "if-guard pattern tolerates compound && -z: && [ -z" "$ok"

# Positive-direction -z pin: compound || single-bracket form
# `something || [ -z "$var" ] && do_work`.
# Mirrors the && pin above but with || trigger, covering the third
# trigger-keyword variant in the (if|&&|\|\|) alternation.
fixture_z_or=$(mk_fixture)
printf 'something || [ -z "$var" ] && do_work\n' > "$fixture_z_or"
if ! _has_if_n_guard "$fixture_z_or" 2>/dev/null; then ok=true; else ok=false; fi
check "if-guard pattern tolerates compound || -z: || [ -z" "$ok"

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

# Compound guard chained with ||: `something || [ -n "$var" ] && do_work`
# The (if|&&|\|\|) alternation must also cover the || trigger form.
fixture_compound_or=$(mk_fixture)
printf 'something || [ -n "$var" ] && do_work\n' > "$fixture_compound_or"
if _has_if_n_guard "$fixture_compound_or" 2>/dev/null; then ok=true; else ok=false; fi
check "_has_if_n_guard detects compound || guard: || [ -n" "$ok"

# ------------------------------------------------------------------------------
# Robustness: _unused_has_helpers dynamic self-check meta-helper
# ------------------------------------------------------------------------------
# These fixtures exercise the extraction+counting logic in isolation so a
# regression in _unused_has_helpers is caught by a dedicated failure rather
# than by a silent pass in the file-level self-check below.

echo ""
echo "--- Robustness: _unused_has_helpers meta-helper ---"

# Fixture: helper defined and called on another line → reported as used
# (_unused_has_helpers prints nothing).
fixture_used=$(mk_fixture)
printf '_has_foo() { :; }\n_has_foo "$1"\n' > "$fixture_used"
if [ -z "$(_unused_has_helpers "$fixture_used" 2>/dev/null)" ]; then ok=true; else ok=false; fi
check "_unused_has_helpers reports empty when every helper has a call site" "$ok"

# Fixture: helper defined but never called → name is printed.
fixture_unused=$(mk_fixture)
printf '_has_foo() { :; }\necho unrelated\n' > "$fixture_unused"
if [ "$(_unused_has_helpers "$fixture_unused" 2>/dev/null)" = "_has_foo" ]; then ok=true; else ok=false; fi
check "_unused_has_helpers reports a defined-but-uncalled helper name" "$ok"

# Fixture: prefix-overlapping names.  `_has_foo` is defined and ONLY
# `_has_foo_bar` is referenced on a second line; word-boundary matching
# must NOT count that as a call to `_has_foo`, so `_has_foo` is reported
# as unused.  Guards against a naive `grep -F`-style implementation.
fixture_prefix=$(mk_fixture)
printf '_has_foo() { :; }\n_has_foo_bar "$1"\n' > "$fixture_prefix"
if [ "$(_unused_has_helpers "$fixture_prefix" 2>/dev/null)" = "_has_foo" ]; then ok=true; else ok=false; fi
check "_unused_has_helpers uses word boundaries (prefix collision immune)" "$ok"

# Fixture: no `_has_*` helpers defined at all → empty output.  Exercises
# the early-return path where extraction finds nothing.
fixture_none=$(mk_fixture)
printf 'echo just some script\n' > "$fixture_none"
if [ -z "$(_unused_has_helpers "$fixture_none" 2>/dev/null)" ]; then ok=true; else ok=false; fi
check "_unused_has_helpers returns empty when no _has_* helpers are defined" "$ok"

# Self-check: every file-local _has_* helper is used at least once.
# Dynamic replacement for the former enumerated AND-chained grep list —
# when a new _has_* helper is added, this invariant auto-discovers it
# (no manual self-check update required).
unused_helpers=$(_unused_has_helpers "${BASH_SOURCE[0]}" 2>/dev/null)
if [ -z "$unused_helpers" ]; then
    ok=true
else
    ok=false
fi
check "every file-local _has_* helper has a call site (unused: ${unused_helpers:-none})" "$ok"

# Self-check: file defines at least 3 _has_* helpers.  Independent guard
# against a silent-pass regression in _unused_has_helpers — if extraction
# were ever broken to produce no names, the "no unused" check above would
# vacuously pass.  This asserts the floor so that a broken extraction
# shows up as an explicit failure rather than a silent green.
_has_helper_count=$(grep -cE '^_has_[a-z_]+\(\)' "${BASH_SOURCE[0]}" 2>/dev/null || echo 0)
if [ "$_has_helper_count" -ge 3 ]; then
    ok=true
else
    ok=false
fi
check "file defines >= 3 _has_* helpers (floor guard for dynamic self-check, got $_has_helper_count)" "$ok"

# Self-check: no legacy _check_defines / _check_has helper naming.
# Preserved from the prior enumerated self-check as an independent
# anti-pattern guard — the dynamic _unused_has_helpers check above does
# not cover this, so keep it as a separate assertion.
if ! grep -qE '^_check_(defines|has)' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "file has no legacy _check_defines / _check_has helper naming" "$ok"

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

# Self-check: all behavioral assert_sync_ref_exists subshells use bash with -eu flag.
# Count subshell-initiating lines (_src_beh_*_out assignments using bash with strict mode).
# Must equal 3 (guard, happy-path, mismatch-path).
_eu_flag="-eu"
_beh_eu_count=$(grep -cE "_src_beh.*_out=\\\$\(bash ${_eu_flag} -c" "${BASH_SOURCE[0]}" || true)
if [ "$_beh_eu_count" -eq 3 ]; then
    ok=true
else
    ok=false
fi
check "all 3 behavioral subshells use bash -eu -c (S3 hardening, got $_beh_eu_count)" "$ok"

# Self-check: _ws_label uses a comprehensive case statement with readable labels.
# Grep for the literal case-arm assignment to verify the readable-label mapping exists.
if grep -q "_ws_label='(1 space)'" "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "_ws_label case statement maps single-space to readable label" "$ok"

# Self-check: defensive trap comment warns about the single main-shell EXIT trap.
# Grep for the comment marker to verify the defensive trap comment exists.
if grep -q '# only main-shell EXIT trap' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "trap line has defensive comment about single main-shell EXIT trap invariant" "$ok"

# Self-check: no self-check comment contains stale 'absent until step-N adds it' phrasing.
if ! grep -qE 'absent until step-[23] adds it' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "self-check comments contain no stale 'absent until step-N adds it' phrasing" "$ok"

# Self-check: defensive trap comment has no drifting 'lines ~NNN' references.
if ! grep -qE 'lines [~][34][0-9]{2}' "${BASH_SOURCE[0]}"; then
    ok=true
else
    ok=false
fi
check "defensive trap comment has no drifting 'lines ~NNN' references" "$ok"

echo ""
echo "--- Robustness: EXPR_FILE guard fires when reify-expr source file absent ---"

_expr_guard_beh_dir=$(mktemp -d -p "$_robust_tmpdir")
mkdir -p "$_expr_guard_beh_dir/tests"
cp "$SYNC_FILE" "$_expr_guard_beh_dir/tests/sync_comments_test.sh"
_expr_guard_beh_rc=0
_expr_guard_beh_out=$(bash "$_expr_guard_beh_dir/tests/sync_comments_test.sh" 2>&1) || _expr_guard_beh_rc=$?

if [ "$_expr_guard_beh_rc" -ne 0 ]; then ok=true; else ok=false; fi
check "EXPR_FILE guard: exits non-zero when reify-expr source file absent" "$ok"

if echo "$_expr_guard_beh_out" | grep -q 'ERROR:'; then ok=true; else ok=false; fi
check "EXPR_FILE guard: output contains ERROR:" "$ok"

if echo "$_expr_guard_beh_out" | grep -q 'reify-expr'; then ok=true; else ok=false; fi
check "EXPR_FILE guard: error message names reify-expr path" "$ok"

echo ""
echo "--- Robustness: STDLIB_FILE guard fires when reify-stdlib source file absent ---"

_stdlib_guard_beh_dir=$(mktemp -d -p "$_robust_tmpdir")
mkdir -p "$_stdlib_guard_beh_dir/crates/reify-expr/src"
mkdir -p "$_stdlib_guard_beh_dir/tests"
printf '// SYNC: reify-stdlib::sanitize_value\nfn stub() {}\n' \
    > "$_stdlib_guard_beh_dir/crates/reify-expr/src/sanitize.rs"
cp "$SYNC_FILE" "$_stdlib_guard_beh_dir/tests/sync_comments_test.sh"
_stdlib_guard_beh_rc=0
_stdlib_guard_beh_out=$(bash "$_stdlib_guard_beh_dir/tests/sync_comments_test.sh" 2>&1) || _stdlib_guard_beh_rc=$?

if [ "$_stdlib_guard_beh_rc" -ne 0 ]; then ok=true; else ok=false; fi
check "STDLIB_FILE guard: exits non-zero when reify-stdlib source file absent" "$ok"

if echo "$_stdlib_guard_beh_out" | grep -q 'ERROR:'; then ok=true; else ok=false; fi
check "STDLIB_FILE guard: output contains ERROR:" "$ok"

if echo "$_stdlib_guard_beh_out" | grep -q 'reify-stdlib'; then ok=true; else ok=false; fi
check "STDLIB_FILE guard: error message names reify-stdlib path" "$ok"

# ==============================================================================
# Pipeline divergence documentation check
# test_helpers.sh must document that test_tree_sitter_pipeline.sh uses its own
# richer assert API and is intentionally excluded from this shared module.
# ==============================================================================

echo ""
echo "--- Pipeline divergence documented in test_helpers.sh ---"

if grep -q 'tests/infra/test_tree_sitter_pipeline.sh' "$HELPER_FILE" 2>/dev/null; then ok=true; else ok=false; fi
check "test_helpers.sh documents pipeline divergence" "$ok"

# ==============================================================================
# Warm-lane test isolation helpers (tasks 5590/5612)
#
# init_isolated_lane_root <stem> + make_isolated_lane <prefix> are promoted out
# of tests/infra/test_seed_warm_lane.sh Block R into the shared library so the
# seven warm-lane suites share ONE implementation.
#
# WHY the facility exists at all: scripts/seed-warm-lane.sh computes
# RESEED_TRASH_DIR as dirname(LANE_DIR)/.reseed-trash and renames a non-empty
# <lane>/target there before re-seeding. A lane created bare under /tmp makes
# that path the machine-shared /tmp/.reseed-trash, shared with every other
# agent/test run on the host. Nesting each lane under its own private parent
# makes dirname(LANE_DIR) unique per lane, so the computed trash dir is
# run-private.
#
# WHY the init/make SPLIT: call sites read `X_LANE="$(make_isolated_lane p)"`,
# so make_isolated_lane's body runs in a command-substitution SUBSHELL where any
# `_TMPDIRS+=(...)` is silently discarded when the subshell exits — leaking every
# private parent. So registration happens ONCE, in the main shell, in
# init_isolated_lane_root; make_isolated_lane must append to nothing.
#
# WHY these probes never touch the real /tmp: _wl_run redirects TMPDIR into this
# file's own $_robust_tmpdir. Several probes deliberately mint lane roots and
# lanes, and a facility whose own unit tests littered the machine-shared path it
# defends would be self-defeating.
# ==============================================================================

echo ""
echo "--- Warm-lane isolation: init_isolated_lane_root / make_isolated_lane contract ---"

_WL_DIR="$(mktemp -d "$_robust_tmpdir/wl-XXXXXX")"

# _wl_run <probe-script> [args...] — run a probe against $HELPER_FILE in a fresh
# bash process with a private per-probe TMPDIR. The probe receives the library
# path as $1. Sets _WL_RC / _WL_OUT / _WL_ERR / _WL_TMP (the private TMPDIR, so
# a caller can assert on exactly what the probe created there).
_wl_run() {
    local _script="$1"
    shift
    _WL_TMP="$(mktemp -d "$_WL_DIR/tmpdir-XXXXXX")"
    local _errf="$_WL_TMP.err"
    _WL_RC=0
    _WL_OUT="$(TMPDIR="$_WL_TMP" bash "$_script" "$HELPER_FILE" "$@" 2>"$_errf")" || _WL_RC=$?
    _WL_ERR="$(cat "$_errf")"
    rm -f "$_errf"
}

# _wl_flat <text> — squash newlines so a multi-line diagnostic stays on one
# check() line and does not corrupt the suite's PASS/FAIL line format.
_wl_flat() { printf '%s' "${1//$'\n'/ ; }"; }

# (a) Both functions are defined after sourcing.
for _wl_fn in init_isolated_lane_root make_isolated_lane; do
    if bash -c "source '$HELPER_FILE' && declare -f $_wl_fn >/dev/null" 2>/dev/null; then ok=true; else ok=false; fi
    check "WL-a: $_wl_fn is defined after sourcing test_helpers.sh" "$ok"
done

# (b) Sourcing alone is INERT. 153 files in the tree source this library; none
# may pay a mktemp or leak a temp entry for a facility only 7 of them use.
cat > "$_WL_DIR/inert.sh" <<'PROBE'
set -uo pipefail
source "$1"
# ${var-UNSET} (no colon) distinguishes set-but-empty from genuinely unset —
# the defaults must be SET (so `set -u` consumers can read them) and EMPTY.
printf 'LANE_ROOT=[%s] HITS=[%s]\n' "${_LANE_ROOT-UNSET}" "${_TRASH_HITS_FILE-UNSET}"
PROBE
_wl_run "$_WL_DIR/inert.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "LANE_ROOT=[] HITS=[]" ]; then ok=true; else ok=false; fi
check "WL-b1: sourcing alone leaves _LANE_ROOT/_TRASH_HITS_FILE set-but-empty (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

_wl_n="$(ls -A "$_WL_TMP" | wc -l)"
if [ "$_wl_n" -eq 0 ]; then ok=true; else ok=false; fi
check "WL-b2: sourcing alone creates no entry under \$TMPDIR — no source-time mktemp (got $_wl_n)" "$ok"

# (c) init_isolated_lane_root fails LOUDLY when _TMPDIRS is not yet declared.
# A call placed before the suite's own `_TMPDIRS=()` would otherwise register
# into an array that assignment then wipes, leaking the root for the whole run.
cat > "$_WL_DIR/init-no-tmpdirs.sh" <<'PROBE'
set -uo pipefail
source "$1"
init_isolated_lane_root teststem
PROBE
_wl_run "$_WL_DIR/init-no-tmpdirs.sh"
if [ "$_WL_RC" -ne 0 ]; then ok=true; else ok=false; fi
check "WL-c1: init_isolated_lane_root fails when _TMPDIRS is not already declared (rc=$_WL_RC)" "$ok"

if [[ "$_WL_ERR" == *_TMPDIRS* ]]; then ok=true; else ok=false; fi
check "WL-c2: ... naming _TMPDIRS in a stderr diagnostic (got: $(_wl_flat "$_WL_ERR"))" "$ok"

_wl_n="$(ls -A "$_WL_TMP" | wc -l)"
if [ "$_wl_n" -eq 0 ]; then ok=true; else ok=false; fi
check "WL-c3: ... and mints no unregistered root that nothing would ever reclaim (got $_wl_n)" "$ok"

# (d) Happy path: the root exists, is registered, is stem-named, lives in TMPDIR.
cat > "$_WL_DIR/init-ok.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }
[ -d "$_LANE_ROOT" ] || { echo "root-not-a-dir"; exit 8; }
_found=no
for _d in "${_TMPDIRS[@]}"; do
    if [ "$_d" = "$_LANE_ROOT" ]; then _found=yes; fi
done
printf 'found=%s count=%s base=%s parent=%s\n' \
    "$_found" "${#_TMPDIRS[@]}" "$(basename "$_LANE_ROOT")" "$(dirname "$_LANE_ROOT")"
PROBE
_wl_run "$_WL_DIR/init-ok.sh"
if [ "$_WL_RC" -eq 0 ] && [[ "$_WL_OUT" == "found=yes count=1 "* ]]; then ok=true; else ok=false; fi
check "WL-d1: init_isolated_lane_root registers an existing _LANE_ROOT into _TMPDIRS (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

if [[ "$_WL_OUT" == *"base=teststem-lane-root-"* ]]; then ok=true; else ok=false; fi
check "WL-d2: the root is named from the caller's stem, so litter stays attributable (got: $(_wl_flat "$_WL_OUT"))" "$ok"

if [[ "$_WL_OUT" == *"parent=$_WL_TMP"* ]]; then ok=true; else ok=false; fi
check "WL-d3: the root is minted under \$TMPDIR, not a hardcoded /tmp (got: $(_wl_flat "$_WL_OUT"))" "$ok"

# (e) make_isolated_lane's structural contract, the whole reason it exists.
cat > "$_WL_DIR/make-lane.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }
L="$(make_isolated_lane pfx)" || { echo "make-failed"; exit 8; }
P="$(dirname "$L")"
[ -d "$L" ]                || { echo "lane-not-a-dir"; exit 1; }
[ "$P" != "/tmp" ]         || { echo "parent-is-bare-tmp"; exit 1; }
[ "$P" != "${TMPDIR%/}" ]  || { echo "parent-is-bare-tmpdir"; exit 1; }
case "$L" in "$_LANE_ROOT"/*) ;; *) echo "lane-not-under-lane-root"; exit 1 ;; esac
_n="$(ls -A "$P" | wc -l)"
[ "$_n" -eq 1 ]            || { echo "parent-holds-$_n-entries"; exit 1; }
[ ! -e "$P/.reseed-trash" ] || { echo "reseed-trash-already-exists"; exit 1; }
echo OK
PROBE
_wl_run "$_WL_DIR/make-lane.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "OK" ]; then ok=true; else ok=false; fi
check "WL-e: make_isolated_lane yields a lane under a private parent — never bare /tmp, parent holds only the lane, no sibling .reseed-trash (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

# (f) Distinct parents per call — a shared parent would put two lanes' trash
# dirs on one path and defeat the isolation.
cat > "$_WL_DIR/two-lanes.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }
A="$(make_isolated_lane pfx)" || { echo "make-failed"; exit 8; }
B="$(make_isolated_lane pfx)" || { echo "make-failed"; exit 8; }
_sp=no; [ "$(dirname "$A")" = "$(dirname "$B")" ] && _sp=yes
_sl=no; [ "$A" = "$B" ] && _sl=yes
printf 'same_parent=%s same_lane=%s\n' "$_sp" "$_sl"
PROBE
_wl_run "$_WL_DIR/two-lanes.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "same_parent=no same_lane=no" ]; then ok=true; else ok=false; fi
check "WL-f: two make_isolated_lane calls with the same prefix get distinct private parents (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

# (g) THE subshell-safety property — the reason for the init/make split.
cat > "$_WL_DIR/subshell-safe.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }
_before="${#_TMPDIRS[@]}"
# Command substitution: make_isolated_lane's body runs in a SUBSHELL, so any
# array append it attempted would be silently discarded right here.
L="$(make_isolated_lane pfx)" || { echo "make-failed"; exit 8; }
_after="${#_TMPDIRS[@]}"
[ -d "$L" ] || { echo "lane-not-a-dir"; exit 1; }
# The caller's cleanup() reclaims only what _TMPDIRS holds. Prove the single
# registered root is sufficient to reclaim a lane minted after registration.
rm -rf "$_LANE_ROOT"
_gone=no; [ -e "$L" ] || _gone=yes
printf 'before=%s after=%s lane_gone=%s\n' "$_before" "$_after" "$_gone"
PROBE
_wl_run "$_WL_DIR/subshell-safe.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "before=1 after=1 lane_gone=yes" ]; then ok=true; else ok=false; fi
check "WL-g: make_isolated_lane appends to no array (subshell-safe) yet its lane is still reclaimed via \$_LANE_ROOT (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

# (h) make_isolated_lane without init must fail cleanly, not mktemp into a
# bare/empty path (which would resolve to "/lane-XXXXXX" or CWD).
cat > "$_WL_DIR/make-no-init.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
make_isolated_lane pfx
PROBE
_wl_run "$_WL_DIR/make-no-init.sh"
if [ "$_WL_RC" -ne 0 ]; then ok=true; else ok=false; fi
check "WL-h1: make_isolated_lane fails when init_isolated_lane_root was never called (rc=$_WL_RC)" "$ok"

if [[ "$_WL_ERR" == *init_isolated_lane_root* ]]; then ok=true; else ok=false; fi
check "WL-h2: ... naming init_isolated_lane_root in a stderr diagnostic (got: $(_wl_flat "$_WL_ERR"))" "$ok"

_wl_n="$(ls -A "$_WL_TMP" | wc -l)"
if [ "$_wl_n" -eq 0 ]; then ok=true; else ok=false; fi
check "WL-h3: ... and mktemps nothing into a bare/empty path (got $_wl_n entries under \$TMPDIR)" "$ok"

# ==============================================================================
# Warm-lane shared-trash runtime detector (tasks 5590/5612)
#
# _note_shared_trash_use is the RECORDER: warm-lane suites call it as a bare
# unguarded statement from inside their run_helper wrappers after every seed
# invocation. It inspects the captured seed stderr ($ERR_OUT) and records a hit
# whenever the invocation named $_SHARED_TRASH_DIR — exact evidence that this
# invocation renamed into the machine-shared path.
# _assert_no_shared_trash_use is the matching CHECKER.
#
# Three invariants below are load-bearing, not stylistic:
#   * the trailing `return 0` — the recorder runs as a bare unguarded statement
#     under `set -euo pipefail`, so any nonzero return would abort a whole suite
#     rather than fail one assert;
#   * state is an append-only FILE, not a bash array — two real call sites
#     invoke the helper inside a backgrounded ( ... ) & subshell, where an array
#     append is discarded on subshell exit, silently blinding the detector on
#     exactly the runs most likely to reach seed's rename-into-trash path;
#   * the case pattern quotes the variable (*"$_SHARED_TRASH_DIR"*) so a glob
#     metacharacter in the path is matched literally, not as a wildcard.
# ==============================================================================

echo ""
echo "--- Warm-lane isolation: shared-trash runtime detector ---"

# (b) Both detector entry points are defined after sourcing.
for _wl_fn in _note_shared_trash_use _assert_no_shared_trash_use; do
    if bash -c "source '$HELPER_FILE' && declare -f $_wl_fn >/dev/null" 2>/dev/null; then ok=true; else ok=false; fi
    check "WL-i1: $_wl_fn is defined after sourcing test_helpers.sh" "$ok"
done

# (a) The default is the real machine-shared path, and it is a plain variable a
# caller can redirect (the positive controls in the warm-lane suites depend on
# redirecting it to a run-private trash dir).
_wl_sd="$(bash -c "source '$HELPER_FILE' && printf '%s' \"\${_SHARED_TRASH_DIR-UNSET}\"" 2>/dev/null || echo ERROR)"
if [ "$_wl_sd" = "/tmp/.reseed-trash" ]; then ok=true; else ok=false; fi
check "WL-i2: _SHARED_TRASH_DIR defaults to the literal /tmp/.reseed-trash (got: $_wl_sd)" "$ok"

# (c) init_isolated_lane_root mints the hits file under $_LANE_ROOT, empty.
# Placement matters: it must be a SIBLING of each lane's private parent, never
# inside one, or the "parent holds only the lane" structural check breaks.
cat > "$_WL_DIR/hits-file.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }
[ -n "$_TRASH_HITS_FILE" ]  || { echo "hits-file-path-empty"; exit 1; }
[ -f "$_TRASH_HITS_FILE" ]  || { echo "hits-file-not-created"; exit 1; }
[ ! -s "$_TRASH_HITS_FILE" ] || { echo "hits-file-not-empty"; exit 1; }
[ "$(dirname "$_TRASH_HITS_FILE")" = "$_LANE_ROOT" ] || { echo "hits-file-not-under-lane-root"; exit 1; }
L="$(make_isolated_lane pfx)" || { echo "make-failed"; exit 8; }
P="$(dirname "$L")"
case "$_TRASH_HITS_FILE" in "$P"/*) echo "hits-file-inside-a-lane-parent"; exit 1 ;; esac
_n="$(ls -A "$P" | wc -l)"
[ "$_n" -eq 1 ] || { echo "lane-parent-holds-$_n-entries"; exit 1; }
echo OK
PROBE
_wl_run "$_WL_DIR/hits-file.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "OK" ]; then ok=true; else ok=false; fi
check "WL-i3: init_isolated_lane_root mints an empty _TRASH_HITS_FILE directly under \$_LANE_ROOT, beside (never inside) a lane's private parent (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

# (d)+(e) Recorder semantics: one line on a match, nothing on a miss, rc 0 in
# BOTH cases — and rc 0 even under `set -u` with ERR_OUT entirely unset, which
# is why the body must read ${ERR_OUT:-} rather than bare $ERR_OUT now that the
# library is sourced by 153 files with no such wrapper.
cat > "$_WL_DIR/note-hits.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }

ERR_OUT="info: Renaming non-empty /x/target -> $_SHARED_TRASH_DIR/lane.999 before re-seed"
_note_shared_trash_use match-probe
_rc_match=$?
_n_match="$(wc -l < "$_TRASH_HITS_FILE")"
_body_match="$(cat "$_TRASH_HITS_FILE")"

: > "$_TRASH_HITS_FILE"
ERR_OUT="info: reflink copy completed, nothing renamed"
_note_shared_trash_use nomatch-probe
_rc_nomatch=$?
_n_nomatch="$(wc -l < "$_TRASH_HITS_FILE")"

: > "$_TRASH_HITS_FILE"
unset ERR_OUT
_note_shared_trash_use unset-probe
_rc_unset=$?
_n_unset="$(wc -l < "$_TRASH_HITS_FILE")"

printf 'match=%s/%s/[%s] nomatch=%s/%s unset=%s/%s\n' \
    "$_rc_match" "$_n_match" "$_body_match" \
    "$_rc_nomatch" "$_n_nomatch" "$_rc_unset" "$_n_unset"
PROBE
_wl_run "$_WL_DIR/note-hits.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "match=0/1/[match-probe] nomatch=0/0 unset=0/0" ]; then ok=true; else ok=false; fi
check "WL-i4: _note_shared_trash_use records exactly one labelled line on a match, nothing on a miss, and returns 0 in every case including ERR_OUT unset under set -u (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

# (f) Glob metacharacters in the path are matched literally.
cat > "$_WL_DIR/note-glob.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }

# Unquoted inside the case pattern, '/tmp/a*b[x]/.reseed-trash' would ALSO
# match an ERR_OUT naming '/tmp/aZZZbx/.reseed-trash' — a false positive that
# would fail a suite for a path seed never touched.
_SHARED_TRASH_DIR='/tmp/a*b[x]/.reseed-trash'

ERR_OUT="renamed into /tmp/a*b[x]/.reseed-trash now"
_note_shared_trash_use literal-hit
_n_literal="$(wc -l < "$_TRASH_HITS_FILE")"

: > "$_TRASH_HITS_FILE"
ERR_OUT="renamed into /tmp/aZZZbx/.reseed-trash now"
_note_shared_trash_use glob-expanded-miss
_n_glob="$(wc -l < "$_TRASH_HITS_FILE")"

printf 'literal=%s glob=%s\n' "$_n_literal" "$_n_glob"
PROBE
_wl_run "$_WL_DIR/note-glob.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "literal=1 glob=0" ]; then ok=true; else ok=false; fi
check "WL-i5: a _SHARED_TRASH_DIR holding glob metacharacters is matched literally, not as a wildcard (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

# (g) THE reason the state is file-backed: an append made inside a backgrounded
# subshell must be visible to the parent shell. A bash array append would be
# discarded here, silently blinding the detector.
cat > "$_WL_DIR/note-subshell.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }
(
    ERR_OUT="Renaming non-empty /x/target -> $_SHARED_TRASH_DIR before re-seed"
    _note_shared_trash_use subshell-probe
) &
_pid=$!
wait "$_pid" 2>/dev/null || true
printf 'lines=%s body=[%s]\n' "$(wc -l < "$_TRASH_HITS_FILE")" "$(cat "$_TRASH_HITS_FILE")"
PROBE
_wl_run "$_WL_DIR/note-subshell.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "lines=1 body=[subshell-probe]" ]; then ok=true; else ok=false; fi
check "WL-i6: a recorder append made inside a backgrounded ( ... ) & subshell is visible to the parent shell (file-backed state, rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

# (h) Checker semantics, message content included: a refactor that dropped the
# hit dump or the path from the message would still exit 1 but lose every scrap
# of forensic value.
cat > "$_WL_DIR/assert-no-use.sh" <<'PROBE'
set -uo pipefail
source "$1"
_TMPDIRS=()
init_isolated_lane_root teststem || { echo "init-failed"; exit 9; }

_out_clean="$(_assert_no_shared_trash_use 2>&1)"; _rc_clean=$?

printf 'stale-hit-one\nstale-hit-two\n' >> "$_TRASH_HITS_FILE"
_out_dirty="$(_assert_no_shared_trash_use 2>&1)"; _rc_dirty=$?

_names_dir=no; case "$_out_dirty" in *"$_SHARED_TRASH_DIR"*) _names_dir=yes ;; esac
_names_h1=no;  case "$_out_dirty" in *stale-hit-one*)         _names_h1=yes  ;; esac
_names_h2=no;  case "$_out_dirty" in *stale-hit-two*)         _names_h2=yes  ;; esac

printf 'clean=%s/[%s] dirty=%s names_dir=%s h1=%s h2=%s\n' \
    "$_rc_clean" "$_out_clean" "$_rc_dirty" "$_names_dir" "$_names_h1" "$_names_h2"
PROBE
_wl_run "$_WL_DIR/assert-no-use.sh"
if [ "$_WL_RC" -eq 0 ] && [ "$_WL_OUT" = "clean=0/[] dirty=1 names_dir=yes h1=yes h2=yes" ]; then ok=true; else ok=false; fi
check "WL-i7: _assert_no_shared_trash_use passes silently on an empty hits file and fails naming both _SHARED_TRASH_DIR and every recorded hit (rc=$_WL_RC got: $(_wl_flat "$_WL_OUT$_WL_ERR"))" "$ok"

# -- Summary -------------------------------------------------------------------
echo ""
echo "Results: $T_PASS passed, $T_FAIL failed"
if [ "$T_FAIL" -gt 0 ]; then
    exit 1
fi
