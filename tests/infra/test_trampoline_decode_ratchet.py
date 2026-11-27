#!/usr/bin/env python3
"""Construct ratchets for trampoline decode and result build.

PRD docs/prds/v0_6/trampoline-decode-and-result-build.md §4 C6; engine in
tests/infra/rust_construct_ratchet.py, whose contract suite is
tests/infra/test_rust_construct_ratchet.py. The discovered member is the wrapper
tests/infra/test_trampoline_decode_ratchet.sh. Later S15 leaves add their rows to
this file. Run this file directly to print each row's measured count.
"""

import fnmatch
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

REIFY_EVAL_SOURCES = "crates/reify-eval/src/**/*.rs"
SRC_TEST_ONLY_NAMES = ("*tests.rs", "*test_support*.rs")
COMPUTE_TARGETS = "crates/reify-eval/src/compute_targets/"
STDLIB_INPUTS = COMPUTE_TARGETS + "stdlib_inputs/"
MODAL_OPS = "crates/reify-eval/src/modal_ops.rs"
SHELL_EXTRACT = "crates/reify-eval/src/shell_extract_compute.rs"
STDLIB_INPUTS_CONSUMERS = tuple(
    COMPUTE_TARGETS + name for name in
    ("elastic_static.rs", "buckling.rs", "as_printed_material.rs", "as_printed_material_r0.rs")
) + (MODAL_OPS,)
ELASTIC_MATERIAL_PARAMS = ("youngs_modulus", "poisson_ratio")
PARAM_LABEL_ALLOWLIST = {
    "youngs_modulus": (COMPUTE_TARGETS + "membrane_load.rs",
                       COMPUTE_TARGETS + "tensegrity_load.rs"),
}

REGISTRY_FREE_LITERAL = re.compile(r"StructureTypeId\(\s*(?:u32::MAX|0)\s*\)")
REGISTRY_META_BY_ID = re.compile(
    r"\.(?:meta|declared_bounds|name_for)\(\s*&?\s*"
    r"[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*\.type_id\b")
READ_SCALAR_SI = re.compile(r"\bread_scalar_si\b")
SILENT_ZERO = re.compile(r"unwrap_or\(\s*0\.0\s*\)|=>\s*0\.0\b|\[\s*0\.0\s*;\s*3\s*\]")
# A construction opens its body at the line end or with a `field:`; a pattern
# opens it with `..` or a shorthand `field,`.
INLINE_FAILED = re.compile(r"ComputeOutcome::Failed\s*\{(?:\s*$|\s*[A-Za-z_]\w*\s*:(?!:))")


def result_type_write(structure):
    """A write of `structure`'s type_name literal: a `type_name:` field, a
    `registry_free(` / `mint_[registry_free_]instance(` argument, or rustfmt's
    multi-line argument -- the literal, optionally converted to a `String`,
    alone on its line and ended by a comma. Reads such as
    `d.type_name == "<S>"`, `StructureRef("<S>")` and a match arm
    `"<S>" => ...` match none of these."""
    literal = '"' + re.escape(structure) + '"'
    return re.compile(r'(?:type_name\s*:\s*|registry_free\(\s*|instance\(\s*)' + literal
                      + r'|^\s*' + literal
                      + r'(?:\.(?:to_string|to_owned|into)\(\))?\s*,')


def _is_excluded(path, excluded, excluded_names):
    name = path.rsplit("/", 1)[-1]
    return (any(path.startswith(entry) if entry.endswith("/") else path == entry
                for entry in excluded)
            or any(fnmatch.fnmatchcase(name, pattern) for pattern in excluded_names))


def scan_set(root, globs, excluded, excluded_names=()):
    """Repo-relative posix paths the globs match, minus each path equal to an
    excluded file entry or under an excluded directory entry (ending in '/'),
    and minus each path whose file name matches an excluded name pattern."""
    root = Path(root)
    matched = {path.relative_to(root).as_posix()
               for glob in globs for path in root.glob(glob) if path.is_file()}
    return tuple(sorted(path for path in matched
                        if not _is_excluded(path, excluded, excluded_names)))


def reify_eval_production(root, excluded=()):
    """reify-eval's src/ files minus its test-only files (a `#[cfg(test)] mod`
    declared from a parent, which the lexer cannot see) and `excluded`."""
    return scan_set(root, (REIFY_EVAL_SOURCES,), excluded, SRC_TEST_ONLY_NAMES)


def param_literal(param):
    return re.compile('"' + re.escape(param) + '"')


def decoded_param_read_row(root, structure_slug, param):
    """C1 inv 1: a declared param is read only by its decoder in stdlib_inputs."""
    return rcr.Row(row_id=f"decoded-param-read-{structure_slug}-{param.replace('_', '-')}",
                   construct=param_literal(param),
                   view=rcr.View.STARTS_IN_CODE,
                   paths=reify_eval_production(
                       root, (STDLIB_INPUTS,) + PARAM_LABEL_ALLOWLIST.get(param, ())),
                   baseline=0, target=0, lowered_by="α #8674")


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
    ) + tuple(decoded_param_read_row(root, "elastic-material", param)
              for param in ELASTIC_MATERIAL_PARAMS) + (
        rcr.Row(row_id="read-scalar-si",
                construct=READ_SCALAR_SI,
                view=rcr.View.CODE,
                paths=reify_eval_production(root),
                baseline=0, target=0, lowered_by="α #8674"),
        rcr.Row(row_id="modal-silent-zero",
                construct=SILENT_ZERO,
                view=rcr.View.CODE,
                paths=(MODAL_OPS,),
                baseline=4, target=3,
                lowered_by="β #8679 (ModalOptions.shift_frequency); the rest are "
                           "non-input index/sampler residuals"),
        rcr.Row(row_id="inline-failed",
                construct=INLINE_FAILED,
                view=rcr.View.CODE,
                paths=STDLIB_INPUTS_CONSUMERS,
                baseline=6, target=6,
                lowered_by="hold: non-decode outcomes, S16 trampoline factoring"),
        rcr.Row(row_id="inline-failed-shell-extract",
                construct=INLINE_FAILED,
                view=rcr.View.CODE,
                paths=(SHELL_EXTRACT,),
                baseline=13, target=1, lowered_by="λ #8706"),
    )


FAILED_CONSTRUCTION = "\n".join((
    "    return ComputeOutcome::Failed {",
    "        diagnostics,",
    "        structured_detail: vec![],",
    "    };"))

FIXTURE_LINES = {
    "registry-free-literal": "    let id = StructureTypeId(u32::MAX);",
    "registry-free-placeholder": "    let id = StructureTypeId(0);",
    "registry-free-gui": "    let id = StructureTypeId(u32::MAX);",
    "registry-meta-by-id": "    let m = registry.meta(data.type_id);",
    "result-type-literal-mass-properties": '    let v = mint_instance("MassProperties", fields);',
    "decoded-param-read-elastic-material-youngs-modulus":
        '    let e = data.fields.get("youngs_modulus");',
    "decoded-param-read-elastic-material-poisson-ratio":
        '    let nu = data.fields.get("poisson_ratio");',
    "read-scalar-si": "    let x = read_scalar_si(v);",
    "modal-silent-zero": "    let m = masses.get(i).copied().unwrap_or(0.0);",
    "inline-failed": FAILED_CONSTRUCTION,
    "inline-failed-shell-extract": FAILED_CONSTRUCTION,
}

PRODUCER = "crates/demo/src/producer.rs"
GUI_FILE = "gui/src-tauri/src/demo.rs"
GUI_TEST_FILE = "gui/src-tauri/src/tests/demo_tests.rs"
EXCLUDED_FILE = "crates/reify-ir/src/v.rs"
EVAL_TRAMPOLINE = COMPUTE_TARGETS + "demo_trampoline.rs"
ELASTIC_STATIC = STDLIB_INPUTS_CONSUMERS[0]
MEMBRANE_LOAD = PARAM_LABEL_ALLOWLIST["youngs_modulus"][0]


def sample_file(row_id):
    return {"registry-free-literal": PRODUCER,
            "registry-free-placeholder": CTOR_EQUIVALENT_PLACEHOLDERS[-1],
            "registry-free-gui": GUI_FILE,
            "registry-meta-by-id": GUI_FILE,
            "result-type-literal-mass-properties": PRODUCER,
            "decoded-param-read-elastic-material-youngs-modulus": EVAL_TRAMPOLINE,
            "decoded-param-read-elastic-material-poisson-ratio": EVAL_TRAMPOLINE,
            "read-scalar-si": MODAL_OPS,
            "modal-silent-zero": MODAL_OPS,
            "inline-failed": ELASTIC_STATIC,
            "inline-failed-shell-extract": SHELL_EXTRACT}[row_id]


def write_clean_twin(root):
    """Every row's scope, holding exactly `baseline` samples per row."""
    for rel in ((PRODUCER, GUI_FILE, MASS_PROPERTIES_OWNER, EVAL_TRAMPOLINE, SHELL_EXTRACT)
                + CTOR_EQUIVALENT_PLACEHOLDERS + STDLIB_INPUTS_CONSUMERS):
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


def evaluate_row(root, row_id):
    (result,) = rcr.evaluate([row for row in rows(root) if row.row_id == row_id], root)
    return result


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

    def test_reify_eval_production_drops_test_only_names_and_other_crates(self):
        kept = (MODAL_OPS, ELASTIC_STATIC, STDLIB_INPUTS + "mod.rs")
        dropped = ("crates/reify-eval/src/engine_build/tests.rs",
                   COMPUTE_TARGETS + "elastic_static_tests.rs",
                   STDLIB_INPUTS + "test_support.rs",
                   "crates/reify-eval/src/realization_read_test_support.rs",
                   "crates/reify-compiler/src/other.rs")
        for rel in kept + dropped:
            self.write(rel, rust_fn())
        self.assertEqual(reify_eval_production(self.root), tuple(sorted(kept)))

    def test_reify_eval_production_drops_excluded_entries(self):
        kept = (MODAL_OPS,)
        dropped = (STDLIB_INPUTS + "mod.rs", MEMBRANE_LOAD)
        for rel in kept + dropped:
            self.write(rel, rust_fn())
        self.assertEqual(reify_eval_production(self.root, (STDLIB_INPUTS, MEMBRANE_LOAD)),
                         kept)


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
                result = evaluate_row(root, row_id)
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

def decoded_param_not_counted(param):
    read = f'let v = data.fields.get("{param}");'
    return (
        Case(rust_fn(f'    // data.fields.get("{param}")')),
        Case(rust_fn(f'    /// read_field(material, "{param}", &spec)')),
        Case(rust_fn(f'    let s = r#"fields.get("{param}")"#;')),
        Case(cfg_test_module(read)),
        Case(rust_fn("    " + read), rel=STDLIB_INPUTS + "elastic_material.rs"),
        Case(rust_fn("    " + read), rel=COMPUTE_TARGETS + "elastic_static_tests.rs"),
        Case(rust_fn("    " + read), rel="crates/reify-eval/src/realization_read_test_support.rs"),
        Case(rust_fn("    " + read), rel="crates/reify-solver-elastic/src/material.rs"),
    ) + tuple(Case(rust_fn("    " + read), rel=rel)
              for rel in PARAM_LABEL_ALLOWLIST.get(param, ()))


INLINE_FAILED_NOT_COUNTED = (
    Case(rust_fn("    let failed = matches!(o, ComputeOutcome::Failed { .. });")),
    Case(rust_fn("    if let ComputeOutcome::Failed { diagnostics, .. } = o {",
                 "        drop(diagnostics);",
                 "    }")),
    Case(rust_fn("    // return ComputeOutcome::Failed {")),
    Case(rust_fn('    let s = "ComputeOutcome::Failed { diagnostics: vec![] }";')),
    Case(cfg_test_module("let o = ComputeOutcome::Failed { diagnostics: vec![], "
                         "structured_detail: vec![] };")),
    Case(rust_fn("    return e.into_failed(TARGET);")),
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
        Case(rust_fn("    match d.type_name.as_str() {",
                     '        "MassProperties" => true,',
                     '        "MassProperties" | "Frame3" => true,',
                     "        _ => false,",
                     "    };")),
        Case(cfg_test_module('let v = mint_instance("MassProperties", fields);')),
        Case(rust_fn('    let v = mint_instance("MassProperties", fields);'),
             rel=MASS_PROPERTIES_OWNER),
    ),
    "decoded-param-read-elastic-material-youngs-modulus":
        decoded_param_not_counted("youngs_modulus"),
    "decoded-param-read-elastic-material-poisson-ratio":
        decoded_param_not_counted("poisson_ratio"),
    "read-scalar-si": (
        Case(rust_fn("    // read_scalar_si(v) is gone")),
        Case(rust_fn('    let s = "read_scalar_si";')),
        Case(rust_fn("    let x = read_scalar_si_checked(v);")),
        Case(cfg_test_module("let x = read_scalar_si(v);")),
        Case(rust_fn("    let x = read_scalar_si(v);"),
             rel="crates/reify-eval/src/geometry_ops/tests.rs"),
        Case(rust_fn("    let x = read_scalar_si(v);"), rel="crates/reify-eval-fea/src/lib.rs"),
    ),
    "modal-silent-zero": (
        Case(rust_fn("    // unwrap_or(0.0)")),
        Case(rust_fn('    let s = "unwrap_or(0.0) / _ => 0.0, / [0.0; 3]";')),
        Case(rust_fn("    let x = v.unwrap_or(0.05);")),
        Case(rust_fn("    let a = [0.05; 3];")),
        Case(rust_fn("    let x = match v {",
                     "        Some(x) => x,",
                     "        _ => 0.05,",
                     "    };")),
        Case(rust_fn("    let acc = vec![0.0_f64; n];")),
        Case(cfg_test_module("let x = v.unwrap_or(0.0);")),
        Case(rust_fn("    let x = v.unwrap_or(0.0);"), rel=ELASTIC_STATIC),
    ),
    "inline-failed": INLINE_FAILED_NOT_COUNTED + (
        Case(rust_fn(FAILED_CONSTRUCTION), rel=STDLIB_INPUTS + "mod.rs"),
        Case(rust_fn(FAILED_CONSTRUCTION), rel=SHELL_EXTRACT),
    ),
    "inline-failed-shell-extract": INLINE_FAILED_NOT_COUNTED + (
        Case(rust_fn(FAILED_CONSTRUCTION), rel=ELASTIC_STATIC),
    ),
}

REGISTRY_FREE_COUNTED = ((3, Case(rust_fn(
    "    let a = StructureTypeId(u32::MAX);",
    "    let b = StructureTypeId( 0 );",
    "    let c = reify_ir::StructureTypeId(0);"))),)

def decoded_param_counted(param):
    return ((4, Case(rust_fn(
        f'    let a = data.fields.get("{param}");',
        f'    let b = read_field(material, "{param}", &spec);',
        f'    let c = accept_field(data, "{param}", &spec);',
        "    let d = modulus_field(",
        "        data,",
        f'        "{param}",',
        "    );"))),)


INLINE_FAILED_COUNTED = ((2, Case(rust_fn(
    "    return ComputeOutcome::Failed {",
    "        diagnostics: vec![d],",
    "        structured_detail: vec![],",
    "    };",
    "    let q = crate::ComputeOutcome::Failed { diagnostics: vec![], structured_detail: vec![] };"))),)

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
        (1, Case(rust_fn(
            "    let d = StructureInstanceData::registry_free(",
            '        "MassProperties".to_string(),',
            "        f,",
            "    );"))),
    ),
    "decoded-param-read-elastic-material-youngs-modulus": decoded_param_counted("youngs_modulus"),
    "decoded-param-read-elastic-material-poisson-ratio": decoded_param_counted("poisson_ratio") + (
        (1, Case(rust_fn('    let nu = data.fields.get("poisson_ratio");'), rel=MEMBRANE_LOAD)),
    ),
    "read-scalar-si": (
        (2, Case("fn read_scalar_si(v: &Value) -> f64 {\n    1.0\n}\n"
                 + rust_fn("    let x = read_scalar_si(v);"))),
        (1, Case(rust_fn("    let x = read_scalar_si(v);"), rel=STDLIB_INPUTS + "mod.rs")),
    ),
    "modal-silent-zero": ((3, Case(rust_fn(
        "    let a = v.unwrap_or(0.0);",
        "    let b = match v {",
        "        Some(x) => x,",
        "        _ => 0.0,",
        "    };",
        "    let c = shape.get(node).copied().unwrap_or([0.0; 3]);"))),),
    "inline-failed": INLINE_FAILED_COUNTED + (
        (1, Case(rust_fn(FAILED_CONSTRUCTION), rel=STDLIB_INPUTS_CONSUMERS[1])),
    ),
    "inline-failed-shell-extract": INLINE_FAILED_COUNTED,
}


class RowExclusions(TempRoot):

    def evaluate_case(self, row_id, case):
        root = self.fresh_root()
        write_clean_twin(root)
        write_file(root, case.rel or sample_file(row_id), case.text, mode="a")
        return evaluate_row(root, row_id)

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
        allowlisted = tuple(rel for rels in PARAM_LABEL_ALLOWLIST.values() for rel in rels)
        for entry in (EXCLUSIONS + GUI_TEST_ONLY + (MASS_PROPERTIES_OWNER, STDLIB_INPUTS)
                      + allowlisted):
            with self.subTest(entry):
                self.assertTrue((REPO_ROOT / entry).exists(), f"{entry} names no path")

    def test_every_allowlisted_pair_still_spells_its_param(self):
        for param, rels in PARAM_LABEL_ALLOWLIST.items():
            for rel in rels:
                with self.subTest(param=param, rel=rel):
                    probe = rcr.Row(row_id=f"allowlist-probe-{param}",
                                    construct=param_literal(param),
                                    view=rcr.View.STARTS_IN_CODE, paths=(rel,),
                                    baseline=0, target=0, lowered_by="allowlist liveness")
                    (result,) = rcr.evaluate((probe,), REPO_ROOT)
                    self.assertGreater(result.count, 0,
                                       f"stale allowlist entry: {rel} no longer spells {param!r}")

    def test_every_row_sits_exactly_at_its_baseline(self):
        for result in rcr.evaluate(rows(REPO_ROOT), REPO_ROOT):
            print(result.summary)
            with self.subTest(result.row.row_id):
                self.assertIsNone(result.violation, result.violation)


if __name__ == "__main__":
    unittest.main()
