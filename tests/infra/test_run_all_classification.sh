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

# Load-tolerant retry-budget helper (task 4585): guarded, fail-open — a
# missing lib just leaves load_tolerant_attempts undefined, and
# classification_stable_empty (run-all-classification-lib.sh) already
# degrades to its fixed BASE attempt count via `declare -F` when that's the
# case, so this guard test adds no hard dependency on the lib's presence.
if [ -f "$SCRIPT_DIR/load_tolerance_lib.sh" ]; then
    # shellcheck disable=SC1091
    source "$SCRIPT_DIR/load_tolerance_lib.sh"
fi

MANIFEST="$SCRIPT_DIR/run-all-classification.manifest"
MANIFEST_REL="tests/infra/run-all-classification.manifest"

# Shared retry-budget base (task 5251) for the three load-fragile guard
# verdicts below (malformed-rows in Test 2, overlap in Test 3, coverage_diff
# in Test 5). Each is routed through classification_stable_empty with this
# same BASE, which auto-scales via load_tolerant_attempts when in scope (see
# run-all-classification-lib.sh) so a transient shell hiccup under
# concurrent-verify load is retried away while a stable/genuine condition
# still surfaces (guard integrity locked in by Test 6 (e)/(f) above).
_CLASSIFICATION_STABLE_BASE=3

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

_MALFORMED="$(classification_stable_empty "$_CLASSIFICATION_STABLE_BASE" classification_malformed_rows)" || true
if [ -n "$_MALFORMED" ]; then
    echo "  Malformed row(s) detected (not exactly 2 fields, or invalid bucket):"
    echo "$_MALFORMED" | sed 's/^/    /'
fi
assert "every manifest row is well-formed (2 fields, valid bucket)" \
    test -z "$_MALFORMED"

# ---------------------------------------------------------------------------
# Test 3: no test basename is declared in more than one bucket
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: no overlap (no test declared in more than one bucket) ---"

_OVERLAP_OUT="$(classification_stable_empty "$_CLASSIFICATION_STABLE_BASE" classification_overlap)" || true
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

_DIFF_OUT="$(classification_stable_empty "$_CLASSIFICATION_STABLE_BASE" classification_coverage_diff)" || true
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

# (d) Malformed-row fixture: real manifest plus one appended 3-field row.
# classification_malformed_rows against this fixture must report NON-empty
# (an injected malformed row must surface, not be silently accepted).
_MALFORMED_SELFCHECK_MANIFEST="$_SELFCHECK_TMPDIR/malformed_selfcheck.manifest"
cp "$MANIFEST" "$_MALFORMED_SELFCHECK_MANIFEST"
echo "test_bogus_malformed_selfcheck_fixture.sh pool extra" >> "$_MALFORMED_SELFCHECK_MANIFEST"

_MALFORMED_SELFCHECK_OUT="$(classification_malformed_rows "$_MALFORMED_SELFCHECK_MANIFEST")"
assert "classification_malformed_rows on a manifest with an injected 3-field row reports NON-empty" \
    test -n "$_MALFORMED_SELFCHECK_OUT"

# (e) Guard-integrity regression lock: routing each injected fixture THROUGH
# the classification_stable_empty retry wrapper must STILL report the
# injected condition (the wrapper masks only non-reproducing transients,
# never a stable/genuine condition — this is what makes it safe to route the
# live guard's verdicts through it in place, below).
_WRAPPED_DRIFT_RC=0
_WRAPPED_DRIFT_OUT="$(classification_stable_empty 3 classification_coverage_diff "$_DRIFT_MANIFEST")" || _WRAPPED_DRIFT_RC=$?
assert "wrapper on the injected drift fixture still reports NON-empty (guard integrity)" \
    test -n "$_WRAPPED_DRIFT_OUT"

_WRAPPED_OVERLAP_RC=0
_WRAPPED_OVERLAP_OUT="$(classification_stable_empty 3 classification_overlap "$_OVERLAP_MANIFEST")" || _WRAPPED_OVERLAP_RC=$?
assert "wrapper on the injected overlap fixture still reports NON-empty (guard integrity)" \
    test -n "$_WRAPPED_OVERLAP_OUT"

_WRAPPED_MALFORMED_RC=0
_WRAPPED_MALFORMED_OUT="$(classification_stable_empty 3 classification_malformed_rows "$_MALFORMED_SELFCHECK_MANIFEST")" || _WRAPPED_MALFORMED_RC=$?
assert "wrapper on the injected malformed-row fixture still reports NON-empty (guard integrity)" \
    test -n "$_WRAPPED_MALFORMED_OUT"

# (f) Sanity: all three verdicts routed through the wrapper on the REAL
# manifest are EMPTY (the wrapper is green on truth, same as the raw
# accessors in (c) above).
_WRAPPED_REAL_DRIFT_RC=0
_WRAPPED_REAL_DRIFT_OUT="$(classification_stable_empty 3 classification_coverage_diff "$MANIFEST")" || _WRAPPED_REAL_DRIFT_RC=$?
assert "wrapper on the real manifest yields EMPTY coverage_diff" \
    test -z "$_WRAPPED_REAL_DRIFT_OUT"

_WRAPPED_REAL_OVERLAP_RC=0
_WRAPPED_REAL_OVERLAP_OUT="$(classification_stable_empty 3 classification_overlap "$MANIFEST")" || _WRAPPED_REAL_OVERLAP_RC=$?
assert "wrapper on the real manifest yields EMPTY overlap" \
    test -z "$_WRAPPED_REAL_OVERLAP_OUT"

_WRAPPED_REAL_MALFORMED_RC=0
_WRAPPED_REAL_MALFORMED_OUT="$(classification_stable_empty 3 classification_malformed_rows "$MANIFEST")" || _WRAPPED_REAL_MALFORMED_RC=$?
assert "wrapper on the real manifest yields EMPTY malformed_rows" \
    test -z "$_WRAPPED_REAL_MALFORMED_OUT"

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

# ---------------------------------------------------------------------------
# Test: classification_malformed_rows accessor
# ---------------------------------------------------------------------------
echo ""
echo "--- Test: classification_malformed_rows accessor ---"

_MALFORMED_ROWS_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$_SELFCHECK_TMPDIR" "$_MALFORMED_ROWS_TMPDIR"' EXIT

_MALFORMED_FIXTURE="$_MALFORMED_ROWS_TMPDIR/malformed_rows.manifest"
cat > "$_MALFORMED_FIXTURE" <<'EOF'
test_x.sh pool
test_y.sh pool extra
test_z.sh bogusbucket
EOF

_MALFORMED_FIXTURE_OUT="$(classification_malformed_rows "$_MALFORMED_FIXTURE")"
export _MALFORMED_FIXTURE_OUT

assert "classification_malformed_rows detects the malformed fixture rows (non-empty)" \
    test -n "$_MALFORMED_FIXTURE_OUT"

assert "classification_malformed_rows fixture output contains the 3-field row (test_y.sh)" \
    bash -c "printf '%s\n' \"\$_MALFORMED_FIXTURE_OUT\" | grep -qF -- test_y.sh"

assert "classification_malformed_rows fixture output contains the invalid-bucket row (test_z.sh)" \
    bash -c "printf '%s\n' \"\$_MALFORMED_FIXTURE_OUT\" | grep -qF -- test_z.sh"

assert "classification_malformed_rows fixture output does NOT contain the well-formed row (test_x.sh)" \
    bash -c "! printf '%s\n' \"\$_MALFORMED_FIXTURE_OUT\" | grep -qF -- test_x.sh"

assert "classification_malformed_rows on the real manifest is EMPTY (guard green on truth)" \
    test -z "$(classification_malformed_rows "$MANIFEST")"

# ---------------------------------------------------------------------------
# Test: classification_stable_empty retry-until-clean wrapper
# ---------------------------------------------------------------------------
echo ""
echo "--- Test: classification_stable_empty retry-until-clean wrapper ---"

_STABLE_EMPTY_TMPDIR="$(mktemp -d)"
trap 'rm -rf "$_SELFCHECK_TMPDIR" "$_MALFORMED_ROWS_TMPDIR" "$_STABLE_EMPTY_TMPDIR"' EXIT

# (a) Always-clean fake: prints nothing, returns 0 on every invocation.
_fake_always_clean() {
    return 0
}

# (b) Blip-then-clean fake: prints "blip" on its FIRST invocation only (a
# counter file records invocation count so later invocations know they are
# not the first) — models a non-reproducing transient shell hiccup that
# clears on retry.
_fake_blip_then_clean() {
    local _counter="$_STABLE_EMPTY_TMPDIR/blip_counter"
    local _n=0
    [ -f "$_counter" ] && _n="$(cat "$_counter")"
    _n=$((_n + 1))
    printf '%s' "$_n" > "$_counter"
    [ "$_n" -eq 1 ] && echo "blip"
    return 0
}

# (c) Always-drift fake: ALWAYS prints "DRIFT" — models a STABLE, genuine
# condition that must survive the whole retry budget (guard integrity: the
# wrapper must never mask this).
_fake_always_drift() {
    echo "DRIFT"
    return 0
}

_CLEAN_RC=0
_CLEAN_OUT="$(classification_stable_empty 3 _fake_always_clean)" || _CLEAN_RC=$?
assert "classification_stable_empty: always-clean fake yields EMPTY output" \
    test -z "$_CLEAN_OUT"
assert "classification_stable_empty: always-clean fake returns rc 0" \
    test "$_CLEAN_RC" -eq 0

_BLIP_RC=0
_BLIP_OUT="$(classification_stable_empty 3 _fake_blip_then_clean)" || _BLIP_RC=$?
assert "classification_stable_empty: a non-reproducing transient blip (1st call only) is masked by retry -> EMPTY output" \
    test -z "$_BLIP_OUT"
assert "classification_stable_empty: a non-reproducing transient blip (1st call only) is masked by retry -> rc 0" \
    test "$_BLIP_RC" -eq 0

# NB: classification_stable_empty returns rc 1 on a STABLE non-empty verdict
# (by design — see its header doc). The capture below MUST tolerate that via
# `|| _DRIFT_RC=$?` (not left unguarded): an unguarded `X="$(cmd)"` whose cmd
# exits 1 would trip `set -e` and silently abort this whole script before the
# assertions below ever run (the same hazard step-6 documents for the live
# guard's own `_OVERLAP_OUT`/`_DIFF_OUT`/`_MALFORMED` captures).
_DRIFT_RC=0
_DRIFT_OUT="$(classification_stable_empty 3 _fake_always_drift)" || _DRIFT_RC=$?
export _DRIFT_OUT
assert "classification_stable_empty: a STABLE genuine condition (always DRIFT) is NOT masked -> non-empty output" \
    test -n "$_DRIFT_OUT"
assert "classification_stable_empty: a STABLE genuine condition (always DRIFT) output contains DRIFT" \
    bash -c "printf '%s\n' \"\$_DRIFT_OUT\" | grep -qF -- DRIFT"
assert "classification_stable_empty: a STABLE genuine condition (always DRIFT) returns rc 1 (not masked, guard integrity)" \
    test "$_DRIFT_RC" -eq 1

test_summary
