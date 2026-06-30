//! End-to-end tests for the extrude_infinite() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.
//!
//! Step-3 tests (compiler-level): RED until step-4 adds SweepKind::ExtrudeInfinite
//! and the compile arm in geometry.rs.
//! Step-5 tests (full eval pipeline): added in the step-5 RED commit; RED until
//! step-6 adds GeometryOp::ExtrudeInfinite and the eval producer arm.

use reify_compiler::{CompiledGeometryOp, SweepKind};

// ---------------------------------------------------------------------------
// Step-3 compiler-level tests
// ---------------------------------------------------------------------------

/// extrude_infinite() with correct 5 args (profile, dx, dy, dz, direction)
/// should produce a Sweep(ExtrudeInfinite) realization with no diagnostics.
///
/// RED until step-4 adds SweepKind::ExtrudeInfinite to types.rs and the
/// compile arm in geometry.rs.
#[test]
fn extrude_infinite_compiler_accepts_five_args() {
    let source = r#"structure S {
    let result = extrude_infinite(circle(5mm), 0, 0, 1, "positive")
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ei5"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(
        template.realizations.len(),
        1,
        "expected 1 realization for extrude_infinite call, got {}",
        template.realizations.len()
    );
    // The last operation in the realization should be the Sweep(ExtrudeInfinite)
    let ops = &template.realizations[0].operations;
    let last_op = ops.last().expect("expected at least one compiled op");
    assert!(
        matches!(
            last_op,
            reify_compiler::CompiledGeometryOp::Sweep {
                kind: reify_compiler::SweepKind::ExtrudeInfinite,
                ..
            }
        ),
        "expected Sweep(ExtrudeInfinite), got {:?}",
        last_op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "expected no diagnostics for extrude_infinite(circle(5mm), 0, 0, 1, \"positive\"), got: {:?}",
        compiled.diagnostics
    );
}

/// extrude_infinite() with 2 args should produce an error diagnostic.
/// (Too few args: missing dx, dy, dz, direction.)
///
/// RED until step-4 adds the compile arm with arg-count validation.
#[test]
fn extrude_infinite_compiler_rejects_two_args() {
    let source = r#"structure S {
    let result = extrude_infinite(circle(5mm), 1)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ei2"));
    let compiled = reify_compiler::compile(&parsed);
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected error diagnostic for extrude_infinite with 2 args (wrong arg count)"
    );
    let has_infinite_op = compiled.templates[0].realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Sweep {
                    kind: reify_compiler::SweepKind::ExtrudeInfinite,
                    ..
                }
            )
        })
    });
    assert!(
        !has_infinite_op,
        "should not produce Sweep(ExtrudeInfinite) op with wrong arg count (2 args)"
    );
}

/// extrude_infinite() with 4 args should produce an error diagnostic.
/// (Missing direction string.)
///
/// RED until step-4 adds the compile arm with arg-count validation.
#[test]
fn extrude_infinite_compiler_rejects_four_args() {
    let source = r#"structure S {
    let result = extrude_infinite(circle(5mm), 0, 0, 1)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ei4"));
    let compiled = reify_compiler::compile(&parsed);
    assert!(
        !compiled.diagnostics.is_empty(),
        "expected error diagnostic for extrude_infinite with 4 args (wrong arg count)"
    );
    let has_infinite_op = compiled.templates[0].realizations.iter().any(|r| {
        r.operations.iter().any(|op| {
            matches!(
                op,
                reify_compiler::CompiledGeometryOp::Sweep {
                    kind: reify_compiler::SweepKind::ExtrudeInfinite,
                    ..
                }
            )
        })
    });
    assert!(
        !has_infinite_op,
        "should not produce Sweep(ExtrudeInfinite) op with wrong arg count (4 args)"
    );
}

// ---------------------------------------------------------------------------
// Step-5 full compile→eval pipeline tests (RED until step-6)
// ---------------------------------------------------------------------------

use reify_compiler::{GeomRef, PrimitiveKind};
use reify_core::Type;
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

/// Exercises the full compile -> eval path for ExtrudeInfinite with direction="positive".
///
/// Op 0: Circle profile at Step(0).
/// Op 1: Sweep(ExtrudeInfinite) with axis=(0,0,1), direction="positive"
///        → axis stays [0,0,1], both=false.
///
/// RED until step-6 adds GeometryOp::ExtrudeInfinite and the eval producer arm.
#[test]
fn extrude_infinite_through_full_eval_pipeline_positive() {
    let e = "TestExtrudeInfinitePositive";
    let dimensionless = |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::dimensionless_scalar());
    let string_lit = |s: &str| reify_ir::CompiledExpr::literal(reify_ir::Value::String(s.to_string()), Type::String);
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let circle_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    let extrude_infinite_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeInfinite,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("dx".into(), dimensionless(0.0)),
            ("dy".into(), dimensionless(0.0)),
            ("dz".into(), dimensionless(1.0)),
            ("direction".into(), string_lit("positive")),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![circle_op, extrude_infinite_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_ei_positive"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 2, "expected 2 ops (circle + extrude_infinite), got {}", ops.len());

    let profile_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::ExtrudeInfinite { profile, axis, both } => {
            assert_eq!(*profile, profile_handle, "profile handle mismatch");
            assert!((axis[0]).abs() < 1e-9, "axis[0] should be ≈ 0 for positive, got {}", axis[0]);
            assert!((axis[1]).abs() < 1e-9, "axis[1] should be ≈ 0 for positive, got {}", axis[1]);
            assert!((axis[2] - 1.0).abs() < 1e-9, "axis[2] should be ≈ 1 for positive, got {}", axis[2]);
            assert!(!both, "both should be false for direction=\"positive\"");
        }
        other => panic!("expected GeometryOp::ExtrudeInfinite at op[1], got {:?}", other),
    }
}

/// direction="negative" → axis ≈ [0, 0, -1], both=false.
///
/// RED until step-6 adds the eval producer arm.
#[test]
fn extrude_infinite_through_full_eval_pipeline_negative() {
    let e = "TestExtrudeInfiniteNegative";
    let dimensionless = |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::dimensionless_scalar());
    let string_lit = |s: &str| reify_ir::CompiledExpr::literal(reify_ir::Value::String(s.to_string()), Type::String);
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let circle_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_infinite_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeInfinite,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("dx".into(), dimensionless(0.0)),
            ("dy".into(), dimensionless(0.0)),
            ("dz".into(), dimensionless(1.0)),
            ("direction".into(), string_lit("negative")),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![circle_op, extrude_infinite_op])
        .build();
    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_ei_negative"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 2, "expected 2 ops, got {}", ops.len());

    match &ops[1].op {
        GeometryOp::ExtrudeInfinite { axis, both, .. } => {
            assert!((axis[2] - (-1.0)).abs() < 1e-9, "axis[2] should be ≈ -1 for negative, got {}", axis[2]);
            assert!(!both, "both should be false for direction=\"negative\"");
        }
        other => panic!("expected GeometryOp::ExtrudeInfinite, got {:?}", other),
    }
}

/// direction="both" → both=true, axis ≈ [0, 0, 1] (unnegated).
///
/// RED until step-6 adds the eval producer arm.
#[test]
fn extrude_infinite_through_full_eval_pipeline_both() {
    let e = "TestExtrudeInfiniteBoth";
    let dimensionless = |v: f64| reify_ir::CompiledExpr::literal(reify_ir::Value::Real(v), Type::dimensionless_scalar());
    let string_lit = |s: &str| reify_ir::CompiledExpr::literal(reify_ir::Value::String(s.to_string()), Type::String);
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let circle_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_infinite_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeInfinite,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("dx".into(), dimensionless(0.0)),
            ("dy".into(), dimensionless(0.0)),
            ("dz".into(), dimensionless(1.0)),
            ("direction".into(), string_lit("both")),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![circle_op, extrude_infinite_op])
        .build();
    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_ei_both"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 2, "expected 2 ops, got {}", ops.len());

    match &ops[1].op {
        GeometryOp::ExtrudeInfinite { axis, both, .. } => {
            assert!((axis[2] - 1.0).abs() < 1e-9, "axis[2] should be ≈ 1 for both, got {}", axis[2]);
            assert!(*both, "both should be true for direction=\"both\"");
        }
        other => panic!("expected GeometryOp::ExtrudeInfinite, got {:?}", other),
    }
}
