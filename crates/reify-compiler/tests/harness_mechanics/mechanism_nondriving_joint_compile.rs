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
//! ## RED until step-6 (bind tests) / step-8 (dim + sweep tests)
//!
//! The `bind` tests (step-5 RED / step-6 GREEN) confirm that
//! `check_expr_mechanism_joint_bound` fires for `bind`.  The `dim` and
//! `sweep` tests (step-7 RED / step-8 GREEN) confirm that the same check
//! is generalised to `dim` (arg0) and `sweep@arity4` (arg1).

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

// ── dim path ─────────────────────────────────────────────────────────────────

/// `dim(couple(prismatic(1.0), -1.0), 0mm..1m, 11)` — arg0 of `dim` is a
/// FunctionCall named "couple" → Path-B resolution → "Coupling" → does not
/// satisfy DrivingJoint → MechanismNonDrivingJoint naming "Coupling".
///
/// RED until step-8: `check_expr_mechanism_joint_bound` currently only
/// handles `bind`; `dim` is not yet in the builtin table.
#[test]
fn dim_couple_emits_mechanism_nondriving_joint_naming_coupling() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let d = dim(couple(prismatic(1.0), -1.0), 0mm..1m, 11)
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
        "expected exactly 1 MechanismNonDrivingJoint for dim(couple(...)), \
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

/// Positive guard: `dim(prismatic(1.0), 0mm..1m, 11)` — arg0 is a
/// FunctionCall named "prismatic" → Path-B → "Prismatic" → satisfies
/// DrivingJoint → zero MechanismNonDrivingJoint diagnostics.
///
/// Trivially passes (no check fires) until step-8; kept to confirm
/// step-8 doesn't over-fire.
#[test]
fn dim_prismatic_emits_no_mechanism_nondriving_joint() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let d = dim(prismatic(1.0), 0mm..1m, 11)
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
        "dim(prismatic(...)) must emit zero MechanismNonDrivingJoint \
         diagnostics (positive guard); got: {:?}",
        diags
    );
}

// ── sweep path ────────────────────────────────────────────────────────────────

/// Kinematic `sweep(mechanism(), couple(prismatic(1.0), -1.0), 0mm..1m, 11)`
/// (arity 4) — arg1 of `sweep` is a FunctionCall named "couple" → Path-B
/// → "Coupling" → does not satisfy DrivingJoint →
/// MechanismNonDrivingJoint naming "Coupling".
///
/// RED until step-8: `check_expr_mechanism_joint_bound` currently only
/// handles `bind`; arity-4 `sweep` is not yet in the builtin table.
#[test]
fn sweep_arity4_couple_emits_mechanism_nondriving_joint_naming_coupling() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let s = sweep(mechanism(), couple(prismatic(1.0), -1.0), 0mm..1m, 11)
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
        "expected exactly 1 MechanismNonDrivingJoint for sweep@arity4(couple(...)), \
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

/// Positive guard: kinematic `sweep(mechanism(), prismatic(1.0), 0mm..1m, 11)`
/// (arity 4) — arg1 is "prismatic" → Path-B → "Prismatic" → satisfies
/// DrivingJoint → zero MechanismNonDrivingJoint diagnostics.
///
/// Trivially passes until step-8; kept to confirm step-8 doesn't over-fire.
#[test]
fn sweep_arity4_prismatic_emits_no_mechanism_nondriving_joint() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let s = sweep(mechanism(), prismatic(1.0), 0mm..1m, 11)
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
        "sweep@arity4(prismatic(...)) must emit zero MechanismNonDrivingJoint \
         diagnostics (positive guard); got: {:?}",
        diags
    );
}

/// Geometry sweep `sweep(cylinder(5mm, 10mm), line_segment(...))` (arity 2)
/// — must NOT emit MechanismNonDrivingJoint.  The arity-2 form is the CSG
/// geometry sweep (docs §3), not the kinematic sweep (§13.4); the check
/// must skip it.
///
/// Trivially passes until step-8; kept to confirm step-8 applies the
/// arity-4 guard and does not mis-check geometry sweeps.
#[test]
fn sweep_geometry_arity2_untouched_no_mechanism_nondriving_joint() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let s = sweep(cylinder(5mm, 10mm), line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 10mm))
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
        "geometry sweep (arity 2) must emit zero MechanismNonDrivingJoint \
         diagnostics; got: {:?}",
        diags
    );
}

// ── Coupling constructor aliases: gear / screw / rack_and_pinion ──────────────
//
// Path B maps the three thin-wrapper constructors (gear, screw, rack_and_pinion)
// to the same "Coupling" nominal type as `couple`.  A regression in any of
// those arms would silently allow non-driving coupling joints through the L2
// guard.  These tests pin each arm explicitly.
//
// The arg shapes below are intentionally minimal (integer literals for
// teeth counts, mm literal for lead/pitch_radius) — eval-level validation
// of the coupling args is not the subject of these tests.  The diagnostic
// filter is strict (MechanismNonDrivingJoint only), so incidental arity /
// type-mismatch diagnostics from the builtin evaluation do not affect the count.

/// `bind(gear(revolute(1.0), 10, 20), 5mm)` — gear wraps a revolute parent into
/// a Coupling → Path-B resolution → "Coupling" → MechanismNonDrivingJoint.
#[test]
fn bind_gear_emits_mechanism_nondriving_joint_naming_coupling() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let b = bind(gear(revolute(1.0), 10, 20), 5mm)
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
        "expected exactly 1 MechanismNonDrivingJoint for bind(gear(...)), \
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

/// `bind(screw(prismatic(1.0), 1mm), 5mm)` — screw wraps a prismatic parent
/// into a Coupling → Path-B resolution → "Coupling" → MechanismNonDrivingJoint.
#[test]
fn bind_screw_emits_mechanism_nondriving_joint_naming_coupling() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let b = bind(screw(prismatic(1.0), 1mm), 5mm)
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
        "expected exactly 1 MechanismNonDrivingJoint for bind(screw(...)), \
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

/// `bind(rack_and_pinion(prismatic(1.0), 5mm), 10mm)` — rack_and_pinion wraps
/// a prismatic parent into a Coupling → Path-B → "Coupling" →
/// MechanismNonDrivingJoint.
#[test]
fn bind_rack_and_pinion_emits_mechanism_nondriving_joint_naming_coupling() {
    let module = compile_source_with_stdlib(
        r#"
structure def Test {
    let b = bind(rack_and_pinion(prismatic(1.0), 5mm), 10mm)
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
        "expected exactly 1 MechanismNonDrivingJoint for bind(rack_and_pinion(...)), \
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
