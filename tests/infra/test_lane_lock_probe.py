#!/usr/bin/env python3
"""
test_lane_lock_probe.py — stdlib unittest for scripts/lib_lane_lock.sh.

The lib is the SINGLE tri-state warm-lane lock probe shared by
scripts/warm-lane-audit.sh (`_probe_live`) and scripts/warm-lane-lock-guard.sh
(`_probe`). It answers one question — does an exclusive holder occupy this
lane's lock inode — in three states, IDLE / BUSY / UNMEASURABLE, and carries
NO fail direction: each caller maps UNMEASURABLE its own way (the guard fails
OPEN, the audit fails CLOSED). The reasoning lives in
docs/design/merge-verify-lane-dispatch-seam.md §3.

This file therefore asserts the MEASUREMENT and the two invariants both callers
share — A1 non-mutating, A2 shared/non-blocking/released-at-once — plus the
tri-state discrimination itself. The per-caller fail directions are asserted in
the callers' own suites (test_warm_lane_lock_guard.sh Block D,
test_warm_lane_audit.sh Block T).

WHAT RUNS THIS: not the gate directly. run_all.sh discovers `test_*.sh` only,
so the discovered member is the thin wrapper tests/infra/test_lane_lock_probe.sh,
which invokes this file. A bare .py here would be silently never run.

HOW HOLDERS ARE TAKEN: in-process, with fcntl.flock on a file this process
opens. Python's open() sets O_CLOEXEC by default, so the probe subprocess opens
a genuinely independent OFD and contends for real — and there is no background
child, pid file or READY handshake to flake on. flock(2) is per-OFD, so a
second open() in THIS process is also an independent contender, which is how
"the holder still holds it afterwards" is checked.

Fixtures live under tempfile dirs; nothing is committed.
"""

import fcntl
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
LIB_PATH = REPO_ROOT / "scripts" / "lib_lane_lock.sh"

# Errexit is ON deliberately: both real callers run under `set -euo pipefail`,
# so a probe that returned non-zero would abort them instead of degrading. And
# `set -u` is what makes "the lib always sets BOTH output variables" an
# assertion rather than an aspiration — an unset one aborts the shell.
_PREAMBLE = 'set -euo pipefail; source "$1";'
_EPILOGUE = 'printf "%s\\n%s" "$LANE_LOCK_PROBE_STATE" "$LANE_LOCK_PROBE_DETAIL"'

IDLE, BUSY, UNMEASURABLE = "IDLE", "BUSY", "UNMEASURABLE"


class ProbeResult:
    """One (state, detail, returncode, stderr) observation of lane_lock_probe."""

    def __init__(self, completed):
        self.returncode = completed.returncode
        self.stderr = completed.stderr
        state, _, detail = completed.stdout.partition("\n")
        self.state = state
        self.detail = detail


class LaneLockProbeTestCase(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.tmpdir = Path(tmp.name)

    # ── fixture helpers ──────────────────────────────────────────────────
    def probe(self, lock_path, flock_bin="flock"):
        """Drive lane_lock_probe in a fresh bash, returning a ProbeResult.

        flock_bin=None omits the second argument entirely, exercising the
        documented default.
        """
        if flock_bin is None:
            script = f'{_PREAMBLE} lane_lock_probe "$2"; {_EPILOGUE}'
            argv = ["bash", "-c", script, "_", str(LIB_PATH), str(lock_path)]
        else:
            script = f'{_PREAMBLE} lane_lock_probe "$2" "$3"; {_EPILOGUE}'
            argv = [
                "bash", "-c", script, "_",
                str(LIB_PATH), str(lock_path), str(flock_bin),
            ]
        return ProbeResult(
            subprocess.run(argv, capture_output=True, text=True, check=False)
        )

    def make_lock(self, name="lane.lock", content="") -> Path:
        lock = self.tmpdir / name
        lock.write_text(content)
        return lock

    def hold(self, lock: Path, mode=fcntl.LOCK_EX) -> int:
        """Take a real flock on `lock` for the rest of the test."""
        fd = os.open(str(lock), os.O_RDONLY)
        self.addCleanup(os.close, fd)
        fcntl.flock(fd, mode | fcntl.LOCK_NB)
        return fd

    def exclusive_lock_is_takeable(self, lock: Path) -> bool:
        """True iff an INDEPENDENT OFD can take LOCK_EX on `lock` right now."""
        fd = os.open(str(lock), os.O_RDONLY)
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return False
        else:
            fcntl.flock(fd, fcntl.LOCK_UN)
            return True
        finally:
            os.close(fd)

    def make_stub(self, name: str, exit_code: int) -> Path:
        """An executable flock stand-in that exits with a fixed status."""
        stub = self.tmpdir / name
        stub.write_text(f"#!/usr/bin/env bash\nexit {exit_code}\n")
        stub.chmod(0o755)
        return stub

    def assertProbe(self, result: ProbeResult, expected_state: str):
        self.assertEqual(
            0, result.returncode,
            f"lane_lock_probe must always return 0; stderr={result.stderr!r}",
        )
        self.assertEqual(
            expected_state, result.state,
            f"detail={result.detail!r} stderr={result.stderr!r}",
        )
        # P11, asserted on every observation rather than once: DETAIL is
        # non-empty iff the state is UNMEASURABLE.
        if expected_state == UNMEASURABLE:
            self.assertNotEqual("", result.detail.strip())
        else:
            self.assertEqual("", result.detail.strip())

    # ── P1-P4: the tri-state discrimination ──────────────────────────────
    def test_p1_unheld_existing_lock_is_idle(self):
        self.assertProbe(self.probe(self.make_lock()), IDLE)

    def test_p1b_flock_bin_argument_is_optional(self):
        self.assertProbe(self.probe(self.make_lock(), flock_bin=None), IDLE)

    def test_p2_absent_lock_is_idle_and_is_never_created(self):
        # A1: a missing lock POSITIVELY means no consumer ever took this lane.
        # It is IDLE, and neither it nor its parent may be materialized.
        parent = self.tmpdir / "no-such-mount"
        lock = parent / "lane.lock"
        self.assertProbe(self.probe(lock), IDLE)
        self.assertFalse(lock.exists(), "the probe created the lock file")
        self.assertFalse(parent.exists(), "the probe created the lock's parent")

    def test_p2b_absent_lock_beats_an_unresolvable_flock(self):
        # Ordering pin: "no consumer ever took this lane" is a POSITIVE answer
        # that needs no flock, so it must be reached before the tool check. A
        # caller that probes a whole pool (the audit) would otherwise report
        # every lockless lane as occupied the moment flock broke.
        lock = self.tmpdir / "never-taken.lock"
        missing = self.tmpdir / "nowhere" / "flock"
        self.assertProbe(self.probe(lock, missing), IDLE)
        self.assertProbe(self.probe(lock, self.make_stub("f1c", 1)), IDLE)
        self.assertFalse(lock.exists())

    def test_p3_exclusive_holder_is_busy(self):
        lock = self.make_lock()
        self.hold(lock, fcntl.LOCK_EX)
        self.assertProbe(self.probe(lock), BUSY)

    def test_p4_shared_holder_is_idle(self):
        # Pins `-s`, not `-x`. A shared holder is not an occupant: every real
        # lane consumer holds an EXCLUSIVE flock while live, and two readers
        # (a second audit run, the lock guard) must never contend. The audit
        # suite's Block Q and the guard's B5a/B5b both rely on this.
        lock = self.make_lock()
        self.hold(lock, fcntl.LOCK_SH)
        self.assertProbe(self.probe(lock), IDLE)

    # ── P5-P6: A2, released at once and never stolen ─────────────────────
    def test_p5_idle_probe_releases_the_lock(self):
        lock = self.make_lock()
        self.assertProbe(self.probe(lock), IDLE)
        self.assertTrue(
            self.exclusive_lock_is_takeable(lock),
            "the probe retained the lock after reporting IDLE",
        )

    def test_p6_busy_probe_neither_steals_nor_retains(self):
        lock = self.make_lock()
        self.hold(lock, fcntl.LOCK_EX)
        self.assertProbe(self.probe(lock), BUSY)
        self.assertFalse(
            self.exclusive_lock_is_takeable(lock),
            "the holder lost its lock across the probe",
        )

    # ── P7-P9: UNMEASURABLE, by each of its three causes ─────────────────
    def test_p7_tool_fault_is_unmeasurable_even_against_a_real_holder(self):
        # Non-vacuous, mirroring the guard suite's D1: a REAL exclusive holder
        # is present, so the only thing distinguishing this from P3 is the
        # stub's bare exit 1. A tool fault must never be laundered into
        # evidence of contention.
        lock = self.make_lock()
        self.hold(lock, fcntl.LOCK_EX)
        stub = self.make_stub("flock_broken", 1)
        self.assertProbe(self.probe(lock, stub), UNMEASURABLE)

    def test_p8_unresolvable_flock_is_unmeasurable(self):
        lock = self.make_lock()
        missing = self.tmpdir / "nowhere" / "flock"
        self.assertProbe(self.probe(lock, missing), UNMEASURABLE)

    @unittest.skipIf(os.geteuid() == 0, "root bypasses file permissions")
    def test_p9_unreadable_lock_is_unmeasurable(self):
        lock = self.make_lock()
        # Restore BEFORE the tmpdir teardown, which a mode-000 file left in
        # place could otherwise wedge.
        self.addCleanup(lock.chmod, 0o644)
        lock.chmod(0o000)
        self.assertProbe(self.probe(lock), UNMEASURABLE)

    # ── P10: the -E conflict-status plumbing ─────────────────────────────
    def test_p10_conflict_status_is_busy(self):
        # Pins LANE_LOCK_PROBE_CONFLICT_RC and the `-E` request together: no
        # holder exists here, so BUSY can only come from the stub's status.
        lock = self.make_lock()
        stub = self.make_stub("flock_conflict", 124)
        self.assertProbe(self.probe(lock, stub), BUSY)

    def test_p10b_conflict_rc_constant_is_exported_and_matches(self):
        out = subprocess.run(
            ["bash", "-c",
             'set -euo pipefail; source "$1"; printf "%s" "$LANE_LOCK_PROBE_CONFLICT_RC"',
             "_", str(LIB_PATH)],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(0, out.returncode, out.stderr)
        self.assertEqual("124", out.stdout)

    def test_p10c_a_bare_one_is_not_read_as_would_block(self):
        # The whole reason `-E` exists: `flock -n` returns a bare 1 on
        # contention, indistinguishable from "flock itself failed". Exit 1
        # must land on UNMEASURABLE, never BUSY — the complement of P10 on the
        # identical fixture.
        lock = self.make_lock()
        self.assertProbe(self.probe(lock, self.make_stub("f1", 1)), UNMEASURABLE)
        self.assertProbe(self.probe(lock, self.make_stub("f124", 124)), BUSY)

    # ── P11: DETAIL is a reason, and only for the state that has one ─────
    def test_p11_detail_is_non_empty_iff_unmeasurable(self):
        # assertProbe checks this on EVERY observation in the file; this case
        # states the invariant once in its own right, over all three states.
        lock = self.make_lock()
        self.assertEqual("", self.probe(lock).detail.strip())
        self.assertEqual(
            "", self.probe(lock, self.make_stub("f124b", 124)).detail.strip()
        )
        self.assertNotEqual(
            "", self.probe(lock, self.make_stub("f1b", 1)).detail.strip()
        )

    # ── P12-P13: A1 byte-level, and the source guard ─────────────────────
    def test_p12_probe_does_not_mutate_the_lock_file(self):
        lock = self.make_lock(content="seeded\n")
        before = os.stat(str(lock))
        self.assertProbe(self.probe(lock), IDLE)
        after = os.stat(str(lock))
        self.assertEqual(before.st_mode, after.st_mode)
        self.assertEqual(before.st_ino, after.st_ino)
        self.assertEqual(before.st_size, after.st_size)
        self.assertEqual("seeded\n", lock.read_text())

    def test_p13_double_sourcing_is_a_no_op(self):
        lock = self.make_lock()
        out = subprocess.run(
            ["bash", "-c",
             'set -euo pipefail; source "$1"; source "$1"; '
             'lane_lock_probe "$2"; ' + _EPILOGUE,
             "_", str(LIB_PATH), str(lock)],
            capture_output=True, text=True, check=False,
        )
        self.assertEqual(0, out.returncode, out.stderr)
        self.assertEqual(IDLE, out.stdout.partition("\n")[0])


if __name__ == "__main__":
    unittest.main()
