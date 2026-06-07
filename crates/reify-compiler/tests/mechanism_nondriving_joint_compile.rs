//! Compile-time E_MECHANISM_NONDRIVING_JOINT tests (task 4310 — mechanism γ).
//!
//! Verifies the L2 compile-time guard: `phase_fn_arg_conformance` (wired in
//! `entities_phase.rs`) rejects `bind(couple(...), v)` with a
//! `DiagnosticCode::MechanismNonDrivingJoint` naming the offending type, and
//! accepts `bind(prismatic(...), v)` without that diagnostic.
//!
//! ## Why `compile_source_with_stdlib`
//!
//! `bind`, `couple`, `prismatic`, and `fixed` are eval-builtins whose
//! `FunctionCall` IR nodes only appear when the stdlib prelude is active.
//! Using `compile_source_with_stdlib` provides the full kinematic stdlib
//! (trait Joint / DrivingJoint, structure defs for Coupling / Prismatic /
//! Fixed, etc.) so `check_mechanism_joint_bound` can look up trait_bounds
//! in the template_registry.
//!
//! ## Diagnostic filter discipline
//!
//! Each test filters strictly by `DiagnosticCode::MechanismNonDrivingJoint`
//! so incidental diagnostics (e.g. arity or type-mismatch errors on the
//! builtin args, which are not the subject of these tests) do not affect
//! the count.
//!
//! ## RED until step-6
//!
//! All three tests currently fail RED: `check_mechanism_joint_bound` does
//! not yet exist and `phase_fn_arg_conformance` does not yet walk
//! `FunctionCall` (bind/dim/sweep) nodes.

use reify_core::DiagnosticCode;
use reify_test_support::compile_source_with_stdlib;

// ── bind path ────────────────────────────────────────────────────────────────

/// `bind(couple(prismatic(...), -1.0), 5mm)` — arg0 of `bind` is a
/// FunctionCall named "couple" → Path-B resolution → "Coupling" → does not
/// satisfy DrivingJoint → MechanismNonDrivingJoint naming "Coupling".
///
/// RED until step-6: no compile check exists yet.
#[test]
fn bind_couple_emits_mechanism_nondriving_joint_naming_coupling() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let b = bind(couple(prismatic(1.0), -1.0), 5mm)
}
"#,
    );
    let diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::MechanismNonDrivingJoint))
        .collect();
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 MechanismNonDrivingJoint for bind(couple(...)), \
         got {}: {:?}",
        diags.len(),
        module.diagnostics
    );
    assert!(
        diags[0].message.contains("Coupling"),
        "diagnostic should name 'Coupling'; got: {}",
        diags[0].message
    );
}

/// `bind(fixed(), 5mm)` — arg0 of `bind` is a FunctionCall named "fixed" →
/// Path-B → "Fixed" → does not satisfy DrivingJoint →
/// MechanismNonDrivingJoint naming "Fixed".
///
/// RED until step-6.
#[test]
fn bind_fixed_emits_mechanism_nondriving_joint_naming_fixed() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let b = bind(fixed(), 5mm)
}
"#,
    );
    let diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::MechanismNonDrivingJoint))
        .collect();
    assert_eq!(
        diags.len(),
        1,
        "expected exactly 1 MechanismNonDrivingJoint for bind(fixed()), \
         got {}: {:?}",
        diags.len(),
        module.diagnostics
    );
    assert!(
        diags[0].message.contains("Fixed"),
        "diagnostic should name 'Fixed'; got: {}",
        diags[0].message
    );
}

/// Positive guard: `bind(prismatic(1.0), 5mm)` — arg0 is a FunctionCall
/// named "prismatic" → Path-B → "Prismatic" → satisfies DrivingJoint →
/// zero MechanismNonDrivingJoint diagnostics.
///
/// RED until step-6 (currently no check emits the diagnostic, so this
/// trivially PASSES — kept RED alongside its siblings to confirm step-6
/// doesn't over-fire).
#[test]
fn bind_prismatic_emits_no_mechanism_nondriving_joint() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let b = bind(prismatic(1.0), 5mm)
}
"#,
    );
    let diags: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::MechanismNonDrivingJoint))
        .collect();
    assert!(
        diags.is_empty(),
        "bind(prismatic(...)) must emit zero MechanismNonDrivingJoint \
         diagnostics (positive guard); got: {:?}",
        diags
    );
}
