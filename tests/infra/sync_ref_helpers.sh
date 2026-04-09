#!/usr/bin/env bash
# Shared helper for sync cross-reference assertions.
# Defines assert_sync_ref_exists() so tests can source it directly without
# text extraction from sync_comments_test.sh.
#
# Usage:  source "$REPO_ROOT/tests/infra/sync_ref_helpers.sh"
#
# Note: This helper sources test_helpers.sh transitively, so callers that
# source this file automatically get assert() and test_summary() as well.
# The _REIFY_TEST_HELPERS_SH_SOURCED guard prevents double-sourcing side
# effects when callers also source test_helpers.sh explicitly.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_SYNC_REF_HELPERS_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_SYNC_REF_HELPERS_SH_SOURCED=1

_SYNC_REF_HELPERS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$_SYNC_REF_HELPERS_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $_SYNC_REF_HELPERS_DIR/test_helpers.sh"; exit 1; }
source "$_SYNC_REF_HELPERS_DIR/test_helpers.sh"

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
        "fn ${display_fn} exists in ${tgt_crate} (as referenced by SYNC in ${src_crate})" \
        grep -qE '^[[:space:]]*(pub(\([^)]*\))?[[:space:]]+)?(unsafe[[:space:]]+)?(const[[:space:]]+)?(async[[:space:]]+)?fn[[:space:]]+'"${ref_fn}"'[[:space:](<]' "$tgt_file"
}
