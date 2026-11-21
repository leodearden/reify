#!/usr/bin/env python3
"""
test_live_refs_namespace.py — stdlib unittest for scripts/lib_live_refs.sh
against processes that reach a lane through ANOTHER mount namespace
(canonical-build-path PRD, dark-factory plans/canonical-build-path-shared-cache-prd.md:
decision 16, C11, boundary row 19).

A holder runs under /usr/bin/bwrap with a lane bound at a scratch canon path,
so from the host its cwd and fd links read as that canon path, never the
lane's. Its visible bwrap parent is killed once the holder is ready, leaving
only the namespaced process alive.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_live_refs_namespace.sh.

Where bwrap cannot create a mount namespace (a landlock sandbox, an absent
binary), every test SKIPS with the named reason.
"""

import functools
import os
import select
import signal
import subprocess
import tempfile
import textwrap
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
LIB = REPO_ROOT / "scripts" / "lib_live_refs.sh"
GC = REPO_ROOT / "scripts" / "warm-lane-gc.sh"
BWRAP = "/usr/bin/bwrap"

BLOCK = 'echo $$; exec sleep infinity </dev/null >/dev/null 2>&1'
HOLD_FD3 = 'exec 3<"$1"; ' + BLOCK

# Makes its last path lookup before it prints its pid, so a namespace root made
# unreadable after that line cannot fail a lookup it still owes (BLOCK's
# </dev/null and exec of sleep would, killing the holder).
PAUSE_AFTER_LAST_LOOKUP = """
import os, signal
devnull = os.open(os.devnull, os.O_RDWR)
print(os.getpid(), flush=True)
for std in (0, 1, 2):
    os.dup2(devnull, std)
signal.pause()
"""

# Maps argv[1] and closes its fd, so the mapping is the holder's only reference.
# The stdlib mmap module would keep a dup of the fd open.
MAP_ONLY = """
import ctypes, os, sys
fd = os.open(sys.argv[1], os.O_RDONLY)
libc = ctypes.CDLL(None, use_errno=True)
libc.mmap.restype = ctypes.c_void_p
libc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int, ctypes.c_int,
                      ctypes.c_int, ctypes.c_long]
PROT_READ, MAP_PRIVATE = 1, 2
addr = libc.mmap(None, 4096, PROT_READ, MAP_PRIVATE, fd, 0)
if addr is None or addr == ctypes.c_void_p(-1).value:
    sys.exit(1)
os.close(fd)
""" + PAUSE_AFTER_LAST_LOOKUP

# The namespace's OLDEST process chroots into argv[2], away from every mount;
# only then does its forked child open argv[1] and announce. The child is the
# only process that can see the lane's mount.
CHROOTED_PARENT_FORKS_HOLDER = """
import os, sys
r, w = os.pipe()
child = os.fork()
if child == 0:
    os.close(w)
    fd = os.open(sys.argv[1], os.O_RDONLY)
    os.read(r, 1)
""" + textwrap.indent(PAUSE_AFTER_LAST_LOOKUP, "    ") + """
os.close(r)
os.chroot(sys.argv[2])
os.write(w, b"x")
os.waitpid(child, 0)
"""


def clean_env():
    return {k: v for k, v in os.environ.items()
            if not k.startswith(("GIT_", "REIFY_"))}


def bwrap_refusal(*options):
    probe = subprocess.run([BWRAP, *options, "--", "/bin/true"],
                           capture_output=True, text=True, env=clean_env(), check=False)
    return "" if probe.returncode == 0 else f"(rc={probe.returncode}): {probe.stderr.strip()}"


@functools.lru_cache(maxsize=None)
def namespace_unavailable_reason():
    if not os.access(BWRAP, os.X_OK):
        return f"{BWRAP} absent"
    with tempfile.TemporaryDirectory(prefix="test-live-refs-ns-probe-") as scratch:
        a, b = Path(scratch, "a"), Path(scratch, "b")
        a.mkdir()
        b.mkdir()
        refusal = bwrap_refusal("--dev-bind", "/", "/", "--bind", str(a), str(b))
    return f"bwrap cannot enter a mount namespace here {refusal}" if refusal else ""


@functools.lru_cache(maxsize=None)
def chroot_capability_unavailable_reason():
    refusal = bwrap_refusal("--dev-bind", "/", "/", "--cap-add", "CAP_SYS_CHROOT")
    return f"bwrap cannot grant CAP_SYS_CHROOT here {refusal}" if refusal else ""


class LiveRefsAcrossMountNamespaces(unittest.TestCase):
    def setUp(self):
        reason = namespace_unavailable_reason()
        if reason:
            self.skipTest(reason)
        tmp = tempfile.TemporaryDirectory(prefix="test-live-refs-ns-")
        self.addCleanup(tmp.cleanup)
        self.root = Path(os.path.realpath(tmp.name))
        # The space makes mountinfo escape the canon mount point as \040.
        self.canon = self.root / "canon dir"
        self.canon.mkdir()
        self.pidfds = {}

    def lane(self, name):
        lane = self.root / name
        (lane / "sub").mkdir(parents=True)
        (lane / "sub" / "f").write_text("lane content\n")
        return lane

    def bind_at_canon(self, lane):
        return ["--dev-bind", "/", "/", "--bind", str(lane), str(self.canon)]

    def unrelated_namespace_binds(self, nsroot):
        return ["--bind", str(nsroot), "/", "--ro-bind", "/usr", "/usr",
                "--symlink", "usr/bin", "/bin", "--symlink", "usr/lib", "/lib",
                "--symlink", "usr/lib64", "/lib64", "--proc", "/proc", "--dev", "/dev"]

    def hold(self, binds, chdir, argv):
        """Start a namespaced holder, wait until it has its reference, then
        kill its visible bwrap parent. Returns the holder's pid."""
        bw = subprocess.Popen([BWRAP, *binds, "--chdir", str(chdir), "--", *argv],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              cwd=self.root, env=clean_env())
        line = bw.stdout.readline()
        if not line:
            bw.wait()
            stderr = bw.stderr.read().decode(errors="replace")
            bw.stdout.close()
            bw.stderr.close()
            self.fail(f"holder never started (bwrap rc={bw.returncode}): {stderr}")
        pid = int(line)
        self.pidfds[pid] = os.pidfd_open(pid)
        self.addCleanup(self.reap, pid)
        bw.kill()
        bw.wait()
        bw.stdout.close()
        bw.stderr.close()
        self.assertNotEqual(os.readlink(f"/proc/{pid}/ns/mnt"), os.readlink("/proc/self/ns/mnt"),
                            "premise: the holder runs in a foreign mount namespace")
        return pid

    def hold_unrelated_unreadable_namespace(self):
        nsroot = self.root / "nsroot"
        nsroot.mkdir()
        pid = self.hold(self.unrelated_namespace_binds(nsroot), "/",
                        ["/usr/bin/python3", "-I", "-c", PAUSE_AFTER_LAST_LOOKUP])
        nsroot.chmod(0)
        self.addCleanup(nsroot.chmod, 0o755)
        with self.assertRaises(PermissionError, msg="premise: the namespace root is unreadable"):
            os.stat(f"/proc/{pid}/root/usr")
        return pid

    def reap(self, pid):
        pidfd = self.pidfds.pop(pid, None)
        if pidfd is None:
            return
        try:
            signal.pidfd_send_signal(pidfd, signal.SIGKILL)
        except ProcessLookupError:
            pass
        select.select([pidfd], [], [])
        os.close(pidfd)

    def present(self, path):
        result = subprocess.run(
            ["bash", "-c", 'source "$1"; live_ref_present "$2"', "_", str(LIB), str(path)],
            capture_output=True, text=True, cwd=self.root, env=clean_env(), check=False)
        self.assertIn(result.returncode, (0, 1), f"live_ref_present failed: {result.stderr}")
        return result.returncode == 0

    def referenced(self, *paths):
        result = subprocess.run(
            ["bash", "-c", 'source "$1"; shift; live_referenced_paths "$@"', "_", str(LIB),
             *map(str, paths)],
            capture_output=True, text=True, cwd=self.root, env=clean_env(), check=False)
        self.assertEqual(result.returncode, 0, f"live_referenced_paths failed: {result.stderr}")
        return set(result.stdout.splitlines())

    def test_cwd_inside_a_lane_bound_elsewhere_is_live(self):
        lane = self.lane("lane")
        pid = self.hold(self.bind_at_canon(lane), self.canon / "sub", ["/bin/sh", "-c", BLOCK])

        self.assertEqual(os.readlink(f"/proc/{pid}/cwd"), f"{self.canon}/sub",
                         "premise: from the host the cwd reads as the canon path")
        os.kill(pid, 0)
        self.assertTrue(self.present(lane))

    def test_open_file_inside_a_lane_bound_elsewhere_is_live(self):
        lane = self.lane("lane")
        free = self.lane("free")
        pid = self.hold(self.bind_at_canon(lane), "/",
                        ["/bin/sh", "-c", HOLD_FD3, "sh", f"{self.canon}/sub/f"])

        self.assertEqual(os.readlink(f"/proc/{pid}/fd/3"), f"{self.canon}/sub/f",
                         "premise: from the host fd 3 reads as the canon path")
        self.assertEqual(os.readlink(f"/proc/{pid}/cwd"), "/", "premise: cwd is outside the lane")
        self.assertTrue(self.present(lane))
        self.assertFalse(self.present(free))

    def test_mapping_only_inside_a_lane_bound_elsewhere_is_live(self):
        lane = self.lane("lane")
        mapped = f"{self.canon}/sub/f"
        pid = self.hold(self.bind_at_canon(lane), "/",
                        ["/usr/bin/python3", "-I", "-c", MAP_ONLY, mapped])

        fd_dir = f"/proc/{pid}/fd"
        fd_targets = [os.readlink(f"{fd_dir}/{fd}") for fd in os.listdir(fd_dir)]
        self.assertFalse([t for t in fd_targets if t == str(self.canon) or t.startswith(f"{self.canon}/")],
                         "premise: no open fd inside the lane")
        self.assertIn(mapped, Path(f"/proc/{pid}/maps").read_text(),
                      "premise: from the host the mapping reads as the canon path")
        self.assertEqual(os.readlink(f"/proc/{pid}/cwd"), "/", "premise: cwd is outside the lane")
        self.assertTrue(self.present(lane))

    def test_open_file_held_beside_an_older_process_chrooted_away_from_the_lane_is_live(self):
        reason = chroot_capability_unavailable_reason()
        if reason:
            self.skipTest(reason)
        lane = self.lane("lane")
        jail = self.root / "jail"
        jail.mkdir()
        pid = self.hold([*self.bind_at_canon(lane), "--cap-add", "CAP_SYS_CHROOT"], "/",
                        ["/usr/bin/python3", "-I", "-c", CHROOTED_PARENT_FORKS_HOLDER,
                         f"{self.canon}/sub/f", str(jail)])

        parent = int(Path(f"/proc/{pid}/stat").read_text().rpartition(")")[2].split()[1])
        self.assertEqual(os.readlink(f"/proc/{parent}/root"), str(jail),
                         "premise: the holder's older parent is chrooted away from the lane")
        self.assertEqual(os.readlink(f"/proc/{parent}/ns/mnt"), os.readlink(f"/proc/{pid}/ns/mnt"),
                         "premise: parent and holder share one namespace")
        self.assertIn(f"{self.canon}/sub/f",
                      [os.readlink(f"/proc/{pid}/fd/{fd}") for fd in os.listdir(f"/proc/{pid}/fd")],
                      "premise: from the host the holder's fd reads as the canon path")
        self.assertTrue(self.present(lane))

    def test_idle_lane_bound_at_the_canon_path_another_namespace_uses_is_not_referenced(self):
        lane_a = self.lane("lane-a")
        lane_b = self.lane("lane-b")
        self.hold(self.bind_at_canon(lane_a), "/",
                  ["/bin/sh", "-c", HOLD_FD3, "sh", f"{self.canon}/sub/f"])
        self.hold(self.bind_at_canon(lane_b), "/", ["/bin/sh", "-c", BLOCK])

        self.assertEqual(self.referenced(lane_a, lane_b), {str(lane_a)})
        self.assertFalse(self.present(lane_b))

    def test_two_namespaces_binding_lanes_at_one_canon_path_are_each_resolved_until_their_holder_dies(self):
        lane_a = self.lane("lane-a")
        lane_b = self.lane("lane-b")
        free = self.lane("free")
        holder_a = self.hold(self.bind_at_canon(lane_a), "/",
                             ["/bin/sh", "-c", HOLD_FD3, "sh", f"{self.canon}/sub/f"])
        self.hold(self.bind_at_canon(lane_b), self.canon / "sub", ["/bin/sh", "-c", BLOCK])

        self.assertEqual(self.referenced(lane_a, lane_b, free), {str(lane_a), str(lane_b)})

        self.reap(holder_a)
        self.assertFalse(self.present(lane_a))
        self.assertTrue(self.present(lane_b))

    def test_unrelated_namespace_with_unreadable_root_neither_counts_nor_breaks_the_scan(self):
        lane = self.lane("lane")
        self.hold_unrelated_unreadable_namespace()

        self.assertFalse(self.present(lane))

        self.hold(self.bind_at_canon(lane), "/",
                  ["/bin/sh", "-c", HOLD_FD3, "sh", f"{self.canon}/sub/f"])
        self.assertTrue(self.present(lane))

    def gc_fixture(self):
        root = self.root / "gc"
        repo, worktrees, base = root / "repo", root / "worktrees", root / "base"
        worktrees.mkdir(parents=True)
        (base / "target.gen.1").mkdir(parents=True)
        (base / "target.gen.1.lock").touch()
        (base / "target").symlink_to(base / "target.gen.1")
        self.git("init", "-q", "-b", "main", str(repo))
        (repo / "README.md").touch()
        self.git("-C", str(repo), "add", "README.md")
        self.git("-C", str(repo), "-c", "user.email=test@test.local", "-c", "user.name=Test",
                 "commit", "-q", "-m", "initial")
        for name in ("_lane-1", "_lane-2"):
            self.git("-C", str(repo), "worktree", "add", "-q", str(worktrees / name))
            (worktrees / name / "target").mkdir()
            (worktrees / name / "target" / "MARK").write_text("divergent\n")
        return worktrees, base

    def git(self, *args):
        subprocess.run(["git", *args], capture_output=True, text=True, cwd=self.root,
                       env=clean_env(), check=True)

    def run_gc_disk_pressure(self, worktrees, base):
        return subprocess.run(
            ["bash", str(GC), "reclaim", "--worktrees-dir", str(worktrees),
             "--base-target", str(base / "target"), "--disk-pressure"],
            capture_output=True, text=True, cwd=self.root, env=clean_env(), check=False)

    def test_gc_preserves_a_lane_whose_only_reference_is_through_another_namespace(self):
        worktrees, base = self.gc_fixture()
        self.hold_unrelated_unreadable_namespace()
        self.hold(self.bind_at_canon(worktrees / "_lane-1"), "/",
                  ["/bin/sh", "-c", HOLD_FD3, "sh", f"{self.canon}/target/MARK"])

        result = self.run_gc_disk_pressure(worktrees, base)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((worktrees / "_lane-1" / "target" / "MARK").exists(),
                        "gc deleted target/ under a live namespaced reference")
        self.assertFalse((worktrees / "_lane-2" / "target").exists(),
                         "gc kept the free lane's target/")
        self.assertIn("preserving _lane-1: live consumer (process reference)", result.stderr)
        self.assertIn("preserved_live_ref=1", result.stdout)

    def test_gc_reclaims_every_lane_when_only_an_unrelated_namespace_is_alive(self):
        worktrees, base = self.gc_fixture()
        self.hold_unrelated_unreadable_namespace()

        result = self.run_gc_disk_pressure(worktrees, base)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((worktrees / "_lane-1" / "target").exists())
        self.assertFalse((worktrees / "_lane-2" / "target").exists())
        self.assertIn("preserved_live_ref=0", result.stdout)


if __name__ == "__main__":
    unittest.main()
