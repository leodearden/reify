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
//! `[ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]ᵀ` (with `γ = 2ε`). Per node `i`,
//! the three columns of `B` for that node are
//!
//! ```text
//!         ┌                            ┐
//!  col 0  │ ∂N_i/∂x  0        0        │  ε_xx
//!  col 1  │ 0        ∂N_i/∂y  0        │  ε_yy
//!  col 2  │ 0        0        ∂N_i/∂z  │  ε_zz
//!  col 0  │ ∂N_i/∂y  ∂N_i/∂x  0        │  γ_xy
//!  col 1  │ 0        ∂N_i/∂z  ∂N_i/∂y  │  γ_yz
//!  col 2  │ ∂N_i/∂z  0        ∂N_i/∂x  │  γ_xz
//!         └                            ┘
//! ```
//!
//! (read column-by-column: each row above is one Voigt component, and the
//! three values shown are at DOF columns `3i+0`, `3i+1`, `3i+2` of `B`.)
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
use crate::elements::{ReferenceElement, tet_p1::TetP1};

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
        let factor = det.abs() * q.weight;
        for a in 0..n_dofs {
            for b in 0..n_dofs {
                let mut s = 0.0;
                for m in 0..6 {
                    s += b_cols[a][m] * db_cols[b][m];
                }
                k_e.add(a, b, s * factor);
            }
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

#[cfg(test)]
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
