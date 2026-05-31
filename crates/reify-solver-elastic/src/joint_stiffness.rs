//! Additive joint-stiffness kernel — PRD compliant-joints-flexures.md task κ §7.2.
//!
//! [`add_joint_stiffness`] accepts an already-assembled global stiffness matrix and a
//! slice of [`JointStiffness`] contributions, returning a new matrix with each joint's
//! spring rate accumulated on its diagonal DOF: `K[dof,dof] += k`.
//!
//! The accumulation relies on faer's duplicate-triplet summation contract (see
//! `assembly/global.rs:60-64`): appending a `(dof, dof, k)` triplet alongside K's
//! stored entries before `SparseRowMat::try_new_from_triplets` gives exact additive
//! semantics. An absent diagonal entry becomes `k`; an existing one becomes
//! `K[dof,dof] + k`. An empty `contributions` slice reproduces the input matrix exactly
//! — the `spring_rate = None` (rigid joint) case, exactly zero contribution.

use faer::sparse::{SparseRowMat, Triplet};

/// One additive diagonal contribution to the global stiffness matrix.
pub struct JointStiffness {
    /// Global DOF index of the joint's spring degree of freedom.
    pub dof: usize,
    /// Spring rate [N/m or N·m/rad]; must be finite.
    pub stiffness: f64,
}

/// Accumulate per-joint diagonal stiffness contributions into the global K.
///
/// Extracts K's stored entries into a triplet list, appends one `(dof, dof, k)`
/// triplet per [`JointStiffness`], and rebuilds via `try_new_from_triplets`. faer
/// sums duplicate `(row, col)` triplets in encounter order (see
/// `assembly/global.rs:60-64`), giving `K_out[dof, dof] = K_in[dof, dof] + k`.
///
/// An empty `contributions` slice produces a structurally identical copy of
/// `k_global` — the `spring_rate = None` (rigid joint) case, preserving existing
/// modal-analysis behaviour (PRD §7.2).
///
/// # Panics
///
/// - `c.dof >= n` where `n = k_global.nrows()` — out-of-range DOF would corrupt
///   the CSR rebuild.
/// - `c.stiffness` is not finite — NaN/Inf would poison the downstream
///   generalized eigenproblem without a diagnostic.
///
/// The kernel is sign-agnostic: a negative but finite stiffness is allowed.
/// SPD-ness of the resulting K is the caller's contract, consistent with
/// `solve_eigen_shift_invert`'s documented SPD precondition.
pub fn add_joint_stiffness(
    k_global: &SparseRowMat<usize, f64>,
    contributions: &[JointStiffness],
) -> SparseRowMat<usize, f64> {
    let _ = contributions;
    unimplemented!(
        "add_joint_stiffness: not yet implemented — step-2 GREEN will fill this in"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use faer::sparse::{SparseRowMat, Triplet};

    // K = [[2, 1], [1, 3]]
    fn small_2x2() -> SparseRowMat<usize, f64> {
        let trips: Vec<Triplet<usize, usize, f64>> = vec![
            Triplet::new(0, 0, 2.0),
            Triplet::new(0, 1, 1.0),
            Triplet::new(1, 0, 1.0),
            Triplet::new(1, 1, 3.0),
        ];
        SparseRowMat::try_new_from_triplets(2, 2, &trips).unwrap()
    }

    fn get_entry(k: &SparseRowMat<usize, f64>, r: usize, c: usize) -> f64 {
        let sym = k.symbolic();
        let cols = sym.col_idx_of_row_raw(r);
        let vals = k.val_of_row(r);
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            if *col_raw == c {
                return val;
            }
        }
        0.0
    }

    // step-1 RED: empty contributions must be a no-op.
    // Fails with "not yet implemented" until step-2 GREEN implements the body.
    #[test]
    fn empty_contributions_noop() {
        let k = small_2x2();
        let result = add_joint_stiffness(&k, &[]);
        assert_eq!(result.nrows(), 2);
        assert_eq!(result.ncols(), 2);
        for r in 0..2 {
            for c in 0..2 {
                assert_eq!(
                    get_entry(&result, r, c),
                    get_entry(&k, r, c),
                    "entry ({r},{c}) changed after empty-contributions call"
                );
            }
        }
    }
}
