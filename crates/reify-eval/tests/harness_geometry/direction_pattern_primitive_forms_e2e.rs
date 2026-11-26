//! The Direction-taking primitive and pattern forms (task ι-c #9648, PRD
//! `docs/prds/v0_6/direction-typing.md` B12″ / B13), compiled from `.ri`
//! SOURCE and built against a mock kernel.
//!
//! B12″ "same IR": each new form emits the `GeometryOp` its retired scalar
//! form emitted, as measured on the tree before the scalar forms were retired.
//!
//! A Point3 origin is not a compile-time slot (no `ExpectedArg` kind holds a
//! Point3), so a bare origin component is gated at BUILD, named
//! `{arg}.x|y|z`.

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_ir::{GeometryOp, Value};
use reify_test_support::{build_against_mock_kernel, compile_source, parse_and_compile};

fn structure_source(call: &str) -> String {
    format!("structure def DirectionPatternPrimitiveForms {{\n    let g = {call}\n}}\n")
}

/// Every op `call` emits, built from strict-compiled source with no Error
/// diagnostics.
fn clean_ops(call: &str) -> Vec<GeometryOp> {
    let (diagnostics, ops) = build_against_mock_kernel(parse_and_compile(&structure_source(call)));
    let errors: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{call}: {errors:?}");
    ops
}

/// The diagnostics and ops of `call`, compiled without asserting a clean
/// compile, for calls whose rejection happens at build.
fn lenient_build(call: &str) -> (Vec<Diagnostic>, Vec<GeometryOp>) {
    build_against_mock_kernel(compile_source(&structure_source(call)))
}

#[track_caller]
fn assert_rejected_naming(diagnostics: &[Diagnostic], needles: &[&str]) {
    assert!(
        diagnostics.iter().any(|d| d.severity == Severity::Error
            && d.code == Some(DiagnosticCode::DimensionedArgRejected)
            && needles.iter().all(|needle| d.message.contains(needle))),
        "expected a DimensionedArgRejected Error naming {needles:?}; got: {diagnostics:#?}"
    );
}

/// Retired `half_space(0mm, 0mm, 0mm, 0, 0, 1)` emitted
/// `HalfSpace { px/py/pz: Scalar{0.0, LENGTH}, nx: Int(0), ny: Int(0), nz: Int(1) }`.
/// The point compares exactly. The normal compares numerically: the Direction
/// form stores the unit components as `Real`, where the integer literals
/// arrived as `Int` (PRD §10 Q6); the kernel reads both through `as_f64`.
#[test]
fn half_space_direction_form_emits_the_retired_scalar_op() {
    let call = "half_space(point3(0mm, 0mm, 0mm), dir(0, 0, 1))";
    let half_spaces: Vec<GeometryOp> = clean_ops(call)
        .into_iter()
        .filter(|op| matches!(op, GeometryOp::HalfSpace { .. }))
        .collect();
    let [
        GeometryOp::HalfSpace {
            px,
            py,
            pz,
            nx,
            ny,
            nz,
        },
    ] = half_spaces.as_slice()
    else {
        panic!("{call}: expected exactly one HalfSpace op, got {half_spaces:?}");
    };
    for (name, got) in [("px", px), ("py", py), ("pz", pz)] {
        assert_eq!(*got, Value::length(0.0), "{call}: {name}");
    }
    let normal = [nx, ny, nz].map(|component| component.as_f64());
    assert_eq!(normal, [Some(0.0), Some(0.0), Some(1.0)], "{call}: normal");
}

/// The origin is written all-bare because `point3` of MIXED dimensions
/// evaluates to Undef before any gate sees it (measured): the gate under test
/// is `accept_length_point3`'s per-component rejection.
#[test]
fn half_space_bare_origin_component_is_rejected_at_build() {
    let call = "half_space(point3(0, 0, 5), dir(0, 0, 1))";
    let (diagnostics, ops) = lenient_build(call);
    assert_rejected_naming(&diagnostics, &["origin.z", "half_space"]);
    assert!(
        !ops.iter()
            .any(|op| matches!(op, GeometryOp::HalfSpace { .. })),
        "{call}: a bare origin component must drop the half_space: {ops:?}"
    );
}

/// Retired `arc(0mm, 0mm, 0mm, 5mm, 0deg, 90deg, 0, 0, 1)` emitted
/// `Arc { center: [0, 0, 0], radius: 0.005, start_angle: 0, end_angle: π/2,
/// axis: [0, 0, 1] }`.
#[test]
fn arc_direction_form_emits_the_retired_scalar_op() {
    let call = "arc(point3(0mm, 0mm, 0mm), 5mm, 0deg, 90deg, dir(0, 0, 1))";
    let arcs: Vec<GeometryOp> = clean_ops(call)
        .into_iter()
        .filter(|op| matches!(op, GeometryOp::Arc { .. }))
        .collect();
    let [
        GeometryOp::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            axis,
        },
    ] = arcs.as_slice()
    else {
        panic!("{call}: expected exactly one Arc op, got {arcs:?}");
    };
    assert_eq!(*center, [0.0, 0.0, 0.0], "{call}: center");
    assert_eq!(*radius, 0.005, "{call}: radius");
    assert_eq!(*start_angle, 0.0, "{call}: start_angle");
    assert_eq!(*end_angle, std::f64::consts::FRAC_PI_2, "{call}: end_angle");
    assert_eq!(*axis, [0.0, 0.0, 1.0], "{call}: axis");
}

/// Every argument is read before any `?`, so a bare centre AND a bare radius
/// are reported in one build.
#[test]
fn arc_reports_a_bare_centre_and_a_bare_radius_in_one_build() {
    let call = "arc(point3(0, 0, 0), 5, 0deg, 90deg, dir(0, 0, 1))";
    let (diagnostics, ops) = lenient_build(call);
    for name in ["center.x", "center.y", "center.z", "radius"] {
        assert_rejected_naming(&diagnostics, &["arc", name]);
    }
    assert!(
        !ops.iter().any(|op| matches!(op, GeometryOp::Arc { .. })),
        "{call}: a bare centre and radius must drop the arc: {ops:?}"
    );
}

/// Retired `linear_pattern(box(2mm, 2mm, 2mm), 1, 0, 0, 3, 5mm)` emitted
/// `LinearPattern { direction: [1, 0, 0], count: 3, spacing: Scalar{0.005,
/// LENGTH} }`.
#[test]
fn linear_pattern_direction_form_emits_the_retired_scalar_op() {
    let call = "linear_pattern(box(2mm, 2mm, 2mm), dir(1, 0, 0), 3, 5mm)";
    let patterns: Vec<GeometryOp> = clean_ops(call)
        .into_iter()
        .filter(|op| matches!(op, GeometryOp::LinearPattern { .. }))
        .collect();
    let [
        GeometryOp::LinearPattern {
            direction,
            count,
            spacing,
            ..
        },
    ] = patterns.as_slice()
    else {
        panic!("{call}: expected exactly one LinearPattern op, got {patterns:?}");
    };
    assert_eq!(*direction, [1.0, 0.0, 0.0], "{call}: direction");
    assert_eq!(*count, 3, "{call}: count");
    assert_eq!(*spacing, Value::length(0.005), "{call}: spacing");
}

/// Retired `linear_pattern_2d(box(2mm, 2mm, 2mm), 1, 0, 0, 2, 5mm, 0, 1, 0, 2,
/// 5mm)` emitted `LinearPattern2D { direction1: [1, 0, 0], count1: 2,
/// spacing1: Scalar{0.005, LENGTH}, direction2: [0, 1, 0], count2: 2,
/// spacing2: Scalar{0.005, LENGTH} }`.
#[test]
fn linear_pattern_2d_direction_form_emits_the_retired_scalar_op() {
    let call = "linear_pattern_2d(box(2mm, 2mm, 2mm), dir(1, 0, 0), 2, 5mm, dir(0, 1, 0), 2, 5mm)";
    let grids: Vec<GeometryOp> = clean_ops(call)
        .into_iter()
        .filter(|op| matches!(op, GeometryOp::LinearPattern2D { .. }))
        .collect();
    let [
        GeometryOp::LinearPattern2D {
            direction1,
            count1,
            spacing1,
            direction2,
            count2,
            spacing2,
            ..
        },
    ] = grids.as_slice()
    else {
        panic!("{call}: expected exactly one LinearPattern2D op, got {grids:?}");
    };
    assert_eq!(*direction1, [1.0, 0.0, 0.0], "{call}: direction1");
    assert_eq!(*count1, 2, "{call}: count1");
    assert_eq!(*spacing1, Value::length(0.005), "{call}: spacing1");
    assert_eq!(*direction2, [0.0, 1.0, 0.0], "{call}: direction2");
    assert_eq!(*count2, 2, "{call}: count2");
    assert_eq!(*spacing2, Value::length(0.005), "{call}: spacing2");
}
