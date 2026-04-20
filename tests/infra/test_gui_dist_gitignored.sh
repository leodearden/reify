#!/usr/bin/env bash
# Regression test: gui/dist/ build artifacts must not be tracked by git.
# Asserts two invariants:
#   1. `git ls-files gui/dist/` produces no output (no tracked artifacts).
#   2. `.gitignore` contains a `gui/dist/` rule (the ignore rule is present).
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== gui/dist/ gitignore tests ==="

# ==============================================================================
# Check 1: no gui/dist/ files are tracked by git
# ==============================================================================
echo ""
echo "--- Check 1: gui/dist/ files are not tracked by git ---"

assert "git ls-files gui/dist/ returns empty (no tracked build artifacts)" \
    bash -c "[ -z \"\$(git -C \"$REPO_ROOT\" ls-files gui/dist/)\" ]"

# ==============================================================================
# Check 2: .gitignore contains the gui/dist/ ignore rule
# ==============================================================================
echo ""
echo "--- Check 2: .gitignore contains gui/dist/ rule ---"

assert ".gitignore contains a gui/dist/ line" \
    grep -qF "gui/dist/" "$REPO_ROOT/.gitignore"

# -- Summary ------------------------------------------------------------------
test_summary
