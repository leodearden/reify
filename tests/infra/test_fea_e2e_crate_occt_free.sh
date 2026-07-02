#!/usr/bin/env bash
# Infrastructure test for task 4937 (Lever P of
# docs/prds/reify-eval-fea-decomposition.md).
#
# BT-4/INV-1: the new integration-test-only crate `reify-eval-fea-tests`
# hosts the engine-driven synthetic-mesh FEA e2e tests without linking OCCT,
# so it runs in the fast nextest pool instead of the OCCT serialization gate.
#
# Assertions:
#   1. `reify-eval-fea-tests` is a real workspace member.
#   2. `cargo tree -p reify-eval-fea-tests -e normal,build` succeeds and its
#      output contains NO `reify-kernel-occt` line (the `-e normal,build`
#      edge filter excludes dev-deps, so reify-eval's dev-only OCCT edge
#      never appears even though reify-eval itself is a direct dev-dep).
#   3. `reify-eval-fea-tests` is ABSENT from scripts/occt-touching-crates.txt
#      (encodes "runs in the fast pool, not the OCCT gate").

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Shared OCCT-scope library: occt_declared_set is the SINGLE implementation of
# the declared list, shared with scripts/verify.sh and
# tests/infra/test_occt_gated_scope.sh so the three cannot drift apart.
[ -f "$REPO_ROOT/scripts/occt-scope-lib.sh" ] || { echo "ERROR: occt-scope-lib.sh not found at $REPO_ROOT/scripts/occt-scope-lib.sh"; exit 1; }
source "$REPO_ROOT/scripts/occt-scope-lib.sh"

CRATE="reify-eval-fea-tests"

echo "=== FEA e2e crate OCCT-free tests (task 4937, BT-4/INV-1) ==="

# ---------------------------------------------------------------------------
# Test 1: reify-eval-fea-tests is a real workspace member
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 1: $CRATE is a real workspace member ---"

WORKSPACE_MEMBERS="$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c "import sys,json; m=json.load(sys.stdin); [print(p['name']) for p in m['packages']]")"
export WORKSPACE_MEMBERS

assert "$CRATE is a real workspace member" \
    bash -c "grep -qxF '$CRATE' <<< \"\$WORKSPACE_MEMBERS\""

# ---------------------------------------------------------------------------
# Test 2 (BT-4/INV-1): normal/build dep closure succeeds and is OCCT-free
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 2 (BT-4/INV-1): cargo tree -p $CRATE -e normal,build succeeds and is OCCT-free ---"

TREE_EXIT=0
TREE_OUT="$(cd "$REPO_ROOT" && cargo tree -p "$CRATE" -e normal,build 2>&1)" || TREE_EXIT=$?
export TREE_OUT

if [ "$TREE_EXIT" -ne 0 ]; then
    echo "  cargo tree output:"
    echo "$TREE_OUT" | sed 's/^/    /'
fi

assert "cargo tree -p $CRATE -e normal,build succeeds (exit 0)" \
    bash -c "exit $TREE_EXIT"

assert "cargo tree -p $CRATE -e normal,build output contains NO reify-kernel-occt line" \
    bash -c "! printf '%s\n' \"\$TREE_OUT\" | grep -q 'reify-kernel-occt'"

# ---------------------------------------------------------------------------
# Test 3: reify-eval-fea-tests is ABSENT from scripts/occt-touching-crates.txt
# ---------------------------------------------------------------------------
echo ""
echo "--- Test 3: $CRATE is ABSENT from scripts/occt-touching-crates.txt ---"

DECLARED_CRATES="$(occt_declared_set)"
export DECLARED_CRATES

assert "$CRATE is NOT in scripts/occt-touching-crates.txt (fast pool, not the OCCT gate)" \
    bash -c "! grep -qxF '$CRATE' <<< \"\$DECLARED_CRATES\""

test_summary
