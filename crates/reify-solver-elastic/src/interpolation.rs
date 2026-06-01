//! Point-in-tet location and P1 shape-function evaluation primitives.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #13.
//!
//! # Scope
//!
//! Ships the Rust math primitives the engine integration layer (PRD §16)
//! wraps as `Field<Point3<Length>, Vector3<Length>>` displacement queries:
//! "at any query point p: locate containing element, interpolate u via
//! element shape functions" (PRD §13). The public surface is plain `f64`
//! types — `Field`-typed wrapping happens at the engine layer, mirroring
//! the pattern in `shell_result.rs` for shells.
//!
//! # Public surface
//!
//! - [`barycentric_p1`] — barycentric coordinates of a query point in a
//!   P1 tetrahedron via the affine reference→physical map.
//! - [`point_in_tet_p1`] — tolerant point-in-tet inclusion test.
//! - [`interpolate_p1_at_point`] — linear interpolation of nodal vector
//!   values at a query point inside a P1 tet.
//! - [`locate_element_p1`] + [`LocatableTet`] — linear-scan search for the
//!   first P1 element containing a query point.

use crate::math::{MIN_JACOBIAN_DET, inverse_transpose_3x3};

/// Compute the four P1 barycentric coordinates of `p` with respect to
/// the tetrahedron `phys_nodes`.
///
/// Returns `[N_0(p), N_1(p), N_2(p), N_3(p)]` — for a P1 (linear)
/// tetrahedron the shape functions ARE the barycentric coordinates.
/// They sum to 1 exactly (partition of unity) by construction of the
/// affine map; entries lie in `[0, 1]` iff `p` is inside the tet.
///
/// # Algorithm
///
/// Solve the affine system `p − v_0 = J · ξ` where
/// `J = [v_1 − v_0 | v_2 − v_0 | v_3 − v_0]` is the 3×3 Jacobian of the
/// reference→physical map and `ξ = (ξ₁, ξ₂, ξ₃)` are the parametric
/// (reference) coordinates. Returns
/// `[1 − ξ₁ − ξ₂ − ξ₃, ξ₁, ξ₂, ξ₃]`.
///
/// `phys_nodes` is in the canonical reference order `(0,0,0), (1,0,0),
/// (0,1,0), (0,0,1)` (mirrors `TetP1`); see `assembly/tet.rs` for the
/// matching connectivity convention.
///
/// # Preconditions
///
/// The tet must be non-degenerate (`det J != 0`). A degenerate tet
/// returns non-finite barycentric coordinates; diagnosing that
/// condition is PRD task #21's job.
pub fn barycentric_p1(phys_nodes: &[[f64; 3]; 4], p: [f64; 3]) -> [f64; 4] {
    // J = [v1−v0 | v2−v0 | v3−v0] — column-stored as J[i][j] = (v_{j+1} − v_0)[i].
    let v0 = phys_nodes[0];
    let mut j_mat = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        j_mat[i][0] = phys_nodes[1][i] - v0[i];
        j_mat[i][1] = phys_nodes[2][i] - v0[i];
        j_mat[i][2] = phys_nodes[3][i] - v0[i];
    }
    let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
        - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
        + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);
    // Degenerate-element guard (debug-only). `det.is_normal()` catches
    // ±0, ±∞, NaN, and subnormals; the absolute-value floor catches the
    // merely-tiny case where division by `det` in `inverse_transpose_3x3`
    // would inflate FP error to dominate the barycentric coords. Mirrors
    // the convention used by `assembly/tet.rs:182` for the same primitive.
    debug_assert!(
        det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
        "degenerate tet in barycentric_p1: |det J| = {} (must be > {} \
         and finite — see PRD task #21 for the future diagnostic path)",
        det.abs(),
        MIN_JACOBIAN_DET,
    );
    // Solve J · ξ = (p − v0). With J⁻¹ from the cofactor formula,
    // ξ = J⁻¹ (p − v0). We use the same primitive as the
    // assembly path: `inverse_transpose_3x3` returns J⁻ᵀ; therefore
    // (J⁻¹)[i][j] = (J⁻ᵀ)[j][i].
    let inv_t = inverse_transpose_3x3(&j_mat, det);
    let rhs = [p[0] - v0[0], p[1] - v0[1], p[2] - v0[2]];
    let mut xi = [0.0_f64; 3];
    for i in 0..3 {
        let mut s = 0.0;
        for j in 0..3 {
            // (J⁻¹)[i][j] = inv_t[j][i]
            s += inv_t[j][i] * rhs[j];
        }
        xi[i] = s;
    }
    [1.0 - xi[0] - xi[1] - xi[2], xi[0], xi[1], xi[2]]
}

/// Tolerant point-in-tet inclusion test for a P1 tetrahedron.
///
/// Returns `true` iff every entry of [`barycentric_p1`]`(phys_nodes, p)`
/// lies in `[−tol, 1 + tol]`. The partition-of-unity sum is `1` exactly
/// (a property of the affine map), so the four-bound check is sufficient
/// — we don't also need to assert the sum.
///
/// `tol` is an **absolute slack on the barycentric coordinates** —
/// barycentric coords live in `[0, 1]` for any non-degenerate tet
/// regardless of physical scale, so `tol` is scale-invariant by
/// construction (effectively a relative slack in tet-edge-length units;
/// callers do **not** scale it by physical edge length). A query point
/// that is in the tet up to floating-point round-off (e.g. a barycentric
/// coord of `−1e-12` along an edge) is classified as inside when
/// `tol = 1e-9`. Use `tol = 0.0` for a strict inclusion test.
///
/// # Preconditions
///
/// The tet must be non-degenerate (`det J != 0`); see [`barycentric_p1`].
pub fn point_in_tet_p1(phys_nodes: &[[f64; 3]; 4], p: [f64; 3], tol: f64) -> bool {
    let bary = barycentric_p1(phys_nodes, p);
    bary.iter().all(|&n| n >= -tol && n <= 1.0 + tol)
}

/// Interpolate a vector-valued nodal field at a query point inside a P1
/// tetrahedron.
///
/// Returns `Σ_i N_i(p) · nodal_values[i]` componentwise, where `N_i` are
/// the P1 barycentric shape functions from [`barycentric_p1`].
///
/// At each vertex `v_i` the result equals `nodal_values[i]` exactly
/// (Kronecker δ); at the centroid it equals the arithmetic mean of the
/// four nodal values (partition of unity).
///
/// # Engine wrapping
///
/// This is the building block the engine layer (PRD §16) wraps as a
/// `Field<Point3<Length>, Vector3<Length>>` displacement evaluator: at
/// any query point `p`, [`locate_element_p1`] finds the containing
/// element, then this function interpolates the per-element nodal
/// displacement vector `u_e` to `p`. Mirrors the `shell_result.rs`
/// pattern of shipping plain-`f64` Rust primitives that the engine
/// later wraps as `Field`-typed values.
///
/// # Preconditions
///
/// The tet must be non-degenerate (`det J != 0`); see [`barycentric_p1`].
/// `p` may lie outside the tet — the interpolant is a well-defined
/// polynomial on all of `R^3`, but the result is then an **extrapolation**
/// rather than an interpolation. Callers that want strict-inside behaviour
/// should gate on [`point_in_tet_p1`].
pub fn interpolate_p1_at_point(
    phys_nodes: &[[f64; 3]; 4],
    nodal_values: &[[f64; 3]; 4],
    p: [f64; 3],
) -> [f64; 3] {
    let bary = barycentric_p1(phys_nodes, p);
    let mut out = [0.0_f64; 3];
    for i in 0..4 {
        for k in 0..3 {
            out[k] += bary[i] * nodal_values[i][k];
        }
    }
    out
}

/// Connectivity carrier for [`locate_element_p1`].
///
/// Borrows the per-element 4-vertex physical-node array from the parent
/// mesh; lets the caller assemble a `Vec<LocatableTet>` for a search
/// without cloning. Mirrors the lifetime-borrowed-slice layout used by
/// [`crate::assembly::AssemblyElement`] and [`crate::result::StressElement`].
#[derive(Debug, Clone, Copy)]
pub struct LocatableTet<'a> {
    /// 4 vertex positions in canonical reference order
    /// `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`.
    pub phys_nodes: &'a [[f64; 3]; 4],
}

/// Linear-scan search for the first P1 element containing `p`.
///
/// Returns `Some(i)` for the lowest index `i` whose
/// [`point_in_tet_p1`]`(elements[i].phys_nodes, p, tol)` is true; `None`
/// if no element contains the point.
///
/// # Complexity
///
/// O(n_elements) per query. The PRD §13 contract does not pin a
/// complexity bound; the engine integration layer (PRD §16) is the
/// natural home for caching a BVH/octree across multiple
/// field-evaluation queries on the same mesh — putting a spatial index
/// in this primitive would couple solver internals to acceleration data
/// structures with no clear ownership story. If GUI probe-point queries
/// surface this as a bottleneck, a `LocatedTets` wrapper can be added at
/// the engine layer (or as a separate helper here) without changing
/// this primitive's signature.
pub fn locate_element_p1(elements: &[LocatableTet<'_>], p: [f64; 3], tol: f64) -> Option<usize> {
    for (i, el) in elements.iter().enumerate() {
        if point_in_tet_p1(el.phys_nodes, p, tol) {
            return Some(i);
        }
    }
    None
}

// ── BVH spatial index ─────────────────────────────────────────────────────────

/// Axis-aligned bounding box used internally by [`TetSpatialIndex`].
#[derive(Debug, Clone, Copy)]
struct Aabb {
    min: [f64; 3],
    max: [f64; 3],
}

impl Aabb {
    #[inline]
    fn contains(&self, p: [f64; 3]) -> bool {
        p[0] >= self.min[0]
            && p[0] <= self.max[0]
            && p[1] >= self.min[1]
            && p[1] <= self.max[1]
            && p[2] >= self.min[2]
            && p[2] <= self.max[2]
    }
}

/// A node in the flat BVH array.
#[derive(Debug, Clone)]
enum BvhNode {
    Internal {
        aabb: Aabb,
        left: usize,
        right: usize,
    },
    Leaf {
        aabb: Aabb,
        /// Inclusive start into `TetSpatialIndex::perm`.
        start: usize,
        /// Exclusive end into `TetSpatialIndex::perm`.
        end: usize,
    },
}

/// A binary BVH spatial index over a P1-tet mesh.
///
/// Accelerates element-location queries from O(n\_elems) to O(log n\_elems)
/// per query while guaranteeing bit-identical results to the linear scan in
/// [`locate_element_p1`].
///
/// # Bit-identical contract
///
/// `locate` and `locate_counted` return the **minimum** element index among
/// all AABB-candidate elements that pass `point_in_tet_p1(.., tol)`.  This
/// matches the linear scan's "first-hit" behaviour because the linear scan
/// also returns the lowest-index containing element; on shared-face boundary
/// points (where two elements both pass) the minimum index is selected in
/// both paths.
///
/// # AABB padding
///
/// Each element AABB is padded by
/// `δ = tol * (max_k − min_k) + TINY_ABS_FLOOR`
/// per axis, making the bounding boxes conservative: every point that
/// `point_in_tet_p1(.., tol)` accepts is guaranteed to lie inside the padded
/// AABB.  Over-padding only adds a few extra `point_in_tet_p1` evaluations;
/// under-padding (not possible here) would silently miss the correct element.
pub struct TetSpatialIndex {
    nodes: Vec<BvhNode>,
    root: usize,
    /// Element-index permutation; leaves reference contiguous sub-ranges.
    perm: Vec<usize>,
}

impl TetSpatialIndex {
    /// Maximum elements per BVH leaf.
    const LEAF_MAX: usize = 8;
    /// Absolute padding floor: prevents zero padding on degenerate (flat) faces.
    const TINY_ABS_FLOOR: f64 = 1e-12;

    /// Build a BVH over the mesh defined by `nodes` and `elems`.
    ///
    /// `tol` controls AABB padding so that the padded boxes conservatively
    /// contain every point accepted by `point_in_tet_p1(.., tol)`.
    pub fn build(nodes: &[[f64; 3]], elems: &[[usize; 4]], tol: f64) -> Self {
        let n = elems.len();
        if n == 0 {
            return TetSpatialIndex { nodes: vec![], root: 0, perm: vec![] };
        }

        // Per-element padded AABB.
        let elem_aabbs: Vec<Aabb> = elems
            .iter()
            .map(|conn| {
                let pts = [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]], nodes[conn[3]]];
                let mut mn = pts[0];
                let mut mx = pts[0];
                for pt in &pts[1..] {
                    for k in 0..3 {
                        if pt[k] < mn[k] {
                            mn[k] = pt[k];
                        }
                        if pt[k] > mx[k] {
                            mx[k] = pt[k];
                        }
                    }
                }
                // Pad per axis: δ = tol * extent + tiny_floor
                for k in 0..3 {
                    let pad = tol * (mx[k] - mn[k]) + Self::TINY_ABS_FLOOR;
                    mn[k] -= pad;
                    mx[k] += pad;
                }
                Aabb { min: mn, max: mx }
            })
            .collect();

        // Centroid of each padded AABB (used for median-split sorting).
        let centroids: Vec<[f64; 3]> = elem_aabbs
            .iter()
            .map(|ab| {
                [
                    0.5 * (ab.min[0] + ab.max[0]),
                    0.5 * (ab.min[1] + ab.max[1]),
                    0.5 * (ab.min[2] + ab.max[2]),
                ]
            })
            .collect();

        let mut perm: Vec<usize> = (0..n).collect();
        let mut bvh_nodes: Vec<BvhNode> = Vec::with_capacity(2 * n);

        let root = Self::build_recursive(
            &mut perm,
            0,
            n,
            &elem_aabbs,
            &centroids,
            &mut bvh_nodes,
        );

        TetSpatialIndex { nodes: bvh_nodes, root, perm }
    }

    fn build_recursive(
        perm: &mut Vec<usize>,
        start: usize,
        end: usize,
        elem_aabbs: &[Aabb],
        centroids: &[[f64; 3]],
        bvh_nodes: &mut Vec<BvhNode>,
    ) -> usize {
        // Union AABB of this range.
        let mut union_aabb = elem_aabbs[perm[start]];
        for i in (start + 1)..end {
            let ab = &elem_aabbs[perm[i]];
            for k in 0..3 {
                if ab.min[k] < union_aabb.min[k] {
                    union_aabb.min[k] = ab.min[k];
                }
                if ab.max[k] > union_aabb.max[k] {
                    union_aabb.max[k] = ab.max[k];
                }
            }
        }

        let count = end - start;
        if count <= Self::LEAF_MAX {
            let idx = bvh_nodes.len();
            bvh_nodes.push(BvhNode::Leaf { aabb: union_aabb, start, end });
            return idx;
        }

        // Choose split axis: longest centroid extent.
        let mut axis = 0;
        let mut max_extent = -1.0_f64;
        for k in 0..3 {
            let lo = perm[start..end]
                .iter()
                .map(|&i| centroids[i][k])
                .fold(f64::INFINITY, f64::min);
            let hi = perm[start..end]
                .iter()
                .map(|&i| centroids[i][k])
                .fold(f64::NEG_INFINITY, f64::max);
            let ext = hi - lo;
            if ext > max_extent {
                max_extent = ext;
                axis = k;
            }
        }

        // Median split: sort perm[start..end] by centroid along `axis`.
        perm[start..end].sort_unstable_by(|&a, &b| {
            centroids[a][axis]
                .partial_cmp(&centroids[b][axis])
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mid = start + count / 2;

        // Reserve this node's slot before recursing (recursion pushes more nodes).
        let this_idx = bvh_nodes.len();
        bvh_nodes.push(BvhNode::Internal { aabb: union_aabb, left: 0, right: 0 }); // placeholder

        let left = Self::build_recursive(perm, start, mid, elem_aabbs, centroids, bvh_nodes);
        let right = Self::build_recursive(perm, mid, end, elem_aabbs, centroids, bvh_nodes);

        // Back-fill the child indices now that they are known.
        bvh_nodes[this_idx] = BvhNode::Internal { aabb: union_aabb, left, right };
        this_idx
    }

    /// Return the minimum element index `e` with `point_in_tet_p1(.., tol) == true`,
    /// or `None` if no element contains `p`.
    ///
    /// Result is bit-identical to [`locate_element_p1`] for every query point.
    pub fn locate(
        &self,
        nodes: &[[f64; 3]],
        elems: &[[usize; 4]],
        p: [f64; 3],
        tol: f64,
    ) -> Option<usize> {
        self.locate_counted(nodes, elems, p, tol).0
    }

    /// Like [`locate`] but also returns the number of `point_in_tet_p1`
    /// evaluations performed (for deterministic performance assertions).
    pub fn locate_counted(
        &self,
        nodes: &[[f64; 3]],
        elems: &[[usize; 4]],
        p: [f64; 3],
        tol: f64,
    ) -> (Option<usize>, usize) {
        if self.nodes.is_empty() {
            return (None, 0);
        }
        let mut best: Option<usize> = None;
        let mut count: usize = 0;
        Self::traverse(
            &self.nodes,
            self.root,
            &self.perm,
            nodes,
            elems,
            p,
            tol,
            &mut best,
            &mut count,
        );
        (best, count)
    }

    fn traverse(
        bvh_nodes: &[BvhNode],
        node_idx: usize,
        perm: &[usize],
        nodes: &[[f64; 3]],
        elems: &[[usize; 4]],
        p: [f64; 3],
        tol: f64,
        best: &mut Option<usize>,
        count: &mut usize,
    ) {
        match &bvh_nodes[node_idx] {
            BvhNode::Internal { aabb, left, right } => {
                if !aabb.contains(p) {
                    return;
                }
                Self::traverse(bvh_nodes, *left, perm, nodes, elems, p, tol, best, count);
                Self::traverse(bvh_nodes, *right, perm, nodes, elems, p, tol, best, count);
            }
            BvhNode::Leaf { aabb, start, end } => {
                if !aabb.contains(p) {
                    return;
                }
                for &elem_idx in &perm[*start..*end] {
                    let conn = &elems[elem_idx];
                    let phys4: [[f64; 3]; 4] = [
                        nodes[conn[0]],
                        nodes[conn[1]],
                        nodes[conn[2]],
                        nodes[conn[3]],
                    ];
                    *count += 1;
                    if point_in_tet_p1(&phys4, p, tol) {
                        *best = Some(match *best {
                            Some(prev) => prev.min(elem_idx),
                            None => elem_idx,
                        });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod bvh_tests {
    use super::*;

    // ── Shared fixture: 6-tet Freudenthal box tiling [0,1]³ ─────────────────
    fn box_nodes() -> Vec<[f64; 3]> {
        vec![
            [0.0, 0.0, 0.0], // 0
            [1.0, 0.0, 0.0], // 1
            [1.0, 1.0, 0.0], // 2
            [0.0, 1.0, 0.0], // 3
            [0.0, 0.0, 1.0], // 4
            [1.0, 0.0, 1.0], // 5
            [1.0, 1.0, 1.0], // 6
            [0.0, 1.0, 1.0], // 7
        ]
    }
    fn box_elems() -> Vec<[usize; 4]> {
        vec![
            [0, 1, 2, 6], // T0
            [0, 2, 3, 6], // T1
            [0, 5, 1, 6], // T2
            [0, 3, 7, 6], // T3
            [0, 4, 5, 6], // T4
            [0, 7, 4, 6], // T5
        ]
    }
    fn tet_centroid(nodes: &[[f64; 3]], conn: &[usize; 4]) -> [f64; 3] {
        let mut c = [0.0_f64; 3];
        for &i in conn {
            for k in 0..3 {
                c[k] += nodes[i][k];
            }
        }
        for k in 0..3 {
            c[k] /= 4.0;
        }
        c
    }

    /// Build a `Vec<LocatableTet>` from node + connectivity arrays.
    /// The `phys` backing store must outlive the returned slice.
    fn make_locatable<'a>(
        nodes: &[[f64; 3]],
        elems: &[[usize; 4]],
        phys: &'a mut Vec<[[f64; 3]; 4]>,
    ) -> Vec<LocatableTet<'a>> {
        *phys = elems
            .iter()
            .map(|conn| {
                [nodes[conn[0]], nodes[conn[1]], nodes[conn[2]], nodes[conn[3]]]
            })
            .collect();
        phys.iter().map(|pn| LocatableTet { phys_nodes: pn }).collect()
    }

    /// Step-3: locate_counted parity + count assertions.
    ///
    /// Asserts:
    /// - `locate_counted` returns the same `Option<usize>` as `locate` for
    ///   inside, boundary, and outside points.
    /// - Far-outside point → count = 0 (root AABB culls all subtrees).
    /// - Inside point → count ≥ 1 (at least the containing tet was tested).
    #[test]
    fn locate_counted_result_matches_locate_and_count_contract() {
        // Two disjoint tets with well-separated x extents:
        //   Tet0: (0,0,0),(1,0,0),(0,1,0),(0,0,1) — x in [0,1]
        //   Tet1: (2,0,0),(3,0,0),(2,1,0),(2,0,1) — x in [2,3]
        let nodes: Vec<[f64; 3]> = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [2.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
            [2.0, 0.0, 1.0],
        ];
        let elems: Vec<[usize; 4]> = vec![
            [0, 1, 2, 3], // Tet0
            [4, 5, 6, 7], // Tet1
        ];
        let tol = 1e-9_f64;
        let idx = TetSpatialIndex::build(&nodes, &elems, tol);

        // (1) Centroid of Tet0 — inside.
        let p_in = [0.25_f64, 0.25, 0.25];
        let (opt_loc, _) = idx.locate_counted(&nodes, &elems, p_in, tol);
        let (opt_cnt, cnt_in) = idx.locate_counted(&nodes, &elems, p_in, tol);
        assert_eq!(opt_loc, idx.locate(&nodes, &elems, p_in, tol), "inside: parity");
        assert_eq!(opt_cnt, Some(0), "inside: must find Tet0");
        assert!(cnt_in >= 1, "inside: count ≥ 1 (at least Tet0 tested)");

        // (2) Centroid of Tet1 — inside.
        let p_in1 = [2.25_f64, 0.25, 0.25];
        let (opt1, _) = idx.locate_counted(&nodes, &elems, p_in1, tol);
        assert_eq!(opt1, idx.locate(&nodes, &elems, p_in1, tol), "Tet1 centroid: parity");
        assert_eq!(opt1, Some(1), "Tet1 centroid: must find Tet1");

        // (3) Far-outside point — no AABB contains it → count = 0.
        let p_out = [10.0_f64, 10.0, 10.0];
        let (opt_out, cnt_out) = idx.locate_counted(&nodes, &elems, p_out, tol);
        assert_eq!(opt_out, None, "far outside: None");
        assert_eq!(cnt_out, 0, "far outside: count must be 0 (root AABB culls all)");
    }

    /// Step-1 RED: TetSpatialIndex does not exist yet; this test fails to compile.
    #[test]
    fn tet_spatial_index_matches_linear_oracle_for_all_cases() {
        let nodes = box_nodes();
        let elems = box_elems();
        let tol = 1e-9_f64;

        // Build BVH index.
        let idx = TetSpatialIndex::build(&nodes, &elems, tol);

        // Build oracle.
        let mut phys: Vec<[[f64; 3]; 4]> = Vec::new();
        let locatable = make_locatable(&nodes, &elems, &mut phys);

        let check = |label: &str, p: [f64; 3]| {
            let bvh = idx.locate(&nodes, &elems, p, tol);
            let oracle = locate_element_p1(&locatable, p, tol);
            assert_eq!(
                bvh, oracle,
                "BVH != oracle at {label} p={p:?}: bvh={bvh:?} oracle={oracle:?}",
            );
        };

        // (a) Centroid of each tet → strictly interior, unique hit.
        for (ti, conn) in elems.iter().enumerate() {
            check(
                &format!("T{ti} centroid"),
                tet_centroid(&nodes, conn),
            );
        }

        // (b) Shared-face boundary between T0 (idx 0) and T1 (idx 1):
        // face nodes 0=(0,0,0), 2=(1,1,0), 6=(1,1,1) → face centroid = (2/3, 2/3, 1/3).
        // Both T0 and T1 contain this point within tol.  Oracle returns the
        // LOWER index (T0=0); BVH must also return 0 via min-index selection.
        check("shared-face T0/T1", [2.0 / 3.0, 2.0 / 3.0, 1.0 / 3.0]);

        // (c) Near-vertex point just-outside within tol: (-1e-12, 0, 0) lies
        // just outside T0's v0=(0,0,0) face (bary[0] ≈ 1 + 1e-12/Δ, well
        // within tol=1e-9 for unit-scale edge).  AABB padding must capture it.
        check("near-vertex within-tol", [-1e-12, 0.0, 0.0]);

        // (d) Far-outside point → None in both BVH and oracle.
        check("far outside", [10.0, 10.0, 10.0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 1e-12;

    /// Canonical unit reference tet: vertices `(0,0,0), (1,0,0), (0,1,0),
    /// (0,0,1)` with reference-tet volume `1/6`. Mirrors the
    /// `UNIT_TET_P1` fixture in `assembly/tet.rs`.
    const UNIT_TET_P1: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    #[test]
    fn locate_element_p1_returns_first_containing_index_or_none_for_outside() {
        // Two unit tets sharing the face on the plane y + z = 0 ... actually
        // pick two disjoint-interior tets that tile a small region. The
        // simplest fixture: take the canonical UNIT_TET_P1 and a translated
        // copy that occupies (1..2)×(0..1)×(0..1)-style region.
        //
        // Element 0: canonical unit tet. Centroid = (0.25, 0.25, 0.25).
        let tet0: [[f64; 3]; 4] = UNIT_TET_P1;
        // Element 1: translated by (+1, 0, 0). Centroid = (1.25, 0.25, 0.25).
        let tet1: [[f64; 3]; 4] = [
            [1.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 0.0, 1.0],
        ];

        let elements = [
            LocatableTet { phys_nodes: &tet0 },
            LocatableTet { phys_nodes: &tet1 },
        ];

        // Centroid of element 0 → Some(0).
        assert_eq!(
            locate_element_p1(&elements, [0.25, 0.25, 0.25], 1e-9),
            Some(0),
            "centroid of element 0 must locate Some(0)",
        );
        // Centroid of element 1 → Some(1).
        assert_eq!(
            locate_element_p1(&elements, [1.25, 0.25, 0.25], 1e-9),
            Some(1),
            "centroid of element 1 must locate Some(1)",
        );
        // Faraway point → None.
        assert_eq!(
            locate_element_p1(&elements, [10.0, 10.0, 10.0], 1e-9),
            None,
            "faraway point must locate None",
        );
    }

    #[test]
    fn interpolate_p1_at_point_recovers_nodal_values_at_vertices_and_is_linear() {
        // Non-trivial nodal values: distinct triples per node so
        // permutations would surface as a test failure.
        let nodal_values: [[f64; 3]; 4] = [
            [1.0, 2.0, 3.0],
            [4.0, 5.0, 6.0],
            [7.0, 8.0, 9.0],
            [10.0, 11.0, 12.0],
        ];

        // (a) At each vertex v_i, the interpolant must recover nodal_values[i] exactly.
        for (i, v) in UNIT_TET_P1.iter().enumerate() {
            let interp = interpolate_p1_at_point(&UNIT_TET_P1, &nodal_values, *v);
            for k in 0..3 {
                assert!(
                    (interp[k] - nodal_values[i][k]).abs() < TOL,
                    "vertex {i} interp[{k}] = {} expected {}",
                    interp[k],
                    nodal_values[i][k],
                );
            }
        }

        // (b) At the centroid, the interpolant equals the arithmetic mean
        //     of the four nodal values componentwise (partition-of-unity
        //     consequence: each N_i = 1/4 ⇒ interp = mean).
        let centroid = [0.25_f64, 0.25, 0.25];
        let interp = interpolate_p1_at_point(&UNIT_TET_P1, &nodal_values, centroid);
        for k in 0..3 {
            let expected = 0.25
                * (nodal_values[0][k]
                    + nodal_values[1][k]
                    + nodal_values[2][k]
                    + nodal_values[3][k]);
            assert!(
                (interp[k] - expected).abs() < TOL,
                "centroid interp[{k}] = {} expected mean {expected}",
                interp[k],
            );
        }
    }

    #[test]
    fn point_in_tet_p1_includes_interior_excludes_exterior_within_tolerance() {
        // Centroid is inside.
        assert!(
            point_in_tet_p1(&UNIT_TET_P1, [0.25, 0.25, 0.25], 1e-9),
            "centroid must be inside",
        );
        // Vertex (0,0,0) is on the boundary; with positive tolerance, it
        // must be classified as inside (boundary points pass).
        assert!(
            point_in_tet_p1(&UNIT_TET_P1, [0.0, 0.0, 0.0], 1e-9),
            "vertex (0,0,0) must be inside (boundary, with tolerance)",
        );
        // (0.5, 0.5, 0.5): barycentric ξ = (0.5, 0.5, 0.5), N₀ = -0.5 < 0
        // ⇒ outside (well beyond the 1e-9 tolerance).
        assert!(
            !point_in_tet_p1(&UNIT_TET_P1, [0.5, 0.5, 0.5], 1e-9),
            "(0.5,0.5,0.5) must be outside",
        );
        // Just-outside-vertex: barycentric N₁ = -1e-12 (within 1e-9 tol),
        // so this passes as inside.
        assert!(
            point_in_tet_p1(&UNIT_TET_P1, [-1e-12, 0.0, 0.0], 1e-9),
            "(-1e-12, 0, 0) must be inside (within tolerance)",
        );
    }

    #[test]
    fn barycentric_and_interpolate_recover_affine_field_on_sheared_translated_tet() {
        // Catches a J⁻ᵀ-vs-J⁻¹ index swap that the unit-tet (J = I) tests
        // can't detect, because for J = I the swap is a no-op. Use a
        // sheared, translated, scaled tet where J is non-symmetric and
        // index transposes would surface as wrong barycentric weights /
        // wrong interpolant values.
        //
        // Vertices chosen so J = [v1−v0 | v2−v0 | v3−v0] is diagonal but
        // not identity (different scale per axis), with a translation:
        //   v0 = (1, 0, 0)
        //   v1 = (3, 0, 0) ⇒ v1−v0 = (2, 0, 0)
        //   v2 = (1, 4, 0) ⇒ v2−v0 = (0, 4, 0)
        //   v3 = (1, 0, 5) ⇒ v3−v0 = (0, 0, 5)
        // To also detect a transpose, perturb v3 to add a non-zero off-axis:
        //   v3 = (1, 0, 5) → keep v3−v0 axis-aligned but rotate the test
        //   point through an off-diagonal direction; equivalently, add a
        //   shear by setting v3 = (2, 0, 5) ⇒ v3−v0 = (1, 0, 5).
        let phys_nodes: [[f64; 3]; 4] = [
            [1.0, 0.0, 0.0],
            [3.0, 0.0, 0.0],
            [1.0, 4.0, 0.0],
            [2.0, 0.0, 5.0],
        ];

        // (1) Partition of unity: at any interior point, the four
        //     barycentric coords must sum to 1 within FP tolerance.
        //     Pick a point inside the tet using known reference coords
        //     ξ = (0.2, 0.3, 0.4) ⇒ p = v0 + 0.2·(v1−v0) + 0.3·(v2−v0)
        //     + 0.4·(v3−v0) = (1, 0, 0) + (0.4, 0, 0) + (0, 1.2, 0) +
        //     (0.4, 0, 2.0) = (1.8, 1.2, 2.0).
        let p = [1.8_f64, 1.2, 2.0];
        let bary = barycentric_p1(&phys_nodes, p);
        let sum: f64 = bary.iter().sum();
        assert!(
            (sum - 1.0).abs() < TOL,
            "sheared-tet bary sum = {sum}, expected 1.0 (partition of unity)",
        );
        // The expected weights are [1 − 0.9, 0.2, 0.3, 0.4] = [0.1, 0.2, 0.3, 0.4].
        let expected = [0.1_f64, 0.2, 0.3, 0.4];
        for (i, (&actual, &exp)) in bary.iter().zip(expected.iter()).enumerate() {
            assert!(
                (actual - exp).abs() < TOL,
                "sheared-tet bary[{i}] = {actual}, expected {exp} \
                 (catches J⁻ᵀ-vs-J⁻¹ index swap)",
            );
        }

        // (2) Affine-field exactness: any P1 nodal field of the form
        //     u(x) = (a·x + b·y + c·z + d, …) is interpolated exactly on
        //     a P1 tet (the interpolant lives in the same polynomial
        //     space as the field). Build the per-vertex nodal values
        //     from a known affine field and verify the interpolant at
        //     `p` equals the analytical evaluation at `p`.
        //   u(x) = (2x + 3y + 5z + 7,
        //           4x + 6y + 8z + 9,
        //           x  +  y +  z + 1)
        let f = |x: [f64; 3]| {
            [
                2.0 * x[0] + 3.0 * x[1] + 5.0 * x[2] + 7.0,
                4.0 * x[0] + 6.0 * x[1] + 8.0 * x[2] + 9.0,
                x[0] + x[1] + x[2] + 1.0,
            ]
        };
        let nodal_values: [[f64; 3]; 4] = [
            f(phys_nodes[0]),
            f(phys_nodes[1]),
            f(phys_nodes[2]),
            f(phys_nodes[3]),
        ];
        let interp = interpolate_p1_at_point(&phys_nodes, &nodal_values, p);
        let exact = f(p);
        for k in 0..3 {
            assert!(
                (interp[k] - exact[k]).abs() < 1e-10,
                "sheared-tet affine interp[{k}] = {} expected {} \
                 (catches J/J⁻ᵀ index bug)",
                interp[k],
                exact[k],
            );
        }
    }

    #[test]
    fn barycentric_p1_returns_kronecker_at_vertices_and_partition_at_centroid() {
        // At each vertex v_i, the P1 shape function N_i = 1 and N_j = 0
        // for j ≠ i (Kronecker delta). At the centroid, all four shape
        // functions equal 1/4 and sum to 1 (partition of unity).
        for (i, v) in UNIT_TET_P1.iter().enumerate() {
            let bary = barycentric_p1(&UNIT_TET_P1, *v);
            for (j, &n_j) in bary.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (n_j - expected).abs() < TOL,
                    "barycentric at vertex {i}: bary[{j}] = {n_j}, expected {expected}",
                );
            }
        }

        let centroid = [0.25_f64, 0.25, 0.25];
        let bary = barycentric_p1(&UNIT_TET_P1, centroid);
        for (j, &n_j) in bary.iter().enumerate() {
            assert!(
                (n_j - 0.25).abs() < TOL,
                "centroid bary[{j}] = {n_j}, expected 0.25",
            );
        }
        let sum: f64 = bary.iter().sum();
        assert!(
            (sum - 1.0).abs() < TOL,
            "centroid Σbary = {sum}, expected 1.0 (partition of unity)",
        );
    }
}
