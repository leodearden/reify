//! P1 (linear, 4-node) tetrahedral geometric-stiffness kernel.
//!
//! See PRD `docs/prds/v0_5/buckling-eigensolver.md` §13 task γ.

use crate::assembly::ElementStiffness;
use crate::elements::tet_p1::GRADS_REF;

use super::InitialStress3;

/// Conservative lower bound on `|det J|` mirroring the convention in
/// `assembly::tet::MIN_JACOBIAN_DET`. Anything at or below this trips a
/// `debug_assert!` rather than silently dividing by it and propagating
/// non-finite values through the inverse Jacobian into `K_g`. Kept in sync
/// by inspection — when the elastic-`K_e` path moves its constant into a
/// shared `crate::math` module the geometric path should follow.
const MIN_JACOBIAN_DET: f64 = 1.0e-30;

/// Return `(M⁻¹)ᵀ = M⁻ᵀ` via the standard 3×3 cofactor formula.
///
/// Mirrors `assembly::tet::inverse_transpose_3x3` (private) verbatim;
/// the two implementations should be consolidated once `crate::math`
/// lands a shared 3×3 inverse-transpose helper.
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

/// Compute the 12×12 geometric stiffness `K_g` for a P1 (linear) tetrahedron
/// under a constant initial Cauchy stress `sigma`.
///
/// `phys_nodes` are the 4 vertex positions in the canonical reference-vertex
/// ordering `(0,0,0), (1,0,0), (0,1,0), (0,0,1)` — same convention as
/// [`crate::element_stiffness_p1`].
///
/// The returned matrix shares the row-major `(3·node_idx + axis)` layout of
/// [`ElementStiffness`], so it can be fed into
/// [`crate::assemble_global_stiffness`] without any repacking.
///
/// # Formula
///
/// With constant physical gradients `∇N_a` (P1 ⇒ gradients independent of
/// reference coordinate) and constant `σ` over the element,
///
/// ```text
/// K_g[3a+α, 3b+α] = (∇N_a · σ · ∇N_b) · V_e          α ∈ {0,1,2}
/// K_g[3a+α, 3b+β] = 0                                α ≠ β
/// ```
///
/// where `V_e = |det J| / 6` is the physical tetrahedral volume.
///
/// # Panics
///
/// Panics under `debug_assertions` when:
/// - any component of `sigma` is non-finite (NaN or ±∞) — non-finite
///   stress propagates non-physical values through the stiffness matrix.
///   The check is finite-only (not finite-positive) because compressive
///   stress is negative and zero stress is mathematically valid.
/// - `|det J| <= MIN_JACOBIAN_DET` or `det J` is non-finite/subnormal —
///   the same degeneracy-guard convention as [`crate::element_stiffness_p1`].
///
/// Uses `|det J|` so left-handed (mirror-flipped) node orderings still
/// produce the physically correct `V_e`; the strain-energy contribution
/// `0.5 · uᵀ K_g u` is invariant under node relabelling provided `u` is
/// reordered consistently.
#[allow(clippy::needless_range_loop)]
pub fn geometric_element_stiffness_tet_p1(
    phys_nodes: &[[f64; 3]; 4],
    sigma: &InitialStress3,
) -> ElementStiffness {
    const N_NODES: usize = 4;
    const N_DOFS: usize = 12;
    debug_assert!(
        sigma.sigma.iter().flatten().all(|x| x.is_finite()),
        "stress must be entrywise finite, got {:?}",
        sigma.sigma,
    );
    let mut k_g = ElementStiffness::zeros(N_DOFS);

    // P1 gradients are compile-time constants — use the shared const
    // directly rather than calling TetP1::shape_grad_at (which allocates a
    // Vec per call).
    let grads_ref = &GRADS_REF;

    // Forward Jacobian J_ij = Σ_k phys_nodes[k][i] · grads_ref[k][j].
    // Inlined (rather than calling TetP1::jacobian) to avoid the
    // intermediate Vec the trait default allocates.
    let mut j_mat = [[0.0_f64; 3]; 3];
    for k in 0..N_NODES {
        for i in 0..3 {
            for jj in 0..3 {
                j_mat[i][jj] += phys_nodes[k][i] * grads_ref[k][jj];
            }
        }
    }
    let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
        - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
        + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);

    debug_assert!(
        det.is_normal() && det.abs() > MIN_JACOBIAN_DET,
        "degenerate element: |det J| = {} (must be > {} and finite)",
        det.abs(),
        MIN_JACOBIAN_DET,
    );

    let j_inv_t = inverse_transpose_3x3(&j_mat, det);

    // Physical-frame gradients: ∇_x N_a = J⁻ᵀ · ∇_ξ N_a.
    let mut grads_phys = [[0.0_f64; 3]; N_NODES];
    for a in 0..N_NODES {
        for r in 0..3 {
            let mut s = 0.0;
            for c in 0..3 {
                s += j_inv_t[r][c] * grads_ref[a][c];
            }
            grads_phys[a][r] = s;
        }
    }

    let v_e = det.abs() / 6.0; // P1 tet physical volume
    let s = &sigma.sigma;

    // For each (a, b) node pair, accumulate the scalar coupling
    //   g_ab = Σ_{i,j} (∂N_a/∂x_i) · σ_ij · (∂N_b/∂x_j)
    // and write coef · I_3 into the 3×3 block at rows [3a..3a+3],
    // cols [3b..3b+3]. Symmetric blocks: g_ab = g_ba whenever σ = σᵀ.
    for a in 0..N_NODES {
        for b in 0..N_NODES {
            let mut g_ab = 0.0;
            for i in 0..3 {
                for j in 0..3 {
                    g_ab += grads_phys[a][i] * s[i][j] * grads_phys[b][j];
                }
            }
            let coef = g_ab * v_e;
            for alpha in 0..3 {
                let row = 3 * a + alpha;
                let col = 3 * b + alpha;
                k_g.data[row * N_DOFS + col] += coef;
            }
        }
    }

    k_g
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonical unit reference tet — vertices `(0,0,0), (1,0,0), (0,1,0),
    /// (0,0,1)` with reference volume `1/6`.
    const UNIT_TET: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    fn read(k: &ElementStiffness, i: usize, j: usize) -> f64 {
        k.data[i * k.n_dofs + j]
    }

    #[test]
    fn returns_12_by_12_matrix() {
        let k_g = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::uniaxial_z(-1.0));
        assert_eq!(k_g.n_dofs, 12);
        assert_eq!(k_g.data.len(), 144);
    }

    #[test]
    fn zero_stress_yields_zero_matrix() {
        let k_g = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::zero());
        for &v in &k_g.data {
            assert_eq!(v, 0.0, "σ=0 must produce K_g ≡ 0 entrywise");
        }
    }

    #[test]
    fn is_symmetric_under_uniaxial_stress() {
        let k_g = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::uniaxial_z(-2.5));
        for i in 0..12 {
            for j in 0..12 {
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
    fn off_axis_blocks_are_zero_block_diagonal_3x3_structure() {
        // Each (a, b) node-pair block in K_g is `coef · I_3` — diagonal in
        // axis-axis indexing. α ≠ β entries must be exactly 0.
        let k_g = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::uniaxial_z(-1.0));
        for a in 0..4 {
            for b in 0..4 {
                for alpha in 0..3 {
                    for beta in 0..3 {
                        if alpha == beta {
                            continue;
                        }
                        let v = read(&k_g, 3 * a + alpha, 3 * b + beta);
                        assert_eq!(v, 0.0, "(a,b,α,β) = ({a},{b},{alpha},{beta}) must be 0");
                    }
                }
            }
        }
    }

    #[test]
    fn linear_in_stress_magnitude() {
        // K_g is linear in σ — doubling σ doubles every entry.
        let k1 = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::uniaxial_z(-1.0));
        let k2 = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::uniaxial_z(-2.0));
        for i in 0..144 {
            let want = 2.0 * k1.data[i];
            let got = k2.data[i];
            let scale = want.abs().max(1.0);
            assert!(
                (got - want).abs() < 1e-12 * scale,
                "linearity at idx {i}: got {got}, expected 2·{} = {want}",
                k1.data[i],
            );
        }
    }

    // ----- debug-only stress-guard tests -----
    // The guard `debug_assert!(sigma.sigma.iter().flatten().all(|x| x.is_finite()), ...)`
    // is compiled in only under `debug_assertions`, so the tests must be
    // gated identically.  The guard is finite-only (not finite-positive)
    // because compressive stress is negative and zero stress is valid.
    // We do NOT add a `zero_stress_does_not_panic` test — that contract
    // is already pinned by `zero_stress_yields_zero_matrix` above.

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "stress must be entrywise finite")]
    fn k_g_p1_panics_on_nan_stress_component() {
        let sigma = InitialStress3 {
            sigma: [
                [f64::NAN, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
            ],
        };
        let _ = geometric_element_stiffness_tet_p1(&UNIT_TET, &sigma);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "stress must be entrywise finite")]
    fn k_g_p1_panics_on_positive_infinite_stress_component() {
        let sigma = InitialStress3 {
            sigma: [
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, f64::INFINITY],
            ],
        };
        let _ = geometric_element_stiffness_tet_p1(&UNIT_TET, &sigma);
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "stress must be entrywise finite")]
    fn k_g_p1_panics_on_negative_infinite_stress_component() {
        let sigma = InitialStress3 {
            sigma: [
                [0.0, 0.0, 0.0],
                [0.0, f64::NEG_INFINITY, 0.0],
                [0.0, 0.0, 0.0],
            ],
        };
        let _ = geometric_element_stiffness_tet_p1(&UNIT_TET, &sigma);
    }

    #[test]
    fn translation_is_in_kernel() {
        // Rigid-body translation u = (a, b, c) per node ⇒ no relative
        // gradient ⇒ K_g · u = 0 for any σ. This is the geometric analogue
        // of K_e's rigid-body-translation null space.
        let k_g = geometric_element_stiffness_tet_p1(&UNIT_TET, &InitialStress3::uniaxial_z(-1.0));
        for axis in 0..3 {
            let mut u = [0.0_f64; 12];
            for node in 0..4 {
                u[3 * node + axis] = 1.0;
            }
            let mut ku = [0.0_f64; 12];
            for (i, ku_i) in ku.iter_mut().enumerate() {
                for (j, &u_j) in u.iter().enumerate() {
                    *ku_i += read(&k_g, i, j) * u_j;
                }
            }
            let linf = ku.iter().fold(0.0_f64, |acc, x| acc.max(x.abs()));
            assert!(linf < 1e-12, "translation axis {axis}: ‖K_g·u‖_∞ = {linf}",);
        }
    }
}
