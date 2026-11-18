//! `frame_at`'s x/z arguments are `Direction` slots: a vector there is a static
//! `ArgTypeMismatch`, a Direction-typed value checks clean (PRD
//! `docs/prds/v0_6/direction-typing.md` B23 / C7, task σ #9636).

use reify_core::{DiagnosticCode, Severity};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// The user-visible fix hint, hard-coded on purpose: this file is the DRIFT
/// PIN, so reading `DIRECTION_MIGRATION_HINT` here would make it a tautology.
const DIRECTION_HINT: &str = "write dir(x, y, z)";

fn frame_at_source(body: &str) -> String {
    format!("structure def FrameAtX {{\n    let o = point3(0mm, 0mm, 0mm)\n    {body}\n}}\n")
}

#[test]
fn vector_axes_are_two_arg_type_mismatches_naming_direction() {
    let module = compile_source_with_stdlib(&frame_at_source(
        "let f = frame_at(o, vec3(1, 0, 0), vec3(0, 0, 1))",
    ));
    let errors = errors_only(&module);

    assert_eq!(
        errors.len(),
        2,
        "expected one Error per vector axis, got {errors:?}"
    );
    for diagnostic in &errors {
        assert_eq!(
            diagnostic.code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "{diagnostic:?}"
        );
        assert_eq!(diagnostic.severity, Severity::Error, "{diagnostic:?}");
        assert!(
            diagnostic.message.ends_with(DIRECTION_HINT),
            "a Vector3 axis must carry the dir(...) fix hint: {diagnostic:?}"
        );
    }
    for prefix in [
        "frame_at: x argument expects Direction, got Vector3<Int>",
        "frame_at: z argument expects Direction, got Vector3<Int>",
    ] {
        assert!(
            errors.iter().any(|d| d.message.starts_with(prefix)),
            "no Error starts with {prefix:?}: {errors:?}"
        );
    }
}

#[test]
fn one_vector_axis_is_reported_alone() {
    let module = compile_source_with_stdlib(&frame_at_source(
        "let f = frame_at(o, self.x, vec3(0, 0, 1))",
    ));
    let errors = errors_only(&module);

    assert_eq!(
        errors.len(),
        1,
        "only the z axis is a vector, got {errors:?}"
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "{:?}",
        errors[0]
    );
    assert!(
        errors[0]
            .message
            .starts_with("frame_at: z argument expects Direction"),
        "{:?}",
        errors[0]
    );
}

#[test]
fn direction_typed_axes_check_clean() {
    let declarations = "param dx : Direction\n    param dz : Direction\n    ";
    for body in [
        "let f = frame_at(o, dir(1, 0, 0), dir(0, 0, 1))".to_string(),
        "let a = axis_x(o)\n    let f = frame_at(o, a.dir, a.dir)".to_string(),
        "let f = frame_at(o, self.x, self.z)".to_string(),
        "let f = frame_at(o, plane_xy(0mm).normal, self.z)".to_string(),
        format!("{declarations}let f = frame_at(o, dx, dz)"),
    ] {
        let module = compile_source_with_stdlib(&frame_at_source(&body));
        let errors = errors_only(&module);
        assert!(
            errors.is_empty(),
            "{body:?} must check clean, got {errors:?}"
        );
    }
}
