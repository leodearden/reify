//! Cross-restart persistent-cache e2e integration test for shell-extract
//! (task #4071 step-5/step-6).
//!
//! ## Observable signal
//!
//! Engine A cold-dispatches `"shell-extract::extract"` with a shared cache dir
//! and a fixed non-zero `ContentHash`.  It writes a `.bin` to the cache and
//! folds `MidSurfaceFace`/`MidSurfaceEdge` entries into its topology_attribute_table.
//!
//! Engine B (fresh, same cache dir + same key) hits the on-disk cache:
//! - `persistent_hit_count() == 1` — the lookup-before-invoke path fired
//! - `persistent_miss_count() == 0` — no fall-through to the trampoline
//! - topology_attribute_table entries byte-identical to Engine A
//!
//! ## RED signal (before step-6)
//!
//! Without the `"shell-extract::extract"` write/lookup arms in compute_persist.rs,
//! Engine A does not write a `.bin` (write arm missing → `_ => {}`) and Engine B
//! does not hit (lookup arm missing → `_ => None`).  Assertions
//! `has_bin_file`, `hit_count == 1`, `miss_count == 0` all fail.

use reify_core::{ComputeNodeId, ContentHash, ValueCellId, VersionId};
use reify_eval::{CancellationHandle, register_shell_extract_compute_fns};
use reify_ir::{
    Freshness, InterpolationKind, Role, SampledField, SampledGridKind, Value,
};
use reify_test_support::make_simple_engine;

// ── Shared fixture (copied verbatim from mid_surface_fold_e2e.rs) ────────────

/// Construct a synthetic thin-slab `SampledField` (5×5×3 grid) whose SDF
/// encodes a slab centred at z=0 with half-thickness 0.1.
fn synthetic_slab_field() -> SampledField {
    let x_grid: Vec<f64> = (0..5).map(|i| i as f64 * 0.25).collect();
    let y_grid: Vec<f64> = (0..5).map(|i| i as f64 * 0.25).collect();
    let z_grid: Vec<f64> = vec![-0.5, 0.0, 0.5];

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

// ── Helper ───────────────────────────────────────────────────────────────────

/// Recursively check whether a `.bin` file exists anywhere under `dir`.
fn has_bin_file(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .any(|e| {
            let p = e.path();
            if p.is_dir() {
                has_bin_file(&p)
            } else {
                p.extension().is_some_and(|x| x == "bin")
            }
        })
}

/// Extract the length of `segmentation.regions` from a
/// `Value::StructureInstance("ShellExtractionResult")`.  Returns `None` if `v`
/// is not the expected shape.  Used to pin the lossy-invariant assertion (B4).
fn segmentation_regions_len(v: &Value) -> Option<usize> {
    let outer = match v {
        Value::StructureInstance(d) if d.type_name == "ShellExtractionResult" => d,
        _ => return None,
    };
    let seg = match outer.fields.get("segmentation") {
        Some(Value::StructureInstance(d)) => d,
        _ => return None,
    };
    match seg.fields.get("regions") {
        Some(Value::List(rs)) => Some(rs.len()),
        _ => None,
    }
}

/// Dispatch `"shell-extract::extract"` on `engine` with the synthetic slab
/// fixture and `cache_key`, then collect the sorted topology_attribute_table
/// entries as `(handle_id, role, local_index, feature_id_str)`.
fn dispatch_and_collect(
    engine: &mut reify_eval::Engine,
    version: u64,
    cache_key: ContentHash,
) -> (Vec<(u64, Role, u32, String)>, Value) {
    let value_inputs = vec![Value::Undef, Value::SampledField(synthetic_slab_field())];
    let c_id = ComputeNodeId::new("ShellExtractCacheRoundTrip", 0);
    let cell = ValueCellId::new("ShellExtractCacheRoundTrip", "result");

    let (dispatched_value, _, _) = engine
        .run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "shell-extract::extract",
            &value_inputs,
            &[],
            &Value::Undef,
            &CancellationHandle::new(),
            VersionId(version),
            cache_key,
        )
        .expect("run_compute_dispatch must succeed");

    // Confirm VC is Final.
    let node = reify_eval::cache::NodeId::Value(cell.clone());
    assert_eq!(
        engine.freshness(&node),
        Freshness::Final,
        "post-dispatch freshness must be Final"
    );

    let mut entries: Vec<(u64, Role, u32, String)> = engine
        .topology_attribute_table()
        .iter()
        .map(|(id, attr)| {
            (
                id.0,
                attr.role,
                attr.local_index,
                attr.feature_id.to_string(),
            )
        })
        .collect();
    entries.sort_by_key(|(id, _, _, _)| *id);
    (entries, dispatched_value)
}

// ── The round-trip test ───────────────────────────────────────────────────────

/// Cross-restart shell-extract persistent-cache round-trip.
///
/// Engine A cold-dispatches (writing cache, folding table from trampoline result).
/// Engine B hits the on-disk cache (no trampoline call, table folded from disk).
/// Asserts: hit_count==1, miss_count==0 on Engine B; table entries byte-identical.
#[test]
fn shell_extract_persistent_cache_cross_restart_round_trip() {
    let tmp = tempfile::TempDir::new().expect("tmp dir creation must succeed");

    // Fixed non-zero cache key — same for both engines so they share the entry.
    let cache_key = ContentHash(0xaa55_1234_aa55_5678_aa55_1234_aa55_5678_u128);

    // ── Engine A: cold dispatch ─────────────────────────────────────────────

    let mut engine_a = make_simple_engine();
    engine_a.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    register_shell_extract_compute_fns(&mut engine_a);

    let (entries_a, value_a) = dispatch_and_collect(&mut engine_a, 1, cache_key);

    // (A1) Engine A is cold — no persistent hit.
    assert_eq!(
        engine_a.persistent_hit_count(),
        0,
        "Engine A is a cold dispatch — persistent_hit_count must be 0",
    );

    // (A2) A .bin must now exist in the cache dir.
    assert!(
        has_bin_file(tmp.path()),
        "A .bin file must exist under the cache dir after Engine A cold shell-extract dispatch",
    );

    // (A3) Engine A must have produced ≥1 table entries.
    assert!(
        !entries_a.is_empty(),
        "Engine A must produce ≥1 topology_attribute_table entries",
    );

    // ── Engine B: warm lookup ───────────────────────────────────────────────

    let mut engine_b = make_simple_engine();
    engine_b.set_persistent_cache_dir(Some(tmp.path().to_path_buf()));
    register_shell_extract_compute_fns(&mut engine_b);

    let (entries_b, value_b) = dispatch_and_collect(&mut engine_b, 2, cache_key);

    // (B1) Persistent HIT count must be exactly 1.
    assert_eq!(
        engine_b.persistent_hit_count(),
        1,
        "Engine B must get exactly 1 persistent cache hit; \
         hit_count==0 means the write arm (step-6) did not fire or the key did not match",
    );

    // (B2) No lookup MISS — confirms trampoline was NOT invoked.
    assert_eq!(
        engine_b.persistent_miss_count(),
        0,
        "Engine B must have 0 persistent misses; \
         miss_count>0 means the lookup arm (step-6) did not fire or returned None",
    );

    // (B3) Table entries must be byte-identical to Engine A (disk rehydration).
    assert_eq!(
        entries_a, entries_b,
        "Engine B topology_attribute_table entries must be byte-identical to Engine A; \
         value_to_shell_extraction_result must faithfully recover naming fields \
         (feature_id + local_index) so fold_mid_surface_attributes_into_table \
         produces the same synthetic GeometryHandleIds on both engines. \
         entries_a={entries_a:?}, entries_b={entries_b:?}",
    );

    // (B4) Self-verifying lossy-invariant pin: the cold Value carries populated
    // segmentation.regions (produced by the actual shell-extract trampoline);
    // the warm-hit Value has regions=[] because value_to_shell_extraction_result
    // defaults this lossy field.  This is intentional — the #3428
    // on-disk-mirrors-in-memory invariant is RELAXED for shell-extract (see
    // engine_compute.rs comment at the persistent_write block).
    //
    // Both halves of the assertion must hold for it to be non-vacuous:
    //   cold > 0  →  the slab fixture actually produced ≥1 region (pin is live)
    //   warm == 0 →  the rehydration path dropped them (lossy default is active)
    // If the slab fixture ever produces 0 regions the first assertion fails,
    // signalling that the fixture needs updating before B4 can be meaningful.
    assert!(
        segmentation_regions_len(&value_a).is_some_and(|n| n > 0),
        "cold-dispatch Value must have segmentation.regions non-empty \
         (slab fixture must produce ≥1 region so the warm==0 pin is non-vacuous); \
         got {:?}",
        segmentation_regions_len(&value_a),
    );
    assert_eq!(
        segmentation_regions_len(&value_b),
        Some(0),
        "warm-hit Value must have segmentation.regions=[] \
         (value_to_shell_extraction_result lossy default — intentional, \
         no downstream consumer reads regions from a shell-extract Value)",
    );
}
