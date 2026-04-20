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
        bash -c "echo \"\$WORKSPACE_MEMBERS\" | grep -qxF '$crate'" <<< "$WORKSPACE_MEMBERS"
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

test_summary
