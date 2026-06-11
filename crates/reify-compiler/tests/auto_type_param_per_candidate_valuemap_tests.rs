//! Unit tests for `seed_candidate_value_map` helper (PRD §5.2 β).
//!
//! # What this file covers
//!
//! Two test groups:
//!
//! 1. **Helper unit tests** — build a synthetic candidate `TopologyTemplate`
//!    and assert that `seed_candidate_value_map` extracts exactly the
//!    direct-literal-default cells (one-level, evaluator-free seeding).
//!
//! 2. **Fixture-Ambiguous baseline** — reads
//!    `examples/auto/bearing_constraint_select.ri` from disk, compiles it
//!    under the default compile-time stub checker, and asserts that an
//!    `AutoTypeParamAmbiguous` Error diagnostic is present.  This pins the
//!    "stub → Ambiguous" baseline that ζ's real-checker e2e will contrast
//!    against (the "real → Selected" half lives in `reify-eval`).
//!
//! # RED states
//!
//! - **Step 1 (RED):** `seed_candidate_value_map` does not yet exist — this
//!   file fails to compile at the `use reify_compiler::auto_type_param::seed_candidate_value_map`
//!   import line.
//!
//! - **Step 3 (RED):** `examples/auto/bearing_constraint_select.ri` does not
//!   yet exist — `bearing_constraint_select_is_ambiguous_under_stub` fails
//!   at the `fs::read_to_string` call (file not found).

use reify_compiler::auto_type_param::seed_candidate_value_map;
use reify_core::{DimensionVector, Type, ValueCellId};
use reify_ir::{CompiledExpr, Value};
use reify_test_support::TopologyTemplateBuilder;

// ─── helper: a length Value for n millimetres ────────────────────────────────

fn mm(n: f64) -> Value {
    Value::Scalar {
        si_value: n * 1e-3,
        dimension: DimensionVector::LENGTH,
    }
}

// ─── helper: a non-literal CompiledExpr (ValueRef) ──────────────────────────

fn non_literal_expr() -> CompiledExpr {
    // Any non-Literal kind is fine; ValueRef is the simplest to construct.
    CompiledExpr::value_ref(
        ValueCellId::new("GasketSeal", "thickness"),
        Type::length(),
    )
}

// ─── synthetic candidate template ────────────────────────────────────────────

/// Build a synthetic "GasketSeal" candidate `TopologyTemplate` with:
/// - `thickness   : Length = 2mm`  — literal default → should be seeded
/// - `outer_diameter : Length = 30mm` — literal default → should be seeded
/// - `color       : Real` with no default (`None`) → must be skipped
/// - `computed    : Length = <ValueRef expr>` — non-literal → must be skipped
fn gasket_seal_template() -> reify_compiler::TopologyTemplate {
    TopologyTemplateBuilder::new("GasketSeal")
        .param(
            "GasketSeal",
            "thickness",
            Type::length(),
            Some(CompiledExpr::literal(mm(2.0), Type::length())),
        )
        .param(
            "GasketSeal",
            "outer_diameter",
            Type::length(),
            Some(CompiledExpr::literal(mm(30.0), Type::length())),
        )
        .param("GasketSeal", "color", Type::Real, None)
        .param(
            "GasketSeal",
            "computed",
            Type::length(),
            Some(non_literal_expr()),
        )
        .build()
}

// ─── Group 1: helper unit tests ──────────────────────────────────────────────

/// `seed_candidate_value_map` must extract exactly the direct-literal-default
/// cells and key them under the supplied `param_member` name.
///
/// Assertions (plan step-1 acceptance criteria a/b/c):
///
/// (a) `seal.thickness` and `seal.outer_diameter` are present, keyed via
///     `ValueCellId::new("seal", field)`.
/// (b) `seal.color` (no default) and `seal.computed` (non-literal default)
///     are NOT present — one-level, literal-only seeding.
/// (c) The map length is exactly 2 (the two literal-default cells).
#[test]
fn seed_extracts_literal_defaults_and_skips_others() {
    let tmpl = gasket_seal_template();
    let map = seed_candidate_value_map(&tmpl, "seal");

    // (a) literal-default cells are present with the expected values
    let thickness_key = ValueCellId::new("seal", "thickness");
    let outer_diameter_key = ValueCellId::new("seal", "outer_diameter");

    assert_eq!(
        map.get(&thickness_key),
        Some(&mm(2.0)),
        "seal.thickness must be seeded with the 2mm literal"
    );
    assert_eq!(
        map.get(&outer_diameter_key),
        Some(&mm(30.0)),
        "seal.outer_diameter must be seeded with the 30mm literal"
    );

    // (b) None-default and non-literal-default cells must be absent
    let color_key = ValueCellId::new("seal", "color");
    let computed_key = ValueCellId::new("seal", "computed");

    assert!(
        map.get(&color_key).is_none(),
        "seal.color (no default) must NOT appear in the seeded ValueMap"
    );
    assert!(
        map.get(&computed_key).is_none(),
        "seal.computed (non-literal ValueRef default) must NOT appear in the seeded ValueMap"
    );

    // (c) exactly 2 entries — no extras
    assert_eq!(
        map.len(),
        2,
        "map must contain exactly the two literal-default cells"
    );
}

/// seed_candidate_value_map returns an empty map for a template whose cells
/// all have `default_expr = None`.
#[test]
fn seed_returns_empty_map_for_template_with_no_literal_defaults() {
    let tmpl = TopologyTemplateBuilder::new("BareSeal")
        .param("BareSeal", "x", Type::Real, None)
        .param("BareSeal", "y", Type::Real, None)
        .build();

    let map = seed_candidate_value_map(&tmpl, "seal");
    assert!(
        map.is_empty(),
        "expected empty ValueMap when the template has no literal-default cells"
    );
}

/// seed_candidate_value_map uses the `param_member` argument as the `entity`
/// half of every `ValueCellId` key, regardless of the candidate template's
/// own entity name.
#[test]
fn seed_keys_entries_under_param_member_not_candidate_name() {
    let tmpl = TopologyTemplateBuilder::new("MySeal")
        .param(
            "MySeal",
            "thickness",
            Type::length(),
            Some(CompiledExpr::literal(mm(3.0), Type::length())),
        )
        .build();

    let map = seed_candidate_value_map(&tmpl, "my_param");

    // Key must use "my_param" (the param_member arg), not "MySeal" (the template name).
    let correct_key = ValueCellId::new("my_param", "thickness");
    let wrong_key = ValueCellId::new("MySeal", "thickness");

    assert!(
        map.get(&correct_key).is_some(),
        "key must use param_member ('my_param') as the entity half"
    );
    assert!(
        map.get(&wrong_key).is_none(),
        "key must NOT use the candidate template name ('MySeal') as the entity half"
    );
}

// ─── Group 2: fixture-Ambiguous baseline ─────────────────────────────────────

/// Path to the fixture under `examples/auto/`.
const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/auto/bearing_constraint_select.ri"
);

/// Compiling `examples/auto/bearing_constraint_select.ri` under the default
/// compile-time stub checker must produce at least one
/// `AutoTypeParamAmbiguous` Error diagnostic.
///
/// Rationale: the fixture uses `auto: Seal` (strict) with two stub-feasible
/// candidates (`ThinSeal` and `ThickSeal`).  The stub returns
/// `Indeterminate` for every constraint check, so both candidates survive
/// Phase B and Phase C emits `E_AUTO_TYPE_PARAM_AMBIGUOUS`.
///
/// This test asserts PRESENCE (not sole-diagnostic) so that any benign
/// notes about member-access on an unresolved TypeParam do not cause a
/// false failure.  The "real → Selected" half of the regression lives in
/// ζ's reify-eval `auto_type_param_completion_e2e` harness under
/// `SimpleConstraintChecker`.
#[test]
fn bearing_constraint_select_is_ambiguous_under_stub() {
    use reify_compiler::{compile_with_stdlib, parse_with_stdlib};
    use reify_core::{DiagnosticCode, ModulePath, Severity};
    use std::fs;

    let source = fs::read_to_string(FIXTURE_PATH).unwrap_or_else(|e| {
        panic!(
            "cannot read fixture '{}': {} — \
             did you forget to author examples/auto/bearing_constraint_select.ri (step-4)?",
            FIXTURE_PATH, e
        )
    });

    let module_path = ModulePath::single("bearing_constraint_select");
    let parsed = parse_with_stdlib(&source, module_path);
    assert!(
        parsed.errors.is_empty(),
        "fixture must parse without errors; got: {:#?}",
        parsed.errors
    );

    let compiled = compile_with_stdlib(&parsed);

    let has_ambiguous = compiled.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && d.code == Some(DiagnosticCode::AutoTypeParamAmbiguous)
    });
    assert!(
        has_ambiguous,
        "expected at least one AutoTypeParamAmbiguous Error diagnostic; got:\n{:#?}",
        compiled.diagnostics
    );
}
