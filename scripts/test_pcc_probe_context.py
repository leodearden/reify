#!/usr/bin/env python3
"""
test_pcc_probe_context.py — scripts/prd-capability-check.py's ProbeContext: the
one value that carries the repo root, the two binaries, the grammar-cache
override and the substrate bound into every probe-running function.

Every case reads the environment through the public seam
ProbeContext.from_environment(environ=<dict>, repo_root=<fixture dir>) and never
patches os.environ, so what is under test is the env-to-field mapping itself.
Shared loader and helpers: scripts/_pcc_test_support.py; runner:
tests/infra/test_prd_capability_check.sh.
"""

import dataclasses
import os
import subprocess
import tempfile
import unittest
import unittest.mock

from _pcc_test_support import (
    pcc,
    _REPO_ROOT,
    make_fixture_repo,
    probe_context,
)


def _write_binary(root: str, relpath: str, mode: int = 0o755) -> str:
    """Create a file at <root>/<relpath> with `mode` and return its absolute path."""
    path = os.path.join(root, relpath)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write("#!/bin/sh\nexit 0\n")
    os.chmod(path, mode)
    return path


class TestFromEnvironment(unittest.TestCase):
    """from_environment() maps an environment mapping onto ProbeContext fields."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="pcc_ctx_")
        self.root = make_fixture_repo(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def _ctx(self, environ):
        return pcc.ProbeContext.from_environment(environ=environ, repo_root=self.root)

    def test_empty_environment_takes_every_default(self):
        ctx = self._ctx({})
        self.assertEqual(ctx.repo_root, self.root)
        self.assertEqual(ctx.tree_sitter_bin, "tree-sitter")
        self.assertEqual(ctx.reify_bin, "reify")
        self.assertIsNone(ctx.cache_home_override)
        self.assertGreater(ctx.substrate_timeout_s, 0)

    def test_tree_sitter_bin_is_read_and_empty_counts_as_unset(self):
        self.assertEqual(self._ctx({"TREE_SITTER_BIN": "/x/ts"}).tree_sitter_bin, "/x/ts")
        self.assertEqual(self._ctx({"TREE_SITTER_BIN": ""}).tree_sitter_bin, "tree-sitter")

    def test_reify_bin_is_taken_verbatim(self):
        self.assertEqual(self._ctx({"REIFY_BIN": "/x/reify"}).reify_bin, "/x/reify")

    def test_empty_reify_bin_falls_through_to_path(self):
        self.assertEqual(self._ctx({"REIFY_BIN": ""}).reify_bin, "reify")

    def test_empty_reify_bin_falls_through_to_the_fixture_binary(self):
        debug = _write_binary(self.root, "target/debug/reify")
        self.assertEqual(self._ctx({"REIFY_BIN": ""}).reify_bin, debug)

    def test_cache_home_override_is_read_and_empty_counts_as_unset(self):
        # The operator-facing name, written as a literal: it is the documented
        # contract in .claude/skills/prd/references/grammar-gate.md.
        self.assertEqual(
            self._ctx({"REIFY_TS_CACHE_HOME": "/x/cache"}).cache_home_override, "/x/cache"
        )
        self.assertIsNone(self._ctx({"REIFY_TS_CACHE_HOME": ""}).cache_home_override)

    def test_a_lone_executable_profile_binary_is_picked(self):
        for relpath in ("target/debug/reify", "target/release/reify"):
            with self.subTest(binary=relpath), tempfile.TemporaryDirectory() as tmp:
                path = _write_binary(tmp, relpath)
                ctx = pcc.ProbeContext.from_environment(environ={}, repo_root=tmp)
                self.assertEqual(ctx.reify_bin, path)
                self.assertTrue(os.path.isabs(ctx.reify_bin))

    def test_a_non_executable_profile_binary_is_not_picked(self):
        _write_binary(self.root, "target/release/reify", mode=0o644)
        self.assertEqual(self._ctx({}).reify_bin, "reify")

    def test_repo_root_defaults_to_this_checkout(self):
        ctx = pcc.ProbeContext.from_environment(environ={})
        self.assertEqual(ctx.repo_root, _REPO_ROOT)


class TestProbeContextInvariants(unittest.TestCase):
    """A ProbeContext that could mislead a probe is refused at construction."""

    def test_each_bad_field_is_refused_naming_the_field_and_value(self):
        cases = [
            ("repo_root", "relative/root"),
            ("tree_sitter_bin", ""),
            ("reify_bin", ""),
            ("cache_home_override", ""),
            ("substrate_timeout_s", 0),
            ("substrate_timeout_s", -1.0),
            ("substrate_timeout_s", float("nan")),
        ]
        for field_name, value in cases:
            with self.subTest(field=field_name, value=value):
                with self.assertRaises(ValueError) as cm:
                    probe_context(**{field_name: value})
                message = str(cm.exception)
                self.assertIn(field_name, message)
                self.assertIn(repr(value), message)

    def test_a_built_context_is_frozen(self):
        ctx = probe_context()
        with self.assertRaises(dataclasses.FrozenInstanceError):
            ctx.reify_bin = "/elsewhere/reify"



def _without_git_vars(environ):
    """`environ` minus every GIT_* key.

    Under a hook (pre-commit / pre-merge-commit) GIT_DIR is exported and would
    aim a test's `git init` / `git commit` at the real repository.
    """
    return {k: v for k, v in environ.items() if not k.startswith("GIT_")}


def _git(root, *args):
    """Run git in `root` with no GIT_* variable and no config write; return stdout."""
    return subprocess.run(
        ["git", "-C", root, "-c", "user.name=t", "-c", "user.email=t@t",
         "-c", "commit.gpgsign=false", "-c", "core.hooksPath=/dev/null", *args],
        env=_without_git_vars(os.environ), capture_output=True, text=True, check=True,
    ).stdout.strip()


class TestReifyBinaryTrust(unittest.TestCase):
    """R14 M6: with both profiles built, only a SHA identity picks one; mtime never does.

    target/.reify-bin-sha is written after `cargo build --release -p reify-cli`,
    so it vouches for target/release/reify alone, and only while it names HEAD.
    """

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="pcc_trust_")
        self.root = self._tmp.name

    def tearDown(self):
        self._tmp.cleanup()

    def _environ(self, **extra):
        environ = _without_git_vars(os.environ)
        environ.pop("REIFY_BIN", None)
        return dict(environ, **extra)

    def _ctx(self, **extra):
        return pcc.ProbeContext.from_environment(environ=self._environ(**extra),
                                                 repo_root=self.root)

    def _both_binaries(self, newer):
        release = _write_binary(self.root, "target/release/reify")
        debug = _write_binary(self.root, "target/debug/reify")
        old, new = 1_600_000_000, 1_700_000_000
        os.utime(release, (new, new) if newer == "release" else (old, old))
        os.utime(debug, (new, new) if newer == "debug" else (old, old))
        return release, debug

    def _sidecar(self, text):
        with open(os.path.join(self.root, "target", ".reify-bin-sha"), "w") as fh:
            fh.write(text)

    def _head(self):
        _git(self.root, "init", "-q")
        _git(self.root, "commit", "-q", "--allow-empty", "-m", "x")
        return _git(self.root, "rev-parse", "HEAD")

    def test_two_unvouched_binaries_are_refused_whichever_is_newer(self):
        for newer in ("debug", "release"):
            with self.subTest(newer=newer):
                release, debug = self._both_binaries(newer)
                ctx = self._ctx()
                self.assertIsNone(ctx.reify_bin)
                self.assertIn(release, ctx.reify_bin_refusal)
                self.assertIn(debug, ctx.reify_bin_refusal)
                self.assertIn("REIFY_BIN", ctx.reify_bin_refusal)

    def test_a_sidecar_naming_head_vouches_for_the_release_binary(self):
        release, _ = self._both_binaries(newer="debug")
        self._sidecar(self._head() + "\n")
        ctx = self._ctx()
        self.assertEqual(ctx.reify_bin, release)
        self.assertIsNone(ctx.reify_bin_refusal)

    def test_an_ambient_git_dir_does_not_decide_whose_head_the_sidecar_names(self):
        """Under a hook GIT_DIR is exported; HEAD must still be the fixture repo's.

        Without the scrub, a GIT_DIR aimed at another repository answers with
        that repository's HEAD, and a nonexistent one with no HEAD at all.
        """
        release, _ = self._both_binaries(newer="debug")
        self._sidecar(self._head() + "\n")
        with tempfile.TemporaryDirectory(prefix="pcc_trust_other_") as other:
            _git(other, "init", "-q")
            _git(other, "commit", "-q", "--allow-empty", "-m", "another repository")
            for label, git_dir in (("another repository", os.path.join(other, ".git")),
                                   ("a nonexistent path", os.path.join(other, "no-such-git"))):
                with self.subTest(git_dir=label):
                    ctx = self._ctx(GIT_DIR=git_dir)
                    self.assertEqual(ctx.reify_bin, release, ctx.reify_bin_refusal)

    def test_a_binary_removed_mid_resolution_is_still_refused(self):
        """A concurrent relink removes target/debug/reify after the presence check.

        Its mtime is diagnostic text, so losing it leaves the refusal standing
        rather than escaping from_environment(), which every mode calls first.
        """
        _, debug = self._both_binaries(newer="debug")
        real_access = os.access

        def access_then_relink_away(path, mode, *args, **kwargs):
            answer = real_access(path, mode, *args, **kwargs)
            if path == debug and os.path.exists(debug):
                os.remove(debug)
            return answer

        with unittest.mock.patch.object(os, "access", side_effect=access_then_relink_away):
            ctx = self._ctx()
        self.assertFalse(os.path.exists(debug), "precondition: the relink happened")
        self.assertIsNone(ctx.reify_bin)
        self.assertIn(f"{debug} (mtime unknown)", ctx.reify_bin_refusal)

    def test_a_stale_sidecar_is_refused_naming_both_shas(self):
        self._both_binaries(newer="release")
        head = self._head()
        stale = "0" * 40
        self._sidecar(stale + "\n")
        ctx = self._ctx()
        self.assertIsNone(ctx.reify_bin)
        self.assertIn(stale, ctx.reify_bin_refusal)
        self.assertIn(head, ctx.reify_bin_refusal)

    def test_a_sidecar_outside_a_git_repo_vouches_for_nothing(self):
        self._both_binaries(newer="release")
        self._sidecar("0" * 40 + "\n")
        ctx = self._ctx()
        self.assertIsNone(ctx.reify_bin)
        self.assertIn("REIFY_BIN", ctx.reify_bin_refusal)

    def test_reify_bin_wins_over_two_unvouched_binaries(self):
        self._both_binaries(newer="debug")
        ctx = self._ctx(REIFY_BIN="/x/reify")
        self.assertEqual(ctx.reify_bin, "/x/reify")
        self.assertIsNone(ctx.reify_bin_refusal)

    def test_a_lone_binary_is_used_whatever_the_sidecar_says(self):
        for relpath in ("target/release/reify", "target/debug/reify"):
            for sidecar in (None, "0" * 40):
                with self.subTest(binary=relpath, sidecar=sidecar), \
                     tempfile.TemporaryDirectory() as root:
                    path = _write_binary(root, relpath)
                    if sidecar is not None:
                        with open(os.path.join(root, "target", ".reify-bin-sha"), "w") as fh:
                            fh.write(sidecar)
                    ctx = pcc.ProbeContext.from_environment(environ=self._environ(),
                                                            repo_root=root)
                    self.assertEqual(ctx.reify_bin, path)
                    self.assertIsNone(ctx.reify_bin_refusal)

    def test_no_binary_falls_back_to_path(self):
        self.assertEqual(self._ctx().reify_bin, "reify")

    def test_require_reify_bin_returns_the_path_or_raises_the_refusal(self):
        self.assertEqual(probe_context(reify_bin="/x/reify").require_reify_bin(), "/x/reify")
        refusing = probe_context(reify_bin=None, reify_bin_refusal="two binaries, no REIFY_BIN")
        self.assertTrue(issubclass(pcc.ReifyBinaryRefused, ValueError))
        with self.assertRaises(pcc.ReifyBinaryRefused) as cm:
            refusing.require_reify_bin()
        self.assertIn("two binaries, no REIFY_BIN", str(cm.exception))

    def test_exactly_one_of_binary_and_refusal_is_set(self):
        for fields in ({"reify_bin": "/x/reify", "reify_bin_refusal": "no"},
                       {"reify_bin": None, "reify_bin_refusal": None}):
            with self.subTest(**fields):
                with self.assertRaises(ValueError) as cm:
                    probe_context(**fields)
                self.assertIn("reify_bin", str(cm.exception))
                self.assertIn("reify_bin_refusal", str(cm.exception))


if __name__ == "__main__":
    unittest.main()
