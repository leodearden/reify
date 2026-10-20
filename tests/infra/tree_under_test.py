"""
tree_under_test.py — a scratch `git clone --shared` of the tree under test
(PRD docs/prds/v0_6/verify-structured-plan-and-role-table.md D13).

Not a test module itself (no `test_` prefix): run_all.sh never discovers it,
and the suites import it via the sys.path idiom.

A suite that must run scripts/verify.sh somewhere other than the real checkout
uses this clone. The real checkout may carry a MERGE_HEAD, because the merge
gate's run_all runs inside a merge, and a MERGE_HEAD forces every non-`all`
scope to `all` for every role. The clone has no MERGE_HEAD and reads the same
tracked tree: the source's HEAD plus its uncommitted tracked diff, committed in
the clone. Untracked files are not carried over.

Every git call runs under hermetic_env() with hooks disabled. Nothing writes to
the source repository: no GIT_INDEX_FILE, no `git worktree add`.
"""

import subprocess
import sys
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from verify_ledger_env import hermetic_env  # noqa: E402

REPO_ROOT = INFRA_DIR.parents[1]
NO_HOOKS = ("-c", "core.hooksPath=/dev/null")
SCRATCH_IDENTITY = (
    "-c", "user.name=tree-under-test",
    "-c", "user.email=tree-under-test@automation.reify.invalid",
)


def scratch_git(repo, *args, stdin=None):
    """`git -C repo args` with hooks off, hermetic_env() and the scratch identity; stdout bytes."""
    return subprocess.run(
        ["git", "-C", str(repo), *NO_HOOKS, *SCRATCH_IDENTITY, *args],
        input=stdin,
        env=hermetic_env(),
        capture_output=True,
        check=True,
    ).stdout


def shared_clone(dest: Path, source: Path = REPO_ROOT) -> Path:
    """Clone `source`'s tree under test into `dest` and return `dest`."""
    head = scratch_git(source, "rev-parse", "HEAD").decode().strip()
    subprocess.run(
        ["git", *NO_HOOKS, "clone", "--shared", "--no-checkout", "-q", str(source), str(dest)],
        env=hermetic_env(),
        capture_output=True,
        check=True,
    )
    scratch_git(dest, "checkout", "-q", "--detach", head)
    main = subprocess.run(
        ["git", "-C", str(source), "rev-parse", "--verify", "-q", "refs/heads/main"],
        env=hermetic_env(),
        capture_output=True,
    )
    if main.returncode == 0:
        scratch_git(dest, "branch", "-f", "main", main.stdout.decode().strip())
    diff = scratch_git(source, "diff", "--binary", "HEAD")
    if diff:
        scratch_git(dest, "apply", "--index", stdin=diff)
        scratch_git(dest, "commit", "-q", "-m", "tree under test: uncommitted diff")
    return dest
