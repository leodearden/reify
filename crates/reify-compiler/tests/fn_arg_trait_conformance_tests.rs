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
