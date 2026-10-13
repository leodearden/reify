#!/usr/bin/env python3
"""Construct ratchets for the compiler declaration passes.

PRD docs/prds/v0_6/compiler-declaration-passes.md §4 D12; engine in
tests/infra/rust_construct_ratchet.py. The discovered member is the wrapper
tests/infra/test_compiler_decl_pass_ratchets.sh. Run this file directly to
print each row's measured count.
"""

import re
import sys
import tempfile
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent
REPO_ROOT = INFRA_DIR.parents[1]

sys.path.insert(0, str(INFRA_DIR))

import rust_construct_ratchet as rcr  # noqa: E402

TEST_MODULE = "#[cfg(test)]\nmod tests {\n    fn t() {\n%s    }\n}\n"


def rust_fn(*body_lines):
    return "fn fixture() {\n" + "".join(line + "\n" for line in body_lines) + "}\n"


def cfg_test_module(*body_lines):
    return TEST_MODULE % "".join(f"        {line}\n" for line in body_lines)


def demo_row(construct, *paths, view=rcr.View.CODE, baseline=10, target=0,
             row_id="demo", lowered_by="owner-x"):
    return rcr.Row(row_id=row_id, construct=re.compile(construct), view=view,
                   paths=paths, baseline=baseline, target=target,
                   lowered_by=lowered_by)


class TempRoot(unittest.TestCase):
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

    def evaluate_one(self, row):
        (result,) = rcr.evaluate((row,), self.root)
        return result


class Views(TempRoot):

    def test_code_view_counts_code_occurrences_only(self):
        self.write("src/a.rs", rust_fn(
            "    needle(1);",
            "    // needle(2)",
            "    /// needle(3)",
            '    let s = "needle(4)";',
            "    /* needle(5) */",
            "    needle(6); needle(7);",
        ) + cfg_test_module("needle(8);"))
        result = self.evaluate_one(demo_row(r"\bneedle\(", "src/a.rs"))
        self.assertEqual(result.count, 3)
        self.assertEqual(result.hits, ("src/a.rs:2", "src/a.rs:7", "src/a.rs:7"))

    def test_code_with_literals_view_sees_strings_but_not_comments(self):
        self.write("src/a.rs", rust_fn(
            '    let a = "needle_x";',
            '    let b = r#"needle_y"#;',
            '    foo(); // "needle_z"',
            '    /// "needle_w"',
            "    let c = needle_v;",
        ) + cfg_test_module('let d = "needle_t";'))
        result = self.evaluate_one(
            demo_row('"needle_', "src/a.rs", view=rcr.View.CODE_WITH_LITERALS))
        self.assertEqual(result.count, 2)
        self.assertEqual(result.hits, ("src/a.rs:2", "src/a.rs:3"))


class Scope(TempRoot):

    def test_glob_recurses_and_sums_across_files(self):
        self.write("crates/one/src/lib.rs", rust_fn("    needle();"))
        self.write("crates/two/src/deep/mod.rs", rust_fn("    needle();", "    needle();"))
        self.write("crates/one/tests/t.rs", rust_fn("    needle();"))
        result = self.evaluate_one(demo_row(r"\bneedle\(", "crates/*/src/**/*.rs"))
        self.assertEqual(result.count, 3)
        self.assertEqual(result.files,
                         ("crates/one/src/lib.rs", "crates/two/src/deep/mod.rs"))

    def test_glob_matching_no_file_is_a_violation(self):
        self.write("src/a.rs", rust_fn("    needle();"))
        result = self.evaluate_one(
            demo_row(r"\bneedle\(", "src/a.rs", "gone/*.rs", row_id="stale-row"))
        self.assertEqual(result.missing_paths, ("gone/*.rs",))
        self.assertIsNotNone(result.violation)
        self.assertIn("stale-row", result.violation)
        self.assertIn("gone/*.rs", result.violation)


class Baseline(TempRoot):

    def setUp(self):
        super().setUp()
        self.write("src/a.rs", rust_fn("    needle();", "    quiet();", "    needle();"))

    def result_at(self, baseline):
        return self.evaluate_one(
            demo_row(r"\bneedle\(", "src/a.rs", baseline=baseline, target=0,
                     row_id="over-row", lowered_by="owner-x"))

    def test_count_at_baseline_is_clean(self):
        result = self.result_at(2)
        self.assertEqual(result.count, 2)
        self.assertIsNone(result.violation)

    def test_count_below_baseline_is_clean(self):
        self.assertIsNone(self.result_at(5).violation)

    def test_count_above_baseline_names_row_owner_target_and_hits(self):
        violation = self.result_at(1).violation
        self.assertIsNotNone(violation)
        for fragment in ("over-row", "2", "baseline 1", "target 0", "owner-x",
                         "src/a.rs:2", "src/a.rs:4"):
            self.assertIn(fragment, violation)

    def test_summary_reports_count_over_baseline_and_owner(self):
        summary = self.evaluate_one(
            demo_row(r"\bneedle\(", "src/a.rs", baseline=3, target=0)).summary
        for fragment in ("demo", "2/3", "owner-x"):
            self.assertIn(fragment, summary)


class LexFailure(TempRoot):

    def test_unbalanced_file_raises_lex_error_naming_the_file(self):
        self.write("src/broken.rs", "fn open() {\n    needle(1);\n")
        with self.assertRaises(rcr.LexError) as caught:
            rcr.evaluate((demo_row(r"\bneedle\(", "src/broken.rs"),), self.root)
        self.assertIn("src/broken.rs", str(caught.exception))


class RowInvariants(unittest.TestCase):

    def assert_rejected(self, offending, **overrides):
        with self.assertRaises(ValueError) as caught:
            demo_row(r"\bneedle\(", *overrides.pop("paths", ("src/a.rs",)),
                     row_id="bad-row", **overrides)
        message = str(caught.exception)
        self.assertIn("bad-row", message)
        self.assertIn(offending, message)

    def test_target_above_baseline(self):
        self.assert_rejected("7", baseline=3, target=7)

    def test_negative_target(self):
        self.assert_rejected("-4", baseline=3, target=-4)

    def test_empty_paths(self):
        self.assert_rejected("()", paths=())

    def test_absolute_path(self):
        self.assert_rejected("/etc/a.rs", paths=("src/a.rs", "/etc/a.rs"))

    def test_dot_dot_path(self):
        self.assert_rejected("src/../../a.rs", paths=("src/../../a.rs",))

    def test_empty_lowered_by(self):
        self.assert_rejected("lowered_by", lowered_by="")


if __name__ == "__main__":
    unittest.main()
