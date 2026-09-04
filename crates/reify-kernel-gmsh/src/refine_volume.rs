//! Volume mesh refinement via Gmsh per-vertex size-field hints.
//!
//! PRD reference: `docs/prds/v0_4/a-posteriori-error-estimation.md` task #2.
//!
//! Exposes [`refine_volume_with_size_field`] with a uniform signature in both
//! `cfg(has_gmsh)` (real FFI) and `cfg(not(has_gmsh))` (stub) build modes —
//! mirrors the convention established by [`crate::mesh_profile_2d::mesh_plane_2d`].
//!
//! # Cache-invalidation contract
//!
//! Different `vertex_sizes` slices produce byte-distinct `VolumeMesh` outputs
//! (different tet counts and connectivity) so upstream cache keys — which are
//! keyed on all inputs — diverge automatically. No new cache-key field is
//! needed; the existing `volume_mesh_cache_key` derivation already covers this.
//!
//! # Global mesh-size clamp: inherits nothing, leaves nothing
//!
//! Gmsh's option table is process-global and survives `gmshClear()`. Since
//! task #6211 this function
//!
//! * **inbound**: sets `Mesh.MeshSizeMin`/`Mesh.MeshSizeMax` itself on every
//!   call rather than inheriting whatever a sibling entry point last left
//!   behind, so its output is a function of its own arguments alone and not of
//!   call order within the process; and
//! * **outbound**: restores that same option pair to gmsh's documented
//!   defaults before returning (via [`crate::mesh_size_clamp::MeshSizeClampReset`]),
//!   so a later *defaults-relying* call — e.g. `mesh_plane_2d` with no
//!   requested size, which deliberately writes no clamp — is not silently
//!   pinned to a fine `MeshSizeMax` left over from an adaptive-refinement
//!   iteration.
//!
//! That guard now lives in [`crate::mesh_size_clamp`] rather than in this
//! file: since task #6298 it is shared infrastructure with a second consumer,
//! `kernel_real::GmshKernel::mesh_to_volume`, and one implementation cannot
//! drift from itself the way two hand-written resets could.
//!
//! Each half has its own guard in `tests/refine_volume_tests.rs`, so neither
//! can rot into a comment: inbound is
//! `uniform_size_field_refines_monotonically_under_leaked_global_clamp`
//! (assertion 2) and `non_uniform_size_field_refines_marked_region_and_caps_
//! the_rest`; outbound is
//! `refine_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call`,
//! which straddles a refine with exactly the `mesh_plane_2d` call named above.
//!
//! Scope of that guarantee: it covers the `MeshSizeMin`/`MeshSizeMax` pair
//! only. The `Mesh.MeshSizeFromPoints` / `MeshSizeFromCurvature` /
//! `MeshSizeExtendFromBoundary` writes below are still left behind for a later
//! caller to inherit — the same defect class in the same direction, tracked as
//! task #6212 because closing it means extending the `mesh_size_clamp` seam to
//! those three across every entry point that writes them (and an
//! `option_get_number` FFI getter to restore *as found* rather than to
//! defaults), not a change local to this file. See the inline rationale at the
//! option writes below.
//!
//! # Cost basis: full remesh from surface
//!
//! `gmshModelMeshRefine()` refines uniformly across the entire existing mesh,
//! defeating localized-refinement requirements. Full remesh with per-vertex
//! `gmshModelMeshSetSize` and `Mesh.MeshSizeFromPoints=1` is the only Gmsh
//! path that honours localised size hints. This means every call regenerates
//! the entire volume mesh from the surface boundary.
//!
//! This is the explicit cost-basis the v0.4 PRD names as the trigger criterion
//! for the MMG3D bookmark (task #3003): if a refinement loop spends >30% of
//! wallclock in remeshing, swap to MMG3D.

use std::collections::HashMap;

use reify_ir::{ElementOrderTag, GeometryError, Mesh, VolumeConnectivity, VolumeMesh};

use crate::options::MeshingOptions;

#[cfg(has_gmsh)]
use crate::mesh_size_clamp::{
    GMSH_MESH_SIZE_MAX_DEFAULT, GMSH_MESH_SIZE_MIN_DEFAULT, MeshSizeClampReset,
};

/// Remesh the volume enclosed by `surface` using per-vertex size hints.
///
/// `vertex_sizes[i]` is the target characteristic element edge length at
/// surface vertex `i` (same indexing as `surface.vertices / 3`). Every
/// surface vertex must have a hint; pass `vec![uniform_size; n_verts]` for
/// a uniform refinement.
///
/// The function performs a **full remesh** from the surface boundary rather
/// than incrementally refining the current volume mesh (see module-level doc
/// for the cost/accuracy rationale).
///
/// # Errors
///
/// `cfg(has_gmsh)`: returns `GeometryError::OperationFailed` on FFI failure
/// or if Gmsh produces no volume elements.
///
/// `cfg(not(has_gmsh))`: always returns `GeometryError::OperationFailed`
/// containing [`crate::STUB_UNAVAILABLE_MARKER`] — downstream callers
/// detect this via `msg.contains(STUB_UNAVAILABLE_MARKER)`.
/// Real FFI-backed remesh implementation.
///
/// Mirrors `crates/reify-kernel-gmsh/src/kernel_real.rs::mesh_to_volume` with
/// two additional steps:
/// 1. After `geo_synchronize`, query all 0D corner entities and set their
///    target mesh size via `gmshModelMeshSetSize`.
/// 2. Enable `Mesh.MeshSizeFromPoints=1` so gmsh interpolates sizes between
///    the corner hints across the whole domain.
#[cfg(has_gmsh)]
pub fn refine_volume_with_size_field(
    surface: &Mesh,
    vertex_sizes: &[f64],
    options: &MeshingOptions,
    order: ElementOrderTag,
) -> Result<VolumeMesh, GeometryError> {
    use crate::{ffi, init};

    // --- Input validation (mirrors mesh_to_volume, with extra vertex_sizes check) ---
    if !surface.vertices.len().is_multiple_of(3) {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: surface.vertices.len()={} is not divisible by 3",
            surface.vertices.len()
        )));
    }
    if !surface.indices.len().is_multiple_of(3) {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: surface.indices.len()={} is not divisible by 3",
            surface.indices.len()
        )));
    }
    let n_verts = surface.vertices.len() / 3;
    if vertex_sizes.len() != n_verts {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: vertex_sizes.len()={} != n_verts={}; \
             one size hint required per surface vertex",
            vertex_sizes.len(),
            n_verts,
        )));
    }
    if let Some(&bad) = surface.indices.iter().find(|&&i| (i as usize) >= n_verts) {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: surface.indices contains {bad}, out of bounds \
             for mesh with {n_verts} vertices"
        )));
    }
    if surface.vertices.is_empty() || surface.indices.is_empty() {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: empty surface mesh \
             (vertices.len()={}, indices.len()={})",
            surface.vertices.len(),
            surface.indices.len()
        )));
    }

    // --- Acquire lock + initialise ---
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    ffi::clear()?;
    ffi::option_set_number("General.Terminal", 0.0)?;

    // --- Gmsh options (mirrors mesh_to_volume) ---
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
    let element_order_value: f64 = match order {
        ElementOrderTag::P1 => 1.0,
        ElementOrderTag::P2 => 2.0,
    };
    ffi::option_set_number("Mesh.ElementOrder", element_order_value)?;
    ffi::option_set_number("Mesh.Algorithm3D", 10.0)?;

    // --- Add discrete surface entity and push surface mesh ---
    ffi::model_add("reify_refine_volume")?;
    let surf_tag = ffi::add_discrete_entity(2, &[])?;

    let node_tags: Vec<u64> = (1..=n_verts as u64).collect();
    let coords_f64: Vec<f64> = surface.vertices.iter().map(|&v| v as f64).collect();
    ffi::add_nodes_2d(surf_tag, &node_tags, &coords_f64)?;

    let n_tris = surface.indices.len() / 3;
    let tri_tags: Vec<u64> = (1..=n_tris as u64).collect();
    let tri_node_tags: Vec<u64> = surface.indices.iter().map(|&i| i as u64 + 1).collect();
    ffi::add_elements_2d(surf_tag, 2, &tri_tags, &tri_node_tags)?;

    // --- Classify and create geometry ---
    //
    // Use a tighter dihedral-angle threshold (PI/12 ≈ 15°) than
    // `mesh_to_volume`'s `CLASSIFY_FEATURE_ANGLE` (PI/4) so that virtually
    // every mesh edge is treated as a "hard" edge.  For the unit-cube test
    // geometry (90° dihedral angles at each edge), this ensures all 12 edges
    // become 1D curve entities and all 8 cube-corner vertices become 0D point
    // entities.  A PI/2 threshold would emit no corner entities at all (gmsh's
    // sharp-edge test is strictly-greater-than and 90° is NOT > PI/2), leaving
    // nowhere to attach per-vertex size hints; that is also what broke
    // `mesh_to_volume` in #6200, which is why it no longer uses PI/2 either.
    // PI/12 stays deliberately sharper than PI/4 (this path wants every edge
    // hard, not just the feature edges), so it is NOT folded into the shared
    // constant.
    //
    // For the `curveAngle` (4th argument) we use the same PI/12 so that
    // vertices at intersections of curves separated by < 15° are still
    // classified as hard corners; this keeps the corner count stable across
    // test geometries.
    ffi::classify_surfaces(
        std::f64::consts::PI / 12.0,
        1,
        1,
        std::f64::consts::PI / 12.0,
        0,
    )?;
    ffi::create_geometry(&[])?;

    let surface_tags = ffi::get_entity_tags(2)?;
    if surface_tags.is_empty() {
        return Err(GeometryError::OperationFailed(
            "refine_volume_with_size_field: no dim=2 entities after classify+create_geometry; \
             surface may be open or non-manifold"
                .into(),
        ));
    }

    let loop_tag = ffi::geo_add_surface_loop(&surface_tags)?;
    let _vol_tag = ffi::geo_add_volume(&[loop_tag])?;
    ffi::geo_synchronize()?;

    // --- Per-vertex size hints ---
    //
    // `Mesh.MeshSizeFromPoints=1`: use 0D-entity (corner) sizes as mesh-size
    // anchors; gmsh interpolates these sizes across the surface and into the
    // volume.
    //
    // `Mesh.MeshSizeFromCurvature=0`: disable curvature-based refinement so
    // only our explicit corner hints drive the mesh size, preventing gmsh from
    // independently inserting small elements where the surface curves sharply.
    //
    // `Mesh.MeshSizeExtendFromBoundary=0`: do NOT propagate the gradient of
    // the 2D boundary mesh sizes into the 3D volume.  With this enabled
    // (default=1), a fine surface mesh on one face (e.g. the marked region at
    // x<0.5) extends its fineness deep into the volume, over-refining the
    // adjacent unmarked region.  Disabling this ensures that only the 0D
    // corner-entity sizes (set by `gmshModelMeshSetSize` below) drive the
    // interior mesh density, with a smooth interpolation between corners rather
    // than an aggressive gradient from the finest boundary face.
    ffi::option_set_number("Mesh.MeshSizeFromPoints", 1.0)?;
    ffi::option_set_number("Mesh.MeshSizeFromCurvature", 0.0)?;
    ffi::option_set_number("Mesh.MeshSizeExtendFromBoundary", 0.0)?;

    // --- Mesh-size clamp: set explicitly, never inherited (task #6211) ---
    //
    // INVARIANT: `vertex_sizes` alone decides element size here. Gmsh's option
    // table is process-global and is NOT reset by `gmshClear()`, and the
    // sibling entry points `mesh_profile_2d::mesh_plane_2d` and
    // `mesh_boundary`'s surface remesh still write
    // `Mesh.MeshSizeMin`/`MeshSizeMax` without restoring them. Without the two
    // writes below, either of those running earlier in the process pins every
    // element of THIS remesh to ITS size and the per-vertex field becomes
    // inert (task #6211: one identical tet count for every hint).
    //
    // `kernel_real::GmshKernel::mesh_to_volume` used to belong on that list and
    // no longer does — since task #6298 it arms the same
    // `mesh_size_clamp::MeshSizeClampReset` on entry. These two writes stay
    // load-bearing regardless: the other two entry points are still open, and
    // an inbound clamp that depends on no sibling's outbound discipline is the
    // only form that makes this function's output a pure function of its own
    // arguments.
    //
    // Both writes are load-bearing, not belt-and-braces: with a leaked
    // Min == Max, lowering only Max leaves Min > Max (gmsh still floors at the
    // leaked value) and lowering only Min leaves the leaked Max capping
    // everything.
    //
    // Min = gmsh's default: no floor, so the finest hint is honoured.
    // Deliberately not `min(vertex_sizes)`, which would forbid gmsh from going
    // finer than the finest hint anywhere in the domain — a new, untested
    // constraint on the localized-refinement path for no measured benefit.
    //
    // Max = the COARSEST requested hint, because with
    // `Mesh.MeshSizeExtendFromBoundary = 0` (set above) the 3D mesher is
    // otherwise free to grow interior elements arbitrarily coarser than
    // anything the caller asked for. Deliberately not `options.mesh_size`:
    // that is the baseline target the per-vertex field exists to supersede, and
    // feeding it back in would re-create the very clamp this defends against.
    //
    // The `is_finite` fallback degrades degenerate input to gmsh's "no cap"
    // default rather than propagating a nonsense clamp. Validating
    // `vertex_sizes` is the caller's job and is already done at
    // `reify_solver_elastic::volume_refine`'s entry point.
    //
    // `MeshSizeClampReset` closes the outbound direction: this pair is returned
    // to gmsh's defaults on every exit path, so the same leak does not run from
    // here into a later defaults-relying call.
    let max_hint = vertex_sizes
        .iter()
        .copied()
        .filter(|s| s.is_finite() && *s > 0.0)
        .fold(f64::NEG_INFINITY, f64::max);
    let max_hint = if max_hint.is_finite() {
        max_hint
    } else {
        GMSH_MESH_SIZE_MAX_DEFAULT
    };
    let _clamp_reset = MeshSizeClampReset::armed(&_guard);
    ffi::option_set_number("Mesh.MeshSizeMin", GMSH_MESH_SIZE_MIN_DEFAULT)?;
    ffi::option_set_number("Mesh.MeshSizeMax", max_hint)?;

    // For each 0D corner entity created by classify_surfaces + create_geometry,
    // map the corner back to its original input surface vertex by **coordinate
    // proximity** (nearest-neighbour scan), then set the target mesh size from
    // `vertex_sizes`.
    //
    // Why coord-based, not tag-based: `classify_surfaces` + `create_geometry`
    // rebuild the discrete entity, and gmsh does not contractually preserve
    // the original mesh-node tags pushed via `add_nodes_2d`. If gmsh ever does
    // reassign tags, a tag-based lookup would silently skip every corner and
    // the refine would return a baseline-looking unrefined mesh — a regression
    // invisible to downstream callers and to the localized-refinement test in
    // `volume_refine_tests.rs`. Coordinates are anchored to physical geometry
    // and therefore robust under reclassification. This mirrors the same
    // proximity-based mapping convention used by
    // `reify_solver_elastic::volume_refine::project_volume_to_surface_vertices`.
    //
    // We track `applied` (corners that successfully received a SetSize call)
    // and `skipped` (corners that failed at any step) so we can fail loudly
    // when zero corners are assigned: a "successful" call without any size
    // field application would silently degrade to the global default mesh
    // size and downstream tests would mistake the result for a working refine.
    let corner_tags = ffi::get_entity_tags(0)?;
    let mut applied: usize = 0;
    let mut skipped: usize = 0;
    for &corner_tag in &corner_tags {
        let (corner_x, corner_y, corner_z) = match ffi::get_nodes_at_entity(0, corner_tag) {
            Ok((_node_tags_at_corner, coords_at_corner)) => {
                match coords_at_corner.chunks_exact(3).next() {
                    Some(xyz) => (xyz[0], xyz[1], xyz[2]),
                    None => {
                        skipped += 1;
                        continue;
                    }
                }
            }
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        // Nearest-neighbour scan over input surface vertices. O(n_verts) per
        // corner is acceptable here because n_corners is small (typically <=
        // O(10s) for FEA geometries — one per "hard" feature vertex).
        let mut best_idx: usize = 0;
        let mut best_d2: f64 = f64::INFINITY;
        for i in 0..n_verts {
            let vx = surface.vertices[3 * i] as f64;
            let vy = surface.vertices[3 * i + 1] as f64;
            let vz = surface.vertices[3 * i + 2] as f64;
            let dx = vx - corner_x;
            let dy = vy - corner_y;
            let dz = vz - corner_z;
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2 < best_d2 {
                best_d2 = d2;
                best_idx = i;
            }
        }
        match ffi::mesh_set_size_at_entity(0, corner_tag, vertex_sizes[best_idx]) {
            Ok(()) => applied += 1,
            Err(_) => skipped += 1,
        }
    }

    if applied == 0 {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: no corner sizes applied \
             ({} corner entities found, {} skipped — size field would have no effect)",
            corner_tags.len(),
            skipped
        )));
    }
    if skipped > 0 {
        tracing::debug!(
            target: "reify_kernel_gmsh::refine_volume",
            applied = applied,
            skipped = skipped,
            total_corners = corner_tags.len(),
            "some corner sizes were not applied"
        );
    }

    // --- Tet meshing ---
    ffi::mesh_generate(3)?;

    // --- Readback (mirrors mesh_to_volume verbatim) ---
    let elem_type = match order {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 11,
    };
    let nodes_per_elem: usize = match order {
        ElementOrderTag::P1 => 4,
        ElementOrderTag::P2 => 10,
    };

    let (out_node_tags, coord_buf) = ffi::get_nodes_all()?;
    if coord_buf.len() != out_node_tags.len() * 3 {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: get_nodes_all stride mismatch: \
             node_tags.len()={}, coord_buf.len()={} (expected {})",
            out_node_tags.len(),
            coord_buf.len(),
            out_node_tags.len() * 3,
        )));
    }
    let (_elem_tags, elem_node_tags) = ffi::get_elements_by_type(elem_type)?;
    if !elem_node_tags.len().is_multiple_of(nodes_per_elem) {
        return Err(GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: get_elements_by_type stride mismatch: \
             elem_node_tags.len()={} not multiple of {nodes_per_elem}",
            elem_node_tags.len(),
        )));
    }

    let mut paired: Vec<(u64, [f64; 3])> = out_node_tags
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
                "refine_volume_with_size_field: {} nodes exceeds u32 tet_indices limit",
                paired.len()
            ))
        })?;
        tag_to_idx.insert(*tag, idx_u32);
        vertices.extend(xyz.iter().map(|&v| v as f32));
    }

    let mut tet_indices: Vec<u32> = Vec::with_capacity(elem_node_tags.len());
    for &tag in &elem_node_tags {
        let idx = *tag_to_idx.get(&tag).ok_or_else(|| {
            GeometryError::OperationFailed(format!(
                "refine_volume_with_size_field: element references unknown node tag {tag}"
            ))
        })?;
        tet_indices.push(idx);
    }

    let _ = ffi::clear();

    Ok(VolumeMesh {
        vertices,
        connectivity: VolumeConnectivity::Tet {
            indices: tet_indices,
            order,
        },
        normals: None,
        boundary: None,
    })
}

/// Stub-build companion: always returns `GeometryError::OperationFailed`
/// containing [`crate::STUB_UNAVAILABLE_MARKER`].
#[cfg(not(has_gmsh))]
pub fn refine_volume_with_size_field(
    _surface: &Mesh,
    _vertex_sizes: &[f64],
    _options: &MeshingOptions,
    _order: ElementOrderTag,
) -> Result<VolumeMesh, GeometryError> {
    Err(GeometryError::OperationFailed(format!(
        "refine_volume_with_size_field: {} in this build \
         (libgmsh not detected at build time)",
        crate::STUB_UNAVAILABLE_MARKER,
    )))
}
