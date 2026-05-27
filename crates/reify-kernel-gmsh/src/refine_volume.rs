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

use reify_ir::{ElementOrderTag, GeometryError, Mesh, VolumeMesh};

use crate::options::MeshingOptions;

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
    // `mesh_to_volume`'s PI/2 so that virtually every mesh edge is treated as
    // a "hard" edge.  For the unit-cube test geometry (90° dihedral angles at
    // each edge), this ensures all 12 edges become 1D curve entities and all
    // 8 cube-corner vertices become 0D point entities.  Without this, the
    // cube corners do NOT become 0D entities under PI/2 threshold (90° is NOT
    // > PI/2), so we'd only have ~1 corner entity and could not assign
    // per-vertex size hints.
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
        tet_indices,
        element_order: order,
        normals: None,
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
