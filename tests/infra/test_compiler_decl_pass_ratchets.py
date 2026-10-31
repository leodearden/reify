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
from typing import NamedTuple

INFRA_DIR = Path(__file__).resolve().parent
REPO_ROOT = INFRA_DIR.parents[1]

sys.path.insert(0, str(INFRA_DIR))

import rust_construct_ratchet as rcr  # noqa: E402

ROWS = (
    # D12 R1: re-runs of the three let-kind predicates.
    rcr.Row(row_id="R1",
            construct=re.compile(
                r"(?<!fn )\b(?:is_geometry_let|classify_geometry_list_let|is_selector_expr)\("),
            view=rcr.View.CODE,
            paths=("crates/reify-compiler/src/entity.rs",
                   "crates/reify-compiler/src/guards.rs"),
            baseline=17, target=1, lowered_by="β #8731"),
    # D12 R2: "__connector_ string literals in production code.
    rcr.Row(row_id="R2",
            construct=re.compile('"__connector_'),
            view=rcr.View.STARTS_IN_CODE,
            paths=("crates/*/src/**/*.rs",),
            baseline=4, target=1, lowered_by="ζ #8735"),
    # D12 R3: per-relation arity checks.
    rcr.Row(row_id="R3",
            construct=re.compile(r"args\.len\(\)\s*==\s*3(?![0-9])"),
            view=rcr.View.CODE,
            paths=("crates/reify-compiler/src/relation_signatures.rs",),
            baseline=10, target=0, lowered_by="λ #8741"),
)

FIXTURE_LINES = {
    "R1": "    is_geometry_let(&value, functions, &known, &selectors);",
    "R2": '    let name = format!("__connector_{}", index);',
    "R3": "    if args.len() == 3 { return None; }",
}

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


def concrete(glob):
    return glob.replace("**", "nested").replace("*", "demo")


def write_file(root, rel, text, mode="w"):
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open(mode, encoding="utf-8") as handle:
        handle.write(text)
    return path


def write_row_scope(root, row):
    """The clean twin: `baseline` samples in the first scope file, the rest empty."""
    first, *rest = (concrete(glob) for glob in row.paths)
    write_file(root, first, rust_fn(*[FIXTURE_LINES[row.row_id]] * row.baseline))
    for rel in rest:
        write_file(root, rel, rust_fn())


def add_flag_sample(root, row):
    """Append one sample to the last scope file; return its 'path:line'."""
    last = concrete(row.paths[-1])
    line = (root / last).read_text(encoding="utf-8").count("\n") + 2
    write_file(root, last, rust_fn(FIXTURE_LINES[row.row_id]), mode="a")
    return f"{last}:{line}"


def rows_by_id():
    return {row.row_id: row for row in ROWS}


class TempRoot(unittest.TestCase):
    """A per-test temp dir standing in for the repo root."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, rel, text):
        return write_file(self.root, rel, text)

    def fresh_root(self):
        return Path(tempfile.mkdtemp(dir=self.root))

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


class EveryRowHasAFlagClearFixture(unittest.TestCase):

    def test_fixture_ids_match_row_ids(self):
        self.assertEqual(sorted(row.row_id for row in ROWS), sorted(FIXTURE_LINES))


class RowsFlagAndClear(TempRoot):

    def test_clean_twin_at_baseline_is_green(self):
        for row in ROWS:
            with self.subTest(row.row_id):
                root = self.fresh_root()
                write_row_scope(root, row)
                (result,) = rcr.evaluate((row,), root)
                self.assertEqual(result.count, row.baseline)
                self.assertIsNone(result.violation, result.violation)

    def test_one_more_in_the_last_scope_file_reds(self):
        for row in ROWS:
            with self.subTest(row.row_id):
                root = self.fresh_root()
                write_row_scope(root, row)
                location = add_flag_sample(root, row)
                (result,) = rcr.evaluate((row,), root)
                self.assertEqual(result.count, row.baseline + 1)
                self.assertIsNotNone(result.violation)
                self.assertIn(row.row_id, result.violation)
                self.assertIn(location, result.violation)


class Case(NamedTuple):
    text: str
    rel: str | None = None


NOT_COUNTED = {
    "R1": (
        Case(rust_fn("    // is_geometry_let(x)")),
        Case(rust_fn("    /// is_selector_expr(x)")),
        Case(rust_fn('    let s = "classify_geometry_list_let(";')),
        Case("fn is_geometry_let(e: &Expr) -> bool { false }\n"),
        Case(rust_fn("    my_is_geometry_let(x);")),
        Case(cfg_test_module("is_geometry_let(x);")),
        Case(rust_fn("    is_geometry_let(x);"), rel="crates/reify-compiler/src/geometry.rs"),
    ),
    "R2": (
        Case(rust_fn('    // mint "__connector_0"')),
        Case(rust_fn('    /// e.g. "__connector_0"')),
        Case(rust_fn('    mint(); /* "__connector_0" */')),
        Case(rust_fn('    let doc = r#"see', '    "__connector_0" here"#;')),
        Case(rust_fn(r'    let quoted = "\"__connector_0\"";')),
        Case(rust_fn("    let p = __connector_prefix;")),
        Case(cfg_test_module('let n = "__connector_0";')),
        Case(rust_fn('    let n = "__connector_0";'), rel="crates/demo/tests/t.rs"),
    ),
    "R3": (
        Case(rust_fn("    if args.len() == 30 {}")),
        Case(rust_fn("    // args.len() == 3")),
        Case(cfg_test_module("assert!(args.len() == 3);")),
    ),
}

COUNTED = {
    "R1": (2, Case(rust_fn("    is_selector_expr(a); classify_geometry_list_let(b);"))),
    "R2": (2, Case(rust_fn('    /* c */ name.starts_with("__connector_") || f("__connector_");'))),
    "R3": (1, Case(rust_fn("    if args.len()==3 {}"))),
}


class RowExclusions(TempRoot):

    def evaluate_case(self, row, case):
        root = self.fresh_root()
        write_row_scope(root, row)
        write_file(root, case.rel or concrete(row.paths[0]), case.text, mode="a")
        (result,) = rcr.evaluate((row,), root)
        return result

    def test_excluded_spellings_do_not_count(self):
        rows = rows_by_id()
        for row_id, cases in NOT_COUNTED.items():
            for case in cases:
                with self.subTest(row_id, text=case.text, rel=case.rel):
                    result = self.evaluate_case(rows[row_id], case)
                    self.assertEqual(result.count, rows[row_id].baseline)
                    self.assertIsNone(result.violation, result.violation)

    def test_counted_spellings_count_every_occurrence(self):
        rows = rows_by_id()
        for row_id, (added, case) in COUNTED.items():
            with self.subTest(row_id, text=case.text):
                result = self.evaluate_case(rows[row_id], case)
                self.assertEqual(result.count, rows[row_id].baseline + added)


class LiveTree(unittest.TestCase):

    def test_every_row_sits_exactly_at_its_baseline(self):
        for result in rcr.evaluate(ROWS, REPO_ROOT):
            print(result.summary)
            with self.subTest(result.row.row_id):
                self.assertIsNone(result.violation, result.violation)


if __name__ == "__main__":
    unittest.main()
