#!/usr/bin/env bash
# Validation script for package manager standardization (task 618).
# Checks static repo state: packageManager fields, lockfile gitignore status.
# Redundant config-file-content checks (4-9) were removed by task 816 — those
# are validated by actual execution on each commit and CI cycle.
# Exit code: number of failed checks (0 = all pass).

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
failures=0

# ── Check 1: packageManager field in all package.json files ──────────
echo "Check 1: packageManager field in package.json files"
for pkg in gui/package.json gui/sidecar/package.json tree-sitter-reify/package.json; do
    if grep -q '"packageManager"' "$ROOT/$pkg" 2>/dev/null; then
        echo "  PASS: $pkg has packageManager field"
    else
        echo "  FAIL: $pkg missing packageManager field"
        failures=$((failures + 1))
    fi
done

# ── Check 2: npm lockfiles NOT in .gitignore ────────────────────────
echo "Check 2: npm lockfiles not gitignored"
for lockfile in gui/package-lock.json gui/sidecar/package-lock.json tree-sitter-reify/package-lock.json; do
    if (cd "$ROOT" && git check-ignore -q "$lockfile" 2>/dev/null); then
        echo "  FAIL: $lockfile is gitignored (should be tracked)"
        failures=$((failures + 1))
    else
        echo "  PASS: $lockfile is not gitignored"
    fi
done

# ── Check 3: pnpm-lock.yaml IS in .gitignore ────────────────────────
echo "Check 3: pnpm-lock.yaml gitignored"
if grep -q 'pnpm-lock\.yaml' "$ROOT/.gitignore" 2>/dev/null; then
    # Verify it uses a glob pattern (not per-directory)
    if grep -q '\*\*/pnpm-lock\.yaml' "$ROOT/.gitignore" 2>/dev/null; then
        echo "  PASS: **/pnpm-lock.yaml glob in .gitignore"
    else
        echo "  FAIL: pnpm-lock.yaml in .gitignore but not using ** glob pattern"
        failures=$((failures + 1))
    fi
else
    echo "  FAIL: pnpm-lock.yaml not in .gitignore"
    failures=$((failures + 1))
fi

# ── Summary ──────────────────────────────────────────────────────────
echo ""
if [ "$failures" -eq 0 ]; then
    echo "All checks passed."
else
    echo "$failures check(s) failed."
fi
exit "$failures"
