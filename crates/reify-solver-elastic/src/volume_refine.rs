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

use std::collections::HashMap;
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
    /// `volume_mesh.vertices.len()` is not a whole multiple of 3, so the flat
    /// buffer does not describe a whole number of XYZ positions (task 4909).
    ///
    /// Raised by [`boundary_surface_mesh`], which copies vertex positions
    /// through by triple. Rejecting rather than truncating mirrors
    /// `reify_eval::compute_targets::elastic_static::volume_mesh_to_solver_mesh`,
    /// which returns `None` on the same condition instead of silently dropping
    /// the trailing partial vertex via truncating integer division.
    MalformedVertexBuffer {
        /// `volume_mesh.vertices.len()`.
        len: usize,
    },
    /// A face of `volume_mesh` is shared by THREE OR MORE elements, so the
    /// mesh has no well-defined two-manifold boundary (task 4909).
    ///
    /// Raised by [`boundary_surface_mesh`]. A free-face extractor keeps the
    /// faces seen exactly once; a face seen twice is interior. A face seen
    /// three or more times fits neither category, and silently dropping it
    /// would emit a surface with a hole — which gmsh reports much later and
    /// far less legibly as "no dim=2 entities after classify+create_geometry;
    /// surface may be open or non-manifold".
    NonManifoldBoundary {
        /// The offending face, as its three vertex indices ascending.
        face: [u32; 3],
        /// How many elements share it (always `>= 3`).
        incident_elements: usize,
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
            RefineError::MalformedVertexBuffer { len } => write!(
                f,
                "malformed vertex buffer: {len} floats is not a whole multiple of \
                 3, so it does not describe a whole number of XYZ positions"
            ),
            RefineError::NonManifoldBoundary {
                face,
                incident_elements,
            } => write!(
                f,
                "non-manifold mesh: face ({}, {}, {}) is shared by {incident_elements} \
                 elements, so the mesh has no well-defined boundary surface",
                face[0], face[1], face[2],
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

/// Position of vertex `v` of `volume_mesh`, widened to `f64`.
///
/// Only [`signed_tet_volume`] reads this — the emitted surface copies the
/// `f32` coordinates through unchanged, so the extractor never round-trips a
/// position through `f64` (see [`boundary_surface_mesh`]'s bit-equality
/// contract).
fn volume_vertex_position(volume_mesh: &VolumeMesh, v: u32) -> [f64; 3] {
    let base = v as usize * 3;
    [
        volume_mesh.vertices[base] as f64,
        volume_mesh.vertices[base + 1] as f64,
        volume_mesh.vertices[base + 2] as f64,
    ]
}

/// Signed volume of tet `[a, b, c, d]`: `dot(d-a, cross(b-a, c-a)) / 6`.
///
/// Positive iff `[a, b, c, d]` is positively oriented, which is the
/// precondition [`boundary_surface_mesh`]'s outward face table assumes. Gmsh
/// does not contractually emit positively-oriented tets, so the sign is
/// checked per element rather than assumed.
fn signed_tet_volume(volume_mesh: &VolumeMesh, a: u32, b: u32, c: u32, d: u32) -> f64 {
    let pa = volume_vertex_position(volume_mesh, a);
    let pb = volume_vertex_position(volume_mesh, b);
    let pc = volume_vertex_position(volume_mesh, c);
    let pd = volume_vertex_position(volume_mesh, d);
    let u = [pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2]];
    let v = [pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2]];
    let w = [pd[0] - pa[0], pd[1] - pa[1], pd[2] - pa[2]];
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    (w[0] * cross[0] + w[1] * cross[1] + w[2] * cross[2]) / 6.0
}

/// The four faces of tet `[a, b, c, d]`, each wound OUTWARD.
///
/// The table `(b,c,d), (a,d,c), (a,b,d), (a,c,b)` is the canonical
/// outward-normal winding for a POSITIVELY oriented tet. When the element is
/// negatively oriented the whole tet is mirrored, so every face's winding is
/// flipped (swap the last two corners) to keep the emitted normals outward.
/// See [`signed_tet_volume`] for why the orientation is measured rather than
/// assumed.
fn outward_tet_faces(volume_mesh: &VolumeMesh, a: u32, b: u32, c: u32, d: u32) -> [[u32; 3]; 4] {
    let mut faces = [[b, c, d], [a, d, c], [a, b, d], [a, c, b]];
    if signed_tet_volume(volume_mesh, a, b, c, d) < 0.0 {
        for face in &mut faces {
            face.swap(1, 2);
        }
    }
    faces
}

/// Orientation-free identity of a triangular face: its three vertex indices,
/// ascending.
///
/// Two tets sharing a face necessarily wind it in OPPOSITE directions (each
/// wants its own outward normal), so a winding-sensitive key would never
/// match them and every face would look free. Sorting collapses both windings
/// onto one key.
fn sorted_face_key(face: [u32; 3]) -> [u32; 3] {
    let mut key = face;
    key.sort_unstable();
    key
}

/// Reconstruct the boundary surface of a tet [`VolumeMesh`] as an
/// outward-wound triangle [`Mesh`].
///
/// # Why this exists
///
/// [`refine_with_size_field`] and [`crate::adaptive::refine_marked_elements`]
/// both take a `surface: &Mesh` argument: the remesher works by re-meshing
/// the volume enclosed by that surface under a new size field, so a caller
/// must supply the boundary its volume mesh came from. On the *realized*
/// (`body : Solid`) path that surface is not available — a realization read
/// handle carries exactly one content variant, and for
/// `solver::elastic_static` that variant is the `VolumeMesh`. This function
/// recovers the surface from the volume mesh instead.
///
/// # Why deriving it from the volume mesh is the RIGHT source, not just the
/// available one
///
/// [`refine_with_size_field_validated`] transfers per-element sizes onto the
/// surface by a NEAREST-VERTEX scan
/// ([`project_volume_to_surface_vertices`]). That transfer is only meaningful
/// when the mesh being refined actually came from the supplied surface. A
/// boundary extracted from the volume mesh has vertices that are an exact
/// (bit-equal) SUBSET of that mesh's vertices, so every nearest-vertex lookup
/// is a distance-0 identity — strictly tighter than an independently
/// tessellated surface of the same solid, whose triangulation would not share
/// vertices with the tet mesh at all.
///
/// # Measured behaviour on real gmsh output (task 4909, libgmsh 4.15.2)
///
/// Seeding a volume from a hand-wound unit cube under a uniform 0.5 size
/// field yields 181 P1 tets; this function extracts 150 triangles over 77
/// vertices from it, which satisfies `V - E + F = 77 - 225 + 150 = 2` — a
/// closed genus-0 manifold. `refine_marked_elements` accepts that extracted
/// boundary and remeshes 181 -> 667 tets when the `x < 0.5` half is marked,
/// so `classify_surfaces` does find 0D corner entities on it (the "no corner
/// sizes applied" failure mode does not occur at this seed density).
///
/// On that same output, 0 of 181 tets were emitted NEGATIVELY oriented, so
/// the orientation swap in [`outward_tet_faces`] was dormant: the canonical
/// face table alone sufficed. The swap is retained because gmsh does not
/// contractually guarantee positive orientation, and is pinned directly by
/// `boundary_surface_mesh_of_a_mirrored_tet_still_emits_outward_faces`.
///
/// # Element order
///
/// P1 and P2 tets are both accepted; a P2 element contributes CORNER-ONLY
/// boundary triangles (its first four indices, gmsh canonical order). The
/// surface is pushed to gmsh as linear triangles regardless of the volume
/// element order requested, so mid-side nodes must not enter it.
///
/// # Errors
///
/// Everything the shared [`tet_shape`] gate rejects —
/// [`RefineError::UnsupportedConnectivity`] for a Hex/Wedge mesh,
/// [`RefineError::MalformedTetIndices`] for an index buffer that is not a
/// whole multiple of the per-element stride, and
/// [`RefineError::InvalidTetIndex`] for an index addressing a vertex that
/// does not exist — plus [`RefineError::MalformedVertexBuffer`] for a vertex
/// buffer that does not describe a whole number of XYZ positions, and
/// [`RefineError::NonManifoldBoundary`] for a mesh with a face shared by
/// three or more elements.
pub fn boundary_surface_mesh(volume_mesh: &VolumeMesh) -> Result<Mesh, RefineError> {
    // Mesh-shape gate: the SAME chokepoint `refine_with_size_field` and
    // `adaptive::refine_marked_elements` run first, rather than a second,
    // divergent validator. It rejects Hex/Wedge, a non-multiple index buffer
    // and out-of-range index VALUES — the last of which is load-bearing here,
    // because `outward_tet_faces` reads `vertices[..]` unguarded.
    let shape = tet_shape(volume_mesh)?;
    // `tet_shape`'s index-range check uses a truncating `vertices.len() / 3`,
    // so a trailing partial vertex slips past it. Reject rather than truncate,
    // mirroring `volume_mesh_to_solver_mesh`'s `is_multiple_of(3)` guard.
    if !volume_mesh.vertices.len().is_multiple_of(3) {
        return Err(RefineError::MalformedVertexBuffer {
            len: volume_mesh.vertices.len(),
        });
    }
    let tet_indices = volume_mesh
        .tet_indices()
        .ok_or(RefineError::UnsupportedConnectivity)?;

    // Pass 1 — enumerate every element face in element order, keeping its
    // outward winding, and tally each face's orientation-free key. Two tets
    // sharing a face necessarily wind it oppositely, so the key must be
    // orientation-free for the tally to see them as the same face.
    //
    // The walk uses `shape.stride` (4 for P1, 10 for P2) and reads only the
    // FIRST FOUR indices of each element — the corner nodes, in gmsh
    // canonical order. Mid-side nodes must not enter the surface: the surface
    // handed to `refine_volume_with_size_field` is pushed as LINEAR
    // `add_elements_2d(.., 2, ..)` triangles regardless of the requested
    // volume element order, so a P2 element still contributes corner-only
    // boundary triangles.
    let mut faces: Vec<[u32; 3]> = Vec::with_capacity(shape.n_elements * 4);
    let mut face_counts: HashMap<[u32; 3], u32> = HashMap::new();
    for tet in tet_indices.chunks_exact(shape.stride) {
        for face in outward_tet_faces(volume_mesh, tet[0], tet[1], tet[2], tet[3]) {
            *face_counts.entry(sorted_face_key(face)).or_insert(0) += 1;
            faces.push(face);
        }
    }

    // Manifoldness gate. A face belongs to one element (boundary) or two
    // (interior); three or more means the mesh has no well-defined
    // two-manifold boundary. Rejecting here turns what gmsh would otherwise
    // report late and opaquely ("no dim=2 entities after
    // classify+create_geometry") into a precise, build-agnostic error naming
    // the offending face.
    //
    // The scan walks `faces` (element order) rather than the map, so the face
    // REPORTED for a mesh with several bad faces is deterministic.
    if let Some(bad) = faces
        .iter()
        .find(|face| face_counts[&sorted_face_key(**face)] > 2)
    {
        let key = sorted_face_key(*bad);
        return Err(RefineError::NonManifoldBoundary {
            face: key,
            incident_elements: face_counts[&key] as usize,
        });
    }

    // Pass 2 — keep exactly the FREE faces (seen once). A face seen twice
    // separates two elements and is interior.
    //
    // The filter walks `faces` (element order), NOT the map, so the emitted
    // face order is a deterministic function of the input mesh and never of
    // `HashMap` iteration order. Determinism here is load-bearing: the
    // adaptive loop's bit-stability invariant (`deterministic: true` in
    // `MeshingOptions`) extends to the surface it remeshes from.
    let kept: Vec<[u32; 3]> = faces
        .into_iter()
        .filter(|face| face_counts[&sorted_face_key(*face)] == 1)
        .collect();

    // Pass 3 — compact. A vertex referenced only by dropped (interior) faces
    // must not survive: gmsh's `classify_surfaces` would see a stray point
    // with no incident triangle, and it would break the "surface vertices are
    // a subset of volume vertices" property the size transfer relies on.
    // Surviving vertices are pushed in ascending ORIGINAL index order, so the
    // remap is stable across runs.
    let vertex_count = volume_mesh.vertices.len() / 3;
    let mut referenced = vec![false; vertex_count];
    for face in &kept {
        for &v in face {
            referenced[v as usize] = true;
        }
    }
    let mut remap = vec![u32::MAX; vertex_count];
    let mut vertices: Vec<f32> = Vec::new();
    for (old, &is_referenced) in referenced.iter().enumerate() {
        if is_referenced {
            remap[old] = (vertices.len() / 3) as u32;
            vertices.extend_from_slice(&volume_mesh.vertices[old * 3..old * 3 + 3]);
        }
    }

    let mut indices: Vec<u32> = Vec::with_capacity(kept.len() * 3);
    for face in &kept {
        for &v in face {
            indices.push(remap[v as usize]);
        }
    }

    Ok(Mesh {
        vertices,
        indices,
        normals: None,
    })
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

    // ---- step-1/2 pins: boundary_surface_mesh, the free-face extractor ----

    /// Single positively-oriented P1 tet: `signed_volume([a,b,c,d]) > 0`
    /// because `dot(d-a, cross(b-a, c-a)) = dot((0,0,1), (0,0,1)) = 1`.
    fn single_tet_mesh() -> VolumeMesh {
        VolumeMesh {
            #[rustfmt::skip]
            vertices: vec![
                0.0, 0.0, 0.0, // 0 = a
                1.0, 0.0, 0.0, // 1 = b
                0.0, 1.0, 0.0, // 2 = c
                0.0, 0.0, 1.0, // 3 = d
            ],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![0, 1, 2, 3],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        }
    }

    /// `(x, y, z)` of surface-mesh vertex `v`, widened to `f64`.
    fn surf_vertex(mesh: &Mesh, v: u32) -> [f64; 3] {
        let base = v as usize * 3;
        [
            mesh.vertices[base] as f64,
            mesh.vertices[base + 1] as f64,
            mesh.vertices[base + 2] as f64,
        ]
    }

    /// Right-hand-rule geometric normal of triangle `t` (NOT normalized —
    /// only its SIGN against an outward reference direction is ever read).
    fn triangle_normal(mesh: &Mesh, t: usize) -> [f64; 3] {
        let p0 = surf_vertex(mesh, mesh.indices[t * 3]);
        let p1 = surf_vertex(mesh, mesh.indices[t * 3 + 1]);
        let p2 = surf_vertex(mesh, mesh.indices[t * 3 + 2]);
        let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let v = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    }

    /// Centroid of triangle `t`.
    fn triangle_centroid(mesh: &Mesh, t: usize) -> [f64; 3] {
        let mut c = [0.0_f64; 3];
        for k in 0..3 {
            let p = surf_vertex(mesh, mesh.indices[t * 3 + k]);
            for a in 0..3 {
                c[a] += p[a] / 3.0;
            }
        }
        c
    }

    /// Mean of every vertex of `volume_mesh` — the interior reference point
    /// an outward face must point AWAY from, for a convex body.
    fn volume_centroid(volume_mesh: &VolumeMesh) -> [f64; 3] {
        let n = volume_mesh.vertices.len() / 3;
        let mut c = [0.0_f64; 3];
        for vertex in volume_mesh.vertices.chunks_exact(3) {
            for (axis, sum) in c.iter_mut().enumerate() {
                *sum += vertex[axis] as f64 / n as f64;
            }
        }
        c
    }

    /// The boundary of a single tet is all four of its faces, each wound
    /// OUTWARD.
    ///
    /// Outwardness is asserted geometrically rather than against a fixed
    /// index table: for a convex body, a face is outward-wound iff its
    /// right-hand-rule normal has a positive dot product with the vector
    /// from the body centroid to the face centroid. That test is what
    /// `refine_volume_with_size_field`'s `classify_surfaces` +
    /// `geo_add_surface_loop` step ultimately depends on (an inward-wound
    /// loop yields no volume), so it is the property worth pinning.
    #[test]
    fn boundary_surface_mesh_of_single_tet_emits_four_outward_wound_faces() {
        let vm = single_tet_mesh();

        let mesh = boundary_surface_mesh(&vm).expect("a single P1 tet is a well-formed tet mesh");

        assert_eq!(
            mesh.vertices.len(),
            12,
            "all 4 tet vertices lie on the boundary: 4 x 3 floats",
        );
        assert_eq!(
            mesh.indices.len(),
            12,
            "a single tet has 4 boundary faces: 4 x 3 indices",
        );

        let body_centroid = volume_centroid(&vm);
        for t in 0..mesh.indices.len() / 3 {
            let n = triangle_normal(&mesh, t);
            let c = triangle_centroid(&mesh, t);
            let outward = [
                c[0] - body_centroid[0],
                c[1] - body_centroid[1],
                c[2] - body_centroid[2],
            ];
            let dot = n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2];
            assert!(
                dot > 0.0,
                "face {t} must be wound OUTWARD: normal={n:?}, outward={outward:?}, dot={dot}",
            );
        }
    }

    // ---- step-3/4 pins: free-face selection + interior-vertex compaction ----

    /// Unit cube `[0,1]^3` (vertices 0..=7, the [`box_surface_mesh`] corner
    /// ordering) fanned into 12 tets about an INTERIOR centre node (vertex 8):
    /// one tet per outward-wound boundary triangle.
    ///
    /// Every one of the 12 boundary triangles appears in exactly one tet, and
    /// every face touching the centre node is shared by exactly two tets — so
    /// the free-face set is precisely the cube boundary, and vertex 8 is
    /// referenced by all 12 elements yet by NO free face. That makes it the
    /// minimal fixture for both halves of the extractor's contract: dropping
    /// interior faces, and compacting away the interior vertex they were the
    /// only carrier of.
    fn cube_fan_with_interior_node() -> VolumeMesh {
        #[rustfmt::skip]
        let corners: [[f32; 3]; 8] = [
            [0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0],
        ];
        #[rustfmt::skip]
        let boundary_tris: [[u32; 3]; 12] = [
            [0, 2, 1], [0, 3, 2], // -Z
            [4, 5, 6], [4, 6, 7], // +Z
            [0, 1, 5], [0, 5, 4], // -Y
            [3, 7, 6], [3, 6, 2], // +Y
            [0, 4, 7], [0, 7, 3], // -X
            [1, 2, 6], [1, 6, 5], // +X
        ];

        let mut vertices: Vec<f32> = corners.iter().flatten().copied().collect();
        vertices.extend_from_slice(&[0.5, 0.5, 0.5]); // 8 = interior centre

        let mut indices = Vec::with_capacity(48);
        for tri in boundary_tris {
            indices.extend_from_slice(&[tri[0], tri[1], tri[2], 8]);
        }

        VolumeMesh {
            vertices,
            connectivity: VolumeConnectivity::Tet {
                indices,
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        }
    }

    /// The extractor must keep only FREE faces (those belonging to exactly
    /// one element) and must compact away vertices no free face references.
    ///
    /// (a) Two tets sharing one face emit 6 triangles, not 8 — a face seen
    ///     twice is interior.
    /// (b) A vertex interior to the volume must not survive into the surface,
    ///     and the kept triangles must be renumbered against the compacted
    ///     vertex buffer.
    /// (c) Every surviving surface vertex must be BIT-EQUAL to some volume
    ///     vertex. This is the property that makes
    ///     `project_volume_to_surface_vertices`' nearest-vertex size transfer
    ///     a distance-0 identity rather than an approximation.
    #[test]
    fn boundary_surface_mesh_drops_shared_faces_and_compacts_interior_vertices() {
        // (a) shared-face drop.
        let bipyramid = two_tet_bipyramid();
        let surf = boundary_surface_mesh(&bipyramid).expect("bipyramid is a well-formed tet mesh");
        assert_eq!(
            surf.indices.len() / 3,
            6,
            "two tets sharing face (0,1,2) must emit 6 free faces, not 8 \
             (got indices={:?})",
            surf.indices,
        );

        // (b) interior-vertex compaction.
        let vm = cube_fan_with_interior_node();
        let surf = boundary_surface_mesh(&vm).expect("cube fan is a well-formed tet mesh");

        assert_eq!(
            surf.indices.len() / 3,
            12,
            "the free-face set of a centre-node cube fan is exactly the 12 \
             boundary triangles",
        );

        let n_surf_verts = surf.vertices.len() / 3;
        for (k, &i) in surf.indices.iter().enumerate() {
            assert!(
                (i as usize) < n_surf_verts,
                "index {k} = {i} is out of range for the compacted vertex \
                 buffer (n={n_surf_verts}) - the kept faces were not renumbered",
            );
        }
        for v in 0..n_surf_verts {
            let p = [
                surf.vertices[v * 3],
                surf.vertices[v * 3 + 1],
                surf.vertices[v * 3 + 2],
            ];
            assert_ne!(
                p,
                [0.5_f32, 0.5, 0.5],
                "the interior centre node must be compacted away, but it \
                 survived at surface vertex {v}",
            );
        }

        // (c) the surface vertex set is a bit-equal SUBSET of the volume's.
        assert!(
            n_surf_verts <= vm.vertices.len() / 3,
            "extraction must never invent vertices: {n_surf_verts} surface vs {} volume",
            vm.vertices.len() / 3,
        );
        let volume_positions: Vec<[f32; 3]> = vm
            .vertices
            .chunks_exact(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        for v in 0..n_surf_verts {
            let p = [
                surf.vertices[v * 3],
                surf.vertices[v * 3 + 1],
                surf.vertices[v * 3 + 2],
            ];
            assert!(
                volume_positions.contains(&p),
                "surface vertex {v} at {p:?} is not bit-equal to any volume \
                 vertex - the distance-0 nearest-vertex identity is broken",
            );
        }
    }

    // ---- step-5/6 pins: the shared tet_shape gate ----

    /// `boundary_surface_mesh` must reject a mis-shaped mesh through the SAME
    /// [`tet_shape`] chokepoint the two remesh entry points already use
    /// (`refine_with_size_field`, `adaptive::refine_marked_elements`), rather
    /// than growing a second, divergent validator.
    ///
    /// The out-of-range case is the load-bearing one: `outward_tet_faces`
    /// reads `volume_mesh.vertices[..]` unguarded to measure the element's
    /// signed volume, so without the gate an index addressing a
    /// non-existent vertex aborts the process instead of returning a
    /// `RefineError`.
    #[test]
    fn boundary_surface_mesh_rejects_malformed_meshes_through_the_shared_tet_shape_gate() {
        // Hex connectivity: the refine pipeline is tet-only.
        let hex = VolumeMesh {
            vertices: vec![0.0_f32; 8 * 3],
            connectivity: VolumeConnectivity::Hex {
                indices: (0..8_u32).collect(),
            },
            normals: None,
            boundary: None,
        };
        assert!(
            matches!(
                boundary_surface_mesh(&hex),
                Err(RefineError::UnsupportedConnectivity),
            ),
            "a Hex mesh must be rejected as UnsupportedConnectivity, got: {:?}",
            boundary_surface_mesh(&hex),
        );

        let wedge = VolumeMesh {
            vertices: vec![0.0_f32; 6 * 3],
            connectivity: VolumeConnectivity::Wedge {
                indices: (0..6_u32).collect(),
            },
            normals: None,
            boundary: None,
        };
        assert!(
            matches!(
                boundary_surface_mesh(&wedge),
                Err(RefineError::UnsupportedConnectivity),
            ),
            "a Wedge mesh must be rejected as UnsupportedConnectivity, got: {:?}",
            boundary_surface_mesh(&wedge),
        );

        // Index buffer that is not a whole multiple of the P1 stride: 5
        // indices describe neither one element nor two. Truncating to one
        // would silently drop a corner.
        let ragged = VolumeMesh {
            vertices: vec![0.0_f32; 5 * 3],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![0, 1, 2, 3, 4],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        };
        assert!(
            matches!(
                boundary_surface_mesh(&ragged),
                Err(RefineError::MalformedTetIndices { len: 5, stride: 4 }),
            ),
            "a ragged index buffer must be rejected as MalformedTetIndices \
             {{ len: 5, stride: 4 }}, got: {:?}",
            boundary_surface_mesh(&ragged),
        );

        // Correctly SHAPED buffer whose index VALUE addresses a vertex that
        // does not exist. Must return before any face enumeration touches
        // `vertices[..]`, i.e. must not panic.
        let out_of_range = VolumeMesh {
            vertices: vec![0.0_f32; 4 * 3],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![0, 1, 2, 9],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        };
        assert!(
            matches!(
                boundary_surface_mesh(&out_of_range),
                Err(RefineError::InvalidTetIndex {
                    vertex_index: 9,
                    vertex_count: 4,
                }),
            ),
            "an out-of-range tet index must be rejected as InvalidTetIndex \
             {{ vertex_index: 9, vertex_count: 4 }} rather than panicking, got: {:?}",
            boundary_surface_mesh(&out_of_range),
        );
    }

    /// A vertex buffer whose length is not a multiple of 3 does not describe a
    /// whole number of XYZ positions. `tet_shape`'s own index-range check uses
    /// a TRUNCATING `vertices.len() / 3`, so the trailing partial vertex slips
    /// past it — hence the separate guard, mirroring
    /// `volume_mesh_to_solver_mesh`'s `is_multiple_of(3)` rejection.
    #[test]
    fn boundary_surface_mesh_rejects_a_vertex_buffer_that_is_not_whole_positions() {
        let mut vm = single_tet_mesh();
        vm.vertices.push(0.0); // 13 floats: 4 positions + 1 stray
        assert!(
            matches!(
                boundary_surface_mesh(&vm),
                Err(RefineError::MalformedVertexBuffer { len: 13 }),
            ),
            "a 13-float vertex buffer must be rejected rather than truncated, got: {:?}",
            boundary_surface_mesh(&vm),
        );
    }

    /// A P2 element contributes CORNER-ONLY boundary triangles.
    ///
    /// The surface is handed to `refine_volume_with_size_field` as LINEAR
    /// `add_elements_2d(.., 2, ..)` triangles regardless of the volume element
    /// order requested, so the six mid-side nodes must not enter the surface —
    /// they are referenced by no free face and are compacted away.
    #[test]
    fn boundary_surface_mesh_of_a_p2_tet_emits_corner_only_faces() {
        // Corners 0..=3 are `single_tet_mesh`'s; 4..=9 are the six edge
        // midpoints in gmsh canonical P2 order (01, 12, 02, 03, 13, 23).
        #[rustfmt::skip]
        let vertices = vec![
            0.0, 0.0, 0.0, // 0
            1.0, 0.0, 0.0, // 1
            0.0, 1.0, 0.0, // 2
            0.0, 0.0, 1.0, // 3
            0.5, 0.0, 0.0, // 4  = mid(0,1)
            0.5, 0.5, 0.0, // 5  = mid(1,2)
            0.0, 0.5, 0.0, // 6  = mid(0,2)
            0.0, 0.0, 0.5, // 7  = mid(0,3)
            0.5, 0.0, 0.5, // 8  = mid(1,3)
            0.0, 0.5, 0.5, // 9  = mid(2,3)
        ];
        let vm = VolumeMesh {
            vertices,
            connectivity: VolumeConnectivity::Tet {
                indices: (0..10_u32).collect(),
                order: ElementOrderTag::P2,
            },
            normals: None,
            boundary: None,
        };

        let surf = boundary_surface_mesh(&vm).expect("a single P2 tet is a well-formed tet mesh");

        assert_eq!(
            surf.indices.len() / 3,
            4,
            "a single tet has 4 boundary faces regardless of element order",
        );
        assert_eq!(
            surf.vertices.len() / 3,
            4,
            "only the 4 CORNER nodes may reach the surface; the 6 mid-side \
             nodes are referenced by no face and must be compacted away",
        );
        for v in 0..surf.vertices.len() / 3 {
            let p = [
                surf.vertices[v * 3],
                surf.vertices[v * 3 + 1],
                surf.vertices[v * 3 + 2],
            ];
            assert!(
                p.iter().all(|c| *c == 0.0 || *c == 1.0),
                "surface vertex {v} at {p:?} is a mid-side node, not a corner",
            );
        }
    }

    /// A mesh whose face is shared by THREE elements has no well-defined
    /// two-manifold boundary and must be rejected, not silently emitted with
    /// a hole.
    ///
    /// Dropping the over-shared face would produce an OPEN surface, which
    /// gmsh reports only much later and far less legibly as "no dim=2
    /// entities after classify+create_geometry; surface may be open or
    /// non-manifold".
    #[test]
    fn boundary_surface_mesh_rejects_a_face_shared_by_three_elements() {
        // Three tets fanned around the shared triangle (0,1,2), with apexes
        // 3, 4 and 5 on both sides and in the plane's normal direction.
        let vm = VolumeMesh {
            #[rustfmt::skip]
            vertices: vec![
                0.0, 0.0,  0.0, // 0 |
                1.0, 0.0,  0.0, // 1 |- shared face (0,1,2)
                0.0, 1.0,  0.0, // 2 |
                0.0, 0.0,  1.0, // 3 apex above
                0.0, 0.0, -1.0, // 4 apex below
                1.0, 1.0,  1.0, // 5 third apex
            ],
            connectivity: VolumeConnectivity::Tet {
                indices: vec![
                    0, 1, 2, 3, //
                    0, 1, 2, 4, //
                    0, 1, 2, 5, //
                ],
                order: ElementOrderTag::P1,
            },
            normals: None,
            boundary: None,
        };

        assert!(
            matches!(
                boundary_surface_mesh(&vm),
                Err(RefineError::NonManifoldBoundary {
                    face: [0, 1, 2],
                    incident_elements: 3,
                }),
            ),
            "a face shared by three elements must be rejected as \
             NonManifoldBoundary, got: {:?}",
            boundary_surface_mesh(&vm),
        );
    }

    /// A NEGATIVELY oriented (mirrored) element must still emit outward-wound
    /// faces — the branch [`outward_tet_faces`]' signed-volume swap exists
    /// for.
    ///
    /// This branch is dormant on real gmsh output (measured: 0 of 181 tets
    /// negatively oriented, see [`boundary_surface_mesh`]'s doc), so without
    /// this test it would be untested code. Gmsh does not contractually
    /// guarantee positive orientation, so the guard is kept and pinned here
    /// rather than removed.
    #[test]
    fn boundary_surface_mesh_of_a_mirrored_tet_still_emits_outward_faces() {
        let mut vm = single_tet_mesh();
        // Swap two corners: same geometry, opposite orientation.
        vm.connectivity = VolumeConnectivity::Tet {
            indices: vec![0, 2, 1, 3],
            order: ElementOrderTag::P1,
        };
        assert!(
            signed_tet_volume(&vm, 0, 2, 1, 3) < 0.0,
            "fixture precondition: [0,2,1,3] must be negatively oriented",
        );

        let mesh = boundary_surface_mesh(&vm).expect("a mirrored tet is still a valid tet mesh");

        assert_eq!(mesh.indices.len(), 12, "a single tet has 4 faces");
        let body_centroid = volume_centroid(&vm);
        for t in 0..mesh.indices.len() / 3 {
            let n = triangle_normal(&mesh, t);
            let c = triangle_centroid(&mesh, t);
            let outward = [
                c[0] - body_centroid[0],
                c[1] - body_centroid[1],
                c[2] - body_centroid[2],
            ];
            let dot = n[0] * outward[0] + n[1] * outward[1] + n[2] * outward[2];
            assert!(
                dot > 0.0,
                "face {t} of a MIRRORED tet must still be wound OUTWARD: \
                 normal={n:?}, outward={outward:?}, dot={dot}",
            );
        }
    }
}
