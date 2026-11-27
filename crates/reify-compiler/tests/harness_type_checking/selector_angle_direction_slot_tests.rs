//! The directional selectors' direction and the arity-2 `angle(a, b)` operands
//! are `Direction` slots: a vector, string or number there is a static
//! `ArgTypeMismatch`, and a Direction-typed value checks clean (PRD
//! `docs/prds/v0_6/direction-typing.md` B15 / B25, task θ-b #9645). The
//! at-point `normal(surface, at)` is typed a `Direction` (B4).

use reify_core::{Diagnostic, DiagnosticCode, Severity, Type};
use reify_test_support::{compile_source_with_stdlib, errors_only, get_let_expr_in};

/// The user-visible fix hint, hard-coded on purpose: this file is the DRIFT
/// PIN, so reading `DIRECTION_MIGRATION_HINT` here would make it a tautology.
const DIRECTION_HINT: &str = "write dir(x, y, z)";

/// Each directional selector and the name its direction slot carries.
const SELECTORS: &[(&str, &str)] = &[
    ("faces_by_normal", "dir"),
    ("edges_parallel_to", "axis"),
    ("faces_perpendicular_to", "axis"),
    ("edges_perpendicular_to", "axis"),
];

/// A geometry-bearing structure body: a box `b` followed by `lines`.
fn box_source(lines: &[&str]) -> String {
    let mut body = String::from("    let b = box(10mm, 10mm, 10mm)\n");
    for line in lines {
        body.push_str("    ");
        body.push_str(line);
        body.push('\n');
    }
    format!("structure def {BOX_STRUCTURE} {{\n{body}}}\n")
}

const BOX_STRUCTURE: &str = "SelectorDirections";

/// A face `top` of the box, a point `o`, and `n = normal(top, o)`.
const NORMAL_LINES: &[&str] = &[
    "let top = single(faces(b))",
    "let o = point3(0mm, 0mm, 0mm)",
    "let n = normal(top, o)",
];

fn errors_of(lines: &[&str]) -> Vec<Diagnostic> {
    let module = compile_source_with_stdlib(&box_source(lines));
    errors_only(&module).into_iter().cloned().collect()
}

/// Exactly one Error, an `ArgTypeMismatch` naming `Direction`; returns its
/// message.
fn assert_one_direction_mismatch(errors: &[Diagnostic], case: &str) -> String {
    assert_eq!(errors.len(), 1, "{case}: expected one Error, got {errors:?}");
    let diagnostic = &errors[0];
    assert_eq!(
        diagnostic.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "{case}: {diagnostic:?}"
    );
    assert_eq!(diagnostic.severity, Severity::Error, "{case}: {diagnostic:?}");
    assert!(
        diagnostic.message.contains("Direction"),
        "{case}: the mismatch must name Direction: {diagnostic:?}"
    );
    diagnostic.message.clone()
}

#[test]
fn a_vector_at_a_selector_direction_is_one_mismatch_with_the_dir_hint() {
    for (selector, arg) in SELECTORS {
        let let_bound = [
            "let v = vec3(0.0, 0.0, 1.0)".to_string(),
            format!("let s = {selector}(b, v, 1deg)"),
        ];
        let inline = [format!("let s = {selector}(b, vec3(0.0, 0.0, 1.0), 1deg)")];
        for lines in [&let_bound[..], &inline[..]] {
            let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
            let case = lines.join("; ");
            let message = assert_one_direction_mismatch(&errors_of(&lines), &case);
            let prefix = format!("{selector}: {arg} argument expects Direction, got Vector3<");
            assert!(
                message.starts_with(&prefix),
                "{case}: expected a message starting {prefix:?}: {message:?}"
            );
            assert!(
                message.ends_with(DIRECTION_HINT),
                "{case}: a Vector3 direction must carry the dir(...) fix hint: {message:?}"
            );
        }
    }
}

#[test]
fn a_non_vector_at_a_selector_direction_is_one_mismatch_without_the_dir_hint() {
    for (selector, arg) in SELECTORS {
        for operand in ["\"up\"", "5"] {
            let line = format!("let s = {selector}(b, {operand}, 1deg)");
            let message = assert_one_direction_mismatch(&errors_of(&[&line]), &line);
            assert!(
                message.starts_with(&format!("{selector}: {arg} argument expects Direction")),
                "{line}: {message:?}"
            );
            assert!(
                !message.contains("write dir"),
                "{line}: only a Vector3 earns the dir(...) hint: {message:?}"
            );
        }
    }
}

#[test]
fn direction_typed_operands_at_selector_directions_check_clean() {
    for operand in [
        "dir(0, 0, 1)",
        "axis_z(point3(0mm, 0mm, 0mm)).dir",
        "frame_at(point3(0mm, 0mm, 0mm), dir(1, 0, 0), dir(0, 0, 1)).z",
    ] {
        for (selector, _) in SELECTORS {
            let line = format!("let s = {selector}(b, {operand}, 1deg)");
            let errors = errors_of(&[&line]);
            assert!(errors.is_empty(), "{line:?} must check clean, got {errors:?}");
        }
    }
}

#[test]
fn the_selector_tol_slot_is_still_checked_beside_a_direction() {
    let line = "let s = faces_by_normal(b, dir(0, 0, 1), 5mm)";
    let errors = errors_of(&[line]);
    assert_eq!(errors.len(), 1, "{line}: expected one Error, got {errors:?}");
    assert!(
        errors[0].message.contains("tol"),
        "{line}: the Error must name `tol`: {:?}",
        errors[0]
    );
}

#[test]
fn arity_two_angle_reports_one_mismatch_per_vector_operand() {
    let lines = [
        "let a = vec3(1.0, 0.0, 0.0)",
        "let c = vec3(0.0, 1.0, 0.0)",
        "let t = angle(a, c)",
    ];
    let errors = errors_of(&lines);
    assert_eq!(errors.len(), 2, "one Error per vector operand, got {errors:?}");
    for prefix in [
        "angle: a argument expects Direction",
        "angle: b argument expects Direction",
    ] {
        assert!(
            errors.iter().any(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch)
                && d.message.starts_with(prefix)
                && d.message.ends_with(DIRECTION_HINT)),
            "no ArgTypeMismatch starts with {prefix:?} and ends with the hint: {errors:?}"
        );
    }
}

#[test]
fn arity_two_angle_over_directions_checks_clean() {
    let line = "let t = angle(dir(1, 0, 0), dir(0, 1, 0))";
    let errors = errors_of(&[line]);
    assert!(errors.is_empty(), "{line:?} must check clean, got {errors:?}");
}

/// The arity-3 `angle` is the relation verb, policed by `relation_signatures`;
/// it carries no builtin Direction slot. Its operands are Axes, which a
/// Direction slot would reject, so a slot leaking to arity 3 reds this.
#[test]
fn the_arity_three_relation_angle_over_axes_checks_clean() {
    let source = "structure Part {\n    \
                  let shank = cylinder(3mm, 20mm)\n    \
                  let shank_axis : Axis = shank.axis\n\
                  }\n\
                  \n\
                  structure Asm {\n    \
                  sub a : Part at auto\n    \
                  sub b : Part\n\
                  \n    \
                  relate {\n        \
                  angle(a.shank_axis, b.shank_axis, 90deg)\n    \
                  }\n\
                  }\n";
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "the relation verb over two Axes must check clean, got {errors:?}"
    );
}

#[test]
fn normal_at_a_point_is_typed_a_direction() {
    let module = compile_source_with_stdlib(&box_source(NORMAL_LINES));
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "the normal query must check clean, got {errors:?}");
    assert_eq!(
        get_let_expr_in(&module, BOX_STRUCTURE, "n").result_type,
        Type::Direction
    );
}

#[test]
fn adding_two_normals_is_an_operand_kind_error_with_the_vector_hint() {
    let mut lines = NORMAL_LINES.to_vec();
    lines.push("let bad = n + n");
    let errors = errors_of(&lines);
    assert!(
        errors.iter().any(|d| d.code == Some(DiagnosticCode::ArithOperandKind)
            && d.message.ends_with("write vector(d)")),
        "Direction + Direction must be an ArithOperandKind ending \"write vector(d)\", \
         got {errors:?}"
    );
}
