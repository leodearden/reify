#!/usr/bin/env python3
"""
test_run_all_phase_log.py — run_all.sh names each phase as it starts and ends.

EXECUTES the real tests/infra/run_all.sh on a tempdir fixture INFRA_DIR, in
run_all_fixture.run_all_env's environment plus a private TMPDIR, and checks
the phase boundary lines it prints on stderr: a start line with the phase's
member count, an end line with that count and its elapsed whole seconds, and
on Phase 2.5 one line per member as its retry starts. Grammar: run_all.sh's
header, "Phase log".

The pool run covers all four phases (1 pool, 2 serial, 2.5 retry, 3 report);
the legacy serial run covers the one serial phase. A run SIGTERMed while its
Phase-2.5 retry is running (the merge envelope's group TERM) must name that
phase on the stdout line its TERM trap adds after the Summary line. Every
assertion compares the parsed (id, name, kind, fields) tuples of
phase_lines(), never a whole sentence.

The only time bounds here are hang guards: the subprocess timeouts and the
wait for the held retry's readiness marker. No assertion bounds how long
anything takes. The held retry member watches a release file, which cleanup
touches before it SIGKILLs run_all's process group, so a failing case leaves
nothing running.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_run_all_phase_log.sh.
"""

import contextlib
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from run_all_fixture import run_all_env, write_executable  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = Path(__file__).resolve().parents[2]
RUN_ALL = REPO_ROOT / "tests" / "infra" / "run_all.sh"

RUN_TIMEOUT_S = 600
READY_WAIT_S = 300

PASSING_MEMBER = """#!/usr/bin/env bash
exit 0
"""

# Fails on its first attempt and passes on the Phase-2.5 retry.
FLAKY_MEMBER = """#!/usr/bin/env bash
n=$(( $(cat "$FIXTURE_STATE/flaky-count" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$FIXTURE_STATE/flaky-count"
[ "$n" -ge 2 ]
"""

# Fails its first attempt. On its Phase-2.5 retry it touches retry-ready,
# then holds until $FIXTURE_STATE/release appears (or 300s pass) and fails.
RETRY_HOLD_MEMBER = """#!/usr/bin/env bash
n=$(( $(cat "$FIXTURE_STATE/hold-count" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$FIXTURE_STATE/hold-count"
[ "$n" -ge 2 ] || exit 1
touch "$FIXTURE_STATE/retry-ready"
for _ in $(seq 300); do [ -e "$FIXTURE_STATE/release" ] && break; sleep 1; done
exit 1
"""

PASS, FLAKY, SERIAL, RETRY_HOLD = "test_pass.sh", "test_flaky.sh", "test_serial.sh", "test_retry_hold.sh"

COMPLETED_RUN_MEMBERS = {
    PASS: (PASSING_MEMBER, "pool"),
    FLAKY: (FLAKY_MEMBER, "pool"),
    SERIAL: (PASSING_MEMBER, "intra-run-serial"),
}

INTERRUPTED_RUN_MEMBERS = {
    PASS: (PASSING_MEMBER, "pool"),
    RETRY_HOLD: (RETRY_HOLD_MEMBER, "pool"),
    SERIAL: (PASSING_MEMBER, "intra-run-serial"),
}

PHASE_TAG_RE = re.compile(r"\bphase (?P<id>[0-9.]+) \((?P<name>[a-z]+)\)(?P<rest>.*)$")
KIND_RE = re.compile(r"^\s*(?P<kind>start|end|retrying)\b")
FIELD_RE = re.compile(r"(?P<key>[A-Za-z_]+)=(?P<value>\S+)")


def phase_lines(text):
    """(id, name, kind, fields) for each line of <text> naming `phase <id> (<name>)`.

    kind is start, end or retrying when that word follows the tag, else None;
    fields holds every key=value token on the line.
    """
    found = []
    for line in text.splitlines():
        tag = PHASE_TAG_RE.search(line)
        if tag is None:
            continue
        kind = KIND_RE.match(tag["rest"])
        fields = {m["key"]: m["value"] for m in FIELD_RE.finditer(line)}
        found.append((tag["id"], tag["name"], kind["kind"] if kind else None, fields))
    return found


def is_whole_seconds(value):
    return value is not None and value.isdigit()


def _default_term():
    """A shell cannot trap a signal it inherits ignored, so run_all starts with TERM at its default."""
    signal.signal(signal.SIGTERM, signal.SIG_DFL)


class Fixture:
    def __init__(self, add_cleanup, members):
        self.add_cleanup = add_cleanup
        self.root = Path(tempfile.mkdtemp(prefix="ra-phase-log-"))
        add_cleanup(shutil.rmtree, self.root, True)
        self.infra = self.root / "infra"
        self.state = self.root / "state"
        self.tmp = self.root / "tmp"
        for directory in (self.infra, self.state, self.tmp):
            directory.mkdir()
        for name, (text, _) in members.items():
            write_executable(self.infra / name, text)
        self.manifest = self.root / "classification.manifest"
        self.manifest.write_text("".join(f"{name} {bucket}\n" for name, (_, bucket) in members.items()))

    def env(self, knobs):
        return run_all_env(
            self.root,
            self.manifest,
            TMPDIR=str(self.tmp),
            FIXTURE_STATE=str(self.state),
            REIFY_RUN_ALL_POOL_CONCURRENCY="2",
            **knobs,
        )

    def reset_state(self):
        for path in self.state.iterdir():
            path.unlink()

    def run(self, **knobs):
        return subprocess.run(
            ["bash", str(RUN_ALL), str(self.infra)],
            env=self.env(knobs),
            capture_output=True,
            text=True,
            errors="surrogateescape",
            timeout=RUN_TIMEOUT_S,
        )

    def spawn(self, **knobs):
        """run_all in a new session (its own process group); cleanup releases the held member, then kills the group."""
        proc = subprocess.Popen(
            ["bash", str(RUN_ALL), str(self.infra)],
            env=self.env(knobs),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            errors="surrogateescape",
            start_new_session=True,
            preexec_fn=_default_term,
        )

        def stop():
            (self.state / "release").touch()
            if proc.poll() is None:
                with contextlib.suppress(ProcessLookupError, PermissionError):
                    os.killpg(proc.pid, signal.SIGKILL)
            with contextlib.suppress(subprocess.TimeoutExpired):
                proc.communicate(timeout=30)

        self.add_cleanup(stop)
        return proc

    def wait_for(self, marker, proc):
        """True once state/<marker> exists; False if run_all exits or the hang guard expires first."""
        deadline = time.monotonic() + READY_WAIT_S
        while time.monotonic() < deadline:
            if (self.state / marker).exists():
                return True
            if proc.poll() is not None:
                return False
            time.sleep(0.2)
        return False


class CompletedRunPhaseLogTests(unittest.TestCase):
    """A run that finishes: every phase it enters prints a start and an end line."""

    @classmethod
    def setUpClass(cls):
        cls.fixture = Fixture(cls.addClassCleanup, COMPLETED_RUN_MEMBERS)
        cls.pool_run = cls.fixture.run()
        cls.fixture.reset_state()
        cls.serial_run = cls.fixture.run(REIFY_RUN_ALL_POOL_DISABLE="1")

    def output(self, run):
        return run.stdout + run.stderr

    def test_pool_run_names_each_phase_in_order(self):
        lines = phase_lines(self.pool_run.stderr)
        self.assertEqual(
            [(pid, name, kind, fields.get("members")) for pid, name, kind, fields in lines],
            [
                ("1", "pool", "start", "2"),
                ("1", "pool", "end", "2"),
                ("2", "serial", "start", "1"),
                ("2", "serial", "end", "1"),
                ("2.5", "retry", "start", "1"),
                ("2.5", "retry", "retrying", None),
                ("2.5", "retry", "end", "1"),
                ("3", "report", "start", "3"),
                ("3", "report", "end", "3"),
            ],
            self.output(self.pool_run),
        )
        retrying = [fields for _, _, kind, fields in lines if kind == "retrying"]
        self.assertEqual(retrying, [{"member": FLAKY}], self.output(self.pool_run))

    def test_every_end_line_carries_its_elapsed_whole_seconds(self):
        ends = [fields for _, _, kind, fields in phase_lines(self.pool_run.stderr) if kind == "end"]
        self.assertNotEqual(ends, [], self.output(self.pool_run))
        for fields in ends:
            with self.subTest(fields=fields):
                self.assertTrue(is_whole_seconds(fields.get("elapsed_s")), self.output(self.pool_run))

    def test_pool_run_stdout_carries_no_phase_line(self):
        self.assertEqual(phase_lines(self.pool_run.stdout), [], self.output(self.pool_run))

    def test_legacy_serial_run_names_its_one_phase(self):
        lines = phase_lines(self.serial_run.stderr)
        self.assertEqual(
            [(pid, name, kind, fields.get("members")) for pid, name, kind, fields in lines],
            [("2", "serial", "start", "3"), ("2", "serial", "end", "3")],
            self.output(self.serial_run),
        )
        self.assertTrue(is_whole_seconds(lines[-1][3].get("elapsed_s")), self.output(self.serial_run))


class InterruptedInRetryTests(unittest.TestCase):
    """A group SIGTERM while Phase 2.5 runs a retry: the TERM trap's summary names Phase 2.5."""

    @classmethod
    def setUpClass(cls):
        cls.fixture = Fixture(cls.addClassCleanup, INTERRUPTED_RUN_MEMBERS)
        proc = cls.fixture.spawn(REIFY_RUN_ALL_MEMBER_TIMEOUT="600")
        if not cls.fixture.wait_for("retry-ready", proc):
            with contextlib.suppress(ProcessLookupError, PermissionError):
                os.killpg(proc.pid, signal.SIGKILL)
            out, err = proc.communicate(timeout=RUN_TIMEOUT_S)
            raise AssertionError(f"{RETRY_HOLD} never started its retry\n{out}{err}")
        os.killpg(proc.pid, signal.SIGTERM)
        cls.stdout, cls.stderr = proc.communicate(timeout=RUN_TIMEOUT_S)
        cls.returncode = proc.returncode

    def output(self):
        return self.stdout + self.stderr

    def test_run_all_dies_of_its_re_raised_term(self):
        self.assertEqual(self.returncode, -signal.SIGTERM, self.output())

    def test_trap_names_the_retry_phase_it_interrupted(self):
        lines = phase_lines(self.stdout)
        self.assertEqual([(pid, name) for pid, name, _, _ in lines], [("2.5", "retry")], self.output())
        fields = lines[0][3]
        self.assertEqual(fields.get("members"), "1", self.output())
        self.assertTrue(is_whole_seconds(fields.get("elapsed_s")), self.output())

    def test_trap_phase_line_follows_the_interrupted_summary(self):
        lines = self.stdout.splitlines()
        summary = [i for i, line in enumerate(lines) if "Summary: INTERRUPTED" in line]
        phase = [i for i, line in enumerate(lines) if phase_lines(line)]
        self.assertEqual(len(summary), 1, self.output())
        self.assertEqual(len(phase), 1, self.output())
        self.assertGreater(phase[0], summary[0], self.output())

    def test_stderr_log_ends_inside_the_retry_phase(self):
        lines = phase_lines(self.stderr)
        self.assertEqual(
            [(pid, name, kind, fields.get("members")) for pid, name, kind, fields in lines],
            [
                ("1", "pool", "start", "2"),
                ("1", "pool", "end", "2"),
                ("2", "serial", "start", "1"),
                ("2", "serial", "end", "1"),
                ("2.5", "retry", "start", "1"),
                ("2.5", "retry", "retrying", None),
            ],
            self.output(),
        )
        self.assertEqual(lines[-1][3], {"member": RETRY_HOLD}, self.output())


if __name__ == "__main__":
    unittest.main()
