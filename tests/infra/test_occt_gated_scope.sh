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
# Nextest occt-group assertions (task 4451):
# (a) [test-groups] occt max-threads = 4 (bounded; was inert 1 when staged).
# (b) [[profile.default.overrides]] filter for test-group 'occt' contains
#     package(<crate>) for every declared OCCT crate (drift catch: a missing
#     crate would escape the max-threads cap and run unbounded in the pool).
# RED: max-threads = 1 today; GREEN after step-2 impl raises it to 4.
# ---------------------------------------------------------------------------
NEXTEST_TOML="$REPO_ROOT/.config/nextest.toml"

echo ""
echo "--- Nextest occt-group (task 4451): max-threads = 4 (bounded, not inert 1) ---"
assert "nextest.toml: [test-groups] occt has max-threads = 4 (bounded, not inert 1)" \
    grep -qF 'occt = { max-threads = 4 }' "$NEXTEST_TOML"

echo ""
echo "--- Nextest occt-group (task 4451): filter drift check (every declared crate is package()-filtered) ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "nextest.toml occt-group filter contains package($crate)" \
        grep -qF "package($crate)" "$NEXTEST_TOML"
done <<< "$DECLARED_CRATES"

# ---------------------------------------------------------------------------
# Tests 4–8: folded-contract plan-shape assertions (task 4451)
# Source of truth: scripts/verify.sh --print-plan (the oracle the orchestrator
# calls). --profile both --scope all forces the full plan; env lines stripped.
#
# Folded contract: (1) no cargo-test-occt-gated.sh invocation; (2) full-workspace
# debug pass is `cargo nextest run --workspace` with NO --exclude; (3) release pass
# includes -p reify-eval (OCCT∩release-sensitive, folded); (4) workspace pass is
# wrapped in the standard outer timeout.
# RED against current verify.sh (which still emits the gated pass).
# ---------------------------------------------------------------------------
TEST_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" test --profile both --scope all --print-plan | grep -v '^#')"
export TEST_PLAN_SEGS

echo ""
echo "--- Test 4 (task 4451): plan has NO cargo-test-occt-gated.sh invocation (fold) ---"
assert "plan contains NO cargo-test-occt-gated.sh (gated pass dropped, OCCT in nextest pool)" \
    bash -c "! printf '%s\n' \"\$TEST_PLAN_SEGS\" | grep -q 'cargo-test-occt-gated\.sh'"

echo ""
echo "--- Test 5 (task 4451): full-workspace nextest pass has --workspace with NO --exclude ---"
FULL_WS_DEBUG="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -E 'cargo nextest run --workspace' | grep -v -- '--release' || true)"
export FULL_WS_DEBUG

assert "full-workspace debug nextest pass exists (cargo nextest run --workspace)" \
    test -n "$FULL_WS_DEBUG"
assert "full-workspace debug nextest pass has NO --exclude (OCCT folded into nextest pool)" \
    bash -c "! printf '%s' \"\$FULL_WS_DEBUG\" | grep -q -- '--exclude'"

echo ""
echo "--- Test 6 (task 4451): no OCCT crate is --exclude'd from the workspace nextest pass ---"
while IFS= read -r crate; do
    [ -z "$crate" ] && continue
    assert "workspace nextest pass does NOT have '--exclude $crate' (OCCT folded in)" \
        bash -c "! printf '%s' \"\$FULL_WS_DEBUG\" | grep -qF ' --exclude $crate'"
done <<< "$DECLARED_CRATES"

echo ""
echo "--- Test 7 (task 4451): release nextest pass includes -p reify-eval (folded) ---"
NEXTEST_RELEASE="$(printf '%s\n' "$TEST_PLAN_SEGS" \
    | grep -v 'cargo-test-occt-gated\.sh' \
    | grep -E 'cargo nextest run' \
    | grep -- '--release' || true)"
export NEXTEST_RELEASE

assert "release nextest pass exists (cargo nextest run ... --release, no gated wrapper)" \
    test -n "$NEXTEST_RELEASE"
assert "release nextest pass has '-p reify-eval' (OCCT∩release-sensitive, folded)" \
    bash -c "printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' -p reify-eval'"
assert "release nextest pass has '--release'" \
    bash -c "printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' --release'"
assert "release nextest pass does NOT have '--workspace' (sensitivity-scoped)" \
    bash -c "! printf '%s' \"\$NEXTEST_RELEASE\" | grep -qF ' --workspace'"

echo ""
echo "--- Test 8 (task 4451): workspace nextest pass is wrapped in outer timeout ---"
assert "workspace nextest pass is wrapped in 'timeout --kill-after=60 [0-9]+m'" \
    bash -c "printf '%s' \"\$FULL_WS_DEBUG\" | grep -qE 'timeout[[:space:]]+--kill-after=60[[:space:]]+[0-9]+m[[:space:]]'"

test_summary
