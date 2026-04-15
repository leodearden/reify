//! End-to-end tests for curve constructor geometry operations.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, CurveKind};
use reify_test_support::*;
use reify_types::{ExportFormat, GeometryOp, Type};

// ---------------------------------------------------------------------------
// Compiler: line_segment recognized and produces correct Curve op
// ---------------------------------------------------------------------------

#[test]
fn line_segment_compiler_accepts_6_args() {
    let source = r#"structure S {
    let wire = line_segment(0mm, 0mm, 0mm, 10mm, 0mm, 0mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_ls"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(op, CompiledGeometryOp::Curve { kind: CurveKind::LineSegment, .. }),
        "expected Curve(LineSegment), got {:?}", op
    );
    assert!(compiled.diagnostics.is_empty(), "unexpected diagnostics: {:?}", compiled.diagnostics);
}

#[test]
fn line_segment_compiler_rejects_wrong_arg_count() {
    let source = r#"structure S {
    let wire = line_segment(0mm, 0mm, 0mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_ls_bad"));
    let compiled = reify_compiler::compile(&parsed);
    assert!(!compiled.diagnostics.is_empty(), "expected diagnostic for wrong arg count");
}

// ---------------------------------------------------------------------------
// Eval pipeline: line_segment through full eval produces correct GeometryOp
// ---------------------------------------------------------------------------

#[test]
fn line_segment_through_full_eval_pipeline() {
    let e = "TestLineSegment";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    let curve_op = CompiledGeometryOp::Curve {
        kind: CurveKind::LineSegment,
        args: vec![
            ("x1".into(), mm_literal(0.0)),
            ("y1".into(), mm_literal(0.0)),
            ("z1".into(), mm_literal(0.0)),
            ("x2".into(), mm_literal(10.0)),
            ("y2".into(), mm_literal(0.0)),
            ("z2".into(), mm_literal(0.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![curve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_ls_eval"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 1, "expected 1 geometry operation, got {}", ops.len());

    match &ops[0].op {
        GeometryOp::LineSegment { x1, y1, z1, x2, y2, z2 } => {
            assert!((x1 - 0.0).abs() < 1e-9);
            assert!((y1 - 0.0).abs() < 1e-9);
            assert!((z1 - 0.0).abs() < 1e-9);
            // 10mm = 0.01m
            assert!((x2 - 0.01).abs() < 1e-9, "expected 0.01, got {}", x2);
            assert!((y2 - 0.0).abs() < 1e-9);
            assert!((z2 - 0.0).abs() < 1e-9);
        }
        other => panic!("expected GeometryOp::LineSegment, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Compiler: arc, helix recognized
// ---------------------------------------------------------------------------

#[test]
fn arc_compiler_accepts_9_args() {
    let source = r#"structure S {
    let wire = arc(0mm, 0mm, 0mm, 10mm, 0rad, 1.5708rad, 0mm, 0mm, 1mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_arc"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(op, CompiledGeometryOp::Curve { kind: CurveKind::Arc, .. }),
        "expected Curve(Arc), got {:?}", op
    );
}

#[test]
fn helix_compiler_accepts_3_args() {
    let source = r#"structure S {
    let wire = helix(5mm, 2mm, 20mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test_helix"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1);
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(op, CompiledGeometryOp::Curve { kind: CurveKind::Helix, .. }),
        "expected Curve(Helix), got {:?}", op
    );
}

// ---------------------------------------------------------------------------
// Eval pipeline: arc through full eval
// ---------------------------------------------------------------------------

#[test]
fn arc_through_full_eval_pipeline() {
    let e = "TestArc";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());
    let rad_literal = |v: f64| {
        reify_types::CompiledExpr::literal(
            reify_types::Value::Scalar {
                si_value: v,
                dimension: reify_types::DimensionVector::ANGLE,
            },
            Type::angle(),
        )
    };

    let curve_op = CompiledGeometryOp::Curve {
        kind: CurveKind::Arc,
        args: vec![
            ("cx".into(), mm_literal(0.0)),
            ("cy".into(), mm_literal(0.0)),
            ("cz".into(), mm_literal(0.0)),
            ("radius".into(), mm_literal(10.0)),
            ("start_angle".into(), rad_literal(0.0)),
            ("end_angle".into(), rad_literal(std::f64::consts::FRAC_PI_2)),
            ("ax".into(), mm_literal(0.0)),
            ("ay".into(), mm_literal(0.0)),
            ("az".into(), mm_literal(1.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![curve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_arc_eval"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 1, "expected 1 geometry operation, got {}", ops.len());

    match &ops[0].op {
        GeometryOp::Arc { center, radius, start_angle, end_angle, axis } => {
            assert!((center[0]).abs() < 1e-9);
            assert!((*radius - 0.01).abs() < 1e-9, "expected 0.01m, got {}", radius);
            assert!((*start_angle).abs() < 1e-9);
            assert!((*end_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9);
            assert!(axis[2].abs() > 1e-9, "z-axis should be non-zero");
        }
        other => panic!("expected GeometryOp::Arc, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Eval pipeline: helix, interp, bezier through full eval
// ---------------------------------------------------------------------------

#[test]
fn helix_through_full_eval_pipeline() {
    let e = "TestHelix";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    let curve_op = CompiledGeometryOp::Curve {
        kind: CurveKind::Helix,
        args: vec![
            ("radius".into(), mm_literal(5.0)),
            ("pitch".into(), mm_literal(2.0)),
            ("height".into(), mm_literal(20.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![curve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_helix_eval"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 1);
    match &ops[0].op {
        GeometryOp::Helix { radius, pitch, height } => {
            assert!((*radius - 0.005).abs() < 1e-9);
            assert!((*pitch - 0.002).abs() < 1e-9);
            assert!((*height - 0.02).abs() < 1e-9);
        }
        other => panic!("expected GeometryOp::Helix, got {:?}", other),
    }
}

#[test]
fn interp_through_full_eval_pipeline() {
    let e = "TestInterp";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // 4 points = 12 coordinate args
    let curve_op = CompiledGeometryOp::Curve {
        kind: CurveKind::InterpCurve,
        args: vec![
            ("c0".into(), mm_literal(0.0)),
            ("c1".into(), mm_literal(0.0)),
            ("c2".into(), mm_literal(0.0)),
            ("c3".into(), mm_literal(10.0)),
            ("c4".into(), mm_literal(10.0)),
            ("c5".into(), mm_literal(0.0)),
            ("c6".into(), mm_literal(20.0)),
            ("c7".into(), mm_literal(0.0)),
            ("c8".into(), mm_literal(0.0)),
            ("c9".into(), mm_literal(30.0)),
            ("c10".into(), mm_literal(10.0)),
            ("c11".into(), mm_literal(0.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![curve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_interp_eval"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 1);
    match &ops[0].op {
        GeometryOp::InterpCurve { points } => {
            assert_eq!(points.len(), 4, "expected 4 points, got {}", points.len());
            assert!((points[0][0]).abs() < 1e-9); // 0mm
            assert!((points[1][0] - 0.01).abs() < 1e-9); // 10mm
        }
        other => panic!("expected GeometryOp::InterpCurve, got {:?}", other),
    }
}

#[test]
fn bezier_through_full_eval_pipeline() {
    let e = "TestBezier";
    let mm_literal = |v: f64| reify_types::CompiledExpr::literal(mm(v), Type::length());

    // 4 control points = 12 coordinate args
    let curve_op = CompiledGeometryOp::Curve {
        kind: CurveKind::BezierCurve,
        args: vec![
            ("c0".into(), mm_literal(0.0)),
            ("c1".into(), mm_literal(0.0)),
            ("c2".into(), mm_literal(0.0)),
            ("c3".into(), mm_literal(10.0)),
            ("c4".into(), mm_literal(20.0)),
            ("c5".into(), mm_literal(0.0)),
            ("c6".into(), mm_literal(30.0)),
            ("c7".into(), mm_literal(20.0)),
            ("c8".into(), mm_literal(0.0)),
            ("c9".into(), mm_literal(40.0)),
            ("c10".into(), mm_literal(0.0)),
            ("c11".into(), mm_literal(0.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![curve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_bezier_eval"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 1);
    match &ops[0].op {
        GeometryOp::BezierCurve { control_points } => {
            assert_eq!(control_points.len(), 4, "expected 4 control points");
            assert!((control_points[0][0]).abs() < 1e-9);
            assert!((control_points[3][0] - 0.04).abs() < 1e-9); // 40mm
        }
        other => panic!("expected GeometryOp::BezierCurve, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// NURBS error-diagnostics tests (review feedback)
// ---------------------------------------------------------------------------

#[test]
fn nurbs_fewer_than_2_args_emits_diagnostic() {
    let e = "TestNurbsTooFew";
    let dim_literal = |v: f64| reify_types::CompiledExpr::literal(
        reify_types::Value::Scalar {
            si_value: v,
            dimension: reify_types::DimensionVector::DIMENSIONLESS,
        },
        Type::dimensionless_scalar(),
    );

    // Only pass 1 arg (just degree=3) — needs at least degree + n_points
    let curve_op = CompiledGeometryOp::Curve {
        kind: CurveKind::NurbsCurve,
        args: vec![
            ("c0".into(), dim_literal(3.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![curve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_nurbs_too_few"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // Should produce NO geometry ops (NURBS eval returns None)
    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 0, "expected no geometry ops for invalid NURBS, got {}", ops.len());

    // Should emit a diagnostic error
    let diag_messages: Vec<String> = result.diagnostics.iter().map(|d| d.message.clone()).collect();
    assert!(
        diag_messages.iter().any(|m| m.contains("nurbs() requires at least degree and n_points arguments")),
        "expected diagnostic about missing NURBS args, got: {:?}", diag_messages,
    );
}

#[test]
fn nurbs_insufficient_coordinate_args_emits_diagnostic() {
    let e = "TestNurbsShortCoords";
    let dim_literal = |v: f64| reify_types::CompiledExpr::literal(
        reify_types::Value::Scalar {
            si_value: v,
            dimension: reify_types::DimensionVector::DIMENSIONLESS,
        },
        Type::dimensionless_scalar(),
    );

    // degree=3, n_points=4, but only provide 3 more args instead of
    // the required 4*3 + 4 = 16 coordinate+weight args
    let curve_op = CompiledGeometryOp::Curve {
        kind: CurveKind::NurbsCurve,
        args: vec![
            ("c0".into(), dim_literal(3.0)),  // degree
            ("c1".into(), dim_literal(4.0)),  // n_points
            ("c2".into(), dim_literal(0.0)),  // only 3 extra args
            ("c3".into(), dim_literal(0.0)),
            ("c4".into(), dim_literal(0.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![curve_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_types::ModulePath::single("test_nurbs_short"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // Should produce NO geometry ops (NURBS eval returns None)
    let ops = ops_ref.lock().unwrap();
    assert_eq!(ops.len(), 0, "expected no geometry ops for insufficient NURBS args, got {}", ops.len());

    // Should emit a diagnostic error about expected n_points
    let diag_messages: Vec<String> = result.diagnostics.iter().map(|d| d.message.clone()).collect();
    assert!(
        diag_messages.iter().any(|m| m.contains("nurbs() got fewer arguments than expected for 4 control points")),
        "expected diagnostic about insufficient NURBS coordinate args, got: {:?}", diag_messages,
    );
}
