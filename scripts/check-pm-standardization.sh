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

# ── Preflight: required tools ────────────────────────────────────────
assert "git is available" command -v git

# ── Check 1: packageManager field set to npm in all package.json files ───────
echo ""
echo "Check 1: packageManager field set to npm in package.json files"
for pkg in gui/package.json gui/sidecar/package.json tree-sitter-reify/package.json; do
    assert "$pkg has packageManager set to npm" grep -qE '"packageManager"\s*:\s*"npm@' "$ROOT/$pkg"
done

# ── Check 2: packageManager version consistent across all package.json files ─
echo ""
echo "Check 2: packageManager version consistent across package.json files"
assert "all package.json files agree on packageManager version" bash -c "
    count=\$(grep -ohE '\"packageManager\"\\s*:\\s*\"[^\"]+\"' \
        '$ROOT/gui/package.json' \
        '$ROOT/gui/sidecar/package.json' \
        '$ROOT/tree-sitter-reify/package.json' | sort -u | wc -l)
    [ \"\$count\" = '1' ]
"

# ── Check 3: npm lockfiles NOT in .gitignore ────────────────────────
echo ""
echo "Check 3: npm lockfiles not gitignored"
assert "no npm lockfiles are gitignored" \
    bash -c "! (cd '$ROOT' && git check-ignore gui/package-lock.json gui/sidecar/package-lock.json tree-sitter-reify/package-lock.json)"

# ── Check 4: pnpm-lock.yaml IS in .gitignore ────────────────────────
echo ""
echo "Check 4: pnpm-lock.yaml gitignored"
assert "**/pnpm-lock.yaml glob in .gitignore" grep -q '\*\*/pnpm-lock\.yaml' "$ROOT/.gitignore"

test_summary
