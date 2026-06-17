//! End-to-end tests for apply_transform(geometry, Transform<3>) stdlib function (task 4164).
//!
//! Three test cases:
//! 1. MOCK happy path: ops == [Box, ApplyTransform], no Error diagnostics, correct IR values.
//! 2. MOCK malformed: Real(5.0) as transform arg → 1 kernel op (Box only) + diagnostic.
//! 3. OCCT acceptance: full pipeline STEP export non-empty + Volume/Centroid queries.

use reify_core::Severity;
use reify_ir::{ExportFormat, GeometryOp, GeometryQuery, Value};
use reify_test_support::{MockConstraintChecker, MockGeometryKernel, compile_source_with_stdlib};

/// Parse the x-component from a JSON centroid value produced by `GeometryQuery::Centroid`.
///
/// # Format contract
///
/// The centroid JSON producer (`OcctKernel::query` → `GeometryQuery::Centroid`) emits
/// **compact** JSON with no whitespace between the key and value: `{"x":<f>,"y":<f>,"z":<f>}`
/// where `<f>` is a standard Rust `f64` `Display` (decimal or scientific notation, no leading
/// space).  The field order is always `x`, `y`, `z`.
///
/// This substring-search convention is identical to `parse_centroid_z` in
/// `centered_primitives_e2e.rs` and `parse_centroid` in `stress_query_consistency.rs`,
/// which are the established codebase patterns for this query result.  If the JSON format
/// ever changes (e.g. spaces added after `:`, key order shuffled), ALL three helpers will
/// need updating simultaneously — the format is owned by the OCCT kernel query handler.
///
/// `serde_json` is not yet a dev-dependency of `reify-eval`; should the format become
/// less stable, adding it and deserializing into `{x:f64, y:f64, z:f64}` is the
/// preferred upgrade path.
fn parse_centroid_x(val: &Value) -> f64 {
    match val {
        Value::String(s) => {
            let prefix = "\"x\":";
            let start = s.find(prefix).expect("no \"x\" field in centroid JSON") + prefix.len();
            let end = s[start..].find([',', '}']).expect("centroid x end not found") + start;
            s[start..end].trim().parse::<f64>().expect("centroid x parse failed")
        }
        other => panic!("expected Value::String (centroid JSON), got {:?}", other),
    }
}

/// `apply_transform(box(10mm,10mm,10mm), transform3(orient_axis_angle(vec3(0,0,1),90deg), vec3(5mm,0,0)))`
/// must emit exactly two kernel ops: Box then ApplyTransform, with the correct rotation/translation.
///
/// RED: "apply_transform" not yet in GEOMETRY_FUNCTION_NAMES → call unrecognized → Undef →
/// no ApplyTransform op (ops != 2) → assertion fails.
#[test]
fn apply_transform_happy_path_mock() {
    let source = r#"structure S {
    let g = apply_transform(
        box(10mm, 10mm, 10mm),
        transform3(orient_axis_angle(vec3(0.0, 0.0, 1.0), 90deg), vec3(5mm, 0mm, 0mm))
    )
}"#;
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(compile_errors.is_empty(), "unexpected compile errors: {:?}", compile_errors);

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
        "expected 2 kernel ops (Box + ApplyTransform), got {} — apply_transform may be unrecognized",
        ops.len()
    );

    assert!(
        matches!(ops[0].op, GeometryOp::Box { .. }),
        "expected GeometryOp::Box at ops[0], got {:?}",
        ops[0].op
    );
    let box_handle = ops[0].result_handle;

    let w = std::f64::consts::FRAC_1_SQRT_2;
    match &ops[1].op {
        GeometryOp::ApplyTransform { target, rotation, translation } => {
            assert_eq!(*target, box_handle, "ApplyTransform target must be the Box handle");
            assert!((rotation[0] - w).abs() < 1e-9, "rotation[0] (w) ≈ {:.10}, got {}", w, rotation[0]);
            assert!(rotation[1].abs() < 1e-9, "rotation[1] (x) ≈ 0, got {}", rotation[1]);
            assert!(rotation[2].abs() < 1e-9, "rotation[2] (y) ≈ 0, got {}", rotation[2]);
            assert!((rotation[3] - w).abs() < 1e-9, "rotation[3] (z) ≈ {:.10}, got {}", w, rotation[3]);
            assert!((translation[0] - 0.005).abs() < 1e-9, "translation[0] ≈ 0.005 m, got {}", translation[0]);
            assert!(translation[1].abs() < 1e-9, "translation[1] ≈ 0, got {}", translation[1]);
            assert!(translation[2].abs() < 1e-9, "translation[2] ≈ 0, got {}", translation[2]);
        }
        other => panic!("expected GeometryOp::ApplyTransform at ops[1], got {:?}", other),
    }
}

/// Passing a non-Transform value (Real 5.0) as the transform arg must:
///   - produce exactly 1 kernel op (Box only — ApplyTransform dropped)
///   - emit a diagnostic mentioning "transform"
///   - not panic
///
/// RED: "apply_transform" not yet recognized → whole call evaluates to Undef →
/// 0 or 1 ops but no "transform" diagnostic → assertion about ops fails.
#[test]
fn apply_transform_malformed_arg_mock() {
    let source = r#"structure S {
    let g = apply_transform(box(10mm, 10mm, 10mm), 5.0)
}"#;
    let compiled = compile_source_with_stdlib(source);

    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let ops = ops_ref.lock().unwrap();
    assert_eq!(
        ops.len(),
        1,
        "malformed transform: expected 1 kernel op (Box only), got {} — {:?}",
        ops.len(),
        ops.iter().map(|r| format!("{:?}", r.op)).collect::<Vec<_>>()
    );
    assert!(
        matches!(ops[0].op, GeometryOp::Box { .. }),
        "expected GeometryOp::Box at ops[0], got {:?}",
        ops[0].op
    );

    let transform_diag: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("transform"))
        .collect();
    assert!(
        !transform_diag.is_empty(),
        "expected a diagnostic mentioning 'transform'; got: {:?}",
        result.diagnostics
    );
}

/// Full-pipeline OCCT acceptance: compile → OCCT Engine::build() → STEP non-empty +
/// no Error diagnostics; then direct kernel queries for Volume ≈ 1e-6 m³ (0.1%)
/// and Centroid x ≈ +0.005 m.
///
/// # Coverage structure
///
/// This test intentionally has two independent parts:
///
/// **Part 1** (compile pipeline integration): verifies that the full compile → eval →
/// OCCT-kernel path produces a non-Undef STEP output with no errors.  It proves the
/// name is recognized, the eval arm fires, and the OCCT execute arm runs without error.
/// It does NOT directly verify the rotation/translation values because `Engine::build()`
/// serialises the geometry to STEP bytes and the kernel handle is not accessible for
/// post-build queries.
///
/// **Part 2** (kernel geometry correctness): exercises `GeometryOp::ApplyTransform`
/// directly on a fresh `OcctKernel` to assert the volume is preserved and the centroid
/// shifts by the expected translation.  This uses the SAME rotation/translation values
/// that the compile pipeline will produce.
///
/// **Transitive closure**: `apply_transform_happy_path_mock` (above) already verifies
/// that the compile → eval path produces EXACTLY those rotation/translation values in the
/// `GeometryOp::ApplyTransform` IR node (within 1e-9).  Part 1 + Part 2 together with
/// the mock test therefore give full end-to-end coverage:
///   mock: compile→eval values correct  +  Part 2: OCCT kernel applies them correctly
///   Part 1: the pipeline actually reaches the OCCT kernel without error
///
/// A fully composed end-to-end query would require `Engine` to expose post-build kernel
/// queries (not yet in the public API); this would be the preferred future upgrade.
///
/// RED: "apply_transform" not yet in GEOMETRY_FUNCTION_NAMES → call unrecognized →
/// geometry Undef → no STEP output (or empty/Error diagnostics) → assertion fails.
#[test]
fn apply_transform_occt_acceptance() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping apply_transform_occt_acceptance: OCCT not available");
        return;
    }

    // ── Part 1: full compile-source pipeline ─────────────────────────────────
    // Verifies: name recognized → compile dispatch → eval arm → OCCT execute → STEP output.
    let source = r#"structure S {
    let g = apply_transform(
        box(10mm, 10mm, 10mm),
        transform3(orient_axis_angle(vec3(0.0, 0.0, 1.0), 90deg), vec3(5mm, 0mm, 0mm))
    )
}"#;
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(compile_errors.is_empty(), "unexpected compile errors: {:?}", compile_errors);

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
    assert!(build_errors.is_empty(), "unexpected build errors: {:?}", build_errors);

    let output = result
        .geometry_output
        .expect("apply_transform must produce geometry output (non-Undef)");
    assert!(!output.is_empty(), "STEP output must be non-empty");

    // ── Part 2: direct kernel — Volume + Centroid ─────────────────────────────
    // Verifies: the exact rotation [w,0,0,z] + translation [0.005,0,0] the compile→eval
    // path produces (confirmed by the mock test) correctly moves the centroid and preserves
    // volume when applied via OcctKernel directly.
    // box(10mm,10mm,10mm): 10mm = 0.01m → volume = 0.01^3 = 1e-6 m³, centroid at origin.
    // orient_axis_angle(z,90°): rotation-only, volume-invariant, centroid stays at origin.
    // vec3(5mm,0,0): translates centroid to (+0.005m, 0, 0).
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let w = std::f64::consts::FRAC_1_SQRT_2;

    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width:  Value::Scalar { si_value: 0.01, dimension: reify_core::DimensionVector::LENGTH },
            height: Value::Scalar { si_value: 0.01, dimension: reify_core::DimensionVector::LENGTH },
            depth:  Value::Scalar { si_value: 0.01, dimension: reify_core::DimensionVector::LENGTH },
        })
        .expect("Box execute must succeed");

    let transformed_handle = kernel
        .execute(&GeometryOp::ApplyTransform {
            target: box_handle.id,
            rotation: [w, 0.0, 0.0, w],
            translation: [0.005, 0.0, 0.0],
        })
        .expect("ApplyTransform execute must succeed");

    // Volume must be preserved (≈ 1e-6 m³ within 0.1%)
    let vol = kernel
        .query(&GeometryQuery::Volume(transformed_handle.id))
        .expect("Volume query must succeed");
    match vol {
        Value::Real(v) => {
            let expected = 1e-6_f64;
            let rel_err = (v - expected).abs() / expected;
            assert!(
                rel_err < 0.001,
                "Volume expected ≈ {:.2e} m³ (0.1% tolerance), got {:.6e} m³ (rel err = {:.4})",
                expected,
                v,
                rel_err
            );
        }
        other => panic!("expected Value::Real for Volume, got {:?}", other),
    }

    // Centroid x must be ≈ +0.005 m (box centroid at origin, shifted +5mm by translation)
    let centroid = kernel
        .query(&GeometryQuery::Centroid(transformed_handle.id))
        .expect("Centroid query must succeed");
    let cx = parse_centroid_x(&centroid);
    assert!(
        (cx - 0.005).abs() < 1e-6,
        "Centroid x expected ≈ +0.005 m, got {:.9} m",
        cx
    );
}

/// Strict multi-kernel-routing test: `Engine::with_registered_kernels` must route
/// `Operation::TransformApplyTransform` to OCCT and produce non-empty STEP output
/// with no Error diagnostics.
///
/// # Why this test exists
///
/// The three tests above all use `Engine::new(checker, Some(kernel))` — the backward-compat
/// single-kernel path.  In that path, when `dispatch()` returns `None` for an unregistered
/// op, the code falls back to the caller-supplied kernel silently.
///
/// `Engine::with_registered_kernels` is the production multi-kernel path.  In strict mode,
/// `dispatch()` returning `None` for `Operation::TransformApplyTransform` triggers the
/// `no_kernel_chain_diagnostic` Error and aborts the realization (engine_build.rs:4382).
///
/// Before step-8 registers `(TransformApplyTransform, BRep)` in `occt_capability_descriptor`,
/// this test fails: OCCT's descriptor lists only the original four transforms, so dispatch
/// returns `None` → Error diagnostic → geometry_output is None.
///
/// RED: `occt_capability_descriptor` does NOT yet declare `TransformApplyTransform` →
/// strict dispatch returns `None` → `no_kernel_chain_diagnostic` Error pushed → at least one
/// Error in result.diagnostics → assertion fails.
#[test]
fn apply_transform_occt_strict_dispatch() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping apply_transform_occt_strict_dispatch: OCCT not available");
        return;
    }

    let source = r#"structure S {
    let g = apply_transform(
        box(10mm, 10mm, 10mm),
        transform3(orient_axis_angle(vec3(0.0, 0.0, 1.0), 90deg), vec3(5mm, 0mm, 0mm))
    )
}"#;
    let compiled = compile_source_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(compile_errors.is_empty(), "unexpected compile errors: {:?}", compile_errors);

    let mut engine = reify_eval::Engine::with_registered_kernels(
        Box::new(reify_constraints::SimpleConstraintChecker),
    );
    let result = engine.build(&compiled, ExportFormat::Step);

    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected build errors (did you forget to register TransformApplyTransform in \
         occt_capability_descriptor?): {:?}",
        errors
    );

    let output = result.geometry_output.expect(
        "apply_transform must produce geometry output via Engine::with_registered_kernels \
         (strict-mode inventory dispatch)",
    );
    assert!(!output.is_empty(), "STEP output must be non-empty");
}
