#!/usr/bin/env bash
# Infrastructure test for task 4390.
# Validates that the release-sensitive crate list is correct and guards
# against drift between the declared set and the grep-derived set.
#
# Assertions (drift-guard core, mirroring test_occt_gated_scope.sh Tests 1-3):
#   1. scripts/release-sensitive-crates.txt exists and is non-empty (after stripping comments/blanks).
#   2. Every declared entry is a real workspace member.
#   3. Declared set EQUALS the grep-derived release-sensitive set (drift catcher).
#      This is the load-bearing guard: it fails if a new release-sensitive test
#      lands in a crate not on the declared list.

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

test_summary
