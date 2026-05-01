#!/usr/bin/env bash
# Regression test: Python bytecode artifacts must not be tracked by git.
# Asserts two invariants:
#   1. `git ls-files *.pyc **/__pycache__/**` produces no output (no tracked artifacts).
#   2. `.gitignore` contains both `__pycache__/` and `*.pyc` rules.
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

echo "=== Python bytecode gitignore tests ==="

# ==============================================================================
# Check 1: no *.pyc or __pycache__ files are tracked by git
# ==============================================================================
echo ""
echo "--- Check 1: no tracked Python bytecode artifacts ---"

assert "git ls-files returns no *.pyc or __pycache__ entries (no tracked Python bytecode artifacts)" \
    bash -c "[ -z \"\$(git -C \"$REPO_ROOT\" ls-files '*.pyc' '**/__pycache__/**')\" ]"

# ==============================================================================
# Check 2: .gitignore contains both ignore rules
# ==============================================================================
echo ""
echo "--- Check 2: .gitignore contains __pycache__/ and *.pyc rules ---"

assert ".gitignore contains a __pycache__/ line" \
    grep -qFx '__pycache__/' "$REPO_ROOT/.gitignore"

assert ".gitignore contains a *.pyc line" \
    grep -qFx '*.pyc' "$REPO_ROOT/.gitignore"

# -- Summary ------------------------------------------------------------------
test_summary
