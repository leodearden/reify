//! End-to-end tests for the chamfer() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, GeomRef, ModifyKind, PrimitiveKind};
use reify_test_support::*;
use reify_types::{ExportFormat, GeometryOp, Type};

// ---------------------------------------------------------------------------
// step-1: Compiler rejects wrong arg counts, accepts correct count
// ---------------------------------------------------------------------------

/// chamfer() with 1 arg (missing distance) should produce diagnostics.
/// Before step-2, chamfer is not recognized at all and no realization is built.
#[test]
fn chamfer_compiler_rejects_one_arg() {
    let source = r#"structure S {
    param target: Scalar = 5mm
    let result = chamfer(target)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_ch1"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    let realizations = &template.realizations;
    let has_chamfer_op = realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Modify {
                    kind: reify_compiler::ModifyKind::Chamfer,
                    ..
                }
            )
        })
    });
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected error diagnostic for wrong arg count (1 arg)"
    );
    assert!(
        !has_chamfer_op,
        "should not produce Modify(Chamfer) op with wrong arg count (1 arg)"
    );
}

/// chamfer() with 3 args should produce diagnostics (too many args).
#[test]
fn chamfer_compiler_rejects_three_args() {
    let source = r#"structure S {
    param target: Scalar = 5mm
    param dist: Scalar = 2mm
    let result = chamfer(target, dist, dist)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_ch3"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    let realizations = &template.realizations;
    let has_chamfer_op = realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Modify {
                    kind: reify_compiler::ModifyKind::Chamfer,
                    ..
                }
            )
        })
    });
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected error diagnostic for wrong arg count (3 args)"
    );
    assert!(
        !has_chamfer_op,
        "should not produce Modify(Chamfer) op with wrong arg count (3 args)"
    );
}

/// chamfer() with correct 2 args should produce a Modify(Chamfer) realization.
/// This test fails before step-2 because chamfer is not recognized as a geometry function.
#[test]
fn chamfer_compiler_accepts_two_args() {
    let source = r#"structure S {
    param target: Scalar = 5mm
    param dist: Scalar = 2mm
    let result = chamfer(target, dist)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_ch2"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for chamfer call, got {}",
        template.realizations.len()
    );
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            reify_compiler::CompiledGeometryOp::Modify {
                kind: reify_compiler::ModifyKind::Chamfer,
                ..
            }
        ),
        "expected Modify(Chamfer), got {:?}",
        op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "expected no diagnostics for chamfer(target, dist), got: {:?}",
        compiled.diagnostics
    );
}

// ---------------------------------------------------------------------------
// step-3: Full eval pipeline test
// ---------------------------------------------------------------------------

/// Exercises the full compile -> eval path for Chamfer.
///
/// Creates a module with 2 ops:
///   Op 0: Box (produces a solid body, serves as chamfer target, produces a handle)
///   Op 1: Modify(Chamfer) referencing Step(0) as target, with distance=3mm
///
/// Verifies that the Chamfer operation receives the correct target handle
/// and a distance value of approximately 0.003 m (3 mm in SI).
///
/// This test should pass immediately since the eval layer already dispatches
/// ModifyKind::Chamfer correctly (reify-eval/src/lib.rs).
#[test]
fn chamfer_through_full_eval_pipeline() {
    let e = "TestChamfer";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Box (produces handle at step index 0)
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(20.0)),
            ("height".into(), mm_literal(20.0)),
            ("depth".into(), mm_literal(20.0)),
        ],
    };

    // Op 1: Chamfer referencing Step(0) as target, distance = 3mm
    let chamfer_op = CompiledGeometryOp::Modify {
        kind: ModifyKind::Chamfer,
        target: GeomRef::Step(0),
        args: vec![
            // Mirrors what the compiler emits in compile_geometry_call() for the "chamfer" match arm.
            // The eval layer resolves the target from GeomRef::Step(0), not from this entry.
            ("target".into(), mm_literal(20.0)),
            ("distance".into(), mm_literal(3.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![box_op, chamfer_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_chamfer"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);
    assert_no_error_diagnostics(&result.diagnostics, "chamfer full-pipeline build");

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 2, "expected 2 geometry operations, got {}", ops.len());

    let target_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::Chamfer { target, distance } => {
            assert_eq!(
                *target, target_handle,
                "Chamfer target should be handle from op 0 ({:?}), got {:?}",
                target_handle, target
            );
            let dist_si = distance.as_f64().expect("distance should be a numeric value");
            assert!(
                (dist_si - 0.003).abs() < 1e-9,
                "Chamfer distance should be 0.003 m (3 mm SI), got {}",
                dist_si
            );
        }
        other => panic!("expected GeometryOp::Chamfer at op index 1, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-4 (1819): Contract test — eval only needs 'distance' arg for Chamfer
// ---------------------------------------------------------------------------

/// Documents the minimal args contract for Chamfer: only 'distance' is needed in args.
///
/// The eval layer resolves the target handle from GeomRef::Step(0) on the Modify
/// variant, not from any 'target' entry in the args vec. This test constructs a
/// Chamfer op with ONLY ('distance', ...) in args (no 'target' entry at all) and
/// verifies that eval still produces the correct Chamfer geometry op.
#[test]
fn chamfer_modify_only_needs_distance_arg() {
    let e = "TestChamferMinimal";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Box (produces handle at step index 0)
    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_literal(10.0)),
            ("height".into(), mm_literal(10.0)),
            ("depth".into(), mm_literal(10.0)),
        ],
    };

    // Op 1: Chamfer with ONLY 'distance' in args — no 'target' entry.
    // "target" handle is resolved from GeomRef::Step(0), not from args.
    let chamfer_op = CompiledGeometryOp::Modify {
        kind: ModifyKind::Chamfer,
        target: GeomRef::Step(0),
        args: vec![("distance".into(), mm_literal(3.0))],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![box_op, chamfer_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_chamfer_minimal"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);
    assert_no_error_diagnostics(&result.diagnostics, "chamfer minimal build");

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 2, "expected 2 geometry operations, got {}", ops.len());

    let target_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::Chamfer { target, distance } => {
            assert_eq!(
                *target, target_handle,
                "Chamfer target should be handle from op 0 ({:?}), got {:?}",
                target_handle, target
            );
            let dist_si = distance.as_f64().expect("distance should be a numeric value");
            assert!(
                (dist_si - 0.003).abs() < 1e-9,
                "Chamfer distance should be 0.003 m (3 mm SI), got {}",
                dist_si
            );
        }
        other => panic!("expected GeometryOp::Chamfer at op index 1, got {:?}", other),
    }
}
