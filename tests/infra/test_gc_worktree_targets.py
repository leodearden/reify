#!/usr/bin/env python3
"""
test_gc_worktree_targets.py — stdlib unittest for scripts/gc-worktree-targets.sh,
the cron sweep that removes idle worktrees' cargo target/ dirs (task 9246).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_gc_worktree_targets.sh.

SAFETY: the script's default sweep root, /home/leo/src/reify/.worktrees, is the
LIVE task-worktree pool on the workstation. Two independent layers keep every
run inside a fixture:
  1. REIFY_GC_WORKTREE_DIR points the sweep at <fixture>/.worktrees;
  2. `rm` and `du` are PATH stubs that record their argv and REFUSE (exit 97,
     touching nothing) any operand whose realpath is outside the fixture root,
     else exec the real binary. Every test asserts no REFUSED line was recorded.
`flock` is a PATH stub too: it records its argv and, when told to, touches a
target before taking the lock, standing in for a lane that builds between the
sweep's idle check and its lock.
"""

import fcntl
import os
import re
import shutil
import subprocess
import tempfile
import time
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "gc-worktree-targets.sh"

TWO_DAYS_AGO = time.time() - 2 * 24 * 3600

GUARD_STUB = """#!/usr/bin/env bash
echo "{name} $*" >> "$REIFY_TEST_CALLS_FILE"
operands_only=0
for a in "$@"; do
    if [ "$operands_only" = 0 ]; then
        case "$a" in
            --) operands_only=1; continue ;;
            -*) continue ;;
        esac
    fi
    real="$(realpath -m -- "$a")"
    case "$real/" in
        "$REIFY_TEST_FIXTURE_ROOT"/*) ;;
        *)
            echo "REFUSED {name} $a" >> "$REIFY_TEST_CALLS_FILE"
            echo "REFUSED: {name} operand outside the fixture: $a" >&2
            exit 97 ;;
    esac
done
if [ "$REIFY_TEST_FAILING_COMMAND" = "{name}" ]; then
    echo "{name}: simulated failure" >&2
    exit 1
fi
exec {real} "$@"
"""

BUILDING_FLOCK_STUB = """#!/usr/bin/env bash
echo "flock $*" >> "$REIFY_TEST_CALLS_FILE"
if [ -n "$REIFY_TEST_BUILD_BEFORE_LOCK" ]; then
    touch -- "$REIFY_TEST_BUILD_BEFORE_LOCK"
fi
exec {real} "$@"
"""


class GcWorktreeTargets(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="test-gc-worktree-targets-")
        self.addCleanup(tmp.cleanup)
        self.root = Path(os.path.realpath(tmp.name))
        self.stub_dir = self.root / "stub-bin"
        self.stub_dir.mkdir()
        for name in ("rm", "du"):
            stub = self.stub_dir / name
            stub.write_text(GUARD_STUB.format(name=name, real=shutil.which(name)))
            stub.chmod(0o755)
        flock_stub = self.stub_dir / "flock"
        flock_stub.write_text(BUILDING_FLOCK_STUB.format(real=shutil.which("flock")))
        flock_stub.chmod(0o755)
        self.calls_file = self.root / "calls.log"
        self.calls_file.touch()
        self.worktrees = self.root / ".worktrees"
        self.merge_verify = self.make_target("_merge-verify", TWO_DAYS_AGO)
        self.idle = self.make_target("task-idle", TWO_DAYS_AGO)
        self.held = self.make_target("task-held", TWO_DAYS_AGO)
        self.fresh = self.make_target("task-fresh", time.time())

    def make_target(self, worktree, mtime):
        target = self.worktrees / worktree / "target"
        (target / "debug").mkdir(parents=True)
        (target / "debug" / "artefact").write_text("built\n")
        os.utime(target, (mtime, mtime))
        return target

    def hold_lane_lock(self, worktree):
        """Take LOCK_EX on <worktree>.lock the way dark_factory does."""
        fd = os.open(str(self.worktrees / f"{worktree}.lock"), os.O_RDWR | os.O_CREAT, 0o644)
        self.addCleanup(os.close, fd)
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)

    def run_gc(self, env=None):
        run_env = {k: v for k, v in os.environ.items()
                   if not k.startswith("REIFY_") and k != "DRY_RUN"}
        run_env.update({
            "PATH": f"{self.stub_dir}{os.pathsep}{run_env.get('PATH', '')}",
            "REIFY_GC_WORKTREE_DIR": str(self.worktrees),
            "REIFY_TEST_CALLS_FILE": str(self.calls_file),
            "REIFY_TEST_FIXTURE_ROOT": str(self.root),
        })
        run_env.update(env or {})
        result = subprocess.run(["bash", str(SCRIPT)], capture_output=True, text=True,
                                env=run_env, cwd=self.root, check=False)
        calls = self.calls_file.read_text()
        self.assertNotIn("REFUSED", calls,
                         f"a guard stub refused an operand outside the fixture\n{result.stderr}")
        return result, calls

    def test_sweep_removes_only_idle_unlocked_targets_and_never_touches_merge_verify(self):
        self.hold_lane_lock("task-held")

        result, calls = self.run_gc()

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.merge_verify.is_dir(), "the _merge-verify target was removed")
        self.assertNotIn("_merge-verify", calls,
                         "rm or du named _merge-verify: it is exempt before any size or lock work")
        self.assertFalse(self.idle.exists(), "the idle, unlocked target survived")
        self.assertTrue(self.held.is_dir(), "a target under a held lane lock was removed")
        self.assertTrue(self.fresh.is_dir(), "a recently built target was removed")

    def test_dry_run_removes_nothing(self):
        result, _ = self.run_gc(env={"DRY_RUN": "1"})

        self.assertEqual(result.returncode, 0, result.stderr)
        for target in (self.merge_verify, self.idle, self.held, self.fresh):
            self.assertTrue(target.is_dir(), f"dry run removed {target}")

    def test_a_target_built_between_the_idle_check_and_the_lane_lock_is_kept(self):
        shutil.rmtree(self.held)  # task-idle is now the only lock the sweep takes

        result, _ = self.run_gc(env={"REIFY_TEST_BUILD_BEFORE_LOCK": str(self.idle)})

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.idle.is_dir(), "a target built just before its lane lock was removed")

    def test_a_failed_removal_fails_the_sweep_naming_the_target(self):
        result, _ = self.run_gc(env={"REIFY_TEST_FAILING_COMMAND": "rm"})

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertRegex(result.stderr, re.compile(
            rf"ERROR: removing {re.escape(str(self.worktrees))}/task-(idle|held)/target failed"))

    def test_an_unopenable_lane_lock_fails_the_sweep_and_removes_nothing(self):
        if os.geteuid() == 0:
            self.skipTest("root opens a lock file in a read-only directory")
        self.worktrees.chmod(0o555)
        self.addCleanup(self.worktrees.chmod, 0o755)

        result, calls = self.run_gc()

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertRegex(result.stderr, re.compile(
            rf"ERROR: cannot open lane lock {re.escape(str(self.worktrees))}/task-(idle|held)\.lock"))
        self.assertTrue(self.idle.is_dir(), "a target was removed without its lane lock")
        self.assertTrue(self.held.is_dir(), "a target was removed without its lane lock")
        self.assertNotIn("rm ", calls)


if __name__ == "__main__":
    unittest.main()
