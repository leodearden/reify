//! Stress tests for degenerate geometry sweep operations.
//!
//! Covers:
//!   - zero_extrude_distance: evaluator skips extrude when distance=0
//!   - revolve_zero_angle: evaluator skips revolve when angle=0
//!   - revolve_720_degrees: valid edge-case double full revolution (4π)
//!   - loft_one_profile_rejected: compiler rejects loft with < 2 profiles
//!   - self_intersecting_path_sweep: kernel failure produces diagnostic
//!   - sweep_degenerate_ri_parses: fixture parses and compiles without errors

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_test_support::*;
use reify_types::{ExportFormat, GeometryOp, ModulePath, Type};

// ---------------------------------------------------------------------------
// step-13: zero_extrude_distance — failing test
// ---------------------------------------------------------------------------

/// Build an Extrude op with distance=0mm (degenerate).
/// After step-14 the evaluator should skip zero-distance extrudes
/// (compile_geometry_op returns None), so no Extrude op reaches the kernel.
///
/// FAILS before step-14 because the evaluator currently dispatches all extrudes.
#[test]
fn zero_extrude_distance() {
    let e = "TestZeroExtrude";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (profile provider at Step(0))
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Extrude referencing Step(0), distance = 0mm (degenerate)
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), mm_literal(0.0)), // ZERO — degenerate
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_zero_extrude"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    // Zero-distance extrude should be skipped — kernel should receive no Extrude ops.
    let extrude_ops: Vec<_> = ops
        .iter()
        .filter(|o| matches!(&o.op, GeometryOp::Extrude { .. }))
        .collect();
    assert!(
        extrude_ops.is_empty(),
        "zero-distance extrude should be skipped by the evaluator, \
         but kernel received {} Extrude op(s): {:?}",
        extrude_ops.len(),
        extrude_ops.iter().map(|o| &o.op).collect::<Vec<_>>()
    );
}
