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
#   5. Ungated release pass: -p <crate> for each of the 6 non-OCCT release-sensitive
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
# truth): an anchored grep over crates/ and gui/src-tauri/ for the two
# release-sensitivity mechanisms (cfg_attr(debug_assertions, ignore ...) and
# cfg(not(debug_assertions))). The full rationale lives in the release_sensitive_set
# doc comment in scripts/release-scope-lib.sh.
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
# Plan-shape assertions (Tests 4-7)
# Source of truth: scripts/verify.sh --print-plan (the oracle the orchestrator
# calls). --profile both --scope all forces the full plan; env lines are stripped
# via `grep -v '^#'`.
# ---------------------------------------------------------------------------
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --print-plan | grep -v '^#')"

# Split by OCCT membership to build the two release crate lists for assertions.
_OCCT_DECLARED_STR="$(occt_declared_set)"
_is_occt() { grep -qxF "$1" <<< "$_OCCT_DECLARED_STR"; }

_RELEASE_GATED_CRATES=()    # OCCT ∩ release-sensitive (expect: just reify-eval)
_RELEASE_UNGATED_CRATES=()  # release-sensitive ∖ OCCT (expect: the other 6)
while IFS= read -r _c; do
    [ -z "$_c" ] && continue
    if _is_occt "$_c"; then
        _RELEASE_GATED_CRATES+=("$_c")
    else
        _RELEASE_UNGATED_CRATES+=("$_c")
    fi
done <<< "$DECLARED_CRATES"

# Extract the gated release line (via the flock wrapper, with --release).
GATED_RELEASE="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep 'cargo-test-occt-gated\.sh' | grep -- '--release' || true)"
export GATED_RELEASE

# Extract the ungated release line (not via the flock wrapper, with --release).
# After step-4 this uses -p flags instead of --workspace; after step-4 it must
# NOT match --workspace (those assertions are the RED condition before step-4).
UNGATED_RELEASE="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -v 'cargo-test-occt-gated\.sh' \
    | grep -E 'cargo (test|nextest run)' \
    | grep -- '--release' || true)"
export UNGATED_RELEASE

echo ""
echo "--- Test 4: gated release pass is scoped to OCCT-intersect crates only ---"
# Only OCCT-intersect crates (reify-eval) should appear in the gated release pass.
for _c in "${_RELEASE_GATED_CRATES[@]}"; do
    assert "gated release has '-p $_c'" \
        bash -c "printf '%s' \"\$GATED_RELEASE\" | grep -qF ' -p $_c'"
done
# The 3 other OCCT crates (reify-kernel-occt, reify-cli, reify-config) must NOT appear.
_OCCT_RELEASE_ABSENT=(reify-kernel-occt reify-cli reify-config)
for _c in "${_OCCT_RELEASE_ABSENT[@]}"; do
    assert "gated release does NOT have '-p $_c'" \
        bash -c "! printf '%s' \"\$GATED_RELEASE\" | grep -qF ' -p $_c'"
done
assert "gated release does not contain --workspace" \
    bash -c "! printf '%s' \"\$GATED_RELEASE\" | grep -qF ' --workspace'"
assert "gated release has '-- --test-threads=1'" \
    bash -c "printf '%s' \"\$GATED_RELEASE\" | grep -qF -- '-- --test-threads=1'"
assert "gated release has 'REIFY_OCCT_TEST_TIMEOUT=4800'" \
    bash -c "printf '%s' \"\$GATED_RELEASE\" | grep -qF 'REIFY_OCCT_TEST_TIMEOUT=4800'"

echo ""
echo "--- Test 5: ungated release pass has '-p <crate>' for each non-OCCT release-sensitive crate ---"
for _c in "${_RELEASE_UNGATED_CRATES[@]}"; do
    assert "ungated release has '-p $_c'" \
        bash -c "printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' -p $_c'"
done
assert "ungated release has '--release'" \
    bash -c "printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' --release'"
assert "ungated release does NOT contain --workspace" \
    bash -c "! printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' --workspace'"
assert "ungated release does NOT contain --exclude" \
    bash -c "! printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' --exclude'"
assert "ungated release does NOT have '-p reify-eval'" \
    bash -c "! printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' -p reify-eval'"

echo ""
echo "--- Test 6: DEBUG pass unchanged (gated debug has all 4 OCCT crates; ungated debug uses --workspace --exclude) ---"
GATED_DEBUG="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep 'cargo-test-occt-gated\.sh' | grep -v -- '--release' || true)"
UNGATED_DEBUG="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -E 'cargo (test|nextest run) --workspace' \
    | grep -v 'cargo-test-occt-gated\.sh' | grep -v -- '--release' || true)"
export GATED_DEBUG UNGATED_DEBUG

# Gated debug must still have ALL 4 OCCT crates.
while IFS= read -r _c; do
    [ -z "$_c" ] && continue
    assert "gated debug has '-p $_c' (OCCT crate, unchanged)" \
        bash -c "printf '%s' \"\$GATED_DEBUG\" | grep -qF ' -p $_c'"
done <<< "$_OCCT_DECLARED_STR"

assert "ungated debug has --workspace (unchanged)" \
    bash -c "printf '%s' \"\$UNGATED_DEBUG\" | grep -qF ' --workspace'"
assert "ungated debug has --exclude (unchanged)" \
    bash -c "printf '%s' \"\$UNGATED_DEBUG\" | grep -qF ' --exclude'"
assert "ungated debug does NOT have --release" \
    bash -c "! printf '%s' \"\$UNGATED_DEBUG\" | grep -qF ' --release'"

echo ""
echo "--- Test 7: gated release appears before ungated release in the plan ---"
_GATED_RELEASE_IDX="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -n 'cargo-test-occt-gated\.sh' | grep -- '--release' | head -1 | cut -d: -f1 || true)"
# After step-4 the ungated release no longer uses --workspace; match any non-gated
# cargo (test|nextest run) line with --release.
_UNGATED_RELEASE_IDX="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -nE 'cargo (test|nextest run)' \
    | grep -v 'cargo-test-occt-gated' \
    | grep -- '--release' | head -1 | cut -d: -f1 || true)"
assert "gated release (segment ${_GATED_RELEASE_IDX:-?}) precedes ungated release (segment ${_UNGATED_RELEASE_IDX:-?})" \
    bash -c "[ '${_GATED_RELEASE_IDX:-0}' -gt 0 ] && [ '${_UNGATED_RELEASE_IDX:-0}' -gt 0 ] && [ '${_GATED_RELEASE_IDX:-0}' -lt '${_UNGATED_RELEASE_IDX:-0}' ]"

test_summary
