//! End-to-end tests for the extrude_symmetric() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_test_support::*;
use reify_types::{ExportFormat, GeometryOp, Type};

/// Exercises the full compile -> eval path for ExtrudeSymmetric.
///
/// Creates a module with 2 ops:
///   Op 0: Sphere (serves as a stand-in profile, produces a handle)
///   Op 1: Sweep(ExtrudeSymmetric) referencing Step(0) as profile,
///         with distance=10mm
///
/// Verifies that the ExtrudeSymmetric operation receives the correct
/// profile handle and a distance of ~0.01 m (10 mm SI).
#[test]
fn extrude_symmetric_through_full_eval_pipeline() {
    let e = "TestExtrudeSymmetric";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (produces handle at step index 0)
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: ExtrudeSymmetric referencing Step(0) as profile, distance = 10mm
    let extrude_sym_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeSymmetric,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)), // placeholder expr
            ("distance".into(), mm_literal(10.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_sym_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_extsym"))
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
        2,
        "expected 2 geometry operations, got {}",
        ops.len()
    );

    let profile_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::ExtrudeSymmetric { profile, distance } => {
            assert_eq!(
                *profile, profile_handle,
                "ExtrudeSymmetric profile should be handle from op 0 ({:?}), got {:?}",
                profile_handle, profile
            );
            let dist_si = distance
                .as_f64()
                .expect("distance should be a numeric value");
            assert!(
                (dist_si - 0.01).abs() < 1e-9,
                "ExtrudeSymmetric distance should be 0.01 m (10 mm SI), got {}",
                dist_si
            );
        }
        other => panic!(
            "expected GeometryOp::ExtrudeSymmetric at op index 1, got {:?}",
            other
        ),
    }
}

/// Non-finite distance (NaN) should be rejected by the eval layer: the
/// ExtrudeSymmetric op is dropped (not present in executed operations),
/// mirroring Extrude's degeneracy check.
#[test]
fn extrude_symmetric_non_finite_distance_is_dropped() {
    let e = "TestExtrudeSymNaN";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());
    // Construct a NaN Value::Scalar for distance.
    let nan_val = reify_types::Value::Scalar {
        si_value: f64::NAN,
        dimension: reify_types::DimensionVector::LENGTH,
    };
    let nan_expr = reify_types::CompiledExpr::literal(nan_val, Type::length());

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_sym_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeSymmetric,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), nan_expr),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_sym_op])
        .build();
    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_extsym_nan"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    // Sphere op should still execute, but ExtrudeSymmetric should be dropped.
    let any_extsym = ops
        .iter()
        .any(|o| matches!(o.op, GeometryOp::ExtrudeSymmetric { .. }));
    assert!(
        !any_extsym,
        "expected ExtrudeSymmetric to be dropped on non-finite distance, \
         but it was executed: {:?}",
        ops.iter().map(|o| &o.op).collect::<Vec<_>>()
    );
}
