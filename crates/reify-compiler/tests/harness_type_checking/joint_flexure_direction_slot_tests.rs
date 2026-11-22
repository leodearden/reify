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

fn assert_one_direction_mismatch_naming(errors: &[&Diagnostic], field: &str, case: &str) -> String {
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
        assert_eq!(
            errors.len(),
            1,
            "{body}: expected one Error, got {errors:?}"
        );
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
        assert_eq!(
            errors.len(),
            1,
            "{body}: expected one Error, got {errors:?}"
        );
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

// ── builtin ctors: the axis argument is a Direction slot ────────────────────

/// `let j = <call>` inside a structure body that binds an origin `o` and a
/// let-bound Direction `a`.
fn builtin_source(call: &str) -> String {
    joint_source(&format!(
        "let o = point3(0mm, 0mm, 0mm)\n    let a = dir(0, 0, 1)\n    let j = {call}"
    ))
}

fn assert_one_slot_mismatch(call: &str, prefix: &str, vector_hint: bool) {
    let module = compile_source_with_stdlib(&builtin_source(call));
    let errors = errors_only(&module);
    assert_eq!(
        errors.len(),
        1,
        "{call}: expected one Error, got {errors:?}"
    );
    let diagnostic = errors[0];
    assert_eq!(
        diagnostic.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "{call}: {diagnostic:?}"
    );
    assert!(
        diagnostic.message.starts_with(prefix),
        "{call}: expected a message starting {prefix:?}: {diagnostic:?}"
    );
    assert_eq!(
        diagnostic.message.ends_with(DIRECTION_HINT),
        vector_hint,
        "{call}: only a Vector3 axis earns the dir(...) hint: {diagnostic:?}"
    );
}

fn assert_builtin_checks_clean(call: &str) {
    let module = compile_source_with_stdlib(&builtin_source(call));
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "{call:?} must check clean, got {errors:?}"
    );
}

#[test]
fn a_vector_at_a_joint_builtin_axis_is_one_mismatch_with_the_dir_hint() {
    for (call, name) in [
        ("prismatic(vec3(0, 1, 0), 0mm .. 10mm)", "prismatic"),
        ("prismatic(vec3(0, 1, 0), 0mm .. 10mm, o)", "prismatic"),
        ("revolute(vec3(0, 0, 1), 0rad .. 1rad)", "revolute"),
        ("revolute(vec3(0, 0, 1), 0rad .. 1rad, o)", "revolute"),
        (
            "cylindrical(vec3(0, 0, 1), 0mm .. 10mm, 0rad .. 1rad)",
            "cylindrical",
        ),
    ] {
        let prefix = format!("{name}: axis argument expects Direction, got Vector3<Int>");
        assert_one_slot_mismatch(call, &prefix, true);
    }
}

#[test]
fn planar_builtin_reports_one_mismatch_per_vector_axis() {
    let call = "planar(vec3(1, 0, 0), vec3(0, 1, 0), 0mm .. 1mm, 0mm .. 1mm, 0rad .. 1rad)";
    let module = compile_source_with_stdlib(&builtin_source(call));
    let errors = errors_only(&module);
    assert_eq!(errors.len(), 2, "one Error per vector axis, got {errors:?}");
    for prefix in [
        "planar: axis_x argument expects Direction",
        "planar: axis_y argument expects Direction",
    ] {
        assert!(
            errors
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch)
                    && d.message.starts_with(prefix)
                    && d.message.ends_with(DIRECTION_HINT)),
            "no ArgTypeMismatch starts with {prefix:?}: {errors:?}"
        );
    }
}

#[test]
fn a_string_at_a_joint_builtin_axis_is_one_mismatch_without_the_dir_hint() {
    assert_one_slot_mismatch(
        "revolute(\"up\", 0rad .. 1rad)",
        "revolute: axis argument expects Direction, got String",
        false,
    );
}

#[test]
fn direction_typed_axes_at_joint_builtins_check_clean() {
    for axis in ["dir(0, 0, 1)", "a", "axis_z(point3(0mm, 0mm, 0mm)).dir"] {
        for call in [
            format!("prismatic({axis}, 0mm .. 10mm)"),
            format!("revolute({axis}, 0rad .. 1rad, o)"),
            format!("cylindrical({axis}, 0mm .. 10mm, 0rad .. 1rad)"),
            format!("planar({axis}, {axis}, 0mm .. 1mm, 0mm .. 1mm, 0rad .. 1rad)"),
        ] {
            assert_builtin_checks_clean(&call);
        }
    }
}

/// One PRB flexure ctor's axis slot, written out independently of the slot
/// table so a wrong index or name in `builtin_signatures` reds here. Each row
/// was read off its parse fn in `reify-stdlib/src/flexures/*`.
struct FlexureAxisRow {
    name: &'static str,
    axis_index: usize,
    arg_name: &'static str,
    min_arity: usize,
    max_arity: usize,
}

const fn row(
    name: &'static str,
    axis_index: usize,
    arg_name: &'static str,
    min_arity: usize,
    max_arity: usize,
) -> FlexureAxisRow {
    FlexureAxisRow {
        name,
        axis_index,
        arg_name,
        min_arity,
        max_arity,
    }
}

/// The 13 PRB flexure ctors (the reify-compiler `FLEXURE_CTOR_FN_NAMES`
/// family, whose own unit test pins the count at 13).
const FLEXURE_AXIS_ROWS: &[FlexureAxisRow] = &[
    row("prb_cantilever_beam", 5, "axis", 6, 8),
    row("prb_fixed_fixed_beam", 5, "axis", 6, 8),
    row("prb_notch_circular", 5, "axis", 6, 8),
    row("prb_notch_elliptical", 5, "axis", 6, 8),
    row("prb_notch_right_circular", 5, "axis", 6, 8),
    row("prb_living_hinge", 5, "axis", 6, 8),
    row("prb_cross_spring_pivot", 5, "axis", 6, 8),
    row("prb_prismatic_blade", 5, "axis", 6, 8),
    row("prb_two_axis_pivot", 5, "axis", 6, 8),
    row("prb_parallelogram_flexure", 5, "motion_axis", 7, 8),
    row("prb_double_parallelogram_flexure", 5, "motion_axis", 7, 8),
    row("prb_let_joint", 6, "axis", 7, 9),
    row("prb_cartwheel_flexure", 6, "axis", 7, 9),
];

/// `name(1mm, …, <axis>, …)` at `arity`, with `axis` at `axis_index`.
fn flexure_call(row: &FlexureAxisRow, arity: usize, axis: &str) -> String {
    let args: Vec<&str> = (0..arity)
        .map(|i| if i == row.axis_index { axis } else { "1mm" })
        .collect();
    format!("{}({})", row.name, args.join(", "))
}

#[test]
fn the_flexure_axis_table_covers_the_thirteen_ctors_once_each() {
    let mut names: Vec<&str> = FLEXURE_AXIS_ROWS.iter().map(|r| r.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 13, "13 distinct flexure ctors, got {names:?}");
}

#[test]
fn a_vector_at_every_flexure_axis_is_one_mismatch_and_a_direction_checks_clean() {
    for row in FLEXURE_AXIS_ROWS {
        for arity in [row.min_arity, row.max_arity] {
            let prefix = format!(
                "{}: {} argument expects Direction, got Vector3<Int>",
                row.name, row.arg_name
            );
            assert_one_slot_mismatch(&flexure_call(row, arity, "vec3(0, 1, 0)"), &prefix, true);
            assert_builtin_checks_clean(&flexure_call(row, arity, "dir(0, 1, 0)"));
        }
    }
}
