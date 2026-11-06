#!/usr/bin/env python3
"""Construct ratchets for the compiler declaration passes.

PRD docs/prds/v0_6/compiler-declaration-passes.md §4 D12; engine in
tests/infra/rust_construct_ratchet.py, whose contract suite is
tests/infra/test_rust_construct_ratchet.py. The discovered member is the wrapper
tests/infra/test_compiler_decl_pass_ratchets.sh. Run this file directly to
print each row's measured count.
"""

import re
import sys
import unittest
from pathlib import Path
from typing import NamedTuple

INFRA_DIR = Path(__file__).resolve().parent
REPO_ROOT = INFRA_DIR.parents[1]

sys.path.insert(0, str(INFRA_DIR))

import rust_construct_ratchet as rcr  # noqa: E402
from rust_construct_ratchet_fixtures import (  # noqa: E402
    TempRoot, cfg_test_module, rust_fn, write_file)

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


def concrete(glob):
    return glob.replace("**", "nested").replace("*", "demo")


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
