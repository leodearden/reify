//! End-to-end tests for rectangle and circle 2-D profile geometry operations.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.
//!
//! RED until step-6 adds CompiledGeometryOp::Profile, ProfileKind, and the
//! compiler lowering/inference arms for rectangle + circle.

use reify_compiler::{CompiledGeometryOp, GeomRef, ProfileKind, SweepKind};
use reify_core::Type;
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

// ---------------------------------------------------------------------------
// Compiler: rectangle and circle recognized and produce correct Profile ops
// ---------------------------------------------------------------------------

/// rectangle(20mm, 10mm) must compile to Profile(Rectangle) with width+height.
///
/// RED until step-6 adds CompiledGeometryOp::Profile, ProfileKind, and the
/// "rectangle" arm in compile_geometry_call.
#[test]
fn rectangle_compiler_accepts_2_args() {
    let source = r#"structure S {
    let r = rectangle(20mm, 10mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_rect"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "rectangle must produce 1 realization");
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Profile {
                kind: ProfileKind::Rectangle,
                ..
            }
        ),
        "expected Profile(Rectangle), got {:?}",
        op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        compiled.diagnostics
    );
}

/// circle(8mm) must compile to Profile(Circle) with radius.
///
/// RED until step-6 adds CompiledGeometryOp::Profile, ProfileKind, and the
/// "circle" arm in compile_geometry_call.
#[test]
fn circle_compiler_accepts_1_arg() {
    let source = r#"structure S {
    let r = circle(8mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_circ"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "circle must produce 1 realization");
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Profile {
                kind: ProfileKind::Circle,
                ..
            }
        ),
        "expected Profile(Circle), got {:?}",
        op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        compiled.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Eval pipeline: rectangle profile through full eval produces correct ops
// ---------------------------------------------------------------------------

/// eval extrude(rectangle(20mm,10mm), 3mm) → op stream contains
/// GeometryOp::RectangleProfile{width≈0.02, height≈0.01} then
/// GeometryOp::Extrude{distance≈0.003}.
///
/// RED until step-6 adds the Profile variant to compiler + eval lowering.
#[test]
fn rectangle_profile_through_full_eval_pipeline() {
    let e = "TestRectangleProfile";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: RectangleProfile — produces a face handle at step index 0
    let rect_op = CompiledGeometryOp::Profile {
        kind: ProfileKind::Rectangle,
        args: vec![
            ("width".into(), mm_literal(20.0)),
            ("height".into(), mm_literal(10.0)),
        ],
    };

    // Op 1: Extrude referencing Step(0) as profile, distance = 3mm
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(20.0)), // positional placeholder
            ("distance".into(), mm_literal(3.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![rect_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_rect_eval"))
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
        "expected 2 geometry operations (RectangleProfile + Extrude), got {}",
        ops.len()
    );

    // Op 0 must be RectangleProfile with correct dimensions
    match &ops[0].op {
        GeometryOp::RectangleProfile { width, height } => {
            let w = width.as_f64().expect("width should be numeric");
            let h = height.as_f64().expect("height should be numeric");
            assert!(
                (w - 0.02).abs() < 1e-9,
                "rectangle width should be 0.02 m (20 mm SI), got {}",
                w
            );
            assert!(
                (h - 0.01).abs() < 1e-9,
                "rectangle height should be 0.01 m (10 mm SI), got {}",
                h
            );
        }
        other => panic!("expected GeometryOp::RectangleProfile at op 0, got {:?}", other),
    }

    // Op 1 must be Extrude referencing the rectangle face handle
    let rect_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::Extrude { profile, distance } => {
            assert_eq!(
                *profile, rect_handle,
                "Extrude profile should be the RectangleProfile handle ({:?}), got {:?}",
                rect_handle, profile
            );
            let d = distance.as_f64().expect("distance should be numeric");
            assert!(
                (d - 0.003).abs() < 1e-9,
                "Extrude distance should be 0.003 m (3 mm SI), got {}",
                d
            );
        }
        other => panic!("expected GeometryOp::Extrude at op 1, got {:?}", other),
    }
}

/// eval extrude(circle(8mm), 2mm) → op stream contains
/// GeometryOp::CircleProfile{radius≈0.008} then GeometryOp::Extrude{distance≈0.002}.
///
/// RED until step-6 adds the Profile variant to compiler + eval lowering.
#[test]
fn circle_profile_through_full_eval_pipeline() {
    let e = "TestCircleProfile";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: CircleProfile — produces a face handle at step index 0
    let circle_op = CompiledGeometryOp::Profile {
        kind: ProfileKind::Circle,
        args: vec![
            ("radius".into(), mm_literal(8.0)),
        ],
    };

    // Op 1: Extrude referencing Step(0) as profile, distance = 2mm
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(8.0)), // positional placeholder
            ("distance".into(), mm_literal(2.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![circle_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_circ_eval"))
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
        "expected 2 geometry operations (CircleProfile + Extrude), got {}",
        ops.len()
    );

    // Op 0 must be CircleProfile with correct radius
    match &ops[0].op {
        GeometryOp::CircleProfile { radius } => {
            let r = radius.as_f64().expect("radius should be numeric");
            assert!(
                (r - 0.008).abs() < 1e-9,
                "circle radius should be 0.008 m (8 mm SI), got {}",
                r
            );
        }
        other => panic!("expected GeometryOp::CircleProfile at op 0, got {:?}", other),
    }

    // Op 1 must be Extrude referencing the circle face handle
    let circle_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::Extrude { profile, distance } => {
            assert_eq!(
                *profile, circle_handle,
                "Extrude profile should be the CircleProfile handle ({:?}), got {:?}",
                circle_handle, profile
            );
            let d = distance.as_f64().expect("distance should be numeric");
            assert!(
                (d - 0.002).abs() < 1e-9,
                "Extrude distance should be 0.002 m (2 mm SI), got {}",
                d
            );
        }
        other => panic!("expected GeometryOp::Extrude at op 1, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// task-4161: polygon + ellipse 2-D profile tests
// ---------------------------------------------------------------------------

/// polygon(0mm,0mm, 10mm,0mm, 10mm,10mm) must compile to Profile(Polygon).
///
/// RED until step-6 adds ProfileKind::Polygon and the "polygon" compiler arm.
#[test]
fn polygon_compiler_accepts_6_args() {
    let source = r#"structure S {
    let r = polygon(0mm, 0mm, 10mm, 0mm, 10mm, 10mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_poly"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "polygon must produce 1 realization");
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Profile {
                kind: ProfileKind::Polygon,
                ..
            }
        ),
        "expected Profile(Polygon), got {:?}",
        op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        compiled.diagnostics
    );
}

/// ellipse(10mm, 5mm) must compile to Profile(Ellipse).
///
/// RED until step-6 adds ProfileKind::Ellipse and the "ellipse" compiler arm.
#[test]
fn ellipse_compiler_accepts_2_args() {
    let source = r#"structure S {
    let r = ellipse(10mm, 5mm)
}"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_ellipse"));
    let compiled = reify_compiler::compile(&parsed);
    let template = &compiled.templates[0];
    assert_eq!(template.realizations.len(), 1, "ellipse must produce 1 realization");
    let op = &template.realizations[0].operations[0];
    assert!(
        matches!(
            op,
            CompiledGeometryOp::Profile {
                kind: ProfileKind::Ellipse,
                ..
            }
        ),
        "expected Profile(Ellipse), got {:?}",
        op
    );
    assert!(
        compiled.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        compiled.diagnostics
    );
}

/// eval extrude(polygon(0mm,0mm, 10mm,0mm, 10mm,10mm), 3mm) → op stream contains
/// GeometryOp::PolygonProfile{points: [[0,0],[0.01,0],[0.01,0.01]]} then
/// GeometryOp::Extrude{distance≈0.003}.
///
/// RED until step-6 adds ProfileKind::Polygon, "polygon" compiler arm, and
/// the Polygon eval arm in geometry_ops.rs.
#[test]
fn polygon_profile_through_full_eval_pipeline() {
    let e = "TestPolygonProfile";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: PolygonProfile with 6 coordinate args (c0..c5) for 3 vertices
    let polygon_op = CompiledGeometryOp::Profile {
        kind: ProfileKind::Polygon,
        args: vec![
            ("c0".into(), mm_literal(0.0)),
            ("c1".into(), mm_literal(0.0)),
            ("c2".into(), mm_literal(10.0)),
            ("c3".into(), mm_literal(0.0)),
            ("c4".into(), mm_literal(10.0)),
            ("c5".into(), mm_literal(10.0)),
        ],
    };

    // Op 1: Extrude referencing Step(0) as profile, distance = 3mm
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(0.0)), // positional placeholder
            ("distance".into(), mm_literal(3.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![polygon_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_poly_eval"))
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
        "expected 2 geometry operations (PolygonProfile + Extrude), got {}",
        ops.len()
    );

    // Op 0 must be PolygonProfile with correct points
    match &ops[0].op {
        GeometryOp::PolygonProfile { points } => {
            assert_eq!(points.len(), 3, "polygon should have 3 vertices, got {}", points.len());
            // First point should be approximately (0.0, 0.0) in SI metres
            let (x0, y0) = (points[0][0], points[0][1]);
            assert!(
                x0.abs() < 1e-9 && y0.abs() < 1e-9,
                "first polygon point should be ~(0,0), got ({x0},{y0})"
            );
            // Second point approximately (0.01, 0) — 10mm in SI
            let (x1, y1) = (points[1][0], points[1][1]);
            assert!(
                (x1 - 0.01).abs() < 1e-9 && y1.abs() < 1e-9,
                "second polygon point should be ~(0.01,0), got ({x1},{y1})"
            );
        }
        other => panic!("expected GeometryOp::PolygonProfile at op 0, got {:?}", other),
    }

    // Op 1 must be Extrude referencing the polygon face handle
    let poly_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::Extrude { profile, distance } => {
            assert_eq!(
                *profile, poly_handle,
                "Extrude profile should be the PolygonProfile handle ({:?}), got {:?}",
                poly_handle, profile
            );
            let d = distance.as_f64().expect("distance should be numeric");
            assert!(
                (d - 0.003).abs() < 1e-9,
                "Extrude distance should be 0.003 m (3 mm SI), got {}",
                d
            );
        }
        other => panic!("expected GeometryOp::Extrude at op 1, got {:?}", other),
    }
}

/// eval ellipse(10mm, 5mm) → op stream contains
/// GeometryOp::EllipseProfile{semi_major≈0.010, semi_minor≈0.005}.
///
/// RED until step-6 adds ProfileKind::Ellipse, "ellipse" compiler arm, and
/// the Ellipse eval arm in geometry_ops.rs.
#[test]
fn ellipse_profile_through_full_eval_pipeline() {
    let e = "TestEllipseProfile";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: EllipseProfile with semi_major=10mm, semi_minor=5mm
    let ellipse_op = CompiledGeometryOp::Profile {
        kind: ProfileKind::Ellipse,
        args: vec![
            ("semi_major".into(), mm_literal(10.0)),
            ("semi_minor".into(), mm_literal(5.0)),
        ],
    };

    // Op 1: Extrude referencing Step(0) as profile, distance = 2mm
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(10.0)), // positional placeholder
            ("distance".into(), mm_literal(2.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![ellipse_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_ellipse_eval"))
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
        "expected 2 geometry operations (EllipseProfile + Extrude), got {}",
        ops.len()
    );

    // Op 0 must be EllipseProfile with correct semi-axes
    match &ops[0].op {
        GeometryOp::EllipseProfile { semi_major, semi_minor } => {
            let a = semi_major.as_f64().expect("semi_major should be numeric");
            let b = semi_minor.as_f64().expect("semi_minor should be numeric");
            assert!(
                (a - 0.010).abs() < 1e-9,
                "ellipse semi_major should be 0.010 m (10 mm SI), got {}",
                a
            );
            assert!(
                (b - 0.005).abs() < 1e-9,
                "ellipse semi_minor should be 0.005 m (5 mm SI), got {}",
                b
            );
        }
        other => panic!("expected GeometryOp::EllipseProfile at op 0, got {:?}", other),
    }

    // Op 1 must be Extrude referencing the ellipse face handle
    let ellipse_handle = ops[0].result_handle;
    match &ops[1].op {
        GeometryOp::Extrude { profile, distance } => {
            assert_eq!(
                *profile, ellipse_handle,
                "Extrude profile should be the EllipseProfile handle ({:?}), got {:?}",
                ellipse_handle, profile
            );
            let d = distance.as_f64().expect("distance should be numeric");
            assert!(
                (d - 0.002).abs() < 1e-9,
                "Extrude distance should be 0.002 m (2 mm SI), got {}",
                d
            );
        }
        other => panic!("expected GeometryOp::Extrude at op 1, got {:?}", other),
    }
}
