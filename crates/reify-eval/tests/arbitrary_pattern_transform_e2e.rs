//! End-to-end tests for `arbitrary_pattern(geometry, List<Transform<3>>)` — the full
//! per-instance rigid transform (rotation+translation) form (task 4168, geom-transform ε),
//! superseding the phantom-partial task 323 (translation-only).
//!
//! Two test cases (mock kernel, no OCCT required):
//! 1. LIST form: a single-instance rotation-only `Transform<3>` decodes to the Y-90
//!    scalar-first quaternion `[0.7071,0,0.7071,0]` with zero translation.
//! 2. TRIPLE form (back-compat guard): the legacy scalar-triple call still produces two
//!    translation-only instances, each carrying the identity quaternion `[1,0,0,0]`, with
//!    translations converted to SI metres.
//!
//! Mirrors the `apply_transform_e2e.rs` mock-kernel harness (task 4164).

use reify_core::Severity;
use reify_ir::{ExportFormat, GeometryOp};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib};

/// `arbitrary_pattern(box(2mm,2mm,10mm), [transform3(orient_axis_angle(vec3(0.0,1.0,0.0),90deg),
/// vec3(0mm,0mm,0mm))])` (2-arg LIST form) must decode to exactly ONE instance whose rotation
/// quaternion is the Y-90 scalar-first quat `[0.7071,0,0.7071,0]` and whose translation is
/// `[0,0,0]`.
///
/// RED (pre-step-8): the compiler (step-6) emits a `"transform_list"` arg, but eval's
/// `pattern_arbitrary` has no list branch yet, so it finds no `t{i}_dx` args and returns
/// "ArbitraryPattern has no transforms" (Err) → the op is dropped → ops.len() != 2.
#[test]
fn arbitrary_pattern_list_form_happy_path_mock() {
    let source = r#"structure S {
    let g = arbitrary_pattern(
        box(2mm, 2mm, 10mm),
        [transform3(orient_axis_angle(vec3(0.0, 1.0, 0.0), 90deg), vec3(0mm, 0mm, 0mm))]
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

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected build errors: {:?}", errors);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "expected 2 kernel ops (Box + ArbitraryPattern), got {} — {:?}",
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
    let box_handle = ops[0].result_handle;

    match &ops[1].op {
        GeometryOp::ArbitraryPattern { target, transforms } => {
            assert_eq!(
                *target, box_handle,
                "ArbitraryPattern target must be the Box handle"
            );
            assert_eq!(
                transforms.len(),
                1,
                "expected exactly 1 decoded instance, got {}",
                transforms.len()
            );
            let (rotation, translation) = transforms[0];
            let w = std::f64::consts::FRAC_1_SQRT_2;
            assert!(
                (rotation[0] - w).abs() < 1e-6,
                "rotation[0] (w) ≈ {:.10}, got {}",
                w,
                rotation[0]
            );
            assert!(
                rotation[1].abs() < 1e-9,
                "rotation[1] (x) ≈ 0, got {}",
                rotation[1]
            );
            assert!(
                (rotation[2] - w).abs() < 1e-6,
                "rotation[2] (y) ≈ {:.10}, got {}",
                w,
                rotation[2]
            );
            assert!(
                rotation[3].abs() < 1e-9,
                "rotation[3] (z) ≈ 0, got {}",
                rotation[3]
            );
            assert!(
                translation[0].abs() < 1e-9,
                "translation[0] ≈ 0, got {}",
                translation[0]
            );
            assert!(
                translation[1].abs() < 1e-9,
                "translation[1] ≈ 0, got {}",
                translation[1]
            );
            assert!(
                translation[2].abs() < 1e-9,
                "translation[2] ≈ 0, got {}",
                translation[2]
            );
        }
        other => panic!(
            "expected GeometryOp::ArbitraryPattern at ops[1], got {:?}",
            other
        ),
    }
}

/// Back-compat guard: `arbitrary_pattern(box(10mm,10mm,10mm), 10mm,0mm,0mm, 0mm,20mm,0mm)`
/// (legacy scalar-triple form) must still produce 2 translation-only instances, each carrying
/// the IDENTITY quaternion `[1,0,0,0]`, with translations converted to SI metres.
#[test]
fn arbitrary_pattern_triple_form_back_compat_mock() {
    let source = r#"structure S {
    let g = arbitrary_pattern(
        box(10mm, 10mm, 10mm),
        10mm, 0mm, 0mm,
        0mm, 20mm, 0mm
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

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected build errors: {:?}", errors);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        2,
        "expected 2 kernel ops (Box + ArbitraryPattern), got {} — {:?}",
        ops.len(),
        ops.iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    match &ops[1].op {
        GeometryOp::ArbitraryPattern { transforms, .. } => {
            assert_eq!(
                transforms.len(),
                2,
                "expected 2 translation-only instances, got {}",
                transforms.len()
            );
            for (rotation, _) in transforms.iter() {
                assert_eq!(
                    *rotation,
                    [1.0, 0.0, 0.0, 0.0],
                    "triple-form instances must carry the identity quaternion, got {:?}",
                    rotation
                );
            }
            let (_, t0) = transforms[0];
            assert!(
                (t0[0] - 0.010).abs() < 1e-9,
                "t0 x ≈ 0.010 m, got {}",
                t0[0]
            );
            assert!(t0[1].abs() < 1e-9, "t0 y ≈ 0, got {}", t0[1]);
            assert!(t0[2].abs() < 1e-9, "t0 z ≈ 0, got {}", t0[2]);

            let (_, t1) = transforms[1];
            assert!(t1[0].abs() < 1e-9, "t1 x ≈ 0, got {}", t1[0]);
            assert!(
                (t1[1] - 0.020).abs() < 1e-9,
                "t1 y ≈ 0.020 m, got {}",
                t1[1]
            );
            assert!(t1[2].abs() < 1e-9, "t1 z ≈ 0, got {}", t1[2]);
        }
        other => panic!(
            "expected GeometryOp::ArbitraryPattern at ops[1], got {:?}",
            other
        ),
    }
}
