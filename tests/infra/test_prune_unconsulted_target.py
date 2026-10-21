#!/usr/bin/env python3
"""
test_prune_unconsulted_target.py — stdlib unittest for
scripts/prune-unconsulted-target.sh, the fingerprint-consult liveness trim of a
cargo profile's deps/ (task 9246; the rule itself is #8352's).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_prune_unconsulted_target.sh.

HERMETIC: `findmnt` is a PATH stub that appends its argv to a calls file and
prints $REIFY_TEST_FINDMNT_OPTIONS (default rw,relatime) as the mount's option
list; REIFY_TEST_FINDMNT_FAIL=1 makes it fail. The real mount options of the
temp filesystem never decide a case.

FIXTURE TIMES: fixed past dates, applied with os.utime to each fingerprint file
LAST, and no fixture file is opened afterwards (reading one under relatime would
move its atime, which is the very evidence under test).

FIXTURE RULE (from tests/infra/test_refresh_warm_base.sh Block LIVE): every
profile with a dead unit also carries a unit consulted at its ANCHOR. Without
one the dead unit is the newest consult, becomes the anchor and is correctly
kept, so a 'gone' assertion would fail for the wrong reason.
"""

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "prune-unconsulted-target.sh"

DAY = 24 * 3600
ANCHOR = datetime(2026, 3, 10, tzinfo=timezone.utc).timestamp()

STUB_FINDMNT = """#!/bin/sh
echo "findmnt $*" >> "${REIFY_TEST_CALLS_FILE:-/dev/null}"
if [ "${REIFY_TEST_FINDMNT_FAIL:-}" = "1" ]; then
    echo "findmnt: SIMULATED failure (REIFY_TEST_FINDMNT_FAIL=1)" >&2
    exit 1
fi
echo "${REIFY_TEST_FINDMNT_OPTIONS-rw,relatime}"
exit 0
"""


def make_unit(profile, pkg, hash16, last_use, deps_names):
    """One cargo unit: <profile>/.fingerprint/<pkg>-<hash16>/lib-<pkg> last
    read and written at `last_use`, plus each named file under <profile>/deps.
    Returns the deps paths."""
    deps = profile / "deps"
    deps.mkdir(parents=True, exist_ok=True)
    paths = []
    for name in deps_names:
        path = deps / name
        path.write_text(f"deps {name}\n")
        os.utime(path, (last_use, last_use))
        paths.append(path)
    fingerprint = profile / ".fingerprint" / f"{pkg}-{hash16}" / f"lib-{pkg}"
    fingerprint.parent.mkdir(parents=True, exist_ok=True)
    fingerprint.write_text(f"fingerprint {pkg}-{hash16}\n")
    os.utime(fingerprint, (last_use, last_use))
    return paths


def make_anchor(profile, anchor=ANCHOR):
    """The unit consulted at the profile's anchor (FIXTURE RULE)."""
    return make_unit(profile, "anchor", "00000000000000a1", anchor,
                     ["libanchor-00000000000000a1.rlib"])


def make_core(profile):
    """The core shape: one stem with a live variant (anchor - 1d) and a
    superseded one (anchor - 5d), each extensionless + .d, plus a hashed deps
    file whose hash has no fingerprint dir at all."""
    anchor = make_anchor(profile)
    live = make_unit(profile, "widget", "1111111111111111", ANCHOR - 1 * DAY,
                     ["widget-1111111111111111", "widget-1111111111111111.d"])
    superseded = make_unit(profile, "widget", "2222222222222222", ANCHOR - 5 * DAY,
                           ["widget-2222222222222222", "widget-2222222222222222.d"])
    orphan = profile / "deps" / "liborphan-3333333333333333.rlib"
    orphan.write_text("no fingerprint dir for this hash\n")
    os.utime(orphan, (ANCHOR - 30 * DAY, ANCHOR - 30 * DAY))
    return {"kept": anchor + live + [orphan], "superseded": superseded}


class PruneHarness(unittest.TestCase):
    """A temp target dir with debug/ and release/ slots, a stub bin first on
    PATH and an empty TMPDIR the script's summary file must not outlive."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-prune-unconsulted-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)
        self.stub_dir = self.tmp / "stub-bin"
        self.stub_dir.mkdir()
        self.write_stub("findmnt", STUB_FINDMNT)
        self.calls_file = self.tmp / "calls.log"
        self.calls_file.touch()
        self.scratch_tmp = self.tmp / "scratch-tmp"
        self.scratch_tmp.mkdir()
        self.target = self.tmp / "target"
        self.debug = self.target / "debug"
        self.release = self.target / "release"

    def write_stub(self, name, body):
        stub = self.stub_dir / name
        stub.write_text(body)
        stub.chmod(0o755)

    def run_prune(self, *args, script=SCRIPT, env=None):
        run_env = {k: v for k, v in os.environ.items() if not k.startswith("REIFY_TEST_")}
        run_env["PATH"] = f"{self.stub_dir}{os.pathsep}{run_env.get('PATH', '')}"
        run_env["REIFY_TEST_CALLS_FILE"] = str(self.calls_file)
        run_env["TMPDIR"] = str(self.scratch_tmp)
        run_env.update(env or {})
        return subprocess.run(["bash", str(script), *map(str, args)],
                              capture_output=True, text=True, env=run_env, check=False)

    def tree(self):
        if not self.target.exists():
            return []
        return sorted(str(p.relative_to(self.target))
                      for p in self.target.rglob("*") if p.is_file())

    def findmnt_calls(self):
        return [line for line in self.calls_file.read_text().splitlines()
                if line.startswith("findmnt")]

    def assertPresent(self, paths):
        missing = [str(p) for p in paths if not p.is_file()]
        self.assertEqual(missing, [], "files that should have survived are gone")

    def assertAbsent(self, paths):
        present = [str(p) for p in paths if p.exists()]
        self.assertEqual(present, [], "files that should have been pruned remain")

    def assertProbed(self, probe):
        pattern = re.compile(rf"^findmnt( .*)? -T {re.escape(str(probe))}( |$)")
        self.assertTrue(any(pattern.search(c) for c in self.findmnt_calls()),
                        f"no `findmnt -T {probe}` in {self.findmnt_calls()}")

    def assertNotProbed(self, probe):
        pattern = re.compile(rf" -T {re.escape(str(probe))}( |$)")
        self.assertFalse(any(pattern.search(c) for c in self.findmnt_calls()),
                         f"findmnt was asked about {probe}: {self.findmnt_calls()}")

    def assertPruneLine(self, stderr, profile, files, victim_bytes=None):
        bytes_re = r"\d+" if victim_bytes is None else str(victim_bytes)
        pattern = (rf"prune deps={re.escape(str(profile / 'deps'))} "
                   rf"files={files} victim_bytes={bytes_re}( |$)")
        self.assertRegex(stderr, re.compile(pattern, re.M))


class CorePredicate(PruneHarness):
    def test_superseded_variant_is_pruned_in_every_profile_and_nothing_else(self):
        debug = make_core(self.debug)
        release = make_core(self.release)
        victim_bytes = sum(p.stat().st_size for p in debug["superseded"])

        result = self.run_prune("--window-days", 3, self.debug, self.release)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertAbsent(debug["superseded"] + release["superseded"])
        self.assertPresent(debug["kept"] + release["kept"])
        self.assertEqual(result.stdout, "")
        self.assertPruneLine(result.stderr, self.debug, 2, victim_bytes)
        self.assertPruneLine(result.stderr, self.release, 2, victim_bytes)
        self.assertEqual(list(self.scratch_tmp.iterdir()), [],
                         "the summary temp file outlived the run")

    def test_fingerprint_dirs_are_never_pruned(self):
        make_core(self.debug)
        fingerprints = sorted(p for p in (self.debug / ".fingerprint").rglob("*") if p.is_file())

        result = self.run_prune("--window-days", 3, self.debug)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertPresent(fingerprints)

    def test_window_edge_keeps_units_inside_it_and_prunes_units_outside_it(self):
        make_anchor(self.debug)
        inside = make_unit(self.debug, "inside", "4444444444444444", ANCHOR - 2 * DAY,
                           ["libinside-4444444444444444.rlib"])
        outside = make_unit(self.debug, "outside", "5555555555555555", ANCHOR - 4 * DAY,
                            ["liboutside-5555555555555555.rlib"])

        result = self.run_prune("--window-days", 3, self.debug)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertPresent(inside)
        self.assertAbsent(outside)

    def test_each_profile_is_anchored_on_its_own_newest_consult(self):
        make_core(self.debug)
        release_anchor = ANCHOR - 20 * DAY
        anchor = make_anchor(self.release, release_anchor)
        live = make_unit(self.release, "rel", "6666666666666666", release_anchor - 1 * DAY,
                         ["librel-6666666666666666.rlib"])
        dead = make_unit(self.release, "rel", "7777777777777777", release_anchor - 5 * DAY,
                         ["librel-7777777777777777.rlib"])

        result = self.run_prune("--window-days", 3, self.debug, self.release)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertPresent(anchor + live)
        self.assertAbsent(dead)


class AtimeGuard(PruneHarness):
    def test_a_mount_without_atime_evidence_skips_the_trim(self):
        for label, env in (("noatime", {"REIFY_TEST_FINDMNT_OPTIONS": "rw,noatime"}),
                           ("findmnt fails", {"REIFY_TEST_FINDMNT_FAIL": "1"}),
                           ("empty option list", {"REIFY_TEST_FINDMNT_OPTIONS": ""})):
            with self.subTest(label):
                shutil.rmtree(self.target, ignore_errors=True)
                make_core(self.debug)
                before = self.tree()

                result = self.run_prune("--window-days", 3, self.debug, env=env)

                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.tree(), before)
                self.assertRegex(result.stderr, re.compile("skip", re.I))

    def test_nodiratime_is_not_noatime(self):
        core = make_core(self.debug)

        result = self.run_prune("--window-days", 3, self.debug,
                                env={"REIFY_TEST_FINDMNT_OPTIONS": "rw,nodiratime"})

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertAbsent(core["superseded"])

    def test_the_probe_defaults_to_the_profile_dir(self):
        make_core(self.debug)

        result = self.run_prune("--window-days", 3, self.debug)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertProbed(self.debug)

    def test_atime_probe_names_the_mount_asked_about(self):
        make_core(self.debug)
        probe = self.tmp / "advancing"
        probe.mkdir()

        result = self.run_prune("--window-days", 3, "--atime-probe", probe, self.debug)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertProbed(probe)
        self.assertNotProbed(self.debug)


class SkippedProfiles(PruneHarness):
    def test_a_profile_without_fingerprints_is_skipped_and_the_others_still_pruned(self):
        debug = make_core(self.debug)
        (self.release / "deps").mkdir(parents=True)
        stray = self.release / "deps" / "libdead-abababababababab.rlib"
        stray.write_text("dead-looking\n")
        os.utime(stray, (ANCHOR - 300 * DAY, ANCHOR - 300 * DAY))

        result = self.run_prune("--window-days", 3, self.debug, self.release)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertPresent([stray])
        self.assertAbsent(debug["superseded"])
        self.assertNotProbed(self.release)
        self.assertRegex(result.stderr, re.compile("skip", re.I))

    def test_a_profile_without_deps_is_skipped(self):
        debug = make_core(self.debug)
        (self.release / ".fingerprint").mkdir(parents=True)

        result = self.run_prune("--window-days", 3, self.debug, self.release)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertAbsent(debug["superseded"])
        self.assertNotProbed(self.release)

    def test_an_absent_profile_dir_is_skipped_and_the_others_still_pruned(self):
        debug = make_core(self.debug)

        result = self.run_prune("--window-days", 3, self.debug, self.release)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.release.exists())
        self.assertAbsent(debug["superseded"])
        self.assertRegex(result.stderr, re.compile("skip", re.I))


class Usage(PruneHarness):
    def test_usage_errors_exit_2_before_touching_the_tree(self):
        cases = (
            ("no --window-days", [self.debug]),
            ("non-integer window", ["--window-days", "abc", self.debug]),
            ("fractional window", ["--window-days", "1.5", self.debug]),
            ("zero window", ["--window-days", 0, self.debug]),
            ("negative window", ["--window-days", "-1", self.debug]),
            ("empty window", ["--window-days", "", self.debug]),
            ("--window-days without a value", ["--window-days"]),
            ("no PROFILE_DIR", ["--window-days", 3]),
            ("unknown flag", ["--window-days", 3, "--bogus", self.debug]),
        )
        make_core(self.debug)
        before = self.tree()
        for label, args in cases:
            with self.subTest(label):
                result = self.run_prune(*args)

                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(self.tree(), before)
                self.assertEqual(self.findmnt_calls(), [])


class Standalone(PruneHarness):
    def test_a_lone_copy_of_the_script_prunes_the_same_files(self):
        """The laptop installs this one file alone, with no sibling scripts."""
        lone_dir = self.tmp / "lone"
        lone_dir.mkdir()
        lone = lone_dir / SCRIPT.name
        shutil.copy2(SCRIPT, lone)
        debug = make_core(self.debug)
        release = make_core(self.release)

        result = self.run_prune("--window-days", 3, self.debug, self.release, script=lone)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(sorted(p.name for p in lone_dir.iterdir()), [SCRIPT.name])
        self.assertAbsent(debug["superseded"] + release["superseded"])
        self.assertPresent(debug["kept"] + release["kept"])


if __name__ == "__main__":
    unittest.main()
