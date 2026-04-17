#!/usr/bin/env bash
# Tests for the git hooks auto-configuration in scripts/setup-dev.sh
# and the corresponding documentation in CLAUDE.md.
#
# Check 0: setup-dev.sh is executable and contains the exact
#           `git config core.hooksPath hooks` line.
# Check 1: sanity check — running `git config core.hooksPath hooks` in a
#           fresh temp repo results in `git config --get core.hooksPath`
#           returning the string `hooks`.
# Check 2: CLAUDE.md under "## Local Dev Setup" documents both
#           `core.hooksPath` and a reference to `hooks/`.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"
CLAUDE_MD="$REPO_ROOT/CLAUDE.md"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== setup-dev.sh git hooks configuration tests ==="

# -- Setup: temp-dir fixture machinery ----------------------------------------
_tmpdir=$(mktemp -d)
trap 'rm -rf "$_tmpdir"' EXIT

# ==============================================================================
# Check 0: setup-dev.sh is executable and contains the hooksPath config line
# ==============================================================================
echo ""
echo "--- Check 0: setup-dev.sh contains git config core.hooksPath hooks ---"

assert "scripts/setup-dev.sh exists" \
    test -f "$SETUP_DEV"

assert "scripts/setup-dev.sh is executable" \
    test -x "$SETUP_DEV"

assert "scripts/setup-dev.sh has an uncommented 'git config core.hooksPath hooks' line" \
    grep -qE '^[[:space:]]*git config core\.hooksPath hooks' "$SETUP_DEV"

# ==============================================================================
# Check 1: sanity — git config core.hooksPath hooks is idempotent in a fresh repo
# ==============================================================================
echo ""
echo "--- Check 1: git config core.hooksPath hooks works in a fresh repo ---"

_tmpgit="$_tmpdir/sanity_repo"
mkdir -p "$_tmpgit"
git init -q "$_tmpgit"

# Run the exact command that setup-dev.sh will use.
(cd "$_tmpgit" && git config core.hooksPath hooks)

assert "git config core.hooksPath hooks sets hooksPath to 'hooks'" \
    bash -c "[ \"\$(git -C '$_tmpgit' config --get core.hooksPath)\" = 'hooks' ]"

# Run again (idempotence).
(cd "$_tmpgit" && git config core.hooksPath hooks)

assert "git config core.hooksPath hooks is idempotent (second run)" \
    bash -c "[ \"\$(git -C '$_tmpgit' config --get core.hooksPath)\" = 'hooks' ]"

# ==============================================================================
# Check 2: CLAUDE.md Local Dev Setup section documents core.hooksPath + hooks/
# ==============================================================================
echo ""
echo "--- Check 2: CLAUDE.md documents core.hooksPath under Local Dev Setup ---"

assert "CLAUDE.md exists" \
    test -f "$CLAUDE_MD"

# Extract the "Local Dev Setup" section: from "## Local Dev Setup" to the next
# "## " heading (exclusive).  The section must contain both tokens.
assert "CLAUDE.md Local Dev Setup section contains 'core.hooksPath'" \
    bash -c "awk '/^## Local Dev Setup/{f=1; next} f && /^## /{exit} f{print}' '$CLAUDE_MD' \
             | grep -qF 'core.hooksPath'"

assert "CLAUDE.md Local Dev Setup section contains a reference to 'hooks/'" \
    bash -c "awk '/^## Local Dev Setup/{f=1; next} f && /^## /{exit} f{print}' '$CLAUDE_MD' \
             | grep -qF 'hooks/'"

# -- Summary ------------------------------------------------------------------
test_summary
