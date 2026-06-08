//! End-to-end tests for the wedge() geometry operation (task-4158, PRD δ).
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp,
//! using MockGeometryKernel to capture executed operations without OCCT, plus
//! OCCT-gated full-pipeline tests (source → parse → compile → Engine → build).
//!
//! User-observable signal (PRD δ):
//!   `wedge(20mm, 10mm, 15mm, 5mm)` → non-degenerate Solid, 6 faces,
//!   volume ≈ 1.875e-6 m³ (= depth·height·(width+top_width)/2, exact for
//!   a flat-faced trapezoidal prism built by BRepPrimAPI_MakeWedge).
//!
//! Volume and face-count assertions at the OCCT kernel level are covered by
//! `kernel_wedge_volume_and_face_count_match_expected` in
//! `crates/reify-kernel-occt/src/lib.rs`. The tests here exercise paths the
//! unit tests cannot: the compiler→eval→engine pipeline (mock and real kernels).

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_core::{ModulePath, Severity};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

/// Exercises the full compile → eval path for Wedge via the mock kernel.
///
/// Creates a module with a single `CompiledGeometryOp::Primitive { kind: Wedge }`
/// carrying width=20mm, depth=10mm, height=15mm, top_width=5mm, then runs it
/// through `Engine::build` with MockGeometryKernel and asserts the captured
/// runtime op is `GeometryOp::Wedge` with the correct SI field values.
#[test]
fn wedge_through_mock_kernel_emits_geometry_op_wedge() {
    let e = "TestWedge";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), reify_core::Type::length());

    let wedge_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Wedge,
        args: vec![
            ("width".into(), mm_literal(20.0)),
            ("depth".into(), mm_literal(10.0)),
            ("height".into(), mm_literal(15.0)),
            ("top_width".into(), mm_literal(5.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![wedge_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_wedge"))
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
        GeometryOp::Wedge {
            width,
            depth,
            height,
            top_width,
        } => {
            let width_si = width.as_f64().expect("width should be numeric");
            let depth_si = depth.as_f64().expect("depth should be numeric");
            let height_si = height.as_f64().expect("height should be numeric");
            let top_width_si = top_width.as_f64().expect("top_width should be numeric");
            assert!(
                (width_si - 0.020).abs() < 1e-9,
                "Wedge width should be 0.020 m (20 mm SI), got {}",
                width_si
            );
            assert!(
                (depth_si - 0.010).abs() < 1e-9,
                "Wedge depth should be 0.010 m (10 mm SI), got {}",
                depth_si
            );
            assert!(
                (height_si - 0.015).abs() < 1e-9,
                "Wedge height should be 0.015 m (15 mm SI), got {}",
                height_si
            );
            assert!(
                (top_width_si - 0.005).abs() < 1e-9,
                "Wedge top_width should be 0.005 m (5 mm SI), got {}",
                top_width_si
            );
        }
        other => panic!("expected GeometryOp::Wedge at op index 0, got {:?}", other),
    }
}

/// Full-pipeline e2e for wedge: source → parse → compile → Engine with real
/// OcctKernel → tessellate_realizations → non-empty mesh vertices and triangles.
///
/// Verifies the PRD δ user-observable signal: `wedge(20mm, 10mm, 15mm, 5mm)`
/// produces a non-degenerate solid through the complete compiler+eval+kernel chain.
#[test]
fn wedge_through_full_pipeline_produces_geometry() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let r = wedge(20mm, 10mm, 15mm, 5mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test_wedge_e2e"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Confirm the realization contains a single Primitive { kind: Wedge }.
    assert_eq!(compiled.templates.len(), 1, "expected 1 template");
    let realization = &compiled.templates[0].realizations[0];
    assert_eq!(
        realization.operations.len(),
        1,
        "expected 1 op in realization, got {}",
        realization.operations.len()
    );
    assert!(
        matches!(
            &realization.operations[0],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Wedge,
                ..
            }
        ),
        "expected Primitive(Wedge), got {:?}",
        &realization.operations[0]
    );

    // Build with a real OCCT kernel.
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let tess_result = engine.tessellate_realizations(&compiled);
    let geom_errors: Vec<_> = tess_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "unexpected geometry errors: {:?}",
        geom_errors
    );
    assert_eq!(
        tess_result.meshes.len(),
        1,
        "expected 1 mesh (single realization), got {}",
        tess_result.meshes.len()
    );
    let mesh = &tess_result.meshes[0].mesh;
    assert!(!mesh.vertices.is_empty(), "wedge mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "wedge mesh should have triangles");
}

