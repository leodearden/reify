//! A joint's axis is a `Direction`: a vector, string or number at a
//! `std.kinematic` joint ctor field is a static `ArgTypeMismatch`, and a
//! Direction-typed value checks clean (PRD `docs/prds/v0_6/direction-typing.md`
//! C3 / B6, task ζ-a #9640).

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// The user-visible fix hint, hard-coded on purpose: this file is the DRIFT
/// PIN, so reading `DIRECTION_MIGRATION_HINT` here would make it a tautology.
const DIRECTION_HINT: &str = "write dir(x, y, z)";

fn joint_source(body: &str) -> String {
    format!("structure def JointAxisX {{\n    {body}\n}}\n")
}

fn assert_one_direction_mismatch_naming(
    errors: &[&Diagnostic],
    field: &str,
    case: &str,
) -> String {
    let naming: Vec<&&Diagnostic> = errors
        .iter()
        .filter(|d| d.message.contains(&format!("'{field}'")))
        .collect();
    assert_eq!(
        naming.len(),
        1,
        "{case}: expected one Error naming '{field}', got {errors:?}"
    );
    let diagnostic = naming[0];
    assert_eq!(
        diagnostic.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "{case}: {diagnostic:?}"
    );
    assert_eq!(
        diagnostic.severity,
        Severity::Error,
        "{case}: {diagnostic:?}"
    );
    assert!(
        diagnostic.message.contains("Direction"),
        "{case}: the mismatch must name Direction: {diagnostic:?}"
    );
    diagnostic.message.clone()
}

#[test]
fn a_vector_at_a_joint_ctor_axis_is_one_mismatch_with_the_dir_hint() {
    for arg in ["vec3(0, 1, 0)", "vec3(0, 0, 0)"] {
        let body = format!("let j = Revolute(axis: {arg})");
        let module = compile_source_with_stdlib(&joint_source(&body));
        let errors = errors_only(&module);
        assert_eq!(errors.len(), 1, "{body}: expected one Error, got {errors:?}");
        let message = assert_one_direction_mismatch_naming(&errors, "axis", &body);
        assert!(
            message.ends_with(DIRECTION_HINT),
            "{body}: a Vector3 axis must carry the dir(...) fix hint: {message:?}"
        );
    }
}

#[test]
fn a_non_vector_at_a_joint_ctor_axis_is_one_mismatch_without_the_dir_hint() {
    for arg in ["\"up\"", "5"] {
        let body = format!("let j = Revolute(axis: {arg})");
        let module = compile_source_with_stdlib(&joint_source(&body));
        let errors = errors_only(&module);
        assert_eq!(errors.len(), 1, "{body}: expected one Error, got {errors:?}");
        let message = assert_one_direction_mismatch_naming(&errors, "axis", &body);
        assert!(
            !message.contains("write dir"),
            "{body}: only a Vector3 earns the dir(...) hint: {message:?}"
        );
    }
}

#[test]
fn planar_reports_one_mismatch_per_vector_axis() {
    let body = "let j = Planar(axis_x: vec3(1, 0, 0), axis_y: vec3(0, 1, 0))";
    let module = compile_source_with_stdlib(&joint_source(body));
    let errors = errors_only(&module);
    assert_eq!(errors.len(), 2, "one Error per vector axis, got {errors:?}");
    for field in ["axis_x", "axis_y"] {
        let message = assert_one_direction_mismatch_naming(&errors, field, body);
        assert!(message.ends_with(DIRECTION_HINT), "{message:?}");
    }
}

#[test]
fn direction_axes_at_joint_ctor_fields_check_clean() {
    for body in [
        "let j = Prismatic(axis: dir(1, 0, 0))",
        "let j = Revolute(axis: dir(0, 1, 0))",
        "let j = Cylindrical(axis: dir(0, 0, 1))",
        "let j = Planar(axis_x: dir(1, 0, 0), axis_y: dir(0, 1, 0))",
    ] {
        let module = compile_source_with_stdlib(&joint_source(body));
        let errors = errors_only(&module);
        assert!(
            errors.is_empty(),
            "{body:?} must check clean, got {errors:?}"
        );
    }
}
