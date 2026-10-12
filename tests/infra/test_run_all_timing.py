#!/usr/bin/env python3
"""
test_run_all_timing.py — run_all.sh's per-member and per-phase timing record.

EXECUTES the real tests/infra/run_all.sh on a tempdir fixture INFRA_DIR (the
recipe of test_run_all.sh Test 9: fixture classification manifest, private
pool lock, PSI disabled, host-infra exclusion removed, flaky ledger isolated)
and reads the JSONL file it writes to REIFY_RUN_ALL_TIMING_OUT. The fixture
has two passing pool members, one pool member that fails once and passes on
the Phase-2.5 serial retry, and one intra-run-serial member.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_run_all_timing.sh. Schema:
docs/notes/verify-instrumentation-ledger.md.
"""

import json
import os
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
RUN_ALL = REPO_ROOT / "tests" / "infra" / "run_all.sh"

MANIFEST = """\
test_pool_a.sh pool
test_pool_b.sh pool
test_flaky.sh pool
test_serial_a.sh intra-run-serial
"""

PASSING_MEMBER = """#!/usr/bin/env bash
env > "$FIXTURE_STATE/$(basename "$0").env"
echo "member $(basename "$0") ok"
exit 0
"""

FLAKY_MEMBER = """#!/usr/bin/env bash
env > "$FIXTURE_STATE/$(basename "$0").env"
n=$(( $(cat "$FIXTURE_STATE/flaky-count" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$FIXTURE_STATE/flaky-count"
echo "member flaky attempt"
[ "$n" -ge 2 ]
"""

# Ambient run_all knobs the gate hands its pool members (this suite is one);
# each would change which path or members the fixture run takes.
_SCRUBBED_ENV = (
    "REIFY_RUN_ALL_EXCLUDE_HOST_INFRA",
    "REIFY_RUN_ALL_CONTENT_SKIP",
    "REIFY_RUN_ALL_SKIP_STATE",
    "REIFY_RUN_ALL_MEMBER_SUBSET",
    "REIFY_RUN_ALL_POOL_DISABLE",
    "REIFY_RUN_ALL_POOL_FORK_FAIL_MEMBER",
    "REIFY_RUN_ALL_TIMING_OUT",
)


def write_executable(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


class Fixture:
    def __init__(self, add_cleanup):
        self.root = Path(tempfile.mkdtemp(prefix="ra-fixture-"))
        add_cleanup(shutil.rmtree, self.root, True)
        self.infra = self.root / "infra"
        self.state = self.root / "state"
        self.infra.mkdir()
        self.state.mkdir()
        for name in ("test_pool_a.sh", "test_pool_b.sh", "test_serial_a.sh"):
            write_executable(self.infra / name, PASSING_MEMBER)
        write_executable(self.infra / "test_flaky.sh", FLAKY_MEMBER)
        self.manifest = self.root / "classification.manifest"
        self.manifest.write_text(MANIFEST)

    def run(self, timing_out=None):
        (self.state / "flaky-count").unlink(missing_ok=True)
        for dump in self.state.glob("*.env"):
            dump.unlink()
        env = {k: v for k, v in os.environ.items() if k not in _SCRUBBED_ENV}
        env.update(
            RUN_ALL_CLASSIFICATION_MANIFEST=str(self.manifest),
            REIFY_RUN_ALL_POOL_LOCK=str(self.root / "pool.lock"),
            REIFY_RUN_ALL_POOL_CONCURRENCY="2",
            REIFY_RUN_ALL_POOL_PSI_DISABLE="1",
            REIFY_RUN_ALL_FLAKY_LEDGER=str(self.root / "flaky-ledger.jsonl"),
            FIXTURE_STATE=str(self.state),
        )
        if timing_out is not None:
            env["REIFY_RUN_ALL_TIMING_OUT"] = str(timing_out)
        return subprocess.run(
            ["bash", str(RUN_ALL), str(self.infra)],
            env=env,
            capture_output=True,
            text=True,
            timeout=600,
        )

    def env_dumps(self):
        return {p.name: p.read_text() for p in self.state.glob("*.env")}


def read_timing(path):
    return [json.loads(line) for line in Path(path).read_text().splitlines()]


class RunAllTimingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.fixture = Fixture(cls.addClassCleanup)
        cls.untimed = cls.fixture.run()
        cls.untimed_tree = [str(p) for p in cls.fixture.root.rglob("*")]
        cls.timing_path = cls.fixture.root / "timing.jsonl"
        cls.timed = cls.fixture.run(cls.timing_path)
        cls.timed_env_dumps = cls.fixture.env_dumps()

    def rows(self):
        return read_timing(self.timing_path)

    def test_run_passes_with_flaky_retry(self):
        self.assertEqual(self.timed.returncode, 0, self.timed.stdout + self.timed.stderr)

    def test_one_pool_summary_with_ordered_phases(self):
        summaries = [r for r in self.rows() if r["kind"] == "summary"]
        self.assertEqual(len(summaries), 1)
        (summary,) = summaries
        self.assertEqual(summary["path"], "pool")
        phases = summary["phases"]
        for name in ("pool", "serial", "retry"):
            with self.subTest(phase=name):
                self.assertLessEqual(phases[name]["began_at"], phases[name]["ended_at"])
        self.assertLessEqual(phases["pool"]["ended_at"], phases["serial"]["began_at"])
        self.assertLessEqual(phases["serial"]["ended_at"], phases["retry"]["began_at"])
        self.assertEqual(
            summary["totals"], {"members": 4, "pool": 3, "serial": 1, "retried": 1}
        )

    def test_member_attempts(self):
        members = [r for r in self.rows() if r["kind"] == "member"]
        attempts = sorted((m["name"], m["phase"]) for m in members)
        self.assertEqual(
            attempts,
            [
                ("test_flaky.sh", "pool"),
                ("test_flaky.sh", "retry"),
                ("test_pool_a.sh", "pool"),
                ("test_pool_b.sh", "pool"),
                ("test_serial_a.sh", "serial"),
            ],
        )
        by_attempt = {(m["name"], m["phase"]): m for m in members}
        for name in ("test_pool_a.sh", "test_pool_b.sh", "test_flaky.sh"):
            with self.subTest(member=name):
                row = by_attempt[(name, "pool")]
                self.assertLessEqual(row["queued_at"], row["started_at"])
                self.assertLessEqual(row["started_at"], row["ended_at"])
        self.assertEqual(by_attempt[("test_pool_a.sh", "pool")]["rc"], 0)
        self.assertNotEqual(by_attempt[("test_flaky.sh", "pool")]["rc"], 0)
        self.assertEqual(by_attempt[("test_flaky.sh", "retry")]["rc"], 0)
        serial = by_attempt[("test_serial_a.sh", "serial")]
        self.assertLessEqual(serial["started_at"], serial["ended_at"])
        self.assertEqual(serial["rc"], 0)

    def test_members_never_inherit_the_timing_path(self):
        self.assertEqual(
            sorted(self.timed_env_dumps),
            ["test_flaky.sh.env", "test_pool_a.sh.env", "test_pool_b.sh.env", "test_serial_a.sh.env"],
        )
        for name, dump in self.timed_env_dumps.items():
            with self.subTest(member=name):
                self.assertNotIn("REIFY_RUN_ALL_TIMING_OUT", dump)

    def test_recorder_is_silent(self):
        self.assertEqual(self.untimed.returncode, self.timed.returncode)
        self.assertEqual(self.untimed.stdout, self.timed.stdout)
        self.assertNotIn("timing", self.timed.stderr.lower())

    def test_no_timing_file_without_the_variable(self):
        self.assertEqual([p for p in self.untimed_tree if "timing" in Path(p).name], [])

    def test_unwritable_path_keeps_exit_code(self):
        blocker = self.fixture.root / "not-a-dir"
        blocker.write_text("")
        run = self.fixture.run(blocker / "timing.jsonl")
        self.assertEqual(run.returncode, self.timed.returncode)
        self.assertEqual(run.stdout, self.timed.stdout)


if __name__ == "__main__":
    unittest.main()
