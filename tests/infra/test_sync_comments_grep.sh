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

# The POSIX-portable fn-existence regex with a concrete name substituted in.
# This mirrors what grep evaluates in sync_comments_test.sh after variable
# expansion of ${_expr_ref_fn} / ${_stdlib_ref_fn}.
PATTERN='^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+sanitize_value[[:space:](]'

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

# -- Reject cases: pattern must NOT match these strings ------------------------

assert "rejects: fn sanitize_value_raw( (suffix false-positive)" \
    bash -c "! printf '%s\n' 'fn sanitize_value_raw(v: Value) -> Value {' | grep -qE '$PATTERN'"

assert "rejects: // fn sanitize_value (comment line)" \
    bash -c "! printf '%s\n' '// fn sanitize_value(v: Value)' | grep -qE '$PATTERN'"

assert "rejects: // SYNC: reify-stdlib::sanitize_value (cross-ref line)" \
    bash -c "! printf '%s\n' '// SYNC: reify-stdlib::sanitize_value' | grep -qE '$PATTERN'"

assert "rejects: let sanitize_value = ... (non-fn binding)" \
    bash -c "! printf '%s\n' 'let sanitize_value = value;' | grep -qE '$PATTERN'"

echo ""
echo "--- Section 2: sync_comments_test.sh source-file consistency ---"

assert "sync_comments_test.sh exists" \
    test -f "$SYNC_TEST"

# This assertion is RED before the impl step (sync_comments_test.sh still uses
# the old pattern) and GREEN after.
assert "sync_comments_test.sh uses POSIX-portable [[:space:](] post-name class" \
    grep -qF '[[:space:](]' "$SYNC_TEST"

# This assertion is RED before the impl step (\b is still present) and GREEN after.
assert "sync_comments_test.sh does not use \b word-boundary anchor" \
    bash -c "! grep -qF '\b' '$SYNC_TEST'"

assert "sync_comments_test.sh does not use grep -P (non-POSIX flag)" \
    bash -c "! grep -qF 'grep -P' '$SYNC_TEST'"

test_summary
