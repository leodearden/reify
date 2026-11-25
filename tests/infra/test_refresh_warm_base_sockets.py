#!/usr/bin/env python3
"""
test_refresh_warm_base_sockets.py — stdlib unittest: scripts/refresh-warm-base.sh
promotes a target without the socket nodes at its top level (task 10230;
dark-factory plans/canonical-build-path-shared-cache-prd.md C4/C12).

A build's private sccache server listens on <build dir>/.reify-sccache.<pid>.sock.
Its supervisor unlinks the socket on close, so only a killed build leaves one
behind; that socket is runtime state, never an artifact, and no lane seeded
from the base may inherit it. Everything else, the canonical-build-path stamp
included, reaches the gen unchanged.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_refresh_warm_base_sockets.sh.

HERMETIC: refresh_warm_base_harness stages the refresh script beside no-op
stubs of every sibling it calls and a `cp` PATH stub that drops
--reflink=always; the lane is a temp git repository on main with an untracked
target/.
"""

import os
import stat
import sys
import tempfile
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(INFRA_DIR))
import refresh_warm_base_harness as refresh_harness  # noqa: E402

ARTIFACT = Path("debug") / "libplaceholder.rlib"
STAMP = Path(".reify-build-root")
SOCKET = Path(".reify-sccache.4242.sock")


def sockets_under(root):
    found = []
    for directory, dirnames, filenames in os.walk(root, followlinks=False):
        for name in dirnames + filenames:
            path = Path(directory, name)
            if stat.S_ISSOCK(os.lstat(path).st_mode):
                found.append(path)
    return found


class SocketsAreNotPromoted(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-refresh-promotion-")
        self.addCleanup(tmp.cleanup)
        self.tmp = Path(tmp.name)
        self.stub_dir = self.tmp / "stub-bin"
        self.stub_dir.mkdir()
        refresh_harness.install_cp_stub(self.stub_dir)
        self.refresh = refresh_harness.stage_refresh(self.tmp / "scripts")
        self.lane = self.tmp / "lane"
        refresh_harness.init_lane(self.lane)
        (self.lane / "tracked.txt").write_text("tracked\n")
        refresh_harness.git(self.lane, "add", "--", "tracked.txt")
        self.head = refresh_harness.commit(self.lane, "-m", "fixture: one tracked file")
        self.target = self.lane / "target"
        (self.target / ARTIFACT).parent.mkdir(parents=True)
        (self.target / ARTIFACT).write_text("placeholder artifact\n")
        (self.target / STAMP).write_text("/opt/reify-canon/ws\n")
        os.mknod(self.target / SOCKET, stat.S_IFSOCK | 0o600)
        self.base = self.tmp / "base"

    def test_the_gen_carries_the_target_without_its_socket(self):
        result = refresh_harness.run_refresh(self.refresh, self.stub_dir, self.target,
                                             self.base, self.head)
        self.assertEqual(result.returncode, 0, result.stderr)

        gen = Path(os.readlink(self.base))
        self.assertEqual(sockets_under(gen), [], result.stderr)
        for kept in (ARTIFACT, STAMP):
            self.assertEqual((gen / kept).read_bytes(), (self.target / kept).read_bytes(),
                             f"{kept} did not reach the gen unchanged")
        self.assertTrue(stat.S_ISSOCK(os.lstat(self.target / SOCKET).st_mode),
                        "the refresh mutated the advancing target")


if __name__ == "__main__":
    unittest.main()
