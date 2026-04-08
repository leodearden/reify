#!/usr/bin/env bash
# Validation script for package manager standardization (task 618).
# Checks static repo state: packageManager fields, lockfile gitignore status.
# Redundant config-file-content checks (4-9) were removed by task 816 — those
# are validated by actual execution on each commit and CI cycle.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# This is a test script, not a build script — source shared test helpers from tests/infra/.
[ -f "$SCRIPT_DIR/../tests/infra/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/../tests/infra/test_helpers.sh"

echo "=== check-pm-standardization ==="

# ── Check 1: packageManager field in all package.json files ──────────
echo ""
echo "Check 1: packageManager field in package.json files"
for pkg in gui/package.json gui/sidecar/package.json tree-sitter-reify/package.json; do
    assert "$pkg has packageManager field" grep -q '"packageManager"' "$ROOT/$pkg"
done

# ── Check 2: npm lockfiles NOT in .gitignore ────────────────────────
echo ""
echo "Check 2: npm lockfiles not gitignored"
for lockfile in gui/package-lock.json gui/sidecar/package-lock.json tree-sitter-reify/package-lock.json; do
    assert "$lockfile is not gitignored" bash -c "! (cd '$ROOT' && git check-ignore -q '$lockfile' 2>/dev/null)"
done

# ── Check 3: pnpm-lock.yaml IS in .gitignore ────────────────────────
echo ""
echo "Check 3: pnpm-lock.yaml gitignored"
assert "gui/pnpm-lock.yaml in .gitignore" grep -q 'gui/pnpm-lock\.yaml' "$ROOT/.gitignore"

test_summary
