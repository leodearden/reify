//! Tetrahedral element-stiffness assembly (P1 and P2).
//!
//! See PRD `docs/prds/v0_3/structural-analysis-fea.md` task #8.

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
                u[3 * node + 0] = omega[1] * x[2] - omega[2] * x[1];
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
