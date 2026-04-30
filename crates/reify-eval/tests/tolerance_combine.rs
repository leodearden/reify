//! Engine-level integration tests for the output-occurrence × active-purpose
//! tolerance combiner (per task 2650 / PRD
//! `docs/prds/v0_2/per-purpose-tolerance.md`).
//!
//! Activates a hand-built `manufacturing` purpose whose constraint is the
//! recognised `RepresentationWithin(<bare-StructureRef>, <length-literal>)`
//! shape, plus a hand-built `STEPOutput` template carrying its own
//! `RepresentationWithin` body constraint, then asserts the combined
//! demanded tolerance is observable via `Engine::demanded_tolerance_for_output`.

use reify_test_support::builders::{
    CompiledModuleBuilder, CompiledPurposeBuilder, TopologyTemplateBuilder,
};
use reify_test_support::make_engine;
use reify_types::{CompiledExpr, DimensionVector, ModulePath, Type, Value, ValueCellId};

/// Build an `STEPOutput`-shaped `TopologyTemplate` carrying a single
/// `RepresentationWithin(<ValueRef typed StructureRef>, <length-literal>)`
/// body constraint at SI `output_tol` metres. The template's name is
/// `"STEPOutput"` so its constraint lands in the runtime graph at
/// `(entity = "STEPOutput", index = 0)` — see
/// `crate::tolerance_combine::extract_output_tolerance_bound` for the
/// recognition contract.
fn step_output_template(output_tol: f64) -> reify_compiler::TopologyTemplate {
    let subject_arg = CompiledExpr::value_ref(
        ValueCellId::new("subject", "self"),
        Type::StructureRef("Structure".to_string()),
    );
    let tol_arg = CompiledExpr::literal(
        Value::Scalar {
            si_value: output_tol,
            dimension: DimensionVector::LENGTH,
        },
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    let rep_within = CompiledExpr::user_function_call(
        "RepresentationWithin".to_string(),
        vec![subject_arg, tol_arg],
        Type::Bool,
    );
    TopologyTemplateBuilder::new("STEPOutput")
        .param("STEPOutput", "subject", Type::StructureRef("Structure".to_string()), None)
        .constraint("STEPOutput", 0, None, rep_within)
        .build()
}

/// Build a `manufacturing`-style `CompiledPurpose` whose sole constraint is
/// `RepresentationWithin(subject, purpose_tol m)`. Mirrors the helper in
/// `tests/tolerance_scope.rs::build_module_with_manufacturing_purpose`.
fn manufacturing_purpose(purpose_name: &str, purpose_tol: f64) -> reify_compiler::CompiledPurpose {
    let subject_arg = CompiledExpr::value_ref(
        ValueCellId::new("subject", "self"),
        Type::StructureRef("Bracket".to_string()),
    );
    let tol_arg = CompiledExpr::literal(
        Value::Scalar {
            si_value: purpose_tol,
            dimension: DimensionVector::LENGTH,
        },
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    let rep_within = CompiledExpr::user_function_call(
        "RepresentationWithin".to_string(),
        vec![subject_arg, tol_arg],
        Type::Bool,
    );
    CompiledPurposeBuilder::new(purpose_name)
        .param("subject", "Structure")
        .constraint("subject", 0, None, rep_within)
        .build()
}

/// Build a minimal `MyDesign` template with one Param cell. Carries no
/// RepresentationWithin of its own — the purpose's tolerance scope is
/// what binds to `MyDesign` when `manufacturing` is activated against it.
fn my_design_template() -> reify_compiler::TopologyTemplate {
    TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::Real, None)
        .build()
}

#[test]
fn engine_demanded_tolerance_for_output_combines_via_min_when_both_active() {
    // Two contributors, both Some, output tighter:
    //   STEPOutput template      → output_bound  = Some(1e-6)
    //   manufacturing @ MyDesign → purpose_bound = Some(50e-6)
    // combine_demanded_tolerance(Some(1e-6), Some(50e-6)) == Some(1e-6) —
    // tighter wins under partial-order "tighter satisfies looser" semantics
    // (same rule as `tolerance_bucket` `<=` and `tolerance_scope::merge_with_min`).
    let module = CompiledModuleBuilder::new(ModulePath::new(vec!["test".to_string()]))
        .template(step_output_template(1e-6))
        .template(my_design_template())
        .compiled_purpose(manufacturing_purpose("manufacturing", 50e-6))
        .build();

    let mut engine = make_engine();
    engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    assert_eq!(
        engine.demanded_tolerance_for_output("STEPOutput", "MyDesign"),
        Some(1e-6),
        "tighter output bound (1e-6) must win over looser purpose bound (50e-6) — \
         partial-order min combination"
    );
}
