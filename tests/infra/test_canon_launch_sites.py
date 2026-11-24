#!/usr/bin/env python3
"""
test_canon_launch_sites.py — stdlib unittest: every reify launch site runs its
command through the tree's scripts/canon_run.py (task 10230; dark-factory
plans/canonical-build-path-shared-cache-prd.md C12, first bullet). The sites:
hooks/project-checks (reached from hooks/pre-commit), hooks/pre-merge-commit,
and scripts/seed-warm-base-initial.sh's cold build.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_canon_launch_sites.sh.

ROUTING SIGNAL: a build directory stamped for a root other than CANON makes
canon_run.py refuse (`canon_run: build directory ...`, exit 126) before it
reads the host config or tries a namespace, so on every host only a command
routed through the primitive produces it. The real-path cases pin the
untouched pass-through. CommitAtCanon adds the PRD's ledger-lane signal where
bwrap can enter a mount namespace, and skips with its named reason elsewhere.

FIXTURE: a canon_run_fixture tree, committed on main, carrying COPIES of the
real hooks and a recording scripts/verify.sh stub that logs its argv, cwd and
environment to a file outside the tree.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = fx.REPO_ROOT
HOOKS = ("pre-commit", "project-checks", "pre-merge-commit", "main-gate-lib.sh")
MAIN_GATE_MARK = REPO_ROOT / "scripts" / "main-gate-mark.sh"
SEED_WARM_BASE_INITIAL = REPO_ROOT / "scripts" / "seed-warm-base-initial.sh"
STAMP_NAME = fx.load_canon_run().STAMP_NAME
EXIT_REFUSED = fx.load_canon_run().EXIT_REFUSED
FOREIGN_ROOT = "/elsewhere/ws"
REFUSAL_PREFIX = "canon_run: build directory"
RECORDED_KEYS = ("PWD", "REIFY_VERIFY_LEDGER_LANE_DIR", "REIFY_CANON_ACTIVE", "DF_VERIFY_ROLE")

VERIFY_STUB = f"""#!{fx.PYTHON} -I
import json, os, sys
record = {{"argv": sys.argv, "getcwd": os.getcwd()}}
record.update({{key: os.environ.get(key) for key in {RECORDED_KEYS!r}}})
with open(os.environ["VERIFY_STUB_LOG"], "a") as log:
    log.write(json.dumps(record) + "\\n")
"""

PASSTHROUGH_STUB = '#!/bin/sh\nexit 0\n'


def _diag(completed):
    return (f"\n--- exit {completed.returncode} ---\n--- stdout ---\n"
            f"{completed.stdout}\n--- stderr ---\n{completed.stderr}")


def _write_executable(path, body):
    path.write_text(body)
    path.chmod(0o755)


class LaunchSiteFixture(unittest.TestCase):
    """A committed tree on main with the real hooks and a recording verify.sh."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-launch-sites-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name).resolve()
        self.stub_log = self.tmp / "verify-stub.jsonl"
        empty_global = self.tmp / "gitconfig-global"
        empty_global.write_text("")
        self.env = fx.scratch_env(self.tmp, VERIFY_STUB_LOG=str(self.stub_log))
        self.env.update({
            "GIT_CONFIG_GLOBAL": str(empty_global),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CEILING_DIRECTORIES": str(self.tmp),
            "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.test",
            "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.test",
        })

        self.tree = Path(fx.make_tree(self.tmp, "tree"))
        hooks = self.tree / "hooks"
        hooks.mkdir()
        for name in HOOKS:
            shutil.copy2(REPO_ROOT / "hooks" / name, hooks / name)
            (hooks / name).chmod(0o755)
        if MAIN_GATE_MARK.exists():
            shutil.copy2(MAIN_GATE_MARK, self.tree / "scripts" / MAIN_GATE_MARK.name)
        _write_executable(self.tree / "scripts" / "verify.sh", VERIFY_STUB)
        (self.tree / ".gitignore").write_text("target/\n")
        self.git("init", "-q", "-b", "main")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "fixture tree")

    def git(self, *args):
        completed = subprocess.run(["git", "-C", str(self.tree), *args], env=self.env,
                                   capture_output=True, text=True)
        if completed.returncode != 0:
            self.fail(f"git {' '.join(args)} failed" + _diag(completed))
        return completed.stdout.strip()

    def stamp_target(self, root):
        target = self.tree / "target"
        target.mkdir(exist_ok=True)
        (target / STAMP_NAME).write_text(f"{root}\n")

    def commit_on_main(self):
        (self.tree / "new.txt").write_text("new\n")
        self.git("add", "--", "new.txt")
        return subprocess.run(
            ["git", "-c", "core.hooksPath=hooks", "commit", "-q", "-m", "fixture: new file"],
            cwd=self.tree, env=self.env, capture_output=True, text=True)

    def stub_calls(self):
        if not self.stub_log.exists():
            return []
        return [json.loads(line) for line in self.stub_log.read_text().splitlines()]

    def assert_refused(self, completed):
        refusals = [line for line in completed.stderr.splitlines()
                    if line.startswith(REFUSAL_PREFIX)]
        self.assertEqual(len(refusals), 1, _diag(completed))
        self.assertIn(FOREIGN_ROOT, refusals[0])
        self.assertEqual(self.stub_calls(), [], "verify.sh ran despite the refusal")


class ProjectChecksRouting(LaunchSiteFixture):
    """hooks/pre-commit → hooks/project-checks → canon_run.py exec -- verify.sh."""

    def test_a_target_stamped_for_another_root_blocks_the_commit(self):
        self.stamp_target(FOREIGN_ROOT)
        head = self.git("rev-parse", "HEAD")
        completed = self.commit_on_main()
        self.assertNotEqual(completed.returncode, 0, _diag(completed))
        self.assert_refused(completed)
        self.assertEqual(self.git("rev-parse", "HEAD"), head)

    def test_with_no_build_directory_verify_runs_untouched(self):
        completed = self.commit_on_main()
        self.assertEqual(completed.returncode, 0, _diag(completed))
        calls = self.stub_calls()
        self.assertEqual(len(calls), 1, _diag(completed))
        self.assertEqual(calls[0]["argv"],
                         [str(self.tree / "scripts" / "verify.sh"), "all", "--profile", "debug",
                          "--scope", "staged", "--include-infra"])
        self.assertEqual(calls[0]["getcwd"], str(self.tree))
        self.assertIsNone(calls[0]["REIFY_CANON_ACTIVE"])


class PreMergeCommitRouting(LaunchSiteFixture):
    """hooks/pre-merge-commit → canon_run.py exec -- verify.sh, as the merge role."""

    def run_hook(self):
        return subprocess.run([str(self.tree / "hooks" / "pre-merge-commit")],
                              cwd=self.tree, env=self.env, capture_output=True, text=True)

    def test_a_target_stamped_for_another_root_refuses_the_merge(self):
        self.stamp_target(FOREIGN_ROOT)
        completed = self.run_hook()
        self.assertEqual(completed.returncode, EXIT_REFUSED, _diag(completed))
        self.assert_refused(completed)

    def test_with_no_build_directory_verify_runs_untouched_as_the_merge_role(self):
        completed = self.run_hook()
        self.assertEqual(completed.returncode, 0, _diag(completed))
        calls = self.stub_calls()
        self.assertEqual(len(calls), 1, _diag(completed))
        self.assertEqual(calls[0]["argv"][1:], ["all", "--profile", "both", "--scope", "all"])
        self.assertEqual(calls[0]["DF_VERIFY_ROLE"], "merge")


class CommitAtCanon(LaunchSiteFixture):
    """A commit in a tree stamped for CANON runs verify.sh at CANON (PRD ledger-lane signal)."""

    def setUp(self):
        reason = fx.namespace_unavailable_reason()
        if reason:
            self.skipTest(reason)
        super().setUp()
        self.canon = self.tmp / "canon"
        self.canon.mkdir()
        self.env["REIFY_CANON_ROOT"] = str(self.canon)
        fx.write_host_config(self.env["REIFY_CANON_HOST_CONFIG"], enabled=True,
                             policy="incremental")
        self.stamp_target(self.canon)

    def test_verify_runs_at_canon_and_records_the_real_lane(self):
        completed = self.commit_on_main()
        self.assertEqual(completed.returncode, 0, _diag(completed))
        calls = self.stub_calls()
        self.assertEqual(len(calls), 1, _diag(completed))
        self.assertEqual(calls[0]["getcwd"], str(self.canon))
        self.assertEqual(calls[0]["argv"][0], str(self.canon / "scripts" / "verify.sh"))
        self.assertEqual(calls[0]["REIFY_VERIFY_LEDGER_LANE_DIR"], str(self.tree))


class SeedWarmBaseInitialRouting(LaunchSiteFixture):
    """seed-warm-base-initial.sh's cold build → canon_run.py exec -- bash -c <build-cmd>."""

    def setUp(self):
        super().setUp()
        stub_bin = self.tmp / "stub-bin"
        stub_bin.mkdir()
        _write_executable(stub_bin / "mountpoint", PASSTHROUGH_STUB)
        # No reflink on a tmp fs: drop the flag so the pre-check's probe copy works.
        _write_executable(
            stub_bin / "cp",
            "#!/bin/sh\n"
            'for arg; do shift; [ "$arg" = "--reflink=always" ] || set -- "$@" "$arg"; done\n'
            f'exec {shutil.which("cp")} "$@"\n')
        self.env["PATH"] = f"{stub_bin}{os.pathsep}{self.env['PATH']}"
        self.mount = self.tmp / "mount"
        self.mount.mkdir()
        self.built = self.tmp / "built"

    def test_a_merge_verify_target_stamped_for_another_root_refuses_the_build(self):
        self.stamp_target(FOREIGN_ROOT)
        completed = subprocess.run(
            ["bash", str(SEED_WARM_BASE_INITIAL), "--mount", str(self.mount),
             "--merge-verify", str(self.tree), "--build-cmd", f"touch {self.built}"],
            cwd=self.tmp, env=self.env, capture_output=True, text=True)
        self.assertNotEqual(completed.returncode, 0, _diag(completed))
        self.assertFalse(self.built.exists(), "the build ran despite the refusal" + _diag(completed))
        self.assertTrue(any(line.startswith(REFUSAL_PREFIX)
                            for line in completed.stderr.splitlines()), _diag(completed))


if __name__ == "__main__":
    unittest.main()
