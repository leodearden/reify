#!/usr/bin/env bash
# Validation script for package manager standardization (task 618).
# Checks static repo state: packageManager fields, lockfile gitignore status.
# Redundant config-file-content checks (5-9) were removed by task 816; Check 4
# (pnpm-lock.yaml gitignored) was retained as a static-state check.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# This is a test script, not a build script — source shared test helpers from tests/infra/.
[ -f "$SCRIPT_DIR/../tests/infra/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found"; exit 1; }
source "$SCRIPT_DIR/../tests/infra/test_helpers.sh"

PKG_FILES='gui/package.json gui/sidecar/package.json tree-sitter-reify/package.json'
# Derive the file count from PKG_FILES so Check 2's total assertion tracks
# the list automatically (task 1366). Intentional word-splitting on $PKG_FILES
# is consistent with how the rest of this script uses the variable.
# shellcheck disable=SC2086
set -- $PKG_FILES
PKG_COUNT=$#
LOCK_FILES='gui/package-lock.json gui/sidecar/package-lock.json tree-sitter-reify/package-lock.json'

echo "=== test_pm_standardization ==="

# ── Preflight: required tools ────────────────────────────────────────
assert "git is available" command -v git
assert "running inside a git repository" git -C "$ROOT" rev-parse --is-inside-work-tree

# ── Check 1: packageManager field set to npm in all package.json files ───────
echo ""
echo "Check 1: packageManager field set to npm in package.json files"
for pkg in $PKG_FILES; do
    assert "$pkg has packageManager set to npm" grep -qE '"packageManager"\s*:\s*"npm@' "$ROOT/$pkg"
done

# ── Check 2: packageManager version consistent across all package.json files ─
# Defensive shape (task 1326): the subshell enables `set -euo pipefail` so that
# grep failures inside the pipeline (e.g. a missing package.json file) propagate
# instead of being masked by the final `tr -d` exit status. An explicit `[ -f ]`
# preflight provides belt-and-braces coverage. The dual assertion — total == PKG_COUNT
# AND unique == 1 — catches the case where one file is missing the
# packageManager field entirely: grep emits fewer lines, which `sort -u` would
# otherwise silently collapse to a single unique line.
echo ""
echo "Check 2: packageManager version consistent across package.json files"
assert "all package.json files agree on packageManager version" bash -c "
    set -euo pipefail
    for p in $PKG_FILES; do
        [ -f \"$ROOT/\$p\" ] || exit 1
    done
    total=\$(for p in $PKG_FILES; do
        grep -ohE '\"packageManager\"\\s*:\\s*\"[^\"]+\"' \"$ROOT/\$p\"
    done | wc -l | tr -d ' ')
    unique=\$(for p in $PKG_FILES; do
        grep -ohE '\"packageManager\"\\s*:\\s*\"[^\"]+\"' \"$ROOT/\$p\"
    done | sort -u | wc -l | tr -d ' ')
    [ \"\$total\" = '$PKG_COUNT' ] && [ \"\$unique\" = '1' ]
"

# ── Check 3: npm lockfiles NOT in .gitignore ────────────────────────
# Refactored in task 976 for three reasons:
#   1. No subshell: eliminates the embedded subprocess shell invocation entirely.
#   2. Pre-computed: git check-ignore runs exactly once; both the assert and the
#      diagnostic guard reuse the cached exit code, avoiding a second process fork.
#   3. Exit-code disambiguation: exit codes >=128 indicate a broken git invocation
#      (corrupt repo, missing binary) and are surfaced as an explicit ERROR rather
#      than masquerading as 'not ignored'. Stderr passes through naturally;
#      no 2>/dev/null suppression.
echo ""
echo "Check 3: npm lockfiles not gitignored"
check_ignore_status=0
# shellcheck disable=SC2086
# SC2086: word-splitting on $LOCK_FILES is intentional — passes multiple
# filenames as separate arguments to git check-ignore, consistent with
# how the rest of this script expands $LOCK_FILES and $PKG_FILES.
check_ignore_output=$(cd "$ROOT" && git check-ignore $LOCK_FILES) || check_ignore_status=$?
if [ "$check_ignore_status" -ge 128 ]; then
    echo "  ERROR: git check-ignore failed with exit status $check_ignore_status" >&2
    exit 1
fi
assert "no npm lockfiles are gitignored" test "$check_ignore_status" -eq 1
if [ "$check_ignore_status" -ne 1 ]; then
    echo "  DIAGNOSTIC: gitignored lockfiles (status=$check_ignore_status):"
    printf '%s\n' "$check_ignore_output" | sed 's/^/    /'
    echo "  DIAGNOSTIC: re-running 'git check-ignore -v' per file to identify offender(s):"
    for f in $LOCK_FILES; do
        (cd "$ROOT" && git check-ignore -v "$f") || true
    done
fi

# ── Check 4: pnpm-lock.yaml IS in .gitignore ────────────────────────
# Two-step assertion (task 976): distinguishes two failure modes.
#   Step 1 fail → pnpm-lock.yaml is absent from .gitignore entirely.
#   Step 1 pass + step 2 fail → mentioned but not in the canonical form
#     (gui/ exact path or **/ glob prefix). The grep -qE accepts both forms
#     so neither .gitignore style forces a migration.
echo ""
echo "Check 4: pnpm-lock.yaml gitignored"
assert "pnpm-lock.yaml is mentioned in .gitignore" grep -q 'pnpm-lock\.yaml' "$ROOT/.gitignore"
assert "pnpm-lock.yaml uses **/ glob or exact gui/ path in .gitignore" \
    grep -qE '(^\*\*/|^gui/)pnpm-lock\.yaml$' "$ROOT/.gitignore"

test_summary
