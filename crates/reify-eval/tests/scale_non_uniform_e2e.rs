//! End-to-end tests for the per-axis (non-rigid) `scale(geometry, factors:
//! Vector3<Real>)` stdlib overload (task 4167, PRD
//! docs/prds/geometry-transforms-frames-projection.md task δ).
//!
//! Two signals, mirroring affine_apply_e2e.rs's structure:
//! 1. OCCT acceptance: `scale(box(10mm,10mm,10mm), vec3(2.0,1.0,0.5))`
//!    produces a solid whose tessellated AABB is ≈20x10x5mm and whose volume
//!    is numerically unchanged (2·1·0.5=1) — the documented G2 user-observable
//!    signal.
//! 2. MOCK: a zero factor component drops the op gracefully — one kernel op
//!    (Box only, ScaleNonUniform dropped), a diagnostic naming the "scale
//!    dropped" condition, no panic and no degenerate (zero-volume) output.

use reify_core::Severity;
use reify_ir::{ExportFormat, GeometryOp, GeometryQuery, Value};
use reify_test_support::{
    MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib, mesh_aabb,
};

/// `scale(box(10mm,10mm,10mm), vec3(2.0,1.0,0.5))`: the resulting solid's
/// tessellated AABB must be ≈20x10x5mm and its volume must be ≈1000mm³
/// (2·1·0.5=1, so volume is numerically unchanged from the source box).
///
/// RED until the compiler dispatches a Vector3 second arg to
/// `GeometryOp::ScaleNonUniform` (step-8): before that, `scale(g, vec3(..))`
/// still lowers to the uniform `Scale{factor}` op, which fails to decode a
/// Vector as a Real factor ("missing or non-finite argument 'factor'"), so no
/// geometry output is produced and the assertions below never run against
/// real geometry.
#[test]
fn scale_non_uniform_occt_acceptance() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping scale_non_uniform_occt_acceptance: OCCT not available");
        return;
    }

    // Part 1: full compile-source pipeline reaches the OCCT kernel without error.
    let source = r#"structure S {
    let g = scale(box(10mm, 10mm, 10mm), vec3(2.0, 1.0, 0.5))
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
        .expect("scale(g, vec3(..)) must produce geometry output (non-Undef)");
    assert!(!output.is_empty(), "STEP output must be non-empty");

    // Part 2: direct kernel — tessellated AABB + volume (the exact factors the
    // compile→eval path produces, confirmed by Part 1 reaching real geometry).
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("Box execute must succeed");
    let scaled = kernel
        .execute(&GeometryOp::ScaleNonUniform {
            target: box_h.id,
            sx: 2.0,
            sy: 1.0,
            sz: 0.5,
        })
        .expect("ScaleNonUniform execute must succeed");
    let mesh = kernel
        .tessellate(scaled.id, 1e-4)
        .expect("tessellate must succeed");
    let (min, max) = mesh_aabb(&mesh);
    let tol = 1e-4_f32;
    assert!(
        (max[0] - min[0] - 0.02).abs() < tol,
        "X extent expected ≈0.02, got {}",
        max[0] - min[0]
    );
    assert!(
        (max[1] - min[1] - 0.01).abs() < tol,
        "Y extent expected ≈0.01, got {}",
        max[1] - min[1]
    );
    assert!(
        (max[2] - min[2] - 0.005).abs() < tol,
        "Z extent expected ≈0.005, got {}",
        max[2] - min[2]
    );

    let vol_box = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        })
        .expect("Box execute must succeed");
    let vol_scaled = kernel
        .execute(&GeometryOp::ScaleNonUniform {
            target: vol_box.id,
            sx: 2.0,
            sy: 1.0,
            sz: 0.5,
        })
        .expect("ScaleNonUniform execute must succeed");
    let vol = kernel
        .query(&GeometryQuery::Volume(vol_scaled.id))
        .expect("volume query must succeed");
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 1000.0).abs() < 1.0,
                "scale(2,1,0.5) should give volume ≈ 1000 (2*1*0.5=1), got {v}"
            );
        }
        other => panic!("expected Value::Real for volume, got {:?}", other),
    }
}

/// `scale(box(10mm,10mm,10mm), vec3(0.0,1.0,1.0))`: a zero factor component
/// must drop the op gracefully — one kernel op (Box only, ScaleNonUniform
/// dropped), a diagnostic naming the "scale dropped" condition, no panic and
/// no degenerate (zero-volume) output.
///
/// RED until step-8: today `scale(g, vec3(..))` lowers to the uniform
/// `Scale{factor}` op, which fails to decode the Vector as a Real factor
/// before ever reaching the zero-component check, so no "scale dropped"
/// diagnostic is emitted (the compile error is a different message).
#[test]
fn scale_non_uniform_zero_component_drops_gracefully() {
    let source = r#"structure S {
    let g = scale(box(10mm, 10mm, 10mm), vec3(0.0, 1.0, 1.0))
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
        "zero-factor scale: expected 1 kernel op (Box only), got {} — {:?}",
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

    let dropped_diag: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("scale dropped"))
        .collect();
    assert!(
        !dropped_diag.is_empty(),
        "expected a diagnostic mentioning 'scale dropped'; got: {:?}",
        result.diagnostics
    );
}
