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
