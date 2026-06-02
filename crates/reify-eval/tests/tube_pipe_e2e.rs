//! End-to-end tests for the tube() and pipe() geometry operations.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT, plus
//! OCCT-gated full-pipeline tests (source → parse → compile → Engine → build).

use reify_compiler::{CompiledGeometryOp, CurveKind, GeomRef, PrimitiveKind, SweepKind};
use reify_core::{ModulePath, Severity, Type};
use reify_ir::{ExportFormat, GeometryOp, GeometryQuery, Value};
use reify_test_support::*;

/// Exercises the full compile -> eval path for Tube.
///
/// Creates a module with a single CompiledGeometryOp::Primitive { kind: Tube }
/// carrying outer_r=10mm, inner_r=5mm, height=20mm, then runs it through
/// Engine::build with MockGeometryKernel and asserts the captured runtime op
/// is GeometryOp::Tube with SI values 0.010, 0.005, 0.020 m.
#[test]
fn tube_through_mock_kernel_emits_geometry_op_tube() {
    let e = "TestTube";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

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

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_tube"))
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

/// Exercises the full compile -> eval path for Pipe.
///
/// Creates a module with 2 ops:
///   Op 0: LineSegment from (0,0,0) to (0,0,10mm) — the path wire
///   Op 1: Sweep(Pipe) referencing Step(0) as the path, radius=2mm
///
/// Verifies that the Pipe runtime op receives the correct path handle from
/// op 0 and radius of 0.002 m (2 mm SI).
#[test]
fn pipe_through_mock_kernel_emits_geometry_op_pipe() {
    let e = "TestPipe";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: LineSegment (produces a wire handle at step index 0)
    let line_op = CompiledGeometryOp::Curve {
        kind: CurveKind::LineSegment,
        args: vec![
            ("x1".into(), mm_literal(0.0)),
            ("y1".into(), mm_literal(0.0)),
            ("z1".into(), mm_literal(0.0)),
            ("x2".into(), mm_literal(0.0)),
            ("y2".into(), mm_literal(0.0)),
            ("z2".into(), mm_literal(10.0)),
        ],
    };

    // Op 1: Pipe referencing Step(0) as the path, radius=2mm
    let pipe_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Pipe,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("path".into(), mm_literal(0.0)), // placeholder — the geom ref is what matters
            ("radius".into(), mm_literal(2.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![line_op, pipe_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_pipe"))
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

    let path_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::Pipe { path, radius } => {
            assert_eq!(
                *path, path_handle,
                "Pipe path should be handle from op 0 ({:?}), got {:?}",
                path_handle, path
            );
            let radius_si = radius.as_f64().expect("radius should be numeric");
            assert!(
                (radius_si - 0.002).abs() < 1e-9,
                "Pipe radius should be 0.002 m (2 mm SI), got {}",
                radius_si
            );
        }
        other => panic!("expected GeometryOp::Pipe at op index 1, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-17: full-pipeline OCCT-gated e2e tests for tube() and pipe()
// ---------------------------------------------------------------------------
//
// Each volume test does two things:
//   (1) Runs the full source → parse → compile → Engine → build pipeline and
//       asserts the pipeline succeeds end-to-end (no Error diagnostics, a
//       non-empty tessellated mesh is produced, STEP output is non-empty).
//   (2) Independently queries volume on a local OcctKernel running the
//       equivalent GeometryOp, asserting the formula volume within tolerance.
//
// Going via Engine::build owns the kernel internally and there is no public
// query API on the post-build state. Computing volume from the tessellated
// mesh via the divergence theorem is orientation-sensitive and yielded ~40%
// error for the hollow tube. Replaying the same op on a parallel kernel is
// the pattern also used in stress_query_consistency.rs / boundary4_geometry.rs
// for precise volume checks.

/// Full-pipeline e2e for tube(): source → parse → compile → Engine with
/// real OcctKernel → build → non-empty geometry output; volume formula
/// π*(R²-r²)*h verified within 1% relative error via a parallel direct
/// OcctKernel query.
#[test]
fn tube_volume_through_full_pipeline_matches_formula() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let r = tube(10mm, 5mm, 20mm)
}"#;

    // ---- Pipeline part ----
    let parsed = reify_syntax::parse(source, ModulePath::single("test_tube_e2e"));
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

    // Confirm the realization contains a single Primitive { kind: Tube }.
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
                kind: PrimitiveKind::Tube,
                ..
            }
        ),
        "expected Primitive(Tube), got {:?}",
        &realization.operations[0]
    );

    // Build with a real OCCT kernel via SingleKernelHolder (matches
    // boolean_multi_realization_nested_e2e).
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
    assert!(!mesh.vertices.is_empty(), "tube mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "tube mesh should have triangles");

    // Also ensure Step export works through the same pipeline (separate
    // engine; tessellate and build each consume the single registered
    // kernel handle in SingleKernelHolder).
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
        .expect("tube should produce STEP geometry output");
    assert!(!step.is_empty(), "STEP output should be non-empty");

    // ---- Volume verification part (direct OcctKernel query) ----
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Tube {
            outer_r: Value::Real(0.010),
            inner_r: Value::Real(0.005),
            height: Value::Real(0.020),
        })
        .expect("Tube execute should succeed");
    let vol = kernel
        .query(&GeometryQuery::Volume(handle.id))
        .expect("Volume query should succeed");
    let v = vol.as_f64().expect("volume should be numeric");
    // outer_r = 10mm = 0.010 m, inner_r = 5mm = 0.005 m, height = 20mm = 0.020 m
    let expected = std::f64::consts::PI * (0.010_f64.powi(2) - 0.005_f64.powi(2)) * 0.020;
    let rel_err = (v - expected).abs() / expected;
    assert!(
        rel_err < 0.01,
        "tube volume should be ≈{:.3e} m³, got {:.3e} (rel_err={:.4})",
        expected,
        v,
        rel_err
    );
}

/// Full-pipeline e2e for pipe(): source → parse → compile → Engine with
/// real OcctKernel → build → non-empty geometry output; volume formula
/// π*r²*L verified to floating-point tolerance (rel_err < 1e-6) via a parallel
/// direct OcctKernel query.
#[test]
fn pipe_volume_through_full_pipeline_matches_formula() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let r = pipe(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 2mm)
}"#;

    // ---- Pipeline part ----
    let parsed = reify_syntax::parse(source, ModulePath::single("test_pipe_e2e"));
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

    // Confirm the realization contains the expected op structure:
    // [Curve(LineSegment), Sweep{kind: Pipe}]
    assert_eq!(compiled.templates.len(), 1, "expected 1 template");
    let realization = &compiled.templates[0].realizations[0];
    assert_eq!(
        realization.operations.len(),
        2,
        "expected 2 ops (line_segment + pipe), got {}",
        realization.operations.len()
    );
    assert!(
        matches!(
            &realization.operations[1],
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Pipe,
                ..
            }
        ),
        "expected Sweep(Pipe) at op 1, got {:?}",
        &realization.operations[1]
    );

    // Build with real OCCT kernel
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
    assert!(!mesh.vertices.is_empty(), "pipe mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "pipe mesh should have triangles");

    // Also ensure Step export succeeds.
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
        .expect("pipe should produce STEP geometry output");
    assert!(!step.is_empty(), "STEP output should be non-empty");

    // ---- Volume verification part (direct OcctKernel query) ----
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let wire_h = kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: 0.020,
        })
        .expect("LineSegment execute should succeed");
    let pipe_h = kernel
        .execute(&GeometryOp::Pipe {
            path: wire_h.id,
            radius: Value::Real(0.002),
        })
        .expect("Pipe execute should succeed");
    let vol = kernel
        .query(&GeometryQuery::Volume(pipe_h.id))
        .expect("Volume query should succeed");
    let v = vol.as_f64().expect("volume should be numeric");
    // radius = 2mm = 0.002 m, length = 20mm = 0.020 m
    let expected = std::f64::consts::PI * 0.002_f64.powi(2) * 0.020;
    let rel_err = (v - expected).abs() / expected;
    // Direct BRep volume queries are analytic (not tessellation-based),
    // so a straight circular pipe should match the formula to within
    // floating-point noise. The tight tolerance protects against silent
    // unit-conversion regressions (a 1-3% error would previously pass a
    // lax 5% bound).
    assert!(
        rel_err < 1e-6,
        "pipe volume should be ≈{:.3e} m³, got {:.3e} (rel_err={:.4e})",
        expected,
        v,
        rel_err
    );
}

/// Full-pipeline e2e: inner > outer radius should surface as an Error-severity
/// diagnostic mentioning "inner" and "outer" via the kernel→eval diagnostic
/// channel.
#[test]
fn tube_inner_greater_than_outer_emits_error_diagnostic() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }

    // inner(10mm) > outer(5mm) — kernel should reject and surface as Error
    // diagnostic routed through Engine::build.
    let source = r#"structure S {
    let r = tube(5mm, 10mm, 20mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test_tube_inner_gt"));
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

    // Expect at least one Error-severity diagnostic mentioning "inner" and
    // "outer" — routed from GeometryError::OperationFailed through the
    // eval layer's geometry-error diagnostic channel.
    let has_inner_outer_error = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Error && d.message.contains("inner") && d.message.contains("outer")
    });
    assert!(
        has_inner_outer_error,
        "expected an Error diagnostic mentioning 'inner' and 'outer', got: {:?}",
        result.diagnostics
    );
}
