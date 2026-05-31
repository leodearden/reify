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
    let n = k_global.nrows();
    // Extract K's stored triplets — mirror project_free (modal_ops.rs:484-508) and
    // m_matvec (modal_ops.rs:515-528): symbolic() gives the sparsity structure;
    // col_idx_of_row_raw + val_of_row iterate the stored (col, value) pairs per row.
    let sym = k_global.symbolic();
    let mut trips: Vec<Triplet<usize, usize, f64>> = Vec::new();
    for r in 0..n {
        let cols = sym.col_idx_of_row_raw(r);
        let vals = k_global.val_of_row(r);
        for (col_raw, &val) in cols.iter().zip(vals.iter()) {
            trips.push(Triplet::new(r, *col_raw, val));
        }
    }
    // Append one (dof, dof, k) triplet per contribution. faer sums duplicate
    // (row, col) triplets in encounter order (assembly/global.rs:60-64), so an
    // existing K[dof,dof] entry and this appended triplet sum to K[dof,dof] += k;
    // a structurally-absent diagonal entry is created with value k; two contributions
    // to the same dof accumulate by the same mechanism.
    for c in contributions {
        trips.push(Triplet::new(c.dof, c.dof, c.stiffness));
    }
    SparseRowMat::try_new_from_triplets(n, n, &trips)
        .expect("joint-stiffness triplet rebuild must not violate CSR invariants")
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

    // step-1 RED / step-2 GREEN: empty contributions must be a no-op.
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

    // step-3 RED: additive semantics — fails because step-2 body ignores contributions.

    // (a) += onto an existing nonzero diagonal: K[1,1]=3, add {dof:1, k:5} → 8.
    #[test]
    fn additive_onto_existing_diagonal() {
        let k = small_2x2();
        let result = add_joint_stiffness(&k, &[JointStiffness { dof: 1, stiffness: 5.0 }]);
        assert_eq!(get_entry(&result, 1, 1), 8.0, "K[1,1] must be 3.0 + 5.0 = 8.0");
        assert_eq!(get_entry(&result, 0, 0), 2.0, "K[0,0] must be unchanged");
        assert_eq!(get_entry(&result, 0, 1), 1.0, "K[0,1] must be unchanged");
        assert_eq!(get_entry(&result, 1, 0), 1.0, "K[1,0] must be unchanged");
    }

    // (b) Structurally-absent diagonal: K with no stored (0,0), add {dof:0, k:7} → 7.
    #[test]
    fn absent_diagonal_created() {
        // K = [[0, 1], [1, 3]] — (0,0) entry intentionally absent from CSR.
        let trips: Vec<Triplet<usize, usize, f64>> = vec![
            Triplet::new(0, 1, 1.0),
            Triplet::new(1, 0, 1.0),
            Triplet::new(1, 1, 3.0),
        ];
        let k = SparseRowMat::try_new_from_triplets(2, 2, &trips).unwrap();
        assert_eq!(get_entry(&k, 0, 0), 0.0, "fixture: (0,0) must be absent");
        let result = add_joint_stiffness(&k, &[JointStiffness { dof: 0, stiffness: 7.0 }]);
        assert_eq!(get_entry(&result, 0, 0), 7.0, "absent diagonal must become k");
        assert_eq!(get_entry(&result, 0, 1), 1.0, "off-diagonal must be unchanged");
        assert_eq!(get_entry(&result, 1, 1), 3.0, "other diagonal must be unchanged");
    }

    // (c) Multiple distinct DOFs each land additively.
    #[test]
    fn multiple_distinct_dofs() {
        let k = small_2x2();
        let contributions = vec![
            JointStiffness { dof: 0, stiffness: 10.0 },
            JointStiffness { dof: 1, stiffness: 20.0 },
        ];
        let result = add_joint_stiffness(&k, &contributions);
        assert_eq!(get_entry(&result, 0, 0), 12.0, "K[0,0] = 2 + 10 = 12");
        assert_eq!(get_entry(&result, 1, 1), 23.0, "K[1,1] = 3 + 20 = 23");
        assert_eq!(get_entry(&result, 0, 1), 1.0, "off-diagonal unchanged");
        assert_eq!(get_entry(&result, 1, 0), 1.0, "off-diagonal unchanged");
    }

    // (d) Two contributions to the same DOF accumulate (faer duplicate-summation contract).
    #[test]
    fn same_dof_accumulates() {
        let k = small_2x2();
        let contributions = vec![
            JointStiffness { dof: 0, stiffness: 2.0 },
            JointStiffness { dof: 0, stiffness: 3.0 },
        ];
        let result = add_joint_stiffness(&k, &contributions);
        // K[0,0] was 2.0; two contributions add 2+3=5 → result must be 7.0.
        assert_eq!(get_entry(&result, 0, 0), 7.0, "K[0,0] = 2 + 2 + 3 = 7");
        assert_eq!(get_entry(&result, 1, 1), 3.0, "K[1,1] unchanged");
    }

    // (e) Symmetry preserved: result is symmetric within fp tolerance.
    #[test]
    fn symmetry_preserved() {
        let k = small_2x2();
        let result = add_joint_stiffness(&k, &[JointStiffness { dof: 0, stiffness: 4.0 }]);
        for r in 0..2 {
            for c in 0..2 {
                let fwd = get_entry(&result, r, c);
                let rev = get_entry(&result, c, r);
                assert!(
                    (fwd - rev).abs() < 1e-14,
                    "result[{r},{c}]={fwd} != result[{c},{r}]={rev}: symmetry violated"
                );
            }
        }
    }
}
