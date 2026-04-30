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

#[test]
fn engine_active_tolerance_for_drops_after_deactivate_purpose() {
    let module = build_module_with_manufacturing_purpose("manufacturing", 1e-6);

    let mut engine = make_engine();
    engine.eval(&module);

    // Activate, then verify the tolerance is set (precondition — if this
    // fires the test failure is upstream of deactivate, not in it).
    engine.activate_purpose("manufacturing", "MyDesign");
    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        Some(1e-6),
        "precondition: subject root must carry tolerance after activation"
    );
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        Some(1e-6),
        "precondition: sub-component descendant must inherit tolerance after activation"
    );

    // Deactivate; both root and descendant tolerance entries must be cleared.
    engine.deactivate_purpose("manufacturing");

    // (a) Subject root no longer carries the tolerance.
    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        None,
        "subject root must lose its tolerance entry after deactivation"
    );
    // (b) Dotted descendant likewise loses its inherited entry.
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        None,
        "sub-component descendant must lose its inherited tolerance after deactivation"
    );

    // Re-activate: the recompute must be idempotent — the tolerance
    // reappears at both the root and the descendant.
    engine.activate_purpose("manufacturing", "MyDesign");
    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        Some(1e-6),
        "re-activation must restore the tolerance at the subject root \
         (recompute_tolerance_scope idempotency)"
    );
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        Some(1e-6),
        "re-activation must restore the inherited tolerance at the descendant"
    );
}

/// Build a module with templates `MyDesign` (subs `head: Head` and
/// `tail: Head` — a sibling pair so we can witness propagation reaching
/// the non-tightened descendant) plus two purposes:
///   - `loose` whose constraint is `RepresentationWithin(subject, loose_tol m)`,
///   - `tight` whose constraint is `RepresentationWithin(subject, tight_tol m)`.
///
/// Both purposes carry the bare-StructureRef-typed `subject` parameter
/// recognised by `extract_tolerance_bindings`.
fn build_module_with_overlapping_purposes(
    loose_name: &str,
    loose_tol: f64,
    tight_name: &str,
    tight_tol: f64,
) -> reify_compiler::CompiledModule {
    let head_template = TopologyTemplateBuilder::new("Head")
        .param("Head", "diameter", Type::Real, None)
        .build();
    // Two siblings so `loose`'s descendant propagation reaches BOTH `head`
    // and `tail`, while `tight` overrides only `head`.
    let my_design_template = TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::Real, None)
        .sub_component("head", "Head", Vec::new())
        .sub_component("tail", "Head", Vec::new())
        .build();

    let make_purpose = |name: &str, kind: &str, si_value: f64| {
        let subject_arg = CompiledExpr::value_ref(
            ValueCellId::new("subject", "self"),
            Type::StructureRef(kind.to_string()),
        );
        let tol_arg = CompiledExpr::literal(
            Value::Scalar {
                si_value,
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
        CompiledPurposeBuilder::new(name)
            .param("subject", "Structure")
            .constraint("subject", 0, None, rep_within)
            .build()
    };
    let loose = make_purpose(loose_name, "Bracket", loose_tol);
    let tight = make_purpose(tight_name, "Head", tight_tol);

    CompiledModuleBuilder::new(ModulePath::new(vec!["test".to_string()]))
        .template(head_template)
        .template(my_design_template)
        .compiled_purpose(loose)
        .compiled_purpose(tight)
        .build()
}

#[test]
fn engine_active_tolerance_for_takes_minimum_across_overlapping_purposes() {
    // Two purposes whose subject prefix-scans overlap on `MyDesign.head`:
    //   loose @ MyDesign       → reaches MyDesign, MyDesign.head, MyDesign.tail
    //   tight @ MyDesign.head  → reaches only MyDesign.head
    // Per the partial-order semantics (tighter satisfies looser),
    // `MyDesign.head` must report the minimum (1e-6), but its sibling
    // `MyDesign.tail` retains the looser 50e-6 contributed by `loose`.
    let module = build_module_with_overlapping_purposes("loose", 50e-6, "tight", 1e-6);

    let mut engine = make_engine();
    engine.eval(&module);

    engine.activate_purpose("loose", "MyDesign");
    engine.activate_purpose("tight", "MyDesign.head");

    // (a) Root carries only the loose contribution. `tight`'s subject is
    //     `MyDesign.head` and its prefix-scan must NOT match `MyDesign`
    //     (the dot-boundary check in propagate_subject_to_descendants).
    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        Some(50e-6),
        "root entity must see only the loose contributor; \
         tight's subject is MyDesign.head and does not propagate up"
    );
    // (b) Overlapped descendant: tighter wins (`min(50e-6, 1e-6) == 1e-6`).
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        Some(1e-6),
        "overlapping descendant must take min across contributors; \
         tighter (1e-6) satisfies looser (50e-6)"
    );
    // (c) Sibling descendant of MyDesign sees ONLY loose (50e-6) — `tight`'s
    //     prefix-scan is rooted at `MyDesign.head` and does not reach
    //     `MyDesign.tail`.
    assert_eq!(
        engine.active_tolerance_for("MyDesign.tail"),
        Some(50e-6),
        "non-overlapped sibling descendant must retain loose (50e-6); \
         tight's prefix-scan does not reach a sibling sub-entity"
    );

    // Deactivate `tight`: the descendant relaxes back to `loose`'s 50e-6
    // (the lone surviving contributor), confirming full recompute correctly
    // sheds the tightening contribution.
    engine.deactivate_purpose("tight");
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        Some(50e-6),
        "deactivating `tight` must re-relax MyDesign.head to the loose \
         contributor's 50e-6 (which still propagates from MyDesign)"
    );
    // Sibling unchanged.
    assert_eq!(
        engine.active_tolerance_for("MyDesign.tail"),
        Some(50e-6),
        "sibling descendant must remain at loose's 50e-6 across the \
         deactivation"
    );
    // Root unchanged.
    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        Some(50e-6),
        "root must remain at loose's 50e-6 across the deactivation"
    );
}
