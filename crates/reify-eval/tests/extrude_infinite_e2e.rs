//! End-to-end tests for `extrude_infinite(profile, direction)` and
//! `extrude_infinite_symmetric(profile, direction)`.
//!
//! Tests span from `.ri` source through compilation and evaluation to the final
//! GeometryOp, using MockGeometryKernel to capture executed operations without
//! OCCT.

use reify_compiler::{CompiledGeometryOp, CompiledModule, SweepKind};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

fn compile(name: &str, call: &str) -> CompiledModule {
    let source = format!("structure S {{\n    let result = {call}\n}}");
    let parsed = reify_syntax::parse(&source, reify_core::ModulePath::single(name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

fn has_extrude_infinite_op(compiled: &CompiledModule) -> bool {
    compiled.templates[0].realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                CompiledGeometryOp::Sweep {
                    kind: SweepKind::ExtrudeInfinite,
                    ..
                }
            )
        })
    })
}

// ---------------------------------------------------------------------------
// Compiler-level tests
// ---------------------------------------------------------------------------

/// Both Direction forms lower to a Sweep(ExtrudeInfinite) with no diagnostics.
#[test]
fn extrude_infinite_compiler_accepts_the_two_arg_direction_forms() {
    for call in [
        "extrude_infinite(circle(5mm), dir(0, 0, 1))",
        "extrude_infinite_symmetric(circle(5mm), dir(0, 0, 1))",
    ] {
        let compiled = compile("test_ei2", call);
        let template = &compiled.templates[0];
        assert_eq!(template.realizations.len(), 1, "{call}");
        let last_op = template.realizations[0]
            .operations
            .last()
            .expect("expected at least one compiled op");
        assert!(
            matches!(
                last_op,
                CompiledGeometryOp::Sweep {
                    kind: SweepKind::ExtrudeInfinite,
                    ..
                }
            ),
            "{call}: expected Sweep(ExtrudeInfinite), got {last_op:?}"
        );
        assert!(
            compiled.diagnostics.is_empty(),
            "expected no diagnostics for {call}, got: {:?}",
            compiled.diagnostics
        );
    }
}

/// The retired 5-arg `(profile, dx, dy, dz, sense)` form and a 4-arg call are
/// arity errors naming the Direction form, and lower no op.
#[test]
fn extrude_infinite_compiler_rejects_the_retired_scalar_arities() {
    for (call, got) in [
        ("extrude_infinite(circle(5mm), 0, 0, 1, \"positive\")", 5),
        ("extrude_infinite(circle(5mm), 0, 0, 1)", 4),
    ] {
        let compiled = compile("test_ei_retired", call);
        assert!(
            compiled.diagnostics.iter().any(|d| d
                .message
                .starts_with("extrude_infinite() expects")
                && d.message.contains("dir(x, y, z)")
                && d.message.ends_with(&format!(", got {got}"))),
            "{call}: expected an arity error naming the Direction form, got: {:?}",
            compiled.diagnostics
        );
        assert!(
            !has_extrude_infinite_op(&compiled),
            "{call}: a wrong arg count must lower no Sweep(ExtrudeInfinite) op"
        );
    }
}

// ---------------------------------------------------------------------------
// Full compile→eval pipeline tests
// ---------------------------------------------------------------------------

/// Builds `call` from source; returns the emitted ExtrudeInfinite's
/// `(axis, both)`, checking its profile is the circle the call emitted first.
fn build_extrude_infinite(call: &str) -> ([f64; 3], bool) {
    let module = compile("test_ei_pipeline", call);
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(
        reify_eval::SolverPosture::SolverFree,
        Box::new(checker),
        Some(Box::new(kernel)),
    );
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "{call}: expected 2 ops (circle + extrude_infinite), got {}",
        ops.len()
    );
    let profile_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::ExtrudeInfinite {
            profile,
            axis,
            both,
        } => {
            assert_eq!(*profile, profile_handle, "{call}: profile handle mismatch");
            (*axis, *both)
        }
        other => panic!("{call}: expected GeometryOp::ExtrudeInfinite at op[1], got {other:?}"),
    }
}

/// `dir(0, 0, 1)` is the retired `0, 0, 1, "positive"`: axis [0, 0, 1], both=false.
#[test]
fn extrude_infinite_through_full_eval_pipeline_positive() {
    assert_eq!(
        build_extrude_infinite("extrude_infinite(circle(5mm), dir(0, 0, 1))"),
        ([0.0, 0.0, 1.0], false)
    );
}

/// `dir(0, 0, -1)` is the retired `0, 0, 1, "negative"`: axis [0, 0, -1], both=false.
#[test]
fn extrude_infinite_through_full_eval_pipeline_negative() {
    assert_eq!(
        build_extrude_infinite("extrude_infinite(circle(5mm), dir(0, 0, -1))"),
        ([0.0, 0.0, -1.0], false)
    );
}

/// `extrude_infinite_symmetric` is the retired `0, 0, 1, "both"`: axis
/// [0, 0, 1] (unnegated), both=true.
#[test]
fn extrude_infinite_through_full_eval_pipeline_both() {
    assert_eq!(
        build_extrude_infinite("extrude_infinite_symmetric(circle(5mm), dir(0, 0, 1))"),
        ([0.0, 0.0, 1.0], true)
    );
}
