#!/usr/bin/env python3
"""
scripts/verify.sh's task-scope selective-infra leaf names a member that exits
non-zero, decodes a signal death, and still propagates the member's status
(task 10333 / esc-9637-1). The leaf is derived from the real --print-plan and
executed against a probe member in a fixture tree.
"""

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_ledger_env import hermetic_env  # noqa: E402  (imported after sys.path manipulation)
from selective_infra_leaf_fixture import (  # noqa: E402  (imported after sys.path manipulation)
    LEAF_GLOB,
    build_task_branch_fixture,
    selective_leaves,
)

PROBE_NAME = "test_verify_zz_exit_probe.sh"
PROBE_BODY = """\
case "${EXIT_PROBE_MODE:-pass}" in
    pass) exit 0 ;;
    status) exit "$EXIT_PROBE_STATUS" ;;
    signal) kill -TERM $$ ;;
esac
"""
RECORD_MARK = "verify.sh: infra member "
RECORD_PREFIX = f"{RECORD_MARK}tests/infra/{PROBE_NAME} exited "
# PRD G6: a plan line run as `bash -c` with no options yields its intended
# status. The strict set is the harshest context the line can meet: a test's
# `bash -e -c`, or a paste into a `set -euo pipefail` shell.
SHELL_OPTION_SETS = ((), ("-e", "-u", "-o", "pipefail"))


class SelectiveInfraMemberExitRecordTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = Path(tempfile.mkdtemp(prefix="vsi-exitrec-"))
        cls.addClassCleanup(shutil.rmtree, cls.fixture, True)
        build_task_branch_fixture(cls.fixture)
        leaves = selective_leaves(cls.fixture)
        if len(leaves) != 1:
            raise AssertionError(f"expected one '{LEAF_GLOB}' leaf, got {leaves}")
        cls.leaf = leaves[0]
        probe = cls.fixture / "tests" / "infra" / PROBE_NAME
        probe.parent.mkdir(parents=True, exist_ok=True)
        probe.write_text(PROBE_BODY, encoding="utf-8")

    def run_leaf(self, opts, *, stderr=subprocess.PIPE, **probe_env):
        return subprocess.run(
            ["bash", *opts, "-c", self.leaf],
            cwd=str(self.fixture),
            env=hermetic_env(**probe_env),
            stdout=subprocess.PIPE,
            stderr=stderr,
            text=True,
            timeout=120,
        )

    def assert_rc(self, proc, expected):
        self.assertEqual(
            proc.returncode,
            expected,
            f"leaf rc {proc.returncode}\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}",
        )

    @staticmethod
    def records(proc):
        return [line for line in proc.stderr.splitlines() if line.startswith(RECORD_MARK)]

    def test_nonzero_exit_is_named(self):
        for opts in SHELL_OPTION_SETS:
            with self.subTest(opts=opts):
                proc = self.run_leaf(opts, EXIT_PROBE_MODE="status", EXIT_PROBE_STATUS="3")
                self.assert_rc(proc, 3)
                self.assertEqual(self.records(proc), [RECORD_PREFIX + "rc=3"])

    def test_signal_death_is_decoded(self):
        for opts in SHELL_OPTION_SETS:
            with self.subTest(opts=opts):
                proc = self.run_leaf(opts, EXIT_PROBE_MODE="signal")
                self.assert_rc(proc, 143)
                self.assertEqual(self.records(proc), [RECORD_PREFIX + "rc=143 (signal TERM)"])

    def test_passing_member_writes_no_record(self):
        for opts in SHELL_OPTION_SETS:
            with self.subTest(opts=opts):
                proc = self.run_leaf(opts, EXIT_PROBE_MODE="pass")
                self.assert_rc(proc, 0)
                self.assertEqual(self.records(proc), [])

    def test_unwritable_stderr_keeps_the_member_status(self):
        for opts in SHELL_OPTION_SETS:
            with self.subTest(opts=opts), open("/dev/full", "w") as full:
                proc = self.run_leaf(
                    opts, stderr=full, EXIT_PROBE_MODE="status", EXIT_PROBE_STATUS="3"
                )
                self.assert_rc(proc, 3)


if __name__ == "__main__":
    unittest.main()
