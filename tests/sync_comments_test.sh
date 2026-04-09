#!/usr/bin/env bash
# Test: Both copies of sanitize_value carry SYNC cross-reference comments.
#
# sanitize_value exists in two crates — reify-expr and reify-stdlib.
# Each copy must have a `// SYNC:` marker so that `grep SYNC:` finds both
# sites when Value variants change.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXPR_FILE="$REPO_ROOT/crates/reify-expr/src/sanitize.rs"
STDLIB_FILE="$REPO_ROOT/crates/reify-stdlib/src/helpers.rs"

[ -f "$REPO_ROOT/tests/infra/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$REPO_ROOT/tests/infra/test_helpers.sh"

# reify-expr's copy must reference reify-stdlib::sanitize_value
assert \
    "reify-expr/src/lib.rs has SYNC marker referencing reify-stdlib::sanitize_value" \
    grep -q "SYNC:.*reify-stdlib::sanitize_value" "$EXPR_FILE"

# reify-stdlib's copy must reference reify-expr::sanitize_value
assert \
    "reify-stdlib/src/lib.rs has SYNC marker referencing reify-expr::sanitize_value" \
    grep -q "SYNC:.*reify-expr::sanitize_value" "$STDLIB_FILE"

# Helper: verify that source_file's SYNC comment references a function that
# exists in target_file.  Args: source_crate target_crate source_file target_file
assert_sync_ref_exists() {
    local src_crate="$1" tgt_crate="$2" src_file="$3" tgt_file="$4"
    # Only the first SYNC cross-reference is validated here; files with multiple
    # cross-references to the same target crate would require a loop.
    local ref_fn
    ref_fn=$(grep 'SYNC:' "$src_file" | grep -oE "${tgt_crate}::[a-z_]+" | head -1 | sed 's/.*:://' || true)
    if [ -z "$ref_fn" ]; then assert "SYNC in ${src_crate} references a ${tgt_crate} function" false; return; fi
    local display_fn="${ref_fn:-<none>}"
    assert \
        "fn ${display_fn} exists in ${tgt_crate}/src/lib.rs (as referenced by SYNC in ${src_crate})" \
        grep -qE '^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?(unsafe[[:space:]]+)?(const[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+'"${ref_fn}"'[[:space:](<]' "$tgt_file"
}

# Verify each SYNC cross-reference points to a real function in the peer crate
assert_sync_ref_exists reify-expr reify-stdlib "$EXPR_FILE" "$STDLIB_FILE"
assert_sync_ref_exists reify-stdlib reify-expr "$STDLIB_FILE" "$EXPR_FILE"

# Helper: extract from the fn signature line to the next line that begins with }
# at column 0.  Content above the fn keyword is naturally excluded by the anchor,
# so doc comments and SYNC markers (which may legitimately differ between
# the two copies) do not affect the body comparison.
# Handles optional visibility modifiers: fn, pub fn, pub(crate) fn, etc.
# Pattern: ^(pub[^f]*)? skips optional visibility prefix (pub, pub(crate), etc.)
# without consuming the 'f' in 'fn'.
extract_fn() {
    local fn_name="$1" file="$2"
    awk '/^(pub[^f]*)?fn '"$fn_name"'[(<]/,/^}/' "$file"
}

# Both copies of sanitize_value must have identical function bodies.
# Capture output first so we can assert non-empty before diffing — an empty
# result from either side would mean the function was not found (e.g. renamed),
# and `diff <() <()` would silently succeed (false negative).
expr_body=$(extract_fn sanitize_value "$EXPR_FILE")
stdlib_body=$(extract_fn sanitize_value "$STDLIB_FILE")
[ -z "$expr_body" ] && assert "extract_fn sanitize_value found in reify-expr" false
[ -z "$stdlib_body" ] && assert "extract_fn sanitize_value found in reify-stdlib" false
assert \
    "sanitize_value body is identical in reify-expr and reify-stdlib" \
    diff \
        <(printf '%s' "$expr_body") \
        <(printf '%s' "$stdlib_body")

test_summary
