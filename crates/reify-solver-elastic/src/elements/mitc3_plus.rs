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

/// MITC3+ Reissner-Mindlin triangular shell element.
///
/// Three-node element on the reference triangle with vertices `(0,0)`,
/// `(1,0)`, `(0,1)`. Each node carries 6 DOFs (3 displacements + 3
/// rotations), totalling `N_DOFS = 18` per element.
pub struct Mitc3Plus;

impl Mitc3Plus {
    /// Number of Lagrangian nodes.
    pub const N_NODES: usize = 3;

    /// DOFs per node (3 displacement + 3 rotation).
    pub const N_DOFS_PER_NODE: usize = 6;

    /// Total DOFs per element: `N_NODES × N_DOFS_PER_NODE = 18`.
    pub const N_DOFS: usize = Self::N_NODES * Self::N_DOFS_PER_NODE;

    /// Number of edge-midpoint tying points for the assumed transverse-shear
    /// strain interpolation (A, B, C in Bathe & Lee 2014 notation).
    pub const N_TYING_POINTS: usize = 3;
}

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
