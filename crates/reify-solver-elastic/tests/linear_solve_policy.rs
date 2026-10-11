//! Pure contract of the `linear_solve` seam: `SpdSystem::new` validation
//! (C1) and the tier policy `choose_strategy` (C4) — PRD
//! `docs/prds/v0_6/elastic-static-solver-performance.md` §5.

use faer::sparse::{SparseRowMat, SymbolicSparseRowMat, Triplet};
use reify_solver_elastic::linear_solve::{
    FactorEstimate, LinearSolverChoice, PreconditionerKind, SolveFailure, SolvePolicy, SpdSystem,
    Strategy, StrategyDecision, StrategyReason, SystemInvalid, choose_strategy,
};

/// `tridiag(-1, 4, -1)` of size `n`, canonical CSR.
fn tridiagonal(n: usize) -> SparseRowMat<usize, f64> {
    let mut t = Vec::new();
    for i in 0..n {
        t.push(Triplet::new(i, i, 4.0));
        if i + 1 < n {
            t.push(Triplet::new(i, i + 1, -1.0));
            t.push(Triplet::new(i + 1, i, -1.0));
        }
    }
    SparseRowMat::try_new_from_triplets(n, n, &t).expect("tridiagonal triplets")
}

/// A CSR matrix whose rows hold exactly `rows[i]`, in the given order, with
/// no sorting or deduplication.
fn csr_as_given(ncols: usize, rows: &[Vec<(usize, f64)>]) -> SparseRowMat<usize, f64> {
    let mut row_ptr = vec![0usize];
    let mut col_idx = Vec::new();
    let mut vals = Vec::new();
    for row in rows {
        for &(c, v) in row {
            col_idx.push(c);
            vals.push(v);
        }
        row_ptr.push(col_idx.len());
    }
    let symbolic =
        SymbolicSparseRowMat::new_unsorted_checked(rows.len(), ncols, row_ptr, None, col_idx);
    SparseRowMat::new(symbolic, vals)
}

/// A canonical 4×4 diagonal-dominant matrix with `diag[i]` on the diagonal
/// (`None` = the diagonal is not stored) and `-1` coupling rows 0–1 and 2–3.
fn with_diagonal(diag: [Option<f64>; 4]) -> SparseRowMat<usize, f64> {
    let rows: Vec<Vec<(usize, f64)>> = (0..4)
        .map(|i| {
            let partner = i ^ 1;
            let mut row = vec![(partner, -1.0)];
            if let Some(d) = diag[i] {
                row.push((i, d));
            }
            row.sort_by_key(|&(c, _)| c);
            row
        })
        .collect();
    csr_as_given(4, &rows)
}

#[test]
fn canonical_spd_matrix_is_accepted() {
    let system = SpdSystem::new(tridiagonal(5)).expect("tridiag(-1,4,-1) is a valid SpdSystem");
    assert_eq!(system.n_dofs(), 5);
}

#[test]
fn non_square_matrix_is_rejected_with_its_shape() {
    let t = [
        Triplet::new(0, 0, 1.0),
        Triplet::new(1, 1, 1.0),
        Triplet::new(2, 2, 1.0),
    ];
    let k = SparseRowMat::try_new_from_triplets(3, 4, &t).expect("3x4 triplets");
    assert_eq!(
        SpdSystem::new(k).err(),
        Some(SystemInvalid::NotSquare { rows: 3, cols: 4 })
    );
}

#[test]
fn unsorted_row_is_non_canonical() {
    let k = csr_as_given(
        3,
        &[
            vec![(0, 4.0), (1, -1.0)],
            vec![(2, -1.0), (1, 4.0), (0, -1.0)],
            vec![(1, -1.0), (2, 4.0)],
        ],
    );
    assert_eq!(SpdSystem::new(k).err(), Some(SystemInvalid::NonCanonical));
}

#[test]
fn duplicated_column_is_non_canonical() {
    let k = csr_as_given(
        3,
        &[
            vec![(0, 4.0), (1, -1.0)],
            vec![(0, -1.0), (1, 2.0), (1, 2.0), (2, -1.0)],
            vec![(1, -1.0), (2, 4.0)],
        ],
    );
    assert_eq!(SpdSystem::new(k).err(), Some(SystemInvalid::NonCanonical));
}

#[test]
fn non_compressed_storage_is_non_canonical() {
    // Row 1 reserves a third slot that `row_nnz` leaves unused.
    let row_ptr = vec![0usize, 2, 5, 7];
    let row_nnz = Some(vec![2usize, 2, 2]);
    let col_idx = vec![0usize, 1, 0, 1, 2, 1, 2];
    let vals = vec![4.0, -1.0, -1.0, 4.0, 0.0, -1.0, 4.0];
    let symbolic = SymbolicSparseRowMat::new_unsorted_checked(3, 3, row_ptr, row_nnz, col_idx);
    let k = SparseRowMat::new(symbolic, vals);
    assert_eq!(SpdSystem::new(k).err(), Some(SystemInvalid::NonCanonical));
}

#[test]
fn missing_diagonal_is_rejected() {
    let k = with_diagonal([Some(4.0), None, Some(4.0), Some(4.0)]);
    assert_eq!(
        SpdSystem::new(k).err(),
        Some(SystemInvalid::NonPositiveDiagonal { dof: 1 })
    );
}

#[test]
fn zero_diagonal_is_rejected() {
    let k = with_diagonal([Some(4.0), Some(4.0), Some(0.0), Some(4.0)]);
    assert_eq!(
        SpdSystem::new(k).err(),
        Some(SystemInvalid::NonPositiveDiagonal { dof: 2 })
    );
}

#[test]
fn negative_diagonal_is_rejected() {
    let k = with_diagonal([Some(-4.0), Some(4.0), Some(4.0), Some(4.0)]);
    assert_eq!(
        SpdSystem::new(k).err(),
        Some(SystemInvalid::NonPositiveDiagonal { dof: 0 })
    );
}

#[test]
fn nan_diagonal_is_rejected() {
    let k = with_diagonal([Some(4.0), Some(4.0), Some(4.0), Some(f64::NAN)]);
    assert_eq!(
        SpdSystem::new(k).err(),
        Some(SystemInvalid::NonPositiveDiagonal { dof: 3 })
    );
}

#[test]
fn lowest_offending_dof_is_named() {
    let k = with_diagonal([Some(4.0), Some(0.0), Some(4.0), None]);
    assert_eq!(
        SpdSystem::new(k).err(),
        Some(SystemInvalid::NonPositiveDiagonal { dof: 1 })
    );
}

#[test]
fn shape_is_checked_before_canonical_form() {
    let k = csr_as_given(
        4,
        &[
            vec![(1, -1.0), (0, 4.0)],
            vec![(0, -1.0), (1, 4.0)],
            vec![(2, 4.0)],
        ],
    );
    assert_eq!(
        SpdSystem::new(k).err(),
        Some(SystemInvalid::NotSquare { rows: 3, cols: 4 })
    );
}

#[test]
fn canonical_form_is_checked_before_the_diagonal() {
    let k = csr_as_given(
        3,
        &[
            vec![(0, -4.0), (1, -1.0)],
            vec![(1, 4.0), (0, -1.0)],
            vec![(2, 4.0)],
        ],
    );
    assert_eq!(SpdSystem::new(k).err(), Some(SystemInvalid::NonCanonical));
}

// ---------------------------------------------------------------------------
// choose_strategy (C4)
// ---------------------------------------------------------------------------

const GIB: u64 = 1024 * 1024 * 1024;

const ESTIMATE: FactorEstimate = FactorEstimate {
    n_dofs: 100,
    len_val: 1000,
    factor_bytes: 10_000,
    predicted_flops: 5.0e4,
};

const JACOBI: Strategy = Strategy::Pcg(PreconditionerKind::Jacobi);

fn policy(auto_direct_budget: u64, direct_ceiling: u64, work: Option<f64>) -> SolvePolicy {
    SolvePolicy {
        auto_direct_budget,
        direct_ceiling,
        auto_direct_work_budget: work,
    }
}

fn decision(strategy: Strategy, reason: StrategyReason) -> Result<StrategyDecision, SolveFailure> {
    Ok(StrategyDecision { strategy, reason })
}

#[test]
fn default_policy_budgets() {
    let p = SolvePolicy::default();
    assert_eq!(p.auto_direct_budget, 2 * 1024u64.pow(3));
    assert_eq!(p.direct_ceiling, 8 * 1024u64.pow(3));
    assert_eq!(p.auto_direct_work_budget, None);
}

#[test]
fn auto_picks_direct_at_exactly_the_byte_budget() {
    assert_eq!(
        choose_strategy(
            LinearSolverChoice::Auto,
            Some(&ESTIMATE),
            &policy(10_000, GIB, None)
        ),
        decision(
            Strategy::Direct,
            StrategyReason::AutoWithinBudget {
                predicted_bytes: 10_000,
                budget: 10_000
            }
        )
    );
}

#[test]
fn auto_one_byte_over_budget_goes_iterative() {
    assert_eq!(
        choose_strategy(
            LinearSolverChoice::Auto,
            Some(&ESTIMATE),
            &policy(9_999, GIB, None)
        ),
        decision(
            JACOBI,
            StrategyReason::AutoOverBudget {
                predicted_bytes: 10_000,
                budget: 9_999
            }
        )
    );
}

#[test]
fn auto_picks_direct_at_exactly_the_work_budget() {
    assert_eq!(
        choose_strategy(
            LinearSolverChoice::Auto,
            Some(&ESTIMATE),
            &policy(10_000, GIB, Some(5.0e4))
        ),
        decision(
            Strategy::Direct,
            StrategyReason::AutoWithinBudget {
                predicted_bytes: 10_000,
                budget: 10_000
            }
        )
    );
}

#[test]
fn auto_over_the_work_budget_goes_iterative() {
    assert_eq!(
        choose_strategy(
            LinearSolverChoice::Auto,
            Some(&ESTIMATE),
            &policy(10_000, GIB, Some(4.9e4))
        ),
        decision(
            JACOBI,
            StrategyReason::AutoOverWorkBudget {
                predicted_flops: 5.0e4,
                budget: 4.9e4
            }
        )
    );
}

#[test]
fn auto_over_both_budgets_names_the_memory_term() {
    assert_eq!(
        choose_strategy(
            LinearSolverChoice::Auto,
            Some(&ESTIMATE),
            &policy(9_999, GIB, Some(4.9e4))
        ),
        decision(
            JACOBI,
            StrategyReason::AutoOverBudget {
                predicted_bytes: 10_000,
                budget: 9_999
            }
        )
    );
}

#[test]
fn direct_within_the_ceiling_is_requested() {
    assert_eq!(
        choose_strategy(
            LinearSolverChoice::Direct,
            Some(&ESTIMATE),
            &policy(GIB, 10_000, None)
        ),
        decision(Strategy::Direct, StrategyReason::Requested)
    );
}

#[test]
fn direct_over_the_ceiling_fails() {
    assert_eq!(
        choose_strategy(
            LinearSolverChoice::Direct,
            Some(&ESTIMATE),
            &policy(GIB, 9_999, None)
        ),
        Err(SolveFailure::FactorExceedsCeiling {
            predicted: 10_000,
            ceiling: 9_999
        })
    );
}

#[test]
fn direct_reads_only_the_ceiling() {
    assert_eq!(
        choose_strategy(
            LinearSolverChoice::Direct,
            Some(&ESTIMATE),
            &policy(1, GIB, Some(0.0))
        ),
        decision(Strategy::Direct, StrategyReason::Requested)
    );
}

#[test]
fn iterative_is_requested_with_or_without_an_estimate() {
    let p = SolvePolicy::default();
    for estimate in [None, Some(&ESTIMATE)] {
        assert_eq!(
            choose_strategy(LinearSolverChoice::Iterative, estimate, &p),
            decision(JACOBI, StrategyReason::Requested)
        );
    }
}

#[test]
#[should_panic(expected = "Auto")]
fn auto_without_an_estimate_is_a_contract_violation() {
    let _ = choose_strategy(LinearSolverChoice::Auto, None, &SolvePolicy::default());
}

#[test]
#[should_panic(expected = "Direct")]
fn direct_without_an_estimate_is_a_contract_violation() {
    let _ = choose_strategy(LinearSolverChoice::Direct, None, &SolvePolicy::default());
}
