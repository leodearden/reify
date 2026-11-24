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
    whose lease is free and whose mtime is over a minute old;
  - neither removes a dir that a live process was started with as its TMPDIR.

The lib classes drive those two functions through `bash -c 'source <lib>; ...'`
in a private root. RunAllWiring EXECUTES the real tests/infra/run_all.sh on a
tempdir fixture INFRA_DIR, in run_all_fixture.run_all_env's environment plus a
private TMPDIR, as test_run_all_member_lifetime.py does. Every fixture process
carries the fixture's unique token or path in its /proc cmdline, and whatever
still carries one is SIGKILLed at cleanup.

The only time bounds are hang guards on the subprocesses, never assertions
that anything is fast.

run_all.sh discovers `test_*.sh` only; the gate runs this file through its
thin wrapper tests/infra/test_run_all_member_tmpdir.sh.
"""

import contextlib
import fcntl
import os
import re
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
import uuid
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
from proc_strays import kill_strays  # noqa: E402  (imported after sys.path manipulation)
from run_all_fixture import run_all_env, write_executable  # noqa: E402  (imported after sys.path manipulation)

REPO_ROOT = INFRA_DIR.parents[1]
RUN_ALL = REPO_ROOT / "tests" / "infra" / "run_all.sh"
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

# Says its TMPDIR on stdout, then waits for its stdin to close.
WAITING_CHILD = r"""#!/usr/bin/env bash
printf '%s\n' "$TMPDIR"
read -r _
exit 0
"""

# Records its TMPDIR, and leaves behind a process that was started with that
# TMPDIR and lives until the caller's stdin closes; records that process's pid.
HOLDER_CHILD = r"""#!/usr/bin/env bash
printf '%s\n' "$TMPDIR" > "$STATE_DIR/tmpdir"
exec 3<&0
cat <&3 >/dev/null 2>&1 3<&- &
printf '%s\n' "$!" > "$STATE_DIR/holder.pid"
"""


class LibHarness(unittest.TestCase):
    """A private base dir: root/ (the TMPDIR root under test), state/ (what a
    child records), bin/ (child scripts) and caller-tmp/ (the TMPDIR the lib's
    caller has)."""

    def setUp(self):
        base = tempfile.TemporaryDirectory(prefix="test-ra-member-tmpdir-")
        self.addCleanup(base.cleanup)
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
        return subprocess.run(
            self.lib_argv(function, *args),
            env=self.env(),
            capture_output=True,
            text=True,
            timeout=HANG_GUARD_S,
            check=False,
        )

    def spawn(self, function, *args):
        """The lib call, running, with stdin, stdout and stderr piped. Closing
        its stdin is what releases a child that waits on it."""
        proc = subprocess.Popen(
            self.lib_argv(function, *args),
            env=self.env(),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.addCleanup(finish, proc)
        return proc

    def reap(self):
        return self.call("member_tmpdir_reap", self.root)

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


def finish(proc):
    """Release a spawned lib call, and kill it if it outlives the hang guard."""
    proc.stdin.close()
    try:
        proc.wait(timeout=HANG_GUARD_S)
    except subprocess.TimeoutExpired:
        proc.kill()
        proc.wait()
    proc.stdout.close()
    proc.stderr.close()


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
        proc = self.spawn("member_tmpdir_run", self.root, self.child("waiting", WAITING_CHILD))
        said = proc.stdout.readline().strip()
        self.assertTrue(said, "the member never started")
        tmpdir = Path(said)
        backdate(tmpdir)

        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(tmpdir.is_dir(), "a live member's TMPDIR was reaped")
        proc.stdin.close()
        self.assertEqual(proc.wait(timeout=HANG_GUARD_S), 0)
        self.assertFalse(os.path.lexists(tmpdir))


class StillInUse(LibHarness):
    """A process a member leaves behind that was started with the member's
    TMPDIR, such as a cache server its cargo respawned, keeps that dir: run
    does not remove it, and reap removes it only once that process is gone."""

    def test_a_dir_a_live_process_has_as_tmpdir_is_kept_until_that_process_exits(self):
        proc = self.spawn("member_tmpdir_run", self.root, self.child("holder", HOLDER_CHILD))
        self.assertEqual(proc.wait(timeout=HANG_GUARD_S), 0)
        holder = self.pidfd(int((self.state / "holder.pid").read_text()))
        tmpdir = self.recorded_tmpdir()

        self.assertTrue(tmpdir.is_dir(), "the dir was removed under a live process")
        naming = [line for line in proc.stderr.read().splitlines() if str(tmpdir) in line]
        self.assertEqual(len(naming), 1, naming)

        backdate(tmpdir)
        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(tmpdir.is_dir(), "the reap removed the dir under a live process")
        self.assertNotIn("reaped", result.stderr)

        proc.stdin.close()
        ready, _, _ = select.select([holder], [], [], HANG_GUARD_S)
        self.assertTrue(ready, "the holder did not exit when its stdin closed")
        result = self.reap()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(os.path.lexists(tmpdir))
        self.assertIn(f"reaped 1 stale member TMPDIR(s) under {self.root}", result.stderr)

    def pidfd(self, pid):
        """A pidfd on <pid>, readable once it exits; the cleanup SIGKILLs it first."""
        fd = os.pidfd_open(pid)
        self.addCleanup(os.close, fd)
        self.addCleanup(self.kill, fd)
        return fd

    @staticmethod
    def kill(pidfd):
        with contextlib.suppress(ProcessLookupError):
            signal.pidfd_send_signal(pidfd, signal.SIGKILL)


# RunAllWiring's members. Each records its TMPDIR to
# $FIXTURE_STATE/<name>.tmpdir and litters it; @TOKEN@ is the fixture's token.
# The hanging member execs its sleep, so the one process that has its TMPDIR is
# the one timeout waits for, and none is left alive to keep the dir.
LITTER_MEMBER = r"""#!/usr/bin/env bash
printf '%s\n' "$TMPDIR" > "$FIXTURE_STATE/$(basename "$0").tmpdir"
nested="$(mktemp -d)"
echo litter > "$nested/file"
python3 -c 'import tempfile; tempfile.mkdtemp()'
echo litter > "$TMPDIR/plain-file"
exit @RC@
"""

WIRING_MEMBERS = {
    "test_litter.sh": LITTER_MEMBER.replace("@RC@", "0"),
    "test_litter_fail.sh": LITTER_MEMBER.replace("@RC@", "3"),
    "test_hang_litter.sh": r"""#!/usr/bin/env bash
printf '%s\n' "$TMPDIR" > "$FIXTURE_STATE/$(basename "$0").tmpdir"
mkdir "$TMPDIR/hang-litter"
exec -a @TOKEN@ sleep 300
""",
}

# Long enough that the litter members finish far inside it on a loaded host,
# short against the 300 s the hanging member would otherwise live.
WIRING_BUDGET_S = 20
WIRING_GRACE_S = 2
RUN_TIMEOUT_S = 600
RUN_ALL_OWN_WORKDIR_PREFIX = "reify-run-all-"

TIMEOUT_LINE_RE = re.compile(r"^MEMBER_TIMEOUT FAIL member=(?P<member>\S+) ", re.M)


class RunAllWiring(unittest.TestCase):
    """One real run_all.sh run over three litter members, with a dead and a
    leased reify-ra-tmp.* dir seeded in its TMPDIR beforehand."""

    @classmethod
    def setUpClass(cls):
        cls.root = Path(tempfile.mkdtemp(prefix="ra-member-tmpdir-"))
        cls.token = f"ra-member-tmpdir-{uuid.uuid4().hex}"
        cls.addClassCleanup(shutil.rmtree, cls.root, True)
        cls.addClassCleanup(kill_strays, str(cls.root) + os.sep, cls.token)
        cls.infra = cls.root / "infra"
        cls.state = cls.root / "state"
        cls.tmp = cls.root / "tmp"
        for d in (cls.infra, cls.state, cls.tmp):
            d.mkdir()
        for name, body in WIRING_MEMBERS.items():
            write_executable(cls.infra / name, body.replace("@TOKEN@", cls.token))
        manifest = cls.root / "classification.manifest"
        manifest.write_text("".join(f"{name} pool\n" for name in WIRING_MEMBERS))

        cls.stale = cls.tmp / f"{PREFIX}stale"
        cls.stale.mkdir()
        (cls.stale / "litter").write_text("left by a SIGKILLed run\n")
        backdate(cls.stale)
        cls.held = cls.tmp / f"{PREFIX}held"
        cls.held.mkdir()
        (cls.held / "litter").write_text("a live run's\n")
        backdate(cls.held)
        held_fd = os.open(cls.held, os.O_RDONLY)
        cls.addClassCleanup(os.close, held_fd)
        fcntl.flock(held_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)

        env = run_all_env(
            cls.root, manifest, TMPDIR=str(cls.tmp), FIXTURE_STATE=str(cls.state),
            REIFY_RUN_ALL_POOL_CONCURRENCY="1",
            REIFY_RUN_ALL_MEMBER_TIMEOUT=str(WIRING_BUDGET_S),
            REIFY_RUN_ALL_MEMBER_KILL_GRACE=str(WIRING_GRACE_S),
        )
        cls.run_all = subprocess.run(
            ["bash", str(RUN_ALL), str(cls.infra)],
            env=env,
            capture_output=True,
            text=True,
            errors="surrogateescape",
            timeout=RUN_TIMEOUT_S,
        )

    def output(self):
        return self.run_all.stdout + self.run_all.stderr

    def recorded_tmpdirs(self):
        return {name: Path((self.state / f"{name}.tmpdir").read_text().strip())
                for name in WIRING_MEMBERS}

    def test_the_verdicts_are_unchanged(self):
        self.assertEqual(self.run_all.returncode, 1, self.output())
        self.assertIn("RESULT: PASS (test_litter.sh)", self.run_all.stdout)
        failed = [line.split()[1:] for line in self.run_all.stdout.splitlines()
                  if line.startswith("FAILED ")]
        self.assertEqual([sorted(names) for names in failed],
                         [["test_hang_litter.sh", "test_litter_fail.sh"]], self.output())
        timed_out = [m["member"] for m in TIMEOUT_LINE_RE.finditer(self.run_all.stdout)]
        self.assertIn("test_hang_litter.sh", timed_out, self.output())
        self.assertNotIn("test_litter_fail.sh", timed_out)

    def test_each_member_ran_in_its_own_tmpdir_under_the_runs_tmpdir(self):
        tmpdirs = self.recorded_tmpdirs()
        for name, tmpdir in tmpdirs.items():
            with self.subTest(name):
                self.assertEqual(tmpdir.parent, self.tmp)
                self.assertTrue(tmpdir.name.startswith(PREFIX), tmpdir)
        self.assertEqual(len(set(tmpdirs.values())), len(tmpdirs), tmpdirs)

    def test_no_member_tmpdir_or_litter_outlives_the_run(self):
        for name, tmpdir in self.recorded_tmpdirs().items():
            with self.subTest(name):
                self.assertFalse(os.path.lexists(tmpdir), self.output())
        left = sorted(p.name for p in self.tmp.iterdir()
                      if not p.name.startswith(RUN_ALL_OWN_WORKDIR_PREFIX) and p != self.held)
        self.assertEqual(left, [], self.output())

    def test_a_dead_runs_stale_tmpdir_is_reaped_at_start(self):
        self.assertFalse(os.path.lexists(self.stale), self.output())
        self.assertIn(f"reaped 1 stale member TMPDIR(s) under {self.tmp}", self.run_all.stderr)

    def test_a_leased_tmpdir_survives_the_run(self):
        self.assertTrue((self.held / "litter").is_file(), self.output())


if __name__ == "__main__":
    unittest.main()
