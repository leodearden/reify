#!/usr/bin/env bash
# Meta-test: verify the fn-existence grep pattern in sync_comments_test.sh is
# POSIX-portable (no \b word-boundary, no grep -P) and correctly anchors the
# function name with [[:space:](<] instead.
#
# Section 1 — fixture assertions — exercise the expected regex literal against
#   synthetic strings and pass on any version of sync_comments_test.sh.
# Section 2 — source-file consistency assertions — grep sync_comments_test.sh
#   for the new pattern and the absence of \b; these originally served as the
#   TDD red→green driver for the task 1309 impl step and now act as regression
#   guards.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SYNC_TEST="$REPO_ROOT/tests/sync_comments_test.sh"
SYNC_REF_HELPERS="$REPO_ROOT/tests/infra/sync_ref_helpers.sh"
THIS_SCRIPT="${BASH_SOURCE[0]}"
# Exported so Sections 2 & 3 `bash -c '...'` subshells can read these paths
# from their own environment instead of parent-shell interpolation, matching
# the Section 1 hardening convention from Task 1322 (see task 1346).
export SYNC_TEST SYNC_REF_HELPERS

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"
exec 3>&2

echo "=== sync_comments grep pattern meta-test ==="

echo ""
echo "--- Section 1: fixture accept/reject assertions (regex correctness) ---"

# Extract the fn-existence regex from sync_comments_test.sh at runtime so
# the meta-test stays coupled to the real test.  Pipeline:
#   1. find the grep -qE invocation line that contains [[:space:](<]
#   2. strip the leading 'grep -qE '' prefix
#   3. strip the trailing '' "$filename"' suffix
#   4. replace the shell variable reference (e.g. '"${ref_fn}"') with 'sanitize_value'
# If extraction fails (empty result) we exit early with a diagnostic so the
# cause is visible rather than producing cryptic fixture failures.
PATTERN=$(
    grep 'grep -qE' "$SYNC_REF_HELPERS" | \
    grep -F '[[:space:](<]' | \
    head -1 | \
    sed "s/^[[:space:]]*grep -qE '//; s/'[[:space:]]*\"[^\"]*\"[[:space:]]*$//; s/'\"[^\"]*\"'/sanitize_value/"
)
if [ -z "$PATTERN" ]; then
    echo "ERROR: could not extract fn-existence regex from $SYNC_REF_HELPERS" >&2
    echo "       Expected a 'grep -qE' line containing '[[:space:](<]'" >&2
    exit 1
fi
export PATTERN
assert "pattern extraction from sync_comments_test.sh succeeded" \
    test -n "$PATTERN"

assert "extracted pattern contains expected fn name" \
    bash -c '[[ "$PATTERN" == *sanitize_value* ]]'

# Split assignment so the regex target never appears contiguously in this
# file — prevents the self-check below from matching its own definition.
_CHECK='bash'; _CHECK+=' -c ".*\$\{?PATTERN'
_no_unhardened_pattern_interp() {
    local m
    m=$(grep -nE "$_CHECK" "$THIS_SCRIPT" 2>/dev/null) || true
    if [ -z "$m" ]; then
        return 0
    fi
    printf 'UNHARDENED PATTERN INTERPOLATION FOUND:\n%s\n' "$m" >&3
    return 1
}
# Parallel guard for Section 2/3 variables (SYNC_TEST, SYNC_REF_HELPERS,
# _SECT3_HELPER). Not anchored to start-of-line so it also catches inlined
# usages (e.g. `... && bash -c "..."`) that _no_double_quoted_bash_c misses.
# Split to prevent self-match — see _CHECK comment above.
_S23_CHECK='bash'; _S23_CHECK+=' -c ".*\$\{?(SYNC_TEST|SYNC_REF_HELPERS|_SECT3_HELPER)'
_no_unhardened_section23_interp() {
    local m
    m=$(grep -nE "$_S23_CHECK" "$THIS_SCRIPT" 2>/dev/null) || true
    if [ -z "$m" ]; then
        return 0
    fi
    printf 'UNHARDENED SECTION 2/3 INTERPOLATION FOUND:\n%s\n' "$m" >&3
    return 1
}
_no_double_quoted_bash_c() {
    local m
    m=$(grep -nE '^[[:space:]]*bash -c "' "$THIS_SCRIPT" 2>/dev/null) || true
    if [ -z "$m" ]; then
        return 0
    fi
    printf 'DOUBLE-QUOTED bash -c FOUND (use single-outer quotes + exported vars):\n%s\n' "$m" >&3
    return 1
}

assert 'no Section 1 bash -c $PATTERN interpolation in this script' \
    _no_unhardened_pattern_interp
assert 'no double-quoted bash -c in this script (all bash -c must be single-outer-quoted)' \
    _no_double_quoted_bash_c
assert 'no Section 2/3 unhardened bash -c interpolation in this script' \
    _no_unhardened_section23_interp

# -- S3: behavioral assertion that PATTERN is actually exported at runtime -----
assert 'PATTERN is actually exported (behavioral)' \
    bash -c '[ -n "${PATTERN+x}" ] && env | grep -q "^PATTERN="'

# -- S3: regression guard that the textual export-PATTERN grep is absent -------
# Split to prevent self-match: the full string is never on one line here.
_S3_CHECK='"this script exports PATTERN'
_S3_CHECK+=' for bash -c subshells"'
_no_textual_export_check() {
    ! grep -qF "$_S3_CHECK" "$THIS_SCRIPT"
}
assert 'no textual export-PATTERN assertion (S3: behavioral check only)' \
    _no_textual_export_check

# -- S1: meta-assertion that _CHECK= has an explanatory comment above it -------
_test_comment_above_check() {
    grep -B1 '^_CHECK=' "$THIS_SCRIPT" | head -1 | grep -q '^#'
}
assert "_CHECK= definition has an explanatory comment directly above it" \
    _test_comment_above_check

# -- S4: regression guard — Section 3 intro comment documents source side-effect
# Fragments split across two variables prevent self-match on these definition
# lines. Grep is scoped to comment lines ('^#') so the assert description below
# does not self-match even when it contains the contiguous phrases.
_DOC_SE_FRAG1='side'; _DOC_SE_FRAG2=' effect'
_DOC_NF_FRAG1='non'; _DOC_NF_FRAG2='-fatal'
_section3_comment_documents_source_side_effect() {
    grep '^#' "$THIS_SCRIPT" | grep -qF "${_DOC_SE_FRAG1}${_DOC_SE_FRAG2}" && \
    grep '^#' "$THIS_SCRIPT" | grep -qF "${_DOC_NF_FRAG1}${_DOC_NF_FRAG2}"
}
assert 'Section 3 intro comment documents SYNC_TEST source side-effect (side effect + non-fatal phrases)' \
    _section3_comment_documents_source_side_effect

# -- S5 (esc-3444-93): regression guard — the Section-3-comment detector above
# must not use a SIGPIPE-prone comment-pipe construct under pipefail. Fragments
# split across two vars prevent self-match (same anti-self-match convention as
# _DOC_SE_FRAG / _DOC_NF_FRAG above).
_SIGPIPE_FRAG1='grep '\''^#'\'' "$THIS_SCRIPT" |'
_SIGPIPE_FRAG2=' grep -q'
_no_sigpipe_prone_comment_pipe() { ! grep -qF "${_SIGPIPE_FRAG1}${_SIGPIPE_FRAG2}" "$THIS_SCRIPT"; }
assert 'Section-3-comment detector uses no SIGPIPE-prone comment-grep pipe under pipefail (esc-3444-93)' \
    _no_sigpipe_prone_comment_pipe

# -- S2: regression guards for the hardening self-check regex ------------------
_test_braced_form_caught() {
    local frag1='bash'
    local frag2=' -c "echo ${PATTERN}"'
    local tmp rc=0
    tmp=$(mktemp)
    printf '%s%s\n' "$frag1" "$frag2" > "$tmp"
    local saved="$THIS_SCRIPT"
    THIS_SCRIPT="$tmp"
    _no_unhardened_pattern_interp 3>/dev/null 2>/dev/null || rc=$?
    THIS_SCRIPT="$saved"
    rm -f "$tmp"
    [ "$rc" -ne 0 ]
}
_test_plain_form_still_caught() {
    local frag1='bash'
    local frag2=' -c "echo $PATTERN"'
    local tmp rc=0
    tmp=$(mktemp)
    printf '%s%s\n' "$frag1" "$frag2" > "$tmp"
    local saved="$THIS_SCRIPT"
    THIS_SCRIPT="$tmp"
    _no_unhardened_pattern_interp 3>/dev/null 2>/dev/null || rc=$?
    THIS_SCRIPT="$saved"
    rm -f "$tmp"
    [ "$rc" -ne 0 ]
}
assert 'braced ${PATTERN} form is caught by _CHECK regex' \
    _test_braced_form_caught
assert 'plain $PATTERN form is still caught by _CHECK regex (regression)' \
    _test_plain_form_still_caught

# -- S2: regression guards for the Section 2/3 hardening self-check regex ------
_test_sect23_plain_sync_test_caught() {
    local frag1='bash'
    local frag2=' -c "echo $SYNC_TEST"'
    local tmp rc=0
    tmp=$(mktemp)
    printf '%s%s\n' "$frag1" "$frag2" > "$tmp"
    local saved="$THIS_SCRIPT"
    THIS_SCRIPT="$tmp"
    _no_unhardened_section23_interp 3>/dev/null 2>/dev/null || rc=$?
    THIS_SCRIPT="$saved"
    rm -f "$tmp"
    [ "$rc" -ne 0 ]
}
_test_sect23_braced_sync_ref_helpers_caught() {
    local frag1='bash'
    local frag2=' -c "echo ${SYNC_REF_HELPERS}"'
    local tmp rc=0
    tmp=$(mktemp)
    printf '%s%s\n' "$frag1" "$frag2" > "$tmp"
    local saved="$THIS_SCRIPT"
    THIS_SCRIPT="$tmp"
    _no_unhardened_section23_interp 3>/dev/null 2>/dev/null || rc=$?
    THIS_SCRIPT="$saved"
    rm -f "$tmp"
    [ "$rc" -ne 0 ]
}
_test_sect23_plain_sect3_helper_caught() {
    local frag1='bash'
    local frag2=' -c "echo $_SECT3_HELPER"'
    local tmp rc=0
    tmp=$(mktemp)
    printf '%s%s\n' "$frag1" "$frag2" > "$tmp"
    local saved="$THIS_SCRIPT"
    THIS_SCRIPT="$tmp"
    _no_unhardened_section23_interp 3>/dev/null 2>/dev/null || rc=$?
    THIS_SCRIPT="$saved"
    rm -f "$tmp"
    [ "$rc" -ne 0 ]
}
assert 'plain $SYNC_TEST form is caught by _S23_CHECK regex' \
    _test_sect23_plain_sync_test_caught
assert 'braced ${SYNC_REF_HELPERS} form is caught by _S23_CHECK regex' \
    _test_sect23_braced_sync_ref_helpers_caught
assert 'plain $_SECT3_HELPER form is caught by _S23_CHECK regex' \
    _test_sect23_plain_sect3_helper_caught

# -- S6: loud diagnostic header on _no_unhardened_pattern_interp failure -------
_test_loud_header_on_failure() {
    local frag1='bash'
    local frag2=' -c "echo $PATTERN"'
    local tmp out
    tmp=$(mktemp)
    out=$(mktemp)
    printf '%s%s\n' "$frag1" "$frag2" > "$tmp"
    local saved_script="$THIS_SCRIPT"
    THIS_SCRIPT="$tmp"
    _no_unhardened_pattern_interp 3>"$out" 2>/dev/null || true
    THIS_SCRIPT="$saved_script"
    local result
    result=$(cat "$out")
    rm -f "$tmp" "$out"
    echo "$result" | grep -q 'UNHARDENED PATTERN INTERPOLATION FOUND:'
}
assert '_no_unhardened_pattern_interp emits loud diagnostic header on failure' \
    _test_loud_header_on_failure

# -- Accept cases: pattern must match these valid Rust fn declarations ----------

assert "accepts: fn sanitize_value(" \
    bash -c 'printf "%s\n" "fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub fn sanitize_value(" \
    bash -c 'printf "%s\n" "pub fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: indented fn sanitize_value( (inside mod block)" \
    bash -c 'printf "%s\n" "    fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: async fn sanitize_value(" \
    bash -c 'printf "%s\n" "async fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub async fn sanitize_value(" \
    bash -c 'printf "%s\n" "pub async fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: tab-indented fn sanitize_value( (inside mod block)" \
    bash -c 'printf "\tfn sanitize_value(v: Value) -> Value {\n" | grep -qE "$PATTERN"'

assert "accepts: multi-space between fn and name (fn   sanitize_value()" \
    bash -c 'printf "%s\n" "fn   sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: trailing space before paren (fn sanitize_value ()" \
    bash -c 'printf "%s\n" "fn sanitize_value (v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(crate) fn sanitize_value(" \
    bash -c 'printf "%s\n" "pub(crate) fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(super) fn sanitize_value(" \
    bash -c 'printf "%s\n" "pub(super) fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: unsafe fn sanitize_value(" \
    bash -c 'printf "%s\n" "unsafe fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: const fn sanitize_value(" \
    bash -c 'printf "%s\n" "const fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: fn sanitize_value<T>(" \
    bash -c 'printf "%s\n" "fn sanitize_value<T>(v: T) -> T {" | grep -qE "$PATTERN"'

assert "accepts: pub(crate) const fn sanitize_value( (pub+const combination)" \
    bash -c 'printf "%s\n" "pub(crate) const fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(crate) unsafe fn sanitize_value( (pub+unsafe combination)" \
    bash -c 'printf "%s\n" "pub(crate) unsafe fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(crate) async fn sanitize_value( (pub+async combination)" \
    bash -c 'printf "%s\n" "pub(crate) async fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(super) const fn sanitize_value( (pub(super)+const combination)" \
    bash -c 'printf "%s\n" "pub(super) const fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(in crate::foo) fn sanitize_value( (pub(in path)+fn combination)" \
    bash -c 'printf "%s\n" "pub(in crate::foo) fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(super) unsafe fn sanitize_value( (pub(super)+unsafe combination)" \
    bash -c 'printf "%s\n" "pub(super) unsafe fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: const unsafe fn sanitize_value( (const+unsafe combination — Rust grammar order)" \
    bash -c 'printf "%s\n" "const unsafe fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: async unsafe fn sanitize_value( (async+unsafe combination — Rust grammar order)" \
    bash -c 'printf "%s\n" "async unsafe fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub const unsafe fn sanitize_value( (pub+const+unsafe triple combination)" \
    bash -c 'printf "%s\n" "pub const unsafe fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(crate) async unsafe fn sanitize_value( (pub(crate)+async+unsafe triple combination)" \
    bash -c 'printf "%s\n" "pub(crate) async unsafe fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "accepts: pub(crate) const unsafe fn sanitize_value( (pub(crate)+const+unsafe triple combination)" \
    bash -c 'printf "%s\n" "pub(crate) const unsafe fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

# -- Grammar-order reject cases ------------------------------------------------
# The two assertions below guard against qualifiers in the wrong ORDER.
# Intentional gap: `const async fn` and `const async unsafe fn` are NOT asserted
# as rejects. The regex validates grammar ORDER (const→async→unsafe→fn), not Rust
# semantics. Both strings match the pattern in the correct positional order even
# though Rust forbids `const async fn` semantically (const fn cannot be async).
# Asserting them as rejects would be incorrect scope creep — this regex is not a
# Rust semantic validator.
assert "rejects: unsafe async fn sanitize_value( (invalid Rust grammar order — Rust requires async unsafe, not unsafe async)" \
    bash -c '! printf "%s\n" "unsafe async fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "rejects: unsafe const fn sanitize_value( (invalid Rust grammar order — Rust requires const unsafe, not unsafe const)" \
    bash -c '! printf "%s\n" "unsafe const fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

# -- Reject cases: pattern must NOT match these strings ------------------------

assert "rejects: fn sanitize_value_raw( (suffix false-positive)" \
    bash -c '! printf "%s\n" "fn sanitize_value_raw(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "rejects: pub(crate) async fn sanitize_value_raw( (pub+async suffix false-positive)" \
    bash -c '! printf "%s\n" "pub(crate) async fn sanitize_value_raw(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "rejects: pub(super) const fn sanitize_value_raw( (pub(super)+const suffix false-positive)" \
    bash -c '! printf "%s\n" "pub(super) const fn sanitize_value_raw(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "rejects: pub(in crate::foo) fn sanitize_value_raw( (pub(in path) suffix false-positive)" \
    bash -c '! printf "%s\n" "pub(in crate::foo) fn sanitize_value_raw(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "rejects: // fn sanitize_value (comment line)" \
    bash -c '! printf "%s\n" "// fn sanitize_value(v: Value)" | grep -qE "$PATTERN"'

assert "rejects: // SYNC: reify-stdlib::sanitize_value (cross-ref line)" \
    bash -c '! printf "%s\n" "// SYNC: reify-stdlib::sanitize_value" | grep -qE "$PATTERN"'

assert "rejects: let sanitize_value = ... (non-fn binding)" \
    bash -c '! printf "%s\n" "let sanitize_value = value;" | grep -qE "$PATTERN"'

assert "rejects: fnsanitize_value( (no space between fn keyword and name)" \
    bash -c '! printf "%s\n" "fnsanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "rejects: my_fn sanitize_value( (false-prefix before fn keyword)" \
    bash -c '! printf "%s\n" "my_fn sanitize_value(v: Value) -> Value {" | grep -qE "$PATTERN"'

assert "rejects: fn sanitize_value_raw<T>( (suffixed name that is also generic)" \
    bash -c '! printf "%s\n" "fn sanitize_value_raw<T>(v: T) -> T {" | grep -qE "$PATTERN"'

# S4 (narrow scoping, task 1346): now that Section 1 is done, drop PATTERN from
# the environment so it cannot shadow any local `PATTERN` variable inside
# scripts sourced by Section 3 subshells (e.g. tests/sync_comments_test.sh).
# A current `grep PATTERN tests/sync_comments_test.sh` returns zero matches,
# so this is future-proofing rather than a live bug fix.
unset PATTERN

echo ""
echo "--- Section 2: sync_comments_test.sh source-file consistency ---"

assert 'PATTERN is unset at start of Section 2 (S4: scoping regression guard)' \
    bash -c '[ -z "${PATTERN+x}" ]'

assert "sync_comments_test.sh exists" \
    test -f "$SYNC_TEST"

assert "sync_ref_helpers.sh uses POSIX-portable [[:space:](<] post-name class" \
    grep -qF '[[:space:](<]' "$SYNC_REF_HELPERS"

# Scoped assertions check only non-comment grep invocation lines
# (^[^#]*grep matches lines where 'grep' appears before any '#').
# File-wide fixed-string searches were replaced because a documentation comment
# like '# POSIX: do not use \b here' would trigger them as false positives,
# breaking CI without any real regression.
# Both sync_ref_helpers.sh and sync_comments_test.sh are checked (paired) to
# guard against regressions in either file.
assert "no \\b in grep invocations in sync_ref_helpers.sh (non-comment lines, scoped)" \
    bash -c '! grep -E "^[^#]*grep[[:space:]].*\\\\b" "$SYNC_REF_HELPERS"'

assert "no \\b in grep invocations in sync_comments_test.sh (non-comment lines, scoped)" \
    bash -c '! grep -E "^[^#]*grep[[:space:]].*\\\\b" "$SYNC_TEST"'

assert "no grep -P in grep invocations in sync_ref_helpers.sh (non-comment lines, scoped)" \
    bash -c '! grep -E "^[^#]*grep[[:space:]]+-P" "$SYNC_REF_HELPERS"'

assert "no grep -P in grep invocations in sync_comments_test.sh (non-comment lines, scoped)" \
    bash -c '! grep -E "^[^#]*grep[[:space:]]+-P" "$SYNC_TEST"'

# S1: regression guard — Section 2 header must not claim it "fails before the
# impl step" (past-tense reframe applied by task 1581).
# Fragments split across two variables prevent self-match.
_S1_FRAG1='fails before the impl'
_S1_FRAG2=' step'
_no_present_tense_redgreen_claim() {
    ! grep -qF "${_S1_FRAG1}${_S1_FRAG2}" "$THIS_SCRIPT"
}
assert 'Section 2 header uses past-tense/regression-guard framing, not present-tense red→green claim (S1)' \
    _no_present_tense_redgreen_claim

assert "stdlib assert description uses crate-name form 'reify-stdlib has SYNC marker'" \
    grep -q '"reify-stdlib has SYNC marker referencing reify-expr::sanitize_value"' "$SYNC_TEST"

assert "extract_fn comment describes allowed prefixes for broad awk pattern" \
    bash -c 'grep "^#" "$SYNC_TEST" | grep -qF "Allowed prefixes"'

# S2: regression guard — extract_fn docstring must not claim modifiers are
# accepted "in any valid subset" (order-enforcing reframe applied by task 1581).
# Fragments split across two variables prevent self-match.
_S2_FRAG1='in any valid subset'
_S2_FRAG2=" before 'fn'"
_no_ambiguous_modifier_subset_claim() {
    ! grep -qF "${_S2_FRAG1}${_S2_FRAG2}" "$SYNC_TEST"
}
assert 'extract_fn comment states modifier order is enforced, not any-subset (S2)' \
    _no_ambiguous_modifier_subset_claim

assert "sync_ref_helpers.sh documents extern fn limitation" \
    grep -qF 'extern "C" fn' "$SYNC_REF_HELPERS"

assert "sync_ref_helpers.sh documents default fn limitation" \
    grep -qF 'default fn' "$SYNC_REF_HELPERS"

assert "sync_ref_helpers.sh documents qualifier order mirrors canonical Rust grammar" \
    grep -qF 'const → async → unsafe → fn' "$SYNC_REF_HELPERS"

echo ""
echo "--- Section 3: extract_fn fixture accept/reject (regex anchoring) ---"

# extract_fn is defined in sync_comments_test.sh. We source it in a subshell
# with test_summary stubbed to no-op, following the established behavioral-test
# pattern from test_test_helpers.sh lines 301-312.
#
# _SECT3_HELPER is exported so the single-quoted `bash -c '...'` subshells
# below read it from their own environment (matching the Section 1 hardening
# convention from Task 1322). SYNC_TEST is already exported near the top.
#
# SOURCE SIDE EFFECT: sourcing SYNC_TEST inside the subshell is not a clean
# import — it also executes sync_comments_test.sh's top-level assertions as
# a side effect (the SYNC-marker grep, the extract_fn / sanitize_value body-
# diff assert, etc.). Those assertions must remain non-fatal: the shared
# `assert` helper in test_helpers.sh already is (it only increments FAIL, it
# does not exit), and `test_summary` is stubbed to no-op by each Section 3
# subshell.  Any future refactor of sync_comments_test.sh must preserve this
# non-fatal contract — introducing a top-level `exit` would silently abort
# the Section 3 subshell and turn a PASS into a silent empty-output failure.
#
# STRICT-MODE NEUTRALIZATION (flake guard, esc-3985-18): sync_comments_test.sh
# runs under `set -euo pipefail`, and sourcing it re-enables those options in
# this subshell. Its heavy top-level side effects (awk/grep/sed pipelines over
# real source files) can fail transiently under the parallel load of
# run_all.sh; with `set -e` active that aborts the subshell BEFORE the real
# `extract_fn` assertion below ever runs, turning a PASS into a spurious
# empty-output FAIL (observed once across many runs; reproduces 0/N standalone).
# We therefore (a) invoke the source in a `|| true` list so `set -e` is
# suppressed for the duration of the source body, and (b) `set +eo pipefail`
# afterwards so the remaining setup lines (mktemp/printf/extract_fn) cannot
# abort the subshell either. The FINAL command of each subshell — the actual
# accept/reject assertion — is unaffected by `set +e` (its exit status is what
# `assert` evaluates), so no real signal is masked.
#
# Follow-up: consider wrapping sync_comments_test.sh's top-level assertions
# in a main() function so that sourcing the file becomes side-effect-free.
# That refactor touches tests/sync_comments_test.sh, which is outside the
# scope of this task (tests/infra only).
_SECT3_HELPER="$SCRIPT_DIR/test_helpers.sh"
export _SECT3_HELPER

# accept: regular fn — fn foo( must be extracted when fn_name=foo
assert "extract_fn: fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && echo "$out" | grep -q "^fn foo("
    '

# accept: generic fn — fn foo<T>( must be extracted when fn_name=foo
assert "extract_fn: fn foo<T>( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "fn foo<T>(\n    x: T,\n) -> T {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && echo "$out" | grep -q "^fn foo<T>("
    '

# reject: fn foobar( must NOT be extracted when fn_name=foo (prefix-collision guard)
assert "extract_fn: fn foobar( NOT extracted when fn_name=foo (prefix collision)" \
    bash -c '
        tmp=$(mktemp)
        printf "fn foobar(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -z "$out" ]
    '

# reject: embedded-fn false-positive guard — "let y = fn foo(x);" must NOT be extracted.
# Regression guard: the old loose awk pattern ^[^/]*fn foo[(<] would match this embedded
# fn line because "let y =" is not a fn declaration.  The structured-modifier anchoring
# (const/async/unsafe) rejects it.
assert "extract_fn: 'let y = fn foo(x);' NOT extracted for fn_name=foo (embedded-fn false-positive)" \
    bash -c '
        tmp=$(mktemp)
        printf "let y = fn foo(x);\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -z "$out" ]
    '

# accept: const fn foo( — must be extracted when fn_name=foo; sed strips const prefix
assert "extract_fn: const fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "const fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && [ "$(printf "%s\n" "$out" | head -1)" = "fn foo(" ]
    '

# accept: unsafe fn foo( — must be extracted when fn_name=foo; sed strips unsafe prefix
assert "extract_fn: unsafe fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "unsafe fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && [ "$(printf "%s\n" "$out" | head -1)" = "fn foo(" ]
    '

# accept: async fn foo( — must be extracted when fn_name=foo; sed strips async prefix
assert "extract_fn: async fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "async fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && [ "$(printf "%s\n" "$out" | head -1)" = "fn foo(" ]
    '

# accept: pub fn foo( — must be extracted when fn_name=foo; sed strips the pub prefix
assert "extract_fn: pub fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "pub fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && echo "$out" | grep -q "fn foo("
    '

# accept: pub(crate) const fn foo( — must be extracted when fn_name=foo; sed strips pub(crate) and const
assert "extract_fn: pub(crate) const fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "pub(crate) const fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && [ "$(printf "%s\n" "$out" | head -1)" = "fn foo(" ]
    '

# accept: async unsafe fn foo( — multi-modifier combination (Rust grammar order) must be extracted; sed strips async+unsafe
assert "extract_fn: async unsafe fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "async unsafe fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && [ "$(printf "%s\n" "$out" | head -1)" = "fn foo(" ]
    '

# accept: pub(crate) const unsafe fn foo( — full modifier chain must be extracted; sed strips all modifiers
assert "extract_fn: pub(crate) const unsafe fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "pub(crate) const unsafe fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && [ "$(printf "%s\n" "$out" | head -1)" = "fn foo(" ]
    '

# accept: const unsafe fn foo( — canonical-order const+unsafe combination; sed strips both modifiers
assert "extract_fn: const unsafe fn foo( extracted correctly for fn_name=foo" \
    bash -c '
        tmp=$(mktemp)
        printf "const unsafe fn foo(\n    x: i32,\n) -> i32 {\n    x\n}\n" > "$tmp"
        source "$_SECT3_HELPER"
        test_summary() { :; }
        source "$SYNC_TEST" || true; set +eo pipefail
        PASS=0; FAIL=0
        out=$(extract_fn foo "$tmp")
        rm -f "$tmp"
        [ -n "$out" ] && [ "$(printf "%s\n" "$out" | head -1)" = "fn foo(" ]
    '

test_summary
