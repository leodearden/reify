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

# Guard: verify the subtasks array is non-empty so a missing/empty stream
# cannot cause the grep-based assertions below to pass trivially.
assert "hook: HEAD tasks.json has at least one subtask (stream non-empty guard)" \
    bash -c "[ \"\$(git -C '$_repo1' show HEAD:.taskmaster/tasks/tasks.json \
             | jq '[.master.tasks[].subtasks[]] | length')\" -gt 0 ]"

assert "hook: numeric subtask ids are string in HEAD after auto-commit" \
    bash -c "! git -C '$_repo1' show HEAD:.taskmaster/tasks/tasks.json | jq -r '.master.tasks[].subtasks[].id | type' | grep -q 'number'"

assert "hook: subtask ids are digit-strings in HEAD after auto-commit" \
    bash -c "! git -C '$_repo1' show HEAD:.taskmaster/tasks/tasks.json | jq -r '.master.tasks[].subtasks[].id' | grep -vqE '^[0-9]+\$'"

# ==============================================================================
# Check 2: RECURSION GUARD — hook is a no-op when guard env var is set
# ==============================================================================
# This check is carefully constructed so that removing the guard from the hook
# would cause BOTH assertions to FAIL.  The key trick: we disable the hook
# during the initial commit so HEAD actually stores numeric IDs.  Then we
# invoke the hook with the guard env var set and verify it did nothing.
#
# Without the guard:
#   - step (5) of the hook would normalise the disk file to digit-strings  → Assert A fails
#   - step (6) would amend HEAD because disk≠HEAD                          → Assert B fails
# With the guard:
#   - hook exits at step (1) before touching anything                       → both pass
echo ""
echo "--- Check 2: recursion guard ---"

mk_repo_fixture _repo2
mkdir -p "$_repo2/.taskmaster/tasks"
cat > "$_repo2/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":"1","subtasks":[{"id":99}]}]}}
JSON

# Disable the hook so the initial commit records genuine numeric IDs in HEAD.
mv "$_repo2/hooks/post-commit" "$_repo2/hooks/post-commit.off"

git -C "$_repo2" add .taskmaster/tasks/tasks.json
git -C "$_repo2" commit --no-verify -m "initial (hook disabled)" -q

# Restore the hook for the guard test.
mv "$_repo2/hooks/post-commit.off" "$_repo2/hooks/post-commit"

# Sanity: on-disk file must be numeric (proves the hook didn't run during commit).
assert "recursion guard setup: on-disk tasks.json is numeric before guard test" \
    bash -c "jq -e '.master.tasks[0].subtasks[0].id | type == \"number\"' \
             '$_repo2/.taskmaster/tasks/tasks.json' >/dev/null"

# Sanity: HEAD's committed copy must also be numeric.
assert "recursion guard setup: HEAD tasks.json has numeric subtask id" \
    bash -c "git -C '$_repo2' show HEAD:.taskmaster/tasks/tasks.json \
             | jq -e '.master.tasks[0].subtasks[0].id | type == \"number\"' >/dev/null"

_guard_sha_before="$(git -C "$_repo2" rev-parse HEAD)"

# Invoke the hook directly with the guard env var set.
# cwd must be inside the repo so git rev-parse --show-toplevel resolves correctly.
(cd "$_repo2" && _REIFY_TASKS_NORMALIZE_AMEND=1 bash hooks/post-commit) || true

# Assert A: on-disk tasks.json is still numeric (hook exited before normalizing).
assert "recursion guard: on-disk tasks.json stays numeric when guard env var is set" \
    bash -c "jq -e '.master.tasks[0].subtasks[0].id | type == \"number\"' \
             '$_repo2/.taskmaster/tasks/tasks.json' >/dev/null"

# Assert B: HEAD sha unchanged (no amend fired).
assert "recursion guard: HEAD sha unchanged when guard env var is set" \
    test "$_guard_sha_before" = "$(git -C "$_repo2" rev-parse HEAD)"

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
# cwd must be inside the fixture so git rev-parse --show-toplevel resolves
# to the fixture repo rather than the real reify repo running the tests.
(cd "$_repo4" && _REIFY_TASKS_NORMALIZE_AMEND="" bash hooks/post-commit) || true
_norm_sha_after="$(git -C "$_repo4" rev-parse HEAD)"

assert "already-normalized: HEAD sha unchanged after re-running hook" \
    test "$_norm_sha" = "$_norm_sha_after"

# ==============================================================================
# Check 5: MERGE COMMIT — hook normalizes IDs introduced via merge
# ==============================================================================
# The hook uses `git diff-tree -m` so that merge commits are diffed against
# each parent.  Without -m, git diff-tree emits no output for merges and the
# hook would silently skip normalization of IDs introduced on the merged branch.
#
# Setup:
#   - initial commit on default branch (no tasks.json)
#   - 'side' branch: add tasks.json with numeric subtask id (committed --no-verify,
#     so the hook does NOT fire on this commit — numeric IDs survive into the merge)
#   - default branch: add an unrelated file to give the merge two real parents
#   - git merge --no-ff side  → merge commit; post-commit hook fires
#
# Regression: removing -m from the hook would cause Assert A below to FAIL
# (tasks.json in HEAD would retain numeric IDs because the merge commit would
# not appear in git diff-tree output without the -m flag).
echo ""
echo "--- Check 5: merge commit — IDs normalized via -m flag ---"

mk_repo_fixture _repo5

# Initial commit on the default branch (no tasks.json).
echo "init" > "$_repo5/README.md"
git -C "$_repo5" add README.md
git -C "$_repo5" commit --no-verify -m "initial" -q

# Capture the default branch name now that HEAD is detached no more.
_main_branch5="$(git -C "$_repo5" rev-parse --abbrev-ref HEAD)"

# Create a side branch and commit tasks.json with numeric IDs.
# Use --no-verify so the post-commit hook does NOT fire on this commit — we
# want numeric IDs to survive into the merge and be normalized only by the
# hook on the merge commit itself.
git -C "$_repo5" checkout -b side -q
mkdir -p "$_repo5/.taskmaster/tasks"
cat > "$_repo5/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":"10","subtasks":[{"id":77}]}]}}
JSON
git -C "$_repo5" add .taskmaster/tasks/tasks.json
git -C "$_repo5" commit --no-verify -m "chore(tasks): add tasks with numeric subtask id" -q

# Switch back to default branch and add an unrelated file so the merge has
# two real parents (prevents fast-forward even with --no-ff).
git -C "$_repo5" checkout "$_main_branch5" -q
echo "other" > "$_repo5/other.txt"
git -C "$_repo5" add other.txt
git -C "$_repo5" commit --no-verify -m "docs: add other file" -q

# Perform the merge.  --no-ff forces a merge commit.  The post-commit hook
# fires automatically after git creates the merge commit.
# Note: --no-verify on git-merge only suppresses pre-commit/commit-msg hooks;
# the post-commit hook still fires — that is the behaviour being tested.
git -C "$_repo5" merge --no-edit --no-ff side -q >/dev/null 2>&1

# Assert A: tasks.json in HEAD has normalized (string) subtask ids.
# Without -m in the hook, diff-tree emits no output for the merge commit and
# the hook exits early without normalizing — this assert would FAIL.
assert "merge commit: numeric subtask id normalized in HEAD after merge" \
    bash -c "! git -C '$_repo5' show HEAD:.taskmaster/tasks/tasks.json \
             | jq -r '.master.tasks[].subtasks[].id | type' | grep -q 'number'"

# Assert B: HEAD is a merge commit (has two parents) — proves the merge path
# was exercised and not a fast-forward or regular commit.
assert "merge commit: HEAD is a merge commit (has two parents)" \
    bash -c "[ \"\$(git -C '$_repo5' log --format='%P' -1 \
             | tr ' ' '\n' | grep -c .)\" = '2' ]"

# ==============================================================================
# Check 6: python3-MISSING is a loud failure (ERROR: + marker + non-zero exit)
# ==============================================================================
# Task 1916 replaced the old silent fallback (exit 0 on python3-missing) with
# a loud failure: the hook now exits non-zero, logs ERROR: to stderr, and
# writes .git/NORMALIZE_FAILED so a broken normalize step surfaces at the
# next `git status` rather than being hidden until the post-rebase verify gate.
echo ""
echo "--- Check 6: python3-missing is a loud failure ---"

mk_repo_fixture _repo6py
mkdir -p "$_repo6py/.taskmaster/tasks"
cat > "$_repo6py/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":1,"subtasks":[]}]}}
JSON

# Set up a numeric-ID commit so the hook has work to do.
git -C "$_repo6py" add .taskmaster/tasks/tasks.json
git -C "$_repo6py" commit --no-verify -m "chore(tasks): numeric ids" -q

# Build a PATH where python3 is absent but standard tools remain.
#
# Why not PATH="$stub:$PATH" with a non-executable python3 placeholder?
# bash's `command -v` skips non-executable files and continues searching
# subsequent PATH directories, so a non-executable stub would not prevent
# it from finding the real python3 in /usr/bin — the check would succeed
# and we would never exercise the python3-missing code path.
#
# Instead we build a symlink stub for common tools and use it as the sole
# PATH.  The list covers coreutils the hook and git helpers may call; extend
# it if the hook gains new dependencies.
_stub6py="$(mktemp -d -p "$_tmpdir")"
for _bin6py in git grep date bash sh env printf cut tr awk sed cat wc; do
    _loc6py="$(command -v "$_bin6py" 2>/dev/null || true)"
    [ -n "$_loc6py" ] && ln -sf "$_loc6py" "$_stub6py/$_bin6py"
done
# GIT_EXEC_PATH ensures git can reach its built-in sub-commands (e.g.
# git-diff-tree in /usr/lib/git-core) even when that directory is not
# listed in the stub PATH.
_git_exec_path6py="$(git --exec-path 2>/dev/null || true)"

_stderr6py="$_tmpdir/stderr6py.txt"
_hook6py_exit=0
(cd "$_repo6py" && GIT_EXEC_PATH="$_git_exec_path6py" PATH="$_stub6py" \
    bash hooks/post-commit 2>"$_stderr6py") || _hook6py_exit=$?

assert "python3-missing: hook exits non-zero" \
    test "$_hook6py_exit" -ne 0
assert "python3-missing: stderr contains ERROR:" \
    grep -q "ERROR:" "$_stderr6py"
assert "python3-missing: .git/NORMALIZE_FAILED marker file exists" \
    test -f "$_repo6py/.git/NORMALIZE_FAILED"

# ==============================================================================
# Check 7: normalizer-CRASH is a loud failure (ERROR: + marker + non-zero exit)
# ==============================================================================
# Symmetric to Check 6: when python3 is present but the normalizer script
# itself exits non-zero, the hook must still surface that loudly.  This
# replaces the prior silent '|| { ... exit 0 }' behaviour.
echo ""
echo "--- Check 7: normalizer-crash is a loud failure ---"

mk_repo_fixture _repo7
mkdir -p "$_repo7/.taskmaster/tasks"
cat > "$_repo7/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":1,"subtasks":[]}]}}
JSON

# Set up a numeric-ID commit.
git -C "$_repo7" add .taskmaster/tasks/tasks.json
git -C "$_repo7" commit --no-verify -m "chore(tasks): numeric ids" -q

# Replace the normalizer with a stub that always exits non-zero.
printf '#!/usr/bin/env python3\nimport sys\nsys.exit(1)\n' > "$_repo7/scripts/normalize_tasks_json.py"
chmod +x "$_repo7/scripts/normalize_tasks_json.py"

_stderr7="$_tmpdir/stderr7.txt"
_hook7_exit=0
(cd "$_repo7" && bash hooks/post-commit 2>"$_stderr7") || _hook7_exit=$?

assert "normalizer-crash: hook exits non-zero" \
    test "$_hook7_exit" -ne 0
assert "normalizer-crash: stderr contains ERROR:" \
    grep -q "ERROR:" "$_stderr7"
assert "normalizer-crash: .git/NORMALIZE_FAILED marker file exists" \
    test -f "$_repo7/.git/NORMALIZE_FAILED"

# ==============================================================================
# Check 8: MARKER CLEARING — a stale NORMALIZE_FAILED is removed on success
# ==============================================================================
# The hook's rm -f on the marker file runs only when both normalization AND
# the amend step succeed.  This check guards the clearing behaviour: a
# regression that moved the rm before python3 (losing the crash-marker
# semantics) or dropped it entirely (making the marker permanent) would be
# caught here.
echo ""
echo "--- Check 8: stale marker cleared on successful normalization ---"

mk_repo_fixture _repo8
mkdir -p "$_repo8/.taskmaster/tasks" "$_repo8/.git"
cat > "$_repo8/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":"1","subtasks":[{"id":42}]}]}}
JSON

# Pre-create the marker to simulate a previous failed attempt.  The hook
# must remove this on a successful run.
printf 'stale\tprior-run\tfeedeadbeef\n' > "$_repo8/.git/NORMALIZE_FAILED"
assert "clearing: stale marker file exists before commit (test setup)" \
    test -f "$_repo8/.git/NORMALIZE_FAILED"

# Commit with numeric subtask id — hook normalizes + amends + clears marker.
git -C "$_repo8" add .taskmaster/tasks/tasks.json
git -C "$_repo8" commit --no-verify -m "chore(tasks): commit with stale marker present" -q

assert "clearing: .git/NORMALIZE_FAILED removed after successful hook run" \
    test ! -e "$_repo8/.git/NORMALIZE_FAILED"
assert "clearing: subtask ids normalized in HEAD (hook actually ran)" \
    bash -c "! git -C '$_repo8' show HEAD:.taskmaster/tasks/tasks.json \
             | jq -r '.master.tasks[].subtasks[].id | type' | grep -q 'number'"

# ==============================================================================
# Check 9: hook surfaces briefing mismatch on set_task_status(555=done) commit
# ==============================================================================
# Builds a fixture repo that includes the briefing script, a review/briefing.yaml
# with a known_gap tracking task 555, and a tasks.json with task 555 done.
# A commit with message matching set_task_status(NNN=done) must trigger the
# briefing check. Hook must still exit 0 (informational only), and the
# hook's stderr must contain "WARN" and "555".
echo ""
echo "--- Check 9: briefing mismatch surfaced on done-task commit ---"

REFRESH_SCRIPT="$REPO_ROOT/scripts/refresh_briefing_known_gaps.py"

mk_repo_fixture _repo9
# Copy the briefing script into the fixture (next to normalize script).
if [ -f "$REFRESH_SCRIPT" ]; then
    cp "$REFRESH_SCRIPT" "$_repo9/scripts/refresh_briefing_known_gaps.py"
    chmod +x "$_repo9/scripts/refresh_briefing_known_gaps.py"
fi

# Create review/briefing.yaml with a known_gap tracking task 555.
mkdir -p "$_repo9/review"
cat > "$_repo9/review/briefing.yaml" <<'YAML'
subprojects:
  tooling:
    known_gaps:
      - what: "LSP gap that was actually fixed"
        tracking: "555"
YAML

# Create tasks.json with task 555 marked done.
mkdir -p "$_repo9/.taskmaster/tasks"
cat > "$_repo9/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":"555","title":"Fix LSP gap","status":"done"}]}}
JSON

# Seed HEAD with an initial commit (--no-verify so hook doesn't fire yet).
git -C "$_repo9" add .
git -C "$_repo9" commit --no-verify -m "chore: seed initial state" -q

# Now make the real commit: message matches set_task_status(555=done).
# The post-commit hook should fire and invoke refresh_briefing_known_gaps.py.
# Capture the hook's combined stdout+stderr to check for WARN.
_stderr9pc="$_tmpdir/stderr9_postcommit.txt"
_hook9_exit=0
(cd "$_repo9" && git commit --allow-empty \
    -m "chore(tasks): auto-commit after set_task_status(555=done)" \
    2>"$_stderr9pc") || _hook9_exit=$?

assert "Check 9: hook exited 0 (briefing check is informational)" \
    test "$_hook9_exit" -eq 0

assert "Check 9: hook stderr contains WARN" \
    grep -q "WARN" "$_stderr9pc"

assert "Check 9: hook stderr contains task id 555" \
    grep -q "555" "$_stderr9pc"

# ==============================================================================
# Check 10: non-task commits must NOT invoke the briefing script
# ==============================================================================
# Regression: a README.md change with message "docs: update" must not trigger
# step (3.5). We replace the briefing script with a stub that writes a sentinel
# file so we can detect whether it was called.
echo ""
echo "--- Check 10: non-task commit does not invoke briefing script ---"

mk_repo_fixture _repo10
cp "$REFRESH_SCRIPT" "$_repo10/scripts/refresh_briefing_known_gaps.py"
chmod +x "$_repo10/scripts/refresh_briefing_known_gaps.py"

mkdir -p "$_repo10/review"
cat > "$_repo10/review/briefing.yaml" <<'YAML'
subprojects:
  tooling:
    known_gaps: []
YAML

mkdir -p "$_repo10/.taskmaster/tasks"
cat > "$_repo10/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[]}}
JSON

# Replace the briefing script with a sentinel stub.
_sentinel10="$_tmpdir/sentinel10.txt"
cat > "$_repo10/scripts/refresh_briefing_known_gaps.py" <<STUB
#!/usr/bin/env python3
import sys
with open("$_sentinel10", "w") as f:
    f.write("called\n")
sys.exit(0)
STUB
chmod +x "$_repo10/scripts/refresh_briefing_known_gaps.py"

# Seed HEAD (no-verify so hook doesn't fire on the seed).
git -C "$_repo10" add .
git -C "$_repo10" commit --no-verify -m "chore: seed" -q

# Commit an unrelated file with a non-matching message.
echo "hello" > "$_repo10/README.md"
git -C "$_repo10" add README.md
git -C "$_repo10" commit -m "docs: update readme" 2>/dev/null || true

assert "Check 10: non-task commit does not invoke briefing script (sentinel absent)" \
    test ! -f "$_sentinel10"

# ==============================================================================
# Check 11: in-progress task commit must NOT invoke the briefing script
# ==============================================================================
# Regression: set_task_status(555=in-progress) must NOT match the done pattern.
echo ""
echo "--- Check 11: in-progress task commit does not invoke briefing script ---"

mk_repo_fixture _repo11
cp "$REFRESH_SCRIPT" "$_repo11/scripts/refresh_briefing_known_gaps.py"
chmod +x "$_repo11/scripts/refresh_briefing_known_gaps.py"

mkdir -p "$_repo11/review"
cat > "$_repo11/review/briefing.yaml" <<'YAML'
subprojects:
  tooling:
    known_gaps: []
YAML

mkdir -p "$_repo11/.taskmaster/tasks"
cat > "$_repo11/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[]}}
JSON

# Replace the briefing script with a sentinel stub.
_sentinel11="$_tmpdir/sentinel11.txt"
cat > "$_repo11/scripts/refresh_briefing_known_gaps.py" <<STUB
#!/usr/bin/env python3
import sys
with open("$_sentinel11", "w") as f:
    f.write("called\n")
sys.exit(0)
STUB
chmod +x "$_repo11/scripts/refresh_briefing_known_gaps.py"

# Seed HEAD then make the in-progress commit.
git -C "$_repo11" add .
git -C "$_repo11" commit --no-verify -m "chore: seed" -q

# Make a new tasks.json change so the tasks.json gate passes (step 4).
echo '{"master":{"tasks":[{"id":"555","status":"in-progress"}]}}' \
    > "$_repo11/.taskmaster/tasks/tasks.json"
git -C "$_repo11" add .taskmaster/tasks/tasks.json
git -C "$_repo11" commit -m "chore(tasks): auto-commit after set_task_status(555=in-progress)" \
    2>/dev/null || true

assert "Check 11: in-progress commit does not invoke briefing script (sentinel absent)" \
    test ! -f "$_sentinel11"

# ==============================================================================
# Check 12: hook tolerates a briefing script that exits non-zero
# ==============================================================================
# REGRESSION-DETECTION MECHANISM:
# If `|| true` is dropped from hooks/post-commit's step (3.5) invocation of the
# briefing script, the briefing script's exit 99 leaks back through `set -eu`,
# causing the hook to abort BEFORE step (5) (the normalizer). With the hook
# aborted, the int→string conversion never happens, so the subtask id in
# tasks.json on disk remains a JSON number. The primary assertion below checks
# for that exact condition via `jq type == "string"`.
#
# The test mechanism works because:
#   1. The seed commit uses an EMPTY tasks.json (no subtasks), so the seed
#      commit's post-commit hook run is a no-op at step (5) and does not
#      pre-normalize the int id we add next.
#   2. After the seed, we overwrite tasks.json with a numeric subtask id so a
#      real commit (not --allow-empty) triggers both step (3.5) (briefing gate)
#      and step (5) (normalizer).
#   3. We confirm step (5) ran end-to-end by checking the on-disk subtask id
#      type in tasks.json after the commit.
#
# Summary: if `|| true` is present → step (3.5) absorbs exit 99 → step (5) runs
# → subtask id is string → assertion PASSES.  If `|| true` is absent → hook
# aborts after step (3.5) → step (5) never runs → subtask id is number →
# assertion FAILS.
echo ""
echo "--- Check 12: hook tolerates briefing script non-zero exit ---"

mk_repo_fixture _repo12
mkdir -p "$_repo12/review" "$_repo12/.taskmaster/tasks"

cat > "$_repo12/review/briefing.yaml" <<'YAML'
subprojects:
  tooling:
    known_gaps: []
YAML

# SEED with empty tasks.json — no subtasks means the normalizer is a no-op at
# step (5) of the seed commit's hook run, so the int subtask id we add next is
# NOT pre-normalized before the real test commit fires.
cat > "$_repo12/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[]}}
JSON

# Replace the briefing script with a stub that always exits 99.
cat > "$_repo12/scripts/refresh_briefing_known_gaps.py" <<'STUB'
#!/usr/bin/env python3
import sys
sys.exit(99)
STUB
chmod +x "$_repo12/scripts/refresh_briefing_known_gaps.py"

# Seed HEAD. Note: --no-verify bypasses pre-commit/commit-msg only; post-commit
# still fires. With empty tasks.json the normalizer is a no-op at step (5).
git -C "$_repo12" add .
git -C "$_repo12" commit --no-verify -m "chore: seed" -q

# Overwrite tasks.json with a numeric subtask id. This is what the normalizer
# (step 5) must convert to a string — confirming it ran past the exit-99 stub.
cat > "$_repo12/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[{"id":"555","title":"Fix thing","status":"done","subtasks":[{"id":42,"title":"sub","status":"done"}]}]}}
JSON

# Commit the updated tasks.json — NOT --allow-empty so the file is in the diff
# and step (4) does NOT short-circuit, ensuring step (5) will actually run.
_head12_before="$(git -C "$_repo12" rev-parse HEAD)"
_hook12_exit=0
(cd "$_repo12" && \
    git add .taskmaster/tasks/tasks.json && \
    git commit -m "chore(tasks): auto-commit after set_task_status(555=done)" \
    ) 2>/dev/null || _hook12_exit=$?

# PRIMARY: normalizer ran past the failing briefing script (subtask id is string).
# If `|| true` is dropped from hooks/post-commit step (3.5): the briefing script's
# exit 99 leaks back through `set -eu`, the hook aborts, step (5) never runs, the
# subtask id on disk stays a JSON number, and this assertion FAILS.
assert "Check 12: normalizer ran past failing briefing script (subtask id is string, not numeric)" \
    bash -c "jq -e '.master.tasks[0].subtasks[0].id | type == \"string\"' \
             '$_repo12/.taskmaster/tasks/tasks.json' >/dev/null"

# Secondary: the commit itself still succeeded and HEAD advanced.
assert "Check 12: HEAD advanced (commit was not rolled back)" \
    bash -c "test \"\$(git -C '$_repo12' rev-parse HEAD)\" != '$_head12_before'"

# Secondary: NORMALIZE_FAILED not created (briefing failure ≠ normalize failure).
assert "Check 12: .git/NORMALIZE_FAILED not created (briefing failure ≠ normalize failure)" \
    test ! -f "$_repo12/.git/NORMALIZE_FAILED"

# ==============================================================================
# Check 13: non-canonical commit message embedding the pattern does NOT trigger
# ==============================================================================
# The regex in step (3.5) is anchored to the full canonical auto-commit prefix:
#   ^chore(tasks): auto-commit after set_task_status(NNN=done)$
# A hand-authored commit like "fix: don't call set_task_status(555=done) twice"
# contains the pattern but is NOT anchored — it must NOT trigger the check.
echo ""
echo "--- Check 13: non-canonical commit embedding pattern does not invoke briefing script ---"

mk_repo_fixture _repo13

REFRESH_SCRIPT_LOCAL="$REPO_ROOT/scripts/refresh_briefing_known_gaps.py"

mkdir -p "$_repo13/review" "$_repo13/.taskmaster/tasks"
cat > "$_repo13/review/briefing.yaml" <<'YAML'
subprojects:
  tooling:
    known_gaps: []
YAML
cat > "$_repo13/.taskmaster/tasks/tasks.json" <<'JSON'
{"master":{"tasks":[]}}
JSON

# Replace the briefing script with a sentinel stub.
_sentinel13="$_tmpdir/sentinel13.txt"
cat > "$_repo13/scripts/refresh_briefing_known_gaps.py" <<STUB
#!/usr/bin/env python3
import sys
with open("$_sentinel13", "w") as f:
    f.write("called\n")
sys.exit(0)
STUB
chmod +x "$_repo13/scripts/refresh_briefing_known_gaps.py"

# Seed HEAD (no-verify so hook does not fire on the seed commit).
git -C "$_repo13" add .
git -C "$_repo13" commit --no-verify -m "chore: seed" -q

# Commit with the pattern embedded in the subject but NOT at the canonical
# auto-commit prefix position.  The anchored regex should NOT match.
echo "extra" > "$_repo13/extra.txt"
git -C "$_repo13" add extra.txt
git -C "$_repo13" commit -m "fix: don't call set_task_status(555=done) twice" 2>/dev/null || true

assert "Check 13: non-canonical embed does not invoke briefing script (sentinel absent)" \
    test ! -f "$_sentinel13"

# -- Summary ------------------------------------------------------------------
test_summary
