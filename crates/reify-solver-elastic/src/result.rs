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
