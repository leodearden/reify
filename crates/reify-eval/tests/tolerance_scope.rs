//! Engine-level integration tests for the tolerance-scope infrastructure
//! (per task 2647 / PRD `docs/prds/v0_2/per-purpose-tolerance.md`).
//!
//! Activates a hand-built `CompiledPurpose` whose constraint is the
//! recognised `RepresentationWithin(<bare-StructureRef>, <length-literal>)`
//! shape, then asserts the propagated tolerance scope is observable via
//! `Engine::active_tolerance_for`.

use reify_core::{DimensionVector, ModulePath, Type, ValueCellId};
use reify_ir::{CompiledExpr, Value};
use reify_test_support::builders::{
    CompiledModuleBuilder, CompiledPurposeBuilder, TopologyTemplateBuilder,
};
use reify_test_support::{
    make_engine, manufacturing_purpose_with_inner_name, my_design_template_with_subs,
};

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
    let my_design_template = my_design_template_with_subs(&[("head", "Head")]);

    // Purpose: RepresentationWithin(subject, si_tolerance m). The subject arg
    // is a ValueRef typed StructureRef("Bracket") (the "bare-purpose-param"
    // shape recognised by `extract_tolerance_bindings` in
    // `crates/reify-eval/src/tolerance_scope.rs`).
    let purpose = manufacturing_purpose_with_inner_name(purpose_name, "Bracket", si_tolerance);

    CompiledModuleBuilder::new(ModulePath::new(vec!["test".to_string()]))
        .template(head_template)
        .template(my_design_template)
        .compiled_purpose(purpose)
        .build()
}

/// Build a `RepresentationWithin(<ValueRef typed StructureRef(inner_kind)>,
/// <Scalar LENGTH literal>)` expression whose subject `ValueRef` entity is
/// `param_name`, matching the "bare-purpose-param" contract used by
/// `extract_tolerance_bindings`. Mirrors the unit-test helper at
/// `src/tolerance_scope.rs` but parameterises the subject entity name.
fn rep_within(param_name: &str, inner_kind: &str, tol: f64) -> CompiledExpr {
    let subject_arg = CompiledExpr::value_ref(
        ValueCellId::new(param_name, "self"),
        Type::StructureRef(inner_kind.to_string()),
    );
    let tol_arg = CompiledExpr::literal(
        Value::Scalar {
            si_value: tol,
            dimension: DimensionVector::LENGTH,
        },
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    CompiledExpr::user_function_call(
        "RepresentationWithin".to_string(),
        vec![subject_arg, tol_arg],
        Type::Bool,
    )
}

/// Build a 2-param purpose `fits` whose constraints are:
///   - `RepresentationWithin(part.self : StructureRef("Bracket"), tol_part m)`
///   - `RepresentationWithin(envelope.self : StructureRef("Envelope"), tol_env m)`
///
/// Both param entities ("part", "envelope") appear in `purpose.params`, so
/// the `extract_tolerance_bindings` membership gate accepts them. Used by
/// the multi-param engine tests (steps 3 and 4).
fn two_param_fits_purpose(
    name: &str,
    tol_part: f64,
    tol_env: f64,
) -> reify_compiler::CompiledPurpose {
    CompiledPurposeBuilder::new(name)
        .param("part", "Structure")
        .param("envelope", "Structure")
        .constraint("part", 0, None, rep_within("part", "Bracket", tol_part))
        .constraint(
            "envelope",
            1,
            None,
            rep_within("envelope", "Envelope", tol_env),
        )
        .build()
}

/// Build a module with:
///   - template `Head`: one `diameter : Real` param
///   - template `MyDesign`: `thickness : Real` param + subs `head: Head`, `tail: Head`
///   - a 2-param `fits` purpose (via `two_param_fits_purpose`)
///
/// Post-eval, entities `MyDesign.head` and `MyDesign.tail` exist and can each
/// be bound to a distinct purpose param by `activate_purpose_with_bindings`.
fn build_module_with_two_param_purpose(
    purpose_name: &str,
    tol_part: f64,
    tol_env: f64,
) -> reify_compiler::CompiledModule {
    let head_template = TopologyTemplateBuilder::new("Head")
        .param("Head", "diameter", Type::Real, None)
        .build();
    let my_design_template = my_design_template_with_subs(&[("head", "Head"), ("tail", "Head")]);
    let purpose = two_param_fits_purpose(purpose_name, tol_part, tol_env);
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
    let my_design_template = my_design_template_with_subs(&[("head", "Head"), ("tail", "Head")]);

    let loose = manufacturing_purpose_with_inner_name(loose_name, "Bracket", loose_tol);
    let tight = manufacturing_purpose_with_inner_name(tight_name, "Head", tight_tol);

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

/// A 2-param purpose activated via `activate_purpose_with_bindings` must
/// propagate each param's tolerance to its own bound entity (not collapsed).
///
/// After `activate_purpose_with_bindings("fits", &[(part, MyDesign.head),
/// (envelope, MyDesign.tail)])`:
///   - `active_tolerance_for("MyDesign.head")` == Some(1e-6)  (part's tol)
///   - `active_tolerance_for("MyDesign.tail")` == Some(5e-6)  (envelope's tol)
///   - `active_tolerance_for("MyDesign")`      == None  (root outside either subtree)
///
/// RED today: the `if bindings.len() == 1` guard in `activate_purpose_constraints_
/// with_bindings_inner` records nothing for multi-param activations, so
/// `recompute_tolerance_scope` produces an empty scope.
#[test]
fn multi_param_purpose_contributes_per_param_tolerance_scope() {
    let module = build_module_with_two_param_purpose("fits", 1e-6, 5e-6);

    let mut engine = make_engine();
    engine.eval(&module);

    engine
        .activate_purpose_with_bindings(
            "fits",
            &[
                ("part".to_string(), "MyDesign.head".to_string()),
                ("envelope".to_string(), "MyDesign.tail".to_string()),
            ],
        )
        .expect("activate_purpose_with_bindings must succeed for valid 2-param purpose");

    // (a) part param's tolerance at its bound entity.
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        Some(1e-6),
        "part param (bound to MyDesign.head) must contribute 1e-6 tolerance there"
    );
    // (b) envelope param's tolerance at its bound entity.
    assert_eq!(
        engine.active_tolerance_for("MyDesign.tail"),
        Some(5e-6),
        "envelope param (bound to MyDesign.tail) must contribute 5e-6 tolerance there"
    );
    // (c) root is outside both subtrees — no tolerance contributed by either param.
    assert_eq!(
        engine.active_tolerance_for("MyDesign"),
        None,
        "root entity (MyDesign) is not a descendant of either bound entity; \
         neither param's prefix-scan reaches it"
    );
}

/// A 2-param purpose activated via `activate_purpose_with_bindings` must
/// survive a second `eval(&module)` call: `is_purpose_active` stays true and
/// both per-param tolerances persist.
///
/// Pre-condition asserts verify the purpose is active and tolerances are set
/// before the re-eval so that a failure isolates the round-trip path.
///
/// RED today: eval()'s `mem::take` sees an empty `active_purpose_bindings` for
/// the multi-param purpose (nothing recorded due to the `len==1` guard), so
/// the preserved_bindings list is empty and the purpose is dropped on re-eval.
/// Even if recording were fixed, the re-apply loop calls
/// `activate_purpose_constraints(name, entity)` which REFUSES purposes with
/// `params.len() != 1` and would silently drop the purpose.
#[test]
fn multi_param_purpose_survives_eval_round_trip() {
    let module = build_module_with_two_param_purpose("fits", 1e-6, 5e-6);

    let mut engine = make_engine();
    engine.eval(&module);

    engine
        .activate_purpose_with_bindings(
            "fits",
            &[
                ("part".to_string(), "MyDesign.head".to_string()),
                ("envelope".to_string(), "MyDesign.tail".to_string()),
            ],
        )
        .expect("activate_purpose_with_bindings must succeed for valid 2-param purpose");

    // Precondition: purpose active and tolerances set before the re-eval.
    assert!(
        engine.is_purpose_active("fits"),
        "precondition: fits must be active before round-trip"
    );
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        Some(1e-6),
        "precondition: MyDesign.head must carry 1e-6 before round-trip"
    );
    assert_eq!(
        engine.active_tolerance_for("MyDesign.tail"),
        Some(5e-6),
        "precondition: MyDesign.tail must carry 5e-6 before round-trip"
    );

    // Re-eval: purpose must survive and tolerances must persist.
    engine.eval(&module);

    assert!(
        engine.is_purpose_active("fits"),
        "fits must remain active after eval() round-trip"
    );
    assert_eq!(
        engine.active_tolerance_for("MyDesign.head"),
        Some(1e-6),
        "MyDesign.head must retain 1e-6 after eval() round-trip"
    );
    assert_eq!(
        engine.active_tolerance_for("MyDesign.tail"),
        Some(5e-6),
        "MyDesign.tail must retain 5e-6 after eval() round-trip"
    );
}
