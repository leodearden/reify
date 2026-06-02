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

use std::hash::{Hash, Hasher};

use reify_core::persistent_cache::PersistentlyCacheable;
use reify_core::Diagnostic;
use reify_ir::{
    FeatureId, GeometryHandleId, OpaqueState, PersistentMap, Role, StructureInstanceData,
    StructureTypeId, TopologyAttribute, TopologyAttributeTable, Value,
};
use reify_shell_extract::{
    GridValidationError, MedialError, MedialOptions, MesherError, MesherOptions, MidSurfaceError,
    MidSurfaceOptions, PruneOptions, SegmentationError, SegmentationOptions, SingleBodyMask,
    compute_medial_mask, extract_mid_surface, mesh_mid_surface, populate_mid_surface_attributes,
    prune_branches, segment_regions,
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
/// - **Projecting `solve_time_ms` as `Value::Int(0)` regardless of the actual
///   elapsed time**: the measured timing is a non-deterministic side channel
///   that would produce different content hashes across re-dispatches on the
///   same inputs, breaking the cache short-circuit contract. The actual timing
///   is used only for `cost_per_byte` (passed to the LRU heuristic) and is
///   NOT folded into the projected `Value`. Consumers that need the timing for
///   display should read it from `ShellExtractionResult.solve_time_ms` directly
///   rather than from the cached `Value`.
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
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
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
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>(),
    );
    let thickness_value = Value::List(
        result
            .mid_surface
            .thickness
            .iter()
            .copied()
            .map(Value::Real)
            .collect::<Vec<_>>(),
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
            .collect::<Vec<_>>(),
    );
    let vertex_labels_value = Value::List(
        result
            .segmentation
            .vertex_labels
            .iter()
            .map(|&l| Value::Int(l as i64))
            .collect::<Vec<_>>(),
    );
    let triangle_labels_value = Value::List(
        result
            .segmentation
            .triangle_labels
            .iter()
            .map(|&l| Value::Int(l as i64))
            .collect::<Vec<_>>(),
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
    // ζ task #3596 step-2: project full face_records and edges lists so the
    // engine-side fold hook (step-6) can reconstruct TopologyAttribute entries
    // from the cached Value without re-running the producer.
    //
    // Each face record carries: feature_id (String) + local_index (Int).
    // Role is implied by the list (face_records → MidSurfaceFace,
    // edges → MidSurfaceEdge) and is not re-encoded.
    // Iteration is in index order (deterministic).
    let face_count = Value::Int(result.naming.face_records.len() as i64);
    let edge_count = Value::Int(result.naming.edges.len() as i64);

    let face_records_value = Value::List(
        result
            .naming
            .face_records
            .iter()
            .map(|attr| {
                let mut rf = PersistentMap::default();
                rf.insert(
                    "feature_id".to_string(),
                    Value::String(attr.feature_id.to_string()),
                );
                rf.insert("local_index".to_string(), Value::Int(attr.local_index as i64));
                Value::StructureInstance(Box::new(StructureInstanceData {
                    type_id: StructureTypeId(0),
                    type_name: "MidSurfaceFaceRecord".to_string(),
                    version: 1,
                    fields: rf,
                }))
            })
            .collect::<Vec<_>>(),
    );

    // Edges: sorted order already guaranteed by populate_mid_surface_attributes
    // (BTreeSet dedup → ascending (min,max) region-pair order).
    let edges_value = Value::List(
        result
            .naming
            .edges
            .iter()
            .map(|edge| {
                let mut ef = PersistentMap::default();
                ef.insert(
                    "feature_id".to_string(),
                    Value::String(edge.attribute.feature_id.to_string()),
                );
                ef.insert(
                    "local_index".to_string(),
                    Value::Int(edge.attribute.local_index as i64),
                );
                Value::StructureInstance(Box::new(StructureInstanceData {
                    type_id: StructureTypeId(0),
                    type_name: "MidSurfaceEdgeRecord".to_string(),
                    version: 1,
                    fields: ef,
                }))
            })
            .collect::<Vec<_>>(),
    );

    let mut naming_fields = PersistentMap::default();
    naming_fields.insert("face_count".to_string(), face_count);
    naming_fields.insert("edge_count".to_string(), edge_count);
    naming_fields.insert("face_records".to_string(), face_records_value);
    naming_fields.insert("edges".to_string(), edges_value);
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
            .collect::<Vec<_>>(),
    );

    // ── top-level StructureInstance ──────────────────────────────────────────
    let mut fields = PersistentMap::default();
    fields.insert("mid_surface".to_string(), mid_surface_value);
    fields.insert("segmentation".to_string(), segmentation_value);
    fields.insert("naming".to_string(), naming_value);
    // Project solve_time_ms as 0 rather than the actual measured time.
    // Rationale: the elapsed time is a non-deterministic measurement that
    // would produce different content hashes across re-dispatches on the same
    // inputs, breaking the cache short-circuit contract (see helper rustdoc
    // and step-8 of task γ, #3834). The actual timing feeds only
    // `cost_per_byte` and must not perturb the projected Value.
    fields.insert("solve_time_ms".to_string(), Value::Int(0));
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
    let opts_value = value_inputs.first().unwrap_or(options);
    let (medial_opts, mid_surf_opts, prune_opts, mesher_opts, seg_opts) =
        parse_elastic_options_from_value(opts_value);

    let t_start = std::time::Instant::now();

    // ── Phase 1: medial mask ───────────────────────────────────────────────
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }
    let medial_mask = match compute_medial_mask(sdf, &medial_opts) {
        Ok(m) => m,
        // PRD §7 row 1 — E_SHELL_NO_VOXEL_GRID: empty axis grid in medial-mask
        // phase. Maps the GridValidationError::EmptyAxisGrid variant with the
        // §7 canonical message template. Other MedialError variants fall through
        // to the generic coded-less arm.
        Err(MedialError::GridValidation(GridValidationError::EmptyAxisGrid { axis })) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: voxel grid is empty on axis {axis}; \
                     cannot compute medial mask. Verify the body geometry produces \
                     a valid voxel grid."
                ))
                .with_code(reify_core::DiagnosticCode::ShellNoVoxelGrid)],
            };
        }
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
        // PRD §7 row 1 — E_SHELL_NO_VOXEL_GRID: empty axis grid in mid-surface
        // phase (same root cause as medial-mask EmptyAxisGrid).
        Err(MidSurfaceError::GridValidation(GridValidationError::EmptyAxisGrid { axis })) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: voxel grid is empty on axis {axis}; \
                     cannot extract mid-surface. Verify the body geometry produces \
                     a valid voxel grid."
                ))
                .with_code(reify_core::DiagnosticCode::ShellNoVoxelGrid)],
            };
        }
        // PRD §7 row 2 — E_SHELL_MEDIAL_MASK_OOB: a medial-mask voxel lies
        // outside the SDF grid extent.
        Err(MidSurfaceError::MaskVoxelOutOfBounds { voxel, grid_extent }) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: medial-mask voxel [{}, {}, {}] is \
                     outside the SDF grid extent [{}, {}, {}].",
                    voxel[0], voxel[1], voxel[2],
                    grid_extent[0], grid_extent[1], grid_extent[2],
                ))
                .with_code(reify_core::DiagnosticCode::ShellMedialMaskOob)],
            };
        }
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
        // PRD §7 row 4 — E_SHELL_PRUNE_FAILED: any branch-pruning failure
        // (invalid ratio, invalid max-iterations, invalid alignment tolerance)
        // maps to ShellPruneFailed. All PruneError variants indicate a
        // configuration or geometry constraint violation.
        Err(e) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: branch-pruning failed: {e}"
                ))
                .with_code(reify_core::DiagnosticCode::ShellPruneFailed)],
            };
        }
    };

    // ── Phase 4: meshing ────────────────────────────────────────────────────
    if cancellation.is_cancelled() {
        return ComputeOutcome::Cancelled;
    }
    let meshed = match mesh_mid_surface(&pruned_mesh, &mesher_opts) {
        Ok(m) => m.mesh,
        // PRD §7 row 5 — E_SHELL_MESH_QUALITY: quality gate fails after all
        // remesh iterations. Maps MesherError::QualityBelowThreshold with the
        // §7 canonical message template. Other MesherError variants (invalid
        // parameters) fall through to the generic coded-less arm.
        Err(MesherError::QualityBelowThreshold { metrics, .. }) => {
            return ComputeOutcome::Failed {
                diagnostics: vec![Diagnostic::error(format!(
                    "shell-extract::extract: mid-surface mesh quality is below threshold \
                     (worst aspect ratio: {:.4}, worst min angle: {:.2}°). \
                     The shell geometry may be too complex or degenerate for meshing.",
                    metrics.min_aspect_ratio, metrics.min_angle_degrees,
                ))
                .with_code(reify_core::DiagnosticCode::ShellMeshQuality)],
            };
        }
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

// ── ζ §9-dispatch-complete fold hook ─────────────────────────────────────────

/// Mint a deterministic synthetic `GeometryHandleId` for a derived mid-surface
/// entity.
///
/// Packing layout (64 bits):
/// - Bit 63: always 1 — marks this as synthetic (disjoint from OCCT kernel
///   handles, which start at 1 and grow upward, never reaching the high bit).
/// - Bits 62–33: 30-bit FxHash of `feature_id` — disambiguates mid-surfaces
///   from different parent features in the same table.
/// - Bit 32: `is_edge` — separates MidSurfaceFace (0) from MidSurfaceEdge (1)
///   records for the same feature/index.
/// - Bits 31–0: `local_index` — 0-based index within the (feature_id, role)
///   pair.
///
/// Uses `rustc_hash::FxHasher` with default (fixed, zero) seed — NOT
/// `std::collections::hash_map::RandomState` — so IDs are identical across
/// process restarts (required for the step-7 re-derivation round-trip).
fn synthetic_mid_surface_handle_id(
    feature_id: &str,
    is_edge: bool,
    local_index: u32,
) -> GeometryHandleId {
    let mut hasher = rustc_hash::FxHasher::default();
    feature_id.hash(&mut hasher);
    let h = hasher.finish();
    let h30 = h & 0x3FFF_FFFF;
    let id = 0x8000_0000_0000_0000u64
        | (h30 << 33)
        | ((is_edge as u64) << 32)
        | (local_index as u64);
    GeometryHandleId(id)
}

/// Fold the MidSurfaceAttributes records from a projected `ShellExtractionResult`
/// Value into `table`.
///
/// Decodes `result.naming.face_records` and `result.naming.edges` (projected by
/// `shell_extraction_result_to_value` in step-2) and records one
/// `TopologyAttribute` per entry using deterministic synthetic
/// `GeometryHandleId`s (see `synthetic_mid_surface_handle_id`).
///
/// # Error handling
///
/// Mirrors the auxiliary-metadata convention of the existing per-op populators
/// (`populate_extrude/revolve/loft_attributes` in
/// `topology_attribute_propagation.rs`): on any decode anomaly (missing or
/// mistyped field) the record is silently skipped and a `tracing::warn!` is
/// emitted.  The function never panics and never causes the dispatch to regress
/// to `Failed`.
///
/// # Hash collision detection
///
/// `synthetic_mid_surface_handle_id` folds the `feature_id` into 30 bits of
/// FxHash.  Two distinct feature_ids that collide in those 30 bits while
/// sharing role and `local_index` would produce the same synthetic
/// `GeometryHandleId`, causing silent data loss.  This is detected at record
/// time: a `tracing::warn!` is emitted when `table.lookup(id)` returns an
/// existing entry whose `feature_id` differs from the incoming one.
/// Practical risk is very low — mid-surface feature_ids per design are few.
///
/// # Re-dispatch and stale entries
///
/// On re-dispatch of the same compute node where the result has *fewer*
/// regions/edges (e.g. a parameter change reduces the region count), synthetic
/// entries from the previous dispatch whose IDs are no longer produced will
/// linger in the table — they are never re-recorded, so the table can reflect
/// the union of all past dispatches rather than exactly the current result.
/// A proper fix requires `TopologyAttributeTable::retain()` or a per-entry
/// remove API, which is out of scope for this task (ζ, #3596).  The gap is
/// latent and non-observable today because `user_label = None` and dot-method
/// selectors fall to `Value::Undef`; it will surface as a phantom
/// `MidSurfaceFace`/`MidSurfaceEdge` target once selector vocab is wired
/// (tasks 2691/2699).
pub(crate) fn fold_mid_surface_attributes_into_table(
    table: &mut TopologyAttributeTable,
    result: &Value,
) {
    let outer = match result {
        Value::StructureInstance(d) => d,
        _ => {
            tracing::warn!("fold_mid_surface_attributes: result is not a StructureInstance");
            return;
        }
    };

    let naming = match outer.fields.get(&"naming".to_string()) {
        Some(Value::StructureInstance(d)) => d,
        _ => {
            tracing::warn!("fold_mid_surface_attributes: naming field missing or not a StructureInstance");
            return;
        }
    };

    // ── face_records → MidSurfaceFace entries ────────────────────────────────
    if let Some(Value::List(face_records)) = naming.fields.get(&"face_records".to_string()) {
        for (i, rec) in face_records.iter().enumerate() {
            let rec_data = match rec {
                Value::StructureInstance(d) => d,
                _ => {
                    tracing::warn!(
                        "fold_mid_surface_attributes: face_records[{i}] is not a StructureInstance"
                    );
                    continue;
                }
            };
            let feature_id_str = match rec_data.fields.get(&"feature_id".to_string()) {
                Some(Value::String(s)) => s.as_str(),
                _ => {
                    tracing::warn!(
                        "fold_mid_surface_attributes: face_records[{i}].feature_id missing or not String"
                    );
                    continue;
                }
            };
            let local_index = match rec_data.fields.get(&"local_index".to_string()) {
                Some(Value::Int(n)) => *n as u32,
                _ => {
                    tracing::warn!(
                        "fold_mid_surface_attributes: face_records[{i}].local_index missing or not Int"
                    );
                    continue;
                }
            };
            let id = synthetic_mid_surface_handle_id(feature_id_str, false, local_index);
            let attr = TopologyAttribute {
                feature_id: FeatureId::new(feature_id_str),
                role: Role::MidSurfaceFace,
                local_index,
                user_label: None,
                mod_history: vec![],
            };
            // Detect 30-bit FxHash collision at record time: warn if the synthetic
            // id is already occupied by a *different* feature_id — silent overwrite
            // would be undetectable data loss (see "Hash collision detection" in doc).
            if let Some(existing) = table.lookup(id)
                && existing.feature_id != attr.feature_id
            {
                tracing::warn!(
                    "fold_mid_surface_attributes: id {:#018x} 30-bit FxHash collision: \
                     existing feature_id={:?} overwritten by {:?}",
                    id.0,
                    existing.feature_id,
                    attr.feature_id,
                );
            }
            table.record(id, attr);
        }
    } else {
        tracing::warn!("fold_mid_surface_attributes: naming.face_records missing or not a List");
    }

    // ── edges → MidSurfaceEdge entries ───────────────────────────────────────
    if let Some(Value::List(edges)) = naming.fields.get(&"edges".to_string()) {
        for (i, rec) in edges.iter().enumerate() {
            let rec_data = match rec {
                Value::StructureInstance(d) => d,
                _ => {
                    tracing::warn!(
                        "fold_mid_surface_attributes: edges[{i}] is not a StructureInstance"
                    );
                    continue;
                }
            };
            let feature_id_str = match rec_data.fields.get(&"feature_id".to_string()) {
                Some(Value::String(s)) => s.as_str(),
                _ => {
                    tracing::warn!(
                        "fold_mid_surface_attributes: edges[{i}].feature_id missing or not String"
                    );
                    continue;
                }
            };
            let local_index = match rec_data.fields.get(&"local_index".to_string()) {
                Some(Value::Int(n)) => *n as u32,
                _ => {
                    tracing::warn!(
                        "fold_mid_surface_attributes: edges[{i}].local_index missing or not Int"
                    );
                    continue;
                }
            };
            let id = synthetic_mid_surface_handle_id(feature_id_str, true, local_index);
            let attr = TopologyAttribute {
                feature_id: FeatureId::new(feature_id_str),
                role: Role::MidSurfaceEdge,
                local_index,
                user_label: None,
                mod_history: vec![],
            };
            // Detect 30-bit FxHash collision at record time (see face-records
            // block above for the full rationale).
            if let Some(existing) = table.lookup(id)
                && existing.feature_id != attr.feature_id
            {
                tracing::warn!(
                    "fold_mid_surface_attributes: id {:#018x} 30-bit FxHash collision: \
                     existing feature_id={:?} overwritten by {:?}",
                    id.0,
                    existing.feature_id,
                    attr.feature_id,
                );
            }
            table.record(id, attr);
        }
    } else {
        tracing::warn!("fold_mid_surface_attributes: naming.edges missing or not a List");
    }
}

// ── Unit tests for fold helper ────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::{StructureTypeId, PersistentMap};

    /// Build a minimal ShellExtractionResult-shaped Value with known face/edge
    /// records for testing fold_mid_surface_attributes_into_table.
    fn make_result_value(
        face_records: &[(&str, u32)],
        edges: &[(&str, u32)],
    ) -> Value {
        let face_records_val = Value::List(
            face_records
                .iter()
                .map(|(fid, li)| {
                    let mut rf = PersistentMap::default();
                    rf.insert("feature_id".to_string(), Value::String(fid.to_string()));
                    rf.insert("local_index".to_string(), Value::Int(*li as i64));
                    Value::StructureInstance(Box::new(StructureInstanceData {
                        type_id: StructureTypeId(0),
                        type_name: "MidSurfaceFaceRecord".to_string(),
                        version: 1,
                        fields: rf,
                    }))
                })
                .collect::<Vec<_>>(),
        );
        let edges_val = Value::List(
            edges
                .iter()
                .map(|(fid, li)| {
                    let mut ef = PersistentMap::default();
                    ef.insert("feature_id".to_string(), Value::String(fid.to_string()));
                    ef.insert("local_index".to_string(), Value::Int(*li as i64));
                    Value::StructureInstance(Box::new(StructureInstanceData {
                        type_id: StructureTypeId(0),
                        type_name: "MidSurfaceEdgeRecord".to_string(),
                        version: 1,
                        fields: ef,
                    }))
                })
                .collect::<Vec<_>>(),
        );
        let mut naming_fields = PersistentMap::default();
        naming_fields.insert("face_count".to_string(), Value::Int(face_records.len() as i64));
        naming_fields.insert("edge_count".to_string(), Value::Int(edges.len() as i64));
        naming_fields.insert("face_records".to_string(), face_records_val);
        naming_fields.insert("edges".to_string(), edges_val);
        let naming_val = Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "MidSurfaceAttributes".to_string(),
            version: 1,
            fields: naming_fields,
        }));
        let mut outer_fields = PersistentMap::default();
        outer_fields.insert("naming".to_string(), naming_val);
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: "ShellExtractionResult".to_string(),
            version: 1,
            fields: outer_fields,
        }))
    }

    /// step-3: fold_mid_surface_attributes_into_table populates one
    /// MidSurfaceFace per face record and one MidSurfaceEdge per edge record,
    /// with correct feature_id/local_index and high bit set on all IDs.
    #[test]
    fn fold_helper_records_face_and_edge_entries_with_correct_attrs_and_synthetic_ids() {
        let feature_id = "synthetic/mid_surface";
        let face_records = [(feature_id, 0u32), (feature_id, 1u32)];
        let edges = [(feature_id, 0u32)];

        let value = make_result_value(&face_records, &edges);
        let mut table = TopologyAttributeTable::default();
        fold_mid_surface_attributes_into_table(&mut table, &value);

        // (1) One MidSurfaceFace per region
        let face_entries: Vec<_> = table
            .iter()
            .filter(|(_, a)| a.role == Role::MidSurfaceFace)
            .collect();
        assert_eq!(face_entries.len(), 2, "expected 2 MidSurfaceFace entries");
        for (id, attr) in &face_entries {
            assert_eq!(attr.feature_id.to_string(), feature_id);
            assert!(attr.user_label.is_none());
            assert!(attr.mod_history.is_empty());
            // High bit set
            assert_eq!(
                id.0 & 0x8000_0000_0000_0000,
                0x8000_0000_0000_0000,
                "MidSurfaceFace id {:#018x} must have high bit set",
                id.0
            );
        }
        // local_index values: {0, 1}
        let mut face_indices: Vec<u32> = face_entries.iter().map(|(_, a)| a.local_index).collect();
        face_indices.sort();
        assert_eq!(face_indices, vec![0, 1]);

        // (2) One MidSurfaceEdge
        let edge_entries: Vec<_> = table
            .iter()
            .filter(|(_, a)| a.role == Role::MidSurfaceEdge)
            .collect();
        assert_eq!(edge_entries.len(), 1, "expected 1 MidSurfaceEdge entry");
        let (edge_id, edge_attr) = &edge_entries[0];
        assert_eq!(edge_attr.local_index, 0);
        assert_eq!(edge_attr.role, Role::MidSurfaceEdge);
        assert_eq!(
            edge_id.0 & 0x8000_0000_0000_0000,
            0x8000_0000_0000_0000,
            "MidSurfaceEdge id must have high bit set"
        );

        // (3) All IDs are distinct
        let mut all_ids: Vec<u64> = table.iter().map(|(id, _)| id.0).collect();
        let total = all_ids.len();
        all_ids.sort();
        all_ids.dedup();
        assert_eq!(all_ids.len(), total, "all synthetic IDs must be distinct");

        // (4) Determinism: fold into a second table yields identical id→attr pairs
        let mut table2 = TopologyAttributeTable::default();
        fold_mid_surface_attributes_into_table(&mut table2, &value);
        let mut entries1: Vec<(u64, Role, u32, String)> = table
            .iter()
            .map(|(id, a)| (id.0, a.role, a.local_index, a.feature_id.to_string()))
            .collect();
        let mut entries2: Vec<(u64, Role, u32, String)> = table2
            .iter()
            .map(|(id, a)| (id.0, a.role, a.local_index, a.feature_id.to_string()))
            .collect();
        entries1.sort_by_key(|(id, _, _, _)| *id);
        entries2.sort_by_key(|(id, _, _, _)| *id);
        assert_eq!(entries1, entries2, "fold must be deterministic");
    }

    /// Verify synthetic_mid_surface_handle_id packs correctly:
    /// high bit set, role bit correct, local_index in low 32 bits.
    #[test]
    fn synthetic_handle_id_packs_bits_correctly() {
        let id_face = synthetic_mid_surface_handle_id("feat/mid_surface", false, 3);
        let id_edge = synthetic_mid_surface_handle_id("feat/mid_surface", true, 3);

        // High bit set
        assert_eq!(id_face.0 >> 63, 1, "high bit must be 1 (synthetic)");
        assert_eq!(id_edge.0 >> 63, 1);

        // Role bit (bit 32): face=0, edge=1
        assert_eq!((id_face.0 >> 32) & 1, 0, "face role bit must be 0");
        assert_eq!((id_edge.0 >> 32) & 1, 1, "edge role bit must be 1");

        // local_index in low 32 bits
        assert_eq!(id_face.0 & 0xFFFF_FFFF, 3);
        assert_eq!(id_edge.0 & 0xFFFF_FFFF, 3);

        // Face and edge with same feature_id/local_index are distinct
        assert_ne!(id_face.0, id_edge.0);

        // Different feature_ids produce different IDs
        let id_other = synthetic_mid_surface_handle_id("other/mid_surface", false, 3);
        assert_ne!(id_face.0, id_other.0);
    }
}
