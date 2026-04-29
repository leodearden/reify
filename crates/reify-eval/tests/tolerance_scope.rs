//! Engine-level integration tests for the tolerance-scope infrastructure
//! (per task 2647 / PRD `docs/prds/v0_2/per-purpose-tolerance.md`).
//!
//! Activates a hand-built `CompiledPurpose` whose constraint is the
//! recognised `RepresentationWithin(<bare-StructureRef>, <length-literal>)`
//! shape, then asserts the propagated tolerance scope is observable via
//! `Engine::active_tolerance_for`.

use reify_test_support::builders::{
    CompiledModuleBuilder, CompiledPurposeBuilder, TopologyTemplateBuilder,
};
use reify_test_support::make_engine;
use reify_types::{CompiledExpr, DimensionVector, ModulePath, Type, Value, ValueCellId};

/// Build a minimal CompiledModule with templates `MyDesign` (sub `head: Head`)
/// and `Head`, plus a `manufacturing` purpose whose sole constraint is
/// `RepresentationWithin(subject, 1e-6 m)`.
fn build_module_with_manufacturing_purpose(
    purpose_name: &str,
    si_tolerance: f64,
) -> reify_compiler::CompiledModule {
    // Template "Head": one Param cell on entity "Head".
    let head_template = TopologyTemplateBuilder::new("Head")
        .param("Head", "diameter", Type::Real, None)
        .build();

    // Template "MyDesign": one Param cell on entity "MyDesign" + sub "head" → Head.
    let my_design_template = TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::Real, None)
        .sub_component("head", "Head", Vec::new())
        .build();

    // Purpose: RepresentationWithin(subject, si_tolerance m). The subject arg
    // is a ValueRef typed StructureRef("Bracket") (the "bare-purpose-param"
    // shape recognised by `extract_tolerance_bindings` in
    // `crates/reify-eval/src/tolerance_scope.rs`). The literal arg is a
    // Scalar with LENGTH dimension.
    let subject_arg = CompiledExpr::value_ref(
        ValueCellId::new("subject", "self"),
        Type::StructureRef("Bracket".to_string()),
    );
    let tol_arg = CompiledExpr::literal(
        Value::Scalar {
            si_value: si_tolerance,
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
    let purpose = CompiledPurposeBuilder::new(purpose_name)
        .param("subject", "Structure")
        .constraint("subject", 0, None, rep_within)
        .build();

    CompiledModuleBuilder::new(ModulePath::new(vec!["test".to_string()]))
        .template(head_template)
        .template(my_design_template)
        .compiled_purpose(purpose)
        .build()
}

#[test]
fn engine_active_tolerance_for_returns_some_after_activate_purpose_with_representation_within() {
    let module = build_module_with_manufacturing_purpose("manufacturing", 1e-6);

    let mut engine = make_engine();
    engine.eval(&module);

    // Activate the purpose against the top-level entity ref ("MyDesign"),
    // matching the entity prefix the value cells were built under.
    engine.activate_purpose("manufacturing", "MyDesign");

    // (a) Subject root carries the tolerance.
    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        Some(1e-6),
        "subject root must carry the RepresentationWithin tolerance after activation"
    );
    // (b) Dotted descendant inherits via prefix-scan propagation.
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        Some(1e-6),
        "sub-component descendant must inherit the propagated tolerance"
    );
    // (c) An unrelated entity has no tolerance entry.
    assert_eq!(
        engine.active_tolerance_for("OtherEntity"),
        None,
        "entities outside the subject's prefix scan must NOT have a tolerance entry"
    );
}
