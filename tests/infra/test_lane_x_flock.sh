#!/usr/bin/env bash
# tests/infra/test_lane_x_flock.sh — mechanism tests for scripts/lib_lane_x_flock.sh
#
# Tests the Lane-X host-exclusive coarse single-slot flock primitive (H8,
# docs/prds/run-all-host-infra-partition.md). Mirrors the structure of
# tests/infra/test_test_run_semaphore.sh, which mirrors
# tests/infra/test_occt_flock_gate.sh.
#
# H8 ships this lib INERT: it is NOT wired into any tests/infra/run_all.sh
# path in this decompose (see the inert-shipping guard below). H9 (sibling
# task) is the intra-batch consumer that acquires it directly.
#
# Auto-discovered by tests/infra/run_all.sh (pattern test_*.sh).
# Each invocation uses an isolated mktemp LOCK base and cleans
# ${LOCK}.slot-1 after each test (hermetic).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
LIB="$REPO_ROOT/scripts/lib_lane_x_flock.sh"

source "$SCRIPT_DIR/test_helpers.sh"

# ===========================================================================
# FOUNDATION tests (Tests 1-4): lib structure and sourceable interface
# ===========================================================================

echo "=== test_lane_x_flock.sh: lib_lane_x_flock.sh mechanism tests ==="
echo ""
echo "--- Test 1: lib file exists ---"
assert "lib file exists at scripts/lib_lane_x_flock.sh" \
    test -f "$LIB"

echo ""
echo "--- Test 2: lib is executable ---"
assert "lib is executable" \
    test -x "$LIB"

echo ""
echo "--- Test 3: lib has correct shebang ---"
assert "first line is #!/usr/bin/env bash" \
    bash -c 'head -1 "$1" | grep -q "^#!/usr/bin/env bash"' -- "$LIB"

echo ""
echo "--- Test 4: lib is sourceable without side effects and defines acquire+release ---"
assert "sourceable: defines lane_x_flock_acquire, lane_x_flock_release" \
    bash -c 'source "$1" >/dev/null 2>&1 && declare -F lane_x_flock_acquire && declare -F lane_x_flock_release' -- "$LIB"

# ===========================================================================
# INERT-SHIPPING GUARD (Test 5): the Part A/B split (PRD §6/§7/§11) depends on
# H8 being inert in Part A — not invoked from any run_all.sh path. A future
# accidental wire would silently pull a host-exclusive lock into the
# concurrent test pool; guard it structurally rather than in prose.
# ===========================================================================

echo ""
echo "--- Test 5: run_all.sh does not reference lib_lane_x_flock (inert-shipping guard) ---"
assert "run_all.sh does NOT reference lib_lane_x_flock (H8 ships inert in Part A)" \
    bash -c '! grep -q lib_lane_x_flock "$1"' -- "$REPO_ROOT/tests/infra/run_all.sh"

# ===========================================================================
# Summary
# ===========================================================================

test_summary
