#!/usr/bin/env python3
"""
test_verify_events.py — the REIFY_VERIFY_EVENT_LOG verify event channel
(PRD docs/prds/v0_6/run-all-runner-infra-harness.md §4, task 8685).

ReaderTests drive tests/infra/verify_events.py, the stdlib reader and CLI,
over channel files written by hand.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_events.sh. Schema: tests/infra/README.md
"Verify event channel".
"""

import json
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
import verify_events  # noqa: E402  (imported after sys.path manipulation)
from verify_ledger_env import hermetic_env  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
READER = INFRA_DIR / "verify_events.py"


def run(argv, env=None, cwd=None, timeout=600):
    """The one spawn site: every child process this suite starts goes here."""
    return subprocess.run(
        argv,
        env=hermetic_env() if env is None else env,
        cwd=cwd,
        capture_output=True,
        text=True,
        errors="surrogateescape",
        timeout=timeout,
    )


def reader_cli(*args, timeout=600):
    return run([sys.executable, "-I", str(READER), *map(str, args)], timeout=timeout)


def record(event, **fields):
    return json.dumps({"v": 1, "ts_ns": 1, "pid": 1, "event": event, **fields})


def run_start(discovered, branch="pool"):
    return record("run_start", run_id="r", scope="all", branch=branch, discovered=discovered)


def member_finish(member, rc, verdict, attempt=1, detail=()):
    return record(
        "member_finish",
        member=member,
        attempt=attempt,
        rc=rc,
        verdict=verdict,
        duration_ms=5,
        fail_detail=list(detail),
        fail_detail_omitted=0,
    )


def slot_timeout(disposition):
    return record(
        "slot_timeout",
        reason="probe",
        slots=1,
        waited=1,
        disposition=disposition,
        lock="/tmp/l",
        emitter="unknown",
    )


def run_finish(discovered, failed=(), flaky=(), skipped=(), interrupted=False):
    return record(
        "run_finish",
        discovered=discovered,
        failures=len(failed),
        failed=list(failed),
        flaky=list(flaky),
        skipped=list(skipped),
        interrupted=interrupted,
        names_omitted=0,
    )


ONE_RUN = [
    run_start(3),
    record("member_start", member="test_a.sh", attempt=1, mode="capture"),
    member_finish("test_a.sh", 1, "FAIL", detail=["  FAIL: x"]),
    slot_timeout("fatal"),
    run_finish(3, failed=["test_a.sh"], skipped=["test_c.sh"]),
]

TWO_RUNS = [
    run_start(3),
    member_finish("test_a.sh", 1, "FAIL"),
    member_finish("test_b.sh", 0, "FLAKY_PASS", attempt=2),
    slot_timeout("soft"),
    run_finish(3, failed=["test_a.sh"], flaky=["test_b.sh"], skipped=["test_c.sh"]),
    run_start(2, branch="serial"),
    slot_timeout("fatal"),
    slot_timeout("soft"),
    run_finish(2, failed=["test_d.sh", "test_e.sh"], skipped=["test_f.sh"], interrupted=True),
]


class ChannelFiles:
    """A per-test scratch dir holding hand-written channel files."""

    def __init__(self, test):
        self.dir = Path(tempfile.mkdtemp(prefix="verify-events-"))
        test.addCleanup(shutil.rmtree, self.dir, True)

    def write(self, name, lines, tail=b""):
        path = self.dir / name
        body = b"".join(
            (line if isinstance(line, bytes) else line.encode()) + b"\n" for line in lines
        )
        path.write_bytes(body + tail)
        return path


class ReaderTests(unittest.TestCase):
    def setUp(self):
        self.files = ChannelFiles(self)

    def test_read_keeps_v1_objects_and_counts_every_other_line(self):
        path = self.files.write(
            "mixed.jsonl",
            ONE_RUN[:2]
            + ["this is not json", "[1, 2]", json.dumps({"v": 2, "event": "run_start"})]
            + ONE_RUN[2:],
            tail=b'{"v":1,"ts_ns":1,"pid":1,"event":"run_fin',
        )
        events = verify_events.read_events(path)
        self.assertEqual(
            [r["event"] for r in events.records],
            ["run_start", "member_start", "member_finish", "slot_timeout", "run_finish"],
        )
        self.assertEqual(events.skipped_lines, 4)
        self.assertTrue(events.channel_present)

    def test_records_without_run_start_are_channel_absent(self):
        path = self.files.write("no-start.jsonl", ONE_RUN[1:])
        events = verify_events.read_events(path)
        self.assertEqual(len(events.records), 4)
        self.assertFalse(events.channel_present)

    def test_missing_file_is_an_empty_absent_channel(self):
        events = verify_events.read_events(self.files.dir / "never-written.jsonl")
        self.assertEqual(events.records, ())
        self.assertEqual(events.skipped_lines, 0)
        self.assertFalse(events.channel_present)

    def test_non_utf8_bytes_round_trip_through_surrogateescape(self):
        raw_lock = b"/tmp/lock-\xff\xfe-base"
        line = (
            b'{"v":1,"ts_ns":1,"pid":1,"event":"slot_timeout","reason":"r","slots":1,'
            b'"waited":0,"disposition":"fatal","lock":"' + raw_lock + b'","emitter":"x"}'
        )
        path = self.files.write("bytes.jsonl", [run_start(0), line])
        events = verify_events.read_events(path)
        self.assertEqual(events.skipped_lines, 0)
        (timeout,) = verify_events.select(events, event="slot_timeout")
        self.assertEqual(timeout["lock"].encode("utf-8", "surrogateescape"), raw_lock)

    def test_summarize_aggregates_every_run_in_file_order(self):
        path = self.files.write("two-runs.jsonl", TWO_RUNS + ["garbage"])
        summary = verify_events.summarize(verify_events.read_events(path))
        self.assertEqual(
            summary,
            {
                "channel": "present",
                "runs": 2,
                "discovered": 5,
                "failures": 3,
                "failed": ["test_a.sh", "test_d.sh", "test_e.sh"],
                "flaky": ["test_b.sh"],
                "skipped": ["test_c.sh", "test_f.sh"],
                "interrupted": True,
                "slot_timeout_fatal": 1,
                "slot_timeout_soft": 2,
                "skipped_lines": 1,
            },
        )

    def test_summarize_an_absent_channel(self):
        summary = verify_events.summarize(
            verify_events.read_events(self.files.dir / "never-written.jsonl")
        )
        self.assertEqual(summary["channel"], "absent")
        self.assertEqual(summary["runs"], 0)
        self.assertEqual(summary["failed"], [])
        self.assertFalse(summary["interrupted"])

    def test_select_compares_non_strings_by_their_json_spelling(self):
        events = verify_events.read_events(self.files.write("two-runs.jsonl", TWO_RUNS))
        cases = [
            ({"event": "member_finish", "where": {"member": "test_a.sh", "rc": "1"}}, 1),
            ({"event": "member_finish", "where": {"rc": "0"}}, 1),
            ({"event": "member_finish", "where": {"member": '"test_a.sh"'}}, 0),
            ({"event": "run_finish", "where": {"interrupted": "true"}}, 1),
            ({"event": "run_finish", "where": {"interrupted": "True"}}, 0),
            ({"event": "run_finish", "where": {"failed": '["test_a.sh"]'}}, 1),
            ({"event": "slot_timeout"}, 3),
            ({"where": {"disposition": "soft"}}, 2),
            ({"where": {"no_such_field": "x"}}, 0),
            ({}, len(TWO_RUNS)),
        ]
        for kwargs, expected in cases:
            with self.subTest(**kwargs):
                self.assertEqual(len(verify_events.select(events, **kwargs)), expected)

    def test_wait_for_is_satisfied_at_once_by_existing_records(self):
        path = self.files.write("two-runs.jsonl", TWO_RUNS)
        self.assertTrue(
            verify_events.wait_for(path, event="slot_timeout", count=3, deadline_s=30)
        )

    def test_wait_for_gives_up_at_the_deadline(self):
        path = self.files.write("one-run.jsonl", ONE_RUN)
        self.assertFalse(
            verify_events.wait_for(path, event="run_start", count=2, deadline_s=0.3, poll_s=0.05)
        )

    def test_wait_for_sees_a_record_appended_while_waiting(self):
        path = self.files.write("growing.jsonl", ONE_RUN[:1])

        def append():
            with path.open("a") as channel:
                channel.write(ONE_RUN[-1] + "\n")

        timer = threading.Timer(0.2, append)
        timer.start()
        self.addCleanup(timer.cancel)
        self.assertTrue(verify_events.wait_for(path, event="run_finish", deadline_s=30))


class ReaderCliTests(unittest.TestCase):
    def setUp(self):
        self.files = ChannelFiles(self)
        self.path = self.files.write("two-runs.jsonl", TWO_RUNS + ["garbage"])

    def test_summary_prints_key_value_scalars(self):
        result = reader_cli("summary", self.path)
        self.assertEqual(result.returncode, 0, result.stderr)
        lines = result.stdout.splitlines()
        for expected in (
            "channel=present",
            "runs=2",
            "discovered=5",
            "failures=3",
            "failed=test_a.sh test_d.sh test_e.sh",
            "flaky=test_b.sh",
            "skipped=test_c.sh test_f.sh",
            "interrupted=true",
            "slot_timeout_fatal=1",
            "slot_timeout_soft=2",
            "skipped_lines=1",
        ):
            with self.subTest(line=expected):
                self.assertIn(expected, lines)

    def test_summary_of_a_missing_file_reports_an_absent_channel(self):
        result = reader_cli("summary", self.files.dir / "never-written.jsonl")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("channel=absent", result.stdout.splitlines())
        self.assertIn("failed=", result.stdout.splitlines())

    def test_query_count(self):
        result = reader_cli(
            "query", self.path, "--event", "member_finish", "--where", "member=test_a.sh", "--count"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "1\n")

    def test_query_prints_one_json_object_per_matching_record(self):
        result = reader_cli("query", self.path, "--event", "member_finish")
        self.assertEqual(result.returncode, 0, result.stderr)
        records = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual([r["member"] for r in records], ["test_a.sh", "test_b.sh"])

    def test_query_with_several_where_clauses(self):
        result = reader_cli(
            "query", self.path, "--where", "verdict=FAIL", "--where", "rc=1", "--count"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "1\n")

    def test_wait_exits_0_when_satisfied(self):
        result = reader_cli("wait", self.path, "--event", "run_start", "--timeout", "5")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_wait_exits_1_with_a_diagnostic_when_unsatisfied(self):
        result = reader_cli(
            "wait", self.path, "--event", "run_start", "--count", "3", "--timeout", "0.3"
        )
        self.assertEqual(result.returncode, 1)
        self.assertIn("verify_events: wait: 2/3 matching records", result.stderr)

    def test_usage_errors_exit_2(self):
        for argv in (("frobnicate", self.path), ("query", self.path, "--where", "no-equals")):
            with self.subTest(argv=argv):
                self.assertEqual(reader_cli(*argv).returncode, 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
