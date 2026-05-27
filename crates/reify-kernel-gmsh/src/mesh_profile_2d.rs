//! 2D plane-surface meshing via Gmsh's built-in CAD API.
//!
//! PRD reference: `docs/prds/v0_3/hex-wedge-meshing.md` task #6.
//!
//! This module exposes [`mesh_plane_2d`] in both `cfg(has_gmsh)` (real FFI)
//! and `cfg(not(has_gmsh))` (stub returning `GeometryError::OperationFailed`)
//! arms — mirroring the kernel-wide single-signature convention so callers
//! in `reify-solver-elastic::mesher` don't need to cfg-gate at every
//! call-site.
//!
//! The real arm parallels [`mesh_volume`](crate::mesh_volume)'s
//! orchestrator template: acquire `init::GMSH_LOCK`, `ensure_initialized`,
//! `clear`, build the model via the built-in CAD API
//! (point → line → curve_loop → plane_surface), optionally enable
//! recombine, `mesh_generate(2)`, read back triangles (element type 2) and
//! quads (element type 3).

use reify_ir::GeometryError;

/// Marker substring embedded in the `GeometryError::OperationFailed` message
/// returned by the stub-build (`cfg(not(has_gmsh))`) arm of [`mesh_plane_2d`].
///
/// Downstream orchestrators (e.g. `reify_solver_elastic::mesher::
/// mesh_swept_profile_2d`) pattern-match on this constant via
/// `msg.contains(STUB_UNAVAILABLE_MARKER)` to distinguish "libgmsh not present
/// at build time" (configuration error) from "libgmsh failed at runtime"
/// (operational error). Keeping the marker as a `pub const` removes the
/// cross-crate magic-string coupling: any rewording of the stub message must
/// go through this constant, which is itself referenced by the orchestrator.
pub const STUB_UNAVAILABLE_MARKER: &str = "Gmsh not available";

/// Output of [`mesh_plane_2d`]: a 2D mesh in flat-XY layout with separate
/// triangle and quad index buffers.
///
/// `vertices_xy` is `[x0, y0, x1, y1, …]` (stride 2). `triangle_indices`
/// is stride-3 and `quad_indices` is stride-4; both index into
/// `vertices_xy / 2`. Both index buffers may be non-empty in the
/// partially-recombined-mesh case (the recombination algorithm leaves
/// some triangles when it cannot form a quad).
#[derive(Debug, Clone)]
pub struct MeshPlane2dResult {
    /// Flat XY vertex buffer, stride 2.
    pub vertices_xy: Vec<f64>,
    /// Stride-3 triangle connectivity; each index is `< vertices_xy.len() / 2`.
    pub triangle_indices: Vec<u32>,
    /// Stride-4 quad connectivity; each index is `< vertices_xy.len() / 2`.
    pub quad_indices: Vec<u32>,
}

/// Mesh a 2D plane surface defined by a polygonal outer boundary and zero
/// or more polygonal hole boundaries.
///
/// `outer` is a sequence of 2D points forming the outer boundary; pass at
/// least 3 distinct points. Each entry in `holes` is a similar sequence;
/// gmsh accepts either winding order for hole rings.
///
/// `mesh_size = Some(s)` sets `Mesh.MeshSizeMin/Max` to `s`; `None` defers
/// to gmsh's defaults. `recombine = true` enables blossom recombination
/// (quad-dominated output); `false` produces triangles only.
/// `deterministic = true` pins `General.NumThreads = 1` so the output is
/// repeatable run-to-run.
///
/// # Errors
///
/// Returns `GeometryError::OperationFailed` on FFI failures (annotated
/// with the failing gmsh function name) or, in stub builds, with the
/// message "Gmsh not available" (mirroring the convention used by other
/// kernel adapters).
#[cfg(has_gmsh)]
pub fn mesh_plane_2d(
    outer: &[[f64; 2]],
    holes: &[Vec<[f64; 2]>],
    mesh_size: Option<f64>,
    recombine: bool,
    deterministic: bool,
) -> Result<MeshPlane2dResult, GeometryError> {
    use std::collections::HashMap;

    use crate::ffi;
    use crate::init;

    // Recover from a poisoned lock rather than propagating: every call begins
    // with `ffi::clear()` immediately below, which wipes any half-built model
    // state left over from a panicked prior call. Mirrors `mesh_to_volume`.
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    ffi::clear()?;
    ffi::option_set_number("General.Terminal", 0.0)?;

    if let Some(s) = mesh_size
        && s > 0.0
    {
        ffi::option_set_number("Mesh.MeshSizeMin", s)?;
        ffi::option_set_number("Mesh.MeshSizeMax", s)?;
    }

    // Thread count: deterministic forces 1; otherwise let gmsh's default
    // parallelism stand. The 2D mesh generator is single-threaded for most
    // algorithms regardless, but pinning to 1 still kills any non-determinism
    // sourced from RecombineAlgorithm tie-breaking under thread interleaving.
    if deterministic {
        ffi::option_set_number("General.NumThreads", 1.0)?;
    }

    ffi::model_add("reify_profile_2d")?;

    // Helper: push a closed polygonal ring (outer or hole) as
    // point → line → curve_loop. Returns the curve-loop tag.
    let push_ring = |ring: &[[f64; 2]]| -> Result<i32, GeometryError> {
        // Caller responsibility: ring length >= 3. We don't re-validate here
        // because mesh_swept_profile_2d does the input-shape check above us;
        // gmsh's own error path would surface a degenerate-ring failure
        // through `geo_add_curve_loop`.
        let mut point_tags: Vec<i32> = Vec::with_capacity(ring.len());
        for &[x, y] in ring {
            point_tags.push(ffi::geo_add_point(x, y, 0.0, 0.0)?);
        }
        let mut line_tags: Vec<i32> = Vec::with_capacity(ring.len());
        for i in 0..ring.len() {
            let start = point_tags[i];
            let end = point_tags[(i + 1) % ring.len()];
            line_tags.push(ffi::geo_add_line(start, end)?);
        }
        ffi::geo_add_curve_loop(&line_tags)
    };

    let outer_loop = push_ring(outer)?;
    let mut wire_tags: Vec<i32> = Vec::with_capacity(1 + holes.len());
    wire_tags.push(outer_loop);
    for hole in holes {
        wire_tags.push(push_ring(hole)?);
    }
    let surf_tag = ffi::geo_add_plane_surface(&wire_tags)?;

    // Synchronise the built-in CAD into the gmsh model BEFORE setting
    // surface-scoped options (recombine resolves its (dim, tag) against the
    // synchronised model, not the built-in CAD — same gotcha hit by the
    // ffi_smoke_tests round-trip).
    ffi::geo_synchronize()?;

    if recombine {
        // Blossom recombination — produces structurally cleaner quad meshes
        // than the default simple algorithm; cost is negligible at the
        // cross-section sizes we'll see.
        ffi::option_set_number("Mesh.RecombinationAlgorithm", 1.0)?;
        // 45° = π/4 in degrees: gmsh's per-corner deviation tolerance for
        // accepting a quad. The downstream skew check in
        // `recombine_quality_ok` re-validates with a configurable threshold.
        ffi::mesh_set_recombine(2, surf_tag, 45.0)?;
    }

    ffi::mesh_generate(2)?;

    // Readback nodes. The flat coord buffer is stride-3 (x, y, z); we drop
    // the z component (always 0 for a plane surface).
    let (node_tags, coord_buf) = ffi::get_nodes_all()?;
    if coord_buf.len() != node_tags.len() * 3 {
        return Err(GeometryError::OperationFailed(format!(
            "mesh_plane_2d: gmsh get_nodes_all stride mismatch: node_tags.len()={}, \
             coord_buf.len()={} (expected {} = node_tags.len()*3)",
            node_tags.len(),
            coord_buf.len(),
            node_tags.len() * 3,
        )));
    }

    // Build (gmsh_tag → 0-based local idx) in sorted-tag order. Vertices
    // emitted in the same order — tag-N maps to index-N after remap.
    let mut paired: Vec<(u64, [f64; 2])> = node_tags
        .iter()
        .copied()
        .zip(coord_buf.chunks_exact(3))
        .map(|(t, c)| (t, [c[0], c[1]]))
        .collect();
    paired.sort_by_key(|(t, _)| *t);

    let mut tag_to_idx: HashMap<u64, u32> = HashMap::with_capacity(paired.len());
    let mut vertices_xy: Vec<f64> = Vec::with_capacity(paired.len() * 2);
    for (idx, (tag, xy)) in paired.iter().enumerate() {
        let idx_u32 = u32::try_from(idx).map_err(|_| {
            GeometryError::OperationFailed(format!(
                "mesh_plane_2d: mesh has {} nodes, exceeding the u32 index limit",
                paired.len()
            ))
        })?;
        tag_to_idx.insert(*tag, idx_u32);
        vertices_xy.push(xy[0]);
        vertices_xy.push(xy[1]);
    }

    // Readback elements. Triangle = element type 2 (3-node); quad =
    // element type 3 (4-node). Both buffers may be non-empty in the
    // partially-recombined case.
    let remap = |elem_node_tags: &[u64]| -> Result<Vec<u32>, GeometryError> {
        let mut out: Vec<u32> = Vec::with_capacity(elem_node_tags.len());
        for &tag in elem_node_tags {
            let idx = *tag_to_idx.get(&tag).ok_or_else(|| {
                GeometryError::OperationFailed(format!(
                    "mesh_plane_2d: element references unknown node tag {tag}"
                ))
            })?;
            out.push(idx);
        }
        Ok(out)
    };

    let (_tri_tags, tri_node_tags) = ffi::get_elements_by_type(2)?;
    if !tri_node_tags.len().is_multiple_of(3) {
        return Err(GeometryError::OperationFailed(format!(
            "mesh_plane_2d: triangle node_tags.len()={} not a multiple of 3",
            tri_node_tags.len(),
        )));
    }
    let triangle_indices = remap(&tri_node_tags)?;

    let (_quad_tags, quad_node_tags) = ffi::get_elements_by_type(3)?;
    if !quad_node_tags.len().is_multiple_of(4) {
        return Err(GeometryError::OperationFailed(format!(
            "mesh_plane_2d: quad node_tags.len()={} not a multiple of 4",
            quad_node_tags.len(),
        )));
    }
    let quad_indices = remap(&quad_node_tags)?;

    // Note: we intentionally do NOT issue a trailing `ffi::clear()` here.
    // The leading `ffi::clear()?` at the top of every `mesh_plane_2d` call
    // is the documented invariant for entering a clean model state; an
    // additional defensive clear at the tail would (a) duplicate that work,
    // and (b) silently swallow any failure (since the function has already
    // produced its successful return value), masking a persistent libgmsh
    // bad-state failure until the next call's leading clear surfaces it.

    Ok(MeshPlane2dResult {
        vertices_xy,
        triangle_indices,
        quad_indices,
    })
}

/// Stub-build companion: returns `GeometryError::OperationFailed` containing
/// [`STUB_UNAVAILABLE_MARKER`]. Downstream callers
/// (`reify-solver-elastic::mesher::mesh_swept_profile_2d`) detect the stub
/// error by `msg.contains(STUB_UNAVAILABLE_MARKER)` and map it to
/// `Mesh2dError::GmshUnavailable`.
#[cfg(not(has_gmsh))]
pub fn mesh_plane_2d(
    _outer: &[[f64; 2]],
    _holes: &[Vec<[f64; 2]>],
    _mesh_size: Option<f64>,
    _recombine: bool,
    _deterministic: bool,
) -> Result<MeshPlane2dResult, GeometryError> {
    Err(GeometryError::OperationFailed(format!(
        "mesh_plane_2d: {STUB_UNAVAILABLE_MARKER} in this build \
         (libgmsh not detected at build time)"
    )))
}
