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
