#!/usr/bin/env bash
# Regression test: the orchestrator's runtime reconciliation.db (and its SQLite
# -shm/-wal sidecars) must not be tracked by git, at BOTH runtime paths:
#   - data/reconciliation/reconciliation.db
#   - data/.orchestrator/reconciliation/reconciliation.db
#
# A tracked, live SQLite DB dirties the main checkout and reproduces the
# 2026-07-12 advance_main `stash_failed` fleet incident (git stash push cannot
# park an open DB file). Mirrors test_queue_db_gitignored.sh (the write_queue.db
# guard), extended to both reconciliation paths. Asserts three invariants:
#   1. `git ls-files <dir>/` produces no output at each path (no tracked
#      runtime reconciliation DB artifacts, incl. any -shm/-wal sidecar).
#   2. `git check-ignore` matches each path (the DB is semantically ignored —
#      the observable signal the fix targets; only true once the file is BOTH
#      untracked and covered by a rule, since check-ignore consults the index).
#   3. `.gitignore` contains the exact `data/reconciliation/*.db*` and
#      `data/.orchestrator/reconciliation/*.db*` rules.
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

echo "=== reconciliation.db gitignore tests (both runtime paths) ==="

# ==============================================================================
# Check 1: no reconciliation DB files are tracked by git (either path)
# ==============================================================================
echo ""
echo "--- Check 1: reconciliation DB dirs have no tracked files ---"

assert "git ls-files data/reconciliation/ returns empty (no tracked runtime reconciliation DB)" \
    bash -c "[ -z \"\$(git -C \"$REPO_ROOT\" ls-files data/reconciliation/)\" ]"

assert "git ls-files data/.orchestrator/reconciliation/ returns empty (no tracked runtime reconciliation DB)" \
    bash -c "[ -z \"\$(git -C \"$REPO_ROOT\" ls-files data/.orchestrator/reconciliation/)\" ]"

# ==============================================================================
# Check 2: git check-ignore matches each reconciliation DB path (semantic)
# ==============================================================================
echo ""
echo "--- Check 2: git check-ignore matches both reconciliation DB paths ---"

assert "git check-ignore matches data/reconciliation/reconciliation.db" \
    git -C "$REPO_ROOT" check-ignore -q data/reconciliation/reconciliation.db

assert "git check-ignore matches data/.orchestrator/reconciliation/reconciliation.db" \
    git -C "$REPO_ROOT" check-ignore -q data/.orchestrator/reconciliation/reconciliation.db

# ==============================================================================
# Check 3: .gitignore contains the exact rule for each reconciliation DB path
# ==============================================================================
echo ""
echo "--- Check 3: .gitignore contains both data/**/reconciliation/*.db* rules ---"

assert ".gitignore contains a data/reconciliation/*.db* line" \
    grep -qFx 'data/reconciliation/*.db*' "$REPO_ROOT/.gitignore"

assert ".gitignore contains a data/.orchestrator/reconciliation/*.db* line" \
    grep -qFx 'data/.orchestrator/reconciliation/*.db*' "$REPO_ROOT/.gitignore"

# -- Summary ------------------------------------------------------------------
test_summary
