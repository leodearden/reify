//! Consistent mass-matrix kernels.
//!
//! See PRD `docs/prds/v0_3/modal-analysis.md` §10 Phase 1 task δ:
//! "consistent mass for tet4 elements; lumped variant deferred to Open
//! Question §12.3". v0.3 ships a single P1 tetrahedron kernel; hex/wedge/
//! shell mass kernels and the row-sum-lumped variant are out of scope for
//! this task.
//!
//! The element matrix shares the row-major `(3·node + axis)` DOF layout of
//! [`crate::assembly::ElementStiffness`], so the global mass matrix `M` is
//! assembled by handing each element `M_e` to the existing
//! [`crate::assemble_global_stiffness`] scatter primitive — no new
//! global-API surface needed (the assembler is agnostic to `K` vs `K_g`
//! vs `M`).

use crate::assembly::ElementStiffness;

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonical unit reference tet — vertices `(0,0,0), (1,0,0), (0,1,0),
    /// (0,0,1)` with reference volume `1/6`. Mirrors the constant in
    /// `geometric_stiffness/tet.rs::tests::UNIT_TET`.
    const UNIT_TET: [[f64; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];

    #[test]
    fn consistent_mass_tet_p1_returns_12_by_12_element_stiffness() {
        let m_e = consistent_element_mass_tet_p1(&UNIT_TET, 1.0);
        assert_eq!(m_e.n_dofs, 12, "P1 tet M_e must be 12-DOF (4 nodes × 3 axes)");
        assert_eq!(
            m_e.data.len(),
            144,
            "row-major 12×12 storage must have 144 entries"
        );
    }
}
