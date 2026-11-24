//! The Direction-taking sweeps (task ι-b #9647, PRD
//! `docs/prds/v0_6/direction-typing.md` B12′), compiled from `.ri` SOURCE and
//! built against a mock kernel.
//!
//! B12′ "same IR": each Direction form emits exactly the `GeometryOp` its
//! retired scalar form emitted. The expected values are the scalar forms' IR
//! literals, compared with `==` — `dir(0, 0, -1)` is `[0.0, 0.0, -1.0]` where
//! the retired `"negative"` fold of `0, 0, 1` produced `[-0.0, -0.0, -1.0]`;
//! the two are equal under IEEE `==` and denote the same axis.
//!
//! The Point3 origin is not a compile-time slot (no `ExpectedArg` kind holds a
//! Point3), so a bare origin is gated at BUILD, under the retired `ox`/`oy`/`oz`
//! names.

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_eval::{BuildResult, Engine};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, compile_source, parse_and_compile,
};

const PROFILE: &str = "rectangle(20mm, 10mm)";

/// The migration hint `arg_acceptance::length_spec` mints.
const LENGTH_HINT: &str = "pass a dimensioned length such as `5mm`";

fn structure_source(lets: &str) -> String {
    format!("structure def SweepDirectionForms {{\n{lets}\n}}\n")
}

/// Builds `compiled` against a mock kernel; returns the diagnostics and every
/// emitted op.
fn build(compiled: &reify_compiler::CompiledModule) -> (Vec<Diagnostic>, Vec<GeometryOp>) {
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );
    let result: BuildResult = engine.build(compiled, ExportFormat::Step);
    let ops = ops_ref
        .lock()
        .unwrap()
        .iter()
        .map(|record| record.op.clone())
        .collect();
    (result.diagnostics, ops)
}

/// The single sweep op `call` emits, built from strict-compiled source with no
/// Error diagnostics.
fn the_sweep_op(call: &str) -> GeometryOp {
    let (diagnostics, ops) = build(&parse_and_compile(&structure_source(&format!(
        "    let g = {call}"
    ))));
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{call}: {errors:?}");
    let sweeps: Vec<GeometryOp> = ops
        .into_iter()
        .filter(|op| {
            matches!(
                op,
                GeometryOp::Revolve { .. } | GeometryOp::ExtrudeInfinite { .. }
            )
        })
        .collect();
    assert_eq!(sweeps.len(), 1, "{call}: {sweeps:?}");
    sweeps.into_iter().next().unwrap()
}

#[track_caller]
fn assert_revolve_ir(call: &str, origin: [f64; 3], dir: [f64; 3], angle: f64) {
    match the_sweep_op(call) {
        GeometryOp::Revolve {
            axis_origin,
            axis_dir,
            angle_rad,
            ..
        } => {
            assert_eq!(axis_origin, origin, "{call}: axis_origin");
            assert_eq!(axis_dir, dir, "{call}: axis_dir");
            assert_eq!(angle_rad, angle, "{call}: angle_rad");
        }
        other => panic!("{call}: expected a Revolve, got {other:?}"),
    }
}

#[track_caller]
fn assert_extrude_infinite_ir(call: &str, expected_axis: [f64; 3], expected_both: bool) {
    match the_sweep_op(call) {
        GeometryOp::ExtrudeInfinite { axis, both, .. } => {
            assert_eq!(axis, expected_axis, "{call}: axis");
            assert_eq!(both, expected_both, "{call}: both");
        }
        other => panic!("{call}: expected an ExtrudeInfinite, got {other:?}"),
    }
}

#[test]
fn revolve_direction_form_emits_the_retired_scalar_ir() {
    assert_revolve_ir(
        &format!(
            "revolve({PROFILE}, point3(-10mm, 0mm, 0mm), dir(0.0, 1.0, 0.0), 6.283185307179586rad)"
        ),
        [-0.01, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        std::f64::consts::TAU,
    );
}

#[test]
fn revolve_axis_form_emits_the_same_axis() {
    assert_revolve_ir(
        &format!("revolve({PROFILE}, axis_y(point3(-10mm, 0mm, 0mm)), 90deg)"),
        [-0.01, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        std::f64::consts::FRAC_PI_2,
    );
}

#[test]
fn revolve_full_direction_form_emits_a_tau_revolve() {
    assert_revolve_ir(
        &format!("revolve_full({PROFILE}, point3(-10mm, 0mm, 0mm), dir(0, 1, 0))"),
        [-0.01, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        std::f64::consts::TAU,
    );
}

#[test]
fn extrude_infinite_direction_forms_emit_the_retired_sense_ir() {
    // The retired `0, 0, 1, "positive"`.
    assert_extrude_infinite_ir(
        "extrude_infinite(circle(5mm), dir(0, 0, 1))",
        [0.0, 0.0, 1.0],
        false,
    );
    // The retired `0, 0, 1, "negative"`, which folded to [-0.0, -0.0, -1.0].
    assert_extrude_infinite_ir(
        "extrude_infinite(circle(5mm), dir(0, 0, -1))",
        [-0.0, -0.0, -1.0],
        false,
    );
    // The retired `0, 0, 1, "both"`.
    assert_extrude_infinite_ir(
        "extrude_infinite_symmetric(circle(5mm), dir(0, 0, 1))",
        [0.0, 0.0, 1.0],
        true,
    );
}

/// The accepted narrowing: a bare origin is no longer a compile-time slot, so
/// the build rejects it under the retired component names and drops the op.
#[test]
fn a_bare_revolve_origin_is_rejected_at_build_naming_the_component() {
    let source = structure_source(&format!(
        "    let g = revolve({PROFILE}, point3(-10, 0, 0), dir(0, 1, 0), 90deg)"
    ));
    let (diagnostics, ops) = build(&compile_source(&source));
    assert!(
        diagnostics.iter().any(|d| d.severity == Severity::Error
            && d.message.contains("ox")
            && d.message.contains("Length")
            && d.message.contains(LENGTH_HINT)),
        "a bare origin must be an Error naming `ox`, Length and the hint: {diagnostics:?}"
    );
    assert!(
        !ops.iter().any(|op| matches!(op, GeometryOp::Revolve { .. })),
        "a bare origin must drop the revolve: {ops:?}"
    );
}

fn assert_degenerate_direction_drops_the_revolve(lets: &str) {
    let (diagnostics, ops) = build(&compile_source(&structure_source(lets)));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::DegenerateDirection)),
        "{lets}: a dir(0, 0, 0) must surface E_DegenerateDirection: {diagnostics:?}"
    );
    assert!(
        !ops.iter().any(|op| matches!(op, GeometryOp::Revolve { .. })),
        "{lets}: a degenerate direction must drop the revolve: {ops:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|d| d.message.contains("degenerate magnitude")),
        "{lets}: the retired eval magnitude Warning must be gone: {diagnostics:?}"
    );
}

#[test]
fn a_let_bound_degenerate_direction_drops_the_revolve() {
    assert_degenerate_direction_drops_the_revolve(&format!(
        "    let d = dir(0, 0, 0)\n    let g = revolve({PROFILE}, point3(-10mm, 0mm, 0mm), d, 90deg)"
    ));
}

#[test]
fn an_inline_degenerate_direction_drops_the_revolve() {
    assert_degenerate_direction_drops_the_revolve(&format!(
        "    let g = revolve({PROFILE}, point3(-10mm, 0mm, 0mm), dir(0, 0, 0), 90deg)"
    ));
}
