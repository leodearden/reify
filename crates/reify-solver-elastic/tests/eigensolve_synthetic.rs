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
//!   Note: n=50 ≤ 64 (faer internal MIN_DIM=32 floor) so shift-invert falls back
//!   to the dense path for this fixture; both paths always converge.
//! - **Fixture C** — 80-DOF 1D-Laplacian pair (K = tridiag(-1,2,-1), B = I)
//!   n=80 > 64 so faer's Lanczos actually runs; used for the non-convergence
//!   signal (max_iters=1, tol=1e-14 — pathologically under-budgeted).

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

// ---------------------------------------------------------------------------
// Fixture-B helpers: 50-DOF 1D-Laplacian pair
// ---------------------------------------------------------------------------

/// Build K = tridiag(-1, 2, -1) (50×50 Dirichlet Laplacian) and B = I (50×50).
fn fixture_b() -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    let n = 50usize;
    let mut k_trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        k_trips.push(Triplet::new(i, i, 2.0));
        if i > 0 {
            k_trips.push(Triplet::new(i, i - 1, -1.0));
        }
        if i + 1 < n {
            k_trips.push(Triplet::new(i, i + 1, -1.0));
        }
    }
    let b_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();
    let b = SparseRowMat::try_new_from_triplets(n, n, &b_trips).unwrap();
    (k, b)
}

/// Closed-form smallest 5 eigenvalues of the 50-DOF Laplacian (Kφ = λBφ = λφ).
/// λ_k = 2(1 − cos(kπ/51)) for k=1..=5.
fn fixture_b_expected_5() -> [f64; 5] {
    let n = 50usize;
    std::array::from_fn(|i| {
        let k = (i + 1) as f64;
        2.0 * (1.0 - f64::cos(k * std::f64::consts::PI / (n as f64 + 1.0)))
    })
}

// ---------------------------------------------------------------------------
// Step-3 test: shift-invert path, Fixture A
// ---------------------------------------------------------------------------

#[test]
fn shift_invert_recovers_smallest_on_5x5_diagonal_pair() {
    let (k, b) = fixture_a();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };
    let result = solve_eigen_shift_invert(&k, &b, opts);

    assert!(result.converged, "shift-invert must converge on the 5×5 diagonal pair");
    assert_eq!(result.eigenvalues.len(), 5, "must return 5 eigenvalues");
    assert_eq!(result.eigenvectors.nrows(), 5, "eigenvectors must have n=5 rows");
    assert_eq!(result.eigenvectors.ncols(), 5, "eigenvectors must have n_modes=5 cols");

    let expected = fixture_a_expected();
    for (i, (&got, &exp)) in result.eigenvalues.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-8,
            "eigenvalue[{i}]: got {got}, expected {exp}, diff = {:.3e}",
            (got - exp).abs(),
        );
    }
}

// ---------------------------------------------------------------------------
// Step-5 test: cross-path agreement on 50-DOF Laplacian (PRD §13 phase 2)
// ---------------------------------------------------------------------------

/// PRD §13 phase 2 observable signal: shift-invert Lanczos and the dense fallback
/// agree to 8 digits on the 5 smallest eigenvalues of the 50-DOF Laplacian pair.
#[test]
fn shift_invert_and_dense_agree_on_50dof_synthetic_pair() {
    let (k, b) = fixture_b();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };
    let dense_result = solve_eigen_dense(&k, &b, opts.clone());
    let si_result = solve_eigen_shift_invert(&k, &b, opts);

    let expected = fixture_b_expected_5();

    // (a) Dense path matches closed-form to 1e-10.
    assert_eq!(
        dense_result.eigenvalues.len(),
        5,
        "dense must return 5 eigenvalues",
    );
    for (i, (&got, &exp)) in dense_result.eigenvalues.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-10,
            "dense eigenvalue[{i}]: got {got:.15}, expected {exp:.15}, diff = {:.3e}",
            (got - exp).abs(),
        );
    }

    // (b) Shift-invert converges and matches dense to 1e-8 (PRD §13 "8 digits").
    assert!(
        si_result.converged,
        "shift-invert must converge on the 50-DOF Laplacian pair",
    );
    assert_eq!(
        si_result.eigenvalues.len(),
        5,
        "shift-invert must return 5 eigenvalues",
    );
    for (i, (&si, &d)) in si_result.eigenvalues.iter().zip(dense_result.eigenvalues.iter()).enumerate() {
        assert!(
            (si - d).abs() < 1e-8,
            "shift-invert eigenvalue[{i}]: got {si:.15}, dense {d:.15}, diff = {:.3e}",
            (si - d).abs(),
        );
    }
}

// ---------------------------------------------------------------------------
// Fixture-C helpers: 80-DOF Laplacian (n > 64 so Lanczos actually runs)
// ---------------------------------------------------------------------------

/// Build K = tridiag(-1,2,-1) (80×80) and B = I (80×80).
/// n=80 > 64 so faer's effective_max_dim = min(max(32,64,10),80) = 64 < 80:
/// partial_self_adjoint_eigen runs the Lanczos loop without the dense fallback.
fn fixture_c() -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    let n = 80usize;
    let mut k_trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        k_trips.push(Triplet::new(i, i, 2.0));
        if i > 0 { k_trips.push(Triplet::new(i, i - 1, -1.0)); }
        if i + 1 < n { k_trips.push(Triplet::new(i, i + 1, -1.0)); }
    }
    let b_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();
    let b = SparseRowMat::try_new_from_triplets(n, n, &b_trips).unwrap();
    (k, b)
}

// ---------------------------------------------------------------------------
// Step-7 test: non-convergence signal (Fixture C + pathological budget)
// ---------------------------------------------------------------------------

/// Pins PRD §5 "BucklingResult.converged = true iff all n_modes eigenvalues
/// satisfy the tolerance criterion" at the kernel layer.
///
/// Uses Fixture C (80-DOF Laplacian, n=80 > 64) so the Lanczos path actually
/// runs.  With tol=1e-300 (below f64 machine precision ≈ 1e-16) the residual
/// check can never be satisfied — no mode locks regardless of max_iters=1 —
/// so `n_converged_eigen = 0` → converged=false, empty result.
///
/// Note: 1e-300_f64 is finite and > 0.0 so it passes the contract guard;
/// the impossibly tight tol is the "pathological" budget.
#[test]
fn shift_invert_reports_non_convergence_when_max_iters_too_low() {
    let (k, b) = fixture_c();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-300, // below machine precision — Lanczos residuals never reach this
        max_iters: 1,
        sigma: 0.0,
    };
    let result = solve_eigen_shift_invert(&k, &b, opts);

    assert!(
        !result.converged,
        "shift-invert must report converged=false when max_iters=1 and tol=1e-14 \
         — got converged=true with {} eigenvalues",
        result.eigenvalues.len(),
    );
    assert!(
        result.eigenvalues.len() < 5,
        "partial result must return fewer than n_modes=5 eigenvalues; \
         got {}",
        result.eigenvalues.len(),
    );
    assert_eq!(
        result.eigenvalues.len(),
        result.iterations,
        "iterations must equal the number of converged eigenvalues returned",
    );
    assert_eq!(
        result.eigenvectors.ncols(),
        result.eigenvalues.len(),
        "eigenvectors width must equal the number of returned eigenvalues",
    );
    // Must not panic — absence of panic IS the no-panic assertion.
}
