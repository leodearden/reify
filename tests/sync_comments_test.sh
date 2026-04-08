#!/usr/bin/env bash
# Test: Both copies of sanitize_value carry SYNC cross-reference comments.
#
# sanitize_value exists in two crates — reify-expr and reify-stdlib.
# Each copy must have a `// SYNC:` marker so that `grep SYNC:` finds both
# sites when Value variants change.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXPR_FILE="$REPO_ROOT/crates/reify-expr/src/lib.rs"
STDLIB_FILE="$REPO_ROOT/crates/reify-stdlib/src/lib.rs"

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
        grep -qE '^[[:space:]]*(pub[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+'"${ref_fn}"'[[:space:](]' "$tgt_file"
}

# Verify each SYNC cross-reference points to a real function in the peer crate
assert_sync_ref_exists reify-expr reify-stdlib "$EXPR_FILE" "$STDLIB_FILE"
assert_sync_ref_exists reify-stdlib reify-expr "$STDLIB_FILE" "$EXPR_FILE"

test_summary
