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

CRATE_LIST="$REPO_ROOT/scripts/occt-touching-crates.txt"
ORCH="$REPO_ROOT/orchestrator.yaml"

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

if [ -f "$CRATE_LIST" ]; then
    DECLARED_CRATES="$(grep -v '^\s*#' "$CRATE_LIST" | grep -v '^\s*$' | sed 's/^[[:space:]]*//;s/[[:space:]]*$//')"
else
    DECLARED_CRATES=""
fi

while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "declared crate '$crate' is a real workspace member" \
        grep -qxF "$crate" <<< "$WORKSPACE_MEMBERS"
done <<< "$DECLARED_CRATES"

# ---------------------------------------------------------------------------
# Test 3: declared set equals cargo-tree-derived OCCT-touching set
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: declared set equals cargo-tree-derived OCCT-touching set ---"

# Compute the actual OCCT-touching set via cargo tree.
# A crate is OCCT-touching iff `cargo tree --prefix none -p <crate>` mentions reify-kernel-occt.
ACTUAL_TOUCHING=""
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    if cargo tree --prefix none -p "$crate" 2>/dev/null | grep -q 'reify-kernel-occt'; then
        ACTUAL_TOUCHING="${ACTUAL_TOUCHING}${crate}"$'\n'
    fi
done <<< "$WORKSPACE_MEMBERS"
# Strip trailing newline for clean comparison.
ACTUAL_TOUCHING="${ACTUAL_TOUCHING%$'\n'}"

assert "declared OCCT-touching set equals cargo-tree-derived set (no missing or extra entries)" \
    bash -c "diff <(echo '$DECLARED_CRATES' | sort) <(echo '$ACTUAL_TOUCHING' | sort) >/dev/null"

# ---------------------------------------------------------------------------
# Tests 4–5: gated invocations use -p <crate> (not --workspace)
# ---------------------------------------------------------------------------
# Split the test_command line on ' && ' to extract per-invocation segments.
TEST_CMD_LINE="$(grep 'test_command:' "$ORCH")"
GATED_DEBUG="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g' \
    | grep 'cargo-test-occt-gated\.sh' | grep -v -- '--release' || true)"
GATED_RELEASE="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g' \
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
echo "--- Test 5: gated release invocation has '-p <crate>' for each declared crate ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "gated release invocation has '-p $crate'" \
        bash -c "printf '%s' \"\$GATED_RELEASE\" | grep -qF ' -p $crate'"
done <<< "$DECLARED_CRATES"

echo ""
echo "--- Test 6: gated invocations do not contain --workspace ---"
assert "gated debug invocation does not contain --workspace" \
    bash -c "! printf '%s' \"\$GATED_DEBUG\" | grep -qF ' --workspace'"
assert "gated release invocation does not contain --workspace" \
    bash -c "! printf '%s' \"\$GATED_RELEASE\" | grep -qF ' --workspace'"

# ---------------------------------------------------------------------------
# Tests 7–11: ungated-exclude assertions
# ---------------------------------------------------------------------------
# Extract ungated workspace passes: segments with 'cargo test --workspace' but
# NOT containing 'cargo-test-occt-gated.sh'.
UNGATED_DEBUG="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g' \
    | grep 'cargo test --workspace' | grep -v 'cargo-test-occt-gated\.sh' | grep -v -- '--release' || true)"
UNGATED_RELEASE="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g' \
    | grep 'cargo test --workspace' | grep -v 'cargo-test-occt-gated\.sh' | grep -- '--release' || true)"
export UNGATED_DEBUG UNGATED_RELEASE

echo ""
echo "--- Test 7: ungated workspace passes exist (one debug, one release) ---"
assert "ungated debug pass (cargo test --workspace, no gate, no --release) exists" \
    test -n "$UNGATED_DEBUG"
assert "ungated release pass (cargo test --workspace --release, no gate) exists" \
    test -n "$UNGATED_RELEASE"

echo ""
echo "--- Test 8: ungated passes have --exclude <crate> for each declared crate ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "ungated debug has '--exclude $crate'" \
        bash -c "printf '%s' \"\$UNGATED_DEBUG\" | grep -qF ' --exclude $crate'"
    assert "ungated release has '--exclude $crate'" \
        bash -c "printf '%s' \"\$UNGATED_RELEASE\" | grep -qF ' --exclude $crate'"
done <<< "$DECLARED_CRATES"

echo ""
echo "--- Test 9: ungated passes are wrapped in 'timeout --kill-after=60 [0-9]+m' ---"
assert "ungated debug invocation contains 'timeout --kill-after=60 [0-9]+m'" \
    bash -c "printf '%s' \"\$UNGATED_DEBUG\" | grep -qE 'timeout[[:space:]]+--kill-after=60[[:space:]]+[0-9]+m[[:space:]]'"
assert "ungated release invocation contains 'timeout --kill-after=60 [0-9]+m'" \
    bash -c "printf '%s' \"\$UNGATED_RELEASE\" | grep -qE 'timeout[[:space:]]+--kill-after=60[[:space:]]+[0-9]+m[[:space:]]'"

echo ""
echo "--- Test 10: gated debug appears before ungated debug in test_command ---"
_ALL_SEGS="$(printf '%s' "$TEST_CMD_LINE" | sed 's/ && /\n/g')"
_GATED_DEBUG_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo-test-occt-gated\.sh' | grep -v -- '--release' | head -1 | cut -d: -f1 || true)"
_UNGATED_DEBUG_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo test --workspace' | grep -v 'cargo-test-occt-gated' | grep -v -- '--release' | head -1 | cut -d: -f1 || true)"
assert "gated debug (segment ${_GATED_DEBUG_IDX:-?}) precedes ungated debug (segment ${_UNGATED_DEBUG_IDX:-?})" \
    bash -c "[ '${_GATED_DEBUG_IDX:-0}' -gt 0 ] && [ '${_UNGATED_DEBUG_IDX:-0}' -gt 0 ] && [ '${_GATED_DEBUG_IDX:-0}' -lt '${_UNGATED_DEBUG_IDX:-0}' ]"

echo ""
echo "--- Test 11: gated release appears before ungated release in test_command ---"
_GATED_RELEASE_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo-test-occt-gated\.sh' | grep -- '--release' | head -1 | cut -d: -f1 || true)"
_UNGATED_RELEASE_IDX="$(printf '%s\n' "$_ALL_SEGS" \
    | grep -n 'cargo test --workspace' | grep -v 'cargo-test-occt-gated' | grep -- '--release' | head -1 | cut -d: -f1 || true)"
assert "gated release (segment ${_GATED_RELEASE_IDX:-?}) precedes ungated release (segment ${_UNGATED_RELEASE_IDX:-?})" \
    bash -c "[ '${_GATED_RELEASE_IDX:-0}' -gt 0 ] && [ '${_UNGATED_RELEASE_IDX:-0}' -gt 0 ] && [ '${_GATED_RELEASE_IDX:-0}' -lt '${_UNGATED_RELEASE_IDX:-0}' ]"

test_summary
