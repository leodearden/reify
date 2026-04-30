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
///
/// The `subject_arg`'s `result_type` uses the param's declared structure-ref
/// name (`"Structure"`) so the fixture stays robust if a future hardening of
/// `tolerance_scope`'s recognition gates asserts inner-name match against the
/// declared param type. Today's matcher only checks the outer
/// `StructureRef(_)` tag, so the inner string is informational; aligning it
/// with the declared param insulates the test from that future tightening.
fn manufacturing_purpose(purpose_name: &str, purpose_tol: f64) -> reify_compiler::CompiledPurpose {
    let subject_arg = CompiledExpr::value_ref(
        ValueCellId::new("subject", "self"),
        Type::StructureRef("Structure".to_string()),
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
fn engine_demanded_tolerance_for_output_handles_partial_inputs() {
    // Three partial-input scenarios pin the engine wrapper's correct
    // delegation to `combine_demanded_tolerance`'s None-handling for both
    // directions and the both-None case (corresponds to the lone-Some / both-
    // None rows of the combiner truth table — see step-4 for the unit-level
    // contract).

    // (a) Output-only — module contains only the STEPOutput template
    //     (no purpose with RepresentationWithin). Evaluate without
    //     activating any purpose → only the output bound contributes.
    {
        let module = CompiledModuleBuilder::new(ModulePath::new(vec!["test_output_only".to_string()]))
            .template(step_output_template(1e-6))
            .template(my_design_template())
            .build();
        let mut engine = make_engine();
        engine.eval(&module);
        // No `activate_purpose` call — purpose-side contributes None.
        assert_eq!(
            engine.demanded_tolerance_for_output("STEPOutput", "MyDesign"),
            Some(1e-6),
            "output-only: lone output bound (Some(1e-6)) must pass through \
             when purpose-side is None"
        );
    }

    // (b) Purpose-only — module contains only the manufacturing purpose
    //     and a MyDesign template. NO STEPOutput template at all, so the
    //     graph holds no STEPOutput-entity constraints → output_bound is None.
    //     Activate purpose → only the purpose bound contributes.
    {
        let module = CompiledModuleBuilder::new(ModulePath::new(vec!["test_purpose_only".to_string()]))
            .template(my_design_template())
            .compiled_purpose(manufacturing_purpose("manufacturing", 50e-6))
            .build();
        let mut engine = make_engine();
        engine.eval(&module);
        engine.activate_purpose("manufacturing", "MyDesign");
        assert_eq!(
            engine.demanded_tolerance_for_output("STEPOutput", "MyDesign"),
            Some(50e-6),
            "purpose-only: lone purpose bound (Some(50e-6)) must pass through \
             when output-side is None — no STEPOutput template ⇒ no graph \
             constraint under that entity"
        );
    }

    // (c) Neither — module with no RepresentationWithin anywhere, evaluated
    //     without any purpose activation. Both contributors are None →
    //     query returns None.
    {
        let module = CompiledModuleBuilder::new(ModulePath::new(vec!["test_neither".to_string()]))
            .template(my_design_template())
            .build();
        let mut engine = make_engine();
        engine.eval(&module);
        // No `activate_purpose` call — both contributors are None.
        assert_eq!(
            engine.demanded_tolerance_for_output("STEPOutput", "MyDesign"),
            None,
            "neither: both contributors None ⇒ result must be None — no \
             demand contributor exists"
        );
    }
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
