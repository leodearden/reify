#!/usr/bin/env bash
# Tests for hooks/post-commit — the ID-normalization post-commit hook.
# All assertions use self-contained synthetic git repos so the real
# .taskmaster/tasks/tasks.json is never modified.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
POST_COMMIT_HOOK="$REPO_ROOT/hooks/post-commit"
NORMALIZE_SCRIPT="$REPO_ROOT/scripts/normalize_tasks_json.py"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== post-commit hook tests ==="

# -- Setup: temp-dir fixture machinery ----------------------------------------
_tmpdir=$(mktemp -d)
trap 'rm -rf "$_tmpdir"' EXIT

# mk_repo_fixture VARNAME
# Creates a self-contained temp git repo with:
#   - scripts/normalize_tasks_json.py copied in and chmod +x
#   - hooks/post-commit copied in and chmod +x (if it exists)
#   - git config core.hooksPath hooks
#   - stub user.email / user.name
# Writes the repo path back to the caller via printf -v.
# Does NOT abort if the hook is missing — assertions about hook behaviour
# will simply FAIL rather than crashing the test script.
mk_repo_fixture() {
    if [ -z "${1:-}" ]; then
        echo "mk_repo_fixture: requires a non-empty varname argument" >&2
        return 1
    fi
    local _varname="$1"
    local d
    d=$(mktemp -d -p "$_tmpdir")
    git -C "$d" init -q
    git -C "$d" config user.email "test@example.com"
    git -C "$d" config user.name "Test User"
    git -C "$d" config core.hooksPath hooks
    mkdir -p "$d/scripts" "$d/hooks"
    cp "$NORMALIZE_SCRIPT" "$d/scripts/normalize_tasks_json.py"
    chmod +x "$d/scripts/normalize_tasks_json.py"
    # Copy hook only if it exists; absence makes Check 0 FAIL gracefully.
    if [ -f "$POST_COMMIT_HOOK" ]; then
        cp "$POST_COMMIT_HOOK" "$d/hooks/post-commit"
        chmod +x "$d/hooks/post-commit"
    fi
    printf -v "$_varname" '%s' "$d"
}

# ==============================================================================
# Check 0: hook file exists and is executable
# ==============================================================================
echo ""
echo "--- Check 0: hook existence and executability ---"

assert "hooks/post-commit exists" \
    test -f "$POST_COMMIT_HOOK"

assert "hooks/post-commit is executable" \
    test -x "$POST_COMMIT_HOOK"

# ==============================================================================
# Check 1: HAPPY PATH — numeric subtask IDs are normalized after commit
# ==============================================================================
echo ""
echo "--- Check 1: happy path — numeric IDs normalized by hook ---"

mk_repo_fixture _repo1
mkdir -p "$_repo1/.taskmaster/tasks"
cat > "$_repo1/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":"966","subtasks":[{"id":1},{"id":2}]}]}}
JSON

git -C "$_repo1" add .taskmaster/tasks/tasks.json
git -C "$_repo1" commit --no-verify -m "chore(tasks): auto-commit after set_task_status(966=pending)" -q

assert "hook: numeric subtask ids are string in HEAD after auto-commit" \
    bash -c "! git -C '$_repo1' show HEAD:.taskmaster/tasks/tasks.json | jq -r '.master.tasks[].subtasks[].id | type' | grep -q 'number'"

assert "hook: subtask ids are digit-strings in HEAD after auto-commit" \
    bash -c "! git -C '$_repo1' show HEAD:.taskmaster/tasks/tasks.json | jq -r '.master.tasks[].subtasks[].id' | grep -vqE '^[0-9]+\$'"

# ==============================================================================
# Check 2: RECURSION GUARD — hook is a no-op when guard env var is set
# ==============================================================================
echo ""
echo "--- Check 2: recursion guard ---"

mk_repo_fixture _repo2
mkdir -p "$_repo2/.taskmaster/tasks"
cat > "$_repo2/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":"1","subtasks":[{"id":99}]}]}}
JSON

# Commit numeric IDs so we have a base commit.
git -C "$_repo2" add .taskmaster/tasks/tasks.json
git -C "$_repo2" commit --no-verify -m "initial" -q

# Capture HEAD sha before invoking hook directly with guard set.
_guard_sha_before="$(git -C "$_repo2" rev-parse HEAD)"

# Invoke the hook directly with the guard env var set.
# It should exit immediately without amending anything.
_REIFY_TASKS_NORMALIZE_AMEND=1 bash "$_repo2/hooks/post-commit" || true

# HEAD sha must be unchanged (no amend fired).
_guard_sha_after="$(git -C "$_repo2" rev-parse HEAD)"

assert "recursion guard: HEAD sha unchanged when guard env var is set" \
    test "$_guard_sha_before" = "$_guard_sha_after"

# ==============================================================================
# Check 3: NON-TASKS COMMIT — hook is a no-op when tasks.json not in commit
# ==============================================================================
echo ""
echo "--- Check 3: non-tasks commit is a no-op ---"

mk_repo_fixture _repo3

# Commit only an unrelated file (no tasks.json in this commit).
echo "hello" > "$_repo3/README.md"
git -C "$_repo3" add README.md
git -C "$_repo3" commit --no-verify -m "docs: add readme" -q

# HEAD should have exactly one file (README.md); no amend should have
# introduced tasks.json.
_nontasks_files="$(git -C "$_repo3" diff-tree --root -r --no-commit-id --name-only HEAD | wc -l | tr -d ' ')"

assert "non-tasks commit: hook does not amend (HEAD still has only 1 file)" \
    test "$_nontasks_files" = "1"

assert "non-tasks commit: HEAD file is README.md (not tasks.json)" \
    bash -c "git -C '$_repo3' diff-tree --root -r --no-commit-id --name-only HEAD | grep -qF 'README.md'"

# ==============================================================================
# Check 4: ALREADY-NORMALIZED — hook amends but HEAD sha is the same
# ==============================================================================
echo ""
echo "--- Check 4: already-normalized commit is a no-op (no amend) ---"

mk_repo_fixture _repo4
mkdir -p "$_repo4/.taskmaster/tasks"
cat > "$_repo4/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":"1","subtasks":[{"id":"101"},{"id":"102"}]}]}}
JSON

git -C "$_repo4" add .taskmaster/tasks/tasks.json
git -C "$_repo4" commit --no-verify -m "chore(tasks): auto-commit" -q

# Capture sha immediately after commit (hook has already fired by now).
_norm_sha="$(git -C "$_repo4" rev-parse HEAD)"

# No second amendment should have happened; verify by re-running the hook
# and checking HEAD is still the same.
_REIFY_TASKS_NORMALIZE_AMEND="" bash "$_repo4/hooks/post-commit" || true
_norm_sha_after="$(git -C "$_repo4" rev-parse HEAD)"

assert "already-normalized: HEAD sha unchanged after re-running hook" \
    test "$_norm_sha" = "$_norm_sha_after"

# -- Summary ------------------------------------------------------------------
test_summary
