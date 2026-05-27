//! Integration tests for the `shell-extract::extract` ComputeNode trampoline
//! and registration wiring (task γ, #3834).
//!
//! See `docs/prds/v0_4/shell-extract-engine-bridge.md` §4–§8 and
//! `docs/prds/v0_3/compute-node-contract.md` §4 for the full specification.

use reify_core::{DiagnosticCode, Severity};
use reify_eval::register_shell_extract_compute_fns;
use reify_ir::{
    InterpolationKind, PersistentMap, SampledField, SampledGridKind, StructureInstanceData,
    StructureTypeId, Value,
};
use reify_test_support::make_simple_engine;

/// Construct a synthetic thin-slab `SampledField` (5×5×3 grid) whose SDF
/// encodes a slab centred at z=0 with half-thickness 0.1.
///
/// - x: [0.0, 0.25, 0.5, 0.75, 1.0] (5 points, spacing=0.25)
/// - y: [0.0, 0.25, 0.5, 0.75, 1.0] (5 points, spacing=0.25)
/// - z: [-0.5, 0.0, 0.5] (3 points, spacing=0.5)
///
/// SDF(x,y,z) = |z| − 0.1  — negative inside the slab, positive outside.
fn synthetic_slab_field() -> SampledField {
    let x_grid: Vec<f64> = (0..5).map(|i| i as f64 * 0.25).collect();
    let y_grid: Vec<f64> = (0..5).map(|i| i as f64 * 0.25).collect();
    let z_grid: Vec<f64> = vec![-0.5, 0.0, 0.5];

    // Flat row-major order: iterate z outermost, then y, then x.
    let mut data = Vec::with_capacity(5 * 5 * 3);
    for &z in &z_grid {
        for _y in &y_grid {
            for _x in &x_grid {
                data.push(z.abs() - 0.1);
            }
        }
    }

    SampledField {
        name: "synthetic_slab".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, -0.5],
        bounds_max: vec![1.0, 1.0, 0.5],
        spacing: vec![0.25, 0.25, 0.5],
        axis_grids: vec![x_grid, y_grid, z_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-1 test (registration)
// ─────────────────────────────────────────────────────────────────────────────

/// Verify that `register_shell_extract_compute_fns` installs the
/// `"shell-extract::extract"` target in the engine's compute dispatch table.
///
/// PRD §4 contract: after registration `engine.compute_dispatch(target).is_some()`.
#[test]
fn register_shell_extract_compute_fns_registers_extract_target() {
    let mut engine = make_simple_engine();
    register_shell_extract_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("shell-extract::extract").is_some(),
        "expected \"shell-extract::extract\" to be registered after \
         register_shell_extract_compute_fns; got None"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-3 test (success path)
// ─────────────────────────────────────────────────────────────────────────────

/// Dispatch the trampoline on a synthetic slab SDF with default options and
/// verify that it returns a `Value::StructureInstance("ShellExtractionResult")`
/// with the five expected top-level keys.
///
/// PRD §8.1 row 3 ("Synthetic-geometry extraction"):
/// - Dispatching with `Value::Undef` options exercises the default-options path.
/// - The result must carry `type_name == "ShellExtractionResult"`.
/// - The field map must contain `mid_surface`, `segmentation`, `naming`,
///   `solve_time_ms`, and `diagnostics`.
/// - All returned per-invocation diagnostics must be `Severity::Info` or
///   `Severity::Warning` (no errors on the success path).
///
/// RED in step-3: skeleton returns `Failed`. GREEN after step-4 wires the
/// full producer pipeline.
#[test]
fn shell_extract_dispatch_on_synthetic_slab_materializes_shell_extraction_result_value() {
    let mut engine = make_simple_engine();
    register_shell_extract_compute_fns(&mut engine);

    let field = synthetic_slab_field();
    let options = Value::Undef;
    let sdf_value = Value::SampledField(field);

    let outcome = engine.dispatch_compute_node(
        "shell-extract::extract",
        &[options, sdf_value],
        &[],
        &Value::Undef,
        None,
    );

    let (result, diagnostics) = outcome.expect(
        "dispatch_compute_node returned Err; expected Ok((result, diags)) on synthetic slab",
    );

    // (1) Result must be a StructureInstance with type_name == "ShellExtractionResult"
    let data = match &result {
        Value::StructureInstance(d) => d,
        other => panic!("expected Value::StructureInstance, got {other:?}"),
    };
    assert_eq!(
        data.type_name, "ShellExtractionResult",
        "expected type_name == \"ShellExtractionResult\", got {:?}",
        data.type_name
    );

    // (2) Five top-level keys must be present
    for key in &["mid_surface", "segmentation", "naming", "solve_time_ms", "diagnostics"] {
        assert!(
            data.fields.contains_key(&key.to_string()),
            "ShellExtractionResult field map missing key {:?}",
            key
        );
    }

    // (3) No error-severity diagnostics on the success path
    for diag in &diagnostics {
        assert_ne!(
            diag.severity,
            reify_core::Severity::Error,
            "unexpected Severity::Error diagnostic on success path: {:?}",
            diag
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-5 test (invalid threshold → E_SHELL_BAD_THRESHOLD)
// ─────────────────────────────────────────────────────────────────────────────

/// Dispatch the trampoline with `shell_threshold = 0.0` (invalid: must be > 0)
/// and verify the failure is mapped to `DiagnosticCode::ShellBadThreshold`
/// per PRD §7 row 3.
///
/// Asserts:
/// 1. `dispatch_compute_node` returns `Err(diagnostics)`.
/// 2. At least one diagnostic has `severity == Severity::Error` AND
///    `code == Some(DiagnosticCode::ShellBadThreshold)`.
/// 3. The diagnostic message contains `"0"` (the offending value).
///
/// RED in step-5: `DiagnosticCode::ShellBadThreshold` does not exist yet and
/// the failure mapping does not call `.with_code(...)`. GREEN after step-6
/// adds the variant and wires the typed code.
#[test]
fn shell_extract_invalid_threshold_returns_failed_with_e_shell_bad_threshold_code() {
    let mut engine = make_simple_engine();
    register_shell_extract_compute_fns(&mut engine);

    // Build ElasticOptions with an invalid shell_threshold = 0.0 (must be > 0).
    let bad_options = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "ElasticOptions".to_string(),
        version: 1,
        fields: PersistentMap::from_iter([(
            "shell_threshold".to_string(),
            Value::Real(0.0),
        )]),
    }));

    let field = synthetic_slab_field();
    let sdf_value = Value::SampledField(field);

    let result = engine.dispatch_compute_node(
        "shell-extract::extract",
        &[bad_options, sdf_value],
        &[],
        &Value::Undef,
        None,
    );

    // (1) Must return Err on invalid threshold
    let diagnostics = result.expect_err(
        "dispatch_compute_node returned Ok; expected Err for shell_threshold=0.0",
    );

    // (2) At least one diagnostic with Severity::Error and ShellBadThreshold code
    let typed = diagnostics.iter().find(|d| {
        d.severity == Severity::Error
            && d.code == Some(DiagnosticCode::ShellBadThreshold)
    });
    assert!(
        typed.is_some(),
        "expected at least one Severity::Error diagnostic with \
         code=DiagnosticCode::ShellBadThreshold; got: {diagnostics:?}"
    );

    // (3) Message must contain the offending value "0"
    let msg = &typed.unwrap().message;
    assert!(
        msg.contains('0'),
        "expected diagnostic message to contain \"0\" (the bad threshold value); \
         got: {msg:?}"
    );
}
