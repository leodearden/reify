//! Integration tests for the `shell-extract::extract` ComputeNode trampoline
//! and registration wiring (task γ, #3834).
//!
//! See `docs/prds/v0_4/shell-extract-engine-bridge.md` §4–§8 and
//! `docs/prds/v0_3/compute-node-contract.md` §4 for the full specification.

use std::sync::atomic::{AtomicUsize, Ordering};

use reify_core::{DiagnosticCode, Severity};
use reify_eval::{
    CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle,
    register_shell_extract_compute_fns, shell_extract_compute_fn,
};
use reify_ir::{
    InterpolationKind, OpaqueState, PersistentMap, SampledField, SampledGridKind,
    StructureInstanceData, StructureTypeId, Value,
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

// ─────────────────────────────────────────────────────────────────────────────
// Step-7 test (cache short-circuit / second-run cache state)
// ─────────────────────────────────────────────────────────────────────────────

/// Number of times `counting_shell_extract_fn` was invoked.
///
/// **Single-test ownership invariant**: this static is touched exclusively by
/// `counting_shell_extract_fn` (target `"test::shell-extract-counting"`), which
/// in turn is registered only by
/// `shell_extract_second_run_hits_in_memory_compute_node_cache`. Adding a second
/// test in this binary that registers a trampoline calling
/// `counting_shell_extract_fn` — directly or via `"test::shell-extract-counting"`
/// — will silently corrupt the count. The test resets the static at entry as
/// belt-and-suspenders against `cargo test` reusing a process across runs.
static SHELL_INVOCATION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Wrapper trampoline: increments `SHELL_INVOCATION_COUNT` then proxies to
/// the production `shell_extract_compute_fn`.
///
/// Registered under target `"test::shell-extract-counting"` (not the
/// production `"shell-extract::extract"`) to keep the test isolated.
fn counting_shell_extract_fn(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    SHELL_INVOCATION_COUNT.fetch_add(1, Ordering::SeqCst);
    shell_extract_compute_fn(
        value_inputs,
        realization_inputs,
        options,
        prior_warm_state,
        cancellation,
    )
}

/// Verify that `run_compute_dispatch` records the result in the cache correctly
/// across two dispatches on the same inputs.
///
/// After the first dispatch:
/// - The trampoline ran exactly once.
/// - `engine.freshness(NodeId::Value(cell)) == Freshness::Final`.
/// - The cache entry carries `Value::StructureInstance("ShellExtractionResult")`.
/// - `result.content_hash()` is captured as `hash1`.
///
/// After the second dispatch (same inputs, VersionId(2)):
/// - The trampoline ran a total of twice (`run_compute_dispatch` has no
///   short-circuit at the helper level — that short-circuit lives in the
///   eval loop at `engine_eval.rs:2169-2194`, tested by
///   `compute_dispatch_registry.rs`).
/// - `freshness` is still `Final`.
/// - `result.content_hash() == hash1` — pinning that
///   `shell_extraction_result_to_value` is **deterministic** for the same
///   pipeline inputs.
///
/// RED in step-7 because `shell_extraction_result_to_value` currently projects
/// the actual measured `solve_time_ms` into `Value::Int`, which can differ
/// between the two runs and thus produces different content hashes. GREEN after
/// step-8 ensures the projection is byte-stable across re-dispatches on the
/// same inputs (solve_time_ms must not perturb the hash).
///
/// PRD §5 cache-key composition forward link:
/// `docs/prds/v0_4/shell-extract-engine-bridge.md §5`.
#[test]
fn shell_extract_second_run_hits_in_memory_compute_node_cache() {
    use reify_core::{ComputeNodeId, ValueCellId, VersionId};
    use reify_eval::cache::NodeId;
    use reify_ir::Freshness;

    // Belt-and-suspenders: reset on entry in case cargo-test reuses a process.
    SHELL_INVOCATION_COUNT.store(0, Ordering::SeqCst);

    let mut engine = make_simple_engine();
    engine.register_compute_fn(
        "test::shell-extract-counting",
        counting_shell_extract_fn as ComputeFn,
    );

    let field = synthetic_slab_field();
    let options = Value::Undef;
    let sdf_value = Value::SampledField(field);
    let value_inputs = vec![options, sdf_value];

    let c_id = ComputeNodeId::new("ShellExtractFixture", 0);
    let cell = ValueCellId::new("ShellExtractFixture", "result");
    let node = NodeId::Value(cell.clone());

    // ── First dispatch (VersionId 1) ─────────────────────────────────────────
    let (result1, diags1) = engine
        .run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::shell-extract-counting",
            &value_inputs,
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(1),
        )
        .expect("first run_compute_dispatch must return Ok on synthetic slab");

    assert!(
        diags1.is_empty(),
        "first dispatch: unexpected diagnostics: {diags1:?}"
    );
    assert_eq!(
        SHELL_INVOCATION_COUNT.load(Ordering::SeqCst),
        1,
        "trampoline must run exactly once after first dispatch"
    );
    assert_eq!(
        engine.freshness(&node),
        Freshness::Final,
        "post-first-dispatch freshness must be Final"
    );

    let data1 = match &result1 {
        Value::StructureInstance(d) => d,
        other => {
            panic!("expected Value::StructureInstance from first dispatch, got {other:?}")
        }
    };
    assert_eq!(
        data1.type_name, "ShellExtractionResult",
        "first dispatch result must carry type_name == \"ShellExtractionResult\""
    );

    let hash1 = result1.content_hash();

    // ── Second dispatch (VersionId 2) ─────────────────────────────────────────
    let (result2, diags2) = engine
        .run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::shell-extract-counting",
            &value_inputs,
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(2),
        )
        .expect("second run_compute_dispatch must return Ok on synthetic slab");

    assert!(
        diags2.is_empty(),
        "second dispatch: unexpected diagnostics: {diags2:?}"
    );
    assert_eq!(
        SHELL_INVOCATION_COUNT.load(Ordering::SeqCst),
        2,
        "trampoline must run exactly twice after second dispatch (no helper-level short-circuit)"
    );
    assert_eq!(
        engine.freshness(&node),
        Freshness::Final,
        "post-second-dispatch freshness must still be Final"
    );

    // The two dispatches run on the same inputs — the projection must be
    // deterministic so `hash(result2) == hash(result1)`.
    //
    // `shell_extraction_result_to_value` must NOT fold in timing-derived
    // values (e.g. actual `solve_time_ms`) that differ between runs.
    // Step-8 ensures the projected `solve_time_ms` field is always `Value::Int(0)`
    // so the content hash is byte-stable across re-dispatches.
    let hash2 = result2.content_hash();
    assert_eq!(
        hash2,
        hash1,
        "content hash must be identical across re-dispatches on the same inputs; \
         `shell_extraction_result_to_value` must project a byte-stable Value \
         (solve_time_ms must not perturb the content hash)"
    );

    // The cache entry after the second dispatch must hold a Final value whose
    // content hash matches hash1 — pinning that the cache correctly stores the
    // trampoline output and that the output is byte-stable across re-dispatches.
    //
    // Note: `NodeCache.result_hash` is the hash of the `CachedResult` envelope
    // (tag + Value hash + DeterminacyState), NOT just `Value::content_hash()`.
    // We extract the inner Value and compare its content_hash against hash1.
    let entry = engine
        .cache_store()
        .get(&node)
        .expect("cache entry must exist after second dispatch");
    match &entry.result {
        reify_eval::cache::CachedResult::Value(v, _det) => {
            assert_eq!(
                v.content_hash(),
                hash1,
                "cache entry value content_hash must match the deterministic hash1"
            );
        }
        other => panic!("expected CachedResult::Value in cache after dispatch, got {other:?}"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-9 test (double-registration panic)
// ─────────────────────────────────────────────────────────────────────────────

/// Verify that calling `register_shell_extract_compute_fns` twice on the same
/// engine panics with a message containing `"shell-extract::extract"`.
///
/// Pins the PRD §4 hard-error contract propagated from
/// `Engine::register_compute_fn`'s `Entry::Occupied → panic!` arm at
/// `engine_admin.rs:739-746`.  This is a defensive pin against a future
/// refactor that accidentally adds an early-return / `if !is_registered { … }`
/// guard that would silently overwrite (or silently skip) a second registration.
///
/// RED in step-9 only if `register_shell_extract_compute_fns` accidentally
/// guards double-registration.  With the current single-call implementation
/// (step-2) the test is GREEN immediately after being written, because the
/// underlying `Engine::register_compute_fn` already panics on duplicate targets.
#[test]
#[should_panic(expected = "shell-extract::extract")]
fn shell_extract_double_registration_panics_naming_target() {
    let mut engine = make_simple_engine();
    // First registration — succeeds.
    register_shell_extract_compute_fns(&mut engine);
    // Second registration — must panic with a message containing the target name.
    register_shell_extract_compute_fns(&mut engine);
}
