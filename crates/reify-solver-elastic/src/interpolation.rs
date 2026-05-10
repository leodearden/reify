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

/// Conservative lower bound on `|det J|` for the debug-mode
/// degenerate-element check inside [`barycentric_p1`].
///
/// Mirrors `crates/reify-solver-elastic/src/assembly/tet.rs:75`'s
/// `MIN_JACOBIAN_DET = 1e-30` — kept in sync by convention rather than a
/// re-export, because that constant is private to the assembly module
/// and its containing file is not in this task's lock set. Anything at
/// or below this threshold is treated as a malformed element and trips
/// a `debug_assert!` rather than silently dividing by it (which would
/// propagate `±∞` / `NaN` through the inverse Jacobian into the
/// barycentric coordinates). PRD task #21 (diagnostics) will replace
/// this placeholder with a proper mesh-scale-aware degeneracy detector.
const MIN_JACOBIAN_DET: f64 = 1.0e-30;

/// Return `(M⁻¹)ᵀ = M⁻ᵀ` for a 3×3 matrix via the standard cofactor /
/// adjugate formula.
///
/// `det` is the determinant of `m`, taken from the caller (already
/// computed alongside the forward Jacobian rather than recomputed). The
/// canonical formula is single-sourced in spirit by
/// `crates/reify-solver-elastic/src/assembly/tet.rs:103` — this is a
/// local copy so this module stays self-contained, per the design
/// decision documented in `.task/plan.json`.
///
/// # Preconditions
///
/// `det != 0`. For a degenerate tet with `det == 0` the result is
/// non-finite (division by zero); diagnosing that condition is PRD task
/// #21's job.
#[allow(clippy::needless_range_loop)]
fn inverse_transpose_3x3(m: &[[f64; 3]; 3], det: f64) -> [[f64; 3]; 3] {
    let mut inv_t = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            let r0 = if i == 0 { 1 } else { 0 };
            let r1 = if i == 2 { 1 } else { 2 };
            let c0 = if j == 0 { 1 } else { 0 };
            let c1 = if j == 2 { 1 } else { 2 };
            let minor = m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0];
            let sign = if (i + j) % 2 == 0 { 1.0 } else { -1.0 };
            inv_t[i][j] = sign * minor / det;
        }
    }
    inv_t
}

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
/// `tol` is a relative slack for points on the element boundary: a
/// query point that is in the tet up to floating-point round-off (e.g.
/// `−1e-12` along an edge) is classified as inside when `tol = 1e-9`.
/// Use `tol = 0.0` for a strict inclusion test.
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
pub fn locate_element_p1(
    elements: &[LocatableTet<'_>],
    p: [f64; 3],
    tol: f64,
) -> Option<usize> {
    for (i, el) in elements.iter().enumerate() {
        if point_in_tet_p1(el.phys_nodes, p, tol) {
            return Some(i);
        }
    }
    None
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
            let expected = 0.25 * (nodal_values[0][k]
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
