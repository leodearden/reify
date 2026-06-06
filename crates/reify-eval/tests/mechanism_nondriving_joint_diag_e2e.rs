//! E2E tests for the `E_MECHANISM_NONDRIVING_JOINT` diagnostic-emission seam
//! (task 4309 — mechanism α).
//!
//! Covers:
//! - bind(couple(...), value): exactly one E_MECHANISM_NONDRIVING_JOINT Error emitted.
//! - bind(prismatic(...), value): zero E_MECHANISM_NONDRIVING_JOINT diagnostics (both
//!   directions — driving joints must not trigger the guard).
//! - dim(couple(...), range, steps): ≥1 E_MECHANISM_NONDRIVING_JOINT Error emitted,
//!   confirming the seam covers the sweep/dim emission site as well.
//!
//! All three tests are GREEN: `detect_nondriving_joint_errors` is wired into
//! both `Engine::eval` and `Engine::eval_cached` (step-10 of the plan), so the
//! assertions below reflect the now-active contract.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// A `.ri` source where `bind` receives a coupling joint.
///
/// The coupling has no independent free motion variable (its DOF is derived
/// from `j`), so `bind(c, 5mm)` must surface `E_MECHANISM_NONDRIVING_JOINT`.
/// The offending binding is let-bound so the δ seam's top-level cell scan
/// finds it.
const NONDRIVING_BIND_SOURCE: &str = r#"
structure def NondrivingBind {
    let j = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let c = couple(j, -1.0)
    let b = bind(c, 5mm)
}
"#;

/// A `.ri` source where `bind` receives a prismatic joint — the happy path.
///
/// `bind(j, 5mm)` must NOT trigger `E_MECHANISM_NONDRIVING_JOINT`.
const DRIVING_BIND_SOURCE: &str = r#"
structure def DrivingBind {
    let j = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let b = bind(j, 5mm)
}
"#;

/// A `.ri` source where `dim` receives a coupling joint.
///
/// `dim(c, range, steps)` must surface `E_MECHANISM_NONDRIVING_JOINT`
/// because coupling has no sweepable DOF.  The offending `dim` result is
/// let-bound so the δ seam's top-level cell scan finds it.
const NONDRIVING_DIM_SOURCE: &str = r#"
structure def NondrivingDim {
    let j = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let c = couple(j, 2.0)
    let d = dim(c, 0mm .. 1000mm, 5)
}
"#;

/// `Engine::eval` must emit exactly one `E_MECHANISM_NONDRIVING_JOINT` Error
/// diagnostic when the source contains `bind(coupling, value)`.
///
/// Keyed on `DiagnosticCode`, not message text, for stability.
#[test]
fn eval_emits_nondriving_joint_error_for_bind_coupling() {
    let compiled = parse_and_compile_with_stdlib(NONDRIVING_BIND_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::MechanismNonDrivingJoint)
        })
        .collect();

    assert_eq!(
        matching.len(),
        1,
        "Engine::eval must emit exactly one E_MECHANISM_NONDRIVING_JOINT Error diagnostic \
         for bind(coupling, value); got {} matching diagnostic(s) out of {} total.\n\
         All diagnostics: {:#?}",
        matching.len(),
        result.diagnostics.len(),
        result.diagnostics,
    );
}

/// `Engine::eval` must emit ZERO `E_MECHANISM_NONDRIVING_JOINT` diagnostics
/// when the source uses `bind` with a prismatic (driving) joint.
#[test]
fn eval_emits_no_nondriving_joint_error_for_bind_prismatic() {
    let compiled = parse_and_compile_with_stdlib(DRIVING_BIND_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::MechanismNonDrivingJoint))
        .collect();

    assert_eq!(
        matching.len(),
        0,
        "Engine::eval must emit zero E_MECHANISM_NONDRIVING_JOINT diagnostics for \
         bind(prismatic, value); got {} matching diagnostic(s) out of {} total.\n\
         All diagnostics: {:#?}",
        matching.len(),
        result.diagnostics.len(),
        result.diagnostics,
    );
}

/// `Engine::eval` must emit at least one `E_MECHANISM_NONDRIVING_JOINT` Error
/// diagnostic when the source contains `dim(coupling, range, steps)`, proving
/// the seam covers the sweep/dim emission site as well as the bind site.
#[test]
fn eval_emits_nondriving_joint_error_for_dim_coupling() {
    let compiled = parse_and_compile_with_stdlib(NONDRIVING_DIM_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::MechanismNonDrivingJoint)
        })
        .collect();

    assert!(
        !matching.is_empty(),
        "Engine::eval must emit at least one E_MECHANISM_NONDRIVING_JOINT Error diagnostic \
         for dim(coupling, range, steps); got 0 out of {} total diagnostics.\n\
         All diagnostics: {:#?}",
        result.diagnostics.len(),
        result.diagnostics,
    );
}
