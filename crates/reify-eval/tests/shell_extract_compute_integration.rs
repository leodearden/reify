//! Integration tests for the `shell-extract::extract` ComputeNode trampoline
//! and registration wiring (task γ, #3834).
//!
//! See `docs/prds/v0_4/shell-extract-engine-bridge.md` §4–§8 and
//! `docs/prds/v0_3/compute-node-contract.md` §4 for the full specification.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use reify_core::{ContentHash, DiagnosticCode, RealizationNodeId, Severity};
use reify_eval::{
    CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle, RealizedContent,
    register_shell_extract_compute_fns, shell_extract_compute_fn,
};
use reify_ir::{
    InterpolationKind, OpaqueState, PersistentMap, SampledField, SampledGridKind,
    StructureInstanceData, StructureTypeId, Value,
};
use reify_test_support::make_simple_engine;

/// Shared builder for synthetic thin-slab `SampledField` fixtures (5×5×3 grid).
///
/// Grid layout: 5 points along x and y with `spacing_xy`; z fixed at
/// [-0.5, 0.0, 0.5] (spacing=0.5). SDF(x,y,z) = |z| − 0.1 — negative inside
/// the slab, positive outside. Medial plane at z=0.
fn slab_field(name: &str, spacing_xy: f64) -> SampledField {
    const N: usize = 5;
    let x_grid: Vec<f64> = (0..N).map(|i| i as f64 * spacing_xy).collect();
    let y_grid: Vec<f64> = (0..N).map(|i| i as f64 * spacing_xy).collect();
    let z_grid: Vec<f64> = vec![-0.5, 0.0, 0.5];

    let mut data = Vec::with_capacity(N * N * 3);
    for &z in &z_grid {
        for _y in &y_grid {
            for _x in &x_grid {
                data.push(z.abs() - 0.1);
            }
        }
    }

    let max_xy = spacing_xy * (N - 1) as f64;
    SampledField {
        name: name.to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, -0.5],
        bounds_max: vec![max_xy, max_xy, 0.5],
        spacing: vec![spacing_xy, spacing_xy, 0.5],
        axis_grids: vec![x_grid, y_grid, z_grid],
        interpolation: InterpolationKind::Linear,
        data,
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// 5×5×3 slab with spacing=0.25, footprint [0,1.0]².
fn synthetic_slab_field() -> SampledField {
    slab_field("synthetic_slab", 0.25)
}

/// 5×5×3 slab with spacing=0.5, footprint [0,2.0]² — used to prove the
/// realization arm (not the value_inputs[1] slab) was the geometry source.
fn synthetic_large_slab_field() -> SampledField {
    slab_field("synthetic_large_slab", 0.5)
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
    for key in &[
        "mid_surface",
        "segmentation",
        "naming",
        "solve_time_ms",
        "diagnostics",
    ] {
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
/// 3. The diagnostic message contains `"= 0"` (value-after-equals per PRD §7,
///    confirming the offending value is surfaced in the expected format).
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
        fields: PersistentMap::from_iter([("shell_threshold".to_string(), Value::Real(0.0))]),
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
    let diagnostics = result
        .expect_err("dispatch_compute_node returned Ok; expected Err for shell_threshold=0.0");

    // (2) At least one diagnostic with Severity::Error and ShellBadThreshold code
    let typed = diagnostics.iter().find(|d| {
        d.severity == Severity::Error && d.code == Some(DiagnosticCode::ShellBadThreshold)
    });
    assert!(
        typed.is_some(),
        "expected at least one Severity::Error diagnostic with \
         code=DiagnosticCode::ShellBadThreshold; got: {diagnostics:?}"
    );

    // (3) Message must contain the value-after-equals shape ("= 0") per PRD §7.
    // Canonical message: "shell_threshold = 0 must be in (0.0, 1.0)."
    // Checking for "= 0" pins that the *offending value* appears after the
    // equals sign, not just that some digit '0' appears anywhere in the message.
    let msg = &typed.unwrap().message;
    assert!(
        msg.contains("= 0"),
        "expected diagnostic message to contain \"= 0\" (value-after-equals per PRD §7); \
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
/// `shell_extract_cache_entry_is_byte_stable_across_redispatches`. Adding a second
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

/// Verify that `shell_extraction_result_to_value` produces a byte-stable
/// `Value` across two re-dispatches on the same pipeline inputs, and that the
/// cache entry written by `run_compute_dispatch` carries that stable value.
///
/// This test does NOT pin the eval-loop short-circuit (the mechanism that avoids
/// calling the trampoline on a cache hit). That contract lives in the eval loop
/// at `engine_eval.rs:2169-2194` and is tested by `compute_dispatch_registry.rs`.
/// What this test pins is the prerequisite for that short-circuit to be correct:
/// the trampoline output must be **deterministic** (same inputs → byte-identical
/// `Value`) so the content hash stored in the cache can be used as a stable key.
///
/// After the first dispatch (VersionId 1):
/// - The trampoline ran exactly once.
/// - `engine.freshness(NodeId::Value(cell)) == Freshness::Final`.
/// - The cache entry carries `Value::StructureInstance("ShellExtractionResult")`.
/// - `result.content_hash()` is captured as `hash1`.
///
/// After the second dispatch (same inputs, VersionId 2):
/// - The trampoline ran a total of twice (`run_compute_dispatch` has no
///   short-circuit at the helper level).
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
fn shell_extract_cache_entry_is_byte_stable_across_redispatches() {
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
            ContentHash(0), // inert: no cache dir in tests
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
            ContentHash(0), // inert: no cache dir in tests
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
        hash2, hash1,
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

// ─────────────────────────────────────────────────────────────────────────────
// Step-9 test (empty-axis-grid → E_SHELL_NO_VOXEL_GRID)
// ─────────────────────────────────────────────────────────────────────────────
//
// Coverage note (esc-3837 reviewer suggestion 2)
// ──────────────────────────────────────────────
// Five of the seven §7 error-code arms are tested by integration tests in this
// file or the e2e fixture tests:
//
//   ShellBadThreshold   — step-5 test above (invalid threshold = 0.0)
//   ShellNoVoxelGrid    — step-9 test below (empty axis grid → Phase 1 fails)
//   ShellNoMedial       — shell_extract_empty_medial_mask_returns_failed_with_shell_no_medial_code
//                         below (uniform far-outside SDF → empty medial mask)
//   ShellTooThick       — shell_too_thick_at_shell_annotation_errors.rs (e2e)
//                         and shell_too_thick_at_auto_falls_back.rs (e2e)
//
// Three arms are intentionally NOT driven by integration tests because no clean
// synthetic SDF input reliably reaches those producer states in the current
// pipeline (as documented in the task ε design decisions, §"All six §7 codes"):
//
//   ShellMedialMaskOob  — requires `medial_mask` to contain indices outside the
//     (MidSurface::       SDF grid bounds, which indicates an internal
//      MaskVoxelOut       mask/grid size mismatch — not triggerable by a
//      OfBounds)          caller-supplied SDF alone; the medial-mask and grid
//                         are both derived from the same SDF, so they share
//                         extent by construction.
//
//   ShellPruneFailed    — requires the branch-pruning step to fail on the
//     (any PruneError)    raw mid-surface mesh.  Pruning validates configuration
//                         parameters (ratio, max-iterations, alignment tolerance)
//                         AFTER phases 1 and 2 succeed, but passing invalid
//                         prune options via ElasticOptions currently propagates
//                         no PruneError (the default options are always valid
//                         and the parser clamps/ignores out-of-range values).
//
//   ShellMeshQuality    — requires the mesher to produce a mesh whose worst
//     (Mesher::Quality    element quality is below threshold.  This is a property
//      BelowThreshold)    of the SDF geometry, not of the caller-supplied options;
//                         a synthetically well-behaved slab always produces a
//                         quality mesh within the default threshold.
//
// These three arms are wired by inspection and rely on the `with_code(...)` call
// being visually verifiable in the source.  A future task that adds shell-extract
// fuzz/property-based testing may be able to drive them.  The serde/constructible
// tests in diagnostics.rs confirm the variants exist and serialize correctly.

/// Build a `SampledField` whose first axis grid is empty, which triggers
/// `GridValidationError::EmptyAxisGrid { axis: 0 }` in Phase 1 (medial-mask).
///
/// All other fields are structurally valid for `Regular3D` (3-element
/// `bounds_min`/`bounds_max`/`spacing`/`axis_grids`), but the empty x-grid
/// fails the "no axis grid is empty" invariant in `validate_regular3d` before
/// any computation begins.
fn empty_axis_grid_field() -> SampledField {
    SampledField {
        name: "empty_axis_grid".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![1.0, 1.0, 1.0],
        spacing: vec![1.0, 1.0, 1.0],
        // axis_grids[0] is intentionally empty → EmptyAxisGrid { axis: 0 }
        axis_grids: vec![vec![], vec![0.0, 1.0], vec![0.0, 1.0]],
        interpolation: InterpolationKind::Linear,
        data: vec![],
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// Dispatch `shell_extract_compute_fn` with an empty-axis-grid `SampledField`
/// and verify that the failure is mapped to `DiagnosticCode::ShellNoVoxelGrid`
/// per PRD §7 row 1 (E_SHELL_NO_VOXEL_GRID).
///
/// Asserts:
/// 1. The outcome is `ComputeOutcome::Failed { diagnostics }`.
/// 2. At least one diagnostic has `code == Some(DiagnosticCode::ShellNoVoxelGrid)`.
///
/// RED in step-9: the Phase 1 (medial-mask) error arm currently emits an
/// un-coded `Diagnostic::error(format!(...))`, so no diagnostic carries
/// `ShellNoVoxelGrid` → assertion (2) fails. GREEN after step-10 wires the
/// `MedialError::GridValidation(GridValidationError::EmptyAxisGrid { .. })`
/// arm with `.with_code(DiagnosticCode::ShellNoVoxelGrid)`.
#[test]
fn shell_extract_empty_axis_grid_returns_failed_with_shell_no_voxel_grid_code() {
    let field = empty_axis_grid_field();
    let options_value = Value::Undef;
    let sdf_value = Value::SampledField(field);

    let outcome = shell_extract_compute_fn(
        &[options_value, sdf_value],
        &[],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    // (1) Must return Failed on empty axis grid
    let diagnostics = match outcome {
        ComputeOutcome::Failed { diagnostics } => diagnostics,
        other => {
            panic!("expected ComputeOutcome::Failed for empty-axis-grid input, got: {other:?}")
        }
    };

    // (2) At least one diagnostic must carry ShellNoVoxelGrid code
    let coded = diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ShellNoVoxelGrid));
    assert!(
        coded.is_some(),
        "expected at least one diagnostic with code=DiagnosticCode::ShellNoVoxelGrid; \
         got: {diagnostics:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-1 test for task #4514 (empty medial mask → E_SHELL_NO_MEDIAL)
// ─────────────────────────────────────────────────────────────────────────────

/// Build a `SampledField` whose SDF is UNIFORM and far outside the narrow band
/// (constant φ = -100.0, 3×3×3, unit spacing), which guarantees an empty
/// medial mask via TWO independent rejection mechanisms:
///
/// 1. Narrow-band filter: |φ| = 100 ≫ band_width ≈ 3 (narrow_band_half_width_voxels=3.0
///    × min_spacing=1.0) — every voxel is outside the narrow band.
/// 2. Degenerate-gradient filter: a uniform field has zero central-difference
///    gradient everywhere — all voxels fail the non-zero-gradient check.
///
/// The field passes `validate_regular3d` and `compute_medial_mask`'s geometry
/// checks, then returns `Ok(MedialMask { voxels: vec![] })` — the empty-mask
/// Ok path that `ShellNoMedial` guards.
fn solid_no_medial_field() -> SampledField {
    let axis: Vec<f64> = vec![0.0, 1.0, 2.0];
    SampledField {
        name: "solid_no_medial".to_string(),
        kind: SampledGridKind::Regular3D,
        bounds_min: vec![0.0, 0.0, 0.0],
        bounds_max: vec![2.0, 2.0, 2.0],
        spacing: vec![1.0, 1.0, 1.0],
        axis_grids: vec![axis.clone(), axis.clone(), axis],
        interpolation: InterpolationKind::Linear,
        // Uniform φ = -100.0 across all 3×3×3 = 27 voxels.
        data: vec![-100.0; 27],
        oob_emitted: std::sync::atomic::AtomicBool::new(false),
    }
}

/// Dispatch `shell_extract_compute_fn` with a uniform far-outside SDF that
/// yields an empty medial mask, and verify the failure is mapped to
/// `DiagnosticCode::ShellNoMedial` per PRD §7 (E_SHELL_NO_MEDIAL).
///
/// Asserts:
/// 1. The outcome is `ComputeOutcome::Failed { diagnostics }`.
/// 2. At least one diagnostic has `code == Some(DiagnosticCode::ShellNoMedial)`.
///
/// RED before task #4514 step-2: the empty medial mask falls through to later
/// phases without the Phase-1 guard, so no diagnostic carries `ShellNoMedial`
/// (or a non-Failed outcome trips the `other => panic!` arm).
/// GREEN after step-2 adds the `medial_mask.voxels.is_empty()` guard that
/// short-circuits with `ComputeOutcome::Failed` + `ShellNoMedial`.
#[test]
fn shell_extract_empty_medial_mask_returns_failed_with_shell_no_medial_code() {
    let field = solid_no_medial_field();
    let options_value = Value::Undef;
    let sdf_value = Value::SampledField(field);

    let outcome = shell_extract_compute_fn(
        &[options_value, sdf_value],
        &[],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    // (1) Must return Failed on empty medial mask
    let diagnostics = match outcome {
        ComputeOutcome::Failed { diagnostics } => diagnostics,
        other => {
            panic!(
                "expected ComputeOutcome::Failed for uniform-SDF (empty medial mask) input, \
                 got: {other:?}"
            )
        }
    };

    // (2) At least one diagnostic must carry ShellNoMedial code
    let coded = diagnostics
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ShellNoMedial));
    assert!(
        coded.is_some(),
        "expected at least one diagnostic with code=DiagnosticCode::ShellNoMedial; \
         got: {diagnostics:?}"
    );

    // (3) Severity must be Error (not Warning or Info)
    let d = coded.unwrap();
    assert_eq!(
        d.severity,
        Severity::Error,
        "expected Severity::Error for ShellNoMedial diagnostic; got: {:?}",
        d.severity
    );

    // (4) Message must contain the canonical phrase and the body name
    let msg = &d.message;
    assert!(
        msg.contains("no medial axis found"),
        "expected message to contain 'no medial axis found'; got: {msg:?}"
    );
    assert!(
        msg.contains("solid_no_medial"),
        "expected message to contain body name 'solid_no_medial'; got: {msg:?}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-1 tests for task #4511 (ε dual-source: prefers realization_inputs[0].sdf())
// ─────────────────────────────────────────────────────────────────────────────

/// Extract the maximum x-coordinate across all mid-surface vertices from a
/// `ComputeOutcome::Completed` result value.
///
/// Navigates: `Value::StructureInstance("ShellExtractionResult")`
///   → `fields["mid_surface"]` → `Value::StructureInstance("MidSurfaceMesh")`
///   → `fields["vertices"]` → `Value::List` of `Value::List([Real, Real, Real])`
///   → max of the first (x) coordinate.
///
/// Returns `f64::NEG_INFINITY` when the vertex list is empty.
fn max_mid_surface_vertex_x(result: &Value) -> f64 {
    let data = match result {
        Value::StructureInstance(d) => d,
        other => panic!("expected Value::StructureInstance for result; got: {other:?}"),
    };
    let mid_surface = data
        .fields
        .get("mid_surface")
        .expect("missing 'mid_surface' field in ShellExtractionResult");
    let ms_data = match mid_surface {
        Value::StructureInstance(d) => d,
        other => panic!("expected Value::StructureInstance for mid_surface; got: {other:?}"),
    };
    let vertices = ms_data
        .fields
        .get("vertices")
        .expect("missing 'vertices' field in MidSurfaceMesh");
    let vlist = match vertices {
        Value::List(vs) => vs,
        other => panic!("expected Value::List for vertices; got: {other:?}"),
    };
    vlist
        .iter()
        .map(|v| {
            let coords = match v {
                Value::List(c) => c,
                other => panic!("expected Value::List for vertex coords; got: {other:?}"),
            };
            match coords.first() {
                Some(Value::Real(x)) => *x,
                other => panic!("expected Value::Real as first vertex coord; got: {other:?}"),
            }
        })
        .fold(f64::NEG_INFINITY, f64::max)
}

/// Assert that the trampoline reads the realization SDF (footprint [0,2.0]²)
/// rather than the value_inputs[1] slab (footprint [0,1.0]²) when a non-None
/// realization handle is present in realization_inputs[0].
///
/// Structural basis (not a tuned tolerance): a slab's medial surface is its z=0
/// mid-plane spanning the in-plane footprint, so the realization field over
/// [0,2.0]² yields mid-surface vertices reaching x≈2.0 (max x > 1.0), whereas
/// the [0,1.0]² slab cannot exceed 1.0. The assertion uniquely identifies which
/// SDF was the geometry source.
///
/// RED today: `_realization_inputs` is ignored; value_inputs[1] (the [0,1.0]²
/// slab) is used → max x ≤ 1.0 → `max_x > 1.0` assertion fails.
/// GREEN after step-2 implements dual-source selection: realization_inputs[0].sdf()
/// is Some → large slab is used → max x > 1.0.
///
/// Pins docs/prds/v0_6/realization-read-api.md §9 task ε / D3
/// "prefer realization_inputs[0].sdf() when present".
#[test]
fn shell_extract_dual_source_prefers_realization_sdf_over_slab() {
    // Build realization handle carrying the LARGE [0,2.0]² slab.
    let large_field = synthetic_large_slab_field();
    let handle = RealizationReadHandle::new(
        RealizationNodeId::new("body", 0),
        ContentHash(1),
        Some(RealizedContent::Sdf(Arc::new(large_field))),
    );

    // value_inputs[1] carries the SMALL [0,1.0]² slab (the fallback path).
    let small_slab = Value::SampledField(synthetic_slab_field());

    let outcome = shell_extract_compute_fn(
        &[Value::Undef, small_slab],
        &[handle],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    // Must succeed (the realization SDF is valid).
    let result = match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!(
            "expected ComputeOutcome::Completed when realization SDF is present; \
             got: {other:?}. (RED until step-2 implements dual-source selection)"
        ),
    };

    // Max mid-surface vertex x must exceed the small slab's footprint (1.0),
    // proving the large realization field [0,2.0]² was the geometry source.
    let max_x = max_mid_surface_vertex_x(&result);
    assert!(
        max_x > 1.0,
        "expected max mid-surface vertex x > 1.0 (realization [0,2.0]² footprint); \
         got max_x = {max_x:.4}. If max_x ≤ 1.0, the fallback value_inputs[1] slab \
         was used instead of the realization SDF — dual-source selection not yet active."
    );
}

/// Assert that the trampoline completes successfully when only the realization
/// SDF is present (value_inputs carries no index 1).
///
/// RED today: the current code falls through to `value_inputs.get(1)` → None →
/// `ComputeOutcome::Failed`. GREEN after step-2 the realization arm is checked
/// first, so the absence of value_inputs[1] is not an error.
///
/// Pins docs/prds/v0_6/realization-read-api.md §9 task ε / D3
/// "realization_inputs[0].sdf() alone is sufficient".
#[test]
fn shell_extract_uses_realization_sdf_when_value_input_slab_absent() {
    // Build realization handle carrying the LARGE [0,2.0]² slab.
    let large_field = synthetic_large_slab_field();
    let handle = RealizationReadHandle::new(
        RealizationNodeId::new("body", 0),
        ContentHash(1),
        Some(RealizedContent::Sdf(Arc::new(large_field))),
    );

    // value_inputs = [options only]; no index 1 (no slab value).
    let outcome = shell_extract_compute_fn(
        &[Value::Undef],
        &[handle],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    match outcome {
        ComputeOutcome::Completed { .. } => {}
        other => panic!(
            "expected ComputeOutcome::Completed when realization SDF is present \
             and value_inputs has no index 1; got: {other:?}. \
             (RED until step-2 implements dual-source selection)"
        ),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Step-3 tests for task #4511 (ε dual-source: error arm + fallback regression)
// ─────────────────────────────────────────────────────────────────────────────

/// Assert that the trampoline fails with a dual-source diagnostic when neither
/// a realization SDF nor a value_inputs[1] slab is present.
///
/// The failure diagnostic must:
/// 1. Be `ComputeOutcome::Failed`.
/// 2. Contain at least one diagnostic message that references BOTH
///    "realization_inputs[0]" AND "value_inputs[1]" — the dual-source contract.
///
/// RED after step-2: step-2 deliberately kept the old single-source wording.
/// GREEN after step-4 rewrites the diagnostic to the dual-source form.
///
/// Pins docs/prds/v0_6/realization-read-api.md §9 ε / D3.
#[test]
fn shell_extract_fails_when_neither_realization_nor_slab_present() {
    // No realization handle (empty slice), no value_inputs[1] slab.
    let outcome = shell_extract_compute_fn(
        &[Value::Undef],
        &[],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    // (1) Must return Failed.
    let diagnostics = match outcome {
        ComputeOutcome::Failed { diagnostics } => diagnostics,
        other => panic!(
            "expected ComputeOutcome::Failed when neither realization SDF nor \
             value_inputs[1] is present; got: {other:?}"
        ),
    };

    // (2) At least one diagnostic must reference the dual-source contract:
    // both "realization_inputs[0]" AND "value_inputs[1]" in the message.
    let dual_source_msg = diagnostics.iter().find(|d| {
        d.message.contains("realization_inputs[0]") && d.message.contains("value_inputs[1]")
    });
    assert!(
        dual_source_msg.is_some(),
        "expected at least one diagnostic referencing both 'realization_inputs[0]' \
         and 'value_inputs[1]' (dual-source contract); got: {diagnostics:?}. \
         (RED until step-4 rewrites the error diagnostic)"
    );
}

/// Assert that the trampoline falls back to value_inputs[1] and completes
/// successfully when the realization handle's content is None (sdf() == None).
///
/// This is a regression test pinning PRD §8 "consumes sdf() when present and
/// falls back to slab when None" — the fallback path introduced in step-2.
///
/// The mid-surface vertex max x-coordinate must be ≤ ~1.0 (tracking the
/// [0,1.0]² slab footprint), confirming the fallback arm was taken rather
/// than some realization-derived geometry.
///
/// GREEN after step-2 because: realization handle has content=None → sdf()==None
/// → fallback to value_inputs[1] slab → Completed, max x ≤ 1.0.
///
/// Pins docs/prds/v0_6/realization-read-api.md §8 / §9 ε / D3.
#[test]
fn shell_extract_falls_back_to_slab_when_realization_sdf_none() {
    // Build a handle with content=None so sdf() returns None.
    let none_handle =
        RealizationReadHandle::new(RealizationNodeId::new("b", 0), ContentHash(0), None);

    let slab = Value::SampledField(synthetic_slab_field());

    let outcome = shell_extract_compute_fn(
        &[Value::Undef, slab],
        &[none_handle],
        &Value::Undef,
        None,
        &CancellationHandle::new(),
    );

    // Must succeed (fallback to value_inputs[1] slab).
    let result = match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!(
            "expected ComputeOutcome::Completed when realization sdf()==None and \
             value_inputs[1] slab is present; got: {other:?}"
        ),
    };

    // Max mid-surface vertex x must be within the [0,1.0]² slab footprint,
    // confirming the fallback slab (not a realization field) was used.
    let max_x = max_mid_surface_vertex_x(&result);
    assert!(
        max_x <= 1.01,
        "expected max mid-surface vertex x ≤ 1.01 (slab [0,1.0]² fallback footprint); \
         got max_x = {max_x:.4}. If max_x > 1.0, some other geometry source was used."
    );
}
