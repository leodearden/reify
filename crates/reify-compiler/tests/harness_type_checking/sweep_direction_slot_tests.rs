//! The Direction-taking sweeps (PRD `docs/prds/v0_6/direction-typing.md`,
//! task ι-b #9647): `revolve(profile, origin, direction, angle)`,
//! `revolve(profile, axis, angle)`, `revolve_full(profile, origin, direction)`,
//! `extrude_infinite(profile, direction)` and
//! `extrude_infinite_symmetric(profile, direction)`.
//!
//! The direction is a `Direction` slot (a vector there is a static
//! `ArgTypeMismatch`), the retired scalar arities are arity errors that name
//! the Direction form, and each form lowers to the op-arg layout
//! `sweep_revolve` / `sweep_extrude_infinite` read.

use reify_compiler::{CompiledGeometryOp, SweepKind};
use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_ir::{CompiledExpr, CompiledExprKind, Value};
use reify_test_support::{compile_source_with_stdlib, errors_only};

/// The user-visible fix hint, hard-coded on purpose: this file is the DRIFT
/// PIN, so reading `DIRECTION_MIGRATION_HINT` here would make it a tautology.
const DIRECTION_HINT: &str = "write dir(x, y, z)";

const PROFILE: &str = "rectangle(20mm, 10mm)";

fn sweep_source(call: &str) -> String {
    format!("structure def SweepDirections {{\n    let g = {call}\n}}\n")
}

fn errors_of(call: &str) -> Vec<Diagnostic> {
    let module = compile_source_with_stdlib(&sweep_source(call));
    errors_only(&module).into_iter().cloned().collect()
}

fn arg_type_mismatches(errors: &[Diagnostic]) -> Vec<&Diagnostic> {
    errors
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .collect()
}

/// Exactly one Error, an `ArgTypeMismatch`; returns its message.
fn one_mismatch(call: &str) -> String {
    let errors = errors_of(call);
    assert_eq!(
        errors.len(),
        1,
        "{call}: expected one Error, got {errors:?}"
    );
    let diagnostic = &errors[0];
    assert_eq!(
        diagnostic.code,
        Some(DiagnosticCode::ArgTypeMismatch),
        "{call}: {diagnostic:?}"
    );
    assert_eq!(
        diagnostic.severity,
        Severity::Error,
        "{call}: {diagnostic:?}"
    );
    diagnostic.message.clone()
}

#[test]
fn a_vector_at_a_sweep_direction_is_one_mismatch_with_the_dir_hint() {
    for (name, call) in [
        (
            "revolve",
            format!("revolve({PROFILE}, point3(0mm, 0mm, 0mm), vec3(0, 0, 1), 90deg)"),
        ),
        (
            "revolve_full",
            format!("revolve_full({PROFILE}, point3(0mm, 0mm, 0mm), vec3(0, 1, 0))"),
        ),
        (
            "extrude_infinite",
            format!("extrude_infinite({PROFILE}, vec3(0, 0, 1))"),
        ),
        (
            "extrude_infinite_symmetric",
            format!("extrude_infinite_symmetric({PROFILE}, vec3(0, 0, 1))"),
        ),
    ] {
        assert_eq!(
            one_mismatch(&call),
            format!(
                "{name}: direction argument expects Direction, got Vector3<Int>; {DIRECTION_HINT}"
            ),
            "{call}"
        );
    }
}

#[test]
fn a_bare_angle_is_one_mismatch_in_both_revolve_forms() {
    for call in [
        format!("revolve({PROFILE}, point3(0mm, 0mm, 0mm), dir(0, 0, 1), 90)"),
        format!("revolve({PROFILE}, axis_z(point3(0mm, 0mm, 0mm)), 90)"),
    ] {
        let message = one_mismatch(&call);
        assert!(
            message.starts_with("revolve: angle argument expects Angle"),
            "{call}: {message:?}"
        );
    }
}

#[test]
fn the_direction_forms_check_clean() {
    for call in [
        format!("revolve({PROFILE}, point3(0mm, 0mm, 0mm), dir(0, 0, 1), 90deg)"),
        format!("revolve({PROFILE}, axis_y(point3(-10mm, 0mm, 0mm)), 90deg)"),
        format!("revolve_full({PROFILE}, point3(0mm, 0mm, 0mm), dir(0, 1, 0))"),
        format!("extrude_infinite({PROFILE}, dir(0, 0, 1))"),
        format!("extrude_infinite_symmetric({PROFILE}, dir(0, 0, -1))"),
    ] {
        let errors = errors_of(&call);
        assert!(
            arg_type_mismatches(&errors).is_empty(),
            "{call} must yield no ArgTypeMismatch, got {errors:?}"
        );
    }
}

/// B13: each retired scalar arity is an arity error that teaches the
/// Direction form.
#[test]
fn the_retired_scalar_arities_are_arity_errors_naming_the_direction_form() {
    for (name, call, got) in [
        (
            "revolve",
            format!("revolve({PROFILE}, -10mm, 0mm, 0mm, 0.0, 1.0, 0.0, 90deg)"),
            8,
        ),
        (
            "revolve_full",
            format!("revolve_full({PROFILE}, -10mm, 0mm, 0mm, 0.0, 1.0, 0.0)"),
            7,
        ),
        (
            "extrude_infinite",
            format!("extrude_infinite({PROFILE}, 0, 0, 1, \"both\")"),
            5,
        ),
    ] {
        let errors = errors_of(&call);
        let arity = errors
            .iter()
            .find(|d| d.message.starts_with(&format!("{name}() expects")))
            .unwrap_or_else(|| panic!("{call}: no `{name}() expects` Error in {errors:?}"));
        assert!(
            arity.message.ends_with(&format!(", got {got}")),
            "{call}: {:?}",
            arity.message
        );
        assert!(
            arity.message.contains("dir(x, y, z)"),
            "{call}: the arity error must name the Direction form: {:?}",
            arity.message
        );
        if name == "extrude_infinite" {
            assert!(
                arity.message.contains("extrude_infinite_symmetric"),
                "{call}: the old \"both\" sense maps to extrude_infinite_symmetric: {:?}",
                arity.message
            );
        }
    }
}

/// The `CompiledGeometryOp::Sweep` the call lowers to: its kind and op args.
fn lowered_sweep(call: &str) -> (SweepKind, Vec<(String, CompiledExpr)>) {
    let module = compile_source_with_stdlib(&sweep_source(call));
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "{call}: {errors:?}");
    module
        .templates
        .iter()
        .flat_map(|t| &t.realizations)
        .flat_map(|r| &r.operations)
        .find_map(|op| match op {
            CompiledGeometryOp::Sweep { kind, args, .. } => Some((*kind, args.clone())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{call}: no Sweep op was lowered"))
}

fn arg_names(args: &[(String, CompiledExpr)]) -> Vec<&str> {
    args.iter().map(|(name, _)| name.as_str()).collect()
}

fn literal_arg<'a>(args: &'a [(String, CompiledExpr)], name: &str) -> &'a Value {
    let (_, expr) = args
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no `{name}` arg in {:?}", arg_names(args)));
    match &expr.kind {
        CompiledExprKind::Literal(value) => value,
        other => panic!("`{name}` must be a synthesized literal, got {other:?}"),
    }
}

#[test]
fn each_form_lowers_to_the_layout_the_eval_reads() {
    let (kind, args) = lowered_sweep(&format!(
        "revolve({PROFILE}, point3(0mm, 0mm, 0mm), dir(0, 0, 1), 90deg)"
    ));
    assert_eq!(kind, SweepKind::Revolve);
    assert_eq!(
        arg_names(&args),
        ["profile", "origin", "direction", "angle"]
    );

    let (kind, args) = lowered_sweep(&format!(
        "revolve({PROFILE}, axis_y(point3(-10mm, 0mm, 0mm)), 90deg)"
    ));
    assert_eq!(kind, SweepKind::Revolve);
    assert_eq!(arg_names(&args), ["profile", "axis", "angle"]);

    let (kind, args) = lowered_sweep(&format!(
        "revolve_full({PROFILE}, point3(0mm, 0mm, 0mm), dir(0, 1, 0))"
    ));
    assert_eq!(kind, SweepKind::Revolve);
    assert_eq!(
        arg_names(&args),
        ["profile", "origin", "direction", "angle"]
    );
    assert_eq!(
        literal_arg(&args, "angle"),
        &Value::angle(std::f64::consts::TAU)
    );

    for (call, symmetric) in [
        (format!("extrude_infinite({PROFILE}, dir(0, 0, 1))"), false),
        (
            format!("extrude_infinite_symmetric({PROFILE}, dir(0, 0, 1))"),
            true,
        ),
    ] {
        let (kind, args) = lowered_sweep(&call);
        assert_eq!(kind, SweepKind::ExtrudeInfinite, "{call}");
        assert_eq!(
            arg_names(&args),
            ["profile", "direction", "symmetric"],
            "{call}"
        );
        assert_eq!(
            literal_arg(&args, "symmetric"),
            &Value::Bool(symmetric),
            "{call}"
        );
    }
}
