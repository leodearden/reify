//! Shell-element stiffness assembly for the MITC3+ Reissner-Mindlin shell.
//!
//! # PRD reference
//!
//! `docs/prds/v0_4/structural-analysis-shells.md` task T6.
//!
//! # Overview
//!
//! Computes the per-element 18×18 stiffness matrix for the MITC3+
//! Reissner-Mindlin shell under a constant-thickness isotropic linear-elastic
//! constitutive law. Through-thickness integration is closed-form; element K
//! is assembled in a local mid-surface frame and then rotated into the global
//! frame so it is ready for the global sparse-assembly consumer (PRD T#11).
//! Output is a [`crate::assembly::ElementStiffness`] with `n_dofs = 18`.

#[cfg(test)]
mod tests {
    use crate::assembly::ElementStiffness;
    use crate::constitutive::IsotropicElastic;
    use super::*;

    fn steel_like() -> IsotropicElastic {
        IsotropicElastic {
            youngs_modulus: 200.0e9,
            poisson_ratio: 0.3,
        }
    }

    const UNIT_TRI: [[f64; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
    ];

    #[test]
    fn shell_element_stiffness_returns_18_by_18_for_unit_triangle() {
        let k = shell_element_stiffness(&UNIT_TRI, 0.05, &steel_like());
        assert_eq!(k.n_dofs, 18);
        assert_eq!(k.data.len(), 324);
    }
}
