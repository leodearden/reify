"""Builds a task-branch fixture whose verify.sh --print-plan emits the selective-infra leaf, and extracts that leaf."""

import shutil
import subprocess
from pathlib import Path

from verify_ledger_env import hermetic_env

REPO_ROOT = Path(__file__).resolve().parents[2]
LEAF_GLOB = "for _vt in tests/infra/test_verify_*.sh"


def git(cwd, *args):
    subprocess.run(
        ["git", "-c", "user.email=t@example.invalid", "-c", "user.name=t", *args],
        cwd=str(cwd),
        env=hermetic_env(),
        check=True,
        capture_output=True,
        timeout=120,
    )


def build_task_branch_fixture(fixture):
    shutil.copytree(REPO_ROOT / "scripts", fixture / "scripts", symlinks=True)
    shutil.copytree(REPO_ROOT / ".config", fixture / ".config", symlinks=True)
    git(fixture, "init", "-q", "-b", "main")
    git(fixture, "add", "-A")
    git(fixture, "commit", "-qm", "base")
    git(fixture, "checkout", "-qb", "task")
    with open(fixture / "scripts" / "verify.sh", "a", encoding="utf-8") as fh:
        fh.write("# fixture edit\n")
    git(fixture, "commit", "-qam", "edit verify.sh")


def selective_leaves(fixture):
    proc = subprocess.run(
        ["bash", "scripts/verify.sh", "test", "--scope", "branch", "--print-plan"],
        cwd=str(fixture),
        env=hermetic_env(DF_VERIFY_ROLE="task", REIFY_RELEASE_DELTA_SKIP="0"),
        capture_output=True,
        text=True,
        timeout=300,
    )
    if proc.returncode != 0:
        raise AssertionError(
            f"verify.sh --print-plan exited {proc.returncode}: {proc.stderr}"
        )
    return [
        line
        for line in proc.stdout.splitlines()
        if line and not line.startswith("#") and LEAF_GLOB in line
    ]
