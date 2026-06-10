//! Two-way boundary tests for the supertrait `trait ElasticMaterial : ConstitutiveLaw`
//! (task γ / task-4441, PRD §7.2 "the H").
//!
//! Mirrors the conformance-test pattern from `fn_arg_trait_conformance_tests.rs`:
//! * filter diagnostics by `DiagnosticCode::TypeNotConformingToTrait`
//! * wrap each call in a `structure NAME { let r = ... }` entity body so the
//!   ctor arg acquires a `StructureRef` result_type via the entity-scope template
//!   registry (required for the conformance walk; see that file's module doc).
//!
//! ## Why `compile_source_with_stdlib`
//!
//! All fixtures reference stdlib symbols — `Steel_AISI_1045`, `PointLoad`,
//! `FixedSupport`, `ElasticOptions`, `solve_elastic_static`, `solve_buckling`,
//! `BucklingOptions` — so a full stdlib prelude is required.
//! `compile_source_with_stdlib` also transitively calls `load_stdlib()`, which
//! panics on any stdlib Error-severity diagnostic; the step-2 relocate therefore
//! gets a built-in stdlib-compiles safety net for free.
//!
//! ## Test inventory
//!
//! (a) `solve_elastic_static_accepts_material_directly` — **RED before step-2**,
//!     GREEN after. Positive probe: `Steel_AISI_1045()` passed directly to the
//!     `material : ConstitutiveLaw` param must produce zero `TypeNotConformingToTrait`
//!     diagnostics and zero Error-severity diagnostics overall.
//!
//! (b) `solve_buckling_still_compiles_with_direct_material` — GREEN before & after.
//!     Preserve guard: `solve_buckling`'s `material : ElasticMaterial` param must
//!     continue to accept `Steel_AISI_1045()` without any `TypeNotConformingToTrait`
//!     diagnostic — the supertrait must not perturb the sibling param.
//!
//! (c) `box_at_material_slot_still_errors` — GREEN before & after.
//!     Over-widening guard: `box(...)` passed to the `material : ConstitutiveLaw`
//!     param must still emit at least one `TypeNotConformingToTrait` mentioning
//!     "ConstitutiveLaw" — the supertrait must not widen conformance to accept
//!     non-material values.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

/// (a) POSITIVE — RED until step-2.
///
/// `Steel_AISI_1045()` passed directly to `solve_elastic_static`'s
/// `material : ConstitutiveLaw` param must compile clean once
/// `trait ElasticMaterial : ConstitutiveLaw` is in place:
///   * zero `TypeNotConformingToTrait` diagnostics
///   * zero Error-severity diagnostics (compiles clean)
///
/// Before step-2 this test is RED because `Steel_AISI_1045` has compile-time
/// type `StructureRef("Steel_AISI_1045")`, which does not yet transitively
/// satisfy `ConstitutiveLaw` — the post-pass emits
/// "type 'Steel_AISI_1045' does not conform to trait 'ConstitutiveLaw'".
#[test]
fn solve_elastic_static_accepts_material_directly() {
    let source = r#"
structure SmokeDirect {
    let r = solve_elastic_static(
        Steel_AISI_1045(),
        1000mm,
        100mm,
        100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")],
        ElasticOptions()
    )
}
"#;
    let module = compile_source_with_stdlib(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "expected zero TypeNotConformingToTrait diagnostics for direct Steel_AISI_1045() \
         pass to solve_elastic_static; got {}: {:?}",
        conformance_errors.len(),
        conformance_errors
    );

    let error_diagnostics: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        error_diagnostics.is_empty(),
        "expected zero Error-severity diagnostics for direct Steel_AISI_1045() call; \
         got {}: {:?}",
        error_diagnostics.len(),
        error_diagnostics
    );
}

/// (b) PRESERVE guard — GREEN before & after step-2.
///
/// `solve_buckling`'s `material : ElasticMaterial` param must continue to
/// accept `Steel_AISI_1045()` with zero `TypeNotConformingToTrait` diagnostics.
/// Guards against the supertrait `ElasticMaterial : ConstitutiveLaw` accidentally
/// perturbing the `ElasticMaterial` param on the sibling solver.
#[test]
fn solve_buckling_still_compiles_with_direct_material() {
    let source = r#"
structure BucklingPreserve {
    let r = solve_buckling(
        Steel_AISI_1045(),
        1000mm,
        100mm,
        100mm,
        [1.0],
        [1.0],
        BucklingOptions()
    )
}
"#;
    let module = compile_source_with_stdlib(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "expected zero TypeNotConformingToTrait diagnostics for solve_buckling with \
         Steel_AISI_1045(); the supertrait must not perturb the ElasticMaterial param. \
         Got {}: {:?}",
        conformance_errors.len(),
        conformance_errors
    );
}

/// (c) NEGATIVE / over-widening guard — GREEN before & after step-2.
///
/// `box(...)` at the `material : ConstitutiveLaw` slot must still emit at
/// least one `TypeNotConformingToTrait` mentioning "ConstitutiveLaw".
/// `box(1000mm, 100mm, 100mm)` compiles to `Type::Scalar{DIMENSIONLESS}`, not
/// a structure ref, so it cannot conform to `ConstitutiveLaw`. Guards against
/// the supertrait widening conformance to accept non-material values.
#[test]
fn box_at_material_slot_still_errors() {
    let source = r#"
structure NonConforming {
    let r = solve_elastic_static(
        box(1000mm, 100mm, 100mm),
        1000mm,
        100mm,
        100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")],
        ElasticOptions()
    )
}
"#;
    let module = compile_source_with_stdlib(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        !conformance_errors.is_empty(),
        "expected at least one TypeNotConformingToTrait for box(...) at material slot; \
         the supertrait must not widen conformance to non-material values. \
         Got 0 diagnostics; all diagnostics: {:?}",
        module.diagnostics
    );

    let mentions_constitutive_law = conformance_errors
        .iter()
        .any(|d| d.message.contains("ConstitutiveLaw"));

    assert!(
        mentions_constitutive_law,
        "at least one TypeNotConformingToTrait diagnostic should mention 'ConstitutiveLaw'; \
         got: {:?}",
        conformance_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
