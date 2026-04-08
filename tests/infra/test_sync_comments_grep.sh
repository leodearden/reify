#!/usr/bin/env bash
# Meta-test: verify the fn-existence grep pattern in sync_comments_test.sh is
# POSIX-portable (no \b word-boundary, no grep -P) and correctly anchors the
# function name with [[:space:](] instead.
#
# Section 1 — fixture assertions — exercise the expected regex literal against
#   synthetic strings and pass on any version of sync_comments_test.sh.
# Section 2 — source-file consistency assertions — grep sync_comments_test.sh
#   for the new pattern and the absence of \b; these are the TDD red→green
#   driver that fails before the impl step and passes after.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SYNC_TEST="$REPO_ROOT/tests/sync_comments_test.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== sync_comments grep pattern meta-test ==="

echo ""
echo "--- Section 1: fixture accept/reject assertions (regex correctness) ---"

# Extract the fn-existence regex from sync_comments_test.sh at runtime so
# the meta-test stays coupled to the real test.  Pipeline:
#   1. find the grep -qE invocation line that contains [[:space:](]
#   2. strip the leading 'grep -qE '' prefix
#   3. strip the trailing '' "$filename"' suffix
#   4. replace the shell variable reference (e.g. '"${ref_fn}"') with 'sanitize_value'
# If extraction fails (empty result) we exit early with a diagnostic so the
# cause is visible rather than producing cryptic fixture failures.
PATTERN=$(
    grep 'grep -qE' "$SYNC_TEST" | \
    grep -F '[[:space:](<]' | \
    head -1 | \
    sed "s/^[[:space:]]*grep -qE '//; s/'[[:space:]]*\"[^\"]*\"[[:space:]]*$//; s/'\"[^\"]*\"'/sanitize_value/"
)
if [ -z "$PATTERN" ]; then
    echo "ERROR: could not extract fn-existence regex from $SYNC_TEST" >&2
    echo "       Expected a 'grep -qE' line containing '[[:space:](<]'" >&2
    exit 1
fi
assert "pattern extraction from sync_comments_test.sh succeeded" \
    test -n "$PATTERN"

# -- Accept cases: pattern must match these valid Rust fn declarations ----------

assert "accepts: fn sanitize_value(" \
    bash -c "printf '%s\n' 'fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: pub fn sanitize_value(" \
    bash -c "printf '%s\n' 'pub fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: indented fn sanitize_value( (inside mod block)" \
    bash -c "printf '%s\n' '    fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: async fn sanitize_value(" \
    bash -c "printf '%s\n' 'async fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: pub async fn sanitize_value(" \
    bash -c "printf '%s\n' 'pub async fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: tab-indented fn sanitize_value( (inside mod block)" \
    bash -c "printf '\tfn sanitize_value(v: Value) -> Value {\n' | grep -qE '$PATTERN'"

assert "accepts: multi-space between fn and name (fn   sanitize_value()" \
    bash -c "printf '%s\n' 'fn   sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: trailing space before paren (fn sanitize_value ()" \
    bash -c "printf '%s\n' 'fn sanitize_value (v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: pub(crate) fn sanitize_value(" \
    bash -c "printf '%s\n' 'pub(crate) fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: pub(super) fn sanitize_value(" \
    bash -c "printf '%s\n' 'pub(super) fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: unsafe fn sanitize_value(" \
    bash -c "printf '%s\n' 'unsafe fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: const fn sanitize_value(" \
    bash -c "printf '%s\n' 'const fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "accepts: fn sanitize_value<T>(" \
    bash -c "printf '%s\n' 'fn sanitize_value<T>(v: T) -> T {' | grep -qE '$PATTERN'"

# -- Reject cases: pattern must NOT match these strings ------------------------

assert "rejects: fn sanitize_value_raw( (suffix false-positive)" \
    bash -c "! printf '%s\n' 'fn sanitize_value_raw(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "rejects: // fn sanitize_value (comment line)" \
    bash -c "! printf '%s\n' '// fn sanitize_value(v: Value)' | grep -qE '$PATTERN'"

assert "rejects: // SYNC: reify-stdlib::sanitize_value (cross-ref line)" \
    bash -c "! printf '%s\n' '// SYNC: reify-stdlib::sanitize_value' | grep -qE '$PATTERN'"

assert "rejects: let sanitize_value = ... (non-fn binding)" \
    bash -c "! printf '%s\n' 'let sanitize_value = value;' | grep -qE '$PATTERN'"

assert "rejects: fnsanitize_value( (no space between fn keyword and name)" \
    bash -c "! printf '%s\n' 'fnsanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "rejects: my_fn sanitize_value( (false-prefix before fn keyword)" \
    bash -c "! printf '%s\n' 'my_fn sanitize_value(v: Value) -> Value {' | grep -qE '$PATTERN'"

echo ""
echo "--- Section 2: sync_comments_test.sh source-file consistency ---"

assert "sync_comments_test.sh exists" \
    test -f "$SYNC_TEST"

assert "sync_comments_test.sh uses POSIX-portable [[:space:](<] post-name class" \
    grep -qF '[[:space:](<]' "$SYNC_TEST"

# Scoped assertions check only non-comment grep invocation lines
# (^[^#]*grep matches lines where 'grep' appears before any '#').
# File-wide fixed-string searches were replaced because a documentation comment
# like '# POSIX: do not use \b here' would trigger them as false positives,
# breaking CI without any real regression.
assert "no \\b in grep invocations (non-comment lines, scoped)" \
    bash -c "! grep -E '^[^#]*grep[[:space:]].*\\\\b' '$SYNC_TEST'"

assert "no grep -P in grep invocations (non-comment lines, scoped)" \
    bash -c "! grep -E '^[^#]*grep[[:space:]]+-P' '$SYNC_TEST'"

test_summary
