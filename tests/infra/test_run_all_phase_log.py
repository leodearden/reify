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
the legacy serial run covers the one serial phase. Every assertion compares
the parsed (id, name, kind, fields) tuples of phase_lines(), never a whole
sentence.

The only time bound here is the subprocess hang guard; no assertion bounds
how long anything takes.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_run_all_phase_log.sh.
"""

import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from run_all_fixture import run_all_env, write_executable  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = Path(__file__).resolve().parents[2]
RUN_ALL = REPO_ROOT / "tests" / "infra" / "run_all.sh"

RUN_TIMEOUT_S = 600

PASSING_MEMBER = """#!/usr/bin/env bash
exit 0
"""

# Fails on its first attempt and passes on the Phase-2.5 retry.
FLAKY_MEMBER = """#!/usr/bin/env bash
n=$(( $(cat "$FIXTURE_STATE/flaky-count" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$FIXTURE_STATE/flaky-count"
[ "$n" -ge 2 ]
"""

PASS, FLAKY, SERIAL = "test_pass.sh", "test_flaky.sh", "test_serial.sh"

COMPLETED_RUN_MEMBERS = {
    PASS: (PASSING_MEMBER, "pool"),
    FLAKY: (FLAKY_MEMBER, "pool"),
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


class Fixture:
    def __init__(self, add_cleanup, members):
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


if __name__ == "__main__":
    unittest.main()
