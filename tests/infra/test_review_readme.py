#!/usr/bin/env python3
"""
test_review_readme.py — stdlib unittest for scripts/review-readme.sh, the
timer-driven job that lets a claude session edit README.md and
docs/getting-started.md in the main checkout and then lands the edit.

WHAT RUNS THIS: run_all.sh discovers `test_*.sh` only, so the discovered member
is the thin wrapper tests/infra/test_review_readme.sh, which invokes this file.

HOW THE SCRIPT IS DRIVEN: every test runs the REAL script as a subprocess
against a hermetic tempdir fixture:
  - `main/`       a `git init -b main` checkout, named to the script through
                  REIFY_MAIN_CHECKOUT (scripts/lib_main_checkout.sh's override);
  - `origin.git`  a bare remote that `main` has been pushed to;
  - `hooks/`      core.hooksPath, holding a pre-commit that records the staged
                  set it was shown and exits with a configurable status;
  - `bin/claude`  a stub standing in for CLAUDE_BIN.

SAFETY: the stub edits NOTHING unless `pwd -P` is the fixture root it was
handed, and run_script ALWAYS points CLAUDE_BIN at the stub. A script that
ignored REIFY_MAIN_CHECKOUT and cd'd into a real checkout therefore produces a
harmless no-edit run, and the real `claude --dangerously-skip-permissions` can
never be spawned from here.

Assertions are on observable outcomes only — commits, identity fields, remote
refs, index and worktree state, the hook's recorded view, exit codes, and files
the script leaves behind. Nothing here greps the script's text.
"""

import functools
import os
import shlex
import stat
import subprocess
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPT = REPO_ROOT / "scripts" / "review-readme.sh"
GIT_ENV_SCRUB_LIB = REPO_ROOT / "scripts" / "lib_git_env_scrub.sh"

README = "README.md"
GETTING_STARTED = "docs/getting-started.md"
TARGETS = (README, GETTING_STARTED)
UNRELATED = "other.txt"

HUMAN_NAME = "Human Tester"
HUMAN_EMAIL = "human@example.test"

# Keys run_script pins itself: the safety seam and the stub's guard. An override
# of any of them could point the script at a real checkout or a real claude.
_PINNED_KEYS = frozenset({"CLAUDE_BIN", "REIFY_MAIN_CHECKOUT", "REVIEW_README_STUB_ROOT"})

# Command-line config (`git -c`) travels to children in these; it outranks the
# fixture's local config, so an inherited core.hooksPath would swap the
# fixture's recording hook for a real one.
_GIT_CONFIG_INJECTION_PREFIXES = ("GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_")
_GIT_CONFIG_INJECTION_KEYS = frozenset({"GIT_CONFIG_PARAMETERS", "GIT_CONFIG_COUNT"})
_GIT_IDENTITY_PREFIXES = ("GIT_AUTHOR_", "GIT_COMMITTER_")


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


def _is_ambient_git_state(key):
    return (
        key in repo_redirect_vars()
        or key in _GIT_CONFIG_INJECTION_KEYS
        or key.startswith(_GIT_CONFIG_INJECTION_PREFIXES)
        or key.startswith(_GIT_IDENTITY_PREFIXES)
    )


def _write_executable(path, text):
    path.write_text(text)
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def _diag(completed):
    return (f"\n--- exit {completed.returncode} ---\n--- stdout ---\n"
            f"{completed.stdout}\n--- stderr ---\n{completed.stderr}")


class ReviewReadmeFixture(unittest.TestCase):
    """A main checkout, a bare origin, a recording hook and a stub claude."""

    def setUp(self):
        tmp = tempfile.TemporaryDirectory(prefix="review-readme-")
        self.addCleanup(tmp.cleanup)
        self.tmpdir = Path(tmp.name).resolve()
        self.main = self.tmpdir / "main"
        self.origin = self.tmpdir / "origin.git"
        self.neutral_cwd = self.tmpdir / "cwd"
        self.neutral_cwd.mkdir()
        self.empty_global_config = self.tmpdir / "empty.gitconfig"
        self.empty_global_config.write_text("")

        self.hook_log = self.tmpdir / "hook-staged.log"
        self.hook_rc = self.tmpdir / "hook-rc"
        self.hook_rc.write_text("0\n")
        self.stub_marker = self.tmpdir / "claude-invoked-in"
        self.stub = self._write_stub_claude()
        hooks_dir = self._write_recording_hook()

        self.main.mkdir()
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", HUMAN_NAME)
        self.git("config", "user.email", HUMAN_EMAIL)
        (self.main / ".git" / "info" / "exclude").write_text("logs/\n")
        (self.main / "docs").mkdir()
        (self.main / README).write_text("# Fixture README\n")
        (self.main / GETTING_STARTED).write_text("# Getting started\n")
        (self.main / UNRELATED).write_text("unrelated line\n")
        self.git("add", "--", *TARGETS, UNRELATED)
        self.fixture_commit("fixture: initial tree")
        self.git("config", "core.hooksPath", str(hooks_dir))

        self.git_bare("init", "-q", "--bare", str(self.origin))
        self.git("remote", "add", "origin", str(self.origin))
        self.git("push", "-q", "origin", "main")
        self.origin_main_at_setup = self.origin_main()

    # ── fixture construction ─────────────────────────────────────────────
    def _write_stub_claude(self):
        bin_dir = self.tmpdir / "bin"
        bin_dir.mkdir()
        stub = bin_dir / "claude"
        _write_executable(stub, f"""#!/bin/sh
here=$(pwd -P)
printf '%s\\n' "$here" > {shlex.quote(str(self.stub_marker))}
[ "$here" = "$REVIEW_README_STUB_ROOT" ] || exit 0
[ -n "${{REVIEW_README_STUB_APPEND:-}}" ] || exit 0
printf '%s\\n' "$REVIEW_README_STUB_APPEND" >> {shlex.quote(README)}
exit 0
""")
        return stub

    def _write_recording_hook(self):
        hooks_dir = self.tmpdir / "hooks"
        hooks_dir.mkdir()
        _write_executable(hooks_dir / "pre-commit", f"""#!/bin/sh
git diff --cached --name-only >> {shlex.quote(str(self.hook_log))}
exit "$(cat {shlex.quote(str(self.hook_rc))})"
""")
        return hooks_dir

    def hermetic_env(self):
        env = {k: v for k, v in os.environ.items() if not _is_ambient_git_state(k)}
        env["GIT_CONFIG_GLOBAL"] = str(self.empty_global_config)
        env["GIT_CONFIG_NOSYSTEM"] = "1"
        return env

    # ── git helpers (all through the hermetic env) ───────────────────────
    def _run_git(self, argv):
        completed = subprocess.run(argv, env=self.hermetic_env(), cwd=self.tmpdir,
                                   capture_output=True, text=True)
        if completed.returncode != 0:
            raise AssertionError(f"fixture git failed: {argv}{_diag(completed)}")
        return completed.stdout.strip()

    def git(self, *args):
        return self._run_git(["git", "-C", str(self.main), *args])

    def git_bare(self, *args):
        return self._run_git(["git", *args])

    def fixture_commit(self, message):
        """A setup commit by the fixture's human, kept out of the hook's record."""
        self.git("commit", "-q", "--no-verify", "-m", message)

    def head(self):
        return self.git("rev-parse", "HEAD")

    def origin_main(self):
        return self._run_git(["git", "--git-dir", str(self.origin),
                              "rev-parse", "refs/heads/main"])

    def files_in_commit(self, rev):
        return set(self.git("show", "--name-only", "--format=", rev).split())

    def stub_invoked_in(self):
        """The directory the stub claude ran in, or None if it never ran."""
        if not self.stub_marker.exists():
            return None
        return self.stub_marker.read_text().strip()

    # ── the one way to run the script ────────────────────────────────────
    def run_script(self, **env_overrides):
        refused = _PINNED_KEYS & env_overrides.keys()
        if refused:
            raise ValueError(f"run_script pins {sorted(refused)}; they cannot be overridden")
        env = self.hermetic_env()
        env.pop("REVIEW_README_STUB_APPEND", None)
        env.update(env_overrides)
        env["CLAUDE_BIN"] = str(self.stub)
        env["REIFY_MAIN_CHECKOUT"] = str(self.main)
        env["REVIEW_README_STUB_ROOT"] = str(self.main)
        return subprocess.run(["bash", str(SCRIPT)], env=env, cwd=self.neutral_cwd,
                              capture_output=True, text=True, timeout=120)


class CheckoutSeamTest(ReviewReadmeFixture):
    def test_operates_on_the_checkout_named_by_REIFY_MAIN_CHECKOUT(self):
        result = self.run_script()
        self.assertEqual(self.stub_invoked_in(), str(self.main), _diag(result))
        self.assertEqual(result.returncode, 0, _diag(result))

    def test_an_edit_lands_as_one_commit_touching_only_the_targets(self):
        before = self.head()
        result = self.run_script(REVIEW_README_STUB_APPEND="REVIEWED-LINE")
        self.assertEqual(result.returncode, 0, _diag(result))
        self.assertEqual(self.git("rev-list", "--count", f"{before}..HEAD"), "1", _diag(result))
        self.assertEqual(self.files_in_commit("HEAD"), {README}, _diag(result))
        self.assertIn("REVIEWED-LINE", self.git("show", f"HEAD:{README}"))

    def test_no_edit_means_no_commit(self):
        before = self.head()
        result = self.run_script()
        self.assertEqual(result.returncode, 0, _diag(result))
        self.assertEqual(self.head(), before, _diag(result))


if __name__ == "__main__":
    unittest.main()
