#!/usr/bin/env bash
# Infrastructure test for task H1 (#4921).
# Validates that every tests/infra/test_*.sh (excl. test_helpers.sh) is
# classified into exactly one of three buckets — pool / intra-run-serial /
# host-exclusive — declared in tests/infra/run-all-classification.manifest,
# and that the declared set never drifts from the live discovered set.
#
# Assertions:
#   1. tests/infra/run-all-classification.manifest exists and is non-empty
#      (after stripping comments/blanks).
#   2. Every manifest row is well-formed: exactly 2 fields, and the bucket
#      field is one of the valid enum tokens.
#   3. No test basename is declared in more than one bucket (no overlap).
#   4. Every declared entry resolves to a real file in tests/infra/.
#   5. The declared union (across the 3 valid buckets) EQUALS the live
#      discovered test_*.sh set (drift catcher — mirrors
#      test_occt_gated_scope.sh's Test 3 declared==derived shape).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Source the shared classification library: classification_* functions are
# the SINGLE implementations of the declared-union, discovered-set, overlap,
# and coverage-diff logic, shared with (eventually) run_all.sh itself so the
# guard and the runner cannot drift apart (Test 5 below is the drift catcher
# that proves declared == discovered).
[ -f "$SCRIPT_DIR/run-all-classification-lib.sh" ] || { echo "ERROR: run-all-classification-lib.sh not found at $SCRIPT_DIR/run-all-classification-lib.sh"; exit 1; }
source "$SCRIPT_DIR/run-all-classification-lib.sh"

MANIFEST="$SCRIPT_DIR/run-all-classification.manifest"
MANIFEST_REL="tests/infra/run-all-classification.manifest"

echo "=== run_all.sh classification drift-guard tests ==="

# ---------------------------------------------------------------------------
# Test 1: manifest file exists and is non-empty
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: $MANIFEST_REL exists and is non-empty ---"

assert "$MANIFEST_REL exists" \
    test -f "$MANIFEST"

assert "$MANIFEST_REL is non-empty after stripping comments/blanks" \
    bash -c "[ -f '$MANIFEST' ] && [ -n \"\$(grep -v '^[[:space:]]*#' '$MANIFEST' | grep -v '^[[:space:]]*\$')\" ]"

# ---------------------------------------------------------------------------
# Test 2: every manifest row is well-formed (2 fields, bucket in the enum)
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2: every manifest row is well-formed (2 fields, valid bucket) ---"

if [ -f "$MANIFEST" ]; then
    ALL_BUCKETS="$(classification_all_buckets)"
    export ALL_BUCKETS
    while IFS= read -r _row; do
        [ -z "$_row" ] && continue
        _nf="$(awk '{print NF}' <<< "$_row")"
        assert "manifest row has exactly 2 fields: '$_row'" \
            test "$_nf" -eq 2
        _bucket="$(awk '{print $2}' <<< "$_row")"
        assert "manifest row bucket is valid (pool|intra-run-serial|host-exclusive): '$_row'" \
            bash -c "printf '%s\n' \"\$ALL_BUCKETS\" | grep -qxF -- \"$_bucket\""
    done < <(grep -v '^[[:space:]]*#' "$MANIFEST" | grep -v '^[[:space:]]*$')
fi

# ---------------------------------------------------------------------------
# Test 3: no test basename is declared in more than one bucket
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: no overlap (no test declared in more than one bucket) ---"

_OVERLAP_OUT="$(classification_overlap)"
if [ -n "$_OVERLAP_OUT" ]; then
    echo "  Overlap detected (declared in more than one bucket):"
    echo "$_OVERLAP_OUT" | sed 's/^/    /'
fi
assert "no test basename is declared in more than one bucket" \
    test -z "$_OVERLAP_OUT"

# ---------------------------------------------------------------------------
# Test 4: every declared entry resolves to a real file in tests/infra/
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 4: every declared entry resolves to a file on disk ---"

while IFS= read -r _entry; do
    [ -z "$_entry" ] && continue
    assert "declared entry resolves to a file: '$_entry'" \
        test -f "$SCRIPT_DIR/$_entry"
done < <(classification_declared_union)

# ---------------------------------------------------------------------------
# Test 5: declared union equals the live discovered test_*.sh set (drift
# catcher — mirrors test_occt_gated_scope.sh Test 3's declared==derived
# shape).
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 5: declared union equals the live discovered set (no drift) ---"

_DIFF_OUT="$(classification_coverage_diff)"
if [ -n "$_DIFF_OUT" ]; then
    echo "  Classification drift detected (< declared union, > live discovered set):"
    echo "$_DIFF_OUT" | sed 's/^/    /'
fi
assert "declared union equals the live discovered test_*.sh set (no missing or extra entries)" \
    test -z "$_DIFF_OUT"

test_summary
