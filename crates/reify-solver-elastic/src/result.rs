//! Per-element stress and nodal-stress gradient recovery for tetrahedral
//! P1 FEA.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #13.
//!
//! # Scope
//!
//! P1-only stress recovery for v0.3. The engine integration layer
//! (PRD §16) wraps the recovered nodal field as
//! `Field<Point3<Length>, Tensor<2,3,Pressure>>`; this crate ships the
//! Rust math primitives in plain `f64` types, mirroring the pattern in
//! `shell_result.rs` for shells.
//!
//! # Public surface
//!
//! - [`element_stress_p1`] — per-element constant Cauchy stress
//!   `σ_e = D · B · u_e` returned as a 3×3 symmetric tensor (Voigt is
//!   internal to the multiplication).
//! - [`tet_volume_p1`] — `|det J| / 6` from the affine map.
//! - [`recover_nodal_stress_p1`] + [`StressElement`] — volume-weighted
//!   averaging across incident elements, producing a continuous nodal
//!   stress field interpolatable via the same P1 shape functions.

use crate::constitutive::IsotropicElastic;
use crate::elements::{ReferenceCoord, ReferenceElement, tet_p1::TetP1};

/// Return `(M⁻¹)ᵀ = M⁻ᵀ` for a 3×3 matrix via the standard cofactor /
/// adjugate formula.
///
/// Local copy of the canonical formula in
/// `crates/reify-solver-elastic/src/assembly/tet.rs:103`; kept here so
/// `result.rs` is self-contained, per the design decision recorded in
/// `.task/plan.json`. If a future task adds a third consumer, the right
/// move is to extract `inverse_transpose_3x3` to a shared `crate::math`
/// helper then; with two consumers, preemptive extraction adds module
/// cost without payback.
///
/// # Preconditions
///
/// `det != 0`. For a degenerate element with `det == 0` the result is
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

/// Compute the constant per-element Cauchy stress tensor for a P1
/// tetrahedron: `σ_e = D · B(p) · u_e`.
///
/// Returns a 3×3 symmetric tensor in the consumer-facing form
/// `[[σxx, σxy, σxz], [σxy, σyy, σyz], [σxz, σyz, σzz]]`. Voigt is
/// internal to the `D · B` multiplication; consumers
/// (`von_mises`, `principal_stresses`, …) want full tensor form.
///
/// # Algorithm
///
/// 1. Compute the forward Jacobian `J_ij = Σ_k phys_nodes[k][i] ·
///    grad_ref[k][j]` at the reference centroid via
///    [`TetP1::shape_grad_at`]. P1 gradients are constant per element
///    (any reference coord works).
/// 2. Compute `J⁻ᵀ` via the local [`inverse_transpose_3x3`] helper.
/// 3. Push reference gradients to physical: `∇x N_i = J⁻ᵀ · ∇ξ N_i`.
/// 4. Build the 6×12 strain-displacement matrix `B` with the same
///    engineering-shear Voigt convention as
///    `assembly/tet.rs:208-222`.
/// 5. Compute `ε_voigt = B · u_e` (engineering strain, length 6).
/// 6. Compute `σ_voigt = D · ε_voigt` via
///    [`IsotropicElastic::d_matrix`].
/// 7. Unpack to the symmetric 3×3 tensor.
///
/// # Voigt convention
///
/// Strain order: `[ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]` with engineering
/// shear (`γ = 2ε`). Stress order: `[σ_xx, σ_yy, σ_zz, σ_xy, σ_yz,
/// σ_xz]`. Drift from this convention would break the patch test in
/// `step-11`; see `crate::constitutive::IsotropicElastic` and
/// `crate::assembly::tet` for the full convention rationale. The
/// uniaxial-strain patch test
/// (`element_stress_p1_uniaxial_strain_patch_test_recovers_lame_diagonal`)
/// pins the layout: a `u(x) = (a·x, 0, 0)` field round-trips to
/// `σ = diag((λ+2μ)·a, λ·a, λ·a)` exactly.
///
/// # Preconditions
///
/// The tet must be non-degenerate (`det J != 0`); see
/// [`inverse_transpose_3x3`].
#[allow(clippy::needless_range_loop)]
pub fn element_stress_p1(
    phys_nodes: &[[f64; 3]; 4],
    material: &IsotropicElastic,
    u_e: &[f64; 12],
) -> [[f64; 3]; 3] {
    // Reference gradients (constant for P1 — any reference coord works).
    let grads_ref = TetP1.shape_grad_at(ReferenceCoord::new(0.25, 0.25, 0.25));

    // Forward Jacobian J_ij = Σ_k phys_nodes[k][i] · grads_ref[k][j].
    let mut j_mat = [[0.0_f64; 3]; 3];
    for k in 0..4 {
        for i in 0..3 {
            for j in 0..3 {
                j_mat[i][j] += phys_nodes[k][i] * grads_ref[k][j];
            }
        }
    }
    let det = j_mat[0][0] * (j_mat[1][1] * j_mat[2][2] - j_mat[1][2] * j_mat[2][1])
        - j_mat[0][1] * (j_mat[1][0] * j_mat[2][2] - j_mat[1][2] * j_mat[2][0])
        + j_mat[0][2] * (j_mat[1][0] * j_mat[2][1] - j_mat[1][1] * j_mat[2][0]);
    let j_inv_t = inverse_transpose_3x3(&j_mat, det);

    // Push to physical gradients: ∇x N_i = J⁻ᵀ · ∇ξ N_i.
    let mut grads_phys = [[0.0_f64; 3]; 4];
    for i in 0..4 {
        for r in 0..3 {
            let mut s = 0.0;
            for c in 0..3 {
                s += j_inv_t[r][c] * grads_ref[i][c];
            }
            grads_phys[i][r] = s;
        }
    }

    // Build B and compute ε_voigt = B · u_e in one fused loop.
    // B is 6×12; row layout matches `assembly/tet.rs:208-222`:
    //   row 0 ε_xx ← ∂N/∂x for u_x
    //   row 1 ε_yy ← ∂N/∂y for u_y
    //   row 2 ε_zz ← ∂N/∂z for u_z
    //   row 3 γ_xy ← ∂N/∂y for u_x  +  ∂N/∂x for u_y
    //   row 4 γ_yz ← ∂N/∂z for u_y  +  ∂N/∂y for u_z
    //   row 5 γ_xz ← ∂N/∂z for u_x  +  ∂N/∂x for u_z
    let mut eps = [0.0_f64; 6];
    for i in 0..4 {
        let (gx, gy, gz) = (grads_phys[i][0], grads_phys[i][1], grads_phys[i][2]);
        let (ux, uy, uz) = (u_e[3 * i], u_e[3 * i + 1], u_e[3 * i + 2]);
        eps[0] += gx * ux;
        eps[1] += gy * uy;
        eps[2] += gz * uz;
        eps[3] += gy * ux + gx * uy;
        eps[4] += gz * uy + gy * uz;
        eps[5] += gz * ux + gx * uz;
    }

    // σ_voigt = D · ε_voigt.
    let d_mat = material.d_matrix();
    let mut sigma_voigt = [0.0_f64; 6];
    for i in 0..6 {
        let mut s = 0.0;
        for j in 0..6 {
            s += d_mat[i][j] * eps[j];
        }
        sigma_voigt[i] = s;
    }

    // Unpack to symmetric 3×3 tensor.
    [
        [sigma_voigt[0], sigma_voigt[3], sigma_voigt[5]],
        [sigma_voigt[3], sigma_voigt[1], sigma_voigt[4]],
        [sigma_voigt[5], sigma_voigt[4], sigma_voigt[2]],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitutive::IsotropicElastic;

    /// Canonical unit reference tet: vertices `(0,0,0), (1,0,0), (0,1,0),
    /// (0,0,1)` with reference-tet volume `1/6`.
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

    #[test]
    fn tet_volume_p1_unit_tet_returns_one_sixth_and_scales_cubically_under_uniform_doubling() {
        // Unit tet: V = 1/6.
        let v_unit = tet_volume_p1(&UNIT_TET_P1);
        assert!(
            (v_unit - 1.0 / 6.0).abs() < 1e-12,
            "V(unit_tet) = {v_unit} expected 1/6",
        );

        // Edge-doubled tet: V = 8/6 (V scales as L³).
        let scaled: [[f64; 3]; 4] = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [0.0, 2.0, 0.0],
            [0.0, 0.0, 2.0],
        ];
        let v_scaled = tet_volume_p1(&scaled);
        assert!(
            (v_scaled - 8.0 / 6.0).abs() < 1e-12,
            "V(scaled_tet) = {v_scaled} expected 8/6",
        );

        // Left-handed ordering (swap nodes 1 and 2): det J flips sign,
        // but |det J|/6 is unchanged. Pin that .abs() is taken.
        let flipped: [[f64; 3]; 4] = [
            UNIT_TET_P1[0],
            UNIT_TET_P1[2],
            UNIT_TET_P1[1],
            UNIT_TET_P1[3],
        ];
        let v_flipped = tet_volume_p1(&flipped);
        assert!(
            (v_flipped - 1.0 / 6.0).abs() < 1e-12,
            "V(flipped_tet) = {v_flipped} expected 1/6 (|.| takes care of left-handed)",
        );
    }

    #[test]
    fn element_stress_p1_uniaxial_strain_patch_test_recovers_lame_diagonal() {
        // Linear displacement u(x) = (a·x, 0, 0) ⇒ ε_xx = a, all other
        // strain components 0. Expect σ_xx = (λ+2μ)·a, σ_yy = σ_zz = λ·a,
        // off-diagonals 0. Pins the B-matrix orientation against
        // assembly/tet.rs's convention.
        let a = 0.01_f64;
        let mat = dimensionless_steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let factor = e / ((1.0 + nu) * (1.0 - 2.0 * nu));
        let lambda = factor * nu;
        let two_mu = factor * (1.0 - 2.0 * nu);
        let lambda_plus_two_mu = lambda + two_mu;

        let mut u_e = [0.0_f64; 12];
        for (i, x) in UNIT_TET_P1.iter().enumerate() {
            u_e[3 * i] = a * x[0];
            // u_y, u_z stay 0
        }

        let stress = element_stress_p1(&UNIT_TET_P1, &mat, &u_e);

        let exp_xx = lambda_plus_two_mu * a;
        let exp_yy = lambda * a;
        let exp_zz = lambda * a;

        let scale_xx = exp_xx.abs().max(1.0);
        let scale_yy = exp_yy.abs().max(1.0);
        assert!(
            (stress[0][0] - exp_xx).abs() < 1e-9 * scale_xx,
            "σ_xx = {} expected (λ+2μ)·a = {exp_xx}",
            stress[0][0],
        );
        assert!(
            (stress[1][1] - exp_yy).abs() < 1e-9 * scale_yy,
            "σ_yy = {} expected λ·a = {exp_yy}",
            stress[1][1],
        );
        assert!(
            (stress[2][2] - exp_zz).abs() < 1e-9 * scale_yy,
            "σ_zz = {} expected λ·a = {exp_zz}",
            stress[2][2],
        );
        // Off-diagonals must vanish (within 1e-9 of the largest σ entry).
        let scale_off = exp_xx.abs().max(1.0);
        for (i, j) in [(0, 1), (0, 2), (1, 2)] {
            assert!(
                stress[i][j].abs() < 1e-9 * scale_off,
                "σ[{i}][{j}] = {} expected 0",
                stress[i][j],
            );
        }
    }

    #[test]
    fn element_stress_p1_zero_displacement_yields_zero_stress() {
        // Regression guard: an off-by-one that leaks the D-matrix
        // diagonal into the result for ε = 0 would surface here.
        let mat = dimensionless_steel_like();
        let stress = element_stress_p1(&UNIT_TET_P1, &mat, &[0.0_f64; 12]);
        for i in 0..3 {
            for j in 0..3 {
                assert_eq!(
                    stress[i][j], 0.0,
                    "zero-displacement σ[{i}][{j}] = {} expected 0.0",
                    stress[i][j],
                );
            }
        }
    }
}
