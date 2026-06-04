//! E2E test for the `E_MECHANISM_DUPLICATE_SOLID` diagnostic-emission seam
//! (task 4308 — mechanism δ).
//!
//! Asserts that `Engine::eval` emits exactly one `Severity::Error` diagnostic
//! carrying `DiagnosticCode::MechanismDuplicateSolid` when the source constructs
//! a mechanism where the same solid name is attached twice (`body()` called with
//! the same solid argument in two steps).
//!
//! This test is intentionally RED until step-2 wires `detect_mechanism_errors`
//! into `Engine::eval`.

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// A `.ri` source where solid `"a"` is attached to two separate body() calls —
/// the canonical duplicate-solid recipe (mirrors `ERRORED_SOURCE` in
/// `forward_kinematics_e2e.rs`).
const DUPLICATE_SOLID_SOURCE: &str = r#"
structure def DuplicateSolid {
    let j_a = prismatic(vec3(1, 0, 0), 0mm .. 1000mm)
    let j_b = prismatic(vec3(0, 1, 0), 0mm .. 1000mm)

    let m0 = mechanism()
    let m1 = body(m0, "a", j_a)
    // Attaching solid "a" a second time (different joint) triggers duplicate_solid.
    let m2 = body(m1, "a", j_b)
}
"#;

/// `Engine::eval` must emit exactly one `E_MECHANISM_DUPLICATE_SOLID` Error
/// diagnostic when the source contains a duplicate-solid mechanism.
///
/// Assertions:
/// 1. The diagnostics list contains EXACTLY ONE entry with
///    `severity == Severity::Error` AND
///    `code == Some(DiagnosticCode::MechanismDuplicateSolid)`.
///
/// The test keys on the `DiagnosticCode`, NOT the message text, to remain
/// stable as the wording evolves.
#[test]
fn eval_emits_mechanism_duplicate_solid_error_diagnostic() {
    let compiled = parse_and_compile_with_stdlib(DUPLICATE_SOLID_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let matching: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::MechanismDuplicateSolid)
        })
        .collect();

    assert_eq!(
        matching.len(),
        1,
        "Engine::eval must emit exactly one E_MECHANISM_DUPLICATE_SOLID Error diagnostic \
         for a duplicate-solid source; got {} matching diagnostic(s) out of {} total.\n\
         All diagnostics: {:#?}",
        matching.len(),
        result.diagnostics.len(),
        result.diagnostics,
    );
}
