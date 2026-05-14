//! Synthetic behavioural tests for the shift-invert Lanczos + dense generalized
//! eigensolver kernel.
//!
//! PRD reference: `docs/prds/v0_5/buckling-eigensolver.md` §5 / §13 phase 2
//! task β observable signal.
//!
//! # Test fixtures
//!
//! - **Fixture A** — 5×5 diagonal pair (K = I, B = diag(1,2,3,4,5))
//!   Closed-form spectrum: ascending |λ| = [0.2, 0.25, 1/3, 0.5, 1.0].
//! - **Fixture B** — 50-DOF 1D-Laplacian pair (K = tridiag(-1,2,-1), B = I)
//!   Closed-form: λ_k = 2(1 − cos(kπ/51)) for k=1..50.
//! - **Fixture C** — pathological budget (Fixture B + max_iters=1 + tol=1e-14)
//!   for non-convergence signal.

use faer::sparse::{SparseRowMat, Triplet};
use reify_solver_elastic::eigensolve::{EigenSolverOptions, solve_eigen_dense, solve_eigen_shift_invert};

// ---------------------------------------------------------------------------
// Fixture-A helpers
// ---------------------------------------------------------------------------

/// Build K = I (5×5 identity) and B = diag(1,2,3,4,5) as SparseRowMat.
fn fixture_a() -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    let n = 5;
    let k_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    let b_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, (i + 1) as f64)).collect();
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();
    let b = SparseRowMat::try_new_from_triplets(n, n, &b_trips).unwrap();
    (k, b)
}

/// Closed-form ascending-|λ| spectrum for Fixture A.
/// Kφ = λBφ → Iφ = λ·diag(b_i)φ → λ_i = 1/b_i.
/// Ascending: 1/5, 1/4, 1/3, 1/2, 1/1.
fn fixture_a_expected() -> [f64; 5] {
    [0.2, 0.25, 1.0 / 3.0, 0.5, 1.0]
}

// ---------------------------------------------------------------------------
// Step-1 test: dense path, Fixture A
// ---------------------------------------------------------------------------

#[test]
fn dense_recovers_known_spectrum_on_5x5_diagonal_pair() {
    let (k, b) = fixture_a();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-12,
        max_iters: 1,
        sigma: 0.0,
    };
    let result = solve_eigen_dense(&k, &b, opts);

    assert!(result.converged, "dense path must always converge (direct solver)");
    assert_eq!(result.iterations, 0, "dense path iterations must be 0 (direct solver)");
    assert_eq!(result.eigenvalues.len(), 5, "must return 5 eigenvalues");

    let expected = fixture_a_expected();
    for (i, (&got, &exp)) in result.eigenvalues.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-10,
            "eigenvalue[{i}]: got {got}, expected {exp}, diff = {:.3e}",
            (got - exp).abs(),
        );
    }
}
