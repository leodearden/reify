//! End-to-end tests for the loft_guided() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_core::Type;
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

/// Exercises the full compile -> eval path for LoftGuided.
///
/// Creates a module with 4 ops:
///   Op 0: Sphere (profile 1 stand-in, handle at step 0)
///   Op 1: Sphere (profile 2 stand-in, handle at step 1)
///   Op 2: Sphere (guide stand-in, handle at step 2)
///   Op 3: Sweep(LoftGuided) with profiles vec [Step(0), Step(1), Step(2)],
///         where the last entry is the guide and the preceding entries are
///         the section profiles (matching the surface convention
///         `loft_guided(p1, p2, ..., guide)`).
///
/// Verifies that the eval layer splits the compiled profiles vec so that
/// `profiles` (all but last) populates `GeometryOp::LoftGuided::profiles`
/// and the final ref populates `guides` as a single-element vec.
#[test]
fn loft_guided_through_full_eval_pipeline() {
    let e = "TestLoftGuided";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (profile 1 stand-in, handle at step index 0)
    let sphere_op_0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Sphere (profile 2 stand-in, handle at step index 1)
    let sphere_op_1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(4.0))],
    };

    // Op 2: Sphere (guide stand-in, handle at step index 2)
    let sphere_op_2 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(3.0))],
    };

    // Op 3: LoftGuided with profiles = [Step(0), Step(1), Step(2)] — last
    // slot is the guide by convention.
    let loft_guided_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::LoftGuided,
        profiles: vec![GeomRef::Step(0), GeomRef::Step(1), GeomRef::Step(2)],
        args: vec![
            ("profile_0".into(), mm_literal(5.0)),
            ("profile_1".into(), mm_literal(4.0)),
            ("guide".into(), mm_literal(3.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(
            e,
            0,
            vec![sphere_op_0, sphere_op_1, sphere_op_2, loft_guided_op],
        )
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_loft_guided"))
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

    let profile_handle_0 = ops[0].result_handle;
    let profile_handle_1 = ops[1].result_handle;
    let guide_handle = ops[2].result_handle;

    match &ops[3].op {
        GeometryOp::LoftGuided { profiles, guides } => {
            assert_eq!(
                profiles,
                &vec![profile_handle_0, profile_handle_1],
                "LoftGuided profiles should be [handle(op 0), handle(op 1)]"
            );
            assert_eq!(
                guides,
                &vec![guide_handle],
                "LoftGuided guides should be [handle(op 2)]"
            );
        }
        other => panic!(
            "expected GeometryOp::LoftGuided at op index 3, got {:?}",
            other
        ),
    }
}
