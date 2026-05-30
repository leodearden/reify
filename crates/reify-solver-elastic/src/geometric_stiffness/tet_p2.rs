//! P2 (quadratic, 10-node) tetrahedral geometric-stiffness kernel.
//!
//! See PRD `docs/prds/v0_5/buckling-eigensolver.md` §13 task δ (P2 follow-up,
//! task 4052).

use crate::assembly::ElementStiffness;
use crate::elements::{ReferenceCoord, ReferenceElement, tet_p2::TetP2};
use crate::math::{MIN_JACOBIAN_DET, inverse_transpose_3x3};

use super::InitialStress3;

/// Compute the 30×30 geometric stiffness `K_g` for a P2 (quadratic, 10-node)
/// tetrahedron under a per-element CONSTANT initial Cauchy stress `sigma`.
///
/// `phys_nodes` are the 10 node positions in canonical Hughes/Gmsh P2 order:
/// 4 vertices followed by 6 edge-midpoint nodes in the
/// [`crate::elements::tet_p2::EDGES`] order.
///
/// The returned matrix shares the row-major `(3·node_idx + axis)` DOF layout of
/// [`ElementStiffness`], so it can be fed into
/// [`crate::assemble_global_stiffness`] alongside the P2 elastic `K_e`
/// without any repacking.
///
/// # Algorithm
///
/// Integrated with TetP2's degree-2-exact 4-point Stroud quadrature rule.
/// For each quadrature point `q`:
/// 1. Compute the forward Jacobian `J_ij = Σ_k phys_nodes[k][i] · ∇ξ N_k[j]`.
///    For a straight-edge P2 tet `J` is **constant** per element; we compute
///    it at the first quadrature point and reuse `J⁻ᵀ` across all points.
/// 2. Push reference gradients to physical: `∇_x N_a = J⁻ᵀ · ∇_ξ N_a(q)`.
/// 3. Accumulate the block-diagonal K_g entries:
///    `K_g[3a+α, 3b+α] += (Σ_{i,j} ∇N_a[i]·σ_ij·∇N_b[j])·|det J|·w`
///    for `α ∈ {0,1,2}`. Off-axis (`α ≠ β`) entries remain 0.
///
/// # Design decisions
///
/// - **Constant σ⁰ per element**: the geometric-stiffness integrand is
///   `(J⁻ᵀ·∇ξN_a)ᵀ σ⁰ (J⁻ᵀ·∇ξN_b)`, a degree-2 polynomial in `(ξ,η,ζ)`
///   (since both reference gradients are degree-1 and the Jacobian is
///   constant). The 4-point Stroud rule integrates degree-2 exactly.
/// - **Constant Jacobian**: for straight-edge P2 tets the Jacobian is
///   coordinate-independent. We compute it once from `TetP2.shape_grad_at`
///   at the centroid and reuse it, saving 3 Jacobian inversions per element.
///
/// # Panics (debug only)
///
/// - Any component of `sigma` is non-finite (NaN or ±∞).
/// - `|det J| ≤ MIN_JACOBIAN_DET` or `det J` is non-finite/subnormal.
#[allow(clippy::needless_range_loop)]
pub fn geometric_element_stiffness_tet_p2(
    phys_nodes: &[[f64; 3]; 10],
    sigma: &InitialStress3,
) -> ElementStiffness {
    const N_NODES: usize = 10;
    const N_DOFS: usize = 30;

    debug_assert!(
        sigma.sigma.iter().flatten().all(|x| x.is_finite()),
        "stress must be entrywise finite, got {:?}",
        sigma.sigma,
    );

    let mut k_g = ElementStiffness::zeros(N_DOFS);

    // ---- Constant Jacobian for straight-edge P2 tet -------------------------
    // Compute J at the centroid (any interior point works; J is constant).
    let grads_center = TetP2.shape_grad_at(ReferenceCoord::new(0.25, 0.25, 0.25));
    let mut j_mat = [[0.0_f64; 3]; 3];
    for k in 0..N_NODES {
        for i in 0..3 {
            for jj in 0..3 {
                j_mat[i][jj] += phys_nodes[k][i] * grads_center[k][jj];
            }
        }
    }
    let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
        - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
        + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);

    debug_assert!(
        det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
        "degenerate element in geometric_element_stiffness_tet_p2: |det J| = {} \
         (must be > {} and finite)",
        det.abs(),
        MIN_JACOBIAN_DET,
    );

    let j_inv_t = inverse_transpose_3x3(&j_mat, det);
    let det_abs = det.abs();
    let s = &sigma.sigma;

    // ---- Quadrature loop ----------------------------------------------------
    for q in TetP2.quad_points() {
        // Reference gradients at this quadrature point (degree-1 in ξ,η,ζ).
        let grads_ref = TetP2.shape_grad_at(q.coord);

        // Physical gradients: ∇_x N_a = J⁻ᵀ · ∇_ξ N_a.
        let mut grads_phys = [[0.0_f64; 3]; N_NODES];
        for a in 0..N_NODES {
            for r in 0..3 {
                let mut sum = 0.0;
                for c in 0..3 {
                    sum += j_inv_t[r][c] * grads_ref[a][c];
                }
                grads_phys[a][r] = sum;
            }
        }

        // Accumulate K_g[3a+α, 3b+α] += g_ab · |det J| · w.
        let w = q.weight;
        for a in 0..N_NODES {
            for b in 0..N_NODES {
                // g_ab = Σ_{i,j} ∇N_a[i] · σ_ij · ∇N_b[j]
                let mut g_ab = 0.0;
                for i in 0..3 {
                    for j in 0..3 {
                        g_ab += grads_phys[a][i] * s[i][j] * grads_phys[b][j];
                    }
                }
                let coef = g_ab * det_abs * w;
                for alpha in 0..3 {
                    let row = 3 * a + alpha;
                    let col = 3 * b + alpha;
                    k_g.data[row * N_DOFS + col] += coef;
                }
            }
        }
    }

    k_g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assembly::test_support::scaled_p2_phys_nodes;

    fn unit_p2() -> [[f64; 3]; 10] {
        scaled_p2_phys_nodes(1.0)
    }

    fn read(k: &ElementStiffness, i: usize, j: usize) -> f64 {
        k.data[i * k.n_dofs + j]
    }

    #[test]
    fn returns_30x30_matrix() {
        let k_g = geometric_element_stiffness_tet_p2(&unit_p2(), &InitialStress3::uniaxial_z(-1.0));
        assert_eq!(k_g.n_dofs, 30);
        assert_eq!(k_g.data.len(), 900);
    }

    #[test]
    fn zero_stress_yields_zero_matrix() {
        let k_g = geometric_element_stiffness_tet_p2(&unit_p2(), &InitialStress3::zero());
        for &v in &k_g.data {
            assert_eq!(v, 0.0, "σ=0 ⇒ K_g ≡ 0 entrywise");
        }
    }

    #[test]
    fn is_symmetric_under_uniaxial_stress() {
        let k_g = geometric_element_stiffness_tet_p2(&unit_p2(), &InitialStress3::uniaxial_z(-2.5));
        for i in 0..30 {
            for j in 0..30 {
                let lhs = read(&k_g, i, j);
                let rhs = read(&k_g, j, i);
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-12 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    #[test]
    fn off_axis_blocks_are_zero() {
        let k_g = geometric_element_stiffness_tet_p2(&unit_p2(), &InitialStress3::uniaxial_z(-1.0));
        for a in 0..10 {
            for b in 0..10 {
                for alpha in 0..3 {
                    for beta in 0..3 {
                        if alpha == beta {
                            continue;
                        }
                        let v = read(&k_g, 3 * a + alpha, 3 * b + beta);
                        assert_eq!(v, 0.0, "(a,b,α,β)=({a},{b},{alpha},{beta}) must be 0");
                    }
                }
            }
        }
    }

    #[test]
    fn linear_in_stress_magnitude() {
        let k1 = geometric_element_stiffness_tet_p2(&unit_p2(), &InitialStress3::uniaxial_z(-1.0));
        let k2 = geometric_element_stiffness_tet_p2(&unit_p2(), &InitialStress3::uniaxial_z(-2.0));
        for i in 0..900 {
            let want = 2.0 * k1.data[i];
            let got = k2.data[i];
            let scale = want.abs().max(k1.data[i].abs()).max(1.0);
            assert!(
                (got - want).abs() < 1e-12 * scale,
                "linearity at [{i}]: got {got}, expected 2·{} = {want}",
                k1.data[i],
            );
        }
    }

    #[test]
    fn translation_is_in_kernel() {
        let k_g = geometric_element_stiffness_tet_p2(&unit_p2(), &InitialStress3::uniaxial_z(-1.0));
        for axis in 0..3 {
            let mut u = [0.0_f64; 30];
            for node in 0..10 {
                u[3 * node + axis] = 1.0;
            }
            let mut ku = [0.0_f64; 30];
            for (i, ku_i) in ku.iter_mut().enumerate() {
                for (j, &u_j) in u.iter().enumerate() {
                    *ku_i += read(&k_g, i, j) * u_j;
                }
            }
            let linf = ku.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()));
            assert!(linf < 1e-12, "translation axis {axis}: ‖K_g·u‖_∞ = {linf}");
        }
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "stress must be entrywise finite")]
    fn panics_on_nan_stress() {
        let sigma = InitialStress3 { sigma: [[f64::NAN, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]] };
        let _ = geometric_element_stiffness_tet_p2(&unit_p2(), &sigma);
    }
}
