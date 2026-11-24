#!/usr/bin/env python3
"""Construct ratchets for trampoline decode and result build.

PRD docs/prds/v0_6/trampoline-decode-and-result-build.md §4 C6; engine in
tests/infra/rust_construct_ratchet.py, whose contract suite is
tests/infra/test_rust_construct_ratchet.py. The discovered member is the wrapper
tests/infra/test_trampoline_decode_ratchet.sh. Later S15 leaves add their rows to
this file. Run this file directly to print each row's measured count.
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

RUST_SOURCES = "crates/*/src/**/*.rs"
GUI_SOURCES = "gui/src-tauri/src/**/*.rs"

IR_AND_CTOR_PATH = (
    "crates/reify-ir/",
    "crates/reify-compiler/src/expr.rs",
    "crates/reify-compiler/src/auto_type_param.rs",
    "crates/reify-expr/src/lib.rs",
)
TEST_ONLY = (
    "crates/reify-test-support/",
    "crates/reify-eval/src/engine_build/tests.rs",
    "crates/reify-stdlib/src/flexures/test_util.rs",
)
CTOR_EQUIVALENT_PLACEHOLDERS = (
    "crates/reify-eval/src/engine_eval.rs",
    "crates/reify-eval/src/engine_constraints.rs",
    "crates/reify-eval/src/unfold.rs",
    "crates/reify-stdlib/src/flexures/common.rs",
)
EXCLUSIONS = IR_AND_CTOR_PATH + TEST_ONLY + CTOR_EQUIVALENT_PLACEHOLDERS
GUI_TEST_ONLY = ("gui/src-tauri/src/tests/",)
MASS_PROPERTIES_OWNER = "crates/reify-stdlib/src/dynamics/mass_props.rs"

REGISTRY_FREE_LITERAL = re.compile(r"StructureTypeId\(\s*(?:u32::MAX|0)\s*\)")
REGISTRY_META_BY_ID = re.compile(
    r"\.(?:meta|declared_bounds|name_for)\(\s*&?\s*"
    r"[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*\.type_id\b")


def result_type_write(structure):
    """A write of `structure`'s type_name literal: a `type_name:` field, a
    `registry_free(` / `mint_[registry_free_]instance(` argument, or the literal
    opening its own line (rustfmt's multi-line argument). A read such as
    `d.type_name == "<S>"` or `StructureRef("<S>")` matches none of these."""
    return re.compile(r'(?:type_name\s*:\s*|registry_free\(\s*|instance\(\s*|^\s*)"'
                      + re.escape(structure) + '"')


def _is_excluded(path, excluded):
    return any(path.startswith(entry) if entry.endswith("/") else path == entry
               for entry in excluded)


def scan_set(root, globs, excluded):
    """Repo-relative posix paths the globs match, minus each path equal to an
    excluded file entry or under an excluded directory entry (ending in '/')."""
    root = Path(root)
    matched = {path.relative_to(root).as_posix()
               for glob in globs for path in root.glob(glob) if path.is_file()}
    return tuple(sorted(path for path in matched if not _is_excluded(path, excluded)))


def rows(root):
    return (
        rcr.Row(row_id="registry-free-literal",
                construct=REGISTRY_FREE_LITERAL,
                view=rcr.View.STARTS_IN_CODE,
                paths=scan_set(root, (RUST_SOURCES,), EXCLUSIONS),
                baseline=18, target=0,
                lowered_by="ε #8690, ζ₀ #9287, ζ #8693, τ #9291 (R5 M5)"),
        rcr.Row(row_id="registry-free-placeholder",
                construct=REGISTRY_FREE_LITERAL,
                view=rcr.View.STARTS_IN_CODE,
                paths=CTOR_EQUIVALENT_PLACEHOLDERS,
                baseline=5, target=5,
                lowered_by="R5 M5 exempt ctor-equivalent; #7867 retires the field"),
        rcr.Row(row_id="registry-free-gui",
                construct=REGISTRY_FREE_LITERAL,
                view=rcr.View.STARTS_IN_CODE,
                paths=scan_set(root, (GUI_SOURCES,), GUI_TEST_ONLY),
                baseline=2, target=2,
                lowered_by="exempt gui display defaults; S25 splits engine.rs, "
                           "#7867 retires the field"),
        rcr.Row(row_id="registry-meta-by-id",
                construct=REGISTRY_META_BY_ID,
                view=rcr.View.STARTS_IN_CODE,
                paths=(RUST_SOURCES, GUI_SOURCES),
                baseline=0, target=0, lowered_by="δ #8686"),
        rcr.Row(row_id="result-type-literal-mass-properties",
                construct=result_type_write("MassProperties"),
                view=rcr.View.STARTS_IN_CODE,
                paths=scan_set(root, (RUST_SOURCES, GUI_SOURCES),
                               TEST_ONLY + GUI_TEST_ONLY + (MASS_PROPERTIES_OWNER,)),
                baseline=0, target=0, lowered_by="η #8697"),
    )


FIXTURE_LINES = {
    "registry-free-literal": "    let id = StructureTypeId(u32::MAX);",
    "registry-free-placeholder": "    let id = StructureTypeId(0);",
    "registry-free-gui": "    let id = StructureTypeId(u32::MAX);",
    "registry-meta-by-id": "    let m = registry.meta(data.type_id);",
    "result-type-literal-mass-properties": '    let v = mint_instance("MassProperties", fields);',
}

PRODUCER = "crates/demo/src/producer.rs"
GUI_FILE = "gui/src-tauri/src/demo.rs"
GUI_TEST_FILE = "gui/src-tauri/src/tests/demo_tests.rs"
EXCLUDED_FILE = "crates/reify-ir/src/v.rs"


def sample_file(row_id):
    return {"registry-free-literal": PRODUCER,
            "registry-free-placeholder": CTOR_EQUIVALENT_PLACEHOLDERS[-1],
            "registry-free-gui": GUI_FILE,
            "registry-meta-by-id": GUI_FILE,
            "result-type-literal-mass-properties": PRODUCER}[row_id]


def write_clean_twin(root):
    """Every row's scope, holding exactly `baseline` samples per row."""
    for rel in (PRODUCER, GUI_FILE, MASS_PROPERTIES_OWNER) + CTOR_EQUIVALENT_PLACEHOLDERS:
        write_file(root, rel, rust_fn())
    for row in rows(root):
        if row.baseline:
            samples = [FIXTURE_LINES[row.row_id]] * row.baseline
            write_file(root, sample_file(row.row_id), rust_fn(*samples), mode="a")


def append_sample(root, rel, line):
    """Append one sample to `rel`; return its 'path:line'."""
    start = (root / rel).read_text(encoding="utf-8").count("\n")
    write_file(root, rel, rust_fn(line), mode="a")
    return f"{rel}:{start + 2}"


def evaluate_by_id(root):
    return {result.row.row_id: result for result in rcr.evaluate(rows(root), root)}


class EveryRowHasAFlagClearFixture(TempRoot):

    def test_fixture_ids_match_row_ids(self):
        write_clean_twin(self.root)
        self.assertEqual(sorted(row.row_id for row in rows(self.root)), sorted(FIXTURE_LINES))


class ScanSet(TempRoot):

    def test_keeps_producers_and_drops_every_exclusion_class(self):
        kept = ("crates/reify-eval/src/compute_targets/demo.rs",
                "crates/reify-compiler/src/other.rs")
        dropped = ("crates/reify-ir/src/v.rs",
                   "crates/reify-compiler/src/expr.rs",
                   "crates/reify-expr/src/lib.rs",
                   "crates/reify-test-support/src/f.rs",
                   "crates/reify-eval/src/engine_build/tests.rs",
                   "crates/reify-eval/src/engine_eval.rs")
        for rel in kept + dropped:
            self.write(rel, rust_fn())
        self.assertEqual(scan_set(self.root, (RUST_SOURCES,), EXCLUSIONS), tuple(sorted(kept)))

    def test_gui_scan_keeps_production_and_drops_the_test_tree(self):
        kept = ("gui/src-tauri/src/engine.rs", "gui/src-tauri/src/debug_server/tools.rs")
        dropped = ("gui/src-tauri/src/tests/engine_tests.rs",)
        for rel in kept + dropped:
            self.write(rel, rust_fn())
        self.assertEqual(scan_set(self.root, (GUI_SOURCES,), GUI_TEST_ONLY), tuple(sorted(kept)))


class RowsFlagAndClear(TempRoot):

    def test_clean_twin_at_baseline_is_green(self):
        write_clean_twin(self.root)
        for row_id, result in evaluate_by_id(self.root).items():
            with self.subTest(row_id):
                self.assertEqual(result.count, result.row.baseline)
                self.assertIsNone(result.violation, result.violation)

    def test_one_more_in_a_scanned_file_reds(self):
        for row_id, line in FIXTURE_LINES.items():
            with self.subTest(row_id):
                root = self.fresh_root()
                write_clean_twin(root)
                location = append_sample(root, sample_file(row_id), line)
                result = evaluate_by_id(root)[row_id]
                self.assertEqual(result.count, result.row.baseline + 1)
                self.assertIsNotNone(result.violation)
                self.assertIn(row_id, result.violation)
                self.assertIn(location, result.violation)


class Case(NamedTuple):
    text: str
    rel: str | None = None


REGISTRY_FREE_NOT_COUNTED = (
    Case(rust_fn("    // StructureTypeId(0)")),
    Case(rust_fn("    /// StructureTypeId(u32::MAX)")),
    Case(rust_fn('    let s = "StructureTypeId(0)";')),
    Case(rust_fn("    let id = StructureTypeId::REGISTRY_FREE;")),
    Case(rust_fn("    let id = StructureTypeId(7);")),
    Case(cfg_test_module("let id = StructureTypeId(0);")),
    Case(rust_fn("    let id = StructureTypeId(0);"), rel=EXCLUDED_FILE),
)

NOT_COUNTED = {
    "registry-free-literal": REGISTRY_FREE_NOT_COUNTED,
    "registry-free-placeholder": REGISTRY_FREE_NOT_COUNTED,
    "registry-free-gui": REGISTRY_FREE_NOT_COUNTED + (
        Case(rust_fn("    let id = StructureTypeId(0);"), rel=GUI_TEST_FILE),),
    "registry-meta-by-id": (
        Case(rust_fn("    let m = registry.meta(id);")),
        Case(rust_fn("    let m = registry.meta_for(data);")),
        Case(rust_fn("    let m = reg.meta(reg.id_for(&d.type_name)?);")),
        Case(rust_fn("    // registry.meta(data.type_id)")),
    ),
    "result-type-literal-mass-properties": (
        Case(rust_fn('    let is_mp = d.type_name == "MassProperties";')),
        Case(rust_fn('    let t = Type::StructureRef("MassProperties".to_string());')),
        Case(rust_fn('    // type_name: "MassProperties"')),
        Case(rust_fn('    let s = r#"type_name: "MassProperties""#;')),
        Case(cfg_test_module('let v = mint_instance("MassProperties", fields);')),
        Case(rust_fn('    let v = mint_instance("MassProperties", fields);'),
             rel=MASS_PROPERTIES_OWNER),
    ),
}

REGISTRY_FREE_COUNTED = ((3, Case(rust_fn(
    "    let a = StructureTypeId(u32::MAX);",
    "    let b = StructureTypeId( 0 );",
    "    let c = reify_ir::StructureTypeId(0);"))),)

COUNTED = {
    "registry-free-literal": REGISTRY_FREE_COUNTED,
    "registry-free-placeholder": REGISTRY_FREE_COUNTED,
    "registry-free-gui": REGISTRY_FREE_COUNTED,
    "registry-meta-by-id": (
        (3, Case(rust_fn(
            "    let a = registry.meta(data.type_id);",
            "    let b = registry.declared_bounds(inst.type_id);",
            "    let c = registry.meta(&self.data.type_id);"))),
        (1, Case(rust_fn("    let m = registry.meta(data.type_id);"), rel=PRODUCER)),
    ),
    "result-type-literal-mass-properties": (
        (3, Case(rust_fn(
            "    let a = StructureInstanceData {",
            '        type_name: "MassProperties".to_string(),',
            "    };",
            '    let b = StructureInstanceData::registry_free("MassProperties", f);',
            "    let c = mint_instance(",
            '        "MassProperties",',
            "        fields,",
            "    );"))),
    ),
}


class RowExclusions(TempRoot):

    def evaluate_case(self, row_id, case):
        root = self.fresh_root()
        write_clean_twin(root)
        write_file(root, case.rel or sample_file(row_id), case.text, mode="a")
        return evaluate_by_id(root)[row_id]

    def test_excluded_spellings_do_not_count(self):
        for row_id, cases in NOT_COUNTED.items():
            for case in cases:
                with self.subTest(row_id, text=case.text, rel=case.rel):
                    result = self.evaluate_case(row_id, case)
                    self.assertEqual(result.count, result.row.baseline)
                    self.assertIsNone(result.violation, result.violation)

    def test_counted_spellings_count_every_occurrence(self):
        for row_id, cases in COUNTED.items():
            for added, case in cases:
                with self.subTest(row_id, text=case.text, rel=case.rel):
                    result = self.evaluate_case(row_id, case)
                    self.assertEqual(result.count, result.row.baseline + added)


class LiveTree(unittest.TestCase):

    def test_every_exclusion_names_a_live_path(self):
        for entry in EXCLUSIONS + GUI_TEST_ONLY + (MASS_PROPERTIES_OWNER,):
            with self.subTest(entry):
                self.assertTrue((REPO_ROOT / entry).exists(), f"{entry} names no path")

    def test_every_row_sits_exactly_at_its_baseline(self):
        for result in rcr.evaluate(rows(REPO_ROOT), REPO_ROOT):
            print(result.summary)
            with self.subTest(result.row.row_id):
                self.assertIsNone(result.violation, result.violation)


if __name__ == "__main__":
    unittest.main()
