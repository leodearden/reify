#!/usr/bin/env python3
"""Namespace ratchet for reify-compiler glob re-exports and glob imports.

The discovered gate member is the sibling test_compiler_namespace_ratchet.sh.
Contract: PRD docs/prds/v0_6/compiler-module-namespace.md §4 C7.
"""

from __future__ import annotations

import re
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
INFRA_DIR = Path(__file__).resolve().parent

sys.path.insert(0, str(INFRA_DIR))

import source_count_ratchet as scr  # noqa: E402

COMPILER_SRC = "crates/reify-compiler/src"
BASELINE = INFRA_DIR / "compiler-namespace-ratchet-baseline.txt"
COUNTERS = (
    scr.Counter(
        "glob_reexports",
        re.compile(r"^pub(\([^)]*\))?\s+use\s[^;]*\*"),
        allowed=frozenset({(f"{COMPILER_SRC}/lib.rs", "pub use types::*;")}),
    ),
    scr.Counter("glob_imports", re.compile(r"^use\s[^;]*\*")),
)


def namespace_ratchet(root: Path, baseline: Path) -> scr.Report:
    return scr.check(root, (root / COMPILER_SRC,), COUNTERS, baseline)


class ScratchTree(unittest.TestCase):
    """A per-test temp dir standing in for the repo root."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, rel, text):
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def src(self, rel, text):
        return self.write(f"{COMPILER_SRC}/{rel}", text)

    def baseline(self, text):
        return self.write("tests/infra/compiler-namespace-ratchet-baseline.txt", text)

    def census(self):
        return scr.census(self.root, (self.root / COMPILER_SRC,), COUNTERS)

    def hits(self, name):
        return [str(site) for site in self.census().hits[name]]


class CountingRules(ScratchTree):

    def test_column0_glob_imports_count_at_their_lines(self):
        self.src("a.rs", "fn f() {}\nuse super::*;\nuse crate::*;\nuse checker::*;\n")
        self.assertEqual(self.hits("glob_imports"), [
            f"{COMPILER_SRC}/a.rs:2",
            f"{COMPILER_SRC}/a.rs:3",
            f"{COMPILER_SRC}/a.rs:4",
        ])
        self.assertEqual(self.hits("glob_reexports"), [])

    def test_indented_and_commented_globs_do_not_count(self):
        self.src("a.rs", "fn f() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n}\n"
                         "// use super::*;\n")
        self.assertEqual(self.hits("glob_imports"), [])

    def test_glob_anywhere_before_the_first_semicolon_counts(self):
        self.src("a.rs", "use super::{*, x};\nuse std::ops::Mul; // a * b\n")
        self.assertEqual(self.hits("glob_imports"), [f"{COMPILER_SRC}/a.rs:1"])

    def test_visibility_qualified_globs_are_reexports_not_imports(self):
        self.src("a.rs", "pub(crate) use a::*;\npub(super) use a::*;\n"
                         "pub(in crate::m) use a::*;\npub use a::*;\n")
        self.assertEqual(self.hits("glob_reexports"),
                         [f"{COMPILER_SRC}/a.rs:{n}" for n in (1, 2, 3, 4)])
        self.assertEqual(self.hits("glob_imports"), [])

    def test_allowlist_is_exactly_lib_rs_pub_use_types(self):
        self.src("lib.rs", "pub use types::*;\npub(crate) use types::*;\npub use other::*;\n")
        self.src("a.rs", "pub use types::*;\n")
        self.assertEqual(self.hits("glob_reexports"), [
            f"{COMPILER_SRC}/a.rs:1",
            f"{COMPILER_SRC}/lib.rs:2",
            f"{COMPILER_SRC}/lib.rs:3",
        ])

    def test_files_outside_the_compiler_source_root_are_not_scanned(self):
        self.write("crates/other/src/lib.rs", "use super::*;\npub use a::*;\n")
        self.src("lib.rs", "mod a;\n")
        self.assertEqual(self.hits("glob_imports"), [])
        self.assertEqual(self.hits("glob_reexports"), [])


if __name__ == "__main__":
    unittest.main()
