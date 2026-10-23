#!/usr/bin/env python3
"""
test_run_all_member_lifetime.py — run_all.sh bounds how long a member lives.

EXECUTES the real tests/infra/run_all.sh on tempdir fixture INFRA_DIRs (the
recipe of test_run_all_timing.py: fixture classification manifest, private
pool lock, PSI disabled, flaky ledger isolated, plus a private TMPDIR) and
checks that:
  - a member that outlives REIFY_RUN_ALL_MEMBER_TIMEOUT is killed together
    with its process group, recorded as FAILED with a MEMBER_TIMEOUT line that
    names it, and not retried;
  - a member that ignores SIGTERM is KILLed after
    REIFY_RUN_ALL_MEMBER_KILL_GRACE;
  - a group SIGTERM to run_all (the merge envelope's shape) still reaches a
    member that ignores it;
  - a malformed budget knob is a usage error (exit 64) before any member runs.

Every fixture process carries the fixture's unique path or token in its
/proc cmdline, so a survivor is found by scanning /proc, and each case
SIGKILLs whatever still carries one at cleanup: a failing run leaves nothing
behind on the host.

The only time bounds here are hang guards: the subprocess timeout and the
settle window, which tells "reaped within seconds" from "lived out its
fixture's 300s", and never asserts that anything is fast.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_run_all_member_lifetime.sh.
"""

import contextlib
import os
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_ledger_env import hermetic_env  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = Path(__file__).resolve().parents[2]
RUN_ALL = REPO_ROOT / "tests" / "infra" / "run_all.sh"

# Long enough that the trivial members finish far inside it on a loaded host,
# short against the 300s the hanging fixtures would otherwise live.
BUDGET_S = 20
GRACE_S = 2
SETTLE_S = 60
START_WAIT_S = 300
RUN_TIMEOUT_S = 600

# Each member appends one line per attempt to $FIXTURE_STATE/<name>.attempts.
# @TOKEN@ is replaced with the fixture's unique token.
MEMBERS = {
    "test_hang.sh": r"""#!/usr/bin/env bash
printf 'attempt\n' >> "$FIXTURE_STATE/$(basename "$0").attempts"
bash -c 'exec -a @TOKEN@ sleep 300' &
bash -c 'exec -a @TOKEN@ sleep 300'
""",
    "test_swallow.sh": r"""#!/usr/bin/env bash
trap '' TERM
printf 'attempt\n' >> "$FIXTURE_STATE/$(basename "$0").attempts"
for _ in $(seq 300); do sleep 1; done
""",
    "test_self124.sh": r"""#!/usr/bin/env bash
printf 'attempt\n' >> "$FIXTURE_STATE/$(basename "$0").attempts"
exit 124
""",
    "test_ok.sh": r"""#!/usr/bin/env bash
printf 'attempt\n' >> "$FIXTURE_STATE/$(basename "$0").attempts"
exit 0
""",
}

TIMEOUT_LINE_RE = re.compile(
    r"^MEMBER_TIMEOUT FAIL member=(?P<member>\S+) budget_s=(?P<budget>\d+) "
    r"elapsed_s=(?P<elapsed>\d+) rc=(?P<rc>\d+)$",
    re.M,
)


def write_executable(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def _strays(*needles):
    """(pid, cmdline) of every live process but this one whose cmdline holds a needle."""
    own = os.getpid()
    wanted = [n.encode() for n in needles]
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit() or int(entry) == own:
            continue
        try:
            cmdline = Path("/proc", entry, "cmdline").read_bytes()
        except OSError:
            continue
        if any(n in cmdline for n in wanted):
            found.append((int(entry), cmdline.replace(b"\0", b" ").decode(errors="replace").strip()))
    return found


def timeout_lines(text, member):
    return [m for m in TIMEOUT_LINE_RE.finditer(text) if m["member"] == member]


def failed_detail(stdout, member):
    """The text of <member>'s FAILED-DETAIL block, or None when it has none."""
    match = re.search(
        rf"^--- FAILED-DETAIL: {re.escape(member)} ---\n(.*?)^--- END FAILED-DETAIL: {re.escape(member)} ---$",
        stdout,
        re.M | re.S,
    )
    return match[1] if match else None


def bare_failed_names(stdout):
    lines = [line for line in stdout.splitlines() if line.startswith("FAILED ")]
    return [line.split()[1:] for line in lines]


class Fixture:
    def __init__(self, add_cleanup, members):
        self.add_cleanup = add_cleanup
        self.root = Path(tempfile.mkdtemp(prefix="ra-member-lifetime-"))
        self.token = f"ra-member-lifetime-{uuid.uuid4().hex}"
        add_cleanup(shutil.rmtree, self.root, True)
        add_cleanup(self.kill_strays)
        self.infra = self.root / "infra"
        self.state = self.root / "state"
        self.tmp = self.root / "tmp"
        for d in (self.infra, self.state, self.tmp):
            d.mkdir()
        for name in members:
            write_executable(self.infra / name, MEMBERS[name].replace("@TOKEN@", self.token))
        self.manifest = self.root / "classification.manifest"
        self.manifest.write_text("".join(f"{name} pool\n" for name in members))

    def env(self, knobs):
        env = {k: v for k, v in hermetic_env().items() if not k.startswith("REIFY_RUN_ALL_")}
        env.update(
            RUN_ALL_CLASSIFICATION_MANIFEST=str(self.manifest),
            REIFY_RUN_ALL_POOL_LOCK=str(self.root / "pool.lock"),
            REIFY_RUN_ALL_POOL_PSI_DISABLE="1",
            REIFY_RUN_ALL_FLAKY_LEDGER=str(self.root / "flaky-ledger.jsonl"),
            TMPDIR=str(self.tmp),
            FIXTURE_STATE=str(self.state),
        )
        env.update(knobs)
        return env

    def argv(self):
        return ["bash", str(RUN_ALL), str(self.infra)]

    def run(self, **knobs):
        return subprocess.run(
            self.argv(),
            env=self.env(knobs),
            capture_output=True,
            text=True,
            errors="surrogateescape",
            timeout=RUN_TIMEOUT_S,
        )

    def spawn(self, **knobs):
        """run_all in a new session (its own process group), stdout+stderr merged."""
        proc = subprocess.Popen(
            self.argv(), env=self.env(knobs), stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, errors="surrogateescape", start_new_session=True,
        )

        def stop():
            if proc.poll() is None:
                with contextlib.suppress(ProcessLookupError, PermissionError):
                    os.killpg(proc.pid, signal.SIGKILL)
            with contextlib.suppress(subprocess.TimeoutExpired):
                proc.communicate(timeout=30)

        self.add_cleanup(stop)
        return proc

    def attempts(self, name):
        path = self.state / f"{name}.attempts"
        return len(path.read_text().splitlines()) if path.exists() else 0

    def reset_attempts(self):
        for path in self.state.glob("*.attempts"):
            path.unlink()

    def wait_for_attempt(self, name, proc):
        """True once <name> has started; False if run_all exits or the hang guard expires first."""
        deadline = time.monotonic() + START_WAIT_S
        while time.monotonic() < deadline:
            if self.attempts(name) > 0:
                return True
            if proc.poll() is not None:
                return False
            time.sleep(0.2)
        return False

    def strays(self):
        return _strays(str(self.root) + os.sep, self.token)

    def settle(self):
        """The fixture's surviving processes once they are gone or SETTLE_S has passed."""
        deadline = time.monotonic() + SETTLE_S
        while True:
            strays = self.strays()
            if not strays or time.monotonic() >= deadline:
                return strays
            time.sleep(0.5)

    def kill_strays(self):
        for pid, _ in self.strays():
            with contextlib.suppress(ProcessLookupError, PermissionError):
                os.kill(pid, signal.SIGKILL)


class PoolPathBudgetTests(unittest.TestCase):
    """Four pool members under a short budget: one hangs, one ignores TERM."""

    @classmethod
    def setUpClass(cls):
        cls.fixture = Fixture(
            cls.addClassCleanup, ["test_hang.sh", "test_ok.sh", "test_self124.sh", "test_swallow.sh"]
        )
        cls.run_all = cls.fixture.run(
            REIFY_RUN_ALL_POOL_CONCURRENCY="4",
            REIFY_RUN_ALL_MEMBER_TIMEOUT=str(BUDGET_S),
            REIFY_RUN_ALL_MEMBER_KILL_GRACE=str(GRACE_S),
        )
        cls.strays_after_settle = cls.fixture.settle()

    def output(self):
        return self.run_all.stdout + self.run_all.stderr

    def test_run_fails_with_three_failed(self):
        self.assertEqual(self.run_all.returncode, 1, self.output())
        self.assertIn("=== Summary: 4 discovered, 3 failed ===", self.run_all.stdout)

    def test_failed_names_and_the_passing_member(self):
        self.assertEqual(
            bare_failed_names(self.run_all.stdout),
            [["test_hang.sh", "test_self124.sh", "test_swallow.sh"]],
            self.output(),
        )
        self.assertIn("RESULT: PASS (test_ok.sh)", self.run_all.stdout)

    def assert_timed_out(self, member, rc):
        detail = failed_detail(self.run_all.stdout, member)
        self.assertIsNotNone(detail, f"no FAILED-DETAIL block for {member}\n{self.output()}")
        lines = timeout_lines(detail, member)
        self.assertEqual(len(lines), 1, f"MEMBER_TIMEOUT lines for {member}\n{self.output()}")
        (line,) = lines
        self.assertEqual(int(line["budget"]), BUDGET_S)
        self.assertGreaterEqual(int(line["elapsed"]), BUDGET_S)
        self.assertEqual(int(line["rc"]), rc)

    def test_hanging_member_is_attributed_to_its_budget(self):
        self.assert_timed_out("test_hang.sh", 124)

    def test_term_swallower_is_killed_after_the_grace(self):
        self.assert_timed_out("test_swallow.sh", 137)

    def test_a_member_exiting_124_on_its_own_is_not_a_timeout(self):
        self.assertEqual(timeout_lines(self.output(), "test_self124.sh"), [])

    def test_timeout_is_named_on_stderr(self):
        self.assertIn("ERROR: run_all.sh: member test_hang.sh exceeded", self.run_all.stderr)

    def test_timed_out_members_are_not_retried(self):
        self.assertEqual(self.fixture.attempts("test_hang.sh"), 1)
        self.assertEqual(self.fixture.attempts("test_swallow.sh"), 1)

    def test_ordinary_failure_still_gets_its_retry(self):
        self.assertEqual(self.fixture.attempts("test_self124.sh"), 2)

    def test_no_member_process_survives(self):
        self.assertEqual(self.strays_after_settle, [])


class StreamPathBudgetTests(unittest.TestCase):
    """The legacy serial path streams each member live; the timeout line lands in its block."""

    @classmethod
    def setUpClass(cls):
        cls.fixture = Fixture(cls.addClassCleanup, ["test_hang.sh", "test_ok.sh"])
        cls.run_all = cls.fixture.run(
            REIFY_RUN_ALL_POOL_DISABLE="1",
            REIFY_RUN_ALL_MEMBER_TIMEOUT=str(BUDGET_S),
            REIFY_RUN_ALL_MEMBER_KILL_GRACE=str(GRACE_S),
        )
        cls.strays_after_settle = cls.fixture.settle()

    def output(self):
        return self.run_all.stdout + self.run_all.stderr

    def test_run_fails_naming_the_hung_member(self):
        self.assertEqual(self.run_all.returncode, 1, self.output())
        self.assertEqual(bare_failed_names(self.run_all.stdout), [["test_hang.sh"]], self.output())

    def test_timeout_line_is_inside_the_live_block(self):
        lines = self.run_all.stdout.splitlines()
        self.assertIn("--- Running: test_hang.sh ---", lines, self.output())
        start = lines.index("--- Running: test_hang.sh ---")
        self.assertIn("  RESULT: FAIL (test_hang.sh)", lines[start:], self.output())
        end = lines.index("  RESULT: FAIL (test_hang.sh)", start)
        block = "\n".join(lines[start:end])
        found = timeout_lines(block, "test_hang.sh")
        self.assertEqual(len(found), 1, self.output())
        self.assertEqual(int(found[0]["rc"]), 124)

    def test_no_member_process_survives(self):
        self.assertEqual(self.strays_after_settle, [])


class OuterGroupTermTests(unittest.TestCase):
    """The merge envelope TERMs run_all's whole process group."""

    def test_group_term_reaches_a_member_that_ignores_it(self):
        fixture = Fixture(self.addCleanup, ["test_swallow.sh"])
        proc = fixture.spawn(
            REIFY_RUN_ALL_POOL_CONCURRENCY="1",
            REIFY_RUN_ALL_MEMBER_TIMEOUT="600",
            REIFY_RUN_ALL_MEMBER_KILL_GRACE=str(GRACE_S),
        )
        if not fixture.wait_for_attempt("test_swallow.sh", proc):
            out = proc.communicate(timeout=RUN_TIMEOUT_S)[0] if proc.poll() is not None else ""
            self.fail(f"test_swallow.sh never started\n{out}")
        os.killpg(proc.pid, signal.SIGTERM)
        out, _ = proc.communicate(timeout=RUN_TIMEOUT_S)
        self.assertIn("INTERRUPTED", out)
        self.assertEqual(fixture.settle(), [], out)


class KnobValidationTests(unittest.TestCase):
    def test_malformed_budget_knob_exits_64_before_any_member(self):
        fixture = Fixture(self.addCleanup, ["test_ok.sh"])
        for knob in ("REIFY_RUN_ALL_MEMBER_TIMEOUT", "REIFY_RUN_ALL_MEMBER_KILL_GRACE"):
            for value in ("abc", "0", "-5", "unlimited"):
                with self.subTest(knob=knob, value=value):
                    fixture.reset_attempts()
                    run = fixture.run(**{knob: value})
                    self.assertEqual(run.returncode, 64, run.stdout + run.stderr)
                    self.assertIn(knob, run.stderr)
                    self.assertEqual(fixture.attempts("test_ok.sh"), 0)


if __name__ == "__main__":
    unittest.main()
