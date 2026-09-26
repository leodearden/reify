//! End-to-end tests for zone_cylinder, zone_annulus, and zone_profile
//! GD&T geometry constructors (task 4476, γ-slice).
//!
//! Structural (always-run) tests compile from source and check the lowered
//! op shapes. OCCT-gated volume oracle tests build through Engine with
//! OcctKernelHandle. The zone_cylinder / zone_annulus oracles then replay the
//! lowered ops on a parallel direct OcctKernel to verify volume identities;
//! the zone_profile oracles read the realized volume from a `volume()` cell.
//!
//! Mirrors tube_pipe_e2e.rs: same harness pattern, validated rel_err bounds
//! (pipe 1e-6, boolean-of-pipes 1e-2).

// Imports for all three constructors (zone_cylinder + zone_annulus + zone_profile).
use reify_compiler::{
    BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PrimitiveKind, SweepKind,
};
use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::{ExportFormat, GeometryOp, GeometryQuery, Value};
use reify_test_support::fixtures::assert_rel;
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
    assert!(
        !mesh.vertices.is_empty(),
        "zone_cylinder mesh should have vertices"
    );
    assert!(
        !mesh.indices.is_empty(),
        "zone_cylinder mesh should have triangles"
    );

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

// ─── zone_annulus (step 3 RED / step 4 GREEN) ────────────────────────────────

/// Structural test: `zone_annulus(line_segment(...), 20mm, 4mm, 20mm)` lowers to
/// [Curve(LineSegment), Sweep{Pipe, R+w/2=0.022}, Sweep{Pipe, R-w/2=0.018},
/// Boolean{Difference, Step(1), Step(2)}] with runtime radii verified by
/// MockGeometryKernel.
///
/// Parameters: R=20mm=0.020m, w=4mm=0.004m, L=20mm=0.020m
/// outer radius = R+w/2 = 0.022m, inner radius = R-w/2 = 0.018m.
///
/// Always-run (no OCCT required). RED until step-4 registers zone_annulus.
#[test]
fn zone_annulus_structural_lowers_to_four_ops() {
    let source = r#"structure S {
    let z = zone_annulus(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 20mm, 4mm, 20mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("zone_annulus_structural"));
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
        "zone_annulus should compile with no error-severity diagnostics, got: {:#?}",
        errors
    );

    // ── Compiled-realization shape ──
    assert_eq!(compiled.templates.len(), 1, "expected 1 template");
    let realization = &compiled.templates[0].realizations[0];
    assert_eq!(
        realization.operations.len(),
        4,
        "expected 4 ops [LineSegment, outer Pipe, inner Pipe, Boolean(Difference)], got {}",
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
        "op[1] should be Sweep(Pipe) (outer), got {:?}",
        &realization.operations[1]
    );
    assert!(
        matches!(
            &realization.operations[2],
            CompiledGeometryOp::Sweep {
                kind: SweepKind::Pipe,
                ..
            }
        ),
        "op[2] should be Sweep(Pipe) (inner), got {:?}",
        &realization.operations[2]
    );
    // op[3]: Boolean{Difference, left:Step(1)=outer, right:Step(2)=inner}
    assert!(
        matches!(
            &realization.operations[3],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(1),
                right: GeomRef::Step(2),
            }
        ),
        "op[3] should be Boolean(Difference, Step(1), Step(2)), got {:?}",
        &realization.operations[3]
    );

    // ── MockGeometryKernel: verify runtime radii ──
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _ = engine.build(&compiled, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        4,
        "engine should dispatch 4 ops (LineSegment, outer Pipe, inner Pipe, Difference), got {}",
        ops.len()
    );
    // op[1]: outer Pipe, radius = R + w/2 = 0.020 + 0.002 = 0.022m
    match &ops[1].op {
        GeometryOp::Pipe { radius, .. } => {
            let r = radius.as_f64().expect("outer radius should be numeric");
            assert!(
                (r - 0.022).abs() < 1e-9,
                "outer Pipe radius should be 0.022 m (R+w/2 = 20mm+2mm), got {}",
                r
            );
        }
        other => panic!(
            "expected GeometryOp::Pipe at op[1] (outer), got {:?}",
            other
        ),
    }
    // op[2]: inner Pipe, radius = R - w/2 = 0.020 - 0.002 = 0.018m
    match &ops[2].op {
        GeometryOp::Pipe { radius, .. } => {
            let r = radius.as_f64().expect("inner radius should be numeric");
            assert!(
                (r - 0.018).abs() < 1e-9,
                "inner Pipe radius should be 0.018 m (R-w/2 = 20mm-2mm), got {}",
                r
            );
        }
        other => panic!(
            "expected GeometryOp::Pipe at op[2] (inner), got {:?}",
            other
        ),
    }
    // op[3]: Boolean(Difference)
    match &ops[3].op {
        GeometryOp::Difference { .. } => {}
        other => panic!("expected GeometryOp::Difference at op[3], got {:?}", other),
    }
}

/// OCCT volume oracle for zone_annulus.
///
/// Formula: V = 2π * R * w * L  (annular shell identity)
/// Parameters: R=20mm=0.020m, w=4mm=0.004m, L=20mm=0.020m
/// Tolerance: rel_err < 1e-2 (validated basis: tube_volume_through_full_pipeline_matches_formula
/// passes 1e-2 for the identical hollow-cylinder-boolean class).
///
/// OCCT-gated; skips cleanly when OCCT is unavailable.
#[test]
fn zone_annulus_volume_matches_formula() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping zone_annulus_volume_matches_formula: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let z = zone_annulus(line_segment(0mm, 0mm, 0mm, 0mm, 0mm, 20mm), 20mm, 4mm, 20mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("zone_annulus_vol"));
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
        "zone_annulus should produce at least 1 mesh"
    );
    let mesh = &tess_result.meshes[0].mesh;
    assert!(
        !mesh.vertices.is_empty(),
        "zone_annulus mesh should have vertices"
    );
    assert!(
        !mesh.indices.is_empty(),
        "zone_annulus mesh should have triangles"
    );

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
        .expect("zone_annulus should produce STEP geometry output");
    assert!(!step.is_empty(), "STEP output should be non-empty");

    // ── Volume: direct OcctKernel replay ──
    // zone_annulus(axis_wire_20mm, R=20mm, w=4mm, L=20mm):
    //   outer Pipe(axis, R+w/2=0.022) minus inner Pipe(axis, R-w/2=0.018)
    // V = 2π * R * w * L = 2π * 0.020 * 0.004 * 0.020
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
    let outer_h = kernel
        .execute(&GeometryOp::Pipe {
            path: wire_h.id,
            radius: Value::Real(0.022), // R + w/2 = 0.020 + 0.002
        })
        .expect("outer Pipe execute should succeed");
    let inner_h = kernel
        .execute(&GeometryOp::Pipe {
            path: wire_h.id,
            radius: Value::Real(0.018), // R - w/2 = 0.020 - 0.002
        })
        .expect("inner Pipe execute should succeed");
    let annulus_h = kernel
        .execute(&GeometryOp::Difference {
            left: outer_h.id,
            right: inner_h.id,
        })
        .expect("Difference execute should succeed");
    let vol = kernel
        .query(&GeometryQuery::Volume(annulus_h.id))
        .expect("Volume query should succeed");
    let v = vol.as_f64().expect("volume should be numeric");
    assert!(v > 0.0, "zone_annulus volume must be positive, got {}", v);
    // V = 2π * R * w * L
    let r = 0.020_f64; // nominal radius
    let w = 0.004_f64; // zone width
    let l = 0.020_f64; // axis length
    let expected = 2.0 * std::f64::consts::PI * r * w * l;
    let rel_err = (v - expected).abs() / expected;
    assert!(
        rel_err < 0.01,
        "zone_annulus volume should be ≈{:.3e} m³ (2π·R·w·L), \
         got {:.3e} m³ (rel_err={:.4e})",
        expected,
        v,
        rel_err
    );
}

// ─── zone_profile ────────────────────────────────────────────────────────────

/// Structural test: `zone_profile(box(10mm,10mm,10mm), 1mm)` lowers to
/// [Primitive{Box}, Modify{OffsetSolid,target:Step(0),distance=+0.0005},
///  Modify{OffsetSolid,target:Step(0),distance=-0.0005},
///  Boolean{Difference,left:Step(1),right:Step(2)}].
///
/// Both OffsetSolid ops target the same box (Step(0)); distances are ±width/2 = ±0.5mm.
/// Always-run (no OCCT required).
#[test]
fn zone_profile_structural_lowers_to_four_ops() {
    let source = r#"structure S {
    let z = zone_profile(box(10mm, 10mm, 10mm), 1mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("zone_profile_structural"));
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
        "zone_profile should compile with no error-severity diagnostics, got: {:#?}",
        errors
    );

    // ── Compiled-realization shape ──
    assert_eq!(compiled.templates.len(), 1, "expected 1 template");
    let realization = &compiled.templates[0].realizations[0];
    assert_eq!(
        realization.operations.len(),
        4,
        "expected 4 ops [Box, OffsetSolid(+w/2), OffsetSolid(-w/2), Boolean(Difference)], got {}",
        realization.operations.len()
    );
    assert!(
        matches!(
            &realization.operations[0],
            CompiledGeometryOp::Primitive {
                kind: PrimitiveKind::Box,
                ..
            }
        ),
        "op[0] should be Primitive(Box), got {:?}",
        &realization.operations[0]
    );
    // op[1]: outer OffsetSolid (+w/2), targets the box at Step(0)
    assert!(
        matches!(
            &realization.operations[1],
            CompiledGeometryOp::Modify {
                kind: ModifyKind::OffsetSolid,
                target: GeomRef::Step(0),
                ..
            }
        ),
        "op[1] should be Modify(OffsetSolid, target=Step(0)), got {:?}",
        &realization.operations[1]
    );
    // op[2]: inner OffsetSolid (-w/2), also targets the box at Step(0)
    assert!(
        matches!(
            &realization.operations[2],
            CompiledGeometryOp::Modify {
                kind: ModifyKind::OffsetSolid,
                target: GeomRef::Step(0),
                ..
            }
        ),
        "op[2] should be Modify(OffsetSolid, target=Step(0)), got {:?}",
        &realization.operations[2]
    );
    // op[3]: Boolean{Difference, left:Step(1)=plus_offset, right:Step(2)=minus_offset}
    assert!(
        matches!(
            &realization.operations[3],
            CompiledGeometryOp::Boolean {
                op: BooleanOp::Difference,
                left: GeomRef::Step(1),
                right: GeomRef::Step(2),
            }
        ),
        "op[3] should be Boolean(Difference, Step(1), Step(2)), got {:?}",
        &realization.operations[3]
    );

    // ── MockGeometryKernel: verify runtime distances and Difference ──
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _ = engine.build(&compiled, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        4,
        "engine should dispatch 4 ops (Box, OffsetSolid+, OffsetSolid-, Difference), got {}",
        ops.len()
    );
    // op[1]: outer OffsetSolid, distance = +w/2 = +1mm/2 = +0.0005m
    match &ops[1].op {
        GeometryOp::OffsetSolid { distance, .. } => {
            let d = distance.as_f64().expect("distance should be numeric");
            assert!(
                (d - 0.0005).abs() < 1e-9,
                "outer OffsetSolid distance should be +0.0005 m (+w/2 = +1mm/2), got {}",
                d
            );
        }
        other => panic!(
            "expected GeometryOp::OffsetSolid at op[1] (plus), got {:?}",
            other
        ),
    }
    // op[2]: inner OffsetSolid, distance = -w/2 = -0.0005m
    match &ops[2].op {
        GeometryOp::OffsetSolid { distance, .. } => {
            let d = distance.as_f64().expect("distance should be numeric");
            assert!(
                (d + 0.0005).abs() < 1e-9,
                "inner OffsetSolid distance should be -0.0005 m (-w/2 = -1mm/2), got {}",
                d
            );
        }
        other => panic!(
            "expected GeometryOp::OffsetSolid at op[2] (minus), got {:?}",
            other
        ),
    }
    // op[3]: Boolean(Difference)
    match &ops[3].op {
        GeometryOp::Difference { .. } => {}
        other => panic!("expected GeometryOp::Difference at op[3], got {:?}", other),
    }
}

fn occt_engine() -> reify_eval::Engine {
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    reify_eval::Engine::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(planner)),
    )
}

/// Compiles `source`; `None` when OCCT is unavailable, so the oracle skips.
fn compile_for_occt(source: &str) -> Option<reify_compiler::CompiledModule> {
    let compiled = parse_and_compile_with_stdlib(source);
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping zone_profile OCCT oracle: OCCT not available");
        return None;
    }
    Some(compiled)
}

/// The realized value of `compiled`'s `S.v` cell, which must be a `volume()`.
fn realized_volume(
    engine: &mut reify_eval::Engine,
    compiled: &reify_compiler::CompiledModule,
) -> f64 {
    let result = engine.build(compiled, ExportFormat::Step);
    let errors = collect_errors(&result.diagnostics);
    assert!(errors.is_empty(), "unexpected build errors: {errors:#?}");
    match result.values.get(&ValueCellId::new("S", "v")) {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::VOLUME,
                "volume() cell must have VOLUME dimension"
            );
            *si_value
        }
        other => panic!("expected a Value::Scalar volume in cell S.v, got {other:?}"),
    }
}

/// [`realized_volume`] on a fresh engine; `None` when OCCT is unavailable.
fn realized_volume_of(source: &str) -> Option<f64> {
    let compiled = compile_for_occt(source)?;
    Some(realized_volume(&mut occt_engine(), &compiled))
}

/// Every face of the 10mm box moves ±0.5mm, so the zone is (11mm)³ − (9mm)³.
#[test]
fn zone_profile_volume_matches_formula() {
    let Some(compiled) = compile_for_occt(
        r#"structure S {
    let z = zone_profile(box(10mm, 10mm, 10mm), 1mm)
    let v = volume(z)
}"#,
    ) else {
        return;
    };
    let mut engine = occt_engine();
    let v = realized_volume(&mut engine, &compiled);
    let box_volume = 0.010_f64.powi(3);
    assert!(
        v < box_volume,
        "zone_profile volume {v:.3e} m³ should be below the box volume {box_volume:.3e} m³"
    );
    assert_rel(
        v,
        0.011_f64.powi(3) - 0.009_f64.powi(3),
        1e-9,
        "zone_profile(box 10mm, 1mm)",
    );

    let tess = engine.tessellate_realizations(&compiled);
    let errors = collect_errors(&tess.diagnostics);
    assert!(
        errors.is_empty(),
        "unexpected tessellate errors: {errors:#?}"
    );
    let mesh = &tess
        .meshes
        .first()
        .expect("zone_profile should produce a mesh")
        .mesh;
    assert!(
        !mesh.vertices.is_empty(),
        "zone_profile mesh should have vertices"
    );
    assert!(
        !mesh.indices.is_empty(),
        "zone_profile mesh should have triangles"
    );
}

/// Cylinder r=5mm h=10mm: π(5.5²·11 − 4.5²·9) mm³. Curved faces integrate
/// numerically, hence the looser tolerance.
#[test]
fn zone_profile_on_a_curved_solid_matches_formula() {
    let Some(v) = realized_volume_of(
        r#"structure S {
    let z = zone_profile(cylinder(5mm, 10mm), 1mm)
    let v = volume(z)
}"#,
    ) else {
        return;
    };
    let expected = std::f64::consts::PI * (0.0055_f64.powi(2) * 0.011 - 0.0045_f64.powi(2) * 0.009);
    assert_rel(v, expected, 1e-6, "zone_profile(cylinder r5 h10, 1mm)");
}

/// A centred cross (two crossing boxes) has concave edges; its offsets are
/// the crosses of the ±0.5mm-offset boxes.
#[test]
fn zone_profile_on_a_concave_solid_matches_formula() {
    let Some(v) = realized_volume_of(
        r#"structure S {
    let z = zone_profile(union(box(20mm, 10mm, 10mm), box(10mm, 10mm, 20mm)), 1mm)
    let v = volume(z)
}"#,
    ) else {
        return;
    };
    let cross = |long: f64, short: f64| 2.0 * long * short * short - short.powi(3);
    let expected = cross(0.021, 0.011) - cross(0.019, 0.009);
    assert_rel(v, expected, 1e-9, "zone_profile(cross, 1mm)");
}

/// A 12mm width offsets the 10mm box inward by 6mm, past its 5mm inradius.
/// Realized without export: STEP export already rejects an empty shape, which
/// would mask a zone that silently came out empty.
#[test]
fn zone_profile_wider_than_the_solid_reports_an_error() {
    let Some(compiled) = compile_for_occt(
        r#"structure S {
    let z = zone_profile(box(10mm, 10mm, 10mm), 12mm)
    let v = volume(z)
}"#,
    ) else {
        return;
    };
    let tess = occt_engine().tessellate_realizations(&compiled);
    assert!(
        collect_errors(&tess.diagnostics)
            .iter()
            .any(|error| error.message.contains("offset_solid_shape")),
        "zone_profile wider than the solid should report the offset collapse as an Error, got: {:#?}",
        tess.diagnostics
    );
}
