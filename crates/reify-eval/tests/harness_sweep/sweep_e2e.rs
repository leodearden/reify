//! End-to-end tests for the sweep() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_core::Type;
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

/// Exercises the full compile -> eval path for Sweep.
///
/// Creates a module with 3 ops:
///   Op 0: Sphere (serves as a stand-in profile, produces a handle)
///   Op 1: Sphere (serves as a stand-in path, produces a handle)
///   Op 2: Sweep(Sweep) referencing Step(0) as profile and Step(1) as path
///
/// Verifies that the Sweep operation receives the correct profile handle
/// from step 0 and path handle from step 1.
#[test]
fn sweep_through_full_eval_pipeline() {
    let e = "TestSweep";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (profile stand-in, produces handle at step index 0)
    let sphere_op_0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Sphere (path stand-in, produces handle at step index 1)
    let sphere_op_1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(3.0))],
    };

    // Op 2: Sweep referencing Step(0) as profile, Step(1) as path.
    // SweepKind::Sweep carries all data in `profiles`; args is intentionally
    // empty — eval reads only profiles[0]/profiles[1] (task-383 S4b/S6).
    let sweep_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Sweep,
        profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
        args: vec![],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op_0, sphere_op_1, sweep_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_sweep"))
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
        3,
        "expected 3 geometry operations, got {}",
        ops.len()
    );

    let profile_handle = ops[0].result_handle;
    let path_handle = ops[1].result_handle;

    match &ops[2].op {
        GeometryOp::Sweep { profile, path } => {
            assert_eq!(
                *profile, profile_handle,
                "Sweep profile should be handle from op 0 ({:?}), got {:?}",
                profile_handle, profile
            );
            assert_eq!(
                *path, path_handle,
                "Sweep path should be handle from op 1 ({:?}), got {:?}",
                path_handle, path
            );
        }
        other => panic!("expected GeometryOp::Sweep at op index 2, got {:?}", other),
    }
}
