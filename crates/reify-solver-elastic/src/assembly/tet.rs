//! Tetrahedral element-stiffness assembly (P1 and P2).
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8.
//!
//! # Formula
//!
//! For each tetrahedral element with reference→physical Jacobian `J(ξ)`,
//! the element-local stiffness is
//!
//! ```text
//! K_e = ∫_Ω̂ Bᵀ(ξ) D B(ξ) |det J(ξ)| dξ
//! ```
//!
//! integrated over the reference tet `Ω̂` via Gauss quadrature
//! (`element.quad_points()`). `D` is the 6×6 isotropic-elastic constitutive
//! matrix from [`crate::constitutive::IsotropicElastic`].
//!
//! # Strain-displacement matrix `B`
//!
//! `B` is a `6 × 3N` matrix that maps element-nodal displacements
//! `[u₁ₓ, u₁ᵧ, u₁ᵤ, u₂ₓ, …]ᵀ` to the **engineering-strain Voigt vector**
//! `[ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]ᵀ`. The shear rows hold the
//! engineering shear strain `γ_ij = 2 ε_ij`; this convention is what lets
//! the constitutive shear-block diagonal be `μ` directly (rather than
//! `2μ`) — see [`crate::constitutive::IsotropicElastic`] for the matching
//! `D`-matrix derivation.
//!
//! For each node `i`, the three columns of `B` at DOF indices
//! `3i+0`, `3i+1`, `3i+2` (axes `x, y, z`) are:
//!
//! ```text
//!                 col 3i+0   col 3i+1   col 3i+2
//!                 (u_x)      (u_y)      (u_z)
//!               ┌                                ┐
//!  row 0 ε_xx   │ ∂N_i/∂x   0          0        │
//!  row 1 ε_yy   │ 0         ∂N_i/∂y    0        │
//!  row 2 ε_zz   │ 0         0          ∂N_i/∂z  │
//!  row 3 γ_xy   │ ∂N_i/∂y   ∂N_i/∂x    0        │
//!  row 4 γ_yz   │ 0         ∂N_i/∂z    ∂N_i/∂y  │
//!  row 5 γ_xz   │ ∂N_i/∂z   0          ∂N_i/∂x  │
//!               └                                ┘
//! ```
//!
//! Read row-by-row: each Voigt component picks up contributions from the
//! three displacement axes of node `i` according to the symmetric strain
//! tensor `ε_ij = ½ (∂u_i/∂x_j + ∂u_j/∂x_i)`, doubled in the shear rows
//! because `γ_ij = 2 ε_ij`.
//!
//! Physical-frame gradients are obtained from reference gradients via
//! `∇_x N_i = J⁻ᵀ ∇_ξ N_i`. The 3×3 inverse-transpose is computed
//! inline ([`inverse_transpose_3x3`]) — no external linear-algebra
//! dependency.
//!
//! # DOF ordering
//!
//! `K_e` is indexed `(3·node_idx + axis, 3·node_idx + axis)` with
//! `axis ∈ {0, 1, 2}` for `(u_x, u_y, u_z)`. See the
//! [`crate::assembly::ElementStiffness`] doc for the row-major storage
//! contract.

use crate::assembly::ElementStiffness;
use crate::constitutive::IsotropicElastic;
use crate::elements::{ReferenceElement, tet_p1::TetP1, tet_p2::TetP2};

/// Conservative lower bound on `|det J|` for [`element_stiffness_generic`]'s
/// debug-mode degenerate-element check.
///
/// Anything at or below this threshold is treated as a malformed element
/// and trips a `debug_assert!` rather than silently dividing by it (which
/// would propagate `±∞` / `NaN` through the inverse Jacobian into `K_e`).
/// `1e-30` is far below any plausible real-world element volume even in
/// micrometre meshes, so the check should never false-positive on valid
/// inputs; PRD task #21 (diagnostics) will replace this placeholder with
/// a proper mesh-scale-aware degeneracy detector.
const MIN_JACOBIAN_DET: f64 = 1.0e-30;

/// Return `(M⁻¹)ᵀ = M⁻ᵀ` via the standard 3×3 cofactor / adjugate formula.
///
/// `det` is the determinant of `m` and is taken from
/// [`crate::elements::Jacobian::det`] (already computed when the element's
/// Jacobian was evaluated) rather than recomputed.
///
/// # Derivation
///
/// For any invertible `M`,
///
/// ```text
/// (adj M)[i][j] = c[j][i]      where c[i][j] is the (i, j) cofactor of M
/// (M⁻¹)[i][j]   = (adj M)[i][j] / det M
/// (M⁻ᵀ)[i][j]   = (M⁻¹)[j][i] = c[i][j] / det M
/// ```
///
/// so the `(i, j)` entry of `M⁻ᵀ` is just the `(i, j)` cofactor divided
/// by `det M`. Each cofactor is `(-1)^(i+j)` times the 2×2 minor obtained
/// by deleting row `i` and column `j`.
///
/// # Preconditions
///
/// `det != 0`. For a degenerate / inverted element with `det == 0` the
/// result is non-finite (division by zero); diagnosing that condition
/// is PRD task #21's job.
#[allow(clippy::needless_range_loop)]
fn inverse_transpose_3x3(m: &[[f64; 3]; 3], det: f64) -> [[f64; 3]; 3] {
    let mut inv_t = [[0.0_f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            // Indices of the two rows / columns that survive after
            // deleting row i / column j.
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

/// Generic element-stiffness assembly: `K_e = ∫ BᵀDB |det J| dV` integrated
/// via the element's Gauss quadrature rule.
///
/// `phys_nodes.len()` must equal `E::N_NODES`; the resulting matrix is
/// `n_dofs × n_dofs` with `n_dofs = 3 · N_NODES`.
///
/// The wrappers `element_stiffness_p1` / `element_stiffness_p2` fix
/// `E = TetP1` / `E = TetP2` and assert the right `phys_nodes` length —
/// callers should prefer those typed entry points.
///
/// Uses `det.abs()` for the volume measure so mirror-flipped (left-handed)
/// node orderings still produce a non-negative strain-energy integrand.
/// Right-handed elements have `det J > 0` and `det.abs() == det`.
#[allow(clippy::needless_range_loop)]
pub(crate) fn element_stiffness_generic<E: ReferenceElement>(
    element: &E,
    phys_nodes: &[[f64; 3]],
    material: &IsotropicElastic,
) -> ElementStiffness {
    assert_eq!(
        phys_nodes.len(),
        E::N_NODES,
        "phys_nodes.len() must equal E::N_NODES",
    );
    let n = E::N_NODES;
    let n_dofs = 3 * n;
    let d_mat = material.d_matrix();
    let mut k_e = ElementStiffness::zeros(n_dofs);

    // Reusable scratch buffers (one allocation per call, not per q-point).
    let mut b_cols: Vec<[f64; 6]> = vec![[0.0_f64; 6]; n_dofs];
    let mut db_cols: Vec<[f64; 6]> = vec![[0.0_f64; 6]; n_dofs];
    let mut grads_phys: Vec<[f64; 3]> = vec![[0.0_f64; 3]; n];

    for q in element.quad_points() {
        // Reference gradients ∇_ξ N_i at this q-point.
        let grads_ref = element.shape_grad_at(q.coord);
        debug_assert_eq!(grads_ref.len(), n);

        // Forward Jacobian J_ij = Σ_k phys_nodes[k][i] · grads_ref[k][j].
        // (Inlined here rather than calling `element.jacobian(...)` so we
        // don't re-allocate `grads_ref` inside the trait default impl.)
        let mut j_mat = [[0.0_f64; 3]; 3];
        for k in 0..n {
            for i in 0..3 {
                for jj in 0..3 {
                    j_mat[i][jj] += phys_nodes[k][i] * grads_ref[k][jj];
                }
            }
        }
        let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
            - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
            + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);
        // Degenerate-element guard. `det.is_normal()` catches ±0, ±∞, NaN,
        // and subnormals; the absolute-value floor (`MIN_JACOBIAN_DET`)
        // catches the merely-tiny case where division by `det` in
        // `inverse_transpose_3x3` would inflate FP error to dominate the
        // final `K_e`. Both conditions trip a `debug_assert!` rather than
        // silently propagating `NaN` / `±∞`. PRD task #21 (diagnostics)
        // will replace this with a mesh-scale-aware degeneracy detector
        // and proper error reporting.
        debug_assert!(
            det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
            "degenerate element: |det J| = {} at quad point {:?} (must be > {} \
             and finite — see PRD task #21 for the future diagnostic path)",
            det.abs(),
            q.coord,
            MIN_JACOBIAN_DET,
        );
        let j_inv_t = inverse_transpose_3x3(&j_mat, det);

        // Push reference gradients to physical: ∇_x N_i = J⁻ᵀ · ∇_ξ N_i.
        for i in 0..n {
            for r in 0..3 {
                let mut s = 0.0;
                for c in 0..3 {
                    s += j_inv_t[r][c] * grads_ref[i][c];
                }
                grads_phys[i][r] = s;
            }
        }

        // Build B columns: b_cols[3i+α][m] = B[m][3i+α].
        // Reset all entries (previous q-point's values are stale).
        for col in b_cols.iter_mut() {
            *col = [0.0; 6];
        }
        for i in 0..n {
            let (gx, gy, gz) = (grads_phys[i][0], grads_phys[i][1], grads_phys[i][2]);
            // α = 0 (u_x): nonzero in rows 0 (ε_xx), 3 (γ_xy), 5 (γ_xz)
            b_cols[3 * i][0] = gx;
            b_cols[3 * i][3] = gy;
            b_cols[3 * i][5] = gz;
            // α = 1 (u_y): nonzero in rows 1 (ε_yy), 3 (γ_xy), 4 (γ_yz)
            b_cols[3 * i + 1][1] = gy;
            b_cols[3 * i + 1][3] = gx;
            b_cols[3 * i + 1][4] = gz;
            // α = 2 (u_z): nonzero in rows 2 (ε_zz), 4 (γ_yz), 5 (γ_xz)
            b_cols[3 * i + 2][2] = gz;
            b_cols[3 * i + 2][4] = gy;
            b_cols[3 * i + 2][5] = gx;
        }

        // db_cols[a][m] = (D · B)[m][a] = Σ_n D[m][n] · B[n][a]
        //               = Σ_n D[m][n] · b_cols[a][n].
        for a in 0..n_dofs {
            for m in 0..6 {
                let mut s = 0.0;
                for n_idx in 0..6 {
                    s += d_mat[m][n_idx] * b_cols[a][n_idx];
                }
                db_cols[a][m] = s;
            }
        }

        // K[a][b] += Σ_m B[m][a] · (DB)[m][b] · |det J| · w
        //         = Σ_m b_cols[a][m] · db_cols[b][m] · factor.
        //
        // BᵀDB is symmetric whenever D is (which the isotropic-elastic D
        // is by construction), so we accumulate only the upper triangle
        // (b ≥ a) here and mirror once after the q-point loop. This both
        // halves the inner-loop ops and guarantees `K_e` is bit-for-bit
        // symmetric (no FP drift from differing summation orders).
        let factor = det.abs() * q.weight;
        for a in 0..n_dofs {
            for b in a..n_dofs {
                let mut s = 0.0;
                for m in 0..6 {
                    s += b_cols[a][m] * db_cols[b][m];
                }
                k_e.add(a, b, s * factor);
            }
        }
    }

    // Mirror upper triangle into lower triangle. Direct `data` access
    // because `ElementStiffness::data` is `pub` and we need a true store
    // (not an `add`) — copying after the q-point sum is finished is an
    // O(n_dofs²) tail with no inner-loop work, dominated by the BᵀDB
    // accumulation above.
    for a in 0..n_dofs {
        for b in (a + 1)..n_dofs {
            let v = k_e.data[a * n_dofs + b];
            k_e.data[b * n_dofs + a] = v;
        }
    }

    k_e
}

/// Compute the 12×12 element stiffness for a P1 (linear) tetrahedron.
///
/// `phys_nodes` are the 4 vertex positions in canonical order
/// matching `TetP1::N_NODES = 4` and the reference vertex layout
/// `(0,0,0), (1,0,0), (0,1,0), (0,0,1)`.
///
/// Quadrature: P1 uses a 1-point centroid rule (degree-1 exact); for
/// affine geometry that's exact for the constant-`B` integrand a P1
/// element produces.
pub fn element_stiffness_p1(
    phys_nodes: &[[f64; 3]; 4],
    material: &IsotropicElastic,
) -> ElementStiffness {
    element_stiffness_generic(&TetP1, &phys_nodes[..], material)
}

/// Compute the 30×30 element stiffness for a P2 (quadratic) tetrahedron.
///
/// `phys_nodes` are the 10 nodal positions in canonical Hughes/Gmsh order:
/// the 4 reference vertices `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` followed
/// by the 6 edge-midpoint nodes in `crate::elements::tet_p2::EDGES` order
/// `(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)`.
///
/// # Quadrature
///
/// Uses the 4-point Stroud rule from [`TetP2::quad_points`] (degree-2
/// exact). For **straight-edge** P2 elements the geometric Jacobian is
/// constant per element, so the BᵀDB integrand is degree-2 in reference
/// coordinates and Stroud integrates it exactly — see the rationale in
/// `crates/reify-solver-elastic/src/elements/tet_p2.rs:31-36`.
///
/// **Curved-edge** P2 (where the edge-midpoint nodes are nudged off the
/// straight midpoint to follow a curved boundary) yields a non-constant
/// Jacobian and would need the 11-point degree-4 rule; that case is
/// deferred to v0.4+ per the crate-level scope note in `lib.rs:19-21`.
pub fn element_stiffness_p2(
    phys_nodes: &[[f64; 3]; 10],
    material: &IsotropicElastic,
) -> ElementStiffness {
    element_stiffness_generic(&TetP2, &phys_nodes[..], material)
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;

    /// Canonical unit reference tet: vertices `(0,0,0), (1,0,0), (0,1,0),
    /// (0,0,1)` with reference-tet volume 1/6.
    const UNIT_TET_P1: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    fn dimensionless_steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 1.0,
            poisson_ratio: 0.3,
        }
    }

    /// Compute K · u for a flat-row-major K of size `n × n`.
    fn matvec(k: &crate::assembly::ElementStiffness, u: &[f64]) -> Vec<f64> {
        assert_eq!(k.n_dofs, u.len());
        let n = k.n_dofs;
        let mut out = vec![0.0; n];
        for i in 0..n {
            for j in 0..n {
                out[i] += k.get(i, j) * u[j];
            }
        }
        out
    }

    /// L∞ norm of a slice.
    fn linf(v: &[f64]) -> f64 {
        v.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()))
    }

    #[test]
    fn p1_returns_12_by_12_stiffness() {
        let k = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        assert_eq!(k.n_dofs, 12);
        assert_eq!(k.data.len(), 144);
    }

    #[test]
    fn p1_is_symmetric() {
        let k = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        for i in 0..12 {
            for j in 0..12 {
                let lhs = k.get(i, j);
                let rhs = k.get(j, i);
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-10 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    #[test]
    fn p1_has_rigid_body_translation_null_space() {
        // For each axis ∈ {0, 1, 2}, the 12-vector u with
        // u[3·k + axis] = 1 ∀k is a uniform translation; K·u must vanish.
        let k = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        for axis in 0..3 {
            let mut u = vec![0.0; 12];
            for node in 0..4 {
                u[3 * node + axis] = 1.0;
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < 1e-9,
                "axis {axis}: ‖K·u‖_∞ = {} (expected <1e-9)",
                linf(&ku),
            );
        }
    }

    #[test]
    fn p1_has_rigid_body_rotation_null_space() {
        // For each axis ω ∈ {ê_x, ê_y, ê_z}, build u_i = ω × x_i (using
        // node phys-coords). Such infinitesimal rotations produce zero
        // strain and must lie in K's kernel.
        let k = element_stiffness_p1(&UNIT_TET_P1, &dimensionless_steel_like());
        for axis in 0..3 {
            let mut omega = [0.0_f64; 3];
            omega[axis] = 1.0;
            let mut u = vec![0.0; 12];
            for (node, x) in UNIT_TET_P1.iter().enumerate() {
                // ω × x  =  (ω_y x_z − ω_z x_y, ω_z x_x − ω_x x_z, ω_x x_y − ω_y x_x)
                u[3 * node] = omega[1] * x[2] - omega[2] * x[1];
                u[3 * node + 1] = omega[2] * x[0] - omega[0] * x[2];
                u[3 * node + 2] = omega[0] * x[1] - omega[1] * x[0];
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < 1e-9,
                "ω-axis {axis}: ‖K·u‖_∞ = {} (expected <1e-9)",
                linf(&ku),
            );
        }
    }

    /// Shared patch-test helper.
    ///
    /// Computes U_K = 0.5 · uᵀ · K · u (treating K as flat row-major) and
    /// U_analytical = 0.5 · εᵀ · D · ε · V, returning `(U_K, U_analytical)`.
    fn strain_energies(
        k: &crate::assembly::ElementStiffness,
        u: &[f64],
        eps_voigt: &[f64; 6],
        d: &[[f64; 6]; 6],
        volume: f64,
    ) -> (f64, f64) {
        // U_K = 0.5 · uᵀ K u
        let ku = matvec(k, u);
        let mut u_dot_ku = 0.0;
        for i in 0..u.len() {
            u_dot_ku += u[i] * ku[i];
        }
        let u_k = 0.5 * u_dot_ku;

        // U_analytical = 0.5 · εᵀ D ε V
        let mut d_eps = [0.0_f64; 6];
        for i in 0..6 {
            for j in 0..6 {
                d_eps[i] += d[i][j] * eps_voigt[j];
            }
        }
        let mut eps_dot_d_eps = 0.0;
        for i in 0..6 {
            eps_dot_d_eps += eps_voigt[i] * d_eps[i];
        }
        let u_analytical = 0.5 * eps_dot_d_eps * volume;
        (u_k, u_analytical)
    }

    #[test]
    fn p1_strain_energy_patch_test_matches_normal_strain_mode() {
        // Linear displacement u(x) = A·x with A = diag(a, b, c); the
        // resulting strain field is constant (ε_xx = a, ε_yy = b, ε_zz = c,
        // shears zero), so for a P1 tet with linear shapes the FE
        // strain energy must equal the analytical 0.5 εᵀDε V exactly
        // (modulo FP).
        let (a, b, c) = (0.01, -0.005, 0.003);
        let mat = dimensionless_steel_like();
        let d = mat.d_matrix();
        let k = element_stiffness_p1(&UNIT_TET_P1, &mat);

        let mut u = vec![0.0; 12];
        for (node_idx, x) in UNIT_TET_P1.iter().enumerate() {
            // (A · x)[axis] = A_axis_axis · x[axis] for diagonal A
            u[3 * node_idx] = a * x[0];
            u[3 * node_idx + 1] = b * x[1];
            u[3 * node_idx + 2] = c * x[2];
        }
        let eps_voigt = [a, b, c, 0.0, 0.0, 0.0];
        let volume = 1.0 / 6.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    #[test]
    fn p1_strain_energy_patch_test_matches_pure_shear_mode() {
        // Linear displacement u_x = (s/2) y, u_y = (s/2) x, u_z = 0
        // ⇒ ε_xx = ε_yy = ε_zz = 0, ε_xy = s/2, γ_xy = 2 ε_xy = s.
        // ε_voigt = [0, 0, 0, s, 0, 0].
        let s = 0.004;
        let mat = dimensionless_steel_like();
        let d = mat.d_matrix();
        let k = element_stiffness_p1(&UNIT_TET_P1, &mat);

        let mut u = vec![0.0; 12];
        for (node_idx, x) in UNIT_TET_P1.iter().enumerate() {
            u[3 * node_idx] = 0.5 * s * x[1];
            u[3 * node_idx + 1] = 0.5 * s * x[0];
            u[3 * node_idx + 2] = 0.0;
        }
        let eps_voigt = [0.0, 0.0, 0.0, s, 0.0, 0.0];
        let volume = 1.0 / 6.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    /// Build the canonical 10-node phys-node layout for a uniformly scaled
    /// reference tet: 4 vertices at `(0,0,0), (s,0,0), (0,s,0), (0,0,s)`
    /// and 6 edge midpoints in `crate::elements::tet_p2::EDGES` order.
    /// Mirrors `tet_p2::tests::scaled_tet_phys_nodes`.
    fn scaled_p2_phys_nodes(s: f64) -> [[f64; 3]; 10] {
        let v: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [s, 0.0, 0.0],
            [0.0, s, 0.0],
            [0.0, 0.0, s],
        ];
        let mid = |a: usize, b: usize| {
            [
                0.5 * (v[a][0] + v[b][0]),
                0.5 * (v[a][1] + v[b][1]),
                0.5 * (v[a][2] + v[b][2]),
            ]
        };
        // EDGES = [(0,1), (1,2), (2,0), (0,3), (1,3), (2,3)]
        [
            v[0],
            v[1],
            v[2],
            v[3],
            mid(0, 1),
            mid(1, 2),
            mid(2, 0),
            mid(0, 3),
            mid(1, 3),
            mid(2, 3),
        ]
    }

    #[test]
    fn p2_returns_30_by_30_stiffness() {
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &dimensionless_steel_like());
        assert_eq!(k.n_dofs, 30);
        assert_eq!(k.data.len(), 900);
    }

    #[test]
    fn p2_is_symmetric() {
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &dimensionless_steel_like());
        for i in 0..30 {
            for j in 0..30 {
                let lhs = k.get(i, j);
                let rhs = k.get(j, i);
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-9 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    #[test]
    fn p2_has_rigid_body_translation_null_space() {
        // u[3·k + axis] = 1 for all 10 nodes is a rigid-body translation;
        // K·u must vanish.
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &dimensionless_steel_like());
        for axis in 0..3 {
            let mut u = vec![0.0; 30];
            for node in 0..10 {
                u[3 * node + axis] = 1.0;
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < 1e-8,
                "axis {axis}: ‖K·u‖_∞ = {} (expected <1e-8)",
                linf(&ku),
            );
        }
    }

    #[test]
    fn p2_has_rigid_body_rotation_null_space() {
        // Build u_i = ω × (x_i − c) about the centroid c = (0.25, 0.25, 0.25)
        // for each ω ∈ {ê_x, ê_y, ê_z}. Linear-in-x displacements live in
        // the P2 basis exactly, so rigid rotations sit in K's kernel.
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &dimensionless_steel_like());
        let centroid = [0.25_f64, 0.25, 0.25];
        for axis in 0..3 {
            let mut omega = [0.0_f64; 3];
            omega[axis] = 1.0;
            let mut u = vec![0.0; 30];
            for (node, x) in phys.iter().enumerate() {
                let r = [
                    x[0] - centroid[0],
                    x[1] - centroid[1],
                    x[2] - centroid[2],
                ];
                u[3 * node] = omega[1] * r[2] - omega[2] * r[1];
                u[3 * node + 1] = omega[2] * r[0] - omega[0] * r[2];
                u[3 * node + 2] = omega[0] * r[1] - omega[1] * r[0];
            }
            let ku = matvec(&k, &u);
            assert!(
                linf(&ku) < 1e-8,
                "ω-axis {axis}: ‖K·u‖_∞ = {} (expected <1e-8)",
                linf(&ku),
            );
        }
    }

    #[test]
    fn p2_strain_energy_patch_test_matches_full_six_component_strain() {
        // u(x) = A·x with A symmetric ⇒ pure-strain (no rotation), with
        // ε_ij = ½(A_ij + A_ji) = A_ij. In Voigt:
        //   ε_voigt = [A_xx, A_yy, A_zz, 2 A_xy, 2 A_yz, 2 A_xz]
        // Pick all 6 entries distinct so every Voigt component is exercised.
        // Working in terms of the desired Voigt entries (a, b, c, d, e, f):
        //   A_xx = a, A_yy = b, A_zz = c,
        //   A_xy = A_yx = d/2, A_yz = A_zy = e/2, A_xz = A_zx = f/2.
        let (a, b, c, d, e_v, f) = (0.01, -0.005, 0.003, 0.002, -0.001, 0.0007);
        let big_a = [
            [a, d / 2.0, f / 2.0],
            [d / 2.0, b, e_v / 2.0],
            [f / 2.0, e_v / 2.0, c],
        ];
        let mat = dimensionless_steel_like();
        let d_mat = mat.d_matrix();
        let phys = scaled_p2_phys_nodes(1.0);
        let k = element_stiffness_p2(&phys, &mat);

        let mut u = vec![0.0; 30];
        for (node_idx, x) in phys.iter().enumerate() {
            // u_i = (A · x)[i] = Σ_j A[i][j] · x[j]
            for i in 0..3 {
                let mut s = 0.0;
                for j in 0..3 {
                    s += big_a[i][j] * x[j];
                }
                u[3 * node_idx + i] = s;
            }
        }
        let eps_voigt = [a, b, c, d, e_v, f];
        let volume = 1.0 / 6.0;

        let (u_k, u_a) = strain_energies(&k, &u, &eps_voigt, &d_mat, volume);
        let scale = u_a.abs().max(1e-300);
        assert!(
            (u_k - u_a).abs() < 1e-9 * scale,
            "U_K = {u_k}, U_analytical = {u_a} (rel err {})",
            (u_k - u_a).abs() / scale,
        );
    }

    #[test]
    fn p1_volume_scaling_doubles_stiffness_when_edge_length_doubles() {
        // K ∝ L for isotropic linear-elastic affine maps: B ∝ 1/L
        // (gradients scale inversely with mesh size), and dV ∝ L³, so
        // BᵀDB·dV ∝ L. Doubling all node coordinates from the unit tet
        // therefore exactly doubles every entry of K_e.
        let mat = dimensionless_steel_like();
        let k_unit = element_stiffness_p1(&UNIT_TET_P1, &mat);

        let scaled: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
        ];
        let k_scaled = element_stiffness_p1(&scaled, &mat);

        for i in 0..12 {
            for j in 0..12 {
                let unit: f64 = k_unit.get(i, j);
                let got: f64 = k_scaled.get(i, j);
                let expected: f64 = 2.0 * unit;
                let scale = expected.abs().max(unit.abs()).max(1.0);
                assert!(
                    (got - expected).abs() < 1e-10 * scale,
                    "K_scaled[{i}][{j}] = {got} (expected 2·K_unit = {expected})",
                );
            }
        }
    }
}
