#!/usr/bin/env bash
# Test: Both copies of sanitize_value carry SYNC cross-reference comments.
#
# sanitize_value exists in two crates — reify-expr and reify-stdlib.
# Each copy must have a `// SYNC:` marker so that `grep SYNC:` finds both
# sites when Value variants change.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# sanitize_value lived in lib.rs before task-1304; moved to sanitize.rs / helpers.rs after the submodule split.
if [ -f "$REPO_ROOT/crates/reify-expr/src/sanitize.rs" ]; then
    EXPR_FILE="$REPO_ROOT/crates/reify-expr/src/sanitize.rs"
else
    EXPR_FILE="$REPO_ROOT/crates/reify-expr/src/lib.rs"
fi
if [ -f "$REPO_ROOT/crates/reify-stdlib/src/helpers.rs" ]; then
    STDLIB_FILE="$REPO_ROOT/crates/reify-stdlib/src/helpers.rs"
else
    STDLIB_FILE="$REPO_ROOT/crates/reify-stdlib/src/lib.rs"
fi

[ -f "$REPO_ROOT/tests/infra/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$REPO_ROOT/tests/infra/test_helpers.sh" || { echo "ERROR: failed to source test_helpers.sh"; exit 1; }

[ -f "$REPO_ROOT/tests/infra/sync_ref_helpers.sh" ] || { echo "ERROR: sync_ref_helpers.sh not found"; exit 1; }
source "$REPO_ROOT/tests/infra/sync_ref_helpers.sh" || { echo "ERROR: failed to source sync_ref_helpers.sh"; exit 1; }

# reify-expr's copy must reference reify-stdlib::sanitize_value
assert \
    "reify-expr has SYNC marker referencing reify-stdlib::sanitize_value" \
    grep -q "SYNC:.*reify-stdlib::sanitize_value" "$EXPR_FILE"

# reify-stdlib's copy must reference reify-expr::sanitize_value
assert \
    "reify-stdlib has SYNC marker referencing reify-expr::sanitize_value" \
    grep -q "SYNC:.*reify-expr::sanitize_value" "$STDLIB_FILE"

# Verify each SYNC cross-reference points to a real function in the peer crate
assert_sync_ref_exists reify-expr reify-stdlib "$EXPR_FILE" "$STDLIB_FILE"
assert_sync_ref_exists reify-stdlib reify-expr "$STDLIB_FILE" "$EXPR_FILE"

# Helper: extract from the fn signature line to the next line that begins with }
# at column 0.  Content above the fn keyword is excluded by the awk range
# anchor start-pattern (which starts matching at the fn declaration line),
# so doc comments and SYNC markers (which may legitimately differ between the
# two copies) do not affect the body comparison.  The start-pattern also skips
# commented-out function signatures via the leading /^[[:space:]]*(pub...)?fn/
# anchor.
#
# The awk start-pattern mirrors the assert_sync_ref_exists regex in
# tests/infra/sync_ref_helpers.sh so both helpers accept exactly the same set
# of fn declarations.  Allowed prefixes (in canonical Rust grammar order):
#   pub, pub(...), const, async, unsafe — in any valid subset before 'fn'.
# The pattern rejects embedded-fn expressions like 'let y = fn foo(x);' because
# the leading identifier ('let') is not one of the permitted modifier keywords.
extract_fn() {
    local fn_name="$1" file="$2"
    # Match fn with optional structured modifier prefixes (pub, pub(crate),
    # const, async, unsafe); strip only pub/pub(...) from the signature line so
    # bodies compare equal across crates that differ only in visibility.
    # Other modifiers (const, async, unsafe) are semantic — mismatches there are
    # real body divergences the diff must flag.
    awk '/^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?(const[[:space:]]+)?(async[[:space:]]+)?(unsafe[[:space:]]+)?fn[[:space:]]+'"$fn_name"'[[:space:](<]/,/^}/' "$file" |
        sed 's/^pub([^)]*) *//' |
        sed 's/^pub //'
}

# Both copies of sanitize_value must have identical function bodies.
# Capture output first so we can assert non-empty before diffing — an empty
# result from either side would mean the function was not found (e.g. renamed),
# and `diff <() <()` would silently succeed (false negative).
expr_body=$(extract_fn sanitize_value "$EXPR_FILE")
stdlib_body=$(extract_fn sanitize_value "$STDLIB_FILE")
[ -z "$expr_body" ] && { assert "extract_fn sanitize_value found in reify-expr" false; test_summary; }
[ -z "$stdlib_body" ] && { assert "extract_fn sanitize_value found in reify-stdlib" false; test_summary; }
assert \
    "sanitize_value body is identical in reify-expr and reify-stdlib" \
    diff \
        <(printf '%s' "$expr_body") \
        <(printf '%s' "$stdlib_body")

test_summary
