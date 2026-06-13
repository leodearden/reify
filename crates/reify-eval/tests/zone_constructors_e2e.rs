//! End-to-end tests for zone_cylinder, zone_annulus, and zone_profile
//! GD&T geometry constructors (task 4476, γ-slice).
//!
//! Structural (always-run) tests compile from source and check the lowered
//! op shapes. OCCT-gated volume oracle tests build through Engine with
//! OcctKernelHandle, then replay the lowered ops on a parallel direct
//! OcctKernel to verify volume identities.
//!
//! Mirrors tube_pipe_e2e.rs: same harness pattern, validated rel_err bounds
//! (pipe 1e-6, boolean-of-pipes 1e-2).

// Step 1 imports (zone_cylinder). Extended in steps 3 and 5.
use reify_compiler::{CompiledGeometryOp, CurveKind, SweepKind};
use reify_core::{ModulePath, Severity};
use reify_ir::{ExportFormat, GeometryOp, GeometryQuery, Value};
use reify_test_support::*;

// ─── zone_cylinder (step 1 RED / step 2 GREEN) ───────────────────────────────

/// Structural test: `zone_cylinder(line_segment(...), 8mm)` lowers to
/// [Curve(LineSegment), Sweep{kind:Pipe}] with runtime radius = width/2
/// = 8mm/2 = 0.004 m.
///
/// Always-run (no OCCT required). RED until step-2 registers zone_cylinder.
#[test]
fn zone_cylinder_structural_lowers_to_line_segment_and_pipe() {
    let source = r#"structure S {
    let z = zone_cylinder(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 8mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("zone_cylinder_structural"));
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
    assert!(
        errors.is_empty(),
        "zone_cylinder should compile with no error-severity diagnostics, got: {:#?}",
        errors
    );

    // ── Compiled-realization shape ──
    assert_eq!(compiled.templates.len(), 1, "expected 1 template");
    let realization = &compiled.templates[0].realizations[0];
    assert_eq!(
        realization.operations.len(),
        2,
        "expected 2 ops [LineSegment, Sweep(Pipe)], got {}",
        realization.operations.len()
    );
    assert!(
        matches!(
            &realization.operations[0],
            CompiledGeometryOp::Curve {
                kind: CurveKind::LineSegment,
                ..
            }
        ),
        "op[0] should be Curve(LineSegment), got {:?}",
        &realization.operations[0]
    );
    assert!(
        matches!(
            &realization.operations[1],
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Pipe,
                ..
            }
        ),
        "op[1] should be Sweep(Pipe), got {:?}",
        &realization.operations[1]
    );

    // ── MockGeometryKernel: confirm runtime radius = width/2 = 0.004 m ──
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _ = engine.build(&compiled, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "engine should dispatch 2 ops (LineSegment, Pipe), got {}",
        ops.len()
    );
    match &ops[1].op {
        GeometryOp::Pipe { radius, .. } => {
            let r = radius.as_f64().expect("radius should be numeric");
            assert!(
                (r - 0.004).abs() < 1e-9,
                "Pipe radius should be 0.004 m (8mm/2 = width/2), got {}",
                r
            );
        }
        other => panic!("expected GeometryOp::Pipe at op[1], got {:?}", other),
    }
}

/// OCCT volume oracle for zone_cylinder.
///
/// Formula: V = π * (d/2)² * L = π * r² * L  (Ø-zone: width is diameter)
/// Parameters: d = 8mm, L = 20mm → V = π * 0.004² * 0.020
/// Tolerance: rel_err < 1e-6 (validated basis: pipe_volume_through_full_pipeline_matches_formula
/// passes 1e-6 for the identical Pipe-of-straight-+Z-wire construction).
///
/// OCCT-gated; skips cleanly when OCCT is unavailable.
#[test]
fn zone_cylinder_volume_matches_formula() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping zone_cylinder_volume_matches_formula: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let z = zone_cylinder(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 8mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("zone_cylinder_vol"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // ── Full-pipeline: Engine + OcctKernelHandle ──
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
        "unexpected geometry errors in tessellate: {:?}",
        geom_errors
    );
    assert!(
        !tess_result.meshes.is_empty(),
        "zone_cylinder should produce at least 1 mesh"
    );
    let mesh = &tess_result.meshes[0].mesh;
    assert!(!mesh.vertices.is_empty(), "zone_cylinder mesh should have vertices");
    assert!(!mesh.indices.is_empty(), "zone_cylinder mesh should have triangles");

    // STEP export
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
        .expect("zone_cylinder should produce STEP geometry output");
    assert!(!step.is_empty(), "STEP output should be non-empty");

    // ── Volume: direct OcctKernel replay ──
    // zone_cylinder(axis_wire_of_length_20mm, 8mm) → Pipe(axis, radius=4mm=0.004m)
    // V = π * r² * L = π * 0.004² * 0.020
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
            radius: Value::Real(0.004), // width/2 = 8mm/2 = 4mm = 0.004m
        })
        .expect("Pipe execute should succeed");
    let vol = kernel
        .query(&GeometryQuery::Volume(pipe_h.id))
        .expect("Volume query should succeed");
    let v = vol.as_f64().expect("volume should be numeric");
    // d = 8mm = 0.008m, r = d/2 = 0.004m, L = 20mm = 0.020m
    // V = π/4 * d² * L
    let d = 0.008_f64;
    let l = 0.020_f64;
    let expected = std::f64::consts::PI / 4.0 * d.powi(2) * l;
    let rel_err = (v - expected).abs() / expected;
    assert!(
        rel_err < 1e-6,
        "zone_cylinder volume should be ≈{:.3e} m³ (π/4·d²·L = π/4·0.008²·0.020), \
         got {:.3e} m³ (rel_err={:.4e})",
        expected,
        v,
        rel_err
    );
}
