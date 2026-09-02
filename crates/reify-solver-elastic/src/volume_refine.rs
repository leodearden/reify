//! A-posteriori volume mesh refinement driven by per-element size hints.
//!
//! PRD reference: `docs/prds/v0_4/a-posteriori-error-estimation.md` task #2.
//!
//! This module is the FEA-domain orchestrator that bridges the a-posteriori
//! error-indicator output (per-element size hints from `ZzIndicator`) to the
//! kernel-gmsh remesher ([`reify_kernel_gmsh::refine_volume_with_size_field`]).
//!
//! # Projection algorithm: per-element → per-vertex (min over incident elements)
//!
//! Gmsh's `SetSize` API assigns a target characteristic length to each surface
//! vertex. The error indicator produces per-*element* hints. The projection
//! uses a conservative `min` over all elements incident to each vertex: any
//! element that wants a smaller mesh wins at the shared vertex. A mean would
//! dilute the refinement signal at marked/unmarked boundaries.
//!
//! # Stub-build routing
//!
//! When the kernel-gmsh crate is compiled without libgmsh
//! (`cfg(not(has_gmsh))`), `refine_volume_with_size_field` returns a
//! `GeometryError::OperationFailed` message containing
//! [`reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER`].  [`map_geometry_error`]
//! routes that to [`RefineError::GmshUnavailable`] so callers can distinguish
//! "no libgmsh in this build" from "libgmsh failed at runtime".

use std::fmt;

use reify_kernel_gmsh::MeshingOptions;
use reify_ir::{ElementOrderTag, GeometryError, Mesh, VolumeMesh};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors returned by [`refine_with_size_field`] and
/// [`crate::adaptive::refine_marked_elements`].
#[derive(Debug)]
pub enum RefineError {
    /// `size_hints.len()` does not match the element count of `volume_mesh`.
    SizeHintsLengthMismatch { got: usize, expected: usize },
    /// A marked element index is `>= element_count` of `volume_mesh` (raised by
    /// [`crate::adaptive::refine_marked_elements`] before it indexes the
    /// per-element sizes).
    MarkedIndexOutOfRange { index: usize, element_count: usize },
    /// A size hint at the given index is `<= 0.0`.
    NonPositiveSize { index: usize, size: f64 },
    /// A size hint at the given index is non-finite (NaN or ±inf).
    NonFiniteSize { index: usize },
    /// The kernel-gmsh crate was compiled without libgmsh — no meshing
    /// is possible in this build.
    GmshUnavailable,
    /// The kernel-gmsh FFI call failed at runtime.
    Gmsh(GeometryError),
    /// `volume_mesh`'s connectivity is `Hex` or `Wedge` — this crate's
    /// a-posteriori refinement pipeline is tet-only (task 4996 hardening;
    /// hex/wedge meshes come from the sweep pipeline and have no refine path
    /// here).
    UnsupportedConnectivity,
    /// `volume_mesh`'s tet index buffer length is not a whole multiple of the
    /// per-element node count, so it does not describe a whole number of
    /// elements. Sibling of [`RefineError::UnsupportedConnectivity`]: both
    /// reject a mis-shaped `VolumeMesh` at the `tet_shape` chokepoint
    /// rather than letting the truncated count panic downstream in
    /// `project_per_element_sizes_to_vertices`'s remainder chunk.
    MalformedTetIndices {
        /// `tet_indices.len()`.
        len: usize,
        /// Per-element node count (4 for P1, 10 for P2).
        stride: usize,
    },
    /// A tet index addresses a vertex that does not exist
    /// (`vertex_index >= volume_mesh.vertices.len() / 3`).
    ///
    /// The *semantic* companion to [`RefineError::MalformedTetIndices`]'s
    /// *structural* check — the same split
    /// `reify_mesh_morph::elasticity::ElasticityFailure` draws between
    /// `MalformedTetIndices` and `InvalidTetIndex`. Without it a mesh of the
    /// right shape but with an out-of-range index aborts the process in
    /// [`project_per_element_sizes_to_vertices`], which indexes
    /// `vertex_sizes[v]` unguarded.
    InvalidTetIndex {
        /// The offending index VALUE read out of the tet index buffer (not its
        /// position in that buffer — unlike the `index` field on the
        /// `size_hints` variants above).
        vertex_index: u32,
        /// `volume_mesh.vertices.len() / 3`, the exclusive upper bound.
        vertex_count: usize,
    },
}

impl fmt::Display for RefineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefineError::SizeHintsLengthMismatch { got, expected } => write!(
                f,
                "size_hints length mismatch: got {got}, expected {expected} (one per element)"
            ),
            RefineError::MarkedIndexOutOfRange {
                index,
                element_count,
            } => write!(
                f,
                "marked element index {index} is out of range (mesh has {element_count} elements)"
            ),
            RefineError::NonPositiveSize { index, size } => write!(
                f,
                "size_hints[{index}] = {size} is non-positive; all hints must be > 0"
            ),
            RefineError::NonFiniteSize { index } => {
                write!(f, "size_hints[{index}] is non-finite (NaN or ±inf)")
            }
            RefineError::GmshUnavailable => {
                write!(f, "libgmsh is not available in this build")
            }
            RefineError::Gmsh(e) => write!(f, "gmsh FFI error: {e}"),
            RefineError::UnsupportedConnectivity => write!(
                f,
                "volume refinement is tet-only: a Hex/Wedge VolumeMesh cannot be \
                 remeshed by the Gmsh size-field refiner"
            ),
            RefineError::MalformedTetIndices { len, stride } => write!(
                f,
                "malformed tet connectivity: {len} indices is not a whole multiple \
                 of the {stride}-node per-element stride"
            ),
            RefineError::InvalidTetIndex {
                vertex_index,
                vertex_count,
            } => write!(
                f,
                "tet index {vertex_index} is out of range (mesh has {vertex_count} \
                 vertices)"
            ),
        }
    }
}

impl std::error::Error for RefineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RefineError::Gmsh(e) => Some(e),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Element topology helpers
// ---------------------------------------------------------------------------

/// The validated shape of a tet [`VolumeMesh`], as established by
/// [`tet_shape`].
///
/// Carrying the order and stride alongside the element count is what lets
/// the post-gate pipeline avoid re-deriving them. Each field has a live
/// consumer:
///
/// * `n_elements` — the expected `size_hints` / `current_sizes` length in
///   [`refine_with_size_field`] and
///   [`crate::adaptive::refine_marked_elements`], and the exclusive upper
///   bound on the latter's marked indices.
/// * `stride` — the chunk width
///   [`project_per_element_sizes_to_vertices`] walks the index buffer with, so
///   the projector chunks by the very stride the divisibility check proved the
///   buffer to be a whole multiple of, rather than re-deriving one that could
///   drift from it.
/// * `order` — handed to the kernel-gmsh remesher. Before this struct existed,
///   [`refine_with_size_field`] followed the gate with a second
///   `volume_mesh.element_order().ok_or(RefineError::UnsupportedConnectivity)?`
///   whose error arm the gate had already proved unreachable — untestable dead
///   code that nonetheless read as a live error path.
///
/// Because [`tet_shape`] is its only constructor, a `TetShape` argument also
/// serves as a lightweight proof token: a function that demands one cannot be
/// reached without *some* mesh having passed the gate. It does not pin *which*
/// mesh, so functions taking both still document a same-mesh caller contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TetShape {
    /// Number of tetrahedral elements (`tet_indices.len() / stride`, exact —
    /// [`tet_shape`] rejects a non-multiple buffer rather than truncating).
    pub(crate) n_elements: usize,
    /// Per-element node count: 4 (P1) or 10 (P2). Consumed by
    /// [`project_per_element_sizes_to_vertices`] as its chunk width.
    pub(crate) stride: usize,
    /// The mesh's element order tag, read once at the gate.
    pub(crate) order: ElementOrderTag,
}

/// Validate `volume_mesh`'s tet connectivity and return its [`TetShape`].
///
/// This is the shared mesh-shape gate for both public entry points
/// ([`refine_with_size_field`] and [`crate::adaptive::refine_marked_elements`],
/// which both call this first) — a Hex/Wedge mesh, or a tet mesh whose index
/// buffer does not describe a whole number of elements, is rejected here,
/// before any panic-prone helper or gmsh call runs.
///
/// The divisibility check is what makes [`TetShape::n_elements`] exact rather
/// than truncated: without it a 5-index P1 mesh would report 1 element, clear
/// the `size_hints` length check, and then panic in
/// [`project_per_element_sizes_to_vertices`], whose `chunks(stride)` walk
/// emits a trailing remainder chunk and indexes `per_element_sizes[1]`.
///
/// # Scope of the guarantee
///
/// Two structural checks (connectivity family, buffer length) plus one
/// semantic check (index values in range). Together they are what make
/// [`project_per_element_sizes_to_vertices`] panic-free for a gated mesh:
/// length divisibility rules out the short remainder chunk, and the
/// index-range scan rules out its unguarded `vertex_sizes[v]` indexing.
///
/// Vertex ORDERING, element quality and degeneracy are explicitly NOT checked
/// — a gated mesh is well-formed enough not to abort this pipeline, not
/// necessarily meshable by gmsh.
///
/// # Errors
///
/// Returns [`RefineError::UnsupportedConnectivity`] if `volume_mesh`'s
/// connectivity is `Hex` or `Wedge`, [`RefineError::MalformedTetIndices`] if
/// `tet_indices.len()` is not a whole multiple of the per-element node count,
/// or [`RefineError::InvalidTetIndex`] if any index is `>= vertices.len() / 3`.
/// The structural checks run before the semantic one, so a buffer that is both
/// mis-sized and out-of-range reports `MalformedTetIndices`.
pub(crate) fn tet_shape(volume_mesh: &VolumeMesh) -> Result<TetShape, RefineError> {
    let tet_indices = volume_mesh
        .tet_indices()
        .ok_or(RefineError::UnsupportedConnectivity)?;
    // `nodes_per_element()` is 4 (P1) or 10 (P2) for `Tet` connectivity, which
    // the guard above has established — never 0, so the `%`/`/` are safe.
    let stride = volume_mesh.nodes_per_element();
    if !tet_indices.len().is_multiple_of(stride) {
        return Err(RefineError::MalformedTetIndices {
            len: tet_indices.len(),
            stride,
        });
    }
    // Semantic check, after the two structural ones: every index must address
    // a vertex that exists. `project_per_element_sizes_to_vertices` indexes
    // `vertex_sizes[v]` (sized `vertices.len() / 3`) with no bounds check, so
    // an out-of-range VALUE in a correctly-SHAPED buffer would abort the
    // process instead of returning a `RefineError`. Mirrors the
    // structural-then-semantic ordering `reify_mesh_morph::elasticity` uses
    // for `MalformedTetIndices` → `InvalidTetIndex`.
    let vertex_count = volume_mesh.vertices.len() / 3;
    if let Some(&vertex_index) = tet_indices.iter().find(|&&i| i as usize >= vertex_count) {
        return Err(RefineError::InvalidTetIndex {
            vertex_index,
            vertex_count,
        });
    }
    // Same guard as the connectivity gate: `Tet` connectivity ⇒
    // `element_order()` is `Some`. Reading it HERE, inside the gate that
    // proves it, is what keeps the `None` arm out of the callers as a phantom
    // error path.
    let order = volume_mesh
        .element_order()
        .ok_or(RefineError::UnsupportedConnectivity)?;
    Ok(TetShape {
        n_elements: tet_indices.len() / stride,
        stride,
        order,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Project per-element size hints onto per-vertex sizes via a conservative
/// `min` over incident elements.
///
/// Returns a `Vec<f64>` of length `volume_mesh.vertices.len() / 3`, where
/// entry `v` is the minimum of `per_element_sizes[e]` for all elements `e`
/// incident to vertex `v`.
///
/// Vertices not touched by any element receive `f64::INFINITY` — callers
/// should validate inputs so this does not occur in practice.
///
/// # Design note: why `min` instead of mean?
///
/// The min-projection ensures any element that wants a locally smaller mesh
/// wins at its shared vertices. A mean would dilute the refinement signal at
/// the boundary between a marked and an unmarked region.
///
/// # Caller contract
///
/// `shape` MUST be `tet_shape(volume_mesh)?` for *this* `volume_mesh`, and the
/// caller MUST validate `per_element_sizes.len() == shape.n_elements` BEFORE
/// invoking. The implementation indexes `per_element_sizes[elem_idx]` without
/// a bounds check; an out-of-bounds element will panic. The only safe caller
/// is [`refine_with_size_field_validated`], which performs that length
/// validation up front (see its `size_hints.len() != n_elements` check).
/// [`tet_shape`], which supplies both the expected length and the `shape`
/// argument, also rejects a non-multiple-of-stride index buffer and any index
/// `>= n_verts`, so the `chunks(shape.stride)` walk below can see neither a
/// short remainder chunk nor an out-of-range `vertex_sizes[v]` — the two panic
/// paths a gated mesh would otherwise still reach. Taking the stride from
/// `shape` rather than re-reading `volume_mesh.nodes_per_element()` is what
/// makes the chunk width provably the one the gate validated.
///
/// The panic contract is pinned by the regression test
/// `project_panics_on_too_short_per_element_sizes` in the in-module `tests`
/// block — future authors who silently misbehave on short slices (e.g. via
/// `get(elem_idx).copied().unwrap_or(...)`) will see that test fail.
///
/// Visibility is `pub(crate)` to prevent external callers from misusing the
/// function with a short slice. The reviewer_comprehensive robustness
/// finding (option (a)) chose visibility narrowing over a `Result`-typed
/// length check; the up-front check in `refine_with_size_field` already
/// covers the validation duty for in-tree callers.
// At time of writing, consumed by same-file caller
// `refine_with_size_field_validated` (~line 199). The G-tool flags same-file
// callers as orphans; the call site is live.
// G-allow: same-file consumer `refine_with_size_field_validated` (G-tool same-file-caller heuristic limitation).
pub(crate) fn project_per_element_sizes_to_vertices(
    volume_mesh: &VolumeMesh,
    shape: TetShape,
    per_element_sizes: &[f64],
) -> Vec<f64> {
    let n_verts = volume_mesh.vertices.len() / 3;
    // The gate-validated stride, NOT a fresh `volume_mesh.nodes_per_element()`
    // lookup: chunking by the same width `tet_shape` proved the buffer to be a
    // whole multiple of is what rules out a short trailing remainder chunk
    // here — a second, independent derivation could only agree by coincidence.
    let nodes_per_elem = shape.stride;

    let mut vertex_sizes = vec![f64::INFINITY; n_verts];

    // Guarded invariant: `shape` can only have come from
    // `tet_shape(volume_mesh)?`, which already proves
    // `volume_mesh.connectivity` is `Tet` (Hex/Wedge is rejected there as
    // `RefineError::UnsupportedConnectivity`) — so this is unreachable for a
    // Hex/Wedge mesh, not a live panic path.
    let tet_indices = volume_mesh.tet_indices().expect(
        "project_per_element_sizes_to_vertices: the `shape` argument is minted \
         only by tet_shape(volume_mesh)?, which rejects Hex/Wedge connectivity \
         — it cannot reach here",
    );
    for (elem_idx, chunk) in tet_indices.chunks(nodes_per_elem).enumerate() {
        let size = per_element_sizes[elem_idx];
        for &v_idx in chunk {
            let v = v_idx as usize;
            if vertex_sizes[v] > size {
                vertex_sizes[v] = size;
            }
        }
    }

    vertex_sizes
}

/// Remesh the volume enclosed by `surface` using per-element size hints.
///
/// Validates `size_hints`, projects them to per-vertex sizes (via
/// [`project_per_element_sizes_to_vertices`]), then delegates to
/// [`reify_kernel_gmsh::refine_volume_with_size_field`].
///
/// # Arguments
///
/// * `surface` — the original closed surface boundary (same surface used to
///   produce `volume_mesh`; needed for full remesh from surface).
/// * `volume_mesh` — the current mesh providing element count and topology
///   for size-hint validation and projection.
/// * `size_hints` — one `f64 > 0` per element of `volume_mesh` (in element
///   order: `size_hints[e]` is the target characteristic edge length for tet
///   `e`). Pass the element-wise sizes derived from the Z-Z error indicator.
/// * `options` — forwarded to the kernel-gmsh mesher unchanged.
///
/// # Errors
///
/// Returns [`RefineError::UnsupportedConnectivity`] if `volume_mesh`'s
/// connectivity is `Hex` or `Wedge` — this refiner is tet-only —
/// [`RefineError::MalformedTetIndices`] if its tet index buffer is not a
/// whole multiple of the per-element node count, or
/// [`RefineError::InvalidTetIndex`] if an index addresses a vertex that does
/// not exist. All three gates are the shared `tet_shape` chokepoint and run
/// **first**, ahead of the size-hint validation below and before any gmsh
/// work, so a malformed mesh fails fast and build-agnostically.
///
/// Otherwise returns `RefineError::SizeHintsLengthMismatch` if
/// `size_hints.len() != element_count`, `RefineError::NonFiniteSize` on NaN
/// or ±∞, `RefineError::NonPositiveSize` on `<= 0`, or kernel errors on
/// Gmsh failures.
pub fn refine_with_size_field(
    surface: &Mesh,
    volume_mesh: &VolumeMesh,
    size_hints: &[f64],
    options: &MeshingOptions,
) -> Result<VolumeMesh, RefineError> {
    // Mesh-shape gate: rejects Hex/Wedge, non-multiple index buffers and
    // out-of-range indices before any other validation, panic-prone helper, or
    // gmsh call runs.
    let shape = tet_shape(volume_mesh)?;
    refine_with_size_field_validated(surface, volume_mesh, shape, size_hints, options)
}

/// [`refine_with_size_field`]'s body, minus the mesh-shape gate.
///
/// Split out so the two public entry points run [`tet_shape`] exactly once
/// per call *between* them: [`crate::adaptive::refine_marked_elements`] needs
/// [`TetShape::n_elements`] for its own length and marked-index guards, and
/// then tail-calls this rather than re-entering [`refine_with_size_field`] and
/// paying a second O(n_indices) validation scan for a guaranteed-identical
/// result on the same `&VolumeMesh`.
///
/// # Caller contract
///
/// `shape` MUST be `tet_shape(volume_mesh)?` for *this* `volume_mesh`. It is
/// the proof that the panic-prone helpers below are unreachable, and it
/// supplies the stride [`project_per_element_sizes_to_vertices`] chunks by.
///
/// # Errors
///
/// Everything [`refine_with_size_field`] documents *except* the three
/// mesh-shape errors, which the caller's own gate has already returned.
pub(crate) fn refine_with_size_field_validated(
    surface: &Mesh,
    volume_mesh: &VolumeMesh,
    shape: TetShape,
    size_hints: &[f64],
    options: &MeshingOptions,
) -> Result<VolumeMesh, RefineError> {
    // The gate also read the element order while proving connectivity is
    // `Tet`, so the kernel call below needs no second `element_order()` lookup
    // (and no unreachable `None` arm).
    let TetShape {
        n_elements,
        order: element_order,
        ..
    } = shape;

    // Validate size_hints length.
    if size_hints.len() != n_elements {
        return Err(RefineError::SizeHintsLengthMismatch {
            got: size_hints.len(),
            expected: n_elements,
        });
    }

    // Validate individual hint values.
    for (i, &s) in size_hints.iter().enumerate() {
        if !s.is_finite() {
            return Err(RefineError::NonFiniteSize { index: i });
        }
        if s <= 0.0 {
            return Err(RefineError::NonPositiveSize { index: i, size: s });
        }
    }

    // Project per-element hints → per-volume-vertex sizes (conservative min).
    let vol_vertex_sizes = project_per_element_sizes_to_vertices(volume_mesh, shape, size_hints);

    // Map per-volume-vertex sizes → per-surface-vertex sizes.
    //
    // The surface boundary vertices of `volume_mesh` correspond to the input
    // `surface` vertices (same positions, f32 coords).  For each surface
    // vertex we find the nearest volume-mesh vertex by squared-distance and
    // adopt its projected size.  This is O(n_surf × n_vol) but acceptable for
    // test-scale meshes; a spatial index would be needed for production-scale
    // refinement loops.
    let surface_vertex_sizes =
        project_volume_to_surface_vertices(surface, volume_mesh, &vol_vertex_sizes);

    // Delegate to the kernel-gmsh helper for the full-remesh with size hints.
    reify_kernel_gmsh::refine_volume_with_size_field(
        surface,
        &surface_vertex_sizes,
        options,
        element_order,
    )
    .map_err(map_geometry_error)
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Map per-volume-mesh-vertex sizes to per-surface-vertex sizes via
/// nearest-neighbour coordinate matching.
///
/// The boundary vertices of `volume_mesh` are the same points as the surface
/// mesh vertices (both stored as f32 flat XYZ coords, same positions).  For
/// each surface vertex we scan all volume vertices and adopt the size of the
/// closest one.  The scan is O(n_surf × n_vol) — acceptable for test-scale
/// meshes (n_surf ≪ n_vol is typical); a spatial index is the right upgrade
/// if this path shows up in profiling.
///
/// If no volume vertex is found within a finite distance (shouldn't happen
/// for a well-formed surface/volume pair), the surface vertex receives the
/// global minimum of `vol_vertex_sizes` as a safe fallback.
fn project_volume_to_surface_vertices(
    surface: &Mesh,
    volume_mesh: &VolumeMesh,
    vol_vertex_sizes: &[f64],
) -> Vec<f64> {
    let n_surf = surface.vertices.len() / 3;
    let n_vol = volume_mesh.vertices.len() / 3;

    // Compute global minimum over FINITE sizes only.
    // `vol_vertex_sizes` may contain f64::INFINITY for volume vertices that
    // are not referenced by any tet element (orphaned surface/boundary nodes
    // produced by gmsh's classify_surfaces + create_geometry step). These
    // orphaned nodes must be excluded from the nearest-neighbour search so
    // the surface vertex sizes are not contaminated by the orphaned infinity.
    let finite_min = vol_vertex_sizes
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(f64::INFINITY, f64::min);
    // Safe fallback: if somehow ALL vol_vertex_sizes are infinite, every
    // surface vertex receives f64::INFINITY too (signals a misconfiguration
    // upstream; callers are responsible for passing a well-formed volume mesh).
    let fallback = finite_min;

    let mut result = vec![fallback; n_surf];
    for (s, result_slot) in result.iter_mut().enumerate() {
        let sx = surface.vertices[s * 3];
        let sy = surface.vertices[s * 3 + 1];
        let sz = surface.vertices[s * 3 + 2];

        let mut best_dist_sq = f32::INFINITY;
        let mut best_size = fallback;
        for (v, &vol_size) in vol_vertex_sizes.iter().enumerate().take(n_vol) {
            // Skip orphaned nodes (not part of any tet) — they carry
            // f64::INFINITY and would pollute the result if chosen as the
            // nearest neighbour.
            if !vol_size.is_finite() {
                continue;
            }
            let vx = volume_mesh.vertices[v * 3];
            let vy = volume_mesh.vertices[v * 3 + 1];
            let vz = volume_mesh.vertices[v * 3 + 2];
            let dist_sq =
                (sx - vx) * (sx - vx) + (sy - vy) * (sy - vy) + (sz - vz) * (sz - vz);
            if dist_sq < best_dist_sq {
                best_dist_sq = dist_sq;
                best_size = vol_size;
            }
        }
        *result_slot = best_size;
    }
    result
}

/// Map a `GeometryError` from the kernel-gmsh layer to a `RefineError`,
/// routing stub-build errors to [`RefineError::GmshUnavailable`].
///
/// The substring anchor is the `pub const STUB_UNAVAILABLE_MARKER` from
/// `reify_kernel_gmsh::mesh_profile_2d` — both this function and the stub
/// body in `refine_volume.rs` reference the same constant, so any reword of
/// the stub message goes through the constant and is caught here at compile
/// time.
///
/// This mirrors the `mesher::map_geometry_error` convention at
/// `crates/reify-solver-elastic/src/mesher.rs:535-544`.
pub(crate) fn map_geometry_error(err: GeometryError) -> RefineError {
    match &err {
        GeometryError::OperationFailed(msg)
            if msg.contains(reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER) =>
        {
            RefineError::GmshUnavailable
        }
        _ => RefineError::Gmsh(err),
    }
}

// ---------------------------------------------------------------------------
// Unit tests (run in both stub and real builds)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::VolumeConnectivity;

    fn two_tet_bipyramid() -> VolumeMesh {
        // 5-vertex bipyramid:
        //   tet A = [0, 1, 2, 3]
        //   tet B = [0, 1, 2, 4]
        // Vertices 0,1,2,3 are in tet A; vertices 0,1,2,4 are in tet B.
        VolumeMesh {
            vertices: vec![
                0.0, 0.0, 0.0, // 0
                1.0, 0.0, 0.0, // 1
                0.0, 1.0, 0.0, // 2
                0.0, 0.0, 1.0, // 3
                0.0, 0.0, -1.0, // 4
            ],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![
                    0, 1, 2, 3, // tet A
                    0, 1, 2, 4, // tet B
                ],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        }
    }

    // ---- step-11 pins: project_per_element_sizes_to_vertices ----

    /// Conservative min projection over shared vertices.
    ///
    /// Two-tet bipyramid: vertices [0,1,2,3] in tet A (size 0.5), vertices
    /// [0,1,2,4] in tet B (size 1.0). Shared vertices 0..=2 take
    /// `min(0.5, 1.0) = 0.5`. Vertex 3 (only in A) stays 0.5. Vertex 4 (only
    /// in B) stays 1.0.
    ///
    /// Relocated from `tests/volume_refine_tests.rs` after step-12 restricted
    /// the projector to `pub(crate)` visibility.
    #[test]
    fn project_per_element_sizes_to_vertices_takes_min_over_incident_elements() {
        let vm = two_tet_bipyramid();
        let per_elem = [0.5_f64, 1.0_f64];
        let shape = super::tet_shape(&vm).expect("bipyramid is a well-formed P1 tet mesh");

        let result = super::project_per_element_sizes_to_vertices(&vm, shape, &per_elem);

        assert_eq!(
            result.len(),
            5,
            "returned slice must have length = n_vertices = 5"
        );
        assert_eq!(
            result,
            vec![0.5, 0.5, 0.5, 0.5, 1.0],
            "vertices 0-3 incident to tet A → min(0.5, 1.0) = 0.5; \
             vertex 4 only in tet B → stays 1.0"
        );
    }

    /// Caller contract: passing fewer `per_element_sizes` than the element
    /// count MUST panic (unguarded indexing).
    ///
    /// This pin documents the projector's caller-validation contract: the
    /// only safe caller is `refine_with_size_field_validated`, which validates
    /// `size_hints.len() == n_elements` up front. Future
    /// authors who silently misbehave on short slices (e.g. via
    /// `get(elem_idx).copied().unwrap_or(...)`) will see this test fail and
    /// be forced to revisit the contract.
    #[test]
    fn project_panics_on_too_short_per_element_sizes() {
        let vm = two_tet_bipyramid(); // 2 tets
        let too_short = [0.5_f64]; // only 1 size for 2 elements
        let shape = super::tet_shape(&vm).expect("bipyramid is a well-formed P1 tet mesh");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::project_per_element_sizes_to_vertices(&vm, shape, &too_short)
        }));
        assert!(
            result.is_err(),
            "project_per_element_sizes_to_vertices must panic on too-short \
             per_element_sizes (got 1, expected 2); contract is documented \
             as caller-validated indexing",
        );
    }

    // ---- step-9 pins: map_geometry_error routing ----

    #[test]
    fn stub_marker_message_maps_to_gmsh_unavailable() {
        let stub_err = GeometryError::OperationFailed(format!(
            "refine_volume_with_size_field: {} in this build",
            reify_kernel_gmsh::STUB_UNAVAILABLE_MARKER,
        ));
        let mapped = map_geometry_error(stub_err);
        assert!(
            matches!(mapped, RefineError::GmshUnavailable),
            "stub marker must map to GmshUnavailable, got: {mapped:?}",
        );
    }

    #[test]
    fn non_stub_operation_failed_maps_to_gmsh_variant() {
        let runtime_err =
            GeometryError::OperationFailed("some runtime gmsh failure".into());
        let mapped = map_geometry_error(runtime_err);
        assert!(
            matches!(mapped, RefineError::Gmsh(_)),
            "non-stub OperationFailed must map to RefineError::Gmsh(_), got: {mapped:?}",
        );
    }
}
