//! End-to-end tests for the sweep_guided() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_core::Type;
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

/// Exercises the full compile -> eval path for SweepGuided.
///
/// Creates a module with 4 ops:
///   Op 0: Sphere (serves as a stand-in profile)
///   Op 1: Sphere (serves as a stand-in path/spine)
///   Op 2: Sphere (serves as a stand-in guide wire)
///   Op 3: Sweep(SweepGuided) referencing Step(0), Step(1), Step(2)
///
/// Verifies that the SweepGuided operation receives the correct profile,
/// path, and guide handles from ops 0, 1, and 2 respectively.
#[test]
fn sweep_guided_through_full_eval_pipeline() {
    let e = "TestSweepGuided";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (profile stand-in, handle at step index 0)
    let sphere_op_0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Sphere (path stand-in, handle at step index 1)
    let sphere_op_1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(3.0))],
    };

    // Op 2: Sphere (guide stand-in, handle at step index 2)
    let sphere_op_2 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(2.0))],
    };

    // SweepKind::SweepGuided carries all data in `profiles`; args is intentionally
    // empty — eval reads only profiles[0]/profiles[1]/profiles[2] (task-383 S4b / task-2122).
    // Op 3: SweepGuided referencing Step(0), Step(1), Step(2)
    let sweep_guided_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::SweepGuided,
        profiles: vec![GeomRef::Step(0), GeomRef::Step(1), GeomRef::Step(2)],
        args: vec![],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(
            e,
            0,
            vec![sphere_op_0, sphere_op_1, sphere_op_2, sweep_guided_op],
        )
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_sweep_guided"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        4,
        "expected 4 geometry operations, got {}",
        ops.len()
    );

    let profile_handle = ops[0].result_handle;
    let path_handle = ops[1].result_handle;
    let guide_handle = ops[2].result_handle;

    match &ops[3].op {
        GeometryOp::SweepGuided {
            profile,
            path,
            guide,
        } => {
            assert_eq!(
                *profile, profile_handle,
                "SweepGuided profile should be handle from op 0 ({:?}), got {:?}",
                profile_handle, profile
            );
            assert_eq!(
                *path, path_handle,
                "SweepGuided path should be handle from op 1 ({:?}), got {:?}",
                path_handle, path
            );
            assert_eq!(
                *guide, guide_handle,
                "SweepGuided guide should be handle from op 2 ({:?}), got {:?}",
                guide_handle, guide
            );
        }
        other => panic!(
            "expected GeometryOp::SweepGuided at op index 3, got {:?}",
            other
        ),
    }
}
