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

#[test]
fn half_space_bare_origin_component_is_rejected_at_build() {
    let call = "half_space(point3(0mm, 0mm, 5), dir(0, 0, 1))";
    let (diagnostics, ops) = lenient_build(call);
    assert_rejected_naming(&diagnostics, &["origin.z", "half_space"]);
    assert!(
        !ops.iter()
            .any(|op| matches!(op, GeometryOp::HalfSpace { .. })),
        "{call}: a bare origin component must drop the half_space: {ops:?}"
    );
}
