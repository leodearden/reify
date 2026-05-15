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
//!   Note: n=50 ≤ 64 (2·faer MIN_DIM floor) so the shift-invert entry point
//!   falls back to the dense path.  Used here for dense closed-form pinning
//!   and to assert the fallback-routing contract — Lanczos numerics are
//!   verified on Fixture C below.
//! - **Fixture C** — 80-DOF 1D-Laplacian pair (K = tridiag(-1,2,-1), B = I)
//!   n=80 > 64 so faer's Lanczos actually runs.  Used both for the PRD §13
//!   phase-2 cross-path agreement signal (shift-invert vs. dense to 1e-8)
//!   and for the non-convergence signal (max_iters=1, tol=1e-300 —
//!   pathologically under-budgeted).

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
    assert_eq!(result.n_converged, 0, "dense path n_converged must be 0 (direct solver)");
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
// Step-3 test: shift-invert entry-point routing on a sub-MIN_DIM problem.
//
// n=5 is well below faer's MIN_DIM=32 Krylov floor, so the
// `effective_max_dim >= n` branch in `solve_eigen_shift_invert` routes the
// call straight to `solve_eigen_dense`.  The Lanczos numerical path is NOT
// exercised here — that is intentional: this test pins the routing/fallback
// contract.  Lanczos numerics are exercised in
// `shift_invert_and_dense_agree_on_80dof_synthetic_pair` below.
// ---------------------------------------------------------------------------

#[test]
fn shift_invert_routes_5x5_diagonal_pair_through_dense_fallback() {
    let (k, b) = fixture_a();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };
    let result = solve_eigen_shift_invert(&k, &b, opts);

    // Fallback → dense always reports n_converged=0 (direct path, no Lanczos).
    assert!(result.converged, "fallback to dense must converge on the 5×5 diagonal pair");
    assert_eq!(
        result.n_converged, 0,
        "dense-fallback n_converged must be 0; got {} (suggests Lanczos was reached)",
        result.n_converged,
    );
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
// Step-5 test: dense path closed-form agreement on 50-DOF Laplacian.
//
// n=50 is still below the effective Krylov window (2·MIN_DIM=64), so the
// shift-invert entry point falls back to dense.  This test keeps the
// Fixture-B closed-form check on the dense path; it does NOT exercise
// Lanczos.  Cross-path Lanczos agreement is verified separately on
// Fixture C (n=80) in `shift_invert_and_dense_agree_on_80dof_synthetic_pair`.
// ---------------------------------------------------------------------------

#[test]
fn dense_recovers_closed_form_on_50dof_laplacian() {
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

    // (b) Shift-invert entry point falls back to dense at n=50 — assert the
    // fallback-routing contract (n_converged=0, converged=true), then verify
    // the returned spectrum matches the direct dense call bit-for-bit.
    assert!(
        si_result.converged,
        "shift-invert dense-fallback must converge on the 50-DOF Laplacian pair",
    );
    assert_eq!(
        si_result.n_converged, 0,
        "n=50 must route through dense fallback (n_converged=0); got {}",
        si_result.n_converged,
    );
    assert_eq!(
        si_result.eigenvalues.len(),
        5,
        "shift-invert must return 5 eigenvalues",
    );
    for (i, (&si, &d)) in si_result.eigenvalues.iter().zip(dense_result.eigenvalues.iter()).enumerate() {
        assert!(
            (si - d).abs() < 1e-12,
            "dense-fallback eigenvalue[{i}]: got {si:.15}, dense {d:.15}, diff = {:.3e}",
            (si - d).abs(),
        );
    }
}

// ---------------------------------------------------------------------------
// Step-5b test: PRD §13 phase-2 cross-path agreement signal on 80-DOF Laplacian.
//
// n=80 > 64 = 2·MIN_DIM, so the shift-invert entry point dispatches to
// `partial_self_adjoint_eigen` — the Lanczos numerical path actually runs.
// Compares the recovered spectrum against (a) the closed-form Laplacian
// eigenvalues and (b) the dense path, to 1e-8 (PRD §13 "8 digits").
// ---------------------------------------------------------------------------

/// Closed-form smallest 5 eigenvalues of the 80-DOF Laplacian (Kφ = λBφ = λφ).
/// λ_k = 2(1 − cos(kπ/81)) for k=1..=5.
fn fixture_c_expected_5() -> [f64; 5] {
    let n = 80usize;
    std::array::from_fn(|i| {
        let k = (i + 1) as f64;
        2.0 * (1.0 - f64::cos(k * std::f64::consts::PI / (n as f64 + 1.0)))
    })
}

#[test]
fn shift_invert_and_dense_agree_on_80dof_synthetic_pair() {
    let (k, b) = fixture_c();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };
    let dense_result = solve_eigen_dense(&k, &b, opts.clone());
    let si_result = solve_eigen_shift_invert(&k, &b, opts);

    let expected = fixture_c_expected_5();

    // (a) Shift-invert must take the Lanczos path (n_converged > 0 reflects
    // info.n_converged_eigen from faer).  If n_converged==0, the fallback
    // routing leaked into n>64 territory and the test no longer covers Lanczos.
    assert!(
        si_result.converged,
        "shift-invert (Lanczos) must converge on the 80-DOF Laplacian pair",
    );
    assert!(
        si_result.n_converged > 0,
        "shift-invert at n=80 must exercise Lanczos (n_converged>0); got 0 \
         (suggests routing fell through to dense — Lanczos coverage lost)",
    );
    assert_eq!(
        si_result.eigenvalues.len(),
        5,
        "shift-invert must return 5 eigenvalues",
    );

    // (b) Shift-invert (Lanczos) matches the closed-form Laplacian spectrum to 1e-8.
    for (i, (&got, &exp)) in si_result.eigenvalues.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-8,
            "shift-invert eigenvalue[{i}]: got {got:.15}, expected {exp:.15}, diff = {:.3e}",
            (got - exp).abs(),
        );
    }

    // (c) Dense and shift-invert agree to 1e-8 (PRD §13 phase-2 "8 digits").
    assert_eq!(dense_result.eigenvalues.len(), 5, "dense must return 5 eigenvalues");
    for (i, (&si, &d)) in si_result.eigenvalues.iter().zip(dense_result.eigenvalues.iter()).enumerate() {
        assert!(
            (si - d).abs() < 1e-8,
            "cross-path eigenvalue[{i}]: shift-invert {si:.15}, dense {d:.15}, diff = {:.3e}",
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
        "shift-invert must report converged=false when max_iters=1 and tol=1e-300 \
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
        result.n_converged,
        "n_converged must equal the number of converged eigenvalues returned",
    );
    assert_eq!(
        result.eigenvectors.ncols(),
        result.eigenvalues.len(),
        "eigenvectors width must equal the number of returned eigenvalues",
    );
    // Must not panic — absence of panic IS the no-panic assertion.
}

// ---------------------------------------------------------------------------
// Step-9 tests: contract guard #[should_panic] tests
// ---------------------------------------------------------------------------

/// (a) n_modes=0 is rejected by the entry-point guard.
#[test]
#[should_panic(expected = "n_modes")]
fn solve_eigen_shift_invert_panics_on_zero_n_modes() {
    let (k, b) = fixture_a();
    let opts = EigenSolverOptions {
        n_modes: 0,
        ..EigenSolverOptions::default()
    };
    let _ = solve_eigen_shift_invert(&k, &b, opts);
}

/// (b) A non-square K (5 rows × 4 cols) is rejected.
#[test]
#[should_panic(expected = "K must be square")]
fn solve_eigen_shift_invert_panics_on_non_square_k() {
    // Produce a 5×4 K by hand.
    let k_trips: Vec<Triplet<usize, usize, f64>> = vec![Triplet::new(0, 0, 1.0)];
    let k_rect = SparseRowMat::try_new_from_triplets(5, 4, &k_trips).unwrap();
    // B must also be non-empty; 5×5 identity (will be rejected before shape check on B).
    let b_trips: Vec<Triplet<usize, usize, f64>> =
        (0..5).map(|i| Triplet::new(i, i, 1.0)).collect();
    let b = SparseRowMat::try_new_from_triplets(5, 5, &b_trips).unwrap();
    let _ = solve_eigen_shift_invert(&k_rect, &b, EigenSolverOptions::default());
}

/// (c) B dimensions mismatched with K (K 5×5, B 4×4).
#[test]
#[should_panic(expected = "B must match K dimensions")]
fn solve_eigen_shift_invert_panics_on_shape_mismatch() {
    let (k, _) = fixture_a(); // 5×5
    let b_small_trips: Vec<Triplet<usize, usize, f64>> =
        (0..4).map(|i| Triplet::new(i, i, 1.0)).collect();
    let b_small = SparseRowMat::try_new_from_triplets(4, 4, &b_small_trips).unwrap();
    let _ = solve_eigen_shift_invert(&k, &b_small, EigenSolverOptions::default());
}

/// (d) tol=NaN is rejected (must be a finite positive value).
#[test]
#[should_panic(expected = "tol")]
fn solve_eigen_shift_invert_panics_on_non_finite_tol() {
    let (k, b) = fixture_a();
    let opts = EigenSolverOptions {
        tol: f64::NAN,
        ..EigenSolverOptions::default()
    };
    let _ = solve_eigen_shift_invert(&k, &b, opts);
}

// ---------------------------------------------------------------------------
// Suggestion-4 robustness test: n-floor no-panic boundary sweep
// ---------------------------------------------------------------------------

/// Verify that `solve_eigen_shift_invert` does not panic at boundary sizes
/// near faer's FAER_MIN_DIM=32 floor: n ∈ {2, 16, 32, 33, 63, 64, 65}.
///
/// For n ≤ 64 the `effective_max_dim >= n` branch fires and the call is
/// forwarded to the dense fallback.  For n = 65 the Krylov window (64) is
/// strictly less than n, so `partial_self_adjoint_eigen` actually runs.
/// Both paths must complete without panic.
///
/// Numerical accuracy is not checked here — that is pinned by the closed-form
/// fixtures.  This test guards only against the "panic on small problems"
/// regression documented in eigensolve.rs FAER_MIN_DIM comment.
#[test]
fn shift_invert_no_panic_at_min_dim_boundaries() {
    for n in [2_usize, 16, 32, 33, 63, 64, 65] {
        // K = tridiag(-1, 2, -1) (SPD Dirichlet Laplacian), B = I.
        let mut k_trips = Vec::with_capacity(3 * n);
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

        let opts = EigenSolverOptions {
            n_modes: 2,
            tol: 1e-10,
            max_iters: 1000,
            sigma: 0.0,
        };
        // Absence of panic IS the assertion.
        let _ = solve_eigen_shift_invert(&k, &b, opts);
    }
}
