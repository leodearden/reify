//! End-to-end tests for the extrude_symmetric() geometry operation.
//!
//! Tests span from the compiled IR through evaluation to the final GeometryOp
//! using MockGeometryKernel to capture executed operations without OCCT.

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_core::Type;
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::*;

/// Exercises the full compile -> eval path for ExtrudeSymmetric.
///
/// Creates a module with 2 ops:
///   Op 0: Sphere (serves as a stand-in profile, produces a handle)
///   Op 1: Sweep(ExtrudeSymmetric) referencing Step(0) as profile,
///         with distance=10mm
///
/// Verifies that the ExtrudeSymmetric operation receives the correct
/// profile handle and a distance of ~0.01 m (10 mm SI).
#[test]
fn extrude_symmetric_through_full_eval_pipeline() {
    let e = "TestExtrudeSymmetric";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (produces handle at step index 0)
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: ExtrudeSymmetric referencing Step(0) as profile, distance = 10mm
    let extrude_sym_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeSymmetric,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)), // placeholder expr
            ("distance".into(), mm_literal(10.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_sym_op])
        .build();

    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extsym"))
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

    let profile_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::ExtrudeSymmetric { profile, distance } => {
            assert_eq!(
                *profile, profile_handle,
                "ExtrudeSymmetric profile should be handle from op 0 ({:?}), got {:?}",
                profile_handle, profile
            );
            let dist_si = distance
                .as_f64()
                .expect("distance should be a numeric value");
            assert!(
                (dist_si - 0.01).abs() < 1e-9,
                "ExtrudeSymmetric distance should be 0.01 m (10 mm SI), got {}",
                dist_si
            );
        }
        other => panic!(
            "expected GeometryOp::ExtrudeSymmetric at op index 1, got {:?}",
            other
        ),
    }
}

/// Non-finite distance (NaN) should be rejected by the eval layer: the
/// ExtrudeSymmetric op is dropped (not present in executed operations),
/// mirroring Extrude's degeneracy check.
#[test]
fn extrude_symmetric_non_finite_distance_is_dropped() {
    let e = "TestExtrudeSymNaN";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    // Construct a NaN Value::Scalar for distance.
    let nan_val = reify_ir::Value::Scalar {
        si_value: f64::NAN,
        dimension: reify_core::DimensionVector::LENGTH,
    };
    let nan_expr = reify_ir::CompiledExpr::literal(nan_val, Type::length());

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_sym_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeSymmetric,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), nan_expr),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_sym_op])
        .build();
    let module = CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extsym_nan"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    // Sphere op should still execute, but ExtrudeSymmetric should be dropped.
    let any_extsym = ops
        .iter()
        .any(|o| matches!(o.op, GeometryOp::ExtrudeSymmetric { .. }));
    assert!(
        !any_extsym,
        "expected ExtrudeSymmetric to be dropped on non-finite distance, \
         but it was executed: {:?}",
        ops.iter().map(|o| &o.op).collect::<Vec<_>>()
    );
}

/// Per-side-threshold boundary: distance = 1.5e-12 m means the per-side
/// (half-) distance = 0.75e-12 m, which is below the `DEGENERATE_LENGTH_M`
/// (1e-12) floor. Under per-side semantics this must be rejected at compile
/// time: the ExtrudeSymmetric op is dropped from the kernel ops, and the
/// diagnostics contain a Warning that names the per-side / half-distance
/// semantics so model authors see a specific explanation.
///
/// Fails under the previous total-distance threshold (`|v| >= 1e-12`), which
/// would admit 1.5e-12 even though the per-side magnitude is sub-picometer.
#[test]
fn extrude_symmetric_per_side_just_below_threshold_rejected() {
    let e = "TestExtrudeSymPerSideBelow";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    // distance = 1.5e-12 m; per-side = 0.75e-12 m — below the DEGENERATE_LENGTH_M floor.
    let tiny_val = reify_ir::Value::Scalar {
        si_value: 1.5e-12,
        dimension: reify_core::DimensionVector::LENGTH,
    };
    let tiny_expr = reify_ir::CompiledExpr::literal(tiny_val, Type::length());

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_sym_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeSymmetric,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), tiny_expr),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_sym_op])
        .build();
    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extsym_per_side_below"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    let any_extsym = ops
        .iter()
        .any(|o| matches!(o.op, GeometryOp::ExtrudeSymmetric { .. }));
    assert!(
        !any_extsym,
        "expected ExtrudeSymmetric to be dropped for per-side-below-threshold distance, \
         but it was executed: {:?}",
        ops.iter().map(|o| &o.op).collect::<Vec<_>>()
    );

    // Diagnostics must explain the per-side semantics so model authors
    // understand that halving a small total distance is what triggered the
    // drop — a plain "distance degenerate" message would be misleading.
    let has_per_side_warning = result.diagnostics.iter().any(|d| {
        matches!(d.severity, reify_core::Severity::Warning)
            && d.message.contains("extrude_symmetric")
            && (d.message.contains("per-side") || d.message.contains("half-distance"))
    });
    assert!(
        has_per_side_warning,
        "expected a Warning mentioning 'extrude_symmetric' and 'per-side' or 'half-distance', \
         got diagnostics: {:?}",
        result.diagnostics
    );

    // The paired Error diagnostic (from `Err(...)` returned by compile_geometry_op,
    // wrapped as "failed to compile geometry operation: ..." by engine_build) must
    // also name the specific op — `extrude_symmetric`, not generic `extrude` — so
    // the two diagnostic channels stay in sync. Locks the Err-string fix so a
    // future refactor can't silently regress the Err back to "extrude distance ...".
    let has_extsym_error = result.diagnostics.iter().any(|d| {
        matches!(d.severity, reify_core::Severity::Error)
            && d.message
                .contains("extrude_symmetric distance is degenerate")
    });
    assert!(
        has_extsym_error,
        "expected an Error diagnostic containing 'extrude_symmetric distance is degenerate' \
         (not the generic 'extrude distance ...'), got diagnostics: {:?}",
        result.diagnostics
    );
}

/// Per-side threshold accepted boundary: distance = 2e-12 m means the
/// per-side half-distance = 1e-12 m exactly, hitting the documented
/// `DEGENERATE_LENGTH_M` floor. Pins the `>=` (inclusive) boundary: if a
/// future refactor flips this to `>`, this test fails.
#[test]
fn extrude_symmetric_per_side_at_threshold_accepted() {
    let e = "TestExtrudeSymPerSideAt";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    // distance = 2e-12 m; per-side = 1e-12 m — exactly at the DEGENERATE_LENGTH_M floor.
    let at_val = reify_ir::Value::Scalar {
        si_value: 2e-12,
        dimension: reify_core::DimensionVector::LENGTH,
    };
    let at_expr = reify_ir::CompiledExpr::literal(at_val, Type::length());

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_sym_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeSymmetric,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), at_expr),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_sym_op])
        .build();
    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extsym_per_side_at"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    let any_extsym = ops
        .iter()
        .any(|o| matches!(o.op, GeometryOp::ExtrudeSymmetric { .. }));
    assert!(
        any_extsym,
        "expected ExtrudeSymmetric to be forwarded at per-side threshold (distance=2e-12 m), \
         but it was dropped; executed ops: {:?}",
        ops.iter().map(|o| &o.op).collect::<Vec<_>>()
    );
}

/// Negative-sign symmetric boundary: distance = -1.5e-12 m. Absolute value
/// 1.5e-12 fails the 2 * DEGENERATE_LENGTH_M = 2e-12 floor. Must be
/// rejected — if a future refactor drops `.abs()` from the guard and
/// compares the raw value, -1.5e-12 would trivially fail `>= 2e-12` and
/// still appear "correct", but a positive 1.5e-12 would incorrectly pass
/// the sign-asymmetric check. This test, together with the positive
/// `_per_side_just_below_threshold_rejected`, locks sign-symmetric
/// semantics at both boundaries.
#[test]
fn extrude_symmetric_negative_per_side_just_below_threshold_rejected() {
    let e = "TestExtrudeSymNegBelow";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let neg_val = reify_ir::Value::Scalar {
        si_value: -1.5e-12,
        dimension: reify_core::DimensionVector::LENGTH,
    };
    let neg_expr = reify_ir::CompiledExpr::literal(neg_val, Type::length());

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_sym_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeSymmetric,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), neg_expr),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_sym_op])
        .build();
    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extsym_neg_below"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    let any_extsym = ops
        .iter()
        .any(|o| matches!(o.op, GeometryOp::ExtrudeSymmetric { .. }));
    assert!(
        !any_extsym,
        "expected ExtrudeSymmetric to be dropped for negative per-side-below-threshold \
         (distance=-1.5e-12), but it was executed: {:?}",
        ops.iter().map(|o| &o.op).collect::<Vec<_>>()
    );
}

/// Negative-sign forward path: distance = -10mm is well above the
/// 2 * DEGENERATE_LENGTH_M floor. The op must be forwarded AND the
/// distance preserved as negative — the sign indicates extrusion
/// direction (symmetric about the profile, but the overall orientation
/// flips). If a refactor inadvertently applies `.abs()` to the value
/// passed to the kernel, this test catches it.
#[test]
fn extrude_symmetric_distance_negative_above_threshold_accepted() {
    let e = "TestExtrudeSymNegAbove";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };
    let extrude_sym_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::ExtrudeSymmetric,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            // distance = -10 mm = -0.01 m (well above 2e-12 floor).
            ("distance".into(), mm_literal(-10.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_sym_op])
        .build();
    let module =
        CompiledModuleBuilder::new(reify_core::ModulePath::single("test_extsym_neg_above"))
            .template(template)
            .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let _result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    // Find the ExtrudeSymmetric op and verify it was forwarded AND the
    // negative sign preserved.
    let extsym = ops
        .iter()
        .find_map(|o| match &o.op {
            GeometryOp::ExtrudeSymmetric { distance, .. } => Some(distance.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            panic!(
                "expected ExtrudeSymmetric to be forwarded for negative above-threshold \
                 distance, but it was dropped; executed ops: {:?}",
                ops.iter().map(|o| &o.op).collect::<Vec<_>>()
            )
        });
    let dist_si = extsym.as_f64().expect("distance should be a numeric value");
    assert!(
        (dist_si - (-0.01)).abs() < 1e-9,
        "ExtrudeSymmetric distance should preserve negative sign (-0.01 m), got {}",
        dist_si
    );
}
