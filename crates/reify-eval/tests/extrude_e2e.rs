//! End-to-end tests for the extrude() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_test_support::*;
use reify_types::{ExportFormat, GeometryOp, Type};

/// Exercises the full compile -> eval path for Extrude.
///
/// Creates a module with 2 ops:
///   Op 0: Sphere (serves as a stand-in profile, produces a handle)
///   Op 1: Sweep(Extrude) referencing Step(0) as profile, with distance=10mm
///
/// Verifies that the Extrude operation receives the correct profile handle
/// and a distance value of approximately 0.01 m (10 mm in SI).
#[test]
fn extrude_through_full_eval_pipeline() {
    let e = "TestExtrude";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (produces handle at step index 0)
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Extrude referencing Step(0) as profile, distance = 10mm
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)), // placeholder expr
            ("distance".into(), mm_literal(10.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_extrude"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 2, "expected 2 geometry operations, got {}", ops.len());

    let profile_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::Extrude { profile, distance } => {
            assert_eq!(
                *profile, profile_handle,
                "Extrude profile should be handle from op 0 ({:?}), got {:?}",
                profile_handle, profile
            );
            let dist_si = distance.as_f64().expect("distance should be a numeric value");
            assert!(
                (dist_si - 0.01).abs() < 1e-9,
                "Extrude distance should be 0.01 m (10 mm SI), got {}",
                dist_si
            );
        }
        other => panic!("expected GeometryOp::Extrude at op index 1, got {:?}", other),
    }
}
