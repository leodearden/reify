#!/usr/bin/env python3
"""Contract tests for the construct-ratchet engine
tests/infra/rust_construct_ratchet.py, over hermetic temp-root fixtures.

The discovered member is the wrapper tests/infra/test_rust_construct_ratchet.sh.
Suites that pin real rows (e.g. tests/infra/test_compiler_decl_pass_ratchets.py)
build on this engine; its contract lives here so it outlives any one of them.
"""

import re
import sys
import unittest
from pathlib import Path

INFRA_DIR = Path(__file__).resolve().parent

sys.path.insert(0, str(INFRA_DIR))

import rust_construct_ratchet as rcr  # noqa: E402
from rust_construct_ratchet_fixtures import TempRoot, cfg_test_module, rust_fn  # noqa: E402


def demo_row(construct, *paths, view=rcr.View.CODE, baseline=10, target=0,
             row_id="demo", lowered_by="owner-x"):
    return rcr.Row(row_id=row_id, construct=re.compile(construct), view=view,
                   paths=paths, baseline=baseline, target=target,
                   lowered_by=lowered_by)


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

    def test_code_view_counts_text_only_the_lexed_line_spells(self):
        self.write("src/a.rs", rust_fn('    let s = "abc";', "    foo/* c */bar();"))
        blanked = self.evaluate_one(demo_row(r'= "";', "src/a.rs", baseline=1))
        spliced = self.evaluate_one(demo_row(r"\bfoobar\(", "src/a.rs", baseline=1))
        self.assertEqual(blanked.hits, ("src/a.rs:2",))
        self.assertEqual(spliced.hits, ("src/a.rs:3",))

    def test_starts_in_code_view_anchors_at_each_raw_line_start(self):
        self.write("src/a.rs", rust_fn('    "needle_a".len();'))
        result = self.evaluate_one(
            demo_row(r'^\s*"needle_', "src/a.rs", view=rcr.View.STARTS_IN_CODE))
        self.assertEqual(result.hits, ("src/a.rs:2",))

    def test_starts_in_code_view_counts_literal_openers_only(self):
        self.write("src/a.rs", rust_fn(
            '    let a = "needle_x";',
            '    let b = r#"needle_y"#;',
            '    foo(); // "needle_z"',
            '    /// "needle_w"',
            "    let c = needle_v;",
            '    foo(); /* "needle_u" */',
            "    /* opened",
            '    "needle_s" */ bar();',
            '    let r = r#"first',
            '    "needle_r" second"#;',
            r'    let e = "\"needle_q\"";',
            '    /* x */ let f = "needle_p";',
            '    let g = "multi',
            '    line"; let h = "needle_o";',
        ) + cfg_test_module('let d = "needle_t";'))
        result = self.evaluate_one(
            demo_row('"needle_', "src/a.rs", view=rcr.View.STARTS_IN_CODE))
        self.assertEqual(result.hits,
                         ("src/a.rs:2", "src/a.rs:3", "src/a.rs:13", "src/a.rs:15"))

    def test_starts_in_code_view_counts_char_literal_openers_only(self):
        self.write("src/a.rs", rust_fn(
            "    let c = 'x';",
            "    let q = '\"';",
            "    let b = b'y';",
            "    let s: &'static str = \"\";",
        ))
        expected_hits = {
            "x'": (), "\"'": (), "y'": (), "';": (),
            "'x'": ("src/a.rs:2",), "'static": ("src/a.rs:5",),
        }
        for construct, hits in expected_hits.items():
            with self.subTest(construct):
                result = self.evaluate_one(
                    demo_row(construct, "src/a.rs", view=rcr.View.STARTS_IN_CODE))
                self.assertEqual(result.hits, hits)

    def test_starts_in_code_view_counts_each_opener_on_a_line(self):
        self.write("src/a.rs", rust_fn('    f("needle_a", "needle_b"); // "needle_c"'))
        result = self.evaluate_one(
            demo_row('"needle_', "src/a.rs", view=rcr.View.STARTS_IN_CODE))
        self.assertEqual(result.hits, ("src/a.rs:2", "src/a.rs:2"))


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
                     row_id="ratchet-row", lowered_by="owner-x"))

    def test_count_at_baseline_is_clean(self):
        result = self.result_at(2)
        self.assertEqual(result.count, 2)
        self.assertIsNone(result.violation)

    def test_count_below_baseline_is_stale_naming_row_owner_and_new_count(self):
        violation = self.result_at(5).violation
        self.assertIsNotNone(violation)
        for fragment in ("ratchet-row", "owner-x", "2 occurrence(s)", "below baseline 5"):
            self.assertIn(fragment, violation)

    def test_count_above_baseline_names_row_owner_target_and_hits(self):
        violation = self.result_at(1).violation
        self.assertIsNotNone(violation)
        for fragment in ("ratchet-row", "2 occurrence(s)", "baseline 1", "target 0",
                         "owner-x", "src/a.rs:2", "src/a.rs:4"):
            self.assertIn(fragment, violation)
        self.assertNotIn("below baseline", violation)

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


class RowIdentity(TempRoot):

    def test_duplicate_row_id_is_rejected_naming_it(self):
        self.write("src/a.rs", rust_fn("    needle();"))
        twins = (demo_row(r"\bneedle\(", "src/a.rs", row_id="twin"),
                 demo_row(r"\bother\(", "src/a.rs", row_id="twin"))
        with self.assertRaises(ValueError) as caught:
            rcr.evaluate(twins, self.root)
        self.assertIn("'twin'", str(caught.exception))


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
