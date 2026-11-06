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
import tempfile
import unittest

from _pcc_test_support import (
    pcc,
    _REPO_ROOT,
    git,
    make_fixture_repo,
    make_git_fixture_repo,
    probe_context,
    trust_environ,
    write_bin_sidecar,
    write_profile_binary,
)


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
        debug = write_profile_binary(self.root, "target/debug/reify")
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
                path = write_profile_binary(tmp, relpath)
                ctx = pcc.ProbeContext.from_environment(environ={}, repo_root=tmp)
                self.assertEqual(ctx.reify_bin, path)
                self.assertTrue(os.path.isabs(ctx.reify_bin))

    def test_a_non_executable_profile_binary_is_not_picked(self):
        write_profile_binary(self.root, "target/release/reify", mode=0o644)
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



class TestReifyBinaryTrust(unittest.TestCase):
    """R14 M6: an auto-discovered binary is used only on a SHA identity; mtime never decides.

    target/.reify-bin-sha is written after `cargo build --release -p reify-cli`,
    and vouches only while it names HEAD.  scripts/reify-bin-freshness.sh is the
    one home of that rule, so every case here drives the REAL script through
    from_environment().  The root is a git repository, so the home's documented
    fail-open for a non-git root never answers a trust case by accident.
    """

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="pcc_trust_")
        self.root = self._tmp.name
        self.head = make_git_fixture_repo(self.root)

    def tearDown(self):
        self._tmp.cleanup()

    def _ctx(self, **extra):
        return pcc.ProbeContext.from_environment(environ=trust_environ(**extra),
                                                 repo_root=self.root)

    def _both_binaries(self, newer):
        release = write_profile_binary(self.root, "target/release/reify")
        debug = write_profile_binary(self.root, "target/debug/reify")
        old, new = 1_600_000_000, 1_700_000_000
        os.utime(release, (new, new) if newer == "release" else (old, old))
        os.utime(debug, (new, new) if newer == "debug" else (old, old))
        return release, debug

    def _sidecar(self, text):
        write_bin_sidecar(self.root, text)

    def _head(self):
        return self.head

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
            git(other, "init", "-q")
            git(other, "commit", "-q", "--allow-empty", "-m", "another repository")
            for label, git_dir in (("another repository", os.path.join(other, ".git")),
                                   ("a nonexistent path", os.path.join(other, "no-such-git"))):
                with self.subTest(git_dir=label):
                    ctx = self._ctx(GIT_DIR=git_dir)
                    self.assertEqual(ctx.reify_bin, release, ctx.reify_bin_refusal)

    def test_a_stale_sidecar_is_refused_naming_both_shas(self):
        self._both_binaries(newer="release")
        head = self._head()
        stale = "0" * 40
        self._sidecar(stale + "\n")
        ctx = self._ctx()
        self.assertIsNone(ctx.reify_bin)
        self.assertIn(stale, ctx.reify_bin_refusal)
        self.assertIn(head, ctx.reify_bin_refusal)

    def test_outside_a_git_checkout_the_trust_home_fails_open(self):
        """The home's documented fail-open: with no HEAD there is no tree to compare."""
        with tempfile.TemporaryDirectory(prefix="pcc_trust_nongit_") as root:
            release = write_profile_binary(root, "target/release/reify")
            write_profile_binary(root, "target/debug/reify")
            write_bin_sidecar(root, "0" * 40 + "\n")
            ctx = pcc.ProbeContext.from_environment(environ=trust_environ(), repo_root=root)
            self.assertEqual(ctx.reify_bin, release, ctx.reify_bin_refusal)
            self.assertIsNone(ctx.reify_bin_refusal)

    def test_an_unconsultable_trust_home_refuses(self):
        """No `bash` can launch on this PATH, so the trust answer is unprovable: refuse."""
        write_profile_binary(self.root, "target/release/reify")
        self._sidecar(self.head + "\n")
        ctx = self._ctx(PATH="/nonexistent-9870")
        self.assertIsNone(ctx.reify_bin)
        self.assertIn("reify-bin-freshness.sh", ctx.reify_bin_refusal)
        self.assertIn("REIFY_BIN", ctx.reify_bin_refusal)

    def test_reify_bin_wins_over_two_unvouched_binaries(self):
        self._both_binaries(newer="debug")
        ctx = self._ctx(REIFY_BIN="/x/reify")
        self.assertEqual(ctx.reify_bin, "/x/reify")
        self.assertIsNone(ctx.reify_bin_refusal)

    _PROFILE_BINARIES = ("target/release/reify", "target/debug/reify")

    def _lone_binary_ctx(self, relpath, sidecar_for_head):
        """(binary path, ctx) over a fresh git repo holding only `relpath`.

        `sidecar_for_head(head)` is the sidecar text to write, or None for none.
        """
        with tempfile.TemporaryDirectory(prefix="pcc_trust_lone_") as root:
            head = make_git_fixture_repo(root)
            path = write_profile_binary(root, relpath)
            sidecar = sidecar_for_head(head)
            if sidecar is not None:
                write_bin_sidecar(root, sidecar)
            return path, pcc.ProbeContext.from_environment(environ=trust_environ(),
                                                           repo_root=root)

    def test_a_lone_binary_the_sidecar_does_not_vouch_for_is_refused(self):
        for relpath in self._PROFILE_BINARIES:
            for sidecar in (None, "0" * 40):
                with self.subTest(binary=relpath, sidecar=sidecar):
                    path, ctx = self._lone_binary_ctx(relpath, lambda _head: sidecar)
                    self.assertIsNone(ctx.reify_bin)
                    self.assertIn(path, ctx.reify_bin_refusal)
                    self.assertIn("REIFY_BIN", ctx.reify_bin_refusal)

    def test_a_lone_binary_the_sidecar_vouches_for_is_used(self):
        for relpath in self._PROFILE_BINARIES:
            with self.subTest(binary=relpath):
                path, ctx = self._lone_binary_ctx(relpath, lambda head: head + "\n")
                self.assertEqual(ctx.reify_bin, path, ctx.reify_bin_refusal)
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
