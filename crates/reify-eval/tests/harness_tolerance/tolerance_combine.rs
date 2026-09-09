//! Engine-level integration tests for the output-occurrence × active-purpose
//! tolerance combiner (per task 2650 / PRD
//! `docs/prds/v0_2/per-purpose-tolerance.md`).
//!
//! Activates a hand-built `manufacturing` purpose whose constraint is the
//! recognised `RepresentationWithin(<bare-StructureRef>, <length-literal>)`
//! shape, plus a hand-built `STEPOutput` template carrying its own
//! `RepresentationWithin` body constraint, then asserts the combined
//! demanded tolerance is observable via `Engine::demanded_tolerance_for_output`.

use reify_core::ModulePath;
use reify_test_support::builders::CompiledModuleBuilder;
use reify_test_support::{
    make_engine, manufacturing_purpose, my_design_template, step_output_template,
    step_output_template_without_rep_within,
};

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
        let module =
            CompiledModuleBuilder::new(ModulePath::new(vec!["test_output_only".to_string()]))
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
        let module =
            CompiledModuleBuilder::new(ModulePath::new(vec!["test_purpose_only".to_string()]))
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
