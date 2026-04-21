//! End-to-end tests for the tube() and pipe() geometry operations.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_test_support::*;
use reify_types::{ExportFormat, GeometryOp, Type};

/// Exercises the full compile -> eval path for Tube.
///
/// Creates a module with a single CompiledGeometryOp::Primitive { kind: Tube }
/// carrying outer_r=10mm, inner_r=5mm, height=20mm, then runs it through
/// Engine::build with MockGeometryKernel and asserts the captured runtime op
/// is GeometryOp::Tube with SI values 0.010, 0.005, 0.020 m.
#[test]
fn tube_through_mock_kernel_emits_geometry_op_tube() {
    let e = "TestTube";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    let tube_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Tube,
        args: vec![
            ("outer_r".into(), mm_literal(10.0)),
            ("inner_r".into(), mm_literal(5.0)),
            ("height".into(), mm_literal(20.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![tube_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_tube"))
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
        1,
        "expected 1 geometry operation, got {}",
        ops.len()
    );

    match &ops[0].op {
        GeometryOp::Tube {
            outer_r,
            inner_r,
            height,
        } => {
            let outer_si = outer_r.as_f64().expect("outer_r should be numeric");
            let inner_si = inner_r.as_f64().expect("inner_r should be numeric");
            let height_si = height.as_f64().expect("height should be numeric");
            assert!(
                (outer_si - 0.010).abs() < 1e-9,
                "Tube outer_r should be 0.010 m (10 mm SI), got {}",
                outer_si
            );
            assert!(
                (inner_si - 0.005).abs() < 1e-9,
                "Tube inner_r should be 0.005 m (5 mm SI), got {}",
                inner_si
            );
            assert!(
                (height_si - 0.020).abs() < 1e-9,
                "Tube height should be 0.020 m (20 mm SI), got {}",
                height_si
            );
        }
        other => panic!("expected GeometryOp::Tube at op index 0, got {:?}", other),
    }
}
