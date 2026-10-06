#!/usr/bin/env python3
"""
test_hooks_armed_guard.py — stdlib unittest for scripts/hooks-armed-guard.sh,
the primitive that decides whether hooks/reference-transaction's refs/stash arm
is actually wired into a worktree (task 6059).

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_hooks_armed_guard.sh, which invokes this
file.

FIXTURE: every test builds a hermetic store in a private tempdir:
  - `main/`        a `git init -b main` checkout carrying COPIES of the REAL
                   hooks/reference-transaction and hooks/main-gate-lib.sh,
                   committed, plus a tracked wip.txt;
  - `lanes/lane`   a linked worktree of main/ — the shape of a warm lane.
The store has no core.hooksPath by default, so git resolves the lane's hooks
to main/.git/hooks (samples only): the default lane is DARK.

SEED WIRING: SeedWiring drives the REAL scripts/seed-warm-lane.sh against the
fixture lane (`--fresh-checkout`, a stub `cp` standing in for the reflink
clone), so the acquire-time call into the real guard is exercised end to end.

ORACLE: a REAL `git stash push` in the lane under REIFY_STASH_GUARD_ENFORCE=1.
Every guard verdict is asserted next to what git actually does, so the guard
cannot drift from reality. Assertions are on exit codes, file state and path
tokens only; nothing here greps the guard's source.
"""

import functools
import os
import shutil
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
GUARD = REPO_ROOT / "scripts" / "hooks-armed-guard.sh"
SEED = REPO_ROOT / "scripts" / "seed-warm-lane.sh"
GIT_ENV_SCRUB_LIB = REPO_ROOT / "scripts" / "lib_git_env_scrub.sh"
HOOK_FILES = ("reference-transaction", "main-gate-lib.sh")
GATE_LOG = "reify-main-gate.log"

WIP = "wip.txt"
WIP_COMMITTED = "committed\n"

# Ambient state that would steer a child git or the hooks away from the
# fixture: `git -c` injection, the stash/main-gate rollout switches, and the
# warm-lane knobs the seed tests set explicitly.
_AMBIENT_KEYS = frozenset({"GIT_CONFIG_PARAMETERS", "GIT_CONFIG_COUNT"})
_AMBIENT_PREFIXES = (
    "GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_",
    "GIT_AUTHOR_", "GIT_COMMITTER_",
    "REIFY_STASH_GUARD_", "REIFY_MAIN_GATE_", "REIFY_WARM_LANE_",
)


@functools.lru_cache(maxsize=None)
def repo_redirect_vars():
    """The one list of repo-redirect git variables, read from its bash home."""
    completed = subprocess.run(
        ["bash", "-c", 'source "$1"; printf %s "$REIFY_GIT_ENV_SCRUB_VARS"',
         "_", str(GIT_ENV_SCRUB_LIB)],
        capture_output=True, text=True, check=True,
    )
    names = frozenset(completed.stdout.split())
    if not names:
        raise RuntimeError(f"{GIT_ENV_SCRUB_LIB} yielded no REIFY_GIT_ENV_SCRUB_VARS")
    return names


def _is_ambient(key):
    return (key in repo_redirect_vars() or key in _AMBIENT_KEYS
            or key.startswith(_AMBIENT_PREFIXES))


def _diag(completed):
    return (f"\n--- exit {completed.returncode} ---\n--- stdout ---\n"
            f"{completed.stdout}\n--- stderr ---\n{completed.stderr}")


def _inode(path):
    return path.stat().st_ino


class GuardFixture(unittest.TestCase):
    """A main checkout with the real hooks committed, and one linked lane."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="stash-liveness-")
        self.addCleanup(tmp.cleanup)
        self.tmpdir = Path(tmp.name).resolve()
        self.env = self._fixture_env()

        self.main = self.tmpdir / "main"
        self.main.mkdir()
        self.git(self.main, "init", "-q", "-b", "main")
        (self.main / "hooks").mkdir()
        for name in HOOK_FILES:
            shutil.copy2(REPO_ROOT / "hooks" / name, self.main / "hooks" / name)
        self._make_executable(self.main / "hooks" / "reference-transaction")
        (self.main / WIP).write_text(WIP_COMMITTED)
        self.git(self.main, "add", "-A")
        self.git(self.main, "commit", "-q", "-m", "fixture base")

        self.lane = self.tmpdir / "lanes" / "lane"
        self.git(self.main, "worktree", "add", "-q", "-b", "task/1", str(self.lane))

    def _fixture_env(self):
        env = {k: v for k, v in os.environ.items() if not _is_ambient(k)}
        empty_global = self.tmpdir / "gitconfig-global"
        empty_global.write_text("")
        env.update({
            "GIT_CONFIG_GLOBAL": str(empty_global),
            "GIT_CONFIG_NOSYSTEM": "1",
            # A plain dir in the tempdir must never discover an enclosing repo.
            "GIT_CEILING_DIRECTORIES": str(self.tmpdir),
            "GIT_AUTHOR_NAME": "Fixture", "GIT_AUTHOR_EMAIL": "fixture@example.test",
            "GIT_COMMITTER_NAME": "Fixture", "GIT_COMMITTER_EMAIL": "fixture@example.test",
        })
        return env

    @staticmethod
    def _make_executable(path):
        path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

    def git(self, where, *args, check=True, extra_env=None):
        env = dict(self.env, **(extra_env or {}))
        completed = subprocess.run(["git", "-C", str(where), *args], env=env,
                                   capture_output=True, text=True)
        if check and completed.returncode != 0:
            self.fail(f"git {' '.join(args)} failed in {where}" + _diag(completed))
        return completed

    def git_out(self, where, *args):
        return self.git(where, *args).stdout.strip()

    def run_guard(self, *argv, env=None):
        return subprocess.run([str(GUARD), *map(str, argv)], env=env or self.env,
                              capture_output=True, text=True)

    def run_guard_without_tmp(self, *argv):
        """The guard on a host whose TMPDIR cannot hold the probe's scratch repo."""
        return self.run_guard(*argv, env=dict(self.env, TMPDIR=str(self.tmpdir / "no-tmp")))

    # -- store state ------------------------------------------------------

    def pin(self, value, lane=None):
        """Pin core.hooksPath in the lane's own config.worktree."""
        self.git(self.main, "config", "extensions.worktreeConfig", "true")
        self.git(lane or self.lane, "config", "--worktree", "core.hooksPath", value)

    def clobber_shared(self, value):
        """The shared-config write Claude Code's worktree feature makes."""
        self.git(self.main, "config", "core.hooksPath", value)

    def hooks_dir(self, lane=None):
        return self.git_out(lane or self.lane, "rev-parse", "--path-format=absolute",
                            "--git-path", "hooks")

    def lane_worktree_config(self, lane=None):
        git_dir = self.git_out(lane or self.lane, "rev-parse", "--path-format=absolute",
                               "--git-dir")
        return Path(git_dir) / "config.worktree"

    def gate_log(self):
        common = self.git_out(self.lane, "rev-parse", "--path-format=absolute",
                              "--git-common-dir")
        return Path(common) / GATE_LOG

    def config_snapshot(self, lane=None):
        """Inodes of the shared config and the lane's config.worktree (or None)."""
        wt_cfg = self.lane_worktree_config(lane)
        return (_inode(self.main / ".git" / "config"),
                _inode(wt_cfg) if wt_cfg.exists() else None)

    def log_size(self):
        log = self.gate_log()
        return log.stat().st_size if log.exists() else None

    def liveness_lines(self):
        log = self.gate_log()
        if not log.exists():
            return []
        return [line for line in log.read_text().splitlines()
                if "stash-guard: liveness:" in line]

    # -- the oracle ---------------------------------------------------------

    def stash_push(self, lane=None):
        """A REAL `git stash push` under ENFORCE: (rc, refs/stash created, WIP intact)."""
        lane = lane or self.lane
        dirty = "dirty for the oracle\n"
        (lane / WIP).write_text(dirty)
        completed = self.git(lane, "stash", "push", "-m", "oracle", check=False,
                             extra_env={"REIFY_STASH_GUARD_ENFORCE": "1"})
        created = self.git(lane, "rev-parse", "-q", "--verify", "refs/stash",
                           check=False).returncode == 0
        intact = (lane / WIP).read_text() == dirty
        return completed, created, intact

    def assert_push_refused(self, lane=None):
        completed, created, intact = self.stash_push(lane)
        self.assertNotEqual(completed.returncode, 0,
                            "oracle: git stash push was NOT refused" + _diag(completed))
        self.assertFalse(created, "oracle: a refused push still created refs/stash")
        self.assertTrue(intact, "oracle: a refused push lost the working-tree WIP")

    def assert_push_succeeds(self, lane=None):
        completed, created, _ = self.stash_push(lane)
        self.assertEqual(completed.returncode, 0,
                         "oracle: git stash push was refused" + _diag(completed))
        self.assertTrue(created, "oracle: a successful push created no refs/stash")

    def assert_rc(self, completed, rc):
        self.assertEqual(completed.returncode, rc, _diag(completed))

    def _assert_one_liveness_line_naming_the_lane(self):
        lines = self.liveness_lines()
        self.assertEqual(len(lines), 1, lines)
        self.assertIn(str(self.lane), lines[0])


class CheckContract(GuardFixture):
    """`check`: 0 armed | 1 not armed | 3 could not check; read-only."""

    def test_t1_default_lane_is_dark(self):
        completed = self.run_guard("check", self.lane)
        self.assert_rc(completed, 1)
        self.assertIn(self.hooks_dir(), completed.stderr)
        self.assert_push_succeeds()

    def test_t2_pinned_and_live_is_armed(self):
        self.pin("hooks")
        self.assert_rc(self.run_guard("check", self.lane), 0)
        self.assert_push_refused()

    def test_t3_live_but_unpinned_is_not_armed(self):
        self.clobber_shared("hooks")
        self.assert_rc(self.run_guard("check", self.lane), 1)
        self.assert_push_refused()

    def test_t4_non_executable_hook_is_not_armed(self):
        self.pin("hooks")
        hook = self.lane / "hooks" / "reference-transaction"
        hook.chmod(hook.stat().st_mode & ~(stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH))
        self.assert_rc(self.run_guard("check", self.lane), 1)
        self.assert_push_succeeds()

    def test_t5_present_but_not_gating_is_not_armed(self):
        self.pin("hooks")
        hook = self.lane / "hooks" / "reference-transaction"
        hook.write_text("#!/bin/sh\ncat >/dev/null\nexit 0\n")
        self._make_executable(hook)
        self.assert_rc(self.run_guard("check", self.lane), 1)
        self.assert_push_succeeds()

    def _assert_check_is_read_only(self, expected_rc):
        before = (self.config_snapshot(), self.log_size())
        self.assert_rc(self.run_guard("check", self.lane), expected_rc)
        self.assertEqual((self.config_snapshot(), self.log_size()), before)
        self.assertIsNone(self.log_size(), "check wrote the store's main-gate log")

    def test_t6_check_is_read_only_on_an_armed_lane(self):
        self.pin("hooks")
        self._assert_check_is_read_only(0)

    def test_t6_check_is_read_only_on_a_dark_lane(self):
        self._assert_check_is_read_only(1)

    def test_t7_plain_directory_cannot_be_checked(self):
        plain = self.tmpdir / "plain"
        plain.mkdir()
        self.assert_rc(self.run_guard("check", plain), 3)

    def test_t7_nonexistent_path_cannot_be_checked(self):
        self.assert_rc(self.run_guard("check", self.tmpdir / "no-such-dir"), 3)

    def test_t7_usage_errors_exit_3(self):
        self.assert_rc(self.run_guard(), 3)
        self.assert_rc(self.run_guard("frobnicate", self.lane), 3)

    def test_t7_help_exits_0(self):
        self.assert_rc(self.run_guard("--help"), 0)

    def test_t7_unrunnable_probe_cannot_check(self):
        # The hook gates (the oracle refuses), so a 1 here would blame a
        # healthy hook for the host's broken tmp dir.
        self.pin("hooks")
        self.assert_rc(self.run_guard_without_tmp("check", self.lane), 3)
        self.assert_push_refused()


class ArmContract(GuardFixture):
    """`arm`: 0 armed | 2 the pin cannot fix it | * failed; lane-scoped repair."""

    def test_t8_arm_repairs_a_clobbered_lane(self):
        clobbered = str(self.tmpdir / "nonexistent")
        self.clobber_shared(clobbered)
        self.assert_rc(self.run_guard("arm", self.lane), 0)
        self._assert_one_liveness_line_naming_the_lane()
        self.assert_rc(self.run_guard("check", self.lane), 0)
        self.assertEqual(self.git_out(self.lane, "config", "--worktree", "--get",
                                      "core.hooksPath"), "hooks")
        self.assertEqual(self.git_out(self.main, "config", "--local", "--get",
                                      "core.hooksPath"), clobbered)
        self.assert_push_refused()

    def test_t9_arm_on_an_armed_lane_writes_nothing(self):
        self.pin("hooks")
        before = (self.config_snapshot(), self.log_size())
        self.assert_rc(self.run_guard("arm", self.lane), 0)
        self.assertEqual((self.config_snapshot(), self.log_size()), before)

    def test_t10_arm_cannot_fix_a_non_executable_hook(self):
        self.pin("hooks")
        hook = self.lane / "hooks" / "reference-transaction"
        hook.chmod(hook.stat().st_mode & ~(stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH))
        before = self.config_snapshot()
        self.assert_rc(self.run_guard("arm", self.lane), 2)
        self.assertEqual(self.config_snapshot(), before)
        self._assert_one_liveness_line_naming_the_lane()
        self.assert_rc(self.run_guard("check", self.lane), 1)
        self.assert_push_succeeds()

    def test_t11_arm_overwrites_a_dark_non_canonical_pin(self):
        self.pin("/dev/null")
        self.assert_rc(self.run_guard("arm", self.lane), 0)
        self.assert_rc(self.run_guard("check", self.lane), 0)
        self.assert_push_refused()

    def test_t12_arm_on_a_plain_directory_cannot_check(self):
        plain = self.tmpdir / "plain"
        plain.mkdir()
        self.assert_rc(self.run_guard("arm", plain), 3)
        self.assertEqual(list(plain.iterdir()), [])

    def test_t12_arm_with_an_unrunnable_probe_cannot_check(self):
        # The pin does fix this lane (the oracle refuses), so a 2 and a
        # "still DARK" line would both be false.
        self.clobber_shared(str(self.tmpdir / "nonexistent"))
        self.assert_rc(self.run_guard_without_tmp("arm", self.lane), 3)
        self.assertEqual(self.liveness_lines(), [])
        self.assert_push_refused()


class SeedWiring(GuardFixture):
    """A `--fresh-checkout` acquire arms the lane it hands over, fail-open."""

    def setUp(self):
        super().setUp()
        base = self.tmpdir / "base"
        self.base_target = base / "target"
        self.base_target.mkdir(parents=True)
        (base / ".warm-base-meta").write_text("RUSTFLAGS=\nINVOCATION=\n")
        stub_bin = self.tmpdir / "stub-bin"
        stub_bin.mkdir()
        # No reflink on a tmp fs: the stub stands in for the clone.
        cp = stub_bin / "cp"
        cp.write_text('#!/bin/sh\nfor arg; do last="$arg"; done\nmkdir -p "$last"\n')
        self._make_executable(cp)
        self.seed_env = dict(
            self.env,
            PATH=f"{stub_bin}{os.pathsep}{self.env.get('PATH', '')}",
            RUSTFLAGS="",
            # Keeps the rerere guard's shared-config writes out of the fixture.
            REIFY_WARM_LANE_RERERE_ARM="0",
        )

    def run_seed(self):
        completed = subprocess.run(
            ["bash", str(SEED), str(self.base_target), str(self.lane), "--fresh-checkout"],
            env=self.seed_env, capture_output=True, text=True)
        self.assert_rc(completed, 0)
        self.assertEqual(completed.stdout, f"{self.lane / 'target'}\n", _diag(completed))
        return completed

    def test_t13_acquire_arms_a_darkened_lane(self):
        self.clobber_shared(str(self.tmpdir / "nonexistent"))
        self.run_seed()
        self.assert_rc(self.run_guard("check", self.lane), 0)
        self.assert_push_refused()

    def test_t14_acquire_is_fail_open_when_the_pin_cannot_help(self):
        hook = self.lane / "hooks" / "reference-transaction"
        hook.chmod(hook.stat().st_mode & ~(stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH))
        self.run_seed()
        # arm's exit 2, seen through seed: it pinned the lane, re-checked it,
        # and logged it still dark.
        self.assertEqual(self.git_out(self.lane, "config", "--worktree", "--get",
                                      "core.hooksPath"), "hooks")
        self._assert_one_liveness_line_naming_the_lane()
        self.assert_rc(self.run_guard("check", self.lane), 1)
        self.assert_push_succeeds()

    def test_t15_acquire_is_fail_open_when_the_guard_itself_fails(self):
        # setup-main-gate-worktree-config.sh refuses a store whose shared
        # config says core.bare=true, so arm fails before it can pin or log.
        self.git(self.main, "config", "core.bare", "true")
        self.run_seed()
        self.assertFalse(self.lane_worktree_config().exists())
        self.assertEqual(self.liveness_lines(), [])
        self.assert_rc(self.run_guard("check", self.lane), 1)


if __name__ == "__main__":
    unittest.main()
