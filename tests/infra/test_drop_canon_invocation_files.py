#!/usr/bin/env python3
"""
test_drop_canon_invocation_files.py — stdlib unittest for
scripts/drop-canon-invocation-files.py and its two callers,
scripts/refresh-warm-base.sh (Step 3a, on the staging copy of a promoted gen)
and scripts/seed-warm-lane.sh (right after its clone) — task 10442,
docs/prds/warm-lane-pool-cow-seeding.md §9.5 inv.17.

A copied target must start with no history and no session of the tree it was
copied from, so the dropper removes every canonical-build-path invocation file
from the copy: the invocation journal at any build directory, and a private
sccache server's pid-named session files. Every such name planted here comes
from scripts/canon_cache.py, the one home of that grammar, never from a literal.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_drop_canon_invocation_files.sh.
"""

import dataclasses
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)
import refresh_warm_base_harness as refresh_harness  # noqa: E402
import seed_warm_lane_harness as seed_harness  # noqa: E402

CANON_CACHE = fx.load_canon_cache()
DROPPER = fx.REPO_ROOT / "scripts" / "drop-canon-invocation-files.py"


def scratch(test):
    tmp = tempfile.TemporaryDirectory(prefix="test-canon-copy-hygiene-")
    test.addCleanup(tmp.cleanup)
    return Path(os.path.realpath(tmp.name))


def dead_pid():
    process = subprocess.Popen(["true"])
    process.wait()
    return process.pid


def plant_journal(build_dir):
    journal = Path(CANON_CACHE.journal_path(build_dir))
    journal.parent.mkdir(parents=True, exist_ok=True)
    journal.write_text('{"event": "build_cache"}\n')
    return journal


def plant_session_files(build_dir, pid):
    """The socket node, server stderr and uncached log a supervisor with this pid leaves in build_dir."""
    named = CANON_CACHE.SessionFiles.in_build_dir(str(build_dir), pid)
    files = [Path(path) for path in dataclasses.astuple(named)]
    build_dir.mkdir(parents=True, exist_ok=True)
    for path in files:
        if path.name.endswith(".sock"):
            os.mknod(path, stat.S_IFSOCK | 0o600)
        else:
            path.write_text(f"left by {pid}\n")
    return files


def kind_and_bytes(path):
    """(file type, bytes when regular) of path itself, never of a link's target."""
    mode = os.lstat(path).st_mode
    return stat.S_IFMT(mode), Path(path).read_bytes() if stat.S_ISREG(mode) else None


def file_state(path):
    return (*kind_and_bytes(path), os.lstat(path).st_mtime_ns)


def entries_under(root, accept):
    """Every path below root, symlinks not followed, whose (name, lstat mode) accept() takes."""
    found = []
    for directory, dirnames, filenames in os.walk(root, followlinks=False):
        for name in dirnames + filenames:
            path = Path(directory, name)
            if accept(name, os.lstat(path).st_mode):
                found.append(path)
    return found


def drop(*args):
    return subprocess.run([sys.executable, str(DROPPER), *map(str, args)], capture_output=True, text=True)


def diag(completed):
    return (f"\n--- exit {completed.returncode} ---\n--- stdout ---\n"
            f"{completed.stdout}\n--- stderr ---\n{completed.stderr}")


class DropRule(unittest.TestCase):
    def setUp(self):
        self.tmp = scratch(self)
        self.tree = self.tmp / "target"
        self.tree.mkdir()

    def test_every_invocation_file_is_removed_wherever_it_lies_and_whoever_wrote_it(self):
        planted = [
            plant_journal(self.tree),
            plant_journal(self.tree / "nested-build"),
            *plant_session_files(self.tree, os.getpid()),
            *plant_session_files(self.tree, dead_pid()),
        ]
        result = drop(self.tree)
        self.assertEqual(result.returncode, 0, diag(result))
        self.assertEqual(result.stdout, "")
        self.assertEqual([path for path in planted if os.path.lexists(path)], [], diag(result))

    def test_other_files_keep_their_bytes_and_mtimes(self):
        keepers = [self.tree / name for name in (
            ".reify-build-root",
            ".reify-cargo-starts.jsonl",
            "reify-verify-attempt.json",
            "debug/deps/libalpha-0123.rlib",
            "debug/build/alpha-1/output",
            ".reify-canon-journal.jsonl.1",
        )]
        for index, path in enumerate(keepers):
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"keeper {index}\n")
            os.utime(path, ns=(1_600_000_000_000_000_000 + index, 1_600_000_000_000_000_000 + index))
        look_alike = self.tree / ".reify-sccache.0.sock"
        os.mknod(look_alike, stat.S_IFSOCK | 0o600)
        keepers.append(look_alike)
        plant_journal(self.tree)
        before = {path: file_state(path) for path in keepers}
        result = drop(self.tree)
        self.assertEqual(result.returncode, 0, diag(result))
        self.assertEqual({path: file_state(path) for path in keepers}, before)

    def test_a_symlink_is_removed_never_followed(self):
        outside = self.tmp / "outside"
        outside_journal = plant_journal(outside)
        outside_file = self.tmp / "outside-file"
        outside_file.write_text("not the tree's\n")
        link = self.tree / "link"
        link.symlink_to(outside, target_is_directory=True)
        journal_link = Path(CANON_CACHE.journal_path(self.tree))
        journal_link.symlink_to(outside_file)
        result = drop(self.tree)
        self.assertEqual(result.returncode, 0, diag(result))
        self.assertTrue(outside_journal.is_file(), "the dropper followed a symlinked directory")
        self.assertTrue(outside_file.is_file(), "the dropper removed a symlink's target")
        self.assertTrue(link.is_symlink())
        self.assertFalse(os.path.lexists(journal_link), diag(result))

    def test_a_file_that_cannot_be_removed_fails_the_run_naming_it(self):
        if os.geteuid() == 0:
            self.skipTest("root unlinks from a read-only directory regardless of its mode")
        locked = self.tree / "locked"
        journal = plant_journal(locked)
        os.chmod(locked, 0o555)
        self.addCleanup(os.chmod, locked, 0o755)
        result = drop(self.tree)
        self.assertEqual(result.returncode, 1, diag(result))
        self.assertEqual(result.stdout, "")
        self.assertIn(str(journal), result.stderr)


class RefreshStep(unittest.TestCase):
    """refresh-warm-base.sh's Step 3a runs the REAL dropper on the staging copy,
    never on the advancing target. Staged by refresh_warm_base_harness; the lane
    is a git repository on main whose untracked target/ advances."""

    KEEPERS = (Path("debug") / "libplaceholder.rlib", Path(".reify-build-root"))

    def setUp(self):
        self.tmp = scratch(self)
        self.stub_dir = self.tmp / "stub-bin"
        self.stub_dir.mkdir()
        refresh_harness.install_cp_stub(self.stub_dir)
        self.refresh = refresh_harness.stage_refresh(self.tmp / "scripts", real=(DROPPER.name,))
        self.lane = self.tmp / "lane"
        refresh_harness.init_lane(self.lane)
        (self.lane / "tracked.txt").write_text("tracked\n")
        refresh_harness.git(self.lane, "add", "--", "tracked.txt")
        self.head = refresh_harness.commit(self.lane, "-m", "fixture: one tracked file")
        self.advancing = self.lane / "target"
        self.planted = [
            plant_journal(self.advancing),
            plant_journal(self.advancing / "nested-build"),
            *plant_session_files(self.advancing, dead_pid()),
        ]
        (self.advancing / self.KEEPERS[0]).parent.mkdir(parents=True)
        (self.advancing / self.KEEPERS[0]).write_text("placeholder artifact\n")
        (self.advancing / self.KEEPERS[1]).write_text("/opt/reify-canon/ws\n")
        self.base = self.tmp / "base"

    def run_refresh(self, refresh=None):
        return refresh_harness.run_refresh(refresh or self.refresh, self.stub_dir, self.advancing,
                                           self.base, self.head)

    def test_the_new_gen_carries_no_invocation_file_and_keeps_the_rest(self):
        result = self.run_refresh()
        self.assertEqual(result.returncode, 0, diag(result))
        gen = Path(os.readlink(self.base))
        self.assertEqual(entries_under(gen, lambda name, mode: CANON_CACHE.is_invocation_file(name)), [],
                         diag(result))
        self.assertEqual(entries_under(gen, lambda name, mode: stat.S_ISSOCK(mode)), [], diag(result))
        for kept in self.KEEPERS:
            self.assertEqual((gen / kept).read_bytes(), (self.advancing / kept).read_bytes(),
                             f"{kept} did not reach the gen unchanged")

    def test_the_advancing_target_keeps_its_own_invocation_files(self):
        """The _merge-verify lane's journal is its own history."""
        watched = [*self.planted, *(self.advancing / kept for kept in self.KEEPERS)]
        before = {path: kind_and_bytes(path) for path in watched}
        result = self.run_refresh()
        self.assertEqual(result.returncode, 0, diag(result))
        self.assertEqual({path: kind_and_bytes(path) for path in watched}, before)

    def test_a_missing_dropper_fails_the_refresh_closed(self):
        refresh = refresh_harness.stage_refresh(self.tmp / "scripts-without-dropper", absent=(DROPPER.name,))
        result = self.run_refresh(refresh)
        self.assertNotEqual(result.returncode, 0, diag(result))
        self.assertFalse(os.path.lexists(self.base), "the base advanced without the drop")
        self.assertEqual(sorted(self.tmp.glob("base.gen.*.partial")), [])
        self.assertIn(DROPPER.name, result.stderr, "the refresh failed before it reached the dropper")


class SeedStep(unittest.TestCase):
    """seed-warm-lane.sh runs the REAL dropper on its clone in both modes, on an
    UNSTAMPED base target too (no .reify-build-root). Built as
    test_seed_depinfo_relocation.py SeedWiringFailsClosed builds its pool: a
    hand-made built target and no cargo."""

    def setUp(self):
        self.tmp = scratch(self)
        self.pool = seed_harness.Pool(self.tmp, "build-root-playing-merge-verify")
        self.pool.write_build_root({"tracked.txt": "v1\n"})
        self.pool.commit_build_root()
        built = self.tmp / "built-target"
        seed_harness.write_file(built / "debug" / ".fingerprint" / "alpha-1" / "dep-lib-alpha", "")
        self.keeper = Path("debug") / "base_artifact.a"
        seed_harness.write_file(built / self.keeper, "base artifact\n")
        planted = [
            plant_journal(built),
            plant_journal(built / "nested-build"),
            *plant_session_files(built, dead_pid()),
        ]
        self.planted = [path.relative_to(built) for path in planted]
        self.pool.stamp_gen(built)

    def assert_no_invocation_file(self, target, result):
        found = entries_under(target, lambda name, mode: CANON_CACHE.is_invocation_file(name))
        self.assertEqual(found, [], diag(result))

    def test_a_fresh_checkout_lane_carries_no_invocation_file_and_keeps_the_rest(self):
        lane = self.pool.add_lane("_lane-1")
        result = self.pool.seed(lane)
        self.assertEqual(result.returncode, 0, diag(result))
        self.assertEqual(result.stdout, f"{lane / 'target'}\n")
        self.assert_no_invocation_file(lane / "target", result)
        self.assertTrue((lane / "target" / self.keeper).is_file(), diag(result))
        self.assertEqual([rel for rel in self.planted if not os.path.lexists(self.pool.gen / rel)], [],
                         "the seed dropped files from the base gen it cloned")

    def test_a_reset_in_place_seed_drops_them_too(self):
        lane = self.pool.add_lane("_lane-2")
        result = self.pool.seed(lane, mode="--reset-in-place")
        self.assertEqual(result.returncode, 0, diag(result))
        self.assert_no_invocation_file(lane / "target", result)

    def test_a_seed_without_the_dropper_aborts_naming_it(self):
        """The copy carries the relocate helper this unstamped clone needs before the drop."""
        copy_dir = self.tmp / "seed-copy"
        copy_dir.mkdir()
        for name in (seed_harness.SEED.name, "relocate-depinfo-paths.py"):
            shutil.copy2(seed_harness.SEED.parent / name, copy_dir / name)
        lane = self.pool.add_lane("_lane-3")
        result = self.pool.seed(lane, seed_script=copy_dir / seed_harness.SEED.name)
        self.assertNotEqual(result.returncode, 0, diag(result))
        self.assertEqual(result.stdout, "")
        self.assertIn(DROPPER.name, result.stderr)


class Usage(unittest.TestCase):
    def setUp(self):
        self.assertTrue(DROPPER.is_file(), f"{DROPPER} is missing, and python3 exits 2 for that too")
        self.tmp = scratch(self)

    def test_no_argument_or_two_arguments_is_a_usage_error(self):
        for args in ((), (self.tmp, self.tmp)):
            with self.subTest(args=args):
                result = drop(*args)
                self.assertEqual(result.returncode, 2, diag(result))
                self.assertEqual(result.stdout, "")

    def test_a_regular_file_is_a_usage_error(self):
        regular = self.tmp / "regular"
        regular.write_text("")
        result = drop(regular)
        self.assertEqual(result.returncode, 2, diag(result))
        self.assertEqual(result.stdout, "")

    def test_a_missing_directory_is_vacuously_clean(self):
        """Nothing can lie under it; test_seed_warm_lane.sh's default cp stub never creates the lane target."""
        result = drop(self.tmp / "no-such-target")
        self.assertEqual(result.returncode, 0, diag(result))
        self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
