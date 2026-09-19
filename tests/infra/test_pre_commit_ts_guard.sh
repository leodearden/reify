#!/usr/bin/env bash
# Infrastructure test for task #7538 (amendment pass): behavioural coverage
# for hooks/pre-commit's tree-sitter generated-file guard.
#
# The guard's alternation now has ten branches (eight literal targets under
# tree-sitter-reify/src/ from the root .gitignore's "Tree-sitter generated"
# block, plus the CLI binary and ad-hoc *.o files from the nested
# tree-sitter-reify/.gitignore) with hand-escaped metacharacters. Before this
# test, nothing ever staged a tree-sitter artifact and ran the hook — a typo
# in any branch (a missing backslash, a `$` anchor silently excluding a case)
# would leave the guard green while the regression it exists to prevent (a
# 63K-line parser.c riding a WIP-save sweep onto a task branch) reappeared.
#
# The path list is DERIVED from the two .gitignore files rather than
# hand-duplicated here, so this test also catches the guard's regex drifting
# out of sync with either list — the property task #7538's follow-up review
# asked for.
#
# The hook is driven DIRECTLY (bash hooks/pre-commit) inside a throwaway temp
# git repo, so the real repository's index is never touched — same pattern as
# tests/infra/test_reference_transaction_gate.sh. A non-main branch is used
# throughout so the guard's own early-exit (branch != main) is reached right
# after the universal guards run, without needing hooks/main-gate-lib.sh or
# hooks/project-checks in the fixture.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== pre-commit tree-sitter generated-file guard (task #7538 amendment) ==="

PRE_COMMIT="$REPO_ROOT/hooks/pre-commit"
ROOT_GITIGNORE="$REPO_ROOT/.gitignore"
NESTED_GITIGNORE="$REPO_ROOT/tree-sitter-reify/.gitignore"

assert "hooks/pre-commit exists" test -f "$PRE_COMMIT"

# -- derive the guarded path list from the two .gitignore files ---------------
# Root .gitignore's "Tree-sitter generated" block: the header comment through
# the next blank line, filtered to the path lines themselves.
mapfile -t ROOT_BLOCK_PATHS < <(
    sed -n '/^# Tree-sitter generated/,/^$/p' "$ROOT_GITIGNORE" | grep '^tree-sitter-reify/'
)
assert "root .gitignore Tree-sitter generated block has 8 path lines" \
    test "${#ROOT_BLOCK_PATHS[@]}" -eq 8

# Nested tree-sitter-reify/.gitignore additionally ignores the CLI binary and
# ad-hoc *.o files — neither pattern exists in the root .gitignore.
assert "nested tree-sitter-reify/.gitignore still ignores /tree-sitter" \
    bash -c "grep -qF '/tree-sitter' '$NESTED_GITIGNORE'"
assert "nested tree-sitter-reify/.gitignore still ignores *.o" \
    bash -c "grep -qxF '*.o' '$NESTED_GITIGNORE'"

# Turn each root-block entry into a concrete stageable file path: a glob
# (.tmp-*) becomes a literal example, and a directory (.generate.lock.d) gets
# a file underneath it (git never stages a bare directory). Everything else
# passes through unchanged.
GUARDED_PATHS=()
for p in "${ROOT_BLOCK_PATHS[@]}"; do
    case "$p" in
        */.tmp-\*)          GUARDED_PATHS+=("${p%\*}foo") ;;
        */.generate.lock.d) GUARDED_PATHS+=("$p/held-by-pid-123") ;;
        *)                  GUARDED_PATHS+=("$p") ;;
    esac
done
GUARDED_PATHS+=("tree-sitter-reify/tree-sitter" "tree-sitter-reify/src/scanner.o")
assert "derived fixture list has 10 concrete guarded paths" \
    test "${#GUARDED_PATHS[@]}" -eq 10

# -- throwaway git repo with just the (tracked) hook ---------------------------
FIX="$(mktemp -d)"; _TMPDIRS+=("$FIX")
git -C "$FIX" init -q
git -C "$FIX" config user.email test@test.com
git -C "$FIX" config user.name Test
# Create the branch before the first commit so this works regardless of the
# host's init.defaultBranch. The guard runs on every branch; staying off
# "main" keeps this test off the main-only project-checks path.
git -C "$FIX" checkout -q -b task/fixture
mkdir -p "$FIX/hooks"
cp "$PRE_COMMIT" "$FIX/hooks/pre-commit"
chmod +x "$FIX/hooks/pre-commit"
git -C "$FIX" add hooks/pre-commit
git -C "$FIX" commit -q -m "base: pre-commit hook"

HOOK="$FIX/hooks/pre-commit"

# stage <path> — write a fixture file (creating parent dirs as needed) and
# force-add it, mirroring a WIP-save sweep that force-adds past .gitignore.
stage() {
    local f="$1"
    mkdir -p "$FIX/$(dirname "$f")"
    echo "content" > "$FIX/$f"
    git -C "$FIX" add -f -- "$f"
}
# run_hook — invoke the hook directly (not via git's hook wiring, same as
# test_reference_transaction_gate.sh) from the fixture root; sets RUN_RC.
run_hook() {
    local rc=0
    ( cd "$FIX" && bash "$HOOK" ) || rc=$?
    RUN_RC=$rc
}
# reset_index — back to the base commit's index/worktree. hooks/pre-commit is
# TRACKED (committed above), so plain `clean -fd` (no -x) never removes it —
# only the untracked fixture files/dirs each scenario creates.
reset_index() {
    git -C "$FIX" reset -q HEAD -- . >/dev/null 2>&1 || true
    git -C "$FIX" clean -qfd
}

# -- (a) each guarded path, staged alongside one ordinary file: unstaged,
#        ordinary file survives, hook exits 0 -------------------------------
for gp in "${GUARDED_PATHS[@]}"; do
    echo ""
    echo "--- (a) $gp: unstaged, ordinary file survives, hook exits 0 ---"
    reset_index
    stage "$gp"
    stage "README.md"
    run_hook
    assert "(a:$gp) hook exits 0 (ordinary file still staged)" test "$RUN_RC" -eq 0
    assert "(a:$gp) guarded path is unstaged" \
        bash -c "! git -C '$FIX' diff --cached --name-only | grep -qxF '$gp'"
    assert "(a:$gp) ordinary file remains staged" \
        bash -c "git -C '$FIX' diff --cached --name-only | grep -qxF 'README.md'"
done

# -- (b) a guarded path as the ONLY staged file: unstaged, hook BLOCKS -------
for gp in "${GUARDED_PATHS[@]}"; do
    echo ""
    echo "--- (b) $gp: sole staged file -> BLOCKED (exit 1) ---"
    reset_index
    stage "$gp"
    run_hook
    assert "(b:$gp) hook exits 1 (nothing left staged)" test "$RUN_RC" -eq 1
    assert "(b:$gp) guarded path is unstaged" \
        bash -c "! git -C '$FIX' diff --cached --name-only | grep -qxF '$gp'"
done

# -- (c) an ordinary file alone is untouched (exit 0, no unstaging) ----------
echo ""
echo "--- (c) ordinary file alone -> exit 0, untouched ---"
reset_index
stage "README.md"
run_hook
assert "(c) hook exits 0" test "$RUN_RC" -eq 0
assert "(c) ordinary file remains staged" \
    bash -c "git -C '$FIX' diff --cached --name-only | grep -qxF 'README.md'"

test_summary
