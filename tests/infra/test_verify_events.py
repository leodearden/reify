#!/usr/bin/env python3
"""
test_verify_events.py — the REIFY_VERIFY_EVENT_LOG verify event channel
(PRD docs/prds/v0_6/run-all-runner-infra-harness.md §4, task 8685).

ReaderTests drive tests/infra/verify_events.py, the stdlib reader and CLI,
over channel files written by hand. EncoderTests drive the writer,
scripts/lib_event_log.sh, from bash and read its output back with that reader.
SlotTimeoutRecordTests force real finite-WAIT deadlines through
scripts/lib_slot_acquire.sh and every wrapper that calls it (PRD §9 R-1).
RunAllChannelTests EXECUTE the real tests/infra/run_all.sh on tempdir fixture
INFRA_DIRs (private manifest, pool lock, Lane-X lock, flaky ledger and TMPDIR)
in each of its four branches.

SELF-POLLUTION: a forced deadline prints a live slot-timeout sentinel on the
child's stderr, and this suite's own output is re-emitted into the merge-gate
log. Failure messages here therefore carry counts and field values only,
never captured stderr, and the sentinel prefix is assembled at runtime (SP).

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_verify_events.sh. Schema: tests/infra/README.md
"Verify event channel".
"""

import contextlib
import fcntl
import json
import os
import re
import shutil
import signal
import stat
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
SCRIPTS = REPO_ROOT / "scripts"
RUN_ALL = REPO_ROOT / "tests" / "infra" / "run_all.sh"
LIB = SCRIPTS / "lib_event_log.sh"
CHANNEL_VAR = "REIFY_VERIFY_EVENT_LOG"
COPY_VAR = "EVENT_LOG_COPY_TO"
LINE_MAX = 4000
DETAIL_ENTRY_MAX = 400


def run(argv, env=None, cwd=None, timeout=600):
    """The spawn site of every run-to-completion child this suite starts.

    RunAllFixture.start's Popen is the only other spawn site; both divert stderr.
    """
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


def channel_env(channel=None, **overrides):
    """hermetic_env() with the channel pointed at `channel`, or unset when None."""
    env = hermetic_env(**overrides)
    if channel is not None:
        env[CHANNEL_VAR] = str(channel)
    return env


def assert_channel_unset(test, env):
    """Keeps an "unset" case non-vacuous when the developer's shell exports the var."""
    test.assertNotIn(CHANNEL_VAR, env)


def lib_bash(snippet, *args, env, cwd=None, strict=False):
    """Run `snippet` in bash with lib_event_log.sh sourced; `args` are its $1.."""
    prologue = "set -euo pipefail; " if strict else ""
    return run(
        ["bash", "-c", prologue + 'source "$1"; shift; ' + snippet, "_", str(LIB), *args],
        env=env,
        cwd=cwd,
    )


def lib_bash_with_copy(snippet, copy, *args, env, cwd=None, strict=False):
    """lib_bash with the copy sink set to `copy` as a shell variable, never exported."""
    return lib_bash(
        f'{COPY_VAR}="$1"; shift; ' + snippet, str(copy), *args, env=env, cwd=cwd, strict=strict
    )


def emit(emitter, *args, env, cwd=None):
    return lib_bash('"$@"', f"event_log_emit_{emitter}", *map(str, args), env=env, cwd=cwd)


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
            + [
                "this is not json",
                "[1, 2]",
                json.dumps({"v": 2, "event": "run_start"}),
                json.dumps({"v": True, "event": "run_start"}),
                json.dumps({"v": 1.0, "event": "run_start"}),
            ]
            + ONE_RUN[2:],
            tail=b'{"v":1,"ts_ns":1,"pid":1,"event":"run_fin',
        )
        events = verify_events.read_events(path)
        self.assertEqual(
            [r["event"] for r in events.records],
            ["run_start", "member_start", "member_finish", "slot_timeout", "run_finish"],
        )
        self.assertEqual(events.skipped_lines, 6)
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


# One call per record type (PRD D3 positional signatures) and the fields the
# C2 table says it carries, beyond the v/ts_ns/pid/event head.
EMITTER_SHAPES = [
    (
        "run_start",
        ["run-1", "all", "pool", "3"],
        {"run_id": "run-1", "scope": "all", "branch": "pool", "discovered": 3},
    ),
    (
        "member_start",
        ["test_a.sh", "1", "capture"],
        {"member": "test_a.sh", "attempt": 1, "mode": "capture"},
    ),
    (
        "member_finish",
        ["test_a.sh", "2", "1", "FAIL", "17", "1000", "2000", "5000", "3", "  FAIL: x", "  FAIL: y"],
        {
            "member": "test_a.sh",
            "attempt": 2,
            "rc": 1,
            "verdict": "FAIL",
            "duration_ms": 17,
            "queued_ns": 1000,
            "started_ns": 2000,
            "ended_ns": 5000,
            "fail_detail": ["  FAIL: x", "  FAIL: y"],
            "fail_detail_omitted": 3,
        },
    ),
    (
        "phase_finish",
        ["pool", "1000", "5000"],
        {"phase": "pool", "started_ns": 1000, "ended_ns": 5000},
    ),
    (
        "slot_timeout",
        ["probe_starvation", "2", "5", "soft", "/tmp/a lock", "lib_x.sh"],
        {
            "reason": "probe_starvation",
            "slots": 2,
            "waited": 5,
            "disposition": "soft",
            "lock": "/tmp/a lock",
            "emitter": "lib_x.sh",
        },
    ),
    (
        "run_finish",
        ["5", "2", "false", "2", "a.sh", "b.sh", "1", "c.sh", "0"],
        {
            "discovered": 5,
            "failures": 2,
            "failed": ["a.sh", "b.sh"],
            "flaky": ["c.sh"],
            "skipped": [],
            "interrupted": False,
            "names_omitted": 0,
        },
    ),
]

ALL_EMITTERS_ONCE = (
    "event_log_emit_run_start r all pool 1; "
    "event_log_emit_member_start m 1 capture; "
    "event_log_emit_member_finish m 1 0 PASS 1 - 1 2 0; "
    "event_log_emit_phase_finish p 1 2; "
    "event_log_emit_slot_timeout r 1 1 fatal /l e; "
    "event_log_emit_run_finish 1 0 false 0 0 0"
)
ALL_EMITTER_NAMES = [
    "run_start", "member_start", "member_finish", "phase_finish", "slot_timeout", "run_finish",
]

AWKWARD = 'sp ace "quote" back\\slash\nnew\rline\ttab\x01ctl\x1fus été del\x7f'


def is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def strace_usable(scratch):
    if shutil.which("strace") is None:
        return "strace is not on PATH"
    probe = run(["strace", "-f", "-qq", "-o", str(scratch / "probe.strace"), "true"])
    if probe.returncode != 0:
        return f"strace cannot trace here (rc={probe.returncode})"
    return None


class EncoderTests(unittest.TestCase):
    def setUp(self):
        self.files = ChannelFiles(self)
        self.channel = self.files.dir / "channel.jsonl"
        self.env = channel_env(self.channel)

    def read(self):
        return verify_events.read_events(self.channel)

    def physical_lines(self):
        return self.channel.read_bytes().split(b"\n")[:-1]

    def test_each_emitter_writes_one_record_of_its_c2_shape(self):
        for emitter, args, fields in EMITTER_SHAPES:
            with self.subTest(emitter=emitter):
                self.channel.unlink(missing_ok=True)
                result = emit(emitter, *args, env=self.env)
                self.assertEqual((result.returncode, result.stderr), (0, ""))
                events = self.read()
                self.assertEqual((len(events.records), events.skipped_lines), (1, 0))
                (rec,) = events.records
                self.assertEqual(rec["v"], 1)
                self.assertEqual(rec["event"], emitter)
                self.assertTrue(is_int(rec["ts_ns"]) and rec["ts_ns"] > 0, rec["ts_ns"])
                self.assertTrue(is_int(rec["pid"]) and rec["pid"] > 0, rec["pid"])
                body = {k: v for k, v in rec.items() if k not in ("v", "ts_ns", "pid", "event")}
                self.assertEqual(body, fields)
                for field in fields:
                    if is_int(fields[field]):
                        self.assertTrue(is_int(body[field]), field)

    def test_every_string_field_round_trips_through_the_one_escaper(self):
        calls = [
            ("slot_timeout", ["r", "1", "0", "fatal", AWKWARD, "e"], lambda r: r["lock"]),
            ("member_start", [AWKWARD, "1", "stream"], lambda r: r["member"]),
            (
                "member_finish",
                ["m", "1", "1", "FAIL", "1", "-", "-", "-", "0", AWKWARD],
                lambda r: r["fail_detail"][0],
            ),
            ("phase_finish", [AWKWARD, "1", "2"], lambda r: r["phase"]),
            ("run_finish", ["1", "1", "false", "1", AWKWARD, "0", "0"], lambda r: r["failed"][0]),
        ]
        for emitter, args, field in calls:
            with self.subTest(emitter=emitter):
                self.channel.unlink(missing_ok=True)
                result = emit(emitter, *args, env=self.env)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(len(self.physical_lines()), 1)
                (rec,) = self.read().records
                self.assertEqual(field(rec), AWKWARD)

    def test_a_non_integer_in_an_int_position_encodes_null(self):
        calls = [
            ("run_start", ["r", "all", "pool", ""], {"discovered": None}),
            ("member_start", ["m", "two", "capture"], {"attempt": None}),
            (
                "member_finish",
                ["m", "0", "", "SKIP", "", "", "", "", "0"],
                {
                    "attempt": 0,
                    "rc": None,
                    "duration_ms": None,
                    "queued_ns": None,
                    "started_ns": None,
                    "ended_ns": None,
                    "fail_detail": [],
                },
            ),
            (
                "member_finish",
                ["m", "1", "abc", "FAIL", "1.5", "-", "1.5", "-", "x"],
                {
                    "rc": None,
                    "duration_ms": None,
                    "queued_ns": None,
                    "started_ns": None,
                    "ended_ns": None,
                    "fail_detail_omitted": None,
                },
            ),
            ("phase_finish", ["retry", "-", "1.5"], {"phase": "retry", "started_ns": None, "ended_ns": None}),
            ("slot_timeout", ["r", "12x", "-3", "fatal", "/l", "e"], {"slots": None, "waited": -3}),
            ("run_finish", ["007", "-0", "maybe", "0", "0", "0"], {"discovered": 7, "failures": 0, "interrupted": None}),
        ]
        for emitter, args, expected in calls:
            with self.subTest(emitter=emitter, args=args):
                self.channel.unlink(missing_ok=True)
                result = emit(emitter, *args, env=self.env)
                self.assertEqual(result.returncode, 0, result.stderr)
                events = self.read()
                self.assertEqual((len(events.records), events.skipped_lines), (1, 0))
                (rec,) = events.records
                self.assertEqual({k: rec[k] for k in expected}, expected)

    def test_member_finish_keeps_the_newest_detail_within_the_line_cap(self):
        detail = [f"{i:02d}" + "x" * 898 for i in range(30)]
        result = emit(
            "member_finish", "m", "1", "1", "FAIL", "9", "1", "2", "3", "4", *detail, env=self.env
        )
        self.assertEqual((result.returncode, result.stderr), (0, ""))
        (line,) = self.physical_lines()
        self.assertLessEqual(len(line) + 1, LINE_MAX)
        (rec,) = self.read().records
        kept = rec["fail_detail"]
        self.assertGreater(len(kept), 0)
        self.assertTrue(all(len(entry) <= DETAIL_ENTRY_MAX for entry in kept))
        truncated = [entry[:DETAIL_ENTRY_MAX] for entry in detail]
        self.assertEqual(kept, truncated[len(truncated) - len(kept):])
        self.assertEqual(rec["fail_detail_omitted"], 4 + len(detail) - len(kept))

    def test_a_byte_cut_under_lc_all_c_drops_a_split_character_whole(self):
        env = dict(self.env, LC_ALL="C")
        for char in ("é", "—", "\U0001f600"):
            width = len(char.encode())
            for prefix in range(DETAIL_ENTRY_MAX - width, DETAIL_ENTRY_MAX):
                with self.subTest(char=ascii(char), prefix=prefix):
                    self.channel.unlink(missing_ok=True)
                    entry = "x" * prefix + char + "tail"
                    result = emit(
                        "member_finish", "m", "1", "1", "FAIL", "1", "-", "-", "-", "0", entry, env=env
                    )
                    self.assertEqual((result.returncode, result.stderr), (0, ""))
                    (line,) = self.physical_lines()
                    rec = json.loads(line.decode("utf-8"))
                    fits = prefix + width <= DETAIL_ENTRY_MAX
                    self.assertEqual(rec["fail_detail"], ["x" * prefix + (char if fits else "")])

    def test_run_finish_fits_names_in_order_and_counts_the_rest(self):
        def names(prefix, count):
            return [f"test_{prefix}_{i:03d}".ljust(40, "z") for i in range(count)]

        # Per case: the input lists, and which list is cut (the lists before
        # it are kept whole, the lists after it are empty).
        cases = {
            "failed overflows": ((names("f", 300), names("k", 5), names("s", 5)), 0),
            "flaky overflows": ((names("f", 50), names("k", 100), names("s", 100)), 1),
        }
        for label, ((failed, flaky, skipped), cut) in cases.items():
            with self.subTest(label):
                self.channel.unlink(missing_ok=True)
                args = [str(len(failed) + 1), str(len(failed)), "false"]
                for group in (failed, flaky, skipped):
                    args += [str(len(group)), *group]
                result = emit("run_finish", *args, env=self.env)
                self.assertEqual((result.returncode, result.stderr), (0, ""))
                (line,) = self.physical_lines()
                self.assertLessEqual(len(line) + 1, LINE_MAX)
                (rec,) = self.read().records
                self.assertEqual(rec["failures"], len(failed))
                total = len(failed) + len(flaky) + len(skipped)
                kept = len(rec["failed"]) + len(rec["flaky"]) + len(rec["skipped"])
                self.assertEqual(kept + rec["names_omitted"], total)
                self.assertGreater(rec["names_omitted"], 0)
                for index, (field, given) in enumerate(
                    (("failed", failed), ("flaky", flaky), ("skipped", skipped))
                ):
                    kept_names = rec[field]
                    if index < cut:
                        self.assertEqual(kept_names, given, field)
                    elif index == cut:
                        self.assertGreater(len(kept_names), 0, field)
                        self.assertLess(len(kept_names), len(given), field)
                        self.assertEqual(kept_names, given[: len(kept_names)], field)
                    else:
                        self.assertEqual(kept_names, [], field)

    def emit_run_finish_strictly(self, *args):
        result = lib_bash(
            'event_log_emit_run_finish "$@"; echo next-command-ran',
            *args,
            env=self.env,
            strict=True,
        )
        self.assertEqual((result.returncode, result.stdout), (0, "next-command-ran\n"), result.stderr)
        (rec,) = self.read().records
        return rec

    def test_run_finish_reads_a_leading_zero_count_as_decimal(self):
        failed = [f"f{i}" for i in range(10)]
        flaky = [f"k{i}" for i in range(9)]
        skipped = [f"s{i}" for i in range(8)]
        rec = self.emit_run_finish_strictly(
            "27", "10", "false", "010", *failed, "09", *flaky, "008", *skipped
        )
        self.assertEqual(
            (rec["failed"], rec["flaky"], rec["skipped"], rec["names_omitted"]),
            (failed, flaky, skipped, 0),
        )

    def test_a_malformed_run_finish_count_empties_that_list_and_every_later_one(self):
        cases = {
            "not an integer": ["1", "a", "x", "b", "1", "c"],
            "negative": ["1", "a", "-1", "b", "1", "c"],
            "exceeds the arguments left": ["1", "a", "5", "b", "1", "c"],
        }
        for label, lists in cases.items():
            with self.subTest(label):
                self.channel.unlink(missing_ok=True)
                rec = self.emit_run_finish_strictly("3", "1", "false", *lists)
                self.assertEqual((rec["failed"], rec["flaky"], rec["skipped"]), (["a"], [], []))

    def unwritable_paths(self):
        regular = self.files.dir / "regular-file"
        regular.write_text("not a dir\n")
        self.addCleanup(lambda: self.assertEqual(regular.read_text(), "not a dir\n"))
        return {
            "parent is a regular file": regular / "channel.jsonl",
            "path is a directory": self.files.dir,
            "parent dir is missing": self.files.dir / "missing" / "channel.jsonl",
        }

    def assert_strict_run_continues(self, result):
        self.assertEqual(
            (result.returncode, result.stdout, result.stderr), (0, "next-command-ran\n", "")
        )

    def test_an_unwritable_channel_is_silent_and_never_changes_control_flow(self):
        for label, path in self.unwritable_paths().items():
            for emitter, args, _ in EMITTER_SHAPES:
                with self.subTest(label, emitter=emitter):
                    result = emit(emitter, *args, env=channel_env(path))
                    self.assertEqual((result.returncode, result.stderr), (0, ""))
            with self.subTest(label, shell="set -euo pipefail"):
                result = lib_bash(
                    ALL_EMITTERS_ONCE + "; echo next-command-ran",
                    env=channel_env(path),
                    strict=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, "next-command-ran\n")
                self.assertEqual(result.stderr, "")

    def test_an_unwritable_sink_never_starves_the_other_one(self):
        copy = self.files.dir / "copy.jsonl"
        for label, path in self.unwritable_paths().items():
            cases = {
                "copy unwritable, channel unset": (None, path, None),
                "copy unwritable, channel writable": (self.channel, path, self.channel),
                "channel unwritable, copy writable": (path, copy, copy),
            }
            for case, (channel, copy_to, written) in cases.items():
                with self.subTest(label, case=case):
                    self.channel.unlink(missing_ok=True)
                    copy.unlink(missing_ok=True)
                    result = lib_bash_with_copy(
                        ALL_EMITTERS_ONCE + "; echo next-command-ran",
                        copy_to,
                        env=channel_env(channel),
                        strict=True,
                    )
                    self.assert_strict_run_continues(result)
                    if written is not None:
                        events = verify_events.read_events(written)
                        self.assertEqual(
                            ([r["event"] for r in events.records], events.skipped_lines),
                            (ALL_EMITTER_NAMES, 0),
                        )

    def test_both_sinks_get_byte_identical_records(self):
        copy = self.files.dir / "copy.jsonl"
        result = lib_bash_with_copy(ALL_EMITTERS_ONCE, copy, env=self.env, strict=True)
        self.assertEqual((result.returncode, result.stderr), (0, ""))
        events = self.read()
        self.assertEqual(
            ([r["event"] for r in events.records], events.skipped_lines), (ALL_EMITTER_NAMES, 0)
        )
        self.assertEqual(copy.read_bytes(), self.channel.read_bytes())

    def test_the_copy_sink_alone_gets_every_record_and_creates_nothing_else(self):
        cwd = self.files.dir / "cwd"
        tmp = self.files.dir / "tmp"
        cwd.mkdir()
        tmp.mkdir()
        copy = self.files.dir / "copy.jsonl"
        env = channel_env(None, TMPDIR=str(tmp))
        assert_channel_unset(self, env)
        result = lib_bash_with_copy(ALL_EMITTERS_ONCE, copy, env=env, cwd=cwd, strict=True)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, "", ""))
        events = verify_events.read_events(copy)
        self.assertEqual(
            ([r["event"] for r in events.records], events.skipped_lines), (ALL_EMITTER_NAMES, 0)
        )
        self.assertEqual((list(cwd.iterdir()), list(tmp.iterdir())), ([], []))

    def test_ts_ns_and_the_ns_fields_read_the_one_public_clock(self):
        result = lib_bash(
            "event_log_clock_ns a; event_log_emit_member_start m 1 capture; "
            'event_log_clock_ns b; printf "%s %s\\n" "$a" "$b"',
            env=self.env,
            strict=True,
        )
        self.assertEqual((result.returncode, result.stderr), (0, ""))
        before, after = result.stdout.split()
        self.assertRegex(before, r"^[0-9]+$")
        self.assertRegex(after, r"^[0-9]+$")
        (rec,) = self.read().records
        self.assertLessEqual(int(before), rec["ts_ns"])
        self.assertLessEqual(rec["ts_ns"], int(after))

    def test_unset_channel_creates_nothing_and_prints_nothing(self):
        cwd = self.files.dir / "cwd"
        tmp = self.files.dir / "tmp"
        cwd.mkdir()
        tmp.mkdir()
        env = channel_env(None, TMPDIR=str(tmp))
        assert_channel_unset(self, env)
        self.assertNotIn(COPY_VAR, env)
        result = lib_bash(ALL_EMITTERS_ONCE, env=env, cwd=cwd, strict=True)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, "", ""))
        self.assertEqual((list(cwd.iterdir()), list(tmp.iterdir())), ([], []))

    def test_unset_channel_costs_no_fork_and_no_open_per_call(self):
        reason = strace_usable(self.files.dir)
        if reason:
            self.skipTest(reason)
        env = channel_env(None)
        assert_channel_unset(self, env)
        self.assertNotIn(COPY_VAR, env)
        loop = 'for ((i = 0; i < $1; i++)); do ' + ALL_EMITTERS_ONCE + "; done"
        counts = {}
        for calls in (0, 25):
            log = self.files.dir / f"calls-{calls}.strace"
            result = run(
                [
                    "strace", "-f", "-qq", "-e", "trace=process,openat,open,creat",
                    "-o", str(log),
                    "bash", "-c", 'source "$1"; shift; ' + loop, "_", str(LIB), str(calls),
                ],
                env=env,
            )
            self.assertEqual((result.returncode, result.stderr), (0, ""))
            counts[calls] = len(log.read_text().splitlines())
        self.assertGreater(counts[0], 0)
        self.assertEqual(counts[25], counts[0])

    def test_concurrent_writers_never_tear_a_line(self):
        writers = (
            'for w in 1 2 3 4 5 6 7 8; do '
            '( for ((i = 0; i < 50; i++)); do event_log_emit_member_start "w$w-$i" 1 capture; done ) & '
            "done; wait"
        )
        result = lib_bash(writers, env=self.env)
        self.assertEqual((result.returncode, result.stderr), (0, ""))
        events = self.read()
        self.assertEqual(events.skipped_lines, 0)
        self.assertEqual(len(events.records), 400)
        self.assertEqual(len({r["member"] for r in events.records}), 400)


SP = "@@REIFY_" + "SLOT_"

# Ambient knobs of the acquire wrappers. The gate hands its members its own
# DISABLE/WAIT/LOCK settings; none of them may decide these runs.
_ACQUIRE_KNOB_PREFIXES = (
    "REIFY_TEST_SEMAPHORE_",
    "REIFY_LANE_X_FLOCK_",
    "REIFY_OCCT_",
    "REIFY_RUN_ALL_",
    "REIFY_SLOT_EVENT_LOG",
)


def acquire_env(channel=None, **overrides):
    env = {
        k: v
        for k, v in channel_env(channel).items()
        if not k.startswith(_ACQUIRE_KNOB_PREFIXES)
    }
    env.update(overrides)
    return env


@contextlib.contextmanager
def holding_lock(path):
    """Hold the flock(2) on `path` in THIS process for the block.

    The descriptor is non-inheritable (PEP 446), so the child under test can
    never be the holder.
    """
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o644)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX)
        yield
    finally:
        os.close(fd)


def holding_slot(lock_base, slot=1):
    """Hold `<lock_base>.slot-<slot>` for the block."""
    return holding_lock(f"{lock_base}.slot-{slot}")


def sentinel_count(text, reason):
    prefix = f"{SP}TIMEOUT@@ reason={reason} "
    return sum(1 for line in text.split("\n") if line.startswith(prefix))


# Every finite-WAIT caller of slot_acquire in the verify tree, forced to its
# deadline: (label, argv after "bash", knobs given the private lock, emitter,
# reason). The lib_test_semaphore.sh row is PRD §4.1 B7.
WRAPPER_CALLERS = [
    (
        "lib_test_semaphore.sh",
        [str(SCRIPTS / "lib_test_semaphore.sh"), "true"],
        lambda lock: {
            "REIFY_TEST_SEMAPHORE_LOCK": lock,
            "REIFY_TEST_SEMAPHORE_WAIT": "1",
            "REIFY_TEST_SEMAPHORE_CONCURRENCY": "1",
        },
        "test_slot_starvation",
    ),
    (
        "cargo-test-occt-gated.sh",
        [str(SCRIPTS / "cargo-test-occt-gated.sh"), "true"],
        lambda lock: {
            "REIFY_OCCT_LOCK": lock,
            "REIFY_OCCT_LOCK_WAIT": "1",
            "REIFY_OCCT_CONCURRENCY": "1",
        },
        "occt_slot_starvation",
    ),
    (
        "lib_lane_x_flock.sh",
        ["-c", 'source "$1/lib_lane_x_flock.sh"; lane_x_flock_acquire', "_", str(SCRIPTS)],
        lambda lock: {"REIFY_LANE_X_FLOCK_LOCK": lock, "REIFY_LANE_X_FLOCK_WAIT": "1"},
        "lane_x_slot_starvation",
    ),
]

SLOT_ACQUIRE_DIRECT = (
    'source "$1/lib_slot_acquire.sh"; shift; slot_acquire "$@"; echo "rc=$?"'
)


class SlotTimeoutRecordTests(unittest.TestCase):
    def setUp(self):
        self.files = ChannelFiles(self)
        self.channel = self.files.dir / "channel.jsonl"
        self.locks = self.files.dir / "locks"
        self.locks.mkdir()

    def timeouts(self):
        return verify_events.select(verify_events.read_events(self.channel), event="slot_timeout")

    def assert_one_timeout(self, lock, **fields):
        records = self.timeouts()
        self.assertEqual(len(records), 1)
        (rec,) = records
        self.assertEqual(rec["lock"], str(lock))
        self.assertTrue(is_int(rec["waited"]) and rec["waited"] >= 0, rec["waited"])
        self.assertEqual({k: rec[k] for k in fields}, fields)

    def slot_acquire_direct(self, lock, *args):
        with holding_slot(lock):
            return run(
                ["bash", "-c", SLOT_ACQUIRE_DIRECT, "_", str(SCRIPTS), str(lock), *args],
                env=acquire_env(self.channel),
            )

    def test_b4_an_awkward_lock_base_round_trips_and_the_text_line_is_unchanged(self):
        lock_dir = self.locks / "lock dir"
        lock_dir.mkdir()
        lock = lock_dir / 'b4 "quoted" back\\slash\nnewline'
        result = self.slot_acquire_direct(lock, "1", "1", "", "b4_probe_starvation")
        self.assertEqual(result.stdout, "rc=75\n")
        self.assert_one_timeout(
            lock,
            reason="b4_probe_starvation",
            slots=1,
            disposition="fatal",
            emitter="unknown",
        )
        self.assertEqual(sentinel_count(result.stderr, "b4_probe_starvation"), 1)

    def test_the_seventh_argument_names_the_emitter(self):
        lock = self.locks / "override.lock"
        result = self.slot_acquire_direct(lock, "1", "1", "", "override_probe", "soft", "my_emitter")
        self.assertEqual(result.stdout, "rc=75\n")
        self.assert_one_timeout(
            lock, reason="override_probe", disposition="soft", emitter="my_emitter"
        )

    def test_every_wrapper_caller_writes_one_fatal_record_naming_itself(self):
        for label, argv, knobs, reason in WRAPPER_CALLERS:
            with self.subTest(caller=label):
                self.channel.unlink(missing_ok=True)
                lock = self.locks / f"{label}.lock"
                with holding_slot(lock):
                    result = run(["bash", *argv], env=acquire_env(self.channel, **knobs(str(lock))))
                self.assertEqual(result.returncode, 75)
                self.assert_one_timeout(
                    lock, reason=reason, slots=1, disposition="fatal", emitter=label
                )
                self.assertEqual(sentinel_count(result.stderr, reason), 1)

    def test_b7_with_the_channel_unset_writes_nothing_and_keeps_the_text_line(self):
        label, argv, knobs, reason = WRAPPER_CALLERS[0]
        scratch = self.files.dir / "scratch"
        scratch.mkdir()
        lock = self.locks / "unset.lock"
        env = acquire_env(None, TMPDIR=str(scratch), **knobs(str(lock)))
        assert_channel_unset(self, env)
        with holding_slot(lock):
            result = run(["bash", *argv], env=env, cwd=scratch)
        self.assertEqual(result.returncode, 75)
        self.assertEqual(list(scratch.iterdir()), [])
        self.assertEqual(sentinel_count(result.stderr, reason), 1)


PASS_MEMBER = """#!/usr/bin/env bash
echo "member $(basename "$0") ok"
exit 0
"""

FAIL_MEMBER = """#!/usr/bin/env bash
echo "  FAIL: x"
exit 1
"""

# Fails its first run and passes every later one, so the pool's serial retry
# turns it into a flaky pass.
FLAKY_MEMBER = """#!/usr/bin/env bash
if [ -e "$FIXTURE_STATE/flaky.failed-once" ]; then
    exit 0
fi
touch "$FIXTURE_STATE/flaky.failed-once"
echo "  FAIL: first attempt"
exit 1
"""

TWELVE_FAILS_MEMBER = """#!/usr/bin/env bash
for i in 1 2 3 4 5 6 7 8 9 10 11 12; do echo "  FAIL: $i"; done
exit 1
"""

# Forces a REAL fatal slot_acquire deadline inside a member (it holds the
# slot itself: a second open file description conflicts under flock(2)), then
# prints a quoted sentinel assembled from fragments. Records what it saw of
# the channel variable and the deadline's rc.
DEADLINE_MEMBER = """#!/usr/bin/env bash
printf '%s\\n' "${REIFY_VERIFY_EVENT_LOG-<unset>}" > "$FIXTURE_STATE/channel-env"
source "$REPO_SCRIPTS/lib_slot_acquire.sh"
exec 8>>"$FIXTURE_STATE/m.lock.slot-1"
flock -x 8
rc=0
slot_acquire "$FIXTURE_STATE/m.lock" 1 1 "" member_probe_starvation || rc=$?
echo "$rc" > "$FIXTURE_STATE/deadline-rc"
token='@@REIFY_'
echo "${token}SLOT_TIMEOUT@@ reason=quoted_by_member slots=1 waited=0 disposition=fatal lock=/quoted"
exit 0
"""

# Runs until the test touches $FIXTURE_STATE/release (600 x 0.2s backstop).
HANG_MEMBER = """#!/usr/bin/env bash
touch "$FIXTURE_STATE/hang.ready"
for _ in $(seq 1 600); do
    [ -e "$FIXTURE_STATE/release" ] && exit 0
    sleep 0.2
done
exit 0
"""

NUKE_MEMBER = """#!/usr/bin/env bash
rm -f "$(dirname "$0")/run_all.sh"
exit 0
"""

B1_MEMBERS = {
    "test_fail.sh": FAIL_MEMBER,
    "test_pass.sh": PASS_MEMBER,
    "test_skippable.sh": PASS_MEMBER,
}
B1_ROWS = {name: "pool" for name in B1_MEMBERS}


def write_executable(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def summary_counts(stdout):
    """(discovered, failed) from run_all's `=== Summary: N discovered, M failed` line."""
    match = re.search(r"^=== Summary: (\d+) discovered, (\d+) failed", stdout, re.M)
    return (int(match.group(1)), int(match.group(2))) if match else None


def failed_line_names(stdout):
    """The names on run_all's bare `FAILED <names>` classifier line ([] if none)."""
    for line in stdout.split("\n"):
        if line.startswith("FAILED "):
            names = line[len("FAILED "):].split()
            return [n for n in names if n != "(partial)"]
    return []


def by_member(records):
    return {r["member"]: r for r in records}


class RunAllFixture:
    """A tempdir INFRA_DIR of members plus every private path run_all.sh needs."""

    def __init__(self, test, members):
        self.root = Path(tempfile.mkdtemp(prefix="ra-events-"))
        test.addCleanup(shutil.rmtree, self.root, True)
        self.infra = self.root / "infra"
        self.state = self.root / "state"
        self.tmp = self.root / "tmp"
        self.cwd = self.root / "cwd"
        for directory in (self.infra, self.state, self.tmp, self.cwd):
            directory.mkdir()
        for name, body in members.items():
            write_executable(self.infra / name, body)
        self.channel = self.root / "channel.jsonl"
        self._manifests = 0

    def manifest(self, rows):
        self._manifests += 1
        path = self.root / f"classification-{self._manifests}.manifest"
        path.write_text("".join(f"{name} {bucket}\n" for name, bucket in rows.items()))
        return path

    def env(self, manifest, channel=True, **overrides):
        env = acquire_env(self.channel if channel else None)
        env.update(
            RUN_ALL_CLASSIFICATION_MANIFEST=str(manifest),
            REIFY_RUN_ALL_POOL_LOCK=str(self.root / "pool.lock"),
            REIFY_RUN_ALL_POOL_CONCURRENCY="2",
            REIFY_RUN_ALL_POOL_PSI_DISABLE="1",
            REIFY_RUN_ALL_FLAKY_LEDGER=str(self.root / "flaky-ledger.jsonl"),
            REIFY_LANE_X_FLOCK_LOCK=str(self.root / "lane-x.lock"),
            TMPDIR=str(self.tmp),
            FIXTURE_STATE=str(self.state),
            REPO_SCRIPTS=str(SCRIPTS),
        )
        env.update(overrides)
        return env

    def argv(self, *flags):
        return ["bash", str(RUN_ALL), str(self.infra), *flags]

    def run(self, *flags, manifest, channel=True, **overrides):
        return run(self.argv(*flags), env=self.env(manifest, channel, **overrides), cwd=self.cwd)

    def start(self, test, env):
        """Start run_all.sh in its own session, stderr merged into the stdout pipe.

        The cleanup releases HANG_MEMBER, then kills whatever is left of the group.
        """
        proc = subprocess.Popen(
            self.argv(), env=env, cwd=self.cwd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, errors="surrogateescape", start_new_session=True,
        )

        def stop():
            (self.state / "release").touch()
            with contextlib.suppress(ProcessLookupError, PermissionError):
                os.killpg(proc.pid, signal.SIGKILL)
            with contextlib.suppress(subprocess.TimeoutExpired):
                proc.communicate(timeout=30)

        test.addCleanup(stop)
        return proc

    def make_infra_a_git_toplevel(self, test):
        """Give INFRA_DIR its own repo and one empty commit, so HEAD resolves."""
        git = [
            "git", "-C", str(self.infra), "-c", "core.hooksPath=/dev/null",
            "-c", "commit.gpgsign=false", "-c", "user.name=fixture",
            "-c", "user.email=fixture@fixture.invalid",
        ]
        for args in (["init", "-q"], ["commit", "-q", "--allow-empty", "-m", "fixture"]):
            test.assertEqual(run([*git, *args]).returncode, 0, args)

    def events(self):
        return verify_events.read_events(self.channel)


# (run_start.branch, extra run_all flags, knobs) for the B1 member set.
B1_BRANCHES = [
    ("pool", {}),
    ("serial", {"REIFY_RUN_ALL_POOL_DISABLE": "1"}),
]


class RunAllChannelTests(unittest.TestCase):
    def assert_one_run(self, result, events, branch):
        starts = verify_events.select(events, event="run_start")
        finishes = verify_events.select(events, event="run_finish")
        self.assertEqual((len(starts), len(finishes)), (1, 1))
        start, finish = starts[0], finishes[0]
        self.assertEqual((start["scope"], start["branch"]), ("all", branch))
        discovered, failed = summary_counts(result.stdout)
        self.assertEqual(
            (start["discovered"], finish["discovered"], finish["failures"]),
            (discovered, discovered, failed),
        )
        self.assertEqual(finish["failed"], failed_line_names(result.stdout))
        self.assertIs(finish["interrupted"], False)
        return finish

    def test_b1_pool_and_serial_write_one_run_and_one_finish_per_member(self):
        for branch, knobs in B1_BRANCHES:
            with self.subTest(branch=branch):
                fx = RunAllFixture(self, B1_MEMBERS)
                result = fx.run(manifest=fx.manifest(B1_ROWS), **knobs)
                self.assertEqual(result.returncode, 1)
                events = fx.events()
                self.assertEqual(events.skipped_lines, 0)
                finish = self.assert_one_run(result, events, branch)
                self.assertEqual(finish["failed"], ["test_fail.sh"])
                finishes = verify_events.select(events, event="member_finish")
                self.assertEqual(sorted(r["member"] for r in finishes), sorted(B1_MEMBERS))
                records = by_member(finishes)
                fail = records["test_fail.sh"]
                self.assertEqual(
                    (fail["verdict"], fail["rc"], fail["fail_detail"], fail["fail_detail_omitted"]),
                    ("FAIL", 1, ["  FAIL: x"], 0),
                )
                self.assertEqual(fail["attempt"], 2 if branch == "pool" else 1)
                for name in ("test_pass.sh", "test_skippable.sh"):
                    record = records[name]
                    self.assertEqual(
                        (record["verdict"], record["rc"], record["attempt"], record["fail_detail"]),
                        ("PASS", 0, 1, []),
                    )
                for record in finishes:
                    self.assertTrue(is_int(record["duration_ms"]) and record["duration_ms"] >= 0)
                starts = verify_events.select(events, event="member_start")
                expected_mode = "capture" if branch == "pool" else "stream"
                self.assertEqual({r["mode"] for r in starts}, {expected_mode})
                fail_attempts = [r["attempt"] for r in starts if r["member"] == "test_fail.sh"]
                self.assertEqual(fail_attempts, [1, 2] if branch == "pool" else [1])

    def test_b1_a_pool_member_that_passes_its_retry_is_one_flaky_pass(self):
        fx = RunAllFixture(self, {"test_flaky.sh": FLAKY_MEMBER, "test_pass.sh": PASS_MEMBER})
        result = fx.run(manifest=fx.manifest({"test_flaky.sh": "pool", "test_pass.sh": "pool"}))
        self.assertEqual(result.returncode, 0)
        events = fx.events()
        finish = self.assert_one_run(result, events, "pool")
        flaky_retried = re.search(r"^=== Summary: .*, (\d+) flaky-retried ===$", result.stdout, re.M)
        self.assertIsNotNone(flaky_retried)
        self.assertEqual(finish["flaky"], ["test_flaky.sh"])
        self.assertEqual(len(finish["flaky"]), int(flaky_retried.group(1)))
        flaky = verify_events.select(events, event="member_finish", where={"member": "test_flaky.sh"})
        self.assertEqual(len(flaky), 1)
        self.assertEqual(
            (flaky[0]["verdict"], flaky[0]["attempt"], flaky[0]["rc"], flaky[0]["fail_detail"]),
            ("FLAKY_PASS", 2, 0, []),
        )
        starts = verify_events.select(events, event="member_start", where={"member": "test_flaky.sh"})
        self.assertEqual([r["attempt"] for r in starts], [1, 2])

    def test_b1_subset_branch_records_the_skipped_member(self):
        fx = RunAllFixture(self, B1_MEMBERS)
        result = fx.run(
            manifest=fx.manifest(B1_ROWS),
            REIFY_RUN_ALL_MEMBER_SUBSET="test_pass.sh test_fail.sh",
        )
        self.assertEqual(result.returncode, 1)
        events = fx.events()
        finish = self.assert_one_run(result, events, "subset")
        self.assertEqual(finish["skipped"], ["test_skippable.sh"])
        records = by_member(verify_events.select(events, event="member_finish"))
        self.assertEqual(sorted(records), sorted(B1_MEMBERS))
        skip = records["test_skippable.sh"]
        self.assertEqual(
            (skip["verdict"], skip["attempt"], skip["rc"], skip["duration_ms"]),
            ("SKIP", 0, None, None),
        )
        started = {r["member"] for r in verify_events.select(events, event="member_start")}
        self.assertEqual(started, {"test_pass.sh", "test_fail.sh"})

    def test_fail_detail_text_block_and_record_share_one_bound(self):
        fx = RunAllFixture(self, {"test_many.sh": TWELVE_FAILS_MEMBER})
        result = fx.run(
            manifest=fx.manifest({"test_many.sh": "pool"}),
            REIFY_RUN_ALL_POOL_DISABLE="1",
            REIFY_RUN_ALL_FAIL_DETAIL_MAX="10",
        )
        self.assertEqual(result.returncode, 1)
        kept = [f"  FAIL: {i}" for i in range(3, 13)]
        lines = result.stdout.split("\n")
        opener = "--- FAILED-DETAIL: test_many.sh ---"
        closer = "--- END FAILED-DETAIL: test_many.sh ---"
        self.assertEqual((lines.count(opener), lines.count(closer)), (1, 1))
        block = lines[lines.index(opener) + 1 : lines.index(closer)]
        self.assertEqual(
            block,
            ["... (2 earlier FAIL line(s) omitted; see per-test output above) ...", *kept],
        )
        records = verify_events.select(
            fx.events(), event="member_finish", where={"member": "test_many.sh"}
        )
        self.assertEqual(len(records), 1)
        self.assertEqual(
            (records[0]["fail_detail"], records[0]["fail_detail_omitted"]), (kept, 2)
        )

    def test_c4_b2_a_member_never_sees_or_writes_the_parent_channel(self):
        branches = [
            ("pool", "pool", (), {}),
            ("serial", "pool", (), {"REIFY_RUN_ALL_POOL_DISABLE": "1"}),
            ("subset", "pool", (), {"REIFY_RUN_ALL_MEMBER_SUBSET": "test_deadline.sh"}),
            ("host-infra", "host-exclusive", ("--scope", "host-infra"), {}),
        ]
        for branch, bucket, flags, knobs in branches:
            with self.subTest(branch=branch):
                fx = RunAllFixture(self, {"test_deadline.sh": DEADLINE_MEMBER})
                result = fx.run(*flags, manifest=fx.manifest({"test_deadline.sh": bucket}), **knobs)
                self.assertEqual(result.returncode, 0)
                self.assertEqual((fx.state / "deadline-rc").read_text(), "75\n")
                events = fx.events()
                self.assertEqual(
                    (
                        (fx.state / "channel-env").read_text(),
                        len(verify_events.select(events, event="slot_timeout")),
                    ),
                    ("<unset>\n", 0),
                )
                self.assertEqual(len(verify_events.select(events, event="run_start")), 1)
                self.assertEqual(len(verify_events.select(events, event="member_finish")), 1)
                self.assertEqual((result.stdout + result.stderr).count(SP), 0)

    def test_b5_an_unset_channel_leaves_no_file_and_the_same_text(self):
        fx = RunAllFixture(self, B1_MEMBERS)
        manifest = fx.manifest(B1_ROWS)
        env = fx.env(manifest, channel=False)
        assert_channel_unset(self, env)
        unset = run(fx.argv(), env=env, cwd=fx.cwd)
        self.assertEqual((list(fx.tmp.iterdir()), list(fx.cwd.iterdir())), ([], []))
        self.assertFalse(fx.channel.exists())
        channel_set = fx.run(manifest=manifest)
        self.assertEqual(unset.returncode, channel_set.returncode)
        self.assertEqual(unset.stdout, channel_set.stdout)
        self.assertTrue(fx.events().channel_present)

    def test_b3_the_pool_wait_deadline_is_the_one_parent_channel_timeout(self):
        fx = RunAllFixture(self, {"test_pass.sh": PASS_MEMBER})
        pool_lock = fx.root / "pool.lock"
        with holding_slot(pool_lock):
            result = fx.run(
                manifest=fx.manifest({"test_pass.sh": "pool"}),
                REIFY_RUN_ALL_POOL_CONCURRENCY="1",
                REIFY_RUN_ALL_POOL_WAIT="1",
            )
        self.assertEqual(result.returncode, 0)
        events = fx.events()
        timeouts = verify_events.select(events, event="slot_timeout")
        self.assertEqual(len(timeouts), 1)
        self.assertEqual(
            {k: timeouts[0][k] for k in ("disposition", "emitter", "reason", "slots", "lock")},
            {
                "disposition": "soft",
                "emitter": "run_all_pool",
                "reason": "run_all_pool_starvation",
                "slots": 1,
                "lock": str(pool_lock),
            },
        )
        finishes = verify_events.select(events, event="member_finish")
        self.assertEqual([(r["member"], r["verdict"]) for r in finishes], [("test_pass.sh", "PASS")])
        self.assertEqual(sentinel_count(result.stderr, "run_all_pool_starvation"), 1)

    def test_a_terminated_run_still_closes_with_an_interrupted_run_finish(self):
        fx = RunAllFixture(self, {"test_hang.sh": HANG_MEMBER})
        proc = fx.start(self, fx.env(fx.manifest({"test_hang.sh": "pool"})))
        self.assertTrue(verify_events.wait_for(fx.channel, event="run_start", deadline_s=120))
        self.assertTrue(
            verify_events.wait_for(
                fx.channel, event="member_start", where={"member": "test_hang.sh"}, deadline_s=120
            )
        )
        os.killpg(proc.pid, signal.SIGTERM)
        output, _ = proc.communicate(timeout=120)
        self.assertNotEqual(proc.returncode, 0)
        self.assertIn("=== Summary: INTERRUPTED", output)
        events = fx.events()
        finishes = verify_events.select(events, event="run_finish")
        self.assertEqual(len(finishes), 1)
        self.assertIs(finishes[0]["interrupted"], True)
        self.assertEqual(finishes[0]["failed"], failed_line_names(output))
        order = [r["event"] for r in events.records if r["event"] in ("run_start", "run_finish")]
        self.assertEqual(order, ["run_start", "run_finish"])

    def test_a_term_after_the_normal_run_finish_writes_no_second_one(self):
        fx = RunAllFixture(self, {"test_pass.sh": PASS_MEMBER})
        fx.make_infra_a_git_toplevel(self)
        skip_state = fx.root / "skip-state"
        closures = fx.root / "skip-closures.manifest"
        closures.write_text("")
        env = fx.env(
            fx.manifest({"test_pass.sh": "pool"}),
            DF_VERIFY_ROLE="merge",
            REIFY_RUN_ALL_CONTENT_SKIP="1",
            REIFY_RUN_ALL_SKIP_STATE=str(skip_state),
            RUN_ALL_SKIP_CLOSURES_MANIFEST=str(closures),
        )
        # The held ledger lock parks run_all's post-run content-skip write,
        # which runs after the normal run_finish and before the exit.
        with holding_lock(f"{skip_state}.lock"):
            proc = fx.start(self, env)
            self.assertTrue(verify_events.wait_for(fx.channel, event="run_finish", deadline_s=120))
            os.killpg(proc.pid, signal.SIGTERM)
            output, _ = proc.communicate(timeout=120)
        self.assertIn("=== Summary: INTERRUPTED", output)
        finishes = verify_events.select(fx.events(), event="run_finish")
        self.assertEqual(len(finishes), 1)
        self.assertIs(finishes[0]["interrupted"], False)

    def test_the_worktree_gone_exit_still_closes_with_an_interrupted_run_finish(self):
        fx = RunAllFixture(
            self, {"test_a_nuke.sh": NUKE_MEMBER, "test_b_second.sh": PASS_MEMBER}
        )
        (fx.infra / "run_all.sh").write_text("# arms run_all.sh's worktree self-check\n")
        result = fx.run(
            manifest=fx.manifest({"test_a_nuke.sh": "pool", "test_b_second.sh": "pool"}),
            REIFY_RUN_ALL_POOL_DISABLE="1",
        )
        self.assertEqual(result.returncode, 99)
        self.assertIn("=== INTERRUPTED (worktree removed) ===", result.stdout.split("\n"))
        events = fx.events()
        finishes = verify_events.select(events, event="run_finish")
        self.assertEqual(len(finishes), 1)
        self.assertIs(finishes[0]["interrupted"], True)
        members = verify_events.select(events, event="member_finish")
        self.assertEqual([(r["member"], r["verdict"]) for r in members], [("test_a_nuke.sh", "PASS")])
        started = [r["member"] for r in verify_events.select(events, event="member_start")]
        self.assertEqual(started, ["test_a_nuke.sh"])

    def test_the_count_only_probe_writes_no_record(self):
        fx = RunAllFixture(self, B1_MEMBERS)
        result = fx.run(
            manifest=fx.manifest(B1_ROWS),
            REIFY_RUN_ALL_SUBSET_COUNT_ONLY="1",
            REIFY_RUN_ALL_MEMBER_SUBSET="test_pass.sh",
        )
        self.assertEqual(result.returncode, 0)
        self.assertIn("@@REIFY_RUN_ALL_SUBSET_MATCHED=1@@", result.stdout.split("\n"))
        self.assertEqual(fx.channel.read_bytes() if fx.channel.exists() else b"", b"")


if __name__ == "__main__":
    unittest.main(verbosity=2)
