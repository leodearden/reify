#!/usr/bin/env python3
"""
test_merge_verify_trim_units.py — stdlib unittest for leo-laptop's
_merge-verify cargo-target trim: deploy/systemd/reify-merge-verify-trim.{service,timer}
running scripts/prune-unconsulted-target.sh (task 9246).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_merge_verify_trim_units.sh.

DEPLOY CONTRACT: dark_factory's scripts/deploy/install_laptop_merge_verify_trim.sh
(task 7042) installs these files and refuses to when its step-1 checks fail;
DeployContract mirrors those checks so that refusal is caught here first.

HERMETIC: ExecStartRuns re-roots the service's own argv onto a temp worktree
base and refuses to spawn while any element still names /home/leo/src/reify.
`findmnt` (relatime) and `df` (ample free space) are PATH stubs.
"""

import configparser
import fcntl
import os
import re
import shlex
import subprocess
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
DEPLOY_DIR = REPO_ROOT / "deploy" / "systemd"
SERVICE_NAME = "reify-merge-verify-trim.service"
TIMER_NAME = "reify-merge-verify-trim.timer"
SERVICE = DEPLOY_DIR / SERVICE_NAME
TIMER = DEPLOY_DIR / TIMER_NAME
PRUNE_SCRIPT = REPO_ROOT / "scripts" / "prune-unconsulted-target.sh"
GC_SCRIPT = REPO_ROOT / "scripts" / "gc-worktree-targets.sh"

INSTALLED_PRUNE = "%h/.local/lib/reify-verify-host/prune-unconsulted-target.sh"
LAPTOP_CHECKOUT = "/home/leo/src/reify"
LAPTOP_WORKTREES = f"{LAPTOP_CHECKOUT}/.worktrees"
LANE_LOCKS = {f"{LAPTOP_WORKTREES}/_merge-verify.lock", f"{LAPTOP_WORKTREES}/.merge_verify.lock"}
PROFILES = [f"{LAPTOP_WORKTREES}/_merge-verify/target/debug",
            f"{LAPTOP_WORKTREES}/_merge-verify/target/release"]
VALUED_FLAGS = ("--window-days", "--atime-probe", "--lock", "--free-floor-gib", "--min-window-days")

DAY = 24 * 3600
ANCHOR = datetime(2026, 3, 10, tzinfo=timezone.utc).timestamp()

STUB_FINDMNT = """#!/bin/sh
echo "rw,relatime"
"""

STUB_DF = """#!/bin/sh
echo "Avail"
echo "10995116277760"
"""


def read_unit(path):
    parser = configparser.ConfigParser(interpolation=None, strict=False)
    parser.optionxform = str
    with open(path, encoding="utf-8") as unit:
        parser.read_file(unit)
    return parser


def exec_start_argv():
    return shlex.split(read_unit(SERVICE)["Service"]["ExecStart"])


def parse_prune_argv(argv):
    """(flags, positionals) of a prune-unconsulted-target.sh argv: flags maps
    each valued flag to the list of its values, in order."""
    flags, positionals, args = {}, [], iter(argv[1:])
    for arg in args:
        if arg in VALUED_FLAGS:
            flags.setdefault(arg, []).append(next(args))
        else:
            positionals.append(arg)
    return flags, positionals


class DeployContract(unittest.TestCase):
    """dark_factory install_laptop_merge_verify_trim.sh step 1: the checks the
    other classes do not already imply. File existence, and the service naming
    the installed copy, follow from UnitFiles' stronger argv[0] assertion."""

    def test_the_prune_script_is_executable(self):
        self.assertTrue(os.access(PRUNE_SCRIPT, os.X_OK))

    def test_the_prune_script_sources_nothing(self):
        sourcing = re.compile(r"^[ \t\v\f\r]*(source|\.)[ \t\v\f\r]")
        lines = PRUNE_SCRIPT.read_text(encoding="utf-8").splitlines()
        self.assertEqual([line for line in lines if sourcing.search(line)], [])

    def test_the_gc_script_knows_merge_verify(self):
        # The installer's text check only; test_gc_worktree_targets.py is the
        # behavioural guard of the exemption.
        self.assertIn("_merge-verify", GC_SCRIPT.read_text(encoding="utf-8"))


class UnitFiles(unittest.TestCase):
    def test_service_is_a_timer_activated_oneshot(self):
        service = read_unit(SERVICE)

        self.assertEqual(service["Service"]["Type"], "oneshot")
        self.assertNotIn("Install", service.sections())

    def test_exec_start_runs_the_installed_script_on_the_merge_verify_target(self):
        argv = exec_start_argv()
        flags, positionals = parse_prune_argv(argv)

        self.assertEqual(argv[0], INSTALLED_PRUNE, "nothing may run from the laptop checkout")
        self.assertGreaterEqual(int(flags["--window-days"][-1]), 3)
        if "--min-window-days" in flags:
            self.assertGreaterEqual(int(flags["--min-window-days"][-1]), 2)
        self.assertEqual(set(flags["--lock"]), LANE_LOCKS)
        self.assertEqual(positionals, PROFILES)

    def test_timer_is_persistent_owns_the_service_and_installs_into_timers(self):
        timer = read_unit(TIMER)

        self.assertTrue(timer["Timer"].get("OnCalendar", "").strip())
        self.assertEqual(timer["Timer"]["Persistent"], "true")
        self.assertEqual(timer["Timer"]["Unit"], SERVICE_NAME)
        self.assertEqual(timer["Install"]["WantedBy"], "timers.target")


def make_unit(profile, pkg, hash16, last_use):
    """One cargo unit consulted at `last_use`; returns its deps file."""
    deps_file = profile / "deps" / f"lib{pkg}-{hash16}.rlib"
    deps_file.parent.mkdir(parents=True, exist_ok=True)
    deps_file.write_text(f"deps {pkg}-{hash16}\n")
    fingerprint = profile / ".fingerprint" / f"{pkg}-{hash16}" / f"lib-{pkg}"
    fingerprint.parent.mkdir(parents=True, exist_ok=True)
    fingerprint.write_text(f"fingerprint {pkg}-{hash16}\n")
    os.utime(fingerprint, (last_use, last_use))
    return deps_file


class ExecStartRuns(unittest.TestCase):
    """The service's real argv, re-rooted onto a fixture worktree base."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-merge-verify-trim-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)
        self.worktrees = self.tmp / "worktrees"
        self.stub_dir = self.tmp / "stub-bin"
        self.stub_dir.mkdir()
        for name, body in (("findmnt", STUB_FINDMNT), ("df", STUB_DF)):
            stub = self.stub_dir / name
            stub.write_text(body)
            stub.chmod(0o755)
        self.kept, self.superseded = [], []
        for profile in ("debug", "release"):
            root = self.worktrees / "_merge-verify" / "target" / profile
            self.kept.append(make_unit(root, "anchor", "00000000000000a1", ANCHOR))
            self.kept.append(make_unit(root, "widget", "1111111111111111", ANCHOR - 1 * DAY))
            self.superseded.append(make_unit(root, "widget", "2222222222222222", ANCHOR - 5 * DAY))

    def rerooted_args(self):
        """The ExecStart arguments (argv[0] dropped: the repo's own script runs
        in its place), with the laptop worktree base swapped for the fixture."""
        args = [arg.replace(LAPTOP_WORKTREES, str(self.worktrees))
                for arg in exec_start_argv()[1:]]
        leaked = [arg for arg in args if LAPTOP_CHECKOUT in arg]
        self.assertEqual(leaked, [], "refusing to run the unit against the real checkout")
        return args

    def hold(self, lock_name):
        fd = os.open(str(self.worktrees / lock_name), os.O_RDWR | os.O_CREAT, 0o644)
        self.addCleanup(os.close, fd)
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def run_unit(self):
        args = self.rerooted_args()
        env = {k: v for k, v in os.environ.items() if not k.startswith("REIFY_TEST_")}
        env["PATH"] = f"{self.stub_dir}{os.pathsep}{env.get('PATH', '')}"
        return subprocess.run(["bash", str(PRUNE_SCRIPT), *args], capture_output=True,
                              text=True, env=env, check=False)

    def assertUntouched(self):
        self.assertEqual([str(p) for p in self.kept + self.superseded if not p.is_file()], [])

    def test_a_held_lane_lock_succeeds_without_trimming(self):
        self.hold("_merge-verify.lock")

        result = self.run_unit()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertUntouched()

    def test_a_held_compat_co_lock_succeeds_without_trimming(self):
        self.hold(".merge_verify.lock")

        result = self.run_unit()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertUntouched()

    def test_with_both_locks_free_the_superseded_units_are_trimmed_from_both_profiles(self):
        result = self.run_unit()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([str(p) for p in self.superseded if p.exists()], [])
        self.assertEqual([str(p) for p in self.kept if not p.is_file()], [])


if __name__ == "__main__":
    unittest.main()
