#!/usr/bin/env bash
# Meta-test for tests/infra/test_pycache_gitignored.sh — verifies the pathspec
# catches root-level __pycache__ artifacts including non-.pyc extensions.
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

PYCACHE_TEST="$SCRIPT_DIR/test_pycache_gitignored.sh"
HELPERS="$SCRIPT_DIR/test_helpers.sh"

[ -f "$PYCACHE_TEST" ] || {
    echo "ERROR: test_pycache_gitignored.sh not found at $PYCACHE_TEST"
    exit 1
}

echo "=== test_pycache_gitignored.sh meta-tests ==="

_tmpdir=$(mktemp -d)
trap 'rm -rf "$_tmpdir"' EXIT

# ==============================================================================
# Test (a): failing-fixture — regression test detects root-level __pycache__ artifact
# The canary is __pycache__/foo.pyo (non-.pyc extension) at repo root.
# *.pyc does NOT match it; only the __pycache__ pathspec can catch it.
# ==============================================================================
echo ""
echo "--- Test (a): regression test exits non-zero when root-level __pycache__/foo.pyo is tracked ---"

_violation="$_tmpdir/violation"
mkdir -p "$_violation/tests/infra" "$_violation/__pycache__"

git init -q "$_violation"
git -C "$_violation" config user.email "test@test.local"
git -C "$_violation" config user.name "Test"

# Copy the regression test and helpers so the script resolves REPO_ROOT from its own location
cp "$PYCACHE_TEST" "$_violation/tests/infra/test_pycache_gitignored.sh"
cp "$HELPERS"      "$_violation/tests/infra/test_helpers.sh"

# Valid .gitignore so Check 2 passes cleanly; isolation is on Check 1
printf '__pycache__/\n*.pyc\n' > "$_violation/.gitignore"

# The canary: root-level __pycache__/foo.pyo (the precise gap the new pathspec must close)
touch "$_violation/__pycache__/foo.pyo"

git -C "$_violation" add -f .gitignore tests/infra "__pycache__/foo.pyo"
git -C "$_violation" commit -q -m "fixture"

_rc_a=0
bash "$_violation/tests/infra/test_pycache_gitignored.sh" >/dev/null 2>&1 || _rc_a=$?

assert "regression test exits non-zero when root-level __pycache__/foo.pyo is tracked" \
    bash -c "[ \"$_rc_a\" -ne 0 ]"

# ==============================================================================
# Test (b): happy-path — regression test exits zero on a clean tmp repo
# No tracked __pycache__ artifacts; only .gitignore and tests/infra/* files.
# ==============================================================================
echo ""
echo "--- Test (b): regression test exits zero on a clean tmp repo ---"

_clean="$_tmpdir/clean"
mkdir -p "$_clean/tests/infra"

git init -q "$_clean"
git -C "$_clean" config user.email "test@test.local"
git -C "$_clean" config user.name "Test"

cp "$PYCACHE_TEST" "$_clean/tests/infra/test_pycache_gitignored.sh"
cp "$HELPERS"      "$_clean/tests/infra/test_helpers.sh"

printf '__pycache__/\n*.pyc\n' > "$_clean/.gitignore"

git -C "$_clean" add -f .gitignore tests/infra
git -C "$_clean" commit -q -m "fixture"

_rc_b=0
bash "$_clean/tests/infra/test_pycache_gitignored.sh" >/dev/null 2>&1 || _rc_b=$?

assert "regression test exits zero on a clean tmp repo" \
    bash -c "[ \"$_rc_b\" -eq 0 ]"

# -- Summary ------------------------------------------------------------------
test_summary
