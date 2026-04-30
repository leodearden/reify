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

/// Core builder for an `STEPOutput`-shaped `TopologyTemplate`. Callers supply
/// the body `CompiledExpr`; the template name, `"subject"` param, and index-0
/// constraint slot are fixed — any future change to the template shape only
/// needs to be made here.
fn step_output_template_with_body(body: CompiledExpr) -> reify_compiler::TopologyTemplate {
    TopologyTemplateBuilder::new("STEPOutput")
        .param("STEPOutput", "subject", Type::StructureRef("Structure".to_string()), None)
        .constraint("STEPOutput", 0, None, body)
        .build()
}

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
    let body = CompiledExpr::user_function_call(
        "RepresentationWithin".to_string(),
        vec![subject_arg, tol_arg],
        Type::Bool,
    );
    step_output_template_with_body(body)
}

/// Build an `STEPOutput`-shaped `TopologyTemplate` whose body constraint is a
/// `Bool` literal rather than a `RepresentationWithin` expression. The
/// constraint is present in the runtime graph under
/// `(entity = "STEPOutput", index = 0)` but carries no tolerance value, so
/// `extract_output_tolerance_bound` returns `None` for this template.
fn step_output_template_without_rep_within() -> reify_compiler::TopologyTemplate {
    step_output_template_with_body(CompiledExpr::literal(Value::Bool(true), Type::Bool))
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
    // Four partial-input scenarios pin the engine wrapper's correct delegation
    // to `combine_demanded_tolerance`'s None-handling for both directions and
    // the both-None case (corresponds to the lone-Some / both-None rows of the
    // combiner truth table — see step-4 for the unit-level contract).
    // Scenarios (b) and (d) both exercise the `None + Some(p) = Some(p)` row
    // via distinct shapes: (b) has no STEPOutput template at all; (d) has the
    // template but its body carries no recognisable tolerance expression.

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

    // (d) Template-present, no RepresentationWithin body — module contains a
    //     STEPOutput template whose body constraint is a `Bool` literal (not a
    //     `RepresentationWithin` expression), plus a manufacturing purpose. The
    //     template IS present in the runtime graph but carries no tolerance
    //     value, so output_bound is None. Purpose-side contributes Some(50e-6).
    //
    //     Contrasts with (b): (b) has no STEPOutput template at all; (d) has
    //     the template but its body is not a tolerance expression. Both produce
    //     output_bound = None. (d) is the more realistic shape because real
    //     STEPOutput templates carry body constraints that aren't always
    //     tolerance bounds.
    {
        let module = CompiledModuleBuilder::new(ModulePath::new(vec![
            "test_template_present_no_rep_within".to_string(),
        ]))
        .template(step_output_template_without_rep_within())
        .template(my_design_template())
        .compiled_purpose(manufacturing_purpose("manufacturing", 50e-6))
        .build();
        let mut engine = make_engine();
        engine.eval(&module);
        engine.activate_purpose("manufacturing", "MyDesign");
        assert_eq!(
            engine.demanded_tolerance_for_output("STEPOutput", "MyDesign"),
            Some(50e-6),
            "template-present no RepresentationWithin: STEPOutput template \
             present but body carries no tolerance value → output_bound is \
             None → purpose-side (50e-6) wins"
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
