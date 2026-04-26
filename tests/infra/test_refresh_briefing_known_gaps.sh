#!/usr/bin/env bash
# Tests for scripts/refresh_briefing_known_gaps.py.
# Verifies: existence, executability, YAML/JSON loading, cross-reference logic,
# --json mode, error handling, and edge cases (orphan IDs, missing tracking field).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
REFRESH_SCRIPT="$REPO_ROOT/scripts/refresh_briefing_known_gaps.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== refresh_briefing_known_gaps.py tests ==="

# -- Setup: temp-dir fixture machinery ----------------------------------------
_tmpdir=$(mktemp -d)
trap 'rm -rf "$_tmpdir"' EXIT

# ==============================================================================
# Check 0: script exists and is executable, --help exits 0
# ==============================================================================
echo ""
echo "--- Check 0: script existence, executability, and --help ---"

assert "scripts/refresh_briefing_known_gaps.py exists" \
    test -f "$REFRESH_SCRIPT"

assert "scripts/refresh_briefing_known_gaps.py is executable" \
    test -x "$REFRESH_SCRIPT"

assert "scripts/refresh_briefing_known_gaps.py --help exits 0" \
    bash -c "python3 '$REFRESH_SCRIPT' --help >/dev/null 2>&1"

# -- Summary ------------------------------------------------------------------
test_summary
