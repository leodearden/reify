#!/usr/bin/env bash
# Regression test: the orchestrator's runtime write_queue.db (and its SQLite
# -shm/-wal sidecars) must not be tracked by git.
# Asserts two invariants:
#   1. `git ls-files data/queue/` produces no output (no tracked runtime
#      queue DB artifacts).
#   2. `.gitignore` contains a `data/queue/*.db*` rule (the ignore rule is
#      present).
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

echo "=== data/queue/ write_queue.db gitignore tests ==="

# ==============================================================================
# Check 1: no data/queue/ files are tracked by git
# ==============================================================================
echo ""
echo "--- Check 1: data/queue/ files are not tracked by git ---"

assert "git ls-files data/queue/ returns empty (no tracked runtime queue DB)" \
    bash -c "[ -z \"\$(git -C \"$REPO_ROOT\" ls-files data/queue/)\" ]"

# ==============================================================================
# Check 2: .gitignore contains the data/queue/*.db* ignore rule
# ==============================================================================
echo ""
echo "--- Check 2: .gitignore contains data/queue/*.db* rule ---"

assert ".gitignore contains a data/queue/*.db* line" \
    grep -qFx 'data/queue/*.db*' "$REPO_ROOT/.gitignore"

# -- Summary ------------------------------------------------------------------
test_summary
