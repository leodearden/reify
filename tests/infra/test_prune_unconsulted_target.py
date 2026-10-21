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

import fcntl
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
REFRESH = REPO_ROOT / "scripts" / "refresh-warm-base.sh"

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

# df stub: the Nth call prints the Nth byte value of $REIFY_TEST_DF_SEQ_FILE (one
# per line), repeating the last once the sequence is exhausted.
STUB_DF = """#!/bin/sh
echo "df $*" >> "$REIFY_TEST_CALLS_FILE"
if [ "${REIFY_TEST_DF_FAIL:-}" = "1" ]; then
    echo "df: SIMULATED failure (REIFY_TEST_DF_FAIL=1)" >&2
    exit 1
fi
n=$(grep -c '^df ' "$REIFY_TEST_CALLS_FILE")
total=$(wc -l < "$REIFY_TEST_DF_SEQ_FILE")
[ "$n" -le "$total" ] || n=$total
echo "Avail"
sed -n "${n}p" "$REIFY_TEST_DF_SEQ_FILE"
"""

GIB = 1024 ** 3


def write_executable(path, body):
    path.write_text(body)
    path.chmod(0o755)


def env_without(*prefixes):
    return {k: v for k, v in os.environ.items() if not k.startswith(prefixes)}


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
        self.write_stub("df", STUB_DF)
        self.calls_file = self.tmp / "calls.log"
        self.calls_file.touch()
        self.scratch_tmp = self.tmp / "scratch-tmp"
        self.scratch_tmp.mkdir()
        self.target = self.tmp / "target"
        self.debug = self.target / "debug"
        self.release = self.target / "release"

    def write_stub(self, name, body):
        write_executable(self.stub_dir / name, body)

    def run_prune(self, *args, script=SCRIPT, env=None):
        run_env = env_without("REIFY_TEST_", "GIT_")
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
        return self.calls_of("findmnt")

    def calls_of(self, tool):
        return [line for line in self.calls_file.read_text().splitlines()
                if line.startswith(f"{tool} ")]

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


class LaneLocks(PruneHarness):
    """--lock FILE...: dark_factory's lane lock and compat co-lock. A held lock
    is a normal contention and exits 0 having read nothing; a lock that cannot
    be opened is a misconfiguration and fails closed."""

    def setUp(self):
        super().setUp()
        self.worktree_base = self.tmp / "worktrees"
        self.worktree_base.mkdir()
        self.lane_lock = self.worktree_base / "_merge-verify.lock"
        self.compat_lock = self.worktree_base / ".merge_verify.lock"
        self.debug_core = make_core(self.debug)
        self.release_core = make_core(self.release)

    def hold(self, lock):
        """Take LOCK_EX on `lock` for the rest of the test, as dark_factory does."""
        fd = os.open(str(lock), os.O_RDWR | os.O_CREAT, 0o644)
        self.addCleanup(os.close, fd)
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def run_locked(self, *locks):
        lock_args = [arg for lock in locks for arg in ("--lock", lock)]
        return self.run_prune("--window-days", 3, *lock_args, self.debug, self.release)

    def assertHeldLockSkips(self, held):
        self.hold(held)
        before = self.tree()

        result = self.run_locked(self.lane_lock, self.compat_lock)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.tree(), before)
        self.assertIn(str(held), result.stderr)
        self.assertEqual(self.findmnt_calls(), [], "the tree was read under a held lock")

    def test_a_held_lane_lock_exits_0_having_read_and_deleted_nothing(self):
        self.assertHeldLockSkips(self.lane_lock)

    def test_a_held_compat_co_lock_exits_0_having_read_and_deleted_nothing(self):
        self.assertHeldLockSkips(self.compat_lock)

    def test_free_locks_let_the_trim_run_and_absent_lock_files_are_created(self):
        result = self.run_locked(self.lane_lock, self.compat_lock)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertAbsent(self.debug_core["superseded"] + self.release_core["superseded"])
        self.assertPresent(self.debug_core["kept"] + self.release_core["kept"])
        self.assertTrue(self.lane_lock.is_file())
        self.assertTrue(self.compat_lock.is_file())

    def test_an_unopenable_lock_fails_closed_naming_it(self):
        unopenable = self.tmp / "no-such-dir" / "_merge-verify.lock"
        before = self.tree()

        result = self.run_locked(unopenable, self.compat_lock)

        self.assertNotEqual(result.returncode, 0)
        self.assertNotEqual(result.returncode, 2, "a lock fault is not a usage error")
        self.assertEqual(self.tree(), before)
        self.assertIn(str(unopenable), result.stderr)
        self.assertEqual(self.findmnt_calls(), [])


class FreeSpaceFloor(PruneHarness):
    """--free-floor-gib N --min-window-days M: while free space is below the
    floor, tighten the window a day at a time, never below M."""

    def setUp(self):
        super().setUp()
        self.anchor = make_anchor(self.debug)
        self.units = {
            age: make_unit(self.debug, f"aged{tag}", f"a00000000000000{tag}",
                           ANCHOR - age * DAY, [f"libaged{tag}-a00000000000000{tag}.rlib"])
            for age, tag in ((6, "6"), (4.5, "5"), (3.5, "4"), (2.5, "3"), (1.5, "2"))
        }
        self.df_seq = self.tmp / "df-seq"

    def run_floor(self, *floor_args, df_gib=(), env=None):
        self.df_seq.write_text("".join(f"{int(g * GIB)}\n" for g in df_gib))
        run_env = {"REIFY_TEST_DF_SEQ_FILE": str(self.df_seq), **(env or {})}
        return self.run_prune("--window-days", 5, *floor_args, self.debug, env=run_env)

    def assertPruned(self, gone_ages):
        self.assertAbsent([p for age in gone_ages for p in self.units[age]])
        kept = [p for age, paths in self.units.items() if age not in gone_ages for p in paths]
        self.assertPresent(kept + self.anchor)

    def test_without_a_floor_the_window_stays_put_and_df_is_never_asked(self):
        result = self.run_floor(df_gib=[10])

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertPruned({6})
        self.assertEqual(self.calls_of("df"), [])

    def test_below_the_floor_the_window_tightens_down_to_but_not_past_the_minimum(self):
        result = self.run_floor("--free-floor-gib", 50, "--min-window-days", 2, df_gib=[10])

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertPruned({6, 4.5, 3.5, 2.5})
        df_calls = self.calls_of("df")
        self.assertTrue(df_calls)
        for call in df_calls:
            self.assertTrue(call.endswith(f" {self.debug}"), call)

    def test_tightening_stops_once_the_floor_is_met(self):
        result = self.run_floor("--free-floor-gib", 50, "--min-window-days", 2, df_gib=[10, 100])

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertPruned({6, 4.5})
        windows = re.findall(r"^.*prune deps=.* window_days=(\d+)", result.stderr, re.M)
        self.assertEqual(windows, ["5", "4"])

    def test_a_failed_measurement_never_deletes_more(self):
        result = self.run_floor("--free-floor-gib", 50, "--min-window-days", 2,
                                env={"REIFY_TEST_DF_FAIL": "1"})

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertPruned({6})

    def test_floor_usage_errors_exit_2_before_touching_the_tree(self):
        cases = (
            ("floor without minimum", ["--free-floor-gib", 50]),
            ("minimum without floor", ["--min-window-days", 2]),
            ("minimum above the window", ["--free-floor-gib", 50, "--min-window-days", 6]),
            ("zero minimum", ["--free-floor-gib", 50, "--min-window-days", 0]),
            ("zero floor", ["--free-floor-gib", 0, "--min-window-days", 2]),
            ("non-integer floor", ["--free-floor-gib", "lots", "--min-window-days", 2]),
        )
        before = self.tree()
        for label, floor_args in cases:
            with self.subTest(label):
                result = self.run_floor(*floor_args, df_gib=[10])

                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(self.tree(), before)
                self.assertEqual(self.findmnt_calls(), [])
                self.assertEqual(self.calls_of("df"), [])


# A stand-in sibling for refresh-warm-base.sh: records its argv, one call per
# line, tab-separated, and prunes nothing.
RECORDING_PRUNE = """#!/bin/sh
printf '%s\t' "$@" >> "$REIFY_TEST_PRUNE_RECORD"
printf '\n' >> "$REIFY_TEST_PRUNE_RECORD"
exit 0
"""


class RefreshDelegation(PruneHarness):
    """refresh-warm-base.sh's Step 3b prune comes ONLY from its sibling
    prune-unconsulted-target.sh: with a no-op sibling nothing is pruned, and
    with no sibling the refresh fails closed rather than skipping the prune."""

    def setUp(self):
        super().setUp()
        real_cp = shutil.which("cp")
        self.write_stub("cp", f"""#!/usr/bin/env bash
args=()
for a in "$@"; do
    [ "$a" = "--reflink=always" ] && continue
    args+=("$a")
done
exec {real_cp} "${{args[@]}}"
""")
        self.lane = self.make_git_lane(self.tmp / "lane")
        self.advancing = self.lane / "advancing"
        make_anchor(self.advancing / "debug")
        self.dead = make_unit(self.advancing / "debug", "dead", "deadbeef00009246",
                              ANCHOR - 37 * DAY, ["libdead-deadbeef00009246.rlib"])
        self.base = self.tmp / "base"
        self.scripts_dir = self.tmp / "scripts"
        self.scripts_dir.mkdir()
        self.refresh = self.scripts_dir / REFRESH.name
        shutil.copy2(REFRESH, self.refresh)
        self.record = self.tmp / "prune-record"
        self.record.touch()

    def git(self, *args):
        return subprocess.run(["git", *args], env=env_without("GIT_"),
                              check=True, capture_output=True, text=True).stdout.strip()

    def make_git_lane(self, lane):
        """A clean worktree whose HEAD is on main, with an untracked advancing/
        (mirrors mk_git_advancing in tests/infra/test_refresh_warm_base.sh)."""
        lane.mkdir()
        (lane / ".placeholder").write_text("placeholder\n")
        self.git("init", "-q", "-b", "main", str(lane))
        self.git("-C", str(lane), "add", "--", ".placeholder")
        self.git("-C", str(lane), "-c", "user.email=warm-lane-test@localhost",
                 "-c", "user.name=Warm Lane Test", "-c", "commit.gpgsign=false",
                 "commit", "-q", "--no-verify", "-m", "fixture: hermetic advancing lane")
        (lane / "advancing").mkdir()
        return lane

    def run_refresh(self):
        head = self.git("-C", str(self.lane), "rev-parse", "HEAD")
        return self.run_prune(self.advancing, self.base, "--landed-commit", head,
                              script=self.refresh,
                              env={"REIFY_TEST_PRUNE_RECORD": str(self.record)})

    def partials(self):
        return sorted(self.tmp.glob("base.gen.*.partial"))

    def test_the_prune_is_the_siblings_and_nothing_else_prunes(self):
        write_executable(self.scripts_dir / SCRIPT.name, RECORDING_PRUNE)

        result = self.run_refresh()

        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.record.read_text().splitlines()
        self.assertEqual(len(calls), 1, calls)
        profile_arg = calls[0].rstrip("\t").split("\t")[-1]
        self.assertRegex(profile_arg, rf"^{re.escape(str(self.base))}\.gen\.\d+\.partial/debug$")
        gen = Path(os.readlink(self.base))
        self.assertPresent([gen / "debug" / "deps" / p.name for p in self.dead])

    def test_a_missing_sibling_fails_the_refresh_closed(self):
        result = self.run_refresh()

        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(os.path.lexists(self.base), "the base advanced without a prune")
        self.assertEqual(self.partials(), [])


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
