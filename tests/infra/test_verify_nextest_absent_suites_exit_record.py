#!/usr/bin/env python3
"""
tests/infra/test_verify_nextest_absent_suites.sh leaves an EXIT RECORD on
stderr whenever it exits non-zero, naming the decoded status, what was in
flight, the last nested suite's result and the failing command, and decodes a
nested suite's signal death (task 10333 / esc-9637-1).

A byte copy of the real harness and its source closure runs in a temp tree
against a stubbed scripts/verify.sh and stubbed covered suites, so each
scenario reproduces one abort shape through the harness's public seam: its
exit status, stdout and stderr.
"""

import os
import re
import select
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_ledger_env import hermetic_env  # noqa: E402  (imported after sys.path manipulation)

INFRA_DIR = Path(__file__).resolve().parent
HARNESS = "test_verify_nextest_absent_suites.sh"
SOURCE_CLOSURE = (HARNESS, "test_helpers.sh", "nextest_absent_lib.sh", "plan_capture_lib.sh")
TRIGGER = "test_verify_scope.sh"
COVERED_ROW = re.compile(
    r"^\s+_suite_is_clean_without_nextest\s+(\S+\.sh)\s+\d+\s*$", re.MULTILINE
)
CLEAN_RESULTS = 'echo "Results: 100000 passed, 0 failed"\n'
VERIFY_STUB = """\
if command -v cargo-nextest >/dev/null 2>&1; then
    echo "# verify.sh plan — nextest=1"
else
    echo "# verify.sh plan — nextest=0"
fi
"""
# `started` and `release` are FIFOs: each step blocks on the test's end of it.
WAIT_FOR_RELEASE = """\
: > "$PROBE_DIR/started"
read -r _ < "$PROBE_DIR/release"
""" + CLEAN_RESULTS
TERM_THE_HARNESS = 'kill -TERM "$PROBE_HARNESS_PID"\n' + CLEAN_RESULTS
TERM_ITSELF = "kill -TERM $$\n"
FAIL_ON_HARNESS_STDERR = """\
for _fd in /proc/$$/fd/*; do
    if [ "$(readlink "$_fd")" = "$PROBE_HARNESS_STDERR" ]; then
        echo "inherited the harness stderr as fd ${_fd##*/}"
        exit 1
    fi
done
""" + CLEAN_RESULTS
RECORD_PREFIX = f"EXIT RECORD ({HARNESS}): "
LAUNCH = 'export PROBE_HARNESS_PID=$$; exec bash "$1"'
WAIT_TIMEOUT = 300


class NextestAbsentSuitesExitRecordTests(unittest.TestCase):
    def setUp(self):
        root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix="nx-exitrec-")))
        self.fx = root / "fx"
        self.tmp = root / "tmp"
        self.probe = root / "probe"
        self.stderr_path = root / "stderr"
        infra = self.fx / "tests" / "infra"
        for directory in (infra, self.fx / "scripts", self.tmp, self.probe):
            directory.mkdir(parents=True)
        for name in SOURCE_CLOSURE:
            source = INFRA_DIR / name
            self.assertTrue(source.is_file(), f"premise: {source} is missing")
            shutil.copy2(source, infra / name)
        (self.fx / "scripts" / "verify.sh").write_text(VERIFY_STUB, encoding="utf-8")
        self.covered = COVERED_ROW.findall((infra / HARNESS).read_text(encoding="utf-8"))
        self.assertIn(TRIGGER, self.covered, f"premise: covered suites {self.covered}")
        for name in self.covered:
            self.write_suite(name, CLEAN_RESULTS)

    def write_suite(self, name, body):
        (self.fx / "tests" / "infra" / name).write_text(body, encoding="utf-8")

    def spawn(self, *, stderr_file=None, restore_signals=True, **probe_env):
        stderr = stderr_file or self.enterContext(open(self.stderr_path, "w"))
        return subprocess.Popen(
            ["bash", "-c", LAUNCH, "_", str(self.fx / "tests" / "infra" / HARNESS)],
            cwd=str(self.fx),
            env=hermetic_env(TMPDIR=str(self.tmp), PROBE_DIR=str(self.probe), **probe_env),
            stdout=subprocess.PIPE,
            stderr=stderr,
            text=True,
            restore_signals=restore_signals,
        )

    def finish(self, proc):
        """Wait for the harness; returns (rc, stdout, stderr)."""
        try:
            stdout, _ = proc.communicate(timeout=WAIT_TIMEOUT)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.communicate()
            self.fail(f"harness did not exit within {WAIT_TIMEOUT}s")
        stderr = self.stderr_path.read_text(encoding="utf-8") if self.stderr_path.exists() else ""
        self.skip_if_harness_unavailable(stdout or "")
        self.assert_no_leftover_directory()
        return proc.returncode, stdout or "", stderr

    def await_trigger_start(self, proc, started_fd):
        """Block until the trigger stub opens `started`; fail at once if the harness exits first."""
        exited_fd = os.pidfd_open(proc.pid)
        self.addCleanup(os.close, exited_fd)
        poller = select.poll()
        poller.register(started_fd, select.POLLIN)
        poller.register(exited_fd, select.POLLIN)
        ready = {fd for fd, _ in poller.poll(WAIT_TIMEOUT * 1000)}
        if started_fd in ready:
            return
        if exited_fd in ready:
            self.fail(f"harness exited rc={proc.wait()} before {TRIGGER} started")
        proc.kill()
        proc.wait()
        self.fail(f"{TRIGGER} never started")

    def skip_if_harness_unavailable(self, stdout):
        if "SKIP: harness unavailable" in stdout:
            self.skipTest("the nextest-absent harness is unavailable on this host")

    def assert_no_leftover_directory(self):
        leftovers = [p.name for p in self.tmp.iterdir() if p.is_dir()]
        self.assertEqual(leftovers, [], "the harness left a directory in its TMPDIR")

    def records(self, stderr):
        return [line for line in stderr.splitlines() if line.startswith(RECORD_PREFIX)]

    def single_record(self, stderr):
        records = self.records(stderr)
        self.assertEqual(len(records), 1, f"expected one EXIT RECORD line\nstderr:\n{stderr}")
        return records[0]

    def assert_rc(self, rc, expected, stdout, stderr):
        self.assertEqual(rc, expected, f"harness rc {rc}\nstdout:\n{stdout}\nstderr:\n{stderr}")

    def test_clean_run_writes_no_record(self):
        rc, stdout, stderr = self.finish(self.spawn())
        results = [line for line in stdout.splitlines() if line.startswith("Results:")]
        self.assertTrue(results and results[-1].endswith(" 0 failed"), f"premise: {results}")
        self.assert_rc(rc, 0, stdout, stderr)
        self.assertEqual(self.records(stderr), [])

    def test_stdout_write_failure_is_recorded(self):
        self.write_suite(TRIGGER, WAIT_FOR_RELEASE)
        started, release = self.probe / "started", self.probe / "release"
        os.mkfifo(started)
        os.mkfifo(release)
        started_fd = os.open(started, os.O_RDONLY | os.O_NONBLOCK)
        self.addCleanup(os.close, started_fd)
        # Python leaves SIGPIPE ignored; inherited, it turns the harness's next
        # stdout write into EPIPE instead of a kill: the incident's silent rc=1.
        proc = self.spawn(restore_signals=False)
        self.await_trigger_start(proc, started_fd)
        proc.stdout.close()
        # On Linux an O_RDWR open of a FIFO needs no reader; holding it open
        # keeps the release line buffered until the stub reads it.
        release_fd = os.open(release, os.O_RDWR)
        self.addCleanup(os.close, release_fd)
        os.write(release_fd, b"\n")
        try:
            proc.wait(timeout=WAIT_TIMEOUT)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
            self.fail(f"harness did not exit within {WAIT_TIMEOUT}s")
        stderr = self.stderr_path.read_text(encoding="utf-8")
        self.assert_no_leftover_directory()
        self.assert_rc(proc.returncode, 1, "<closed>", stderr)
        record = self.single_record(stderr)
        self.assertTrue(record.startswith(RECORD_PREFIX + "rc=1;"), record)
        self.assertIn(f"in flight: S-row verdict after nested suite {TRIGGER};", record)
        self.assertIn(f"last nested result: {TRIGGER} rc=0", record)
        self.assertIn("last command: echo", record)

    def test_nested_suite_does_not_inherit_the_harness_stderr(self):
        # A process a nested suite leaves behind would otherwise hold the
        # harness's real stderr open past the harness's own exit.
        self.write_suite(TRIGGER, FAIL_ON_HARNESS_STDERR)
        rc, stdout, stderr = self.finish(
            self.spawn(PROBE_HARNESS_STDERR=str(self.stderr_path.resolve()))
        )
        self.assert_rc(rc, 0, stdout, stderr)
        self.assertEqual(self.records(stderr), [])

    def test_harness_signal_death_is_recorded(self):
        self.write_suite(TRIGGER, TERM_THE_HARNESS)
        rc, stdout, stderr = self.finish(self.spawn())
        self.assert_rc(rc, 143, stdout, stderr)
        record = self.single_record(stderr)
        self.assertTrue(record.startswith(RECORD_PREFIX + "rc=143 (signal TERM);"), record)
        self.assertIn(TRIGGER, record)
        # A record write that fails must not replace the exit status.
        with open("/dev/full", "w") as full:
            rc, stdout, _ = self.finish(self.spawn(stderr_file=full))
        self.assert_rc(rc, 143, stdout, "<on /dev/full>")

    def test_nested_signal_death_is_decoded_and_distinct_from_an_abort(self):
        self.write_suite(TRIGGER, TERM_ITSELF)
        rc, stdout, stderr = self.finish(self.spawn())
        self.assert_rc(rc, 1, stdout, stderr)
        self.assertIn(
            f"{TRIGGER} FAILED under the nextest-absent harness: rc=143 (signal TERM)", stdout
        )
        self.assertTrue(
            any(line.startswith("Results:") for line in stdout.splitlines()), stdout
        )
        record = self.single_record(stderr)
        self.assertTrue(record.startswith(RECORD_PREFIX + "rc=1;"), record)
        self.assertIn("in flight: test_summary", record)


if __name__ == "__main__":
    unittest.main()
