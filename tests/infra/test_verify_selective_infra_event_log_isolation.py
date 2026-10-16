#!/usr/bin/env python3
"""
C4 of docs/prds/v0_6/run-all-runner-infra-harness.md §4, applied to
scripts/verify.sh's task-scope selective-infra leaf (#8919): no infra member
ever sees the parent's REIFY_VERIFY_EVENT_LOG. The leaf is derived from the
real --print-plan and executed against a probe member in a fixture tree.
"""

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_ledger_env import hermetic_env  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = Path(__file__).resolve().parents[2]
LEAF_GLOB = "for _vt in tests/infra/test_verify_*.sh"
PROBE_NAME = "test_verify_zz_event_log_probe.sh"
PROBE_BODY = """\
printf '%s' "${REIFY_VERIFY_EVENT_LOG-<unset>}" > "$EVLOG_PROBE_DIR/event_log"
printf '%s' "${EVLOG_PROBE_MARK-<unset>}" > "$EVLOG_PROBE_DIR/mark"
if [ -n "${REIFY_VERIFY_EVENT_LOG:-}" ]; then
    printf '%s\\n' '{"event":"probe"}' >> "$REIFY_VERIFY_EVENT_LOG"
fi
exit "${EVLOG_PROBE_EXIT:-0}"
"""


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


class SelectiveInfraEventLogIsolationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = Path(tempfile.mkdtemp(prefix="vsi-evlog-"))
        cls.addClassCleanup(shutil.rmtree, cls.fixture, True)
        build_task_branch_fixture(cls.fixture)
        cls.leaves = selective_leaves(cls.fixture)
        probe = cls.fixture / "tests" / "infra" / PROBE_NAME
        probe.parent.mkdir(parents=True, exist_ok=True)
        probe.write_text(PROBE_BODY, encoding="utf-8")

    def single_leaf(self):
        self.assertEqual(
            len(self.leaves), 1, f"expected one '{LEAF_GLOB}' leaf, got {self.leaves}"
        )
        return self.leaves[0]

    def run_leaf(self, *, with_channel, **overrides):
        """Run the leaf; returns (proc, probe_dir, channel_path)."""
        leaf = self.single_leaf()
        scratch = Path(tempfile.mkdtemp(prefix="vsi-evlog-run-"))
        self.addCleanup(shutil.rmtree, scratch, True)
        probe_dir = scratch / "probe"
        probe_dir.mkdir()
        channel = scratch / "channel.jsonl"
        env = hermetic_env(EVLOG_PROBE_DIR=str(probe_dir), **overrides)
        if with_channel:
            env["REIFY_VERIFY_EVENT_LOG"] = str(channel)
        else:
            env.pop("REIFY_VERIFY_EVENT_LOG", None)
        proc = subprocess.run(
            ["bash", "-c", leaf],
            cwd=str(self.fixture),
            env=env,
            capture_output=True,
            text=True,
            timeout=120,
        )
        return proc, probe_dir, channel

    def assert_rc(self, proc, expected):
        self.assertEqual(
            proc.returncode,
            expected,
            f"leaf rc {proc.returncode}\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}",
        )

    def probe_read(self, probe_dir, name):
        path = probe_dir / name
        self.assertTrue(path.exists(), f"probe member never ran: {path} absent")
        return path.read_text(encoding="utf-8")

    def test_plan_emits_selective_leaf_for_probe_glob(self):
        self.single_leaf()

    def test_member_loses_only_the_parent_event_log(self):
        proc, probe_dir, channel = self.run_leaf(with_channel=True, EVLOG_PROBE_MARK="kept")
        self.assert_rc(proc, 0)
        self.assertEqual(self.probe_read(probe_dir, "event_log"), "<unset>")
        self.assertFalse(channel.exists(), "a member wrote into the parent event channel")
        self.assertEqual(self.probe_read(probe_dir, "mark"), "kept")

    def test_member_exit_status_still_propagates(self):
        proc, _, _ = self.run_leaf(with_channel=True, EVLOG_PROBE_EXIT="3")
        self.assert_rc(proc, 3)

    def test_parent_without_channel_runs_member_unchanged(self):
        proc, probe_dir, _ = self.run_leaf(with_channel=False)
        self.assert_rc(proc, 0)
        self.assertEqual(self.probe_read(probe_dir, "event_log"), "<unset>")


if __name__ == "__main__":
    unittest.main()
