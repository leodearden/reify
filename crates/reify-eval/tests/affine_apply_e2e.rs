//! End-to-end tests for the `affine_apply(geometry, AffineMap<3>)` stdlib function
//! (task 3963, PRD docs/prds/v0_6/affine-map-type.md task ζ, §5/§9 Phase 3).
//!
//! Four signals, mirroring apply_transform_e2e.rs's structure:
//! 1. OCCT acceptance: a non-uniform scale doubles the tessellated Z extent
//!    while leaving X/Y unchanged (the §3/§9 user-observable signal).
//! 2. MOCK: a singular map (det=0) drops the op gracefully — one kernel op
//!    (Box only), a diagnostic naming the singular condition, no panic.
//! 3. OCCT acceptance: a reflection (det<0) still yields a valid solid.
//! 4. OCCT rigid-parity (§8.1): affine_apply(box, affine_from_transform(t))
//!    produces the same tessellated AABB as apply_transform(box, t) for a
//!    rigid t, within tolerance.

use reify_core::Severity;
use reify_ir::{ExportFormat, GeometryOp, Value};
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib, mesh_aabb,
};

/// `affine_apply(box(10mm,10mm,10mm), affine_scale(1.0, 1.0, 2.0))`: the
/// resulting solid's tessellated AABB must have Z-extent ≈ 20mm and
/// X/Y-extent ≈ 10mm (linear·x mapping, zero translation).
///
/// RED until "affine_apply" is recognised end-to-end: before that, the call
/// evaluates to Undef, geometry_output stays None, and the AABB assertions
/// below never run against real geometry.
#[test]
fn affine_apply_non_uniform_scale_occt_acceptance() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping affine_apply_non_uniform_scale_occt_acceptance: OCCT not available");
        return;
    }

    // Part 1: full compile-source pipeline reaches the OCCT kernel without error.
    let source = r#"structure S {
    let g = affine_apply(box(10mm, 10mm, 10mm), affine_scale(1.0, 1.0, 2.0))
}"#;
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "unexpected build errors: {:?}",
        build_errors
    );
    let output = result
        .geometry_output
        .expect("affine_apply must produce geometry output (non-Undef)");
    assert!(!output.is_empty(), "STEP output must be non-empty");

    // Part 2: direct kernel — tessellated AABB (the exact linear part the
    // compile→eval path produces, confirmed by Part 1 reaching real geometry).
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("Box execute must succeed");
    let transformed = kernel
        .execute(&GeometryOp::AffineApply {
            target: box_h.id,
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]],
            translation: [0.0, 0.0, 0.0],
        })
        .expect("AffineApply execute must succeed");
    let mesh = kernel
        .tessellate(transformed.id, 1e-4)
        .expect("tessellate must succeed");
    let (min, max) = mesh_aabb(&mesh);
    let tol = 1e-4_f32;
    assert!(
        (max[0] - min[0] - 0.01).abs() < tol,
        "X extent expected ≈0.01, got {}",
        max[0] - min[0]
    );
    assert!(
        (max[1] - min[1] - 0.01).abs() < tol,
        "Y extent expected ≈0.01, got {}",
        max[1] - min[1]
    );
    assert!(
        (max[2] - min[2] - 0.02).abs() < tol,
        "Z extent expected ≈0.02 (doubled by linear[2][2]=2.0), got {}",
        max[2] - min[2]
    );
}

/// `affine_apply(box, affine_map(matrix([[1,0,0],[0,1,0],[0,0,0]]), vec3(0mm,0mm,0mm)))`:
/// a singular linear part (det=0) must drop the op gracefully — one kernel op
/// (Box only, AffineApply dropped), a diagnostic naming the singular
/// condition, no panic and no non-manifold output.
///
/// Uses `affine_map` (not `affine_scale`, which itself rejects a zero factor
/// at construction — producing `Undef` rather than a genuinely singular
/// `Value::AffineMap`) to reach `transform_affine_apply`'s det==0 guard with
/// a real `Value::AffineMap`.
#[test]
fn affine_apply_singular_map_drops_gracefully() {
    let source = r#"structure S {
    let g = affine_apply(
        box(10mm, 10mm, 10mm),
        affine_map(matrix([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 0.0]]), vec3(0mm, 0mm, 0mm))
    )
}"#;
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        1,
        "singular affine_apply: expected 1 kernel op (Box only), got {} — {:?}",
        ops.len(),
        ops.iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );
    assert!(
        matches!(ops[0].op, GeometryOp::Box { .. }),
        "expected GeometryOp::Box at ops[0], got {:?}",
        ops[0].op
    );

    let singular_diag: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("singular"))
        .collect();
    assert!(
        !singular_diag.is_empty(),
        "expected a diagnostic mentioning 'singular'; got: {:?}",
        result.diagnostics
    );
}

/// `affine_apply(box(10mm,10mm,10mm), affine_scale(-1.0, 1.0, 1.0))`: a
/// reflection (det<0) is not a degenerate map and must yield a valid solid
/// (non-empty STEP output, no Error diagnostic).
#[test]
fn affine_apply_reflection_occt_acceptance() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping affine_apply_reflection_occt_acceptance: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let g = affine_apply(box(10mm, 10mm, 10mm), affine_scale(-1.0, 1.0, 1.0))
}"#;
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "unexpected compile errors: {:?}",
        compile_errors
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "reflection (det<0) must not error: {:?}",
        build_errors
    );
    let output = result
        .geometry_output
        .expect("reflection affine_apply must still produce geometry output");
    assert!(!output.is_empty(), "STEP output must be non-empty");
}

/// §8.1 rigid-parity: for a rigid transform `t`, the tessellated AABB of
/// `affine_apply(box, affine_from_transform(t))` must match the tessellated
/// AABB of `apply_transform(box, t)` within tolerance — the affine-map
/// widening bridge (`affine_from_transform`) must be geometry-preserving for
/// rigid inputs.
///
/// `t` = rotate 90° about Z + translate (5mm, 0, 0) — the same values
/// `apply_transform_e2e.rs`'s OCCT acceptance test uses.
#[test]
fn affine_apply_rigid_parity_with_apply_transform() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping affine_apply_rigid_parity_with_apply_transform: OCCT not available");
        return;
    }

    let w = std::f64::consts::FRAC_1_SQRT_2;
    let rotation = [w, 0.0, 0.0, w];
    let translation = [0.005, 0.0, 0.0];

    // affine_from_transform(t) — the SAME widening the compile→eval path
    // uses (reify_stdlib::geometry's "affine_from_transform" arm).
    let transform_value = Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: rotation[0],
            x: rotation[1],
            y: rotation[2],
            z: rotation[3],
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(translation[0]),
            Value::length(translation[1]),
            Value::length(translation[2]),
        ])),
    };
    let widened = reify_stdlib::eval_builtin("affine_from_transform", &[transform_value]);
    let (linear, affine_translation) = match widened {
        Value::AffineMap {
            linear,
            translation,
        } => (linear, translation),
        other => panic!(
            "expected Value::AffineMap from affine_from_transform, got {:?}",
            other
        ),
    };

    let mut kernel = reify_kernel_occt::OcctKernel::new();

    let box_a = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("Box execute must succeed");
    let rigid = kernel
        .execute(&GeometryOp::ApplyTransform {
            target: box_a.id,
            rotation,
            translation,
        })
        .expect("ApplyTransform execute must succeed");
    let rigid_mesh = kernel
        .tessellate(rigid.id, 1e-4)
        .expect("tessellate (ApplyTransform) must succeed");
    let (rigid_min, rigid_max) = mesh_aabb(&rigid_mesh);

    let box_b = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("Box execute must succeed");
    let affine = kernel
        .execute(&GeometryOp::AffineApply {
            target: box_b.id,
            linear,
            translation: affine_translation,
        })
        .expect("AffineApply execute must succeed");
    let affine_mesh = kernel
        .tessellate(affine.id, 1e-4)
        .expect("tessellate (AffineApply) must succeed");
    let (affine_min, affine_max) = mesh_aabb(&affine_mesh);

    let tol = 1e-4_f32;
    for axis in 0..3 {
        assert!(
            (rigid_min[axis] - affine_min[axis]).abs() < tol,
            "rigid-parity min[{axis}] mismatch: ApplyTransform={}, AffineApply={}",
            rigid_min[axis],
            affine_min[axis]
        );
        assert!(
            (rigid_max[axis] - affine_max[axis]).abs() < tol,
            "rigid-parity max[{axis}] mismatch: ApplyTransform={}, AffineApply={}",
            rigid_max[axis],
            affine_max[axis]
        );
    }
}
