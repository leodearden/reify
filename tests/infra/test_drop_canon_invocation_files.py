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
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
import canon_run_fixture as fx  # noqa: E402  (imported after sys.path manipulation)

CANON_CACHE = fx.load_canon_cache()
DROPPER = fx.REPO_ROOT / "scripts" / "drop-canon-invocation-files.py"


def scratch(test):
    tmp = tempfile.TemporaryDirectory(prefix="test-drop-canon-invocation-files-")
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
    files = [Path(path) for path in dataclasses.astuple(CANON_CACHE.SessionFiles.in_build_dir(str(build_dir), pid))]
    build_dir.mkdir(parents=True, exist_ok=True)
    for path in files:
        if path.name.endswith(".sock"):
            os.mknod(path, stat.S_IFSOCK | 0o600)
        else:
            path.write_text(f"left by {pid}\n")
    return files


def file_state(path):
    """(file type, bytes when regular, mtime in ns) of path itself, never of a link's target."""
    info = os.lstat(path)
    content = Path(path).read_bytes() if stat.S_ISREG(info.st_mode) else None
    return stat.S_IFMT(info.st_mode), content, info.st_mtime_ns


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
