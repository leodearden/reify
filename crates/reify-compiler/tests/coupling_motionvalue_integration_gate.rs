//! Integration gate: Coupling<P>::MotionValue reduction over real stdlib types
//! (task #4605 ε — H two-way boundary leaf).
//!
//! PRD: docs/prds/type-args-and-assoc-type-projection.md §7 (ε two-way H boundary).
//!
//! Tests in source order:
//!   - step-1 RED: PRODUCER — Coupling<P>::MotionValue reduces to concrete types
//!   - step-1 RED: CONSUMER-CORRECT — Coupling<Prismatic> motion var in Length slot
//!   - step-1 RED: CONSUMER-MISMATCH — Coupling<Revolute> (Angle) into Length slot
//!   - step-4 RED: nondriving regression guard — let-bound coupling still triggers
//!                 E_MECHANISM_NONDRIVING_JOINT for bind()
//!
//! RED in step-1 because kinematic.ri has non-generic `Coupling : Joint {}` with
//! no HasMotion trait and no MotionValue assoc type — `Coupling<Prismatic>::MotionValue`
//! triggers a TypeArgArity error (Coupling takes 0 type args, not 1).
//!
//! GREEN after step-3 edits kinematic.ri to declare `trait HasMotion`,
//! add `+ HasMotion` / `type MotionValue = Length/Angle` to Prismatic/Revolute,
//! and make Coupling generic: `Coupling<P: DrivingJoint + HasMotion>`.

use reify_core::{DiagnosticCode, Type};
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ── Fixtures ──────────────────────────────────────────────────────────────────

const COUPLING_MOTIONVALUE_OK: &str =
    include_str!("fixtures/coupling_motionvalue_ok.ri");
const COUPLING_MOTIONVALUE_MISMATCH: &str =
    include_str!("fixtures/coupling_motionvalue_mismatch.ri");

// ── PRODUCER: Coupling<P>::MotionValue reduces to concrete types ────────────

/// Coupling<Prismatic>::MotionValue must reduce to Type::length() and
/// Coupling<Revolute>::MotionValue must reduce to Type::angle() — on REAL
/// stdlib types (not synthetic inline redeclarations).
///
/// Mirrors `applied_base_projection_reduces_to_concrete_type` in
/// `assoc_type_projection_reduction_tests.rs` but targets the real kinematic.ri
/// Coupling/Prismatic/Revolute declarations.
///
/// RED in step-1: kinematic.ri Coupling is non-generic, so `Coupling<Prismatic>`
/// triggers TypeArgArity and `::MotionValue` cannot resolve.
#[test]
fn producer_coupling_motionvalue_reduces_to_concrete_types() {
    let module = compile_source_with_stdlib(COUPLING_MOTIONVALUE_OK);
    let errors = errors_only(&module);

    assert!(
        errors.is_empty(),
        "CouplingProbe must compile without errors; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "CouplingProbe")
        .expect("CouplingProbe template should be compiled");

    let cell_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == member)
            .unwrap_or_else(|| panic!("value cell `{member}` should exist"))
            .cell_type
            .clone()
    };

    assert_eq!(
        cell_type("p"),
        Type::length(),
        "Coupling<Prismatic>::MotionValue must reduce to Type::length() (real stdlib); \
         got: {:?}",
        cell_type("p")
    );
    assert_eq!(
        cell_type("r"),
        Type::angle(),
        "Coupling<Revolute>::MotionValue must reduce to Type::angle() (real stdlib); \
         got: {:?}",
        cell_type("r")
    );
}

// ── CONSUMER-CORRECT: Coupling<Prismatic> motion var in Length slot ─────────

/// A Coupling<Prismatic> motion variable (Length) combined additively with a
/// Length param must compile without errors.
///
/// RED in step-1: same TypeArgArity/unresolved-projection block as above.
#[test]
fn consumer_correct_coupling_prismatic_in_length_slot() {
    let module = compile_source_with_stdlib(COUPLING_MOTIONVALUE_OK);
    let errors = errors_only(&module);

    assert!(
        errors.is_empty(),
        "CouplingConsumerOk (Length + Coupling<Prismatic>::MotionValue == Length + Length) \
         must compile without errors; got: {:?}",
        errors
    );
}

// ── CONSUMER-MISMATCH: Coupling<Revolute> (Angle) in Length slot ────────────

/// A Coupling<Revolute> motion variable (Angle) combined additively with a
/// Length param must produce EXACTLY ONE DimensionMismatch Error.
///
/// Anti-cascade:
///   - NO value cell has cell_type == Type::Error (params are cleanly typed).
///   - NONE of UnresolvedType / AmbiguousAssocType / TypeArgArity / TypeArgBound
///     appear (the projection resolves cleanly before the dimensional check fires).
///
/// RED in step-1: the projection fails before reaching the dimensional check;
/// the error count or codes differ from exactly-one-DimensionMismatch.
#[test]
fn consumer_mismatch_coupling_revolute_angle_in_length_slot() {
    let module = compile_source_with_stdlib(COUPLING_MOTIONVALUE_MISMATCH);
    let errors = errors_only(&module);

    // Exactly one error.
    assert_eq!(
        errors.len(),
        1,
        "CouplingConsumerMismatch must emit exactly 1 Error diagnostic \
         (Length + Angle => DimensionMismatch); got {} diagnostic(s): {:?}",
        errors.len(),
        errors
    );

    // That error is a DimensionMismatch.
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::DimensionMismatch),
        "the sole error must be DiagnosticCode::DimensionMismatch; got: {:?}",
        errors[0].code
    );

    // Anti-cascade: no value cell has cell_type == Type::Error.
    let mismatch_template = module
        .templates
        .iter()
        .find(|t| t.name == "CouplingConsumerMismatch")
        .expect("CouplingConsumerMismatch template should be compiled");

    let error_typed_cells: Vec<_> = mismatch_template
        .value_cells
        .iter()
        .filter(|vc| vc.cell_type == Type::Error)
        .collect();
    assert!(
        error_typed_cells.is_empty(),
        "no value cell should have cell_type == Type::Error (anti-cascade — \
         projection resolves cleanly to Type::angle() before the dimensional \
         check fires); error-typed cells: {:?}",
        error_typed_cells.iter().map(|vc| &vc.id.member).collect::<Vec<_>>()
    );

    // Anti-cascade: no noise codes.
    for code in &[
        DiagnosticCode::UnresolvedType,
        DiagnosticCode::AmbiguousAssocType,
        DiagnosticCode::TypeArgArity,
        DiagnosticCode::TypeArgBound,
    ] {
        assert!(
            !errors.iter().any(|d| d.code == Some(*code)),
            "must NOT emit {:?} (anti-cascade / no noise); all errors: {:?}",
            code,
            errors
        );
    }
}
