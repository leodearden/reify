#!/usr/bin/env bash
# Infrastructure test for task 4390.
# Validates that the release-sensitive crate list is correct and that
# verify.sh emits a correctly scoped release test pass.
#
# Assertions (drift-guard core, mirroring test_occt_gated_scope.sh Tests 1-3):
#   1. scripts/release-sensitive-crates.txt exists and is non-empty (after stripping comments/blanks).
#   2. Every declared entry is a real workspace member.
#   3. Declared set EQUALS the grep-derived release-sensitive set (drift catcher).
#      This is the load-bearing guard: it fails if a new release-sensitive test
#      lands in a crate not on the declared list.
#
# Plan-shape assertions (--print-plan oracle, added in step-3):
#   4. Gated release pass: cargo-test-occt-gated.sh with -p reify-eval --release
#      -- --test-threads=1; NO other OCCT crate; NO --workspace.
#   5. Ungated release pass: -p <crate> for each of the non-OCCT release-sensitive
#      crates; has --release; NO --workspace; NO --exclude; NO -p reify-eval.
#   6. Debug pass unchanged: gated debug still has all 4 OCCT crates; ungated debug
#      still uses --workspace --exclude.
#   7. Ordering: gated-release precedes ungated-release in the plan.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Source the shared release-scope library: release_declared_set + release_sensitive_set
# are the SINGLE implementations of the declared and grep-derived sets, shared with
# scripts/verify.sh so the two cannot drift apart (Test 3 below is the drift catcher
# that proves they agree).
[ -f "$REPO_ROOT/scripts/release-scope-lib.sh" ] || { echo "ERROR: release-scope-lib.sh not found at $REPO_ROOT/scripts/release-scope-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/release-scope-lib.sh"

# Source the OCCT-scope library to determine which release-sensitive crates are
# OCCT-touching (and therefore stay flock-gated in release).
[ -f "$REPO_ROOT/scripts/occt-scope-lib.sh" ] || { echo "ERROR: occt-scope-lib.sh not found at $REPO_ROOT/scripts/occt-scope-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/occt-scope-lib.sh"

CRATE_LIST="$REPO_ROOT/scripts/release-sensitive-crates.txt"

echo "=== Release-scoped scope tests ==="

# ---------------------------------------------------------------------------
# Test 1: declared list file exists and is non-empty
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: scripts/release-sensitive-crates.txt exists and is non-empty ---"

assert "scripts/release-sensitive-crates.txt exists" \
    test -f "$CRATE_LIST"

assert "scripts/release-sensitive-crates.txt is non-empty after stripping comments/blanks" \
    bash -c "[ -f '$CRATE_LIST' ] && [ -n \"\$(grep -v '^\s*#' '$CRATE_LIST' | grep -v '^\s*\$')\" ]"

# ---------------------------------------------------------------------------
# Test 2: every declared entry is a real workspace member
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2: every declared crate is a real workspace member ---"

# Collect workspace members via cargo metadata.
WORKSPACE_MEMBERS="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c "import sys,json; m=json.load(sys.stdin); [print(p['name']) for p in m['packages']]")"

# Declared set comes from the shared library (single source of truth).
DECLARED_CRATES="$(release_declared_set)"

while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "declared crate '$crate' is a real workspace member" \
        grep -qxF "$crate" <<< "$WORKSPACE_MEMBERS"
done <<< "$DECLARED_CRATES"

# ---------------------------------------------------------------------------
# Test 3: declared set equals grep-derived release-sensitive set
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: declared set equals grep-derived release-sensitive set ---"

# Actual release-sensitive set comes from the shared library (single source of
# truth): an anchored grep over crates/ and gui/src-tauri/ for the three
# release-sensitivity mechanisms (cfg_attr(debug_assertions, ignore ...) /
# cfg(not(debug_assertions)) / runtime cfg!(debug_assertions)).  The full
# rationale lives in the release_sensitive_set doc comment in
# scripts/release-scope-lib.sh.
ACTUAL_SENSITIVE="$(release_sensitive_set)"

# Write both sets to temp files and diff for actionable failure output.
# On mismatch the diff is printed so the reader can see exactly which crate
# drifted without re-running locally.
_DECLARED_TMP="$(mktemp)"
_ACTUAL_TMP="$(mktemp)"
echo "$DECLARED_CRATES" | sort > "$_DECLARED_TMP"
echo "$ACTUAL_SENSITIVE" | sort > "$_ACTUAL_TMP"
_DIFF_OUT="$(diff "$_DECLARED_TMP" "$_ACTUAL_TMP" 2>&1 || true)"
rm -f "$_DECLARED_TMP" "$_ACTUAL_TMP"
if [ -n "$_DIFF_OUT" ]; then
    echo "  Release-sensitive set drift detected (< declared, > grep-derived):"
    echo "$_DIFF_OUT" | sed 's/^/    /'
fi
assert "declared release-sensitive set equals grep-derived set (no missing or extra entries)" \
    test -z "$_DIFF_OUT"

# ---------------------------------------------------------------------------
# Test 3a: Mechanism C regression guard — runtime cfg!(debug_assertions) branching
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3a: grep-derived set includes reify-mesh-morph (Mechanism C regression guard) ---"
# crates/reify-mesh-morph/src/diagnostics.rs:511 uses cfg!(debug_assertions) at
# RUNTIME (not as a compile-time attribute), so Mechanisms A and B miss it.  The test
# record_quality_remesh_pass_never_touches_a_counter asserts different outcomes in debug
# vs release, meaning the release pass is required to cover the release-only no-op path.
# This assertion is RED against the current two-mechanism detector; step-7's Mechanism C
# (anchored '^[^/]*cfg!(debug_assertions)' grep) turns it GREEN.
_MESH_MORPH_IN_DERIVED="$(printf '%s\n' "$ACTUAL_SENSITIVE" | grep -cxF 'reify-mesh-morph' || echo 0)"
assert "grep-derived release_sensitive_set includes reify-mesh-morph (Mechanism C runtime cfg! branch)" \
    test "${_MESH_MORPH_IN_DERIVED:-0}" -gt 0

# ---------------------------------------------------------------------------
# Plan-shape assertions (Tests 4-7) — task 4451 folded contract
# Source of truth: scripts/verify.sh --print-plan (the oracle the orchestrator
# calls). --profile both --scope all forces the full plan; env lines are stripped
# via `grep -v '^#'`.
#
# Folded contract (task 4451):
#   - NO cargo-test-occt-gated.sh invocation in the plan
#   - Release nextest pass: one `cargo nextest run -p <all-release-sensitive> --release`
#     (reify-eval is now folded into the nextest release pass alongside non-OCCT crates)
#   - Debug pass: `cargo nextest run --workspace` with NO --exclude (OCCT folded in)
# ---------------------------------------------------------------------------
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --print-plan | grep -v '^#')"

# All release-sensitive crates (declared set from the shared lib).
_OCCT_DECLARED_STR="$(occt_declared_set)"

# Extract the nextest release pass (no gated wrapper).
NEXTEST_RELEASE="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -v 'cargo-test-occt-gated\.sh' \
    | grep -E 'cargo nextest run' \
    | grep -- '--release' || true)"
export NEXTEST_RELEASE

echo ""
echo "--- Test 4: no gated release pass; nextest release pass exists with -p reify-eval (task 4451) ---"
# Task 4451: reify-eval (OCCT ∩ release-sensitive) is folded into the nextest
# release pass. No flock wrapper is used; the nextest occt group bounds concurrency.
assert "plan has NO cargo-test-occt-gated.sh invocation at all (task 4451: fold)" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'cargo-test-occt-gated\.sh'"
assert "nextest release pass exists (cargo nextest run ... --release, no gated wrapper)" \
    test -n "$NEXTEST_RELEASE"
assert "nextest release pass has '-p reify-eval' (OCCT∩release-sensitive, folded, task 4451)" \
    bash -c "printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' -p reify-eval'"
assert "nextest release pass does NOT have '--workspace' (sensitivity-scoped)" \
    bash -c "! printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' --workspace'"
assert "nextest release pass does NOT have '-- --test-threads=1' (nextest serializes via occt group)" \
    bash -c "! printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF '-- --test-threads=1'"

echo ""
echo "--- Test 5: nextest release pass has '-p <crate>' for each release-sensitive crate (task 4451) ---"
# After the fold, ALL release-sensitive crates (incl. reify-eval) go through the
# single nextest release pass. The OCCT/non-OCCT split is gone.
while IFS= read -r _c; do
    [ -z "$_c" ] && continue
    assert "nextest release has '-p $_c' (release-sensitive, task 4451)" \
        bash -c "printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' -p $_c'"
done <<< "$DECLARED_CRATES"
assert "nextest release has '--release'" \
    bash -c "printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' --release'"
assert "nextest release does NOT contain --workspace" \
    bash -c "! printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' --workspace'"
assert "nextest release does NOT contain --exclude" \
    bash -c "! printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' --exclude'"

echo ""
echo "--- Test 5a: reify-mesh-morph in nextest release pass (Mechanism C regression guard, task 4451) ---"
# reify-mesh-morph is release-sensitive (Mechanism C) and non-OCCT; it goes
# into the single nextest release pass (no gated/ungated split after the fold).
assert "nextest release has '-p reify-mesh-morph' (Mechanism C regression guard, task 4451)" \
    bash -c "printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' -p reify-mesh-morph'"

echo ""
echo "--- Test 6: debug pass is 'cargo nextest run --workspace' with NO --exclude (OCCT folded in, task 4451) ---"
NEXTEST_DEBUG="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -E 'cargo nextest run --workspace' \
    | grep -v -- '--release' || true)"
export NEXTEST_DEBUG

assert "debug nextest pass exists (cargo nextest run --workspace, no --release)" \
    test -n "$NEXTEST_DEBUG"
assert "debug nextest pass has --workspace (full-workspace)" \
    bash -c "printf '%s' \"\$NEXTEST_DEBUG\" | grep -qF ' --workspace'"
assert "debug nextest pass has NO --exclude (OCCT folded in, task 4451)" \
    bash -c "! printf '%s' \"\$NEXTEST_DEBUG\" | grep -qF ' --exclude'"
assert "debug nextest pass does NOT have --release" \
    bash -c "! printf '%s' \"\$NEXTEST_DEBUG\" | grep -qF ' --release'"

echo ""
echo "--- Test 7: debug pass appears before release pass in the plan (task 4451) ---"
# Ordering: debug nextest pass (--workspace) before release nextest pass (-p ... --release).
_DEBUG_IDX="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -nE 'cargo nextest run --workspace' | grep -v -- '--release' | head -1 | cut -d: -f1 || true)"
_RELEASE_IDX="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -v 'cargo-test-occt-gated' \
    | grep -nE 'cargo nextest run' \
    | grep -- '--release' | head -1 | cut -d: -f1 || true)"
assert "debug nextest pass (segment ${_DEBUG_IDX:-?}) precedes release nextest pass (segment ${_RELEASE_IDX:-?})" \
    bash -c "[ '${_DEBUG_IDX:-0}' -gt 0 ] && [ '${_RELEASE_IDX:-0}' -gt 0 ] && [ '${_DEBUG_IDX:-0}' -lt '${_RELEASE_IDX:-0}' ]"

test_summary
