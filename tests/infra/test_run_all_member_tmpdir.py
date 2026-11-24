#!/usr/bin/env python3
"""
test_run_all_member_tmpdir.py — every run_all.sh member gets a private,
leased TMPDIR that is removed when it exits, and stale ones are reaped
(task 10372).

Infra members leak temp dirs into /tmp when they die before their own cleanup
runs: a member budget kill, a cancelled verify. No per-test fix survives
SIGKILL, so the fix sits at run_all.sh's one member spawn site, in
tests/infra/run-all-member-tmpdir-lib.sh:
  - member_tmpdir_run <root> <cmd>... runs <cmd> with TMPDIR set to a fresh
    <root>/reify-ra-tmp.* dir, holds a flock lease on that dir which <cmd> does
    not inherit, and removes the dir when <cmd> exits;
  - member_tmpdir_reap <root> removes the reify-ra-tmp.* dirs under <root>
    whose lease is free and whose mtime is over a minute old.

The lib classes drive those two functions through `bash -c 'source <lib>; ...'`
in a private root. The only time bounds are hang guards on the subprocesses,
never assertions that anything is fast.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_run_all_member_tmpdir.sh.
"""

import fcntl
import os
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
LIB = INFRA_DIR / "run-all-member-tmpdir-lib.sh"

PREFIX = "reify-ra-tmp."
HANG_GUARD_S = 120
STALE_AGE_S = 10 * 60

# Records its TMPDIR, whether that dir existed and was the user's, and the
# targets of its own open fds; litters the TMPDIR; exits with $1.
LITTER_CHILD = r"""#!/usr/bin/env bash
state="$STATE_DIR"
printf '%s\n' "$TMPDIR" > "$state/tmpdir"
if [ -d "$TMPDIR" ] && [ -O "$TMPDIR" ]; then echo yes; else echo no; fi > "$state/owned"
for fd in /proc/$$/fd/*; do readlink "$fd" || true; done > "$state/fds"
nested="$(mktemp -d)"
echo litter > "$nested/file"
python3 -c 'import tempfile; tempfile.mkdtemp()'
echo litter > "$TMPDIR/plain-file"
exit "$1"
"""

# Leaves a directory it cannot write into, holding a file.
READONLY_CHILD = r"""#!/usr/bin/env bash
mkdir "$TMPDIR/locked"
echo litter > "$TMPDIR/locked/file"
chmod 0500 "$TMPDIR/locked"
printf '%s\n' "$TMPDIR" > "$STATE_DIR/tmpdir"
"""

# Records its TMPDIR, then waits for $STATE_DIR/go (bounded by a hang guard).
WAITING_CHILD = r"""#!/usr/bin/env bash
printf '%s\n' "$TMPDIR" > "$STATE_DIR/tmpdir.partial"
mv "$STATE_DIR/tmpdir.partial" "$STATE_DIR/tmpdir"
for _ in $(seq 1200); do
    [ -e "$STATE_DIR/go" ] && exit 0
    sleep 0.1
done
exit 99
"""


class LibHarness(unittest.TestCase):
    """A private base dir: root/ (the TMPDIR root under test), state/ (what a
    child records), bin/ (child scripts) and caller-tmp/ (the TMPDIR the lib's
    caller has)."""

    def setUp(self):
        base = tempfile.TemporaryDirectory(prefix="test-ra-member-tmpdir-")
        self.addCleanup(base.cleanup)
        # A read-only subdir a failed case leaves behind would defeat cleanup().
        self.addCleanup(subprocess.run, ["chmod", "-R", "u+rwx", base.name], check=False)
        self.base = Path(base.name)
        self.root = self.base / "root"
        self.state = self.base / "state"
        self.bin = self.base / "bin"
        self.caller_tmp = self.base / "caller-tmp"
        for d in (self.root, self.state, self.bin, self.caller_tmp):
            d.mkdir()

    def child(self, name, body):
        path = self.bin / name
        path.write_text(body)
        path.chmod(0o755)
        return path

    def env(self):
        env = dict(os.environ)
        env.update(TMPDIR=str(self.caller_tmp), STATE_DIR=str(self.state))
        return env

    def lib_argv(self, function, *args):
        return ["bash", "-c", 'source "$1"; shift; "$@"', "lib", str(LIB), function,
                *map(str, args)]

    def call(self, function, *args):
        return subprocess.run(self.lib_argv(function, *args), env=self.env(),
                              capture_output=True, text=True, timeout=HANG_GUARD_S, check=False)

    def recorded_tmpdir(self):
        return Path((self.state / "tmpdir").read_text().strip())

    def root_entries(self):
        return sorted(p.name for p in self.root.iterdir())

    def leftovers(self):
        return [name for name in self.root_entries() if name.startswith(PREFIX)]

    def stale_dir(self, name, contents=True):
        """<root>/<name> holding a file, with its mtime 10 minutes back."""
        path = self.root / name
        path.mkdir()
        if contents:
            (path / "litter").write_text("left by a killed member\n")
        backdate(path)
        return path


def backdate(path, follow_symlinks=True):
    then = time.time() - STALE_AGE_S
    os.utime(path, (then, then), follow_symlinks=follow_symlinks)


class MemberTmpdirRun(LibHarness):
    def test_the_child_runs_in_a_fresh_private_tmpdir_under_the_root(self):
        result = self.call("member_tmpdir_run", self.root, self.child("litter", LITTER_CHILD), 0)

        self.assertEqual(result.returncode, 0, result.stderr)
        tmpdir = self.recorded_tmpdir()
        self.assertEqual(tmpdir.parent, self.root)
        self.assertTrue(tmpdir.name.startswith(PREFIX), tmpdir)
        self.assertEqual((self.state / "owned").read_text().strip(), "yes")

    def test_the_child_holds_no_fd_on_its_tmpdir(self):
        result = self.call("member_tmpdir_run", self.root, self.child("litter", LITTER_CHILD), 0)

        self.assertEqual(result.returncode, 0, result.stderr)
        fds = (self.state / "fds").read_text().splitlines()
        self.assertNotIn(str(self.recorded_tmpdir()), fds, "the lease leaked into the member")

    def test_everything_the_child_leaves_in_its_tmpdir_is_removed(self):
        result = self.call("member_tmpdir_run", self.root, self.child("litter", LITTER_CHILD), 0)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(os.path.lexists(self.recorded_tmpdir()))
        self.assertEqual(self.leftovers(), [])
        self.assertEqual(sorted(self.caller_tmp.iterdir()), [],
                         "the child wrote into the caller's TMPDIR")

    def test_the_childs_exit_status_is_returned_unchanged(self):
        child = self.child("litter", LITTER_CHILD)
        for rc in (0, 3, 124):
            with self.subTest(rc=rc):
                result = self.call("member_tmpdir_run", self.root, child, rc)

                self.assertEqual(result.returncode, rc, result.stderr)
                self.assertEqual(self.leftovers(), [])

    def test_a_dir_the_child_made_unwritable_is_still_removed(self):
        result = self.call("member_tmpdir_run", self.root, self.child("readonly", READONLY_CHILD))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(os.path.lexists(self.recorded_tmpdir()))
        self.assertEqual(self.leftovers(), [])

    def test_an_unusable_root_runs_the_child_with_the_callers_tmpdir(self):
        unwritable = self.base / "unwritable"
        unwritable.mkdir()
        unwritable.chmod(0o500)
        for label, root in (("missing", self.base / "no-such-root"), ("not writable", unwritable)):
            with self.subTest(label):
                result = self.call("member_tmpdir_run", root, self.child("litter", LITTER_CHILD), 3)

                self.assertEqual(result.returncode, 3, result.stderr)
                self.assertEqual(self.recorded_tmpdir(), self.caller_tmp)
                naming = [line for line in result.stderr.splitlines() if str(root) in line]
                self.assertEqual(len(naming), 1, result.stderr)


class MemberTmpdirReap(LibHarness):
    def reap(self):
        return self.call("member_tmpdir_reap", self.root)

    def test_an_unleased_stale_dir_is_removed(self):
        dead = self.stale_dir(f"{PREFIX}dead")
        locked = dead / "locked"
        locked.mkdir()
        (locked / "file").write_text("unwritable parent\n")
        locked.chmod(0o500)
        backdate(dead)

        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(os.path.lexists(dead))
        self.assertIn(f"reaped 1 stale member TMPDIR(s) under {self.root}", result.stderr)

    def test_a_leased_dir_is_kept(self):
        held = self.stale_dir(f"{PREFIX}held")
        fd = os.open(held, os.O_RDONLY)
        self.addCleanup(os.close, fd)
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)

        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((held / "litter").is_file())
        self.assertNotIn("reaped", result.stderr)

    def test_a_freshly_created_dir_is_kept(self):
        fresh = self.root / f"{PREFIX}fresh"
        fresh.mkdir()

        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(fresh.is_dir())

    def test_a_symlink_is_not_followed(self):
        outside = self.base / "outside"
        outside.mkdir()
        sentinel = outside / "sentinel"
        sentinel.write_text("must survive\n")
        backdate(outside)
        link = self.root / f"{PREFIX}link"
        link.symlink_to(outside)
        backdate(link, follow_symlinks=False)

        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(link.is_symlink())
        self.assertTrue(sentinel.is_file())

    def test_other_names_and_non_directories_are_untouched(self):
        plain_file = self.root / f"{PREFIX}x"
        plain_file.write_text("a regular file with the prefix\n")
        backdate(plain_file)
        other = self.stale_dir("tmp.abcdef")

        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(plain_file.is_file())
        self.assertTrue((other / "litter").is_file())

    def test_a_running_members_dir_is_kept_even_when_its_mtime_is_old(self):
        proc = subprocess.Popen(
            self.lib_argv("member_tmpdir_run", self.root, self.child("waiting", WAITING_CHILD)),
            env=self.env(), stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        self.addCleanup(self.finish, proc)
        recorded = self.state / "tmpdir"
        deadline = time.monotonic() + HANG_GUARD_S
        while not recorded.exists() and proc.poll() is None and time.monotonic() < deadline:
            time.sleep(0.1)
        self.assertTrue(recorded.exists(), "the member never started")
        tmpdir = self.recorded_tmpdir()
        backdate(tmpdir)

        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(tmpdir.is_dir(), "a live member's TMPDIR was reaped")
        (self.state / "go").touch()
        self.assertEqual(proc.wait(timeout=HANG_GUARD_S), 0)
        self.assertFalse(os.path.lexists(tmpdir))

    def finish(self, proc):
        (self.state / "go").touch()
        if proc.poll() is None:
            try:
                proc.wait(timeout=HANG_GUARD_S)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
        proc.stdout.close()


if __name__ == "__main__":
    unittest.main()
