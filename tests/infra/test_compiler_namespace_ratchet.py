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

    BASELINE_REL = "tests/infra/compiler-namespace-ratchet-baseline.txt"

    def baseline(self, text):
        return self.write(self.BASELINE_REL, text)

    def append(self, rel, text):
        path = self.root / COMPILER_SRC / rel
        path.write_text(path.read_text(encoding="utf-8") + text, encoding="utf-8")

    def report(self):
        return namespace_ratchet(self.root, self.root / self.BASELINE_REL)

    def census(self):
        return scr.census(self.root, (self.root / COMPILER_SRC,), COUNTERS)

    def hits(self, name):
        return [str(site) for site in self.census().hits[name]]

    def skipped(self):
        return self.census().skipped


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


def child(rel, declared_in, line):
    return scr.CfgTestChild(f"{COMPILER_SRC}/{rel}", scr.Site(f"{COMPILER_SRC}/{declared_in}", line))


class CfgTestChildren(ScratchTree):
    """Only files declared under #[cfg(test)] are skipped; each names its declaration."""

    def assert_counted(self, *rels):
        self.assertEqual(self.skipped(), ())
        self.assertEqual(self.hits("glob_imports"), [f"{COMPILER_SRC}/{rel}:1" for rel in rels])

    def test_child_declared_under_cfg_test_is_skipped_naming_its_declaration(self):
        self.src("a.rs", "fn f() {}\n\n#[cfg(test)]\nmod tests;\n")
        self.src("a/tests.rs", "use super::*;\n")
        self.assertEqual(self.skipped(), (child("a/tests.rs", "a.rs", 4),))
        self.assertEqual(self.hits("glob_imports"), [])

    def test_child_declared_without_the_attribute_is_counted(self):
        self.src("a.rs", "mod tests;\n")
        self.src("a/tests.rs", "use super::*;\n")
        self.assert_counted("a/tests.rs")

    def test_undeclared_tests_file_is_counted(self):
        self.src("a.rs", "fn f() {}\n")
        self.src("a/tests.rs", "use super::*;\n")
        self.assert_counted("a/tests.rs")

    def test_recognised_attribute_blocks_are_skipped(self):
        blocks = {
            "extra attribute below": "#[cfg(test)]\n#[allow(clippy::all)]\nmod tests;\n",
            "extra attribute above": "#[allow(clippy::all)]\n#[cfg(test)]\nmod tests;\n",
            "inner whitespace": "#[cfg( test )]\nmod tests;\n",
            "visibility-qualified mod": "#[cfg(test)]\npub(crate) mod tests;\n",
        }
        for label, block in blocks.items():
            with self.subTest(label):
                self.src("a.rs", block)
                self.src("a/tests.rs", "use super::*;\n")
                self.assertEqual(self.skipped(),
                                 (child("a/tests.rs", "a.rs", block.count("\n")),))
                self.assertEqual(self.hits("glob_imports"), [])

    def test_attribute_separated_from_the_declaration_is_counted(self):
        self.src("a.rs", "#[cfg(test)]\nuse x::y;\nmod tests;\n")
        self.src("a/tests.rs", "use super::*;\n")
        self.assert_counted("a/tests.rs")

    def test_sibling_child_declared_in_lib_rs_is_skipped(self):
        self.src("lib.rs", "#[cfg(test)]\nmod engine_tests;\n")
        self.src("engine_tests.rs", "use super::*;\n")
        self.assertEqual(self.skipped(), (child("engine_tests.rs", "lib.rs", 2),))
        self.assertEqual(self.hits("glob_imports"), [])

    def test_child_of_a_mod_rs_parent_is_skipped(self):
        self.src("conformance/mod.rs", "#[cfg(test)]\nmod tests;\n")
        self.src("conformance/tests.rs", "use super::*;\n")
        self.assertEqual(self.skipped(), (child("conformance/tests.rs", "conformance/mod.rs", 2),))
        self.assertEqual(self.hits("glob_imports"), [])

    def test_directory_module_child_and_its_descendants_are_skipped(self):
        self.src("a.rs", "#[cfg(test)]\nmod tests;\n")
        self.src("a/tests/mod.rs", "use super::*;\nmod helpers;\n")
        self.src("a/tests/helpers.rs", "use super::*;\n")
        self.assertEqual(self.skipped(), (
            child("a/tests/helpers.rs", "a.rs", 2),
            child("a/tests/mod.rs", "a.rs", 2),
        ))
        self.assertEqual(self.hits("glob_imports"), [])

    def test_nested_cfg_test_child_is_skipped_once_naming_the_outermost(self):
        self.src("a.rs", "#[cfg(test)]\nmod tests;\n")
        self.src("a/tests/mod.rs", "#[cfg(test)]\nmod helpers;\n")
        self.src("a/tests/helpers.rs", "use super::*;\n")
        self.assertEqual(self.skipped(), (
            child("a/tests/helpers.rs", "a.rs", 2),
            child("a/tests/mod.rs", "a.rs", 2),
        ))

    def test_indented_declaration_is_not_recognised(self):
        self.src("a.rs", "mod inner {\n    #[cfg(test)]\n    mod tests;\n}\n")
        self.src("a/inner/tests.rs", "use super::*;\n")
        self.assert_counted("a/inner/tests.rs")

    def test_unrecognised_declaration_shapes_fall_to_production(self):
        shapes = {
            "same-line attribute": "#[cfg(test)] mod tests;\n",
            "comment between": "#[cfg(test)]\n// the tests\nmod tests;\n",
            "compound cfg": "#[cfg(all(test, feature = \"x\"))]\nmod tests;\n",
            "path attribute": "#[cfg(test)]\n#[path = \"a/tests.rs\"]\nmod tests;\n",
        }
        for label, block in shapes.items():
            with self.subTest(label):
                self.src("a.rs", block)
                self.src("a/tests.rs", "use super::*;\n")
                self.assert_counted("a/tests.rs")


SEED_BASELINE = "# header comment\nglob_reexports 1\nglob_imports 1\n"


class Verdicts(ScratchTree):
    """The baseline is exact: growth and a stale row are both red."""

    def seed(self):
        self.src("lib.rs", "mod a;\nmod b;\npub use types::*;\npub(crate) use a::*;\n")
        self.src("a.rs", "use super::*;\n")
        self.src("b.rs", "use crate::x;\n")
        self.baseline(SEED_BASELINE)

    def single_violation(self):
        violations = self.report().violations
        self.assertEqual(len(violations), 1, violations)
        return violations[0]

    def assert_green(self):
        report = self.report()
        self.assertEqual(report.violations, (), "\n".join(report.violations))

    def test_seed_is_green_and_reports_counts(self):
        self.seed()
        report = self.report()
        self.assertEqual(report.violations, ())
        self.assertEqual(report.count_lines, ("glob_reexports 1", "glob_imports 1"))

    def test_added_glob_import_is_red_naming_every_counted_line(self):
        self.seed()
        self.append("b.rs", "use super::*;\n")
        violation = self.single_violation()
        for fragment in ("glob_imports", "2", "'glob_imports 1'",
                         f"{COMPILER_SRC}/a.rs:1", f"{COMPILER_SRC}/b.rs:2"):
            self.assertIn(fragment, violation)

    def test_added_glob_reexport_is_red(self):
        self.seed()
        self.append("lib.rs", "pub(crate) use b::*;\n")
        violation = self.single_violation()
        self.assertIn("glob_reexports", violation)
        self.assertIn(f"{COMPILER_SRC}/lib.rs:5", violation)

    def test_allowlisted_line_outside_lib_rs_is_red(self):
        self.seed()
        self.append("a.rs", "pub use types::*;\n")
        violation = self.single_violation()
        self.assertIn("glob_reexports", violation)
        self.assertIn(f"{COMPILER_SRC}/a.rs:2", violation)

    def test_removed_glob_is_red_until_baseline_lowered_then_green(self):
        self.seed()
        self.assert_green()
        self.append("b.rs", "use super::*;\n")
        self.assertIn("glob_imports", self.single_violation())
        self.src("b.rs", "use crate::x;\n")
        self.assert_green()
        self.src("a.rs", "fn f() {}\n")
        violation = self.single_violation()
        for fragment in ("'glob_imports 1'", f"{self.BASELINE_REL}:3", "'glob_imports 0'"):
            self.assertIn(fragment, violation)
        self.baseline("# header comment\nglob_reexports 1\nglob_imports 0\n")
        self.assert_green()

    def test_report_names_skipped_children(self):
        self.seed()
        self.append("a.rs", "#[cfg(test)]\nmod tests;\n")
        self.src("a/tests.rs", "use super::*;\n")
        report = self.report()
        self.assertEqual(report.violations, ())
        self.assertEqual(report.skipped, (child("a/tests.rs", "a.rs", 3),))

    def test_baseline_input_errors_name_the_input(self):
        path = self.root / self.BASELINE_REL
        cases = {
            "malformed count": ("# h\nglob_reexports 1\nglob_imports twenty\n", f"{path}:3"),
            "extra field": ("# h\nglob_reexports 1\nglob_imports 1 extra\n", f"{path}:3"),
            "negative count": ("# h\nglob_reexports 1\nglob_imports -1\n", f"{path}:3"),
            "leading zero": ("# h\nglob_reexports 1\nglob_imports 01\n", f"{path}:3"),
            "duplicate row": ("# h\nglob_reexports 1\nglob_imports 1\nglob_imports 1\n",
                              f"{path}:4"),
            "unknown counter": ("glob_reexports 1\nglob_imports 1\nglob_other 1\n", f"{path}:3"),
            "missing counter row": ("# h\nglob_reexports 1\n", "glob_imports"),
        }
        self.seed()
        for label, (text, fragment) in cases.items():
            with self.subTest(label):
                self.baseline(text)
                with self.assertRaises(scr.RatchetInputError) as caught:
                    self.report()
                self.assertIn(str(path), str(caught.exception))
                self.assertIn(fragment, str(caught.exception))
        with self.subTest("missing baseline file"):
            path.unlink()
            with self.assertRaises(scr.RatchetInputError) as caught:
                self.report()
            self.assertIn(str(path), str(caught.exception))

    def test_missing_source_root_names_it(self):
        self.baseline(SEED_BASELINE)
        with self.assertRaises(scr.RatchetInputError) as caught:
            self.report()
        self.assertIn(str(self.root / COMPILER_SRC), str(caught.exception))


if __name__ == "__main__":
    unittest.main()
