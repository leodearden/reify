#!/usr/bin/env bash
# Infrastructure test for task 2000.
# Validates that the OCCT-touching crate list is correct and that
# orchestrator.yaml routes exactly those crates through the flock gate.
#
# Assertions:
#   1. scripts/occt-touching-crates.txt exists and is non-empty (after stripping comments/blanks).
#   2. Every declared entry is a real workspace member.
#   3. Declared set EQUALS the cargo-tree-derived OCCT-touching set (drift catcher).
#   4. Each declared crate has -p <crate> in the gated debug AND release invocations.
#   5. The gated invocations do NOT contain --workspace.
#   6. Each declared crate has --exclude <crate> in the ungated debug AND release invocations.
#   7. Each ungated invocation is wrapped with timeout --kill-after=60 [0-9]+m.
#   8. Gated debug invocation appears before ungated debug invocation (ordering).
#   9. Gated release invocation appears before ungated release invocation (ordering).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Source the shared OCCT-scope library: occt_declared_set + occt_touching_set are
# the SINGLE implementations of the declared and cargo-metadata-derived sets,
# shared with scripts/verify.sh so the two cannot drift apart (Test 3 below is the
# drift catcher that proves they agree).
[ -f "$REPO_ROOT/scripts/occt-scope-lib.sh" ] || { echo "ERROR: occt-scope-lib.sh not found at $REPO_ROOT/scripts/occt-scope-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/occt-scope-lib.sh"

CRATE_LIST="$REPO_ROOT/scripts/occt-touching-crates.txt"

echo "=== OCCT gated scope tests ==="

# ---------------------------------------------------------------------------
# Test 1: declared list file exists and is non-empty
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: scripts/occt-touching-crates.txt exists and is non-empty ---"

assert "scripts/occt-touching-crates.txt exists" \
    test -f "$CRATE_LIST"

assert "scripts/occt-touching-crates.txt is non-empty after stripping comments/blanks" \
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
DECLARED_CRATES="$(occt_declared_set)"

while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "declared crate '$crate' is a real workspace member" \
        grep -qxF "$crate" <<< "$WORKSPACE_MEMBERS"
done <<< "$DECLARED_CRATES"

# ---------------------------------------------------------------------------
# Test 3: declared set equals cargo-metadata-derived OCCT-touching set
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: declared set equals cargo-metadata-derived OCCT-touching set ---"

# Actual OCCT-touching set comes from the shared library (single source of
# truth): a single `cargo metadata` invocation over the workspace-unified
# resolve graph. The full rationale for that approach lives in the
# occt_touching_set doc comment in scripts/occt-scope-lib.sh.
ACTUAL_TOUCHING="$(occt_touching_set)"

# Write both sets to temp files and diff for actionable failure output.
# On mismatch the diff is printed so the reader can see exactly which crate
# drifted without re-running locally.
_DECLARED_TMP="$(mktemp)"
_ACTUAL_TMP="$(mktemp)"
echo "$DECLARED_CRATES" | sort > "$_DECLARED_TMP"
echo "$ACTUAL_TOUCHING" | sort > "$_ACTUAL_TMP"
_DIFF_OUT="$(diff "$_DECLARED_TMP" "$_ACTUAL_TMP" 2>&1 || true)"
rm -f "$_DECLARED_TMP" "$_ACTUAL_TMP"
if [ -n "$_DIFF_OUT" ]; then
    echo "  OCCT-touching set drift detected (< declared, > cargo-metadata-derived):"
    echo "$_DIFF_OUT" | sed 's/^/    /'
fi
assert "declared OCCT-touching set equals cargo-metadata-derived set (no missing or extra entries)" \
    test -z "$_DIFF_OUT"

# ---------------------------------------------------------------------------
# Tests 4–5: gated invocations use -p <crate> (not --workspace)
# ---------------------------------------------------------------------------
# Source of truth is now scripts/verify.sh --print-plan (the oracle that the
# orchestrator itself calls), NOT orchestrator.yaml's inlined test_command.
# --scope all forces the full plan so the result is independent of the working
# index; --print-plan emits one command per line with env lines as '# ' comments
# (stripped via `grep -v '^#'`). Both runner spellings (cargo test / cargo
# nextest run) are accepted in the ungated assertions below.
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --print-plan | grep -v '^#')"
GATED_DEBUG="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep 'cargo-test-occt-gated\.sh' | grep -v -- '--release' || true)"
GATED_RELEASE="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep 'cargo-test-occt-gated\.sh' | grep -- '--release' || true)"
export GATED_DEBUG GATED_RELEASE

echo ""
echo "--- Test 4: gated debug invocation has '-p <crate>' for each declared crate ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "gated debug invocation has '-p $crate'" \
        bash -c "printf '%s' \"\$GATED_DEBUG\" | grep -qF ' -p $crate'"
done <<< "$DECLARED_CRATES"

echo ""
echo "--- Test 5: gated release invocation scoped to only reify-eval (sensitivity-scoped) ---"
# Only reify-eval is in both the OCCT set and the release-sensitive set (it has
# OCCT C++ deps AND debug_assertions-dependent tests). The other 3 OCCT crates
# (reify-kernel-occt, reify-cli, reify-config) have zero release-sensitive tests
# and correctly drop out of the release pass.
assert "gated release invocation has '-p reify-eval'" \
    bash -c "printf '%s' \"\$GATED_RELEASE\" | grep -qF ' -p reify-eval'"
assert "gated release does NOT have '-p reify-kernel-occt' (no release-sensitive tests)" \
    bash -c "! printf '%s' \"\$GATED_RELEASE\" | grep -qF ' -p reify-kernel-occt'"
assert "gated release does NOT have '-p reify-cli' (no release-sensitive tests)" \
    bash -c "! printf '%s' \"\$GATED_RELEASE\" | grep -qF ' -p reify-cli'"
assert "gated release does NOT have '-p reify-config' (no release-sensitive tests)" \
    bash -c "! printf '%s' \"\$GATED_RELEASE\" | grep -qF ' -p reify-config'"

echo ""
echo "--- Test 6: gated invocations do not contain --workspace ---"
assert "gated debug invocation does not contain --workspace" \
    bash -c "! printf '%s' \"\$GATED_DEBUG\" | grep -qF ' --workspace'"
assert "gated release invocation does not contain --workspace" \
    bash -c "! printf '%s' \"\$GATED_RELEASE\" | grep -qF ' --workspace'"

# ---------------------------------------------------------------------------
# Tests 7–11: ungated-exclude assertions
# ---------------------------------------------------------------------------
# Extract ungated workspace passes: leaves running 'cargo (test|nextest run)
# --workspace' but NOT via the gate wrapper. The (test|nextest run) alternation
# keeps the assertions valid for both runner spellings.
UNGATED_DEBUG="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -E 'cargo (test|nextest run) --workspace' | grep -v 'cargo-test-occt-gated\.sh' | grep -v -- '--release' || true)"
# Ungated release: after sensitivity-scoping, uses -p flags (not --workspace).
# Match any non-gated cargo (test|nextest run) line that carries --release.
UNGATED_RELEASE="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -v 'cargo-test-occt-gated\.sh' \
    | grep -E 'cargo (test|nextest run)' \
    | grep -- '--release' || true)"
export UNGATED_DEBUG UNGATED_RELEASE

echo ""
echo "--- Test 7: ungated passes exist (one debug, one release) ---"
assert "ungated debug pass (cargo test --workspace, no gate, no --release) exists" \
    test -n "$UNGATED_DEBUG"
assert "ungated release pass (-p scoped, --release, no gate) exists" \
    test -n "$UNGATED_RELEASE"

echo ""
echo "--- Test 8: ungated debug has --exclude; ungated release is sensitivity-scoped (no --workspace/--exclude) ---"
# Debug side unchanged: each OCCT crate is still --exclude'd from the debug workspace pass.
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "ungated debug has '--exclude $crate'" \
        bash -c "printf '%s' \"\$UNGATED_DEBUG\" | grep -qF ' --exclude $crate'"
done <<< "$DECLARED_CRATES"
# Release side: sensitivity-scoped to -p flags, so --workspace and --exclude are absent.
# The 3 non-eval OCCT crates must be absent from the ungated release pass entirely.
assert "ungated release does NOT have '--workspace'" \
    bash -c "! printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' --workspace'"
assert "ungated release does NOT have '--exclude'" \
    bash -c "! printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' --exclude'"
assert "ungated release does NOT have '-p reify-kernel-occt'" \
    bash -c "! printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' -p reify-kernel-occt'"
assert "ungated release does NOT have '-p reify-cli'" \
    bash -c "! printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' -p reify-cli'"
assert "ungated release does NOT have '-p reify-config'" \
    bash -c "! printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' -p reify-config'"

echo ""
echo "--- Test 9: ungated passes are wrapped in 'timeout --kill-after=60 [0-9]+m' ---"
assert "ungated debug invocation contains 'timeout --kill-after=60 [0-9]+m'" \
    bash -c "printf '%s' \"\$UNGATED_DEBUG\" | grep -qE 'timeout[[:space:]]+--kill-after=60[[:space:]]+[0-9]+m[[:space:]]'"
assert "ungated release invocation contains 'timeout --kill-after=60 [0-9]+m'" \
    bash -c "printf '%s' \"\$UNGATED_RELEASE\" | grep -qE 'timeout[[:space:]]+--kill-after=60[[:space:]]+[0-9]+m[[:space:]]'"

echo ""
echo "--- Test 10: gated debug appears before ungated debug in the plan ---"
_ALL_SEGS="$TEST_PLAN_SEGS"
_GATED_DEBUG_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo-test-occt-gated\.sh' | grep -v -- '--release' | head -1 | cut -d: -f1 || true)"
_UNGATED_DEBUG_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -nE 'cargo (test|nextest run) --workspace' | grep -v 'cargo-test-occt-gated' | grep -v -- '--release' | head -1 | cut -d: -f1 || true)"
assert "gated debug (segment ${_GATED_DEBUG_IDX:-?}) precedes ungated debug (segment ${_UNGATED_DEBUG_IDX:-?})" \
    bash -c "[ '${_GATED_DEBUG_IDX:-0}' -gt 0 ] && [ '${_UNGATED_DEBUG_IDX:-0}' -gt 0 ] && [ '${_GATED_DEBUG_IDX:-0}' -lt '${_UNGATED_DEBUG_IDX:-0}' ]"

echo ""
echo "--- Test 11: gated release appears before ungated release in the plan ---"
_GATED_RELEASE_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo-test-occt-gated\.sh' | grep -- '--release' | head -1 | cut -d: -f1 || true)"
# After sensitivity-scoping, ungated release uses -p flags (not --workspace);
# match any non-gated cargo (test|nextest run) line carrying --release.
_UNGATED_RELEASE_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -nE 'cargo (test|nextest run)' \
    | grep -v 'cargo-test-occt-gated' \
    | grep -- '--release' | head -1 | cut -d: -f1 || true)"
assert "gated release (segment ${_GATED_RELEASE_IDX:-?}) precedes ungated release (segment ${_UNGATED_RELEASE_IDX:-?})" \
    bash -c "[ '${_GATED_RELEASE_IDX:-0}' -gt 0 ] && [ '${_UNGATED_RELEASE_IDX:-0}' -gt 0 ] && [ '${_GATED_RELEASE_IDX:-0}' -lt '${_UNGATED_RELEASE_IDX:-0}' ]"

test_summary
