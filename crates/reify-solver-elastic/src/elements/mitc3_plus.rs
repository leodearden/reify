//! MITC3+ Reissner-Mindlin shell element (Bathe & Lee 2014).
//!
//! # Reference
//!
//! Bathe, K.-J. & Lee, P.-S. (2014). "Towards improving the MITC9 shell
//! element." *Computers & Structures*, 81, 477–489; and Bathe, K.-J. &
//! Lee, P.-S. (2011). "An improvement of the MITC shell elements."
//! *Computers & Structures*, 89, 1413–1422.
//!
//! # Element description
//!
//! Three-node triangular shell element parameterized on a 2D mid-surface
//! reference triangle with vertices `(0,0)`, `(1,0)`, `(0,1)` in local
//! `(ξ, η)` coordinates.  Each node carries 6 DOFs (3 displacement + 3
//! rotation), giving 18 DOFs per element.
//!
//! The "+" distinguishes MITC3+ from plain MITC3: the rotation field is
//! enriched by a deviatoric cubic bubble `f_b(ξ,η) = ξ·η·(1−ξ−η)` that
//! eliminates spurious transverse-shear locking without additional DOFs.
//! Transverse-shear strains are interpolated from values sampled at the
//! three canonical edge-midpoint tying points A=(½,0), B=(0,½), C=(½,½).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mitc3_plus_dof_constants_match_18_dof_specification() {
        assert_eq!(Mitc3Plus::N_NODES, 3);
        assert_eq!(Mitc3Plus::N_DOFS_PER_NODE, 6);
        assert_eq!(Mitc3Plus::N_DOFS, 18);
        assert_eq!(Mitc3Plus::N_TYING_POINTS, 3);
    }
}
