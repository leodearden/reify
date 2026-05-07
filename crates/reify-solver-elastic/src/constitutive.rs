//! Constitutive laws for the linear-elastostatic FEA solver.
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8. This module
//! ships the isotropic linear-elastic 6×6 D-matrix used by element-stiffness
//! assembly. The Voigt component order is `[εxx, εyy, εzz, γxy, γyz, γxz]`
//! with **engineering shear strain** (`γ = 2ε`); see [`IsotropicElastic`] for
//! the convention details.

/// Isotropic linear-elastic constitutive law parameterised by Young's
/// modulus `E` and Poisson's ratio `ν`.
///
/// # Voigt convention
///
/// The 6×6 matrix returned by [`IsotropicElastic::d_matrix`] maps a
/// **Voigt strain vector with engineering shear** to a Voigt stress vector,
///
/// ```text
/// ε = [ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]ᵀ          (γ_ij = 2 ε_ij)
/// σ = [σ_xx, σ_yy, σ_zz, σ_xy, σ_yz, σ_xz]ᵀ
/// σ = D · ε
/// ```
///
/// Because shear strain enters as the engineering quantity `γ = 2ε`, the
/// shear-block diagonal of `D` is the shear modulus `μ = G = E / (2(1+ν))`
/// directly — **without** the additional factor of 2 that appears when
/// using tensorial shear strain. Consumers that build the
/// strain-displacement matrix `B` must match this convention by placing
/// `(∂N/∂y, ∂N/∂x, 0)` (no halving) in the row corresponding to `γ_xy`.
///
/// # Lamé form
///
/// Internally the D matrix is written in Lamé form. With
/// `factor = E / ((1+ν)(1−2ν))`,
///
/// ```text
/// λ      = factor · ν                  (Lamé first parameter)
/// 2μ     = factor · (1 − 2ν)           (twice the shear modulus)
/// μ      = factor · (1 − 2ν) / 2       (shear modulus G)
/// ```
///
/// then
///
/// ```text
/// D = [ λ+2μ   λ     λ     0   0   0
///       λ      λ+2μ  λ     0   0   0
///       λ      λ     λ+2μ  0   0   0
///       0      0     0     μ   0   0
///       0      0     0     0   μ   0
///       0      0     0     0   0   μ ]
/// ```
///
/// # Preconditions
///
/// `0 ≤ ν < 0.5`. The strict upper bound excludes the incompressible limit
/// where `factor` blows up; this matches the stdlib `ElasticMaterial`
/// constraint at `crates/reify-compiler/stdlib/materials_fea.ri:97-103`.
/// `youngs_modulus` should be positive (any consistent units — the D matrix
/// is linear in `E`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IsotropicElastic {
    /// Young's modulus `E` (any consistent unit; the D matrix is linear in `E`).
    pub youngs_modulus: f64,
    /// Poisson's ratio `ν`. Must satisfy `0 ≤ ν < 0.5`.
    pub poisson_ratio: f64,
}

impl IsotropicElastic {
    /// Return the 6×6 elasticity matrix `D` in engineering-strain Voigt form.
    ///
    /// See the type-level documentation for the Voigt component order
    /// (`[ε_xx, ε_yy, ε_zz, γ_xy, γ_yz, γ_xz]`) and the rationale for the
    /// shear-block diagonal being `μ = G` (not `2G`).
    #[allow(clippy::needless_range_loop)]
    pub fn d_matrix(&self) -> [[f64; 6]; 6] {
        let e = self.youngs_modulus;
        let nu = self.poisson_ratio;
        let factor = e / ((1.0 + nu) * (1.0 - 2.0 * nu));
        let lambda = factor * nu;
        let two_mu = factor * (1.0 - 2.0 * nu);
        let mu = 0.5 * two_mu;
        let lambda_plus_two_mu = lambda + two_mu;

        let mut d = [[0.0_f64; 6]; 6];
        // Normal-stress block (rows/cols 0..3).
        for i in 0..3 {
            for j in 0..3 {
                d[i][j] = if i == j { lambda_plus_two_mu } else { lambda };
            }
        }
        // Shear-stress block (rows/cols 3..6) — diagonal μ, off-diagonal 0.
        for k in 3..6 {
            d[k][k] = mu;
        }
        // Off-diagonal blocks are zero (initialised that way).
        d
    }
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;

    /// Multiply a 6×6 matrix by a 6-vector.
    fn matvec(d: &[[f64; 6]; 6], v: &[f64; 6]) -> [f64; 6] {
        let mut out = [0.0_f64; 6];
        for i in 0..6 {
            for j in 0..6 {
                out[i] += d[i][j] * v[j];
            }
        }
        out
    }

    /// Steel-like reference: E = 200 GPa, ν = 0.3 (Pa, dimensionless).
    fn steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 200.0e9,
            poisson_ratio: 0.3,
        }
    }

    #[test]
    fn d_matrix_is_symmetric_for_steel_like_inputs() {
        let d = steel_like().d_matrix();
        for i in 0..6 {
            for j in 0..6 {
                let lhs = d[i][j];
                let rhs = d[j][i];
                let scale = lhs.abs().max(rhs.abs()).max(1.0);
                assert!(
                    (lhs - rhs).abs() < 1e-9 * scale,
                    "asymmetry at ({i},{j}): {lhs} vs {rhs}",
                );
            }
        }
    }

    #[test]
    fn d_matrix_hydrostatic_strain_yields_hydrostatic_stress_with_bulk_modulus() {
        // ε_v = 1e-4 in each normal slot; expect σ_xx = σ_yy = σ_zz and
        // trace(σ)/3 = K · trace(ε), K = E / (3 (1 − 2ν)).
        let mat = steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let bulk = e / (3.0 * (1.0 - 2.0 * nu));
        let eps_v = 1.0e-4;
        let strain = [eps_v, eps_v, eps_v, 0.0, 0.0, 0.0];

        let sigma = matvec(&mat.d_matrix(), &strain);

        let trace_sigma = sigma[0] + sigma[1] + sigma[2];
        let trace_eps = 3.0 * eps_v;
        let expected_mean = bulk * trace_eps;
        let actual_mean = trace_sigma / 3.0;
        assert!(
            (actual_mean - expected_mean).abs() < 1e-9 * expected_mean.abs(),
            "mean stress: got {actual_mean}, expected {expected_mean}",
        );

        // All three normal components equal under hydrostatic loading.
        let scale = sigma[0].abs().max(1.0);
        assert!((sigma[0] - sigma[1]).abs() < 1e-9 * scale);
        assert!((sigma[0] - sigma[2]).abs() < 1e-9 * scale);

        // No shear response under hydrostatic strain.
        for k in 3..6 {
            assert!(sigma[k].abs() < 1e-9 * scale, "shear leak at {k}: {}", sigma[k]);
        }
    }

    #[test]
    fn d_matrix_pure_shear_strain_yields_shear_stress_via_g() {
        // ε = (0, 0, 0, γ, 0, 0) → σ_xy = G·γ with G = E / (2(1+ν));
        // all other σ-components vanish.
        let mat = steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let g = e / (2.0 * (1.0 + nu));
        let gamma = 2.5e-4;
        let strain = [0.0, 0.0, 0.0, gamma, 0.0, 0.0];

        let sigma = matvec(&mat.d_matrix(), &strain);

        let expected_shear = g * gamma;
        assert!(
            (sigma[3] - expected_shear).abs() < 1e-9 * expected_shear.abs(),
            "σ_xy: got {}, expected {expected_shear}",
            sigma[3],
        );

        // Other five components must vanish.
        let scale = sigma[3].abs().max(1.0);
        for (k, val) in sigma.iter().enumerate() {
            if k == 3 {
                continue;
            }
            assert!(val.abs() < 1e-9 * scale, "non-zero σ[{k}] = {val}");
        }
    }

    #[test]
    fn d_matrix_zero_poisson_limit_is_diagonal_with_e_and_e_over_two() {
        // ν = 0 ⇒ λ = 0, μ = E/2; the D matrix collapses to
        // diag(E, E, E, E/2, E/2, E/2).
        let e: f64 = 1.0;
        let mat = IsotropicElastic {
            youngs_modulus: e,
            poisson_ratio: 0.0,
        };
        let d = mat.d_matrix();
        for i in 0..6 {
            for j in 0..6 {
                let expected: f64 = if i == j {
                    if i < 3 { e } else { e / 2.0 }
                } else {
                    0.0
                };
                let scale = expected.abs().max(1.0);
                assert!(
                    (d[i][j] - expected).abs() < 1e-9 * scale,
                    "D[{i}][{j}] = {} (expected {expected})",
                    d[i][j],
                );
            }
        }
    }

    #[test]
    fn d_matrix_uniaxial_strain_recovers_lame_diagonal_and_off_diagonal() {
        // ε = (1, 0, 0, 0, 0, 0) ⇒ σ_xx = λ + 2μ, σ_yy = σ_zz = λ,
        // shears all zero.
        let mat = steel_like();
        let e = mat.youngs_modulus;
        let nu = mat.poisson_ratio;
        let factor = e / ((1.0 + nu) * (1.0 - 2.0 * nu));
        let lambda = factor * nu;
        let two_mu = factor * (1.0 - 2.0 * nu);
        let lambda_plus_two_mu = lambda + two_mu;

        let strain = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let sigma = matvec(&mat.d_matrix(), &strain);

        assert!(
            (sigma[0] - lambda_plus_two_mu).abs() < 1e-9 * lambda_plus_two_mu.abs(),
            "σ_xx: got {}, expected λ+2μ = {lambda_plus_two_mu}",
            sigma[0],
        );
        assert!(
            (sigma[1] - lambda).abs() < 1e-9 * lambda.abs(),
            "σ_yy: got {}, expected λ = {lambda}",
            sigma[1],
        );
        assert!(
            (sigma[2] - lambda).abs() < 1e-9 * lambda.abs(),
            "σ_zz: got {}, expected λ = {lambda}",
            sigma[2],
        );
        for k in 3..6 {
            let scale = sigma[0].abs().max(1.0);
            assert!(sigma[k].abs() < 1e-9 * scale, "σ[{k}] should vanish, got {}", sigma[k]);
        }
    }
}
