//! ComputeNode trampoline and registration for the `"shell-extract::extract"`
//! target (task γ, #3834).
//!
//! See `docs/prds/v0_4/shell-extract-engine-bridge.md` §4–§8 and
//! `docs/prds/v0_3/compute-node-contract.md` §4 for the full specification.
//!
//! # γ-only SampledField seam
//!
//! PRD §5 contract: `value_inputs=[options: ElasticOptions]`,
//! `realization_inputs=[body_geom: BRep or Mesh]`. However,
//! `RealizationReadHandle` content accessors are deferred to δ/ε/ζ per
//! `engine_compute.rs:104-110`. For γ the trampoline reads the geometry SDF
//! from `value_inputs[1]` (a `Value::SampledField`) with an inline
//! `// γ-only seam` comment. Tasks δ/ε will migrate it to
//! `realization_inputs[0]` once the realization-read API lands.
//!
//! # Cancellation granularity
//!
//! Per PRD §11 OQ-5 (decided during γ): cancellation is polled at each of
//! the five phase boundaries (medial-mask, mid-surface, prune, mesh, segment)
//! rather than per-voxel. Per-phase polling is sufficient for sub-100ms
//! synthetic-slab runs; tighter inner-loop granularity can land in ε or a
//! follow-up without interface breakage.

use reify_core::persistent_cache::PersistentlyCacheable;
use reify_core::Diagnostic;
use reify_ir::{FeatureId, OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_shell_extract::{
    MedialOptions, MesherOptions, MidSurfaceOptions, PruneOptions, SegmentationError,
    SegmentationOptions, SingleBodyMask, compute_medial_mask, extract_mid_surface,
    mesh_mid_surface, populate_mid_surface_attributes, prune_branches, segment_regions,
};

use crate::engine_compute::{ComputeFn, ComputeOutcome, RealizationReadHandle};
use crate::graph::CancellationHandle;
use crate::Engine;

// ── Value-projection helper ──────────────────────────────────────────────────

/// Project a `ShellExtractionResult` into a `Value::StructureInstance`.
///
/// The projection is **deterministic**: same inputs → byte-identical output.
/// This is essential for the in-memory compute-node cache (step-7/8 of task
/// γ, #3834): after `run_compute_dispatch` the cache entry's content hash
/// must be stable across re-dispatches.
///
/// Determinism is preserved by:
/// - Iterating `mesh.vertices` / `mesh.triangles` / `mesh.thickness` in index
///   order (not via `PersistentMap` walk, which is not guaranteed ordered).
/// - Iterating `segmentation.regions` in their existing index order.
/// - Leaving the diagnostics list in arrival order (empty on the success path).
///
/// PRD §5 cache-key composition forward link: `shell-extract-engine-bridge.md §5`.
fn shell_extraction_result_to_value(
    result: &reify_shell_extract::ShellExtractionResult,
) -> Value {
    // ── mid_surface ─────────────────────────────────────────────────────────
    let vertices_value = Value::List(
        result
            .mid_surface
            .vertices
            .iter()
            .map(|v| {
                Value::List(
                    v.iter()
                        .copied()
                        .map(Value::Real)
                        .collect::<Vec<_>>()
                        .into(),
                )
            })
            .collect::<Vec<_>>()
            .into(),
    );
    let triangles_value = Value::List(
        result
            .mid_surface
            .triangles
            .iter()
            .map(|t| {
                Value::List(
                    t.iter()
                        .map(|&i| Value::Int(i as i64))
                        .collect::<Vec<_>>()
                        .into(),
                )
            })
            .collect::<Vec<_>>()
            .into(),
    );
    let thickness_value = Value::List(
        result
            .mid_surface
            .thickness
            .iter()
            .copied()
            .map(Value::Real)
            .collect::<Vec<_>>()
            .into(),
    );
    let mut mid_surface_fields = PersistentMap::default();
    mid_surface_fields.insert("vertices".to_string(), vertices_value);
    mid_surface_fields.insert("triangles".to_string(), triangles_value);
    mid_surface_fields.insert("thickness".to_string(), thickness_value);
    let mid_surface_value = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "MidSurfaceMesh".to_string(),
        version: 1,
        fields: mid_surface_fields,
    }));

    // ── segmentation ─────────────────────────────────────────────────────────
    // Iterate regions in index order to preserve determinism.
    let regions_value = Value::List(
        result
            .segmentation
            .regions
            .iter()
            .map(|r| {
                let mut rf = PersistentMap::default();
                rf.insert(
                    "classification".to_string(),
                    Value::String(format!("{:?}", r.classification)),
                );
                rf.insert(
                    "voxel_count".to_string(),
                    Value::Int(r.voxels.len() as i64),
                );
                Value::StructureInstance(Box::new(StructureInstanceData {
                    type_id: StructureTypeId(0),
                    type_name: "RegionInfo".to_string(),
                    version: 1,
                    fields: rf,
                }))
            })
            .collect::<Vec<_>>()
            .into(),
    );
    let vertex_labels_value = Value::List(
        result
            .segmentation
            .vertex_labels
            .iter()
            .map(|&l| Value::Int(l as i64))
            .collect::<Vec<_>>()
            .into(),
    );
    let triangle_labels_value = Value::List(
        result
            .segmentation
            .triangle_labels
            .iter()
            .map(|&l| Value::Int(l as i64))
            .collect::<Vec<_>>()
            .into(),
    );
    let mut seg_fields = PersistentMap::default();
    seg_fields.insert("regions".to_string(), regions_value);
    seg_fields.insert("vertex_labels".to_string(), vertex_labels_value);
    seg_fields.insert("triangle_labels".to_string(), triangle_labels_value);
    let segmentation_value = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "SegmentationResult".to_string(),
        version: 1,
        fields: seg_fields,
    }));

    // ── naming ───────────────────────────────────────────────────────────────
    let face_count = Value::Int(result.naming.face_records.len() as i64);
    let edge_count = Value::Int(result.naming.edges.len() as i64);
    let mut naming_fields = PersistentMap::default();
    naming_fields.insert("face_count".to_string(), face_count);
    naming_fields.insert("edge_count".to_string(), edge_count);
    let naming_value = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "MidSurfaceAttributes".to_string(),
        version: 1,
        fields: naming_fields,
    }));

    // ── diagnostics list ─────────────────────────────────────────────────────
    // Arrival order preserved; empty on the success path.
    let diags_value = Value::List(
        result
            .diagnostics
            .iter()
            .map(|d| Value::String(format!("{:?}", d.message)))
            .collect::<Vec<_>>()
            .into(),
    );

    // ── top-level StructureInstance ──────────────────────────────────────────
    let mut fields = PersistentMap::default();
    fields.insert("mid_surface".to_string(), mid_surface_value);
    fields.insert("segmentation".to_string(), segmentation_value);
    fields.insert("naming".to_string(), naming_value);
    fields.insert(
        "solve_time_ms".to_string(),
        Value::Int(result.solve_time_ms as i64),
    );
    fields.insert("diagnostics".to_string(), diags_value);

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "ShellExtractionResult".to_string(),
        version: 1,
        fields,
    }))
}

// ── Options parsing helper ───────────────────────────────────────────────────

/// Parse `shell_threshold`, `shell_voxel_size`, and `shell_branch_prune_ratio`
/// from a `Value::StructureInstance("ElasticOptions")` or fall back to
/// producer defaults on `Value::Undef` / missing fields.
///
/// Mapping per PRD §5:
/// - `shell_threshold` → `SegmentationOptions.shell_threshold`
/// - `shell_voxel_size` → `MedialOptions.distance_tolerance` (proxy)
/// - `shell_branch_prune_ratio` → `PruneOptions.shell_branch_prune_ratio`
///
/// Returns the five option structs needed by the producer pipeline.
fn parse_elastic_options_from_value(
    opts: &Value,
) -> (
    MedialOptions,
    MidSurfaceOptions,
    PruneOptions,
    MesherOptions,
    SegmentationOptions,
) {
    let mut medial_opts = MedialOptions::default();
    let mid_surf_opts = MidSurfaceOptions::default();
    let mut prune_opts = PruneOptions::default();
    // γ-trampoline default: relax the angle gate to 10° (vs. the producer's
    // strict 20° FEA default). The trampoline processes arbitrary SDF geometry
    // including coarse synthetic grids; 10° still rejects near-degenerate
    // slivers while accepting the stretched triangles that emerge from
    // anisotropic grid spacings. Callers can override via an ElasticOptions
    // StructureInstance once the options field map exposes min_angle_degrees.
    let mesher_opts = MesherOptions {
        min_angle_degrees: 10.0,
        ..MesherOptions::default()
    };
    let mut seg_opts = SegmentationOptions::default();

    if let Value::StructureInstance(data) = opts {
        if let Some(Value::Real(v)) = data.fields.get(&"shell_threshold".to_string()) {
            seg_opts.shell_threshold = *v;
        }
        if let Some(Value::Real(v)) = data.fields.get(&"shell_voxel_size".to_string()) {
            medial_opts.distance_tolerance = *v;
        }
        if let Some(Value::Real(v)) = data.fields.get(&"shell_branch_prune_ratio".to_string()) {
            prune_opts.shell_branch_prune_ratio = *v;
        }
    }
    // Value::Undef or any other variant → all defaults (correct per PRD §5).

    (medial_opts, mid_surf_opts, prune_opts, mesher_opts, seg_opts)
}

// ── Trampoline ───────────────────────────────────────────────────────────────

/// Synchronous compute trampoline for `"shell-extract::extract"`.
///
/// # Inputs (γ-only shape)
///
/// - `value_inputs[0]`: `Value::StructureInstance("ElasticOptions")` or
///   `Value::Undef` (use producer defaults)
/// - `value_inputs[1]`: `Value::SampledField` carrying the SDF of the body
///   geometry — **γ-only seam**; tasks δ/ε will migrate this to
///   `realization_inputs[0]` once `RealizationReadHandle` content accessors land.
///
/// # Cancellation
///
/// Polled at each of the five phase boundaries (medial-mask, mid-surface,
/// prune, mesh, segment). Per PRD §11 OQ-5: per-phase polling is sufficient
/// for synthetic-slab runtimes; tighter inner-loop granularity deferred to ε.
pub fn shell_extract_compute_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // ── Read SampledField from value_inputs[1] (γ-only seam) ─────────────────
    let sdf = match value_inputs.get(1) {
        Some(Value::SampledField(sf)) => sf,
        _ => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(
                    "shell-extract::extract: value_inputs[1] must be Value::SampledField \
                     (γ-only seam; δ migrates to realization_inputs[0])",
                )],
            };
        }
    };

    // ── Parse options ─────────────────────────────────────────────────────────
    // value_inputs[0] takes priority; fall back to the `options` parameter.
    let opts_value = value_inputs.get(0).unwrap_or(options);
    let (medial_opts, mid_surf_opts, prune_opts, mesher_opts, seg_opts) =
        parse_elastic_options_from_value(opts_value);

    let t_start = std::time::Instant::now();

    // ── Phase 1: medial mask ───────────────────────────────────────────────
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }
    let medial_mask = match compute_medial_mask(sdf, &medial_opts) {
        Ok(m) => m,
        Err(e) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: medial-mask phase: {e}"
                ))],
            };
        }
    };

    // ── Phase 2: mid-surface extraction ───────────────────────────────────
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }
    let raw_mesh = match extract_mid_surface(sdf, &medial_mask, &mid_surf_opts) {
        Ok(m) => m,
        Err(e) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: mid-surface phase: {e}"
                ))],
            };
        }
    };

    // ── Phase 3: branch pruning ────────────────────────────────────────────
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }
    let pruned_mesh = match prune_branches(&raw_mesh, &prune_opts) {
        Ok(pr) => pr.mesh,
        Err(e) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: prune phase: {e}"
                ))],
            };
        }
    };

    // ── Phase 4: meshing ────────────────────────────────────────────────────
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }
    let meshed = match mesh_mid_surface(&pruned_mesh, &mesher_opts) {
        Ok(m) => m.mesh,
        Err(e) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: mesh phase: {e}"
                ))],
            };
        }
    };

    // ── Phase 5: segmentation ───────────────────────────────────────────────
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }
    let single_body = SingleBodyMask::new(medial_mask);
    let segmentation = match segment_regions(&single_body, &meshed, &seg_opts) {
        Ok(s) => s,
        Err(SegmentationError::InvalidThreshold { value }) => {
            // PRD §7 row 3 — E_SHELL_BAD_THRESHOLD mapping.
            // `DiagnosticCode::ShellBadThreshold` added in step-6 (task γ,
            // #3834). Message template matches the PRD §7 canonical form.
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell_threshold = {value} must be in (0.0, 1.0)."
                ))
                .with_code(reify_core::DiagnosticCode::ShellBadThreshold)],
            };
        }
        Err(e) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: segmentation phase: {e}"
                ))],
            };
        }
    };

    // ── Phase 6: naming (fast in-memory; no cancellation poll needed) ────────
    let naming =
        populate_mid_surface_attributes(&FeatureId::new("synthetic"), &meshed, &segmentation);

    let solve_time_ms = t_start.elapsed().as_millis() as u64;

    // ── Build ShellExtractionResult ────────────────────────────────────────
    let result = match reify_shell_extract::ShellExtractionResult::new(
        meshed,
        segmentation,
        naming,
        solve_time_ms,
        vec![],
    ) {
        Ok(r) => r,
        Err(e) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: length invariant violated: {e}"
                ))],
            };
        }
    };

    // Uncompressed byte size for cost_per_byte; guard against zero with .max(1)
    // to avoid division by zero on empty results. The `.max(1)` means the
    // effective cost floor is solve_time_ms nanoseconds-per-byte — acceptable
    // for LRU heuristics.
    let uncompressed_bytes = result.uncompressed_byte_size().max(1);
    let cost_per_byte = (solve_time_ms as f64) / (uncompressed_bytes as f64);

    let result_value = shell_extraction_result_to_value(&result);

    ComputeOutcome::Completed {
        result: result_value,
        new_warm_state: None,
        cost_per_byte: Some(cost_per_byte),
        diagnostics: vec![],
    }
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Register the `"shell-extract::extract"` trampoline with `engine`.
///
/// Called by binary entry points (CLI, GUI, test harnesses) that wish to
/// enable the shell-extract pipeline. Panics if `"shell-extract::extract"` is
/// already registered (PRD §4 hard-error contract, propagated from
/// `Engine::register_compute_fn`).
///
/// # Design note
///
/// This is a stand-alone `pub fn` rather than a generic aggregator because no
/// workspace-wide `register_compute_fns` function exists today (PRD §4 and
/// γ design decision). Future task ι (end-to-end smoke binary) is the natural
/// point to introduce an aggregator if needed.
pub fn register_shell_extract_compute_fns(engine: &mut Engine) {
    engine.register_compute_fn("shell-extract::extract", shell_extract_compute_fn as ComputeFn);
}
