//! Fill / coverage metrics for a realized tetrahedral [`VolumeMesh`].
//!
//! # Why this exists
//!
//! A tet mesh can be structurally well-formed — every element a valid,
//! positively-oriented tetrahedron, every index in range — and still cover only
//! part of the solid it was asked to fill. That failure mode is invisible to
//! any per-element check; it only shows up when the *total* meshed volume is
//! compared against an independent reference. #6200 was exactly this: gmsh's
//! `classify_surfaces` was handed a 90° feature angle, a box's dihedral angle
//! is *exactly* 90°, and gmsh's sharp-edge test is strictly-greater-than, so
//! the box's own edges were never registered. The resulting B-rep decomposition
//! bounded a region smaller than the box, and HXT dutifully produced a
//! perfectly valid tet mesh of 74–86% of it.
//!
//! # The two reference volumes, and when each is meaningful
//!
//! **`aabb_fill_fraction` — a PRISMATIC-ONLY invariant.** A complete conforming
//! tetrahedralization of a convex polytope *partitions* it, so `Σ|V_tet|`
//! equals the polytope's volume exactly. For an axis-aligned box the AABB of
//! the tet-referenced nodes *is* the box, so `abs_volume_sum == aabb_volume`
//! holds to floating-point exactness. This does **not** generalise: a cylinder
//! legitimately fills only π/4 ≈ 0.785 of its AABB, and any non-convex body
//! fills strictly less than 1.0 by construction. Reading this ratio as a
//! health metric is only valid when the body is known to be prismatic and
//! axis-aligned.
//!
//! **`surface_match_ratio` — the geometry-agnostic one.** The volume enclosed
//! by the *input surface mesh*, via [`enclosed_volume_of_surface`], is a
//! correct reference for **any** closed body, curved or non-convex. A complete
//! tetrahedralization matches it regardless of shape. This is the ratio worth
//! reporting on a production path.
//!
//! Neither is enforced at runtime. `mesh_surface_to_volume_with_diagnostics`
//! surfaces the report non-fatally through `MeshSurfaceToVolumeReport::fill`;
//! no threshold is applied, because no fill floor exists that is simultaneously
//! scale-free and shape-free, and a guessed constant would reject legitimate
//! curved geometry. The regression guard for #6200 lives in this crate's tests.
//!
//! # Gating
//!
//! Deliberately unconditional (no `cfg(has_gmsh)` gate), following the
//! precedent set by the pure helpers in [`crate::mesh_volume`]: this module is
//! `reify_ir` arithmetic with no FFI, so it compiles and is unit-testable in
//! stub builds on hosts without libgmsh.

use reify_ir::{ElementOrderTag, Mesh, VolumeMesh};

/// Fill / coverage metrics for a tetrahedral [`VolumeMesh`].
///
/// The field set deliberately mirrors the measurements taken downstream in
/// #6154, so the producer-side and consumer-side numbers are directly
/// comparable without re-deriving either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TetFillReport {
    /// Total nodes in the mesh's vertex buffer — including any that no element
    /// references. gmsh's readback can carry such orphans; comparing this
    /// against the tet-referenced count is how you spot them.
    pub n_nodes: usize,
    /// Number of tetrahedral elements (P1 and P2 alike — one per element, not
    /// one per node group).
    pub n_tets: usize,
    /// Volume of the axis-aligned bounding box of the **tet-referenced** nodes.
    /// Orphan nodes are excluded deliberately: including them would inflate the
    /// box and understate the fill fraction on an otherwise-correct mesh.
    pub aabb_volume: f64,
    /// `Σ |V_tet|` — total meshed volume, orientation-insensitive.
    pub abs_volume_sum: f64,
    /// `Σ V_tet` with sign retained. Equals [`Self::abs_volume_sum`] exactly
    /// when every element is consistently wound; a smaller magnitude means
    /// inverted elements are cancelling real volume.
    pub signed_volume_sum: f64,
    /// Count of elements with negative signed volume (inverted winding).
    pub inverted_tets: usize,
    /// Tet-referenced nodes lying strictly inside the AABB on **all three**
    /// axes. A prismatic body meshed at a resolution finer than its smallest
    /// extent must have some; zero interior nodes on a body that should have
    /// them is itself diagnostic of a degenerate B-rep decomposition.
    pub strictly_interior_nodes: usize,
}

impl TetFillReport {
    /// `abs_volume_sum / aabb_volume` — 1.0 for a complete tetrahedralization
    /// of an axis-aligned prismatic body.
    ///
    /// **Prismatic-only.** See the module docs: a cylinder legitimately reads
    /// ≈ π/4 here. Returns [`Self::DEGENERATE_RATIO`] rather than NaN/inf when
    /// the AABB has zero volume.
    pub fn aabb_fill_fraction(&self) -> f64 {
        ratio(self.abs_volume_sum, self.aabb_volume)
    }

    /// `abs_volume_sum / surface_volume` — the geometry-agnostic completeness
    /// ratio, 1.0 for a complete tetrahedralization of **any** closed body.
    ///
    /// Pair with [`enclosed_volume_of_surface`] over the surface mesh that was
    /// handed to the mesher. Returns [`Self::DEGENERATE_RATIO`] rather than
    /// NaN/inf when `surface_volume` is zero or non-finite.
    pub fn surface_match_ratio(&self, surface_volume: f64) -> f64 {
        ratio(self.abs_volume_sum, surface_volume)
    }

    /// Sentinel returned by the ratio accessors when the denominator is zero or
    /// non-finite. Chosen so callers can compare, sum, and format the result
    /// without special-casing NaN — a degenerate input reads as "no fill".
    pub const DEGENERATE_RATIO: f64 = 0.0;
}

/// Divide, substituting [`TetFillReport::DEGENERATE_RATIO`] for any result that
/// would not be finite.
fn ratio(numerator: f64, denominator: f64) -> f64 {
    if denominator == 0.0 || !denominator.is_finite() {
        return TetFillReport::DEGENERATE_RATIO;
    }
    let r = numerator / denominator;
    if r.is_finite() {
        r
    } else {
        TetFillReport::DEGENERATE_RATIO
    }
}

/// Signed volume of a tetrahedron: `det[v1-v0 | v2-v0 | v3-v0] / 6`.
///
/// The sign is **retained** (no `.abs()`): it is what makes an inverted element
/// detectable, and what makes the `signed_volume_sum == abs_volume_sum`
/// cross-check meaningful. The reference tet
/// `[(0,0,0), (1,0,0), (0,1,0), (0,0,1)]` measures exactly `+1/6`.
///
/// Adapted from `reify_solver_elastic::result::tet_volume_p1` (copied rather
/// than depended on: `reify-solver-elastic` already depends on this crate, so
/// the reverse edge would close a Cargo cycle).
pub fn tet_signed_volume(c: &[[f64; 3]; 4]) -> f64 {
    let a = [c[1][0] - c[0][0], c[1][1] - c[0][1], c[1][2] - c[0][2]];
    let b = [c[2][0] - c[0][0], c[2][1] - c[0][1], c[2][2] - c[0][2]];
    let d = [c[3][0] - c[0][0], c[3][1] - c[0][1], c[3][2] - c[0][2]];
    // Explicit 3x3 cofactor expansion of det[a | b | d].
    let det = a[0] * (b[1] * d[2] - b[2] * d[1]) - a[1] * (b[0] * d[2] - b[2] * d[0])
        + a[2] * (b[0] * d[1] - b[1] * d[0]);
    det / 6.0
}

/// Volume enclosed by a closed, outward-wound triangle mesh, by the divergence
/// theorem: `Σ a·(b×c) / 6` over triangles.
///
/// Exact for polyhedra (zero tessellation error), so a unit cube yields exactly
/// `1.0`. Because each term is an integral of the origin-tetrahedron `(0,a,b,c)`
/// and never references vertex *identity*, the result is independent of vertex
/// welding — which matters here because `OcctKernel::tessellate` hands the
/// production path a per-face **unwelded** surface, and the repair pre-stage
/// welds it only afterwards.
///
/// The determinant sum is accumulated first and divided once at the end (rather
/// than dividing per triangle, as `reify_kernel_manifold::queries::mass_properties`
/// does) so integral-coordinate inputs stay bit-exact.
///
/// Formula adapted from `crates/reify-kernel-manifold/src/queries.rs:606`
/// `mass_properties`, which is `pub(crate)` and therefore not callable across
/// crates; dev-depending on `reify-kernel-manifold` from here would invert the
/// intended layering.
///
/// Returns `None` when the mesh is not measurable as a triangle soup:
/// - an index buffer whose length is not a whole number of triangles;
/// - any index out of range for the vertex buffer;
/// - a vertex buffer whose length is not a multiple of 3.
///
/// **Why `Option`, and not a skip-the-bad-triangle sum.** This number is the
/// geometry-agnostic REFERENCE that completeness is judged against
/// ([`TetFillReport::surface_match_ratio`], and the acceptance guard in
/// `tests/volume_fill_fraction.rs`). Dropping a malformed triangle shrinks the
/// reference in the *same direction* as an under-filled volume mesh, so a real
/// under-fill would divide out to a ratio near 1.0 — a structurally plausible
/// number carrying a wrong answer, which is exactly the failure mode this
/// module exists to catch. The signature is therefore symmetric with
/// [`tet_fill_report`]: malformed connectivity is `None`, never a quiet
/// approximation.
///
/// A mesh whose winding is inconsistent, or which is not closed, is a
/// *different* class of input: it is well-formed connectivity that produces a
/// meaningless number, and cannot be detected here. Callers must supply a
/// vetted closed outward-wound surface.
pub fn enclosed_volume_of_surface(m: &Mesh) -> Option<f64> {
    if !m.vertices.len().is_multiple_of(3) || !m.indices.len().is_multiple_of(3) {
        return None;
    }
    let n_nodes = m.vertices.len() / 3;
    let mut det_sum = 0.0f64;
    for tri in m.indices.chunks_exact(3) {
        let a = node_at(&m.vertices, tri[0] as usize, n_nodes)?;
        let b = node_at(&m.vertices, tri[1] as usize, n_nodes)?;
        let c = node_at(&m.vertices, tri[2] as usize, n_nodes)?;
        // a · (b × c)
        det_sum += a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0]);
    }
    Some(det_sum / 6.0)
}

/// Read node `i` from a flat f32 XYZ buffer, widened to f64. `None` if out of
/// range.
fn node_at(vertices: &[f32], i: usize, n_nodes: usize) -> Option<[f64; 3]> {
    if i >= n_nodes {
        return None;
    }
    Some([
        vertices[i * 3] as f64,
        vertices[i * 3 + 1] as f64,
        vertices[i * 3 + 2] as f64,
    ])
}

/// Measure fill / coverage of a tetrahedral [`VolumeMesh`].
///
/// Returns `None` when the mesh is not measurable as tets:
/// - `Hex` or `Wedge` connectivity (this report is tet-only);
/// - a vertex buffer whose length is not a multiple of 3;
/// - an index buffer whose length is not a whole number of elements
///   (stride 4 for P1, 10 for P2);
/// - any index out of range for the vertex buffer.
///
/// P2 elements are measured off their **first four** (corner) nodes; the six
/// edge-midpoint nodes carry no additional volume information for a
/// straight-sided element.
///
/// All arithmetic is done in f64. `VolumeMesh::vertices` is `Vec<f32>`, so the
/// storage itself bounds achievable accuracy at ≈ 1.19e-7 relative per
/// coordinate; a tet volume being a degree-3 form, expect ≲ 3.6e-7 relative
/// error against an exact reference.
pub fn tet_fill_report(vm: &VolumeMesh) -> Option<TetFillReport> {
    let indices = vm.tet_indices()?;
    let stride = match vm.element_order()? {
        ElementOrderTag::P1 => 4usize,
        ElementOrderTag::P2 => 10usize,
    };
    if !vm.vertices.len().is_multiple_of(3) || !indices.len().is_multiple_of(stride) {
        return None;
    }
    let n_nodes = vm.vertices.len() / 3;

    let mut abs_volume_sum = 0.0f64;
    let mut signed_volume_sum = 0.0f64;
    let mut inverted_tets = 0usize;
    // AABB is taken over TET-REFERENCED nodes only — see the field docs.
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut referenced = vec![false; n_nodes];

    for elem in indices.chunks_exact(stride) {
        let mut corners = [[0.0f64; 3]; 4];
        for (slot, &idx) in elem[..4].iter().enumerate() {
            let node = node_at(&vm.vertices, idx as usize, n_nodes)?;
            corners[slot] = node;
            referenced[idx as usize] = true;
            for axis in 0..3 {
                min[axis] = min[axis].min(node[axis]);
                max[axis] = max[axis].max(node[axis]);
            }
        }
        // P2 midside nodes are still part of the element; mark them referenced
        // so the orphan-node accounting stays honest, but do NOT let them move
        // the AABB — a curved P2 edge can bow outside the corner hull, and the
        // AABB must stay the corner-defined box the volume sum is compared to.
        for &idx in &elem[4..] {
            if (idx as usize) >= n_nodes {
                return None;
            }
            referenced[idx as usize] = true;
        }

        let v = tet_signed_volume(&corners);
        signed_volume_sum += v;
        abs_volume_sum += v.abs();
        if v < 0.0 {
            inverted_tets += 1;
        }
    }

    let (aabb_volume, strictly_interior_nodes) = if indices.is_empty() {
        (0.0, 0)
    } else {
        let extent = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
        let volume = extent[0] * extent[1] * extent[2];
        let interior = (0..n_nodes)
            .filter(|&i| referenced[i])
            .filter(|&i| {
                let Some(p) = node_at(&vm.vertices, i, n_nodes) else {
                    return false;
                };
                (0..3).all(|axis| p[axis] > min[axis] && p[axis] < max[axis])
            })
            .count();
        (volume, interior)
    };

    Some(TetFillReport {
        n_nodes,
        n_tets: indices.len() / stride,
        aabb_volume,
        abs_volume_sum,
        signed_volume_sum,
        inverted_tets,
        strictly_interior_nodes,
    })
}
