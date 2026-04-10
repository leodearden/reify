#!/usr/bin/env bash
# Infrastructure test for task 1410.
# Validates that orchestrator.yaml's test_command includes a release-mode
# cargo test pass so that tests gated on #[cfg(not(debug_assertions))] are
# exercised in CI.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== release-mode test_command tests ==="

ORCH="$REPO_ROOT/orchestrator.yaml"

# -- Test 1: release pass exists -----------------------------------------------
echo ""
echo "--- Test 1: release pass present in test_command ---"

assert "test_command contains 'cargo test --workspace --release'" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'cargo test --workspace --release'"

# -- Test 2: debug pass preserved ----------------------------------------------
echo ""
echo "--- Test 2: debug pass preserved in test_command ---"

assert "test_command still contains 'cargo test --workspace -- --test-threads=1'" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'cargo test --workspace -- --test-threads=1'"

# -- Test 3: release pass uses --test-threads=1 --------------------------------
echo ""
echo "--- Test 3: release pass uses --test-threads=1 ---"

assert "test_command contains 'cargo test --workspace --release -- --test-threads=1'" \
    bash -c "grep 'test_command:' '$ORCH' | grep -q 'cargo test --workspace --release -- --test-threads=1'"

# -- Test 4: ordering (release AFTER debug) ------------------------------------
echo ""
echo "--- Test 4: release pass appears after debug pass ---"

assert "release pass byte position is greater than debug pass byte position" \
    bash -c "
        LINE=\$(grep 'test_command:' '$ORCH')
        DEBUG_POS=\$(awk 'BEGIN { s=ARGV[1]; p=ARGV[2]; print index(s, p) }' \"\$LINE\" 'cargo test --workspace -- --test-threads=1')
        RELEASE_POS=\$(awk 'BEGIN { s=ARGV[1]; p=ARGV[2]; print index(s, p) }' \"\$LINE\" 'cargo test --workspace --release -- --test-threads=1')
        [ \"\$DEBUG_POS\" -gt 0 ] && [ \"\$RELEASE_POS\" -gt 0 ] && [ \"\$RELEASE_POS\" -gt \"\$DEBUG_POS\" ]
    "

# -- Test 5: release pass NOT in lint_command ----------------------------------
echo ""
echo "--- Test 5: 'cargo test --release' absent from lint_command ---"

assert "lint_command does NOT contain 'cargo test --release'" \
    bash -c "! grep 'lint_command:' '$ORCH' | grep -q 'cargo test --release'"

# -- Test 6: sanity check — release-only test exists in workspace --------------
echo ""
echo "--- Test 6: at least one #[cfg(not(debug_assertions))] test exists ---"

assert "gui/src-tauri/src/tests/engine_tests.rs contains #[cfg(not(debug_assertions))]" \
    grep -q '#\[cfg(not(debug_assertions))\]' "$REPO_ROOT/gui/src-tauri/src/tests/engine_tests.rs"

test_summary
