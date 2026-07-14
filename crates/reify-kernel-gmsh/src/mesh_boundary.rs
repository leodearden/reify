//! NodeAttachment producer — B-rep attribution threading through the
//! surface→volume meshing path.
//!
//! Implements task γ (M-005) from PRD
//! `docs/prds/v0_3/mesh-morphing-phase-2.md` §3.3: emit a
//! [`BoundaryAssociation`] alongside the produced [`VolumeMesh`], threading
//! caller-supplied per-input-vertex B-rep attribution through the HXT meshing
//! path.
//!
//! # Design
//!
//! All types and functions in this module are feature-gated behind
//! `#[cfg(feature = "mesh-morph")]` (applied at the `pub mod mesh_boundary`
//! declaration in `lib.rs`). The `#[cfg(has_gmsh)]`-gated orchestrating
//! function `mesh_surface_to_volume_with_attribution` is additionally gated on
//! `has_gmsh` because it calls `mesh_surface_to_volume_with_diagnostics` which
//! only exists in the real FFI build.

use reify_ir::{BoundaryAssociation, GeometryError, GeometryHandleId, Mesh, NodeAttachment, VolumeConnectivity, VolumeMesh};
// `MeshInvariant` / `MeshViolationCounts` / `MeshWitness` compose the
// #4876 watertightness preflight's `GeometryError::MeshContractViolation`.
// Not re-exported at the `reify_ir` crate root (see
// `reify_ir::lib::pub use geometry::{..}`), so they are imported from their
// defining submodule, mirroring the `reify_ir::geometry::` qualified-path
// precedent used elsewhere in the workspace (e.g.
// `crates/reify-eval/tests/result_carried_topology.rs`).
use reify_ir::geometry::{MeshInvariant, MeshViolationCounts, MeshWitness};

// `ElementOrderTag` is only referenced inside `#[cfg(has_gmsh)]` functions
// (`mesh_surface_to_volume_with_attribution`, `run_meshing_with_entity_queries`).
// `VolumeMesh` is moved to the unconditional import above: the struct field
// `BoundaryAttributedReport::volume: VolumeMesh` must resolve in every build
// mode (matches the `MeshSurfaceToVolumeReport` precedent in mesh_volume.rs).
#[cfg(has_gmsh)]
use reify_ir::ElementOrderTag;
#[cfg(has_gmsh)]
use std::borrow::Cow;
#[cfg(has_gmsh)]
use crate::{
    auto_size::AutoSizeConfig,
    mesh_volume::{compute_thickness_warnings, resolve_mesh_size},
    options::MeshingOptions,
    repair::{RepairConfig, repair_surface_mesh_with_correspondence},
    through_thickness::{ThroughThicknessConfig, ThroughThicknessWarning},
};

// ---------------------------------------------------------------------------
// Per-B-rep-entity attribution input type
// ---------------------------------------------------------------------------

/// Per-B-rep-entity attribution input for the gmsh volume-meshing producer.
///
/// After `classify_surfaces` + `create_geometry`, gmsh assigns each output mesh
/// node to a B-rep entity of dimension 0 (vertex), 1 (edge), or 2 (face).
/// Entities are identified by `(dim, tag)` inside gmsh — the caller cannot know
/// the tags in advance. Instead the caller provides an **anchor position** for
/// each of its B-rep entities (e.g. face centroid from OCCT). The producer
/// computes an anchor for each gmsh entity (average of its node positions) and
/// matches by nearest-anchor within `match_tolerance`.
///
/// # Tolerances
///
/// `match_tolerance` is an absolute Euclidean distance on the anchor positions
/// (f64). A value of ~10 % of the smallest geometric feature is generally safe.
/// `0.0` disables all matching (results in an empty `BoundaryAssociation`).
///
/// # Handle uniqueness
///
/// Each `GeometryHandleId` in `faces`, `edges`, and `vertices` should be
/// unique within its slice. Duplicate handles are permitted (they associate
/// multiple anchors with the same handle) but the behaviour may be surprising
/// if the anchor-matching produces duplicate mappings.
#[derive(Debug, Clone)]
pub struct EntityAttribution {
    /// Face B-rep entities: `(caller_handle, anchor_position)`.
    /// Anchor is typically the face centroid from the OCCT B-rep.
    pub faces: Vec<(GeometryHandleId, [f64; 3])>,

    /// Edge B-rep entities: `(caller_handle, anchor_position)`.
    /// Anchor is typically the edge midpoint from the OCCT B-rep.
    pub edges: Vec<(GeometryHandleId, [f64; 3])>,

    /// Vertex B-rep entities: `(caller_handle, anchor_position)`.
    /// Anchor is the vertex position from the OCCT B-rep.
    pub vertices: Vec<(GeometryHandleId, [f64; 3])>,

    /// Absolute Euclidean-distance tolerance for anchor matching.
    ///
    /// Nearest-anchor matching is guaranteed unambiguous when
    /// `match_tolerance < suggested_match_tolerance()` — i.e., strictly less
    /// than half the minimum same-dim pairwise anchor spacing (a sufficient
    /// condition; values above the bound do not necessarily cause mis-assignment
    /// for a given query set).  The producer matches each gmsh entity only
    /// against same-dimension caller anchors (dim-2 against `faces`, dim-1
    /// against `edges`, dim-0 against `vertices`), so cross-dim anchor
    /// distances are irrelevant for ambiguity analysis.
    ///
    /// Use [`EntityAttribution::suggested_match_tolerance`] to derive a
    /// principled safe bound from the anchor geometry; choose any value in
    /// `(0.0, suggested_match_tolerance())`.
    ///
    /// `0.0` disables all matching (results in an empty
    /// [`BoundaryAssociation`]).
    pub match_tolerance: f64,
}

impl EntityAttribution {
    /// Derive a safe upper bound for `match_tolerance` from the anchor
    /// geometry.
    ///
    /// Returns `0.5 × min_same_dim_pairwise_distance`, where the minimum is
    /// taken independently within each dimension (faces, edges, vertices) and
    /// then the overall minimum is used.  Nearest-anchor matching is guaranteed
    /// unambiguous when `match_tolerance < suggested_match_tolerance()` (a
    /// sufficient condition via the triangle inequality): any query within
    /// tolerance of one anchor is guaranteed to be farther from all other
    /// same-dim anchors.
    ///
    /// Returns [`f64::INFINITY`] when no dimension has ≥ 2 anchors (no
    /// same-dim ambiguity is possible regardless of tolerance).
    ///
    /// Cross-dim spacing is deliberately excluded: the producer matches each
    /// gmsh entity only against the caller anchors for the **same** dimension,
    /// so face–edge or face–vertex distances cannot cause mis-assignment.
    ///
    /// # Example
    ///
    /// For a unit cube (side 1.0, centred at origin) the adjacent face-centre
    /// distance is √0.5 ≈ 0.707, giving `suggested_match_tolerance` ≈ 0.354.
    /// The hand-picked `match_tolerance = 0.3` used in the integration tests
    /// is safely below this bound.
    pub fn suggested_match_tolerance(&self) -> f64 {
        let slices: [&[(_, [f64; 3])]; 3] =
            [self.faces.as_slice(), self.edges.as_slice(), self.vertices.as_slice()];
        let mut global_min_sq = f64::INFINITY;
        for anchors in slices {
            let n = anchors.len();
            if n < 2 {
                continue;
            }
            for i in 0..n {
                for j in (i + 1)..n {
                    let a = anchors[i].1;
                    let b = anchors[j].1;
                    let dx = a[0] - b[0];
                    let dy = a[1] - b[1];
                    let dz = a[2] - b[2];
                    let d2 = dx * dx + dy * dy + dz * dz;
                    if d2 < global_min_sq {
                        global_min_sq = d2;
                    }
                }
            }
        }
        if global_min_sq.is_infinite() {
            f64::INFINITY
        } else {
            0.5 * global_min_sq.sqrt()
        }
    }
}

// ---------------------------------------------------------------------------
// Output type: BoundaryAttributedReport
// ---------------------------------------------------------------------------

/// Output of [`mesh_surface_to_volume_with_attribution`].
///
/// Bundles the produced volume mesh with the B-rep boundary attribution and
/// any through-thickness under-resolution warnings.
#[derive(Debug, Clone)]
pub struct BoundaryAttributedReport {
    /// The produced volume mesh (tetrahedral).
    pub volume: VolumeMesh,

    /// Through-thickness under-resolution warnings from the post-stage.
    /// Empty when `thickness_cfg = None` or when no under-resolved regions
    /// were found.
    #[cfg(has_gmsh)]
    pub through_thickness_warnings: Vec<ThroughThicknessWarning>,

    /// Per-node B-rep entity attribution for surface nodes of the volume mesh.
    /// Interior tet nodes (not on any B-rep entity) are absent.
    /// Keys are 0-based local indices into `volume.vertices`.
    pub boundary: BoundaryAssociation,
}

// ---------------------------------------------------------------------------
// Watertightness preflight (task #4876, PRD kernel-seam-contracts.md §9
// NEAR-TERM preflight leaf; INV-GEO-1)
// ---------------------------------------------------------------------------

/// Preflight the watertightness of `surface` before it is handed to the
/// gmsh attributed producer ([`mesh_surface_to_volume_with_attribution`]).
///
/// This is a generic, pure watertightness check with no opinion on whether
/// `surface` has been welded — the unit tests below exercise it directly on
/// both an unwelded per-face-block fixture and a welded fixture. Its sole
/// production caller, [`mesh_surface_to_volume_with_attribution`], calls this
/// TWICE in the general case (task ξ, #5116): first as a cheap probe on the
/// RAW surface, but ONLY when the caller passed no explicit `repair_cfg` —
/// an `Ok` here short-circuits the weld entirely, since an already-watertight
/// surface needs no repair — and, only when that probe fails (or is skipped
/// because `repair_cfg` was `Some`), again on the WELDED result (via
/// `repair_surface_mesh_with_correspondence`) as a fail-closed net (see that
/// function's `# Stage order`). So in practice this now fails closed in two
/// distinct scenarios rather than one:
///
/// - The [`Mesh::validate`] check below still catches a GENUINE hole (e.g. a
///   face missing from the B-rep) that welding cannot fix — welding only
///   merges near-coincident vertex positions, it cannot fabricate a missing
///   triangle.
/// - The [`Mesh::weldedness`] check below is a fail-safe for a weld that did
///   not fully collapse near-duplicate positions (e.g. a caller-supplied
///   `repair_cfg` with a merge epsilon too tight for the input's duplicate
///   spacing). The default `RepairConfig` collapses OCCT's bit-identical
///   per-face duplicates, so this branch should not fire for a correctly
///   configured weld, but it remains load-bearing defense-in-depth.
///
/// # Why weldedness, not just `validate`
///
/// OCCT emits per-face vertex blocks (unwelded by design,
/// `occt_wrapper.cpp:5847`): every corner shared by several faces is
/// duplicated once per incident face. Such a surface is a fully valid,
/// closed, consistently-wound 2-manifold on its POSITION-WELDED QUOTIENT
/// topology, so [`Mesh::validate`] alone happily returns `Ok` on the RAW,
/// unwelded index buffer — which is genuinely non-watertight (an open edge
/// at every shared face-perimeter edge) and SIGSEGVs inside gmsh's FFI if
/// handed in directly, a failure mode the caller's `Result`-based
/// honest-degradation fallback cannot catch. `validate` alone cannot tell a
/// truly closed surface apart from one that is only closed on its welded
/// quotient, which is exactly why this preflight adds the weldedness check.
///
/// This is the `docs/prds/kernel-seam-contracts.md` §4 site-3 fail-closed
/// exception: rather than let an uncatchable crash happen, this preflight
/// returns `Err` up front so the caller degrades to the plain
/// (non-attributed) producer instead.
///
/// # Checks (in order)
///
/// 1. [`Mesh::validate`] — the producer obligations (finite coordinates,
///    in-bounds indices, non-degenerate triangles, closed + consistently
///    wound on the welded quotient), bridged to a
///    [`GeometryError::MeshContractViolation`] naming kernel `"gmsh"` via
///    [`reify_ir::geometry::MeshContractViolation::into_geometry_error`].
/// 2. [`Mesh::weldedness`] — the CONSUMER-CAPABILITY axis: is the raw index
///    buffer already welded? If not, a raw (unwelded) directed-edge scan
///    ([`raw_open_edge_census`]) computes a truthful open-edge count and a
///    first-open-edge witness, and this synthesizes a
///    `MeshContractViolation` naming [`MeshInvariant::Closed`] — there is no
///    dedicated "unwelded" variant; the raw surface genuinely has open
///    boundary edges, so `Closed` is the honest obligation to report.
///
/// `tol` is forwarded to both `validate` and `weldedness` unchanged.
pub fn preflight_watertight_surface(surface: &Mesh, tol: f64) -> Result<(), GeometryError> {
    surface
        .validate(tol)
        .map_err(|violation| violation.into_geometry_error("gmsh"))?;

    if !surface.weldedness(tol).raw_welded {
        let (open_edges, (u, v)) = raw_open_edge_census(surface);
        return Err(GeometryError::MeshContractViolation {
            kernel: "gmsh",
            invariant: MeshInvariant::Closed,
            counts: MeshViolationCounts {
                open_edges,
                ..Default::default()
            },
            witness: MeshWitness::Edge { u, v },
        });
    }

    Ok(())
}

/// Count RAW (pre-weld) directed edges whose reverse is absent, and return a
/// first-offender witness `(u, v)` in triangle-emission order (deterministic,
/// independent of the internal `HashSet`'s iteration order).
///
/// `pub` (not crate-private): this is the SINGLE shared implementation of
/// the raw directed-edge scan. Both [`preflight_watertight_surface`] (above)
/// and the #4876/#5115 characterization test
/// (`crates/reify-eval/tests/fea_face_selector_bc_e2e.rs::characterizes_4876_occt_tessellation_unwelded_witness`)
/// call this same function — the latter as
/// `reify_kernel_gmsh::mesh_boundary::raw_open_edge_census`, reachable
/// because `reify-eval`'s dev-dependency on this crate enables the
/// `mesh-morph` feature this module is gated on — rather than each
/// maintaining its own copy of the `HashSet` directed-edge scan, which could
/// silently diverge.
///
/// The witness defaults to `(u32::MAX, u32::MAX)` (mirroring the dangling-tail
/// sentinel in `reify_ir::geometry::Mesh::check_contract`) in the degenerate
/// case where `open_edges == 0` — every triangle-referenced edge already has
/// its reverse, so there is no offending edge to name even though the caller
/// observed `weldedness(tol).raw_welded == false` (e.g. an unreferenced
/// duplicate-position vertex outside `indices`; pinned by
/// `preflight_rejects_unreferenced_duplicate_vertex_with_zero_open_edges`
/// below). Preferred over panicking: this helper backs a fail-closed
/// preflight whose entire purpose is avoiding an uncatchable crash.
pub fn raw_open_edge_census(surface: &Mesh) -> (usize, (u32, u32)) {
    use std::collections::HashSet;

    let mut directed: HashSet<(u32, u32)> = HashSet::new();
    for tri in surface.indices.chunks_exact(3) {
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        for &(u, v) in &[(a, b), (b, c), (c, a)] {
            directed.insert((u, v));
        }
    }

    let open_edges = directed
        .iter()
        .filter(|&&(u, v)| !directed.contains(&(v, u)))
        .count();

    let witness = surface
        .indices
        .chunks_exact(3)
        .find_map(|tri| {
            let (a, b, c) = (tri[0], tri[1], tri[2]);
            [(a, b), (b, c), (c, a)]
                .into_iter()
                .find(|&(u, v)| !directed.contains(&(v, u)))
        })
        .unwrap_or((u32::MAX, u32::MAX));

    (open_edges, witness)
}

// ---------------------------------------------------------------------------
// cfg(has_gmsh): mesh_surface_to_volume_with_attribution
// ---------------------------------------------------------------------------

/// Mesh a closed triangulated surface to a tetrahedral volume mesh, and
/// emit a [`BoundaryAssociation`] that maps each surface output node to its
/// B-rep entity attribution (face / edge / vertex).
///
/// **Attribution model:** gmsh's `classify_surfaces` + `create_geometry`
/// pipeline re-meshes from scratch; input vertex identities are NOT preserved
/// in the output (see `crates/reify-kernel-gmsh/tests/gmsh_classify_diagnostics.rs`).
/// Attribution is therefore derived from gmsh entity membership: after
/// `mesh_generate(3)`, the producer queries which B-rep entity each output
/// node belongs to (`ffi::get_nodes_at_entity`) and matches those entities to
/// caller-provided OCCT handles via nearest-anchor matching within
/// `attribution.match_tolerance`.
///
/// # Repair / welding (task ξ, #5116)
///
/// The surface is welded before being handed to gmsh whenever it is not
/// already watertight, via
/// `repair_surface_mesh_with_correspondence(surface, repair_cfg.unwrap_or_default())`
/// — `repair_cfg` selects the weld/repair configuration (`None` uses
/// `RepairConfig::default()`) rather than gating whether welding can happen
/// at all, as it did pre-ξ. When `repair_cfg` is `None`, a cheap
/// `preflight_watertight_surface` probe on the RAW surface decides whether
/// the weld's O(n²) vertex-merge scan (repair.rs) is worth paying for: an
/// already-watertight surface (e.g. a hand-built, pre-welded fixture) is
/// used as-is, since welding it with the default config would be a
/// bit-identical no-op anyway. An explicit `Some(repair_cfg)` always runs
/// the weld, even on an already-watertight raw surface: the caller asked for
/// a specific configuration (e.g. a coarser merge epsilon meant to always
/// apply), and the RAW-surface probe is a purely topological check that
/// cannot know whether a non-default config would still change the output —
/// so an explicit config is never silently dropped just because the surface
/// already happens to be closed. This is safe for attribution: entity
/// anchors are POSITIONS (mean node position / face centroid), and
/// position-welding only merges
/// near-coincident vertices onto a shared position — surviving vertices keep
/// their exact original coordinates, so no anchor moves and per-node
/// attribution (derived from gmsh entity membership, not input-vertex
/// identity — see above) is unaffected. The correspondence map returned
/// alongside the welded mesh makes the weld non-lossy for input-node
/// identity; it is consumed here via a `tracing::debug!` weld-stats event
/// (mirroring `mesh_volume.rs::apply_repair_if_requested`'s pattern) rather
/// than threaded further, since gmsh's re-meshing already discards raw input
/// node identity regardless of welding.
///
/// # Stage order
///
/// 1. Watertightness preflight (task #4876) probe on the RAW surface —
///    skipped entirely when the caller passed an explicit `repair_cfg`. An
///    `Ok` here (only possible when `repair_cfg` is `None`) means the
///    surface is already watertight, so it is used as-is and stage 2's weld
///    is skipped.
/// 2. Weld — `repair_surface_mesh_with_correspondence`, always run when
///    `repair_cfg` is `Some`, or when `repair_cfg` is `None` and stage 1
///    returned `Err`; re-runs the preflight on the WELDED result as a
///    fail-closed net, so it fails closed only for genuinely-open input (a
///    real hole survives welding).
/// 3. `resolve_mesh_size` — honours caller override or derives from features.
/// 4. GMesh pipeline — classify + create_geometry + HXT tet meshing.
/// 5. Entity-membership queries (`ffi::get_nodes_at_entity`).
/// 6. Nearest-anchor matching → builds `BoundaryAssociation`.
/// 7. `compute_thickness_warnings` post-stage.
#[cfg(has_gmsh)]
pub fn mesh_surface_to_volume_with_attribution(
    surface: &Mesh,
    options: &MeshingOptions,
    order: ElementOrderTag,
    repair_cfg: Option<RepairConfig>,
    auto_size_cfg: Option<AutoSizeConfig>,
    thickness_cfg: Option<ThroughThicknessConfig>,
    attribution: &EntityAttribution,
) -> Result<BoundaryAttributedReport, GeometryError> {
    // --- Weld (task ξ, #5116), short-circuited when already watertight AND
    // no explicit repair_cfg was requested ---
    //
    // gmsh requires watertight input; a real OCCT-tessellated surface is
    // unwelded by design (per-face vertex blocks) and previously SIGSEGVd
    // inside gmsh's FFI if handed straight in. When `repair_cfg` is `None`,
    // probe the RAW surface with the #4876 preflight first, and only pay
    // for the weld's O(n²) vertex-merge scan (repair.rs) when that probe
    // fails: an already-watertight surface (e.g. a hand-built, pre-welded
    // fixture) needs no repair, and welding it with the default config
    // would be a bit-identical no-op anyway (see fn doc) — so skipping it
    // changes no observable behavior, only cost. An explicit
    // `Some(repair_cfg)` always runs the weld, bypassing the probe: the
    // preflight is a purely topological check, so it cannot know whether a
    // caller-supplied (e.g. non-default-epsilon) config would still change
    // the output, and a caller that explicitly asked for a repair
    // configuration must never have it silently dropped (see fn doc). `Cow`
    // avoids cloning the surface in the common already-watertight,
    // no-explicit-cfg case, mirroring
    // `mesh_volume.rs::apply_repair_if_requested`.
    //
    // When welding IS needed, position-based entity anchors (see fn doc)
    // make it safe for attribution. `correspondence` maps each ORIGINAL
    // vertex index to its compacted post-weld index (`u32::MAX` if its
    // merge-survivor was compacted away); gmsh re-meshes from scratch and
    // discards input-node identity regardless of welding, so
    // `correspondence` is not threaded any further here — it is consumed
    // via a weld-stats debug event (its `dropped_verts` field is computed
    // lazily, inline, so the O(n) scan only runs when DEBUG is enabled) so
    // it stays genuinely used rather than dead.
    let welded: Cow<'_, Mesh> = if repair_cfg.is_none()
        && preflight_watertight_surface(surface, 0.0).is_ok()
    {
        Cow::Borrowed(surface)
    } else {
        let (w, correspondence) =
            repair_surface_mesh_with_correspondence(surface, repair_cfg.unwrap_or_default());
        tracing::debug!(
            target: "reify_kernel_gmsh::mesh_boundary",
            original_verts = correspondence.len(),
            welded_verts = w.vertices.len() / 3,
            dropped_verts = correspondence.iter().filter(|&&c| c == u32::MAX).count(),
            "mesh_surface_to_volume_with_attribution: weld pre-stage applied"
        );

        // Fail-closed net (task #4876): evaluated on the WELDED surface, so
        // this rejects only a surface that is STILL non-watertight after
        // welding (a genuine hole, not just per-face-block duplication),
        // degrading gracefully to the plain producer instead of crashing.
        preflight_watertight_surface(&w, 0.0)?;
        Cow::Owned(w)
    };

    // --- Pre-stage: resolve mesh size ---
    let resolved = resolve_mesh_size(welded.as_ref(), options, auto_size_cfg)?;
    let inner_options = MeshingOptions { mesh_size: resolved, ..options.clone() };

    // --- GMesh pipeline + entity-membership queries ---
    let (volume, node_attribution) =
        run_meshing_with_entity_queries(welded.as_ref(), &inner_options, order, attribution)?;

    // --- Build BoundaryAssociation from entity-membership map ---
    let mut boundary = BoundaryAssociation::default();
    for (local_idx, attachment) in node_attribution {
        boundary.associate(local_idx, attachment);
    }

    // --- Post-stage: through-thickness warnings ---
    let through_thickness_warnings =
        compute_thickness_warnings(&volume, welded.as_ref(), thickness_cfg);

    Ok(BoundaryAttributedReport { volume, through_thickness_warnings, boundary })
}

// ---------------------------------------------------------------------------
// Internal helper: run the gmsh pipeline and return entity-attributed nodes
// ---------------------------------------------------------------------------

/// Run the full gmsh surface-to-volume meshing pipeline and return the
/// produced `VolumeMesh` together with a per-node attribution map built
/// from entity-membership queries executed before the model is cleared.
///
/// This is the engine of [`mesh_surface_to_volume_with_attribution`].  It
/// deliberately mirrors the structure of `kernel_real.rs::mesh_to_volume`
/// (acquiring `GMSH_LOCK`, calling the same FFI sequence) while adding the
/// entity-membership queries between `mesh_generate(3)` and `ffi::clear()`.
/// The existing `mesh_to_volume` function is left unchanged.
#[cfg(has_gmsh)]
fn run_meshing_with_entity_queries(
    surface: &Mesh,
    options: &MeshingOptions,
    element_order: ElementOrderTag,
    attribution: &EntityAttribution,
) -> Result<(VolumeMesh, Vec<(u32, NodeAttachment)>), GeometryError> {
    use std::collections::HashMap;
    use crate::{ffi, init};

    // --- Input validation (mirrors kernel_real.rs checks) ---
    let n_verts = surface.vertices.len() / 3;
    if let Some(&bad) = surface.indices.iter().find(|&&i| (i as usize) >= n_verts) {
        return Err(GeometryError::OperationFailed(format!(
            "mesh_surface_to_volume_with_attribution: surface.indices contains {bad}, \
             out of bounds for {n_verts}-vertex mesh"
        )));
    }
    if surface.vertices.is_empty() || surface.indices.is_empty() {
        return Err(GeometryError::OperationFailed(format!(
            "mesh_surface_to_volume_with_attribution: empty surface mesh \
             (vertices.len()={}, indices.len()={})",
            surface.vertices.len(),
            surface.indices.len()
        )));
    }

    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    ffi::clear()?;
    ffi::option_set_number("General.Terminal", 0.0)?;

    // Mesh-size options
    if let Some(s) = options.mesh_size
        && s > 0.0
    {
        ffi::option_set_number("Mesh.MeshSizeMin", s)?;
        ffi::option_set_number("Mesh.MeshSizeMax", s)?;
    }

    // Algorithm: HXT (3D code 10)
    ffi::option_set_number("Mesh.Algorithm3D", 10.0)?;

    // Thread count
    let num_threads: f64 = if options.deterministic {
        1.0
    } else {
        match options.threads {
            Some(t) => t as f64,
            None => std::thread::available_parallelism()
                .map(|n| n.get() as f64)
                .unwrap_or(1.0),
        }
    };
    ffi::option_set_number("General.NumThreads", num_threads)?;

    // Element order
    let order_value: f64 = match element_order {
        ElementOrderTag::P1 => 1.0,
        ElementOrderTag::P2 => 2.0,
    };
    ffi::option_set_number("Mesh.ElementOrder", order_value)?;

    ffi::model_add("reify_attribution_mesh")?;
    let surf_tag = ffi::add_discrete_entity(2, &[])?;

    // Push input nodes (tags 1..=N)
    let node_tags_in: Vec<u64> = (1..=n_verts as u64).collect();
    let coords_f64: Vec<f64> = surface.vertices.iter().map(|&v| v as f64).collect();
    ffi::add_nodes_2d(surf_tag, &node_tags_in, &coords_f64)?;

    // Push triangles
    let n_tris = surface.indices.len() / 3;
    let tri_tags: Vec<u64> = (1..=n_tris as u64).collect();
    let tri_node_tags: Vec<u64> = surface.indices.iter().map(|&i| i as u64 + 1).collect();
    ffi::add_elements_2d(surf_tag, 2, &tri_tags, &tri_node_tags)?;

    // Classify surfaces + create geometry.
    //
    // NOTE (task 3591, at time of writing): the feature angle is FRAC_PI_4
    // (45°), NOT the FRAC_PI_2 (90°) that `mesh_to_volume` uses. Volume meshing
    // only needs a closed watertight surface, but ATTRIBUTION additionally needs
    // gmsh to reconstruct the B-rep topology (faces / edges / corner points). A
    // cube's dihedral angle is exactly 90°, so a 90° feature angle fails to
    // separate adjacent faces: gmsh then emits a degenerate decomposition with
    // no corner (dim-0) entities, yielding zero OnVertex attributions. FRAC_PI_4
    // puts the cube's 90° edges safely above the threshold so corners, edges and
    // faces are recovered. See `tests/node_attachment_producer.rs` (signal test)
    // and `tests/gmsh_classify_diagnostics.rs` (pinned re-meshing property).
    ffi::classify_surfaces(
        std::f64::consts::FRAC_PI_4,
        1,
        1,
        std::f64::consts::FRAC_PI_4,
        0,
    )?;
    ffi::create_geometry(&[])?;

    // Wrap in surface loop + volume
    let surface_tags = ffi::get_entity_tags(2)?;
    if surface_tags.is_empty() {
        let _ = ffi::clear();
        return Err(GeometryError::OperationFailed(
            "gmsh produced no dim=2 entities after classify_surfaces+create_geometry".into(),
        ));
    }
    let loop_tag = ffi::geo_add_surface_loop(&surface_tags)?;
    let _vol_tag = ffi::geo_add_volume(&[loop_tag])?;
    ffi::geo_synchronize()?;

    ffi::mesh_generate(3)?;

    // -----------------------------------------------------------------------
    // Entity-membership queries (must happen BEFORE ffi::clear)
    // -----------------------------------------------------------------------

    // For each B-rep dimension, build (dim, entity_tag) → caller_handle map.
    // Each entity's "anchor" is the average of its mesh-node positions.
    let mut node_tag_to_attachment: HashMap<u64, NodeAttachment> = HashMap::new();
    let tol_sq = attribution.match_tolerance * attribution.match_tolerance;

    for dim in [0i32, 1, 2] {
        let entity_tags = ffi::get_entity_tags(dim)?;
        for entity_tag in entity_tags {
            // Get nodes belonging to this entity (includeBoundary=0)
            let (entity_node_tags, entity_coords) =
                ffi::get_nodes_at_entity(dim, entity_tag)?;
            if entity_node_tags.is_empty() {
                continue;
            }
            // Compute entity anchor = mean of node positions
            let n = entity_node_tags.len() as f64;
            let (sx, sy, sz) = entity_coords
                .chunks_exact(3)
                .fold((0.0f64, 0.0, 0.0), |(ax, ay, az), c| {
                    (ax + c[0], ay + c[1], az + c[2])
                });
            let anchor = [sx / n, sy / n, sz / n];

            // Match anchor to caller-provided list for this dim
            let caller_candidates = match dim {
                0 => attribution.vertices.as_slice(),
                1 => attribution.edges.as_slice(),
                2 => attribution.faces.as_slice(),
                _ => &[],
            };
            let matched = find_closest_anchor(anchor, caller_candidates, tol_sq);
            let Some(handle) = matched else { continue };

            let attachment = match dim {
                0 => NodeAttachment::OnVertex(handle),
                1 => NodeAttachment::OnEdge(handle),
                2 => NodeAttachment::OnFace(handle),
                _ => continue,
            };
            for tag in &entity_node_tags {
                node_tag_to_attachment.insert(*tag, attachment);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Read out the volume mesh (mirrors kernel_real.rs post-mesh readout)
    // -----------------------------------------------------------------------

    let (all_node_tags, coord_buf) = ffi::get_nodes_all()?;
    if coord_buf.len() != all_node_tags.len() * 3 {
        let _ = ffi::clear();
        return Err(GeometryError::OperationFailed(format!(
            "gmsh get_nodes_all stride mismatch: node_tags.len()={}, coord_buf.len()={}",
            all_node_tags.len(),
            coord_buf.len()
        )));
    }

    let elem_type = match element_order {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 11,
    };
    let (_elem_tags, elem_node_tags) = ffi::get_elements_by_type(elem_type)?;
    let nodes_per_elem: usize = match element_order {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 10,
    };
    if !elem_node_tags.len().is_multiple_of(nodes_per_elem) {
        let _ = ffi::clear();
        return Err(GeometryError::OperationFailed(format!(
            "gmsh element stride mismatch: elem_node_tags.len()={} not multiple of {nodes_per_elem}",
            elem_node_tags.len()
        )));
    }

    // Sort by tag → assign local indices
    let mut paired: Vec<(u64, [f64; 3])> = all_node_tags
        .iter()
        .copied()
        .zip(coord_buf.chunks_exact(3))
        .map(|(t, c)| (t, [c[0], c[1], c[2]]))
        .collect();
    paired.sort_by_key(|(t, _)| *t);

    let mut tag_to_idx: HashMap<u64, u32> = HashMap::with_capacity(paired.len());
    let mut vertices: Vec<f32> = Vec::with_capacity(paired.len() * 3);
    for (idx, (tag, xyz)) in paired.iter().enumerate() {
        let idx_u32 = u32::try_from(idx).map_err(|_| {
            GeometryError::OperationFailed(format!(
                "mesh has {} nodes, exceeding u32 limit",
                paired.len()
            ))
        })?;
        tag_to_idx.insert(*tag, idx_u32);
        vertices.extend(xyz.iter().map(|&v| v as f32));
    }

    // Remap connectivity
    let mut tet_indices: Vec<u32> = Vec::with_capacity(elem_node_tags.len());
    for &tag in &elem_node_tags {
        let idx = *tag_to_idx.get(&tag).ok_or_else(|| {
            GeometryError::OperationFailed(format!(
                "element references unknown node tag {tag}"
            ))
        })?;
        tet_indices.push(idx);
    }

    // Build node_tag → local_idx based attribution list
    let node_attribution: Vec<(u32, NodeAttachment)> = node_tag_to_attachment
        .iter()
        .filter_map(|(tag, attachment)| {
            tag_to_idx.get(tag).map(|&idx| (idx, *attachment))
        })
        .collect();

    let _ = ffi::clear();

    Ok((
        VolumeMesh {
            vertices,
            connectivity: VolumeConnectivity::Tet {
                indices: tet_indices,
                order: element_order,
            },
            normals: None,
            boundary: None,
        },
        node_attribution,
    ))
}

// ---------------------------------------------------------------------------
// Internal helper: nearest-anchor matching
// ---------------------------------------------------------------------------

/// Find the handle in `candidates` whose anchor is closest to `query_anchor`
/// (Euclidean squared distance). Returns `None` if `candidates` is empty or
/// the nearest distance exceeds `tol_sq`.
///
/// # Fail-closed NaN policy (INV-FEA-3)
///
/// Candidates whose squared distance to `query_anchor` is non-finite (NaN or
/// ±Inf) are excluded from matching — a non-finite distance can never be the
/// nearest match, so it is dropped rather than treated as an equally-near
/// tie. When one or more candidates are excluded this way, exactly one WARN
/// event is emitted at `reify_kernel_gmsh::mesh_boundary` (filterable via
/// `RUST_LOG=reify_kernel_gmsh::mesh_boundary=warn`) so upstream mesh
/// pathology in anchor coordinates surfaces to operators instead of being
/// silently dropped.
fn find_closest_anchor(
    query_anchor: [f64; 3],
    candidates: &[(GeometryHandleId, [f64; 3])],
    tol_sq: f64,
) -> Option<GeometryHandleId> {
    if tol_sq <= 0.0 {
        return None;
    }
    // Single pass: track the running-nearest finite candidate and how many
    // non-finite distances were seen, so the common case (all-finite, the
    // vast majority of calls — this runs once per entity per dimension in
    // the mesh-readout loop above) stays allocation-free.
    let mut best: Option<(f64, GeometryHandleId)> = None;
    let mut n_excluded: usize = 0;
    for &(handle, anchor) in candidates {
        let dx = query_anchor[0] - anchor[0];
        let dy = query_anchor[1] - anchor[1];
        let dz = query_anchor[2] - anchor[2];
        let d2 = dx * dx + dy * dy + dz * dz;
        if !d2.is_finite() {
            n_excluded += 1;
            continue;
        }
        if d2 < tol_sq && best.is_none_or(|(best_d2, _)| d2 < best_d2) {
            best = Some((d2, handle));
        }
    }

    if n_excluded > 0 {
        // Distinguish "the query anchor itself is non-finite" (every
        // candidate distance is poisoned by the same bad input) from "one or
        // more candidate anchors are non-finite" — otherwise `n_candidates`
        // reads as "many pathological candidates" when the real fault is a
        // single bad query anchor (e.g. an entity anchor computed from
        // corrupt mesh-node coordinates). `n_excluded` additionally reports
        // how many candidates were actually dropped (as opposed to
        // `n_candidates`, the total considered), so a single pathological
        // anchor among many healthy ones doesn't read as widespread
        // corruption.
        let query_finite = query_anchor.iter().all(|v| v.is_finite());
        tracing::warn!(
            target: "reify_kernel_gmsh::mesh_boundary",
            reason = "non_finite_anchor_distance",
            n_candidates = candidates.len(),
            n_excluded,
            query_finite,
            "find_closest_anchor: excluding candidate(s) with non-finite squared distance \
             (likely upstream pathology in mesh anchor coordinates); a non-finite distance \
             can never be the nearest match"
        );
    }

    best.map(|(_, handle)| handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    /// Builds two candidates — one at `[coord, 0.0, 0.0]` (non-finite when
    /// `coord` is NaN or infinite) and one at `[0.1, 0.0, 0.0]` (a legitimate
    /// finite match within `tol_sq = 1.0`) — and asserts that
    /// `find_closest_anchor` (a) still returns the finite match (the
    /// non-finite candidate is excluded, never treated as an equally-near
    /// tie) and (b) emits exactly one WARN at
    /// `reify_kernel_gmsh::mesh_boundary` documenting the exclusion.
    fn assert_non_finite_candidate_excluded_and_warns(coord: f64) {
        // Prime the callsite cache so per-test with_default subscribers see
        // events even if a prior test thread hit the callsite with no
        // subscriber active.
        reify_test_support::prime_tracing_callsite_cache();

        let query = [0.0, 0.0, 0.0];
        let candidates = vec![
            (GeometryHandleId(1), [coord, 0.0, 0.0]),
            (GeometryHandleId(2), [0.1, 0.0, 0.0]),
        ];
        let tol_sq = 1.0;

        let (subscriber, counters) = reify_test_support::CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_kernel_gmsh::mesh_boundary")
            .build();
        let warn = Arc::clone(&counters[&tracing::Level::WARN]);

        let matched = tracing::subscriber::with_default(subscriber, || {
            find_closest_anchor(query, &candidates, tol_sq)
        });

        // (a) The non-finite candidate must be excluded, never treated as an
        // equally-near tie — the finite candidate must still win.
        assert_eq!(
            matched,
            Some(GeometryHandleId(2)),
            "non-finite candidate (coord={coord}) must be excluded, not treated as an \
             equally-near tie; expected the finite candidate GeometryHandleId(2) to win"
        );

        // (b) Exactly one WARN event must be emitted at the named target.
        let warn_count = warn.load(Ordering::Acquire);
        assert_eq!(
            warn_count, 1,
            "expected exactly 1 WARN event at reify_kernel_gmsh::mesh_boundary \
             (coord={coord}); got {warn_count}"
        );
    }

    #[test]
    fn nan_candidate_excluded_and_warns() {
        assert_non_finite_candidate_excluded_and_warns(f64::NAN);
    }

    #[test]
    fn inf_candidate_excluded_and_warns() {
        assert_non_finite_candidate_excluded_and_warns(f64::INFINITY);
    }

    /// Negative control: two finite candidates must produce no WARN at all
    /// (guards against spurious warnings on healthy input) and must select
    /// the nearer handle.
    #[test]
    fn all_finite_candidates_emit_no_warn() {
        reify_test_support::prime_tracing_callsite_cache();

        let query = [0.0, 0.0, 0.0];
        let candidates = vec![
            (GeometryHandleId(1), [0.1, 0.0, 0.0]),
            (GeometryHandleId(2), [0.9, 0.0, 0.0]),
        ];
        let tol_sq = 1.0;

        let (subscriber, counters) = reify_test_support::CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_kernel_gmsh::mesh_boundary")
            .build();
        let warn = Arc::clone(&counters[&tracing::Level::WARN]);

        let matched = tracing::subscriber::with_default(subscriber, || {
            find_closest_anchor(query, &candidates, tol_sq)
        });

        assert_eq!(matched, Some(GeometryHandleId(1)));

        let warn_count = warn.load(Ordering::Acquire);
        assert_eq!(
            warn_count, 0,
            "healthy finite input must not emit any WARN; got {warn_count}"
        );
    }

    /// The `query_anchor` itself can be non-finite (e.g. an entity anchor
    /// computed as the mean of corrupt mesh-node coordinates). Every
    /// candidate distance is then poisoned by the same bad input — so no
    /// candidate can be finite — and the function must return `None` (fail
    /// closed; there is no finite candidate left to pick) while still
    /// emitting exactly one WARN with `query_finite = false`, distinguishing
    /// a bad query anchor from a bad candidate anchor.
    #[test]
    fn non_finite_query_anchor_returns_none_and_warns() {
        reify_test_support::prime_tracing_callsite_cache();

        let query = [f64::NAN, 0.0, 0.0];
        let candidates = vec![
            (GeometryHandleId(1), [0.1, 0.0, 0.0]),
            (GeometryHandleId(2), [0.2, 0.0, 0.0]),
        ];
        let tol_sq = 1.0;

        let (subscriber, capture) = reify_test_support::CapturingSubscriberBuilder::new(
            tracing::Level::WARN,
        )
        .target_prefix("reify_kernel_gmsh::mesh_boundary")
        .build();

        let matched = tracing::subscriber::with_default(subscriber, || {
            find_closest_anchor(query, &candidates, tol_sq)
        });

        assert_eq!(
            matched, None,
            "a non-finite query anchor poisons every candidate distance; with no \
             finite candidate left to match, the result must be None"
        );

        assert_eq!(
            capture.count(),
            1,
            "expected exactly one WARN event; got {:?}",
            capture.messages()
        );
        let fields = capture.fields_by_event();
        assert!(
            fields.iter().any(|f| {
                f.get("query_finite").map(String::as_str) == Some("false")
                    && f.get("n_candidates").map(String::as_str) == Some("2")
                    && f.get("n_excluded").map(String::as_str) == Some("2")
            }),
            "expected a WARN event with query_finite=false, n_candidates=2, n_excluded=2 \
             (both candidates poisoned by the non-finite query); got {fields:?}"
        );
    }

    /// Fail-closed boundary: when the *only* candidate(s) available are
    /// non-finite — no finite fallback at all — `best` must stay `None` for
    /// the whole call, so the function returns `None` rather than ever
    /// selecting a non-finite distance, while still emitting exactly one
    /// WARN.
    #[test]
    fn all_candidates_non_finite_returns_none_and_warns() {
        reify_test_support::prime_tracing_callsite_cache();

        let query = [0.0, 0.0, 0.0];
        let candidates = vec![(GeometryHandleId(1), [f64::NAN, 0.0, 0.0])];
        let tol_sq = 1.0;

        let (subscriber, counters) = reify_test_support::CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_kernel_gmsh::mesh_boundary")
            .build();
        let warn = Arc::clone(&counters[&tracing::Level::WARN]);

        let matched = tracing::subscriber::with_default(subscriber, || {
            find_closest_anchor(query, &candidates, tol_sq)
        });

        assert_eq!(
            matched, None,
            "with no finite candidate available, find_closest_anchor must return None \
             rather than selecting a non-finite distance"
        );

        let warn_count = warn.load(Ordering::Acquire);
        assert_eq!(
            warn_count, 1,
            "expected exactly 1 WARN event when the only candidate is non-finite; \
             got {warn_count}"
        );
    }

    /// `n_excluded` must report only the number of candidates actually
    /// dropped, not the total candidate count (`n_candidates`) — otherwise a
    /// single pathological anchor among many healthy ones would read as
    /// widespread corruption.
    #[test]
    fn n_excluded_counts_only_non_finite_candidates() {
        reify_test_support::prime_tracing_callsite_cache();

        let query = [0.0, 0.0, 0.0];
        let candidates = vec![
            (GeometryHandleId(1), [f64::NAN, 0.0, 0.0]),
            (GeometryHandleId(2), [0.1, 0.0, 0.0]),
            (GeometryHandleId(3), [0.2, 0.0, 0.0]),
        ];
        let tol_sq = 1.0;

        let (subscriber, capture) = reify_test_support::CapturingSubscriberBuilder::new(
            tracing::Level::WARN,
        )
        .target_prefix("reify_kernel_gmsh::mesh_boundary")
        .build();

        let matched = tracing::subscriber::with_default(subscriber, || {
            find_closest_anchor(query, &candidates, tol_sq)
        });

        assert_eq!(
            matched,
            Some(GeometryHandleId(2)),
            "the nearer of the two finite candidates must still win despite the \
             one non-finite candidate"
        );

        let fields = capture.fields_by_event();
        assert!(
            fields.iter().any(|f| {
                f.get("n_candidates").map(String::as_str) == Some("3")
                    && f.get("n_excluded").map(String::as_str) == Some("1")
            }),
            "expected a WARN event with n_candidates=3 but n_excluded=1 (only the \
             single non-finite candidate, not the whole candidate set); got {fields:?}"
        );
    }

    // ── Watertightness preflight (task #4876, PRD kernel-seam-contracts.md
    //    §9 NEAR-TERM preflight leaf; INV-GEO-1) ─────────────────────────────
    //
    // These fixtures hand-rebuild the shapes of the private
    // `reify_ir::geometry::tests::{tetra_positions,tetra_faces,
    // welded_tetra_mesh,per_face_block_tetra_mesh}` fixtures (a different
    // crate's private test module, not reusable here) so
    // `preflight_watertight_surface` — a pure fn with no gmsh/OCCT
    // dependency — can be unit-tested in the fast stub build.

    /// V0=(0,0,0) V1=(1,0,0) V2=(0,1,0) V3=(0,0,1) — a minimal outward-wound
    /// closed tetrahedron shared by the preflight tests below.
    fn tetra_positions() -> [[f32; 3]; 4] {
        [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ]
    }

    /// The four outward-wound faces of [`tetra_positions`].
    fn tetra_faces() -> [[u32; 3]; 4] {
        [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]
    }

    /// The tetrahedron with each corner referenced once — already
    /// position-welded (`weldedness().raw_welded == true`).
    fn welded_tetra_mesh() -> Mesh {
        let positions = tetra_positions();
        let vertices: Vec<f32> = positions.iter().flat_map(|v| v.iter().copied()).collect();
        let indices: Vec<u32> = tetra_faces().into_iter().flatten().collect();
        Mesh {
            vertices,
            indices,
            normals: None,
        }
    }

    /// The same tetrahedron emitted as 12 per-face-block vertices (each
    /// corner duplicated per incident face), mirroring OCCT's per-face
    /// tessellation output (occt_wrapper.cpp:5847), which is unwelded by
    /// design. Closed + consistently wound on the position-welded quotient
    /// (`Mesh::validate` accepts it) while `weldedness().raw_welded` is
    /// `false` on the raw index buffer the gmsh attributed producer
    /// actually consumes.
    fn per_face_block_tetra_mesh() -> Mesh {
        let positions = tetra_positions();
        let mut vertices = Vec::with_capacity(12 * 3);
        for face in tetra_faces() {
            for corner in face {
                vertices.extend_from_slice(&positions[corner as usize]);
            }
        }
        let indices: Vec<u32> = (0..12).collect();
        Mesh {
            vertices,
            indices,
            normals: None,
        }
    }

    /// An unwelded-but-quotient-closed surface must be rejected: the raw
    /// per-face-block tetra passes `Mesh::validate` (closed on the welded
    /// quotient) but fails `weldedness().raw_welded`, so the preflight must
    /// report a `Closed` violation with a truthful nonzero open-edge count
    /// — the raw watertightness signal the gmsh SIGSEGV actually trips on.
    #[test]
    fn preflight_rejects_unwelded_per_face_block_surface() {
        let mesh = per_face_block_tetra_mesh();
        let err = preflight_watertight_surface(&mesh, 0.0).expect_err(
            "an unwelded per-face-block tetra (OCCT-style) must fail the watertightness \
             preflight — the gmsh attributed producer consumes the raw index buffer, which \
             is genuinely non-watertight even though the welded quotient is closed",
        );
        match err {
            GeometryError::MeshContractViolation {
                kernel,
                invariant,
                counts,
                ..
            } => {
                assert_eq!(kernel, "gmsh", "violation must name the gmsh kernel");
                assert_eq!(invariant, MeshInvariant::Closed);
                assert!(
                    counts.open_edges > 0,
                    "the raw per-face-block surface must report a nonzero open-edge \
                     count: {counts:?}"
                );
            }
            other => panic!("expected GeometryError::MeshContractViolation, got {other:?}"),
        }
    }

    /// Negative control: a welded, closed, consistently-wound surface must
    /// NOT be rejected — guards against over-rejecting the watertight input
    /// the plain (non-attributed) producer and the existing watertight-cube
    /// attributed-producer test both rely on staying accepted.
    #[test]
    fn preflight_accepts_welded_watertight_surface() {
        let mesh = welded_tetra_mesh();
        preflight_watertight_surface(&mesh, 0.0).expect(
            "a welded, closed, consistently-wound tetra must pass the watertightness \
             preflight",
        );
    }

    /// The `Mesh::validate` bridge must run BEFORE the weldedness gate: a
    /// raw-welded mesh (which the weldedness check alone would accept)
    /// carrying a non-finite vertex coordinate must still be rejected, and
    /// with the `Finite` invariant the weldedness gate itself can never
    /// produce.
    #[test]
    fn preflight_rejects_non_finite_before_weldedness_check() {
        let mut mesh = welded_tetra_mesh();
        mesh.vertices[3] = f32::NAN; // vertex 1, x-component
        let err = preflight_watertight_surface(&mesh, 0.0).expect_err(
            "a mesh with a NaN vertex coordinate must fail the watertightness preflight",
        );
        match err {
            GeometryError::MeshContractViolation {
                kernel, invariant, ..
            } => {
                assert_eq!(kernel, "gmsh", "violation must name the gmsh kernel");
                assert_eq!(
                    invariant,
                    MeshInvariant::Finite,
                    "validate() must be checked first, ahead of weldedness"
                );
            }
            other => panic!("expected GeometryError::MeshContractViolation, got {other:?}"),
        }
    }

    /// [`welded_tetra_mesh`] plus one TRAILING vertex whose position exactly
    /// duplicates vertex 0's, which `indices` never references.
    /// `Mesh::weld_positions` (backing `weldedness`) walks the full raw
    /// `vertices` buffer regardless of whether `indices` references a given
    /// slot, so this extra vertex alone makes `weldedness().raw_welded ==
    /// false` — while [`raw_open_edge_census`], which only walks `indices`,
    /// sees exactly the original closed tetra and reports zero open edges.
    /// This is the counterintuitive degenerate case documented on
    /// [`raw_open_edge_census`]: `raw_welded == false` yet `open_edges ==
    /// 0`.
    fn welded_tetra_with_unreferenced_duplicate_vertex() -> Mesh {
        let mut mesh = welded_tetra_mesh();
        let dup = tetra_positions()[0];
        mesh.vertices.extend_from_slice(&dup);
        mesh
    }

    /// Degenerate fail-closed branch: an unreferenced duplicate-position
    /// vertex trips `weldedness().raw_welded == false` (the gate
    /// `preflight_watertight_surface` checks) even though the referenced raw
    /// index buffer is already closed, so `raw_open_edge_census` reports
    /// `open_edges == 0` and falls back to the `(u32::MAX, u32::MAX)`
    /// sentinel witness — no real offending edge exists to name. The
    /// preflight must still report `Err(MeshContractViolation { Closed,
    /// .. })`: it fails closed on the `raw_welded == false` signal alone,
    /// rather than inferring "safe" from a zero open-edge count that here
    /// just means no offender was reachable via `indices`.
    #[test]
    fn preflight_rejects_unreferenced_duplicate_vertex_with_zero_open_edges() {
        let mesh = welded_tetra_with_unreferenced_duplicate_vertex();
        assert!(
            !mesh.weldedness(0.0).raw_welded,
            "fixture setup: the trailing duplicate-position vertex must make \
             the raw vertex buffer unwelded"
        );

        let err = preflight_watertight_surface(&mesh, 0.0).expect_err(
            "an unreferenced duplicate-position vertex must still fail the \
             watertightness preflight (fail-closed on raw_welded == false) \
             even though the referenced index buffer has no open edges",
        );
        match err {
            GeometryError::MeshContractViolation {
                kernel,
                invariant,
                counts,
                witness,
            } => {
                assert_eq!(kernel, "gmsh", "violation must name the gmsh kernel");
                assert_eq!(invariant, MeshInvariant::Closed);
                assert_eq!(
                    counts.open_edges, 0,
                    "the referenced raw index buffer is already closed — the \
                     unreferenced duplicate vertex contributes no triangles/edges: \
                     {counts:?}"
                );
                assert_eq!(
                    witness,
                    MeshWitness::Edge {
                        u: u32::MAX,
                        v: u32::MAX
                    },
                    "with no offending edge reachable via `indices`, the witness \
                     must fall back to the documented sentinel"
                );
            }
            other => panic!("expected GeometryError::MeshContractViolation, got {other:?}"),
        }
    }
}
