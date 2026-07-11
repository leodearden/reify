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
#   6. Non-vacuity self-check: injected drift (a missing declared entry) and
#      injected overlap (a duplicated entry) are each detected as NON-empty
#      by the corresponding accessor, proving the guard is not vacuously
#      green; the real manifest still yields EMPTY from both (sanity).

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

# ---------------------------------------------------------------------------
# Test 6: non-vacuity self-check — prove the guard actually goes RED on
# injected drift/overlap, not merely green by accident on the current
# manifest. Uses synthetic temp-file manifests; cleans up via trap.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 6: non-vacuity self-check (guard detects injected drift/overlap) ---"

_SELFCHECK_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$_SELFCHECK_TMPDIR"' EXIT

# (a) Drift fixture: real manifest minus one declared row. The coverage diff
# against this fixture must report NON-empty (a declared entry silently
# disappearing must surface as drift).
_DRIFT_MANIFEST="$_SELFCHECK_TMPDIR/drift.manifest"
# NB: `awk 'NR==1'` (not `head -n1`) — under `set -o pipefail` a `head` that
# closes the pipe after line 1 can SIGPIPE (141) the still-writing producer,
# aborting the script under load (esc-5172-1 flake). awk consumes the whole
# stream, so the producer never gets SIGPIPE; output is identical.
_FIRST_ENTRY="$(classification_declared_union | awk 'NR==1')"
awk -v drop="$_FIRST_ENTRY" '$1 != drop' "$MANIFEST" > "$_DRIFT_MANIFEST"

_DRIFT_DIFF="$(classification_coverage_diff "$_DRIFT_MANIFEST")"
assert "coverage_diff on a manifest missing a declared entry ('$_FIRST_ENTRY') reports NON-empty drift" \
    test -n "$_DRIFT_DIFF"

# (b) Overlap fixture: real manifest plus a duplicate row for an existing
# entry, filed under a second (different) bucket. The overlap check against
# this fixture must report NON-empty.
_OVERLAP_MANIFEST="$_SELFCHECK_TMPDIR/overlap.manifest"
cp "$MANIFEST" "$_OVERLAP_MANIFEST"
_DUP_LINE="$(grep -v '^[[:space:]]*#' "$MANIFEST" | grep -v '^[[:space:]]*$' | awk 'NR==1')"
_DUP_NAME="$(awk '{print $1}' <<< "$_DUP_LINE")"
_DUP_BUCKET="$(awk '{print $2}' <<< "$_DUP_LINE")"
_OTHER_BUCKET="$(classification_all_buckets | grep -vxF "$_DUP_BUCKET" | awk 'NR==1')"
echo "$_DUP_NAME $_OTHER_BUCKET" >> "$_OVERLAP_MANIFEST"

_OVERLAP_DIFF="$(classification_overlap "$_OVERLAP_MANIFEST")"
assert "overlap check on a manifest with a duplicated entry ('$_DUP_NAME') reports NON-empty overlap" \
    test -n "$_OVERLAP_DIFF"

# (c) Sanity: the REAL manifest still yields EMPTY from both checks (the
# guard is green on the true partition, not merely broken in a way that
# always fails).
assert "the real manifest yields EMPTY coverage_diff (sanity: guard is green on truth)" \
    test -z "$(classification_coverage_diff "$MANIFEST")"
assert "the real manifest yields EMPTY overlap (sanity: guard is green on truth)" \
    test -z "$(classification_overlap "$MANIFEST")"

# ---------------------------------------------------------------------------
# Test 7: load-flaky host-burn tests are host-exclusive (task 4997 / esc-4986
# regression lock). test_cpu_load_governance.sh performs real CPU-burn + real
# cgroup delegation (the alpha/beta/gamma composition proof; PRD §8 boundary
# rows ROW1-ROW4) and false-REDs under concurrent host load when classified
# `pool` — H5's (task 4926) confined-cgroup-quota rescue did not converge:
# four row-level deflake tasks (4656/4846/4967/4970) have since landed and it
# still false-REDs under extreme host saturation. Pin it (and its deflake
# harness sibling) to host-exclusive so a future edit cannot silently move it
# back to `pool` and reintroduce the per-task-gate flake.
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 7: load-flaky host-burn tests are host-exclusive ---"

_HOST_EXCLUSIVE="$(classification_bucket host-exclusive)"
export _HOST_EXCLUSIVE
_POOL="$(classification_bucket pool)"
export _POOL

assert "test_cpu_load_governance.sh is classified host-exclusive" \
    bash -c "printf '%s\n' \"\$_HOST_EXCLUSIVE\" | grep -qxF -- test_cpu_load_governance.sh"

assert "test_cpu_load_governance.sh is NOT classified pool" \
    bash -c "! printf '%s\n' \"\$_POOL\" | grep -qxF -- test_cpu_load_governance.sh"

assert "test_cpu_load_governance_deflake.sh remains classified host-exclusive" \
    bash -c "printf '%s\n' \"\$_HOST_EXCLUSIVE\" | grep -qxF -- test_cpu_load_governance_deflake.sh"

test_summary
