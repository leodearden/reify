"""releasescope — the release pass's load-time facts: the release-sensitive crates release-sensitive-crates.txt declares, and whether the merge's delta skips the pass (PRD D15).

bash computes both at load for every action, before it knows whether the
plan holds a release pass. scripts/release-scope-lib.sh answers both; neither
is reimplemented here.
"""

from collections.abc import Mapping
from pathlib import Path

from reify_verify import bashcall, gitcall, knobs, scope
from reify_verify.bashcall import BashCall
from reify_verify.plan import Refused
from reify_verify.policy import RolePolicy

RELEASE_DECLARED_SET = BashCall("scripts/release-scope-lib.sh", "release_declared_set")
RELEASE_DELTA_REQUIRES_PASS = BashCall("scripts/release-scope-lib.sh", "release_delta_requires_pass")


def release_declared(repo_root: Path, planning: Mapping[str, str]) -> tuple[str, ...] | Refused:
    """The release-sensitive crates release-sensitive-crates.txt declares.

    bash assigns the list under `set -e`, so a failing call ends the run with
    the lib's own stderr and status.
    """
    result = bashcall.ask(RELEASE_DECLARED_SET, (), repo_root, planning)
    if result.returncode != 0:
        return Refused(result.stderr_lines, result.returncode)
    return result.lines


def decide_release_delta(
    role_policy: RolePolicy, repo_root: Path, planning: Mapping[str, str]
) -> tuple[bool, tuple[str, ...]]:
    """Whether the run skips its release pass, and the predicate's stderr lines.

    Only an eligible role with REIFY_RELEASE_DELTA_SKIP=1 asks. The caller's
    affected-crate override is the predicate's whole input; without one, the
    merge's changed files are, and a delta verify cannot derive keeps the pass
    without asking. A non-zero answer skips.
    """
    if not (role_policy.release_delta_skip_eligible and knobs.resolve(knobs.RELEASE_DELTA_SKIP, planning).value):
        return False, ()
    overridden = knobs.resolve(knobs.AFFECTED_CRATES_OVERRIDE, planning).value
    changed = () if overridden else _merge_delta(repo_root, planning)
    if changed is None:
        return False, ()
    answer = bashcall.ask(RELEASE_DELTA_REQUIRES_PASS, changed, repo_root, planning)
    return answer.returncode != 0, answer.stderr_lines


def _merge_delta(repo_root: Path, planning: Mapping[str, str]) -> tuple[str, ...] | None:
    """The files the merge changes, or None when that cannot be derived.

    A merge in progress diffs HEAD against MERGE_HEAD; a merge commit at HEAD
    diffs against its first parent; anything else, or any git failure, is
    underivable. git's stderr is dropped, as bash's `2>/dev/null` drops it.
    """
    if scope.merge_in_progress(repo_root, planning):
        diff = gitcall.run(repo_root, planning, "diff", "--name-only", "HEAD", "MERGE_HEAD")
    else:
        parents = gitcall.run(repo_root, planning, "rev-list", "--parents", "-n1", "HEAD")
        if parents.returncode != 0 or len(parents.stdout.split()) < 3:
            return None
        diff = gitcall.run(repo_root, planning, "diff", "--name-only", "HEAD^1", "HEAD")
    return diff.lines if diff.returncode == 0 else None
