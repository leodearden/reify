//! Stress tests for degenerate geometry sweep operations.
//!
//! Covers:
//!   - zero_extrude_distance: evaluator skips extrude when distance=0
//!   - revolve_720_degrees: valid edge-case double full revolution (4π)
//!   - negative_extrude_distance_is_valid: negative distance passes through (reverse direction)
//!   - negative_revolve_angle_is_valid: negative angle passes through (clockwise rotation)
//!   - loft_one_profile_rejected: compiler rejects loft with < 2 profiles
//!   - self_intersecting_path_sweep: kernel failure produces diagnostic
//!   - sweep_degenerate_ri_parses: fixture parses and compiles without errors

use std::f64::consts::PI;

use reify_compiler::{CompiledGeometryOp, GeomRef, PrimitiveKind, SweepKind};
use reify_core::{ModulePath, Severity, Type};
use reify_ir::{ExportFormat, GeometryOp, Value};
use reify_test_support::*;

// ---------------------------------------------------------------------------
// step-13: zero_extrude_distance — failing test
// ---------------------------------------------------------------------------

/// Build an Extrude op with distance=0mm (degenerate).
/// After step-14 the evaluator should skip zero-distance extrudes
/// (compile_geometry_op returns None), so no Extrude op reaches the kernel.
///
/// FAILS before step-14 because the evaluator currently dispatches all extrudes.
#[test]
fn zero_extrude_distance() {
    let e = "TestZeroExtrude";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (profile provider at Step(0))
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Extrude referencing Step(0), distance = 0mm (degenerate)
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("distance".into(), mm_literal(0.0)), // ZERO — degenerate
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_zero_extrude"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    // Zero-distance extrude should be skipped — kernel should receive no Extrude ops.
    let extrude_ops: Vec<_> = ops
        .iter()
        .filter(|o| matches!(&o.op, GeometryOp::Extrude { .. }))
        .collect();
    assert!(
        extrude_ops.is_empty(),
        "zero-distance extrude should be skipped by the evaluator, \
         but kernel received {} Extrude op(s): {:?}",
        extrude_ops.len(),
        extrude_ops.iter().map(|o| &o.op).collect::<Vec<_>>()
    );

    // The degenerate-extrude warning must propagate through Engine::build to
    // BuildResult.diagnostics so model authors see a specific explanation
    // (not only the generic "failed to compile geometry operation" error).
    // A regression that drops the warning on its way out would not be caught
    // by the kernel-ops check above.
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, Severity::Warning)
                && d.message.contains("extrude dropped")),
        "expected a Warning containing 'extrude dropped' in BuildResult.diagnostics, got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.severity, &d.message))
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-17/18: revolve_720_degrees — edge-case valid operation
// ---------------------------------------------------------------------------

/// Build a Revolve op with angle=4π (720° — double full revolution).
/// This is a valid but edge-case operation (double full revolution).
/// The evaluator should dispatch it with angle_rad ≈ 4π.
#[test]
fn revolve_720_degrees() {
    let e = "TestRevolve720";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal = |v: f64| reify_ir::CompiledExpr::literal(Value::Real(v), Type::dimensionless_scalar());

    // Op 0: Sphere (profile provider at Step(0))
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    let angle_720_rad = 4.0 * PI; // 720° in radians

    // Op 1: Revolve around z-axis with angle=4π (720°)
    let revolve_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Revolve,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)), // z-axis
            ("angle".into(), real_literal(angle_720_rad)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, revolve_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_revolve_720"))
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
        "expected 2 geometry ops (sphere + revolve), got {}",
        ops.len()
    );

    match &ops[1].op {
        GeometryOp::Revolve { angle_rad, .. } => {
            assert!(
                (angle_rad - angle_720_rad).abs() < 1e-9,
                "Revolve angle should be 4π ({}) radians, got {}",
                angle_720_rad,
                angle_rad
            );
        }
        other => panic!(
            "expected GeometryOp::Revolve at op index 1, got {:?}",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// step-1: negative_extrude_distance_is_valid — characterization test
// ---------------------------------------------------------------------------

/// Build an Extrude op with distance=-10mm (negative — reverse direction).
///
/// A negative extrude distance is a valid, physically meaningful operation:
/// it means "extrude along the *negative* profile normal (reverse direction)".
/// The evaluator's rejection threshold (`lib.rs` line 3647) uses `.abs()`:
///   `distance.as_f64().filter(|v| v.is_finite() && v.abs() >= 1e-12)?`
/// so finite negatives with magnitude ≥ 1e-12 pass through unchanged.
///
/// Complementary unit test: `compile_geometry_op_extrude_near_zero_distance_returns_none`
/// in `lib.rs` (line 4481) proves *magnitude-zero* rejects; this test proves
/// *negative-magnitude* dispatches.
///
/// This test will fail if someone changes `v.abs() >= 1e-12` to `v >= 1e-12`,
/// which would silently regress the reverse-direction-extrude use case.
#[test]
fn negative_extrude_distance_is_valid() {
    let e = "TestNegExtrude";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (profile provider at Step(0))
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Extrude referencing Step(0), distance = -10mm (reverse direction — valid)
    let extrude_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Extrude,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),    // placeholder expr
            ("distance".into(), mm_literal(-10.0)), // NEGATIVE — reverse direction
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, extrude_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_neg_extrude"))
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
        "expected 2 geometry ops (sphere + extrude), got {}",
        ops.len()
    );

    let profile_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::Extrude { profile, distance } => {
            assert_eq!(
                *profile, profile_handle,
                "Extrude profile should be handle from op 0 ({:?}), got {:?}",
                profile_handle, profile
            );
            let dist_si = distance
                .as_f64()
                .expect("distance should be a numeric value");
            // Sign must be preserved — negative means reverse direction, not normalized to abs.
            assert!(
                dist_si < 0.0,
                "Extrude distance must be negative (reverse direction), got {}",
                dist_si
            );
            assert!(
                (dist_si - (-0.01)).abs() < 1e-9,
                "Extrude distance should be -0.01 m (-10 mm in SI), got {}",
                dist_si
            );
        }
        other => panic!(
            "expected GeometryOp::Extrude at op index 1, got {:?}",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// step-2: negative_revolve_angle_is_valid — characterization test
// ---------------------------------------------------------------------------

/// Build a Revolve op with angle=-π (−180° — clockwise rotation under right-hand rule).
///
/// A negative revolve angle is a valid, physically meaningful operation:
/// under the right-hand rule, a negative angle means "clockwise rotation about
/// the axis". The evaluator's rejection threshold (`lib.rs` line 3667) uses `.abs()`:
///   `if angle_rad.abs() < 1e-12 { return None; }`
/// so finite negatives with magnitude ≥ 1e-12 pass through unchanged.
///
/// Complementary unit test: `compile_geometry_op_revolve_near_zero_angle_returns_none`
/// in `lib.rs` (line 4551) proves *magnitude-zero* rejects; this test proves
/// *negative-magnitude* dispatches.
///
/// This test will fail if someone changes `angle_rad.abs() < 1e-12` to
/// `angle_rad < 1e-12`, which would silently regress the clockwise-revolve use case.
#[test]
fn negative_revolve_angle_is_valid() {
    let e = "TestNegRevolve";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());
    let real_literal = |v: f64| reify_ir::CompiledExpr::literal(Value::Real(v), Type::dimensionless_scalar());

    // Op 0: Sphere (profile provider at Step(0))
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    let angle_neg_180_rad = -PI; // -180° in radians (clockwise)

    // Op 1: Revolve around z-axis with angle=-π (clockwise — valid)
    let revolve_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Revolve,
        profiles: vec![GeomRef::Step(0)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("ox".into(), real_literal(0.0)),
            ("oy".into(), real_literal(0.0)),
            ("oz".into(), real_literal(0.0)),
            ("ax".into(), real_literal(0.0)),
            ("ay".into(), real_literal(0.0)),
            ("az".into(), real_literal(1.0)), // z-axis
            ("angle".into(), real_literal(angle_neg_180_rad)), // NEGATIVE angle — clockwise
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op, revolve_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_neg_revolve"))
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
        "expected 2 geometry ops (sphere + revolve), got {}",
        ops.len()
    );

    match &ops[1].op {
        GeometryOp::Revolve { angle_rad, .. } => {
            // Sign must be preserved — negative means clockwise, not normalized to abs.
            assert!(
                *angle_rad < 0.0,
                "Revolve angle must be negative (clockwise rotation), got {}",
                angle_rad
            );
            assert!(
                (angle_rad - angle_neg_180_rad).abs() < 1e-9,
                "Revolve angle should be -π ({}) radians, got {}",
                angle_neg_180_rad,
                angle_rad
            );
        }
        other => panic!(
            "expected GeometryOp::Revolve at op index 1, got {:?}",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// step-19/20: loft_one_profile_rejected — compiler rejects < 2 profiles
// ---------------------------------------------------------------------------

/// Verify the compiler rejects loft() with only 1 argument.
/// The compiler requires at least 2 profiles for a valid loft operation.
#[test]
fn loft_one_profile_rejected() {
    let source = r#"structure S {
    param profile: Length = 5mm
    let result = loft(profile)
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_loft_1"));
    // Parse may succeed (loft is syntactically valid with any arg count)
    let compiled = reify_compiler::compile(&parsed);

    // Compiler should produce an error about minimum 2 arguments
    let error_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !error_diags.is_empty(),
        "expected compiler error for loft(1 arg), got no diagnostics"
    );
    let has_min_args_msg = compiled
        .diagnostics
        .iter()
        .any(|d| d.message.contains("expects at least 2 arguments"));
    assert!(
        has_min_args_msg,
        "expected diagnostic containing 'expects at least 2 arguments', got: {:?}",
        compiled
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// task-1752: loft_non_geometry_profiles_silent_fallback
// ---------------------------------------------------------------------------

#[test]
fn loft_non_geometry_profiles_silent_fallback() {
    // Compiling loft(x, y) where x and y are scalar params (not geometry)
    // should silently fall back per profile (no diagnostic emitted), matching
    // the geom_ref convention used by extrude/revolve/translate/etc.
    // The op is still produced (with distinct GeomRef::Step indices per
    // profile, so loft's "distinct cross-sections" semantics are preserved
    // for downstream analysis), and no per-argument geometry-expression
    // error is added.
    let source = r#"structure S {
    param x: Length = 5
    param y: Length = 10
    let result = loft(x, y)
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test_loft_diag"));
    let compiled = reify_compiler::compile(&parsed);

    // No per-argument geometry-expression diagnostics should be emitted by
    // the loft fallback path. Filter by message content so the assertion is
    // robust to unrelated diagnostics elsewhere in the pipeline.
    let geom_expr_diags: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("must be a geometry expression")
                && d.message.contains("loft()")
        })
        .collect();
    assert!(
        geom_expr_diags.is_empty(),
        "expected loft() to silently fall back for non-geometry args (no per-arg \
         diagnostics), got: {:?}",
        geom_expr_diags
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-21/22: self_intersecting_path_sweep — kernel failure diagnostic
// ---------------------------------------------------------------------------

/// Simulate a sweep on a self-intersecting path by using FailingMockGeometryKernel.
/// When the kernel rejects the op, build() should return geometry_output=None
/// and include a diagnostic about all geometry operations failing.
#[test]
fn self_intersecting_path_sweep() {
    let e = "TestSelfIntersect";
    let mm_literal = |v: f64| reify_ir::CompiledExpr::literal(mm(v), Type::length());

    // Op 0: Sphere (stand-in profile)
    let sphere_op_0 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(5.0))],
    };

    // Op 1: Sphere (stand-in path)
    let sphere_op_1 = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_literal(3.0))],
    };

    // Op 2: Sweep referencing Step(0) as profile, Step(1) as path (self-intersecting)
    let sweep_op = CompiledGeometryOp::Sweep {
        kind: SweepKind::Sweep,
        profiles: vec![GeomRef::Step(0), GeomRef::Step(1)],
        args: vec![
            ("profile".into(), mm_literal(5.0)),
            ("path".into(), mm_literal(3.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .realization(e, 0, vec![sphere_op_0, sphere_op_1, sweep_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test_self_intersect"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, ExportFormat::Step);

    // Kernel failure → no geometry output
    assert!(
        result.geometry_output.is_none(),
        "expected geometry_output to be None when kernel fails, got Some({} bytes)",
        result.geometry_output.as_ref().map_or(0, |v| v.len())
    );

    // Should contain a summary diagnostic about all ops failing
    let has_failure_msg = result
        .diagnostics
        .iter()
        .any(|d| d.message.contains("all geometry operations failed"));
    assert!(
        has_failure_msg,
        "expected diagnostic 'all geometry operations failed', got: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// step-23/24: sweep_degenerate_ri_parses — fixture smoke test
// ---------------------------------------------------------------------------

/// Smoke test: verify sweep_degenerate.ri parses and compiles without errors.
/// The fixture contains valid geometry call syntax (extrude, revolve_full, loft)
/// that exercises the geometry function compiler paths.
#[test]
fn sweep_degenerate_ri_parses() {
    let source = std::fs::read_to_string("../../examples/sweep_degenerate.ri")
        .expect("sweep_degenerate.ri should exist");
    let parsed = reify_syntax::parse(&source, ModulePath::single("sweep_degenerate"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in sweep_degenerate.ri: {:?}",
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
        "compile errors in sweep_degenerate.ri: {:?}",
        errors
    );
    // Fixture has 3 valid structures (ValidExtrude, ValidRevolveFullTurn, ValidLoftThreeProfiles)
    assert!(
        !compiled.templates.is_empty(),
        "expected non-empty templates from sweep_degenerate.ri"
    );
}
