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

// Imports for all three constructors (zone_cylinder + zone_annulus + zone_profile).
use reify_compiler::{
    BooleanOp, CompiledGeometryOp, CurveKind, GeomRef, ModifyKind, PrimitiveKind, SweepKind,
};
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

// ─── zone_profile (step 5 RED / step 6 GREEN) ────────────────────────────────

/// Structural test: `zone_profile(box(10mm,10mm,10mm), 1mm)` lowers to
/// [Primitive{Box}, Modify{Thicken,target:Step(0),offset=+0.0005},
///  Modify{Thicken,target:Step(0),offset=-0.0005},
///  Boolean{Difference,left:Step(1),right:Step(2)}].
///
/// Both Thicken ops target the same box (Step(0)); offsets are ±width/2 = ±0.5mm.
/// Always-run (no OCCT required). RED until step-6 registers zone_profile.
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
        "expected 4 ops [Box, Thicken(+w/2), Thicken(-w/2), Boolean(Difference)], got {}",
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
    // op[1]: outer Thicken (+w/2), targets the box at Step(0)
    assert!(
        matches!(
            &realization.operations[1],
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                target: GeomRef::Step(0),
                ..
            }
        ),
        "op[1] should be Modify(Thicken, target=Step(0)), got {:?}",
        &realization.operations[1]
    );
    // op[2]: inner Thicken (-w/2), also targets the box at Step(0)
    assert!(
        matches!(
            &realization.operations[2],
            CompiledGeometryOp::Modify {
                kind: ModifyKind::Thicken,
                target: GeomRef::Step(0),
                ..
            }
        ),
        "op[2] should be Modify(Thicken, target=Step(0)), got {:?}",
        &realization.operations[2]
    );
    // op[3]: Boolean{Difference, left:Step(1)=plus_thicken, right:Step(2)=minus_thicken}
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

    // ── MockGeometryKernel: verify runtime offsets and Difference ──
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _ = engine.build(&compiled, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        4,
        "engine should dispatch 4 ops (Box, Thicken+, Thicken-, Difference), got {}",
        ops.len()
    );
    // op[1]: outer Thicken, offset = +w/2 = +1mm/2 = +0.0005m
    match &ops[1].op {
        GeometryOp::Thicken { offset, .. } => {
            let o = offset.as_f64().expect("offset should be numeric");
            assert!(
                (o - 0.0005).abs() < 1e-9,
                "outer Thicken offset should be +0.0005 m (+w/2 = +1mm/2), got {}",
                o
            );
        }
        other => panic!(
            "expected GeometryOp::Thicken at op[1] (plus), got {:?}",
            other
        ),
    }
    // op[2]: inner Thicken, offset = -w/2 = -0.0005m
    match &ops[2].op {
        GeometryOp::Thicken { offset, .. } => {
            let o = offset.as_f64().expect("offset should be numeric");
            assert!(
                (o + 0.0005).abs() < 1e-9,
                "inner Thicken offset should be -0.0005 m (-w/2 = -1mm/2), got {}",
                o
            );
        }
        other => panic!(
            "expected GeometryOp::Thicken at op[2] (minus), got {:?}",
            other
        ),
    }
    // op[3]: Boolean(Difference)
    match &ops[3].op {
        GeometryOp::Difference { .. } => {}
        other => panic!("expected GeometryOp::Difference at op[3], got {:?}", other),
    }
}

/// OCCT contract pin: `zone_profile` on a CLOSED SOLID is a HARD ERROR.
///
/// `zone_profile(box(10mm,10mm,10mm), 1mm)` lowers to
/// `Difference(Thicken(+w/2), Thicken(-w/2))`.  OCCT's
/// `BRepOffsetAPI_MakeOffsetShape::PerformBySimple` is documented for OPEN
/// SHELLS; on a closed solid it does not yield a valid solid for the cut, so
/// that Difference is GENUINELY EMPTY — zero `TopAbs_VERTEX`, zero volume.
/// The lowering is therefore non-functional on closed solids, not merely
/// imprecise.
///
/// That emptiness used to be SILENT: the empty result was stamped a solid, a
/// 0.0 volume flowed downstream, and this test tolerated it (asserting only
/// `v >= 0.0 && v <= box_volume`, both of which 0.0 satisfies).  Task 5318's
/// empty-boolean-result guard converts exactly that class of silent-empty
/// result into a diagnostic, so what this test pins is now the ERROR, not a
/// volume: the engine surfaces a `Severity::Error` naming the empty result,
/// and the direct `OcctKernel` replay returns
/// `Err(GeometryError::OperationFailed(..))`.
///
/// The underlying geometry defect — zone_profile does not actually build the
/// annular shell — is tracked by #7287 (replace the PerformBySimple
/// Thicken+Difference lowering with a MakeThickSolid-based construction).
/// When #7287 lands, this test should go back to asserting
/// `0.0 < volume < box_volume` (~6e-7 m³); the `Ok(_)` arm below says so.
///
/// Message matching is deliberately loose (substrings, not whole sentences)
/// so re-wording the diagnostic does not make this test brittle.
///
/// Parallel OcctKernel replay: Box + Thicken(+0.5mm) + Thicken(-0.5mm) + Difference.
/// OCCT-gated; skips cleanly when OCCT is unavailable.
#[test]
fn zone_profile_realize_smoke() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping zone_profile_realize_smoke: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let z = zone_profile(box(10mm, 10mm, 10mm), 1mm)
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("zone_profile_smoke"));
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

    // ── Full-pipeline: Engine + OcctKernelHandle — the guard must be VISIBLE ──
    //
    // The failure has to reach the designer as a diagnostic, not just as a
    // zero-volume solid; that is the whole point of the 5318 guard.
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
    let empty_result_error = geom_errors
        .iter()
        .find(|d| d.message.contains("empty result"))
        .unwrap_or_else(|| {
            panic!(
                "expected a Severity::Error diagnostic reporting the empty boolean result; \
                 if #7287 has landed and zone_profile now builds, restore the \
                 0.0 < volume < box_volume assertions instead. Diagnostics: {:?}",
                tess_result.diagnostics
            )
        });
    assert!(
        empty_result_error.message.contains("difference"),
        "the diagnostic should name the user-level operation ('difference'), got: {}",
        empty_result_error.message
    );

    // ── Direct OcctKernel replay — the same empty Difference at kernel level ──
    //
    // Box + both Thickens still succeed, so this also keeps pinning that the
    // compile-to-kernel lowering is wired correctly; only the final cut is
    // empty.
    let box_side = 0.010_f64; // 10mm in metres

    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(box_side),
            height: Value::Real(box_side),
            depth: Value::Real(box_side),
        })
        .expect("Box execute should succeed");
    let outer_h = kernel
        .execute(&GeometryOp::Thicken {
            target: box_h.id,
            offset: Value::Real(0.0005), // +w/2 = +1mm/2 = +0.5mm
        })
        .expect("outer Thicken execute should succeed");
    let inner_h = kernel
        .execute(&GeometryOp::Thicken {
            target: box_h.id,
            offset: Value::Real(-0.0005), // -w/2 = -1mm/2 = -0.5mm
        })
        .expect("inner Thicken execute should succeed");

    let profile = kernel.execute(&GeometryOp::Difference {
        left: outer_h.id,
        right: inner_h.id,
    });
    match profile {
        Err(reify_ir::GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("empty result"),
                "expected a message naming the result as empty, got: {msg}"
            );
            assert!(
                msg.contains("difference"),
                "expected a message naming 'difference', got: {msg}"
            );
        }
        Ok(_) => panic!(
            "zone_profile's Thicken+Difference lowering produced a NON-empty result — \
             if #7287 has landed, this test should now assert \
             0.0 < volume < box_volume ({:.3e} m³, expected ~6e-7) instead of this error",
            box_side.powi(3)
        ),
        Err(other) => {
            panic!("expected OperationFailed for the empty zone_profile Difference, got {other:?}")
        }
    }
}
