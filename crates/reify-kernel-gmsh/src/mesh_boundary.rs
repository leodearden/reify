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

use reify_types::{BoundaryAssociation, GeometryError, GeometryHandleId, Mesh, NodeAttachment};

#[cfg(has_gmsh)]
use reify_types::{ElementOrderTag, VolumeMesh};
#[cfg(has_gmsh)]
use crate::{
    auto_size::AutoSizeConfig,
    mesh_volume::{compute_thickness_warnings, resolve_mesh_size},
    options::MeshingOptions,
    repair::RepairConfig,
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
    /// `0.0` disables all matching.
    pub match_tolerance: f64,
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
/// # Repair incompatibility
///
/// Calling with `repair_cfg = Some(...)` is not currently supported and
/// returns `Err(GeometryError::OperationFailed)`. Apply repair upstream and
/// pass `None`.
///
/// # Stage order
///
/// 1. (Repair guard — rejects `Some(repair_cfg)` for now.)
/// 2. `resolve_mesh_size` — honours caller override or derives from features.
/// 3. GMesh pipeline — classify + create_geometry + HXT tet meshing.
/// 4. Entity-membership queries (`ffi::get_nodes_at_entity`).
/// 5. Nearest-anchor matching → builds `BoundaryAssociation`.
/// 6. `compute_thickness_warnings` post-stage.
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
    // Reject repair (attribution reassignment after vertex merging is not
    // yet supported — see task description).
    if repair_cfg.is_some() {
        return Err(GeometryError::OperationFailed(
            "mesh_surface_to_volume_with_attribution: repair_cfg must be None; \
             vertex-merging repair invalidates per-vertex attribution. \
             Apply repair upstream before building the EntityAttribution."
                .into(),
        ));
    }

    // --- Pre-stage: resolve mesh size ---
    let resolved = resolve_mesh_size(surface, options, auto_size_cfg)?;
    let inner_options = MeshingOptions { mesh_size: resolved, ..options.clone() };

    // --- GMesh pipeline + entity-membership queries ---
    let (volume, node_attribution) =
        run_meshing_with_entity_queries(surface, &inner_options, order, attribution)?;

    // --- Build BoundaryAssociation from entity-membership map ---
    let mut boundary = BoundaryAssociation::default();
    for (local_idx, attachment) in node_attribution {
        boundary.associate(local_idx, attachment);
    }

    // --- Post-stage: through-thickness warnings ---
    let through_thickness_warnings = compute_thickness_warnings(&volume, surface, thickness_cfg);

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
        VolumeMesh { vertices, tet_indices, element_order, normals: None },
        node_attribution,
    ))
}

// ---------------------------------------------------------------------------
// Internal helper: nearest-anchor matching
// ---------------------------------------------------------------------------

/// Find the handle in `candidates` whose anchor is closest to `query_anchor`
/// (Euclidean squared distance). Returns `None` if `candidates` is empty or
/// the nearest distance exceeds `tol_sq`.
fn find_closest_anchor(
    query_anchor: [f64; 3],
    candidates: &[(GeometryHandleId, [f64; 3])],
    tol_sq: f64,
) -> Option<GeometryHandleId> {
    if tol_sq <= 0.0 {
        return None;
    }
    candidates
        .iter()
        .map(|(handle, anchor)| {
            let dx = query_anchor[0] - anchor[0];
            let dy = query_anchor[1] - anchor[1];
            let dz = query_anchor[2] - anchor[2];
            let d2 = dx * dx + dy * dy + dz * dz;
            (d2, *handle)
        })
        .filter(|&(d2, _)| d2 < tol_sq)
        .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, handle)| handle)
}
