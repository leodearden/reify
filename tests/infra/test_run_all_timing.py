#!/usr/bin/env python3
"""
test_run_all_timing.py — run_all.sh's per-member and per-phase timing, as the
event records it writes through scripts/lib_event_log.sh: queued_ns,
started_ns and ended_ns on each member_finish, one phase_finish per phase.

EXECUTES the real tests/infra/run_all.sh on a tempdir fixture INFRA_DIR
(private classification manifest, pool lock, Lane-X lock, flaky ledger and
TMPDIR, PSI disabled) on the pool branch and on each serial branch
(host-infra, member-subset, legacy serial). The fixture has two passing pool
members, one pool member that fails once (the pool retries it in Phase 2.5),
and one intra-run-serial member. The timed pool run sets both sinks: the
REIFY_VERIFY_EVENT_LOG channel and the REIFY_RUN_ALL_TIMING_OUT step file,
which must hold the same records. Clock assertions are orderings only.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_run_all_timing.sh. Schema: tests/infra/README.md
"Verify event channel".
"""

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import verify_events  # noqa: E402  (imported after sys.path manipulation)
from verify_events_lib import read_events  # noqa: E402
from run_all_fixture import run_all_env, write_executable  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[2]
RUN_ALL = REPO_ROOT / "tests" / "infra" / "run_all.sh"

POOL_A, POOL_B, FLAKY, SERIAL_A = (
    "test_pool_a.sh",
    "test_pool_b.sh",
    "test_flaky.sh",
    "test_serial_a.sh",
)
MEMBERS = (POOL_A, POOL_B, FLAKY, SERIAL_A)

POOL_ROWS = {POOL_A: "pool", POOL_B: "pool", FLAKY: "pool", SERIAL_A: "intra-run-serial"}
HOST_ROWS = {POOL_A: "host-exclusive", POOL_B: "pool", FLAKY: "pool", SERIAL_A: "host-exclusive"}

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

SINK_VARS = ("REIFY_VERIFY_EVENT_LOG", "REIFY_RUN_ALL_TIMING_OUT", "EVENT_LOG_COPY_TO")


class Fixture:
    def __init__(self, add_cleanup):
        self.root = Path(tempfile.mkdtemp(prefix="ra-fixture-"))
        add_cleanup(shutil.rmtree, self.root, True)
        self.infra = self.root / "infra"
        self.state = self.root / "state"
        self.tmp = self.root / "tmp"
        for directory in (self.infra, self.state, self.tmp):
            directory.mkdir()
        for name in MEMBERS:
            write_executable(self.infra / name, FLAKY_MEMBER if name == FLAKY else PASSING_MEMBER)
        self.pool_manifest = self.manifest("pool", POOL_ROWS)
        self.host_manifest = self.manifest("host", HOST_ROWS)

    def manifest(self, label, rows):
        path = self.root / f"classification-{label}.manifest"
        path.write_text("".join(f"{name} {bucket}\n" for name, bucket in rows.items()))
        return path

    def env(self, manifest, channel, step_file, knobs):
        env = {
            k: v
            for k, v in run_all_env(self.root, manifest).items()
            if not k.startswith("REIFY_LANE_X_FLOCK_")
        }
        env.update(
            REIFY_RUN_ALL_POOL_CONCURRENCY="2",
            REIFY_LANE_X_FLOCK_LOCK=str(self.root / "lane-x.lock"),
            TMPDIR=str(self.tmp),
            FIXTURE_STATE=str(self.state),
        )
        if channel is not None:
            env["REIFY_VERIFY_EVENT_LOG"] = str(channel)
        if step_file is not None:
            env["REIFY_RUN_ALL_TIMING_OUT"] = str(step_file)
        env.update(knobs)
        return env

    def run(self, *flags, manifest, channel=None, step_file=None, **knobs):
        (self.state / "flaky-count").unlink(missing_ok=True)
        for dump in self.state.glob("*.env"):
            dump.unlink()
        return subprocess.run(
            ["bash", str(RUN_ALL), str(self.infra), *flags],
            env=self.env(manifest, channel, step_file, knobs),
            capture_output=True,
            text=True,
            timeout=600,
        )

    def env_dumps(self):
        return {p.name: p.read_text() for p in self.state.glob("*.env")}


def is_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def by_member(records):
    return {r["member"]: r for r in records}


def within(window, *instants):
    return all(window["started_ns"] <= t <= window["ended_ns"] for t in instants)


# (label, run_all flags, manifest attribute, knobs, members it runs)
SERIAL_BRANCHES = [
    ("host-infra", ("--scope", "host-infra"), "host_manifest", {}, {POOL_A, SERIAL_A}),
    (
        "subset",
        (),
        "pool_manifest",
        {"REIFY_RUN_ALL_MEMBER_SUBSET": f"{POOL_A} {SERIAL_A}"},
        {POOL_A, SERIAL_A},
    ),
    ("serial", (), "pool_manifest", {"REIFY_RUN_ALL_POOL_DISABLE": "1"}, set(MEMBERS)),
]


class RunAllTimingTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        fx = cls.fixture = Fixture(cls.addClassCleanup)
        cls.untimed = fx.run(manifest=fx.pool_manifest)
        cls.untimed_tree = [p.name for p in fx.root.rglob("*")]
        cls.channel = fx.root / "channel.jsonl"
        cls.step_file = fx.root / "step-0.run-all.jsonl"
        cls.timed = fx.run(manifest=fx.pool_manifest, channel=cls.channel, step_file=cls.step_file)
        cls.timed_env_dumps = fx.env_dumps()
        cls.serial_runs = {}
        for label, flags, manifest, knobs, _ in SERIAL_BRANCHES:
            channel = fx.root / f"channel-{label}.jsonl"
            result = fx.run(*flags, manifest=getattr(fx, manifest), channel=channel, **knobs)
            cls.serial_runs[label] = (result, read_events(channel))

    def events(self):
        return read_events(self.channel)

    def test_the_timed_pool_run_passes_with_the_flaky_retry(self):
        self.assertEqual(self.timed.returncode, 0, self.timed.stdout + self.timed.stderr)

    def test_every_pool_member_finish_carries_its_attempt_times(self):
        finishes = verify_events.select(self.events(), event="member_finish")
        self.assertEqual(sorted(r["member"] for r in finishes), sorted(MEMBERS))
        for record in finishes:
            with self.subTest(member=record["member"]):
                started, ended = record["started_ns"], record["ended_ns"]
                self.assertTrue(is_int(started) and is_int(ended), (started, ended))
                self.assertLessEqual(started, ended)
                self.assertEqual(record["duration_ms"], (ended - started) // 1_000_000)
        records = by_member(finishes)
        for name in (POOL_A, POOL_B):
            with self.subTest(member=name):
                record = records[name]
                self.assertEqual(record["attempt"], 1)
                self.assertTrue(is_int(record["queued_ns"]), record["queued_ns"])
                self.assertLessEqual(record["queued_ns"], record["started_ns"])
        self.assertEqual(
            (records[FLAKY]["attempt"], records[FLAKY]["verdict"], records[FLAKY]["queued_ns"]),
            (2, "FLAKY_PASS", None),
        )
        self.assertIsNone(records[SERIAL_A]["queued_ns"])

    def test_pool_phases_are_ordered_and_hold_their_members(self):
        events = self.events()
        phases = verify_events.select(events, event="phase_finish")
        self.assertEqual([p["phase"] for p in phases], ["pool", "serial", "retry"])
        for phase in phases:
            with self.subTest(phase=phase["phase"]):
                self.assertLessEqual(phase["started_ns"], phase["ended_ns"])
        pool, serial, retry = phases
        self.assertLessEqual(pool["ended_ns"], serial["started_ns"])
        self.assertLessEqual(serial["ended_ns"], retry["started_ns"])
        records = by_member(verify_events.select(events, event="member_finish"))
        for name, window, field in (
            (POOL_A, pool, "queued_ns"),
            (POOL_B, pool, "queued_ns"),
            (SERIAL_A, serial, "started_ns"),
            (FLAKY, retry, "started_ns"),
        ):
            with self.subTest(member=name):
                record = records[name]
                self.assertTrue(within(window, record[field], record["ended_ns"]), record)

    def test_the_step_file_holds_the_channel_records(self):
        step_lines = sorted(self.step_file.read_bytes().split(b"\n"))
        channel_lines = sorted(self.channel.read_bytes().split(b"\n"))
        self.assertGreater(len(channel_lines), 1)
        self.assertEqual(step_lines, channel_lines)
        step = read_events(self.step_file)
        self.assertEqual(step.skipped_lines, 0)
        self.assertEqual([r for r in step.records if "kind" in r], [])

    def test_every_serial_branch_times_its_members_inside_one_serial_phase(self):
        for label, _, _, _, ran in SERIAL_BRANCHES:
            with self.subTest(branch=label):
                result, events = self.serial_runs[label]
                self.assertEqual(events.skipped_lines, 0)
                self.assertEqual(len(verify_events.select(events, event="run_start")), 1, result.stderr)
                phases = verify_events.select(events, event="phase_finish")
                self.assertEqual([p["phase"] for p in phases], ["serial"])
                (window,) = phases
                self.assertLessEqual(window["started_ns"], window["ended_ns"])
                records = by_member(verify_events.select(events, event="member_finish"))
                timed = {name for name, r in records.items() if r["verdict"] != "SKIP"}
                self.assertEqual(timed, ran)
                for name in sorted(ran):
                    record = records[name]
                    started, ended = record["started_ns"], record["ended_ns"]
                    self.assertTrue(is_int(started) and is_int(ended), (name, started, ended))
                    self.assertLessEqual(started, ended)
                    self.assertIsNone(record["queued_ns"], name)
                    self.assertTrue(within(window, started, ended), (name, window, record))
                skipped = {name: r for name, r in records.items() if r["verdict"] == "SKIP"}
                if label == "subset":
                    self.assertEqual(sorted(skipped), sorted(set(MEMBERS) - ran))
                for name, record in skipped.items():
                    self.assertEqual(
                        (record["queued_ns"], record["started_ns"], record["ended_ns"]),
                        (None, None, None),
                        name,
                    )
                if label == "host-infra":
                    self.assertEqual(set(records), ran)

    def test_members_never_inherit_a_sink(self):
        self.assertEqual(sorted(self.timed_env_dumps), sorted(f"{name}.env" for name in MEMBERS))
        for name, dump in self.timed_env_dumps.items():
            for needle in (*SINK_VARS, str(self.step_file), str(self.channel)):
                with self.subTest(member=name, needle=needle):
                    self.assertNotIn(needle, dump)

    def test_the_recorder_is_silent(self):
        self.assertEqual(self.untimed.returncode, self.timed.returncode)
        self.assertEqual(self.untimed.stdout, self.timed.stdout)
        for needle in ("timing", "lib_event_log"):
            with self.subTest(needle=needle):
                self.assertNotIn(needle, self.timed.stderr.lower())

    def test_no_file_without_the_variables(self):
        for name in (self.step_file.name, self.channel.name):
            with self.subTest(name=name):
                self.assertNotIn(name, self.untimed_tree)

    def test_an_unwritable_step_file_keeps_the_exit_code_and_the_stdout(self):
        blocker = self.fixture.root / "not-a-dir"
        blocker.write_text("")
        run = self.fixture.run(
            manifest=self.fixture.pool_manifest, step_file=blocker / "step-0.run-all.jsonl"
        )
        self.assertEqual(run.returncode, self.timed.returncode)
        self.assertEqual(run.stdout, self.timed.stdout)
        self.assertNotIn("lib_event_log", run.stderr)


if __name__ == "__main__":
    unittest.main()
