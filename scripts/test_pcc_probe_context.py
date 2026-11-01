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


if __name__ == "__main__":
    unittest.main()
