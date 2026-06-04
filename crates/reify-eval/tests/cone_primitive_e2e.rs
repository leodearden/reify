//! End-to-end tests for the cone() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT, plus
//! OCCT-gated full-pipeline tests (source → parse → compile → Engine → build).
//!
//! RED until step-6 adds PrimitiveKind::Cone and wires the full compiler+eval
//! pipeline (types.rs, units.rs, geometry.rs, geometry_traits_inference.rs,
//! geometry_ops.rs, engine_build.rs compiled_geometry_op_to_operation).

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_core::{ModulePath, Severity};
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

/// Exercises the full compile -> eval path for Cone (mock kernel).
///
/// Creates a module with a single CompiledGeometryOp::Primitive { kind: Cone }
/// carrying bottom_radius=10mm, top_radius=5mm, height=20mm, then runs it through
/// Engine::build with MockGeometryKernel and asserts the captured runtime op
/// is GeometryOp::Cone with SI values 0.010, 0.005, 0.020 m.
///
/// RED until step-6 adds PrimitiveKind::Cone and the Cone eval mapping.
#[test]
fn cone_through_mock_kernel_emits_geometry_op_cone() {
    let e = "TestCone";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), reify_core::Type::length());

    let cone_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Cone,
        args: vec![
            ("bottom_radius".into(), mm_literal(10.0)),
            ("top_radius".into(), mm_literal(5.0)),
            ("height".into(), mm_literal(20.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![cone_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_cone"))
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
        GeometryOp::Cone {
            bottom_radius,
            top_radius,
            height,
        } => {
            let bottom_si = bottom_radius.as_f64().expect("bottom_radius should be numeric");
            let top_si = top_radius.as_f64().expect("top_radius should be numeric");
            let height_si = height.as_f64().expect("height should be numeric");
            assert!(
                (bottom_si - 0.010).abs() < 1e-9,
                "Cone bottom_radius should be 0.010 m (10 mm SI), got {}",
                bottom_si
            );
            assert!(
                (top_si - 0.005).abs() < 1e-9,
                "Cone top_radius should be 0.005 m (5 mm SI), got {}",
                top_si
            );
            assert!(
                (height_si - 0.020).abs() < 1e-9,
                "Cone height should be 0.020 m (20 mm SI), got {}",
                height_si
            );
        }
        other => panic!("expected GeometryOp::Cone at op index 0, got {:?}", other),
    }
}

/// Full-pipeline e2e for cone(frustum): source → parse → compile → Engine with
/// real OcctKernel → build → non-empty geometry output (non-empty mesh vertices,
/// triangles, and STEP export). Volume correctness is verified by the dedicated
/// kernel unit tests (kernel_cone_frustum_volume_matches_closed_form).
///
/// RED until step-6 wires the full compiler+eval pipeline.
#[test]
fn cone_frustum_through_full_pipeline_produces_geometry() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let r = cone(10mm, 5mm, 20mm)
}"#;

    // ---- Pipeline part ----
    let parsed = reify_syntax::parse(source, ModulePath::single("test_cone_frustum_e2e"));
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

    // Confirm the realization contains a single Primitive { kind: Cone }.
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
                kind: PrimitiveKind::Cone,
                ..
            }
        ),
        "expected Primitive(Cone), got {:?}",
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
    assert!(!mesh.vertices.is_empty(), "cone frustum mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "cone frustum mesh should have triangles");

    // Also ensure Step export works.
    let checker2 = reify_constraints::SimpleConstraintChecker;
    let mut planner2 = reify_geometry::SingleKernelHolder::new();
    planner2.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine2 = reify_eval::Engine::new(Box::new(checker2), Some(Box::new(planner2)));
    let build_result = engine2.build(&compiled, ExportFormat::Step);
    let build_errors: Vec<_> = build_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "unexpected build errors: {:?}",
        build_errors
    );
    let step = build_result
        .geometry_output
        .expect("cone frustum should produce STEP geometry output");
    assert!(!step.is_empty(), "STEP output should be non-empty");
}

/// Full-pipeline e2e for cone(pointed apex, top_radius=0): source → parse →
/// compile → Engine with real OcctKernel → build → non-empty geometry output
/// (non-empty mesh vertices and triangles). Volume correctness is verified by
/// kernel_cone_pointed_volume_matches_closed_form in the kernel unit tests.
///
/// RED until step-6 wires the full compiler+eval pipeline.
#[test]
fn cone_pointed_through_full_pipeline_produces_geometry() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let r = cone(10mm, 0mm, 20mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test_cone_pointed_e2e"));
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
        "unexpected geometry errors for pointed cone: {:?}",
        geom_errors
    );
    assert_eq!(
        tess_result.meshes.len(),
        1,
        "expected 1 mesh (pointed cone), got {}",
        tess_result.meshes.len()
    );
    let mesh = &tess_result.meshes[0].mesh;
    assert!(!mesh.vertices.is_empty(), "pointed cone mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "pointed cone mesh should have triangles");
}

/// Full-pipeline e2e: negative bottom_radius should surface as an Error-severity
/// diagnostic via the kernel→eval diagnostic channel.
///
/// RED until step-6 wires the full compiler+eval pipeline.
#[test]
fn cone_negative_bottom_radius_emits_error_diagnostic() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // bottom_radius(-1mm) is negative — kernel should reject and surface as Error
    let source = r#"structure S {
    let r = cone(-1mm, 5mm, 20mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test_cone_neg_radius"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    // Compile side should accept — validation is in the kernel.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile phase should not error (validation happens in the kernel): {:?}",
        errors
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.build(&compiled, ExportFormat::Step);

    // Expect at least one Error-severity diagnostic from the kernel validation failure.
    let has_error = result
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error);
    assert!(
        has_error,
        "expected an Error-severity diagnostic for negative bottom_radius, got: {:?}",
        result.diagnostics
    );
}
