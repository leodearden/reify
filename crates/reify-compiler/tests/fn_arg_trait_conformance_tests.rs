//! Integration tests for function-call-argument trait conformance (task-4081).
//!
//! Tests the end-to-end path: entity body `let bad = couple(FixedThing())` should
//! emit a `TypeNotConformingToTrait` error.  The module under test is
//! `phase_fn_arg_conformance` (wired into lib.rs after
//! `phase_pending_bound_checks`), which walks compiled exprs and calls
//! `check_fn_arg_conformance` for every `UserFunctionCall` whose params carry a
//! trait object.
//!
//! ## Why `compile_source` rather than `compile_source_with_stdlib`
//!
//! The source is self-contained (defines its own trait, structures, function, and
//! entity) so there is no need for the stdlib prelude.  Using `compile_source`
//! keeps the test fast and the dependency minimal.
//!
//! ## Why entity body, not free-function body
//!
//! SIR-α lowers `Foo()` to `StructureInstanceCtor` with `result_type =
//! StructureRef("Foo")` only when the template registry contains `Foo`.  That
//! registry is set in ENTITY scope (`entity.rs:401`, includes local templates)
//! but not in FUNCTION scope (functions compile before entities).  The integration
//! tests therefore drive conformance from an entity `let` body where the ctor arg
//! acquires its `StructureRef` type.

use reify_core::DiagnosticCode;
use reify_test_support::compile_source;

/// Common source preamble: trait, two structures, one fn.
fn preamble() -> &'static str {
    r#"
trait DrivingJoint {}

structure RevoluteJoint : DrivingJoint {
    param x : Real = 0.0
}

structure FixedThing {
    param y : Real = 0.0
}

fn couple(joint : DrivingJoint) -> Real {
    0.0
}
"#
}

/// Step-5 primary: calling `couple(FixedThing())` in an entity `let` binding
/// emits a `TypeNotConformingToTrait` error mentioning `FixedThing` and
/// `DrivingJoint`.
///
/// RED until step-6: step-2 makes the call resolve to a `UserFunctionCall` but
/// no conformance post-pass runs yet, so no `TypeNotConformingToTrait` is present.
#[test]
fn entity_let_bad_arg_emits_type_not_conforming_to_trait() {
    let source = format!(
        r#"{}
structure Test {{
    let bad = couple(FixedThing())
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly 1 TypeNotConformingToTrait diagnostic, got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
    let msg = &conformance_errors[0].message;
    assert!(
        msg.contains("FixedThing"),
        "diagnostic should mention 'FixedThing'; got: {}",
        msg
    );
    assert!(
        msg.contains("DrivingJoint"),
        "diagnostic should mention 'DrivingJoint'; got: {}",
        msg
    );
}

// ── Step-7: constraint, wrapper-param, positive, and eval-builtin cases ─────

/// (a) A non-conforming call inside an entity CONSTRAINT emits TypeNotConformingToTrait.
///
/// RED until step-8: step-6 only walks value-cell default_exprs; constraint
/// exprs are not yet walked.
#[test]
fn entity_constraint_bad_arg_emits_type_not_conforming_to_trait() {
    let source = format!(
        r#"{}
structure T2 {{
    constraint couple(FixedThing()) > 0.0
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected 1 TypeNotConformingToTrait in constraint, got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
}

/// (b) Option<DrivingJoint> param called with some(FixedThing()) emits a conformance error.
///
/// RED until step-8: step-6 value-cell walk won't trigger Option wrapper
/// recursion into a struct built via couple_opt(some(FixedThing())).
/// Actually step-6 does walk value-cells — this MAY already pass. The plan
/// says RED because constraints are not yet walked; but the wrapper test is
/// for a value-cell. Let's keep it as a wrapper regression guard.
///
/// The wrapper param exercises `walk_param_against_arg`'s `Option` arm.
#[test]
fn option_wrapped_trait_param_bad_arg_emits_conformance_error() {
    let source = format!(
        r#"{}
fn couple_opt(joint : Option<DrivingJoint>) -> Real {{
    0.0
}}

structure T3 {{
    let bad = couple_opt(some(FixedThing()))
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected 1 TypeNotConformingToTrait for Option<DrivingJoint> param with FixedThing arg, \
         got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
}

/// (c) A conforming call produces NO TypeNotConformingToTrait error.
///
/// Positive guard: this must hold before and after step-8.
#[test]
fn entity_let_good_arg_emits_no_conformance_error() {
    let source = format!(
        r#"{}
structure TOk {{
    let ok = couple(RevoluteJoint())
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "conforming call must produce no TypeNotConformingToTrait errors; got: {:?}",
        conformance_errors
    );
}

// ── Step-9: overload-collapse robustness ─────────────────────────────────────

/// Two same-name overloads (trait-first, concrete-second) — the name-keyed
/// HashMap in `phase_fn_arg_conformance` collapses both to the last-inserted
/// `couple(Real)` entry, so the conformance check is skipped and the
/// non-conforming call is missed.
///
/// This test is deterministically RED on the step-8 implementation because:
/// - `merge_prelude_functions` preserves declaration order, so
///   `resolution_functions == [couple(DrivingJoint), couple(Real), ...]`.
/// - A name-keyed `HashMap` collapses both to the LAST-inserted `couple(Real)`.
/// - `Real` is not trait-carrying → conformance check is skipped → error missed
///   (got 0, expected 1).
///
/// RED until step-10.
#[test]
fn overload_collapse_bad_arg_emits_type_not_conforming_to_trait() {
    let source = r#"
trait DrivingJoint {}

structure RevoluteJoint : DrivingJoint {
    param x : Real = 0.0
}

structure FixedThing {
    param y : Real = 0.0
}

fn couple(joint : DrivingJoint) -> Real { 0.0 }
fn couple(n : Real) -> Real { n }

structure Test {
    let bad = couple(FixedThing())
}

structure TOk {
    let ok = couple(RevoluteJoint())
}
"#;
    let module = compile_source(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly 1 TypeNotConformingToTrait diagnostic for couple(FixedThing()), \
         got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
    let msg = &conformance_errors[0].message;
    assert!(
        msg.contains("FixedThing"),
        "diagnostic should mention 'FixedThing'; got: {}",
        msg
    );
    assert!(
        msg.contains("DrivingJoint"),
        "diagnostic should mention 'DrivingJoint'; got: {}",
        msg
    );
}

/// Companion positive guard for the overload-collapse scenario:
/// `couple(RevoluteJoint())` in `TOk` must NOT produce any
/// `TypeNotConformingToTrait` diagnostic.
///
/// This uses the same two-overload source as the RED test above but asserts
/// the conforming entity is clean.
#[test]
fn overload_collapse_good_arg_emits_no_conformance_error() {
    let source = r#"
trait DrivingJoint {}

structure RevoluteJoint : DrivingJoint {
    param x : Real = 0.0
}

structure FixedThing {
    param y : Real = 0.0
}

fn couple(joint : DrivingJoint) -> Real { 0.0 }
fn couple(n : Real) -> Real { n }

structure TOk {
    let ok = couple(RevoluteJoint())
}
"#;
    let module = compile_source(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "conforming overloaded call must produce no TypeNotConformingToTrait errors; got: {:?}",
        conformance_errors
    );
}
