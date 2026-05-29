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

use faer::{Mat, Side};
use faer::sparse::{SparseRowMat, Triplet};
use reify_solver_elastic::eigensolve::{EigenSolverOptions, solve_eigen_dense, solve_eigen_shift_invert};
use reify_solver_elastic::{lanczos_shift_invert, SparseStiffnessOp, SparseMetricOp};

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

// NOTE: this test requires the root Cargo.toml profile overrides added in
// task 4055 ([profile.dev.package."*"] opt-level=3 and
// [profile.dev.package.reify-solver-elastic] opt-level=2).  Without them,
// the n=80 Lanczos + dense gevd paths run unoptimised in debug and take
// 300–540 s.  If this test hangs in CI, check those overrides first.
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
// Eigenvector residual check (both paths) on Fixture A.
//
// Catches regressions where eigenvalues are correct but eigenvectors are
// mis-paired (column-copy off-by-one, sort/permutation mismatch between
// eigenvalues and eigenvectors, etc.) — the existing closed-form tests
// only check eigenvalues + eigenvector shape, so this is the residual net.
//
// Residual: ‖K φ_i − λ_i B φ_i‖ / ‖K φ_i‖ < 1e-8 for each returned mode.
// ---------------------------------------------------------------------------

/// Compute y = M · x for a SparseRowMat (CSR) by iterating stored entries.
fn csr_matvec(m: &SparseRowMat<usize, f64>, x: &[f64]) -> Vec<f64> {
    let n = m.nrows();
    assert_eq!(x.len(), m.ncols());
    let m_ref = m.as_ref();
    let m_sym = m_ref.symbolic();
    let mut y = vec![0.0_f64; n];
    for (i, y_i) in y.iter_mut().enumerate() {
        let cols = m_sym.col_idx_of_row_raw(i);
        let vals = m_ref.val_of_row(i);
        let mut acc = 0.0_f64;
        for (col_idx, &val) in cols.iter().zip(vals.iter()) {
            acc += val * x[*col_idx];
        }
        *y_i = acc;
    }
    y
}

fn l2_norm(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

/// Assert ‖K φ_i − λ_i B φ_i‖ / ‖K φ_i‖ < tol for every returned mode.
fn assert_eigen_residuals(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    eigenvalues: &[f64],
    eigenvectors: &Mat<f64>,
    tol: f64,
    label: &str,
) {
    let n = k.nrows();
    assert_eq!(eigenvectors.nrows(), n, "{label}: eigenvector row count mismatch");
    assert_eq!(
        eigenvectors.ncols(),
        eigenvalues.len(),
        "{label}: eigenvector column count must match eigenvalue count",
    );
    for (i, &lam) in eigenvalues.iter().enumerate() {
        let phi: Vec<f64> = (0..n).map(|row| eigenvectors[(row, i)]).collect();
        let k_phi = csr_matvec(k, &phi);
        let b_phi = csr_matvec(b, &phi);
        let resid: Vec<f64> = k_phi.iter().zip(b_phi.iter())
            .map(|(k_, b_)| k_ - lam * b_)
            .collect();
        let nr = l2_norm(&resid);
        let nk = l2_norm(&k_phi);
        let rel = nr / nk.max(f64::MIN_POSITIVE);
        assert!(
            rel < tol,
            "{label} mode[{i}] (λ={lam}): ‖K φ − λ B φ‖/‖K φ‖ = {rel:.3e} ≥ tol={tol:.3e}",
        );
    }
}

#[test]
fn dense_eigenvector_residual_matches_eigenvalue_on_5x5_diagonal_pair() {
    let (k, b) = fixture_a();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-12,
        max_iters: 1,
        sigma: 0.0,
    };
    let result = solve_eigen_dense(&k, &b, opts);
    assert_eigen_residuals(&k, &b, &result.eigenvalues, &result.eigenvectors, 1e-8, "dense");
}

#[test]
fn shift_invert_eigenvector_residual_matches_eigenvalue_on_80dof_laplacian() {
    let (k, b) = fixture_c();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };
    let result = solve_eigen_shift_invert(&k, &b, opts);
    // n=80 > 64: Lanczos path actually runs.
    assert!(result.converged, "shift-invert must converge on 80-DOF Laplacian");
    assert!(result.n_converged > 0, "must take Lanczos path (n_converged>0)");
    assert_eigen_residuals(&k, &b, &result.eigenvalues, &result.eigenvectors, 1e-8, "shift-invert");
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

/// Verify that `solve_eigen_shift_invert` does not panic at problem sizes
/// surrounding faer's FAER_MIN_DIM=32 floor.
///
/// Sweeps every n in 2..=128 — well past 2·FAER_MIN_DIM=64 so the test
/// would catch a future faer MIN_DIM raise (e.g. 48 → window shifts to (64,
/// 96]) that a hand-picked size list would miss.  For n ≤ 64 the
/// `effective_max_dim >= n` branch fires and the call is forwarded to the
/// dense fallback; for n ≥ 65 `partial_self_adjoint_eigen` actually runs.
/// Both paths must complete without panic.
///
/// Numerical accuracy is not checked here — that is pinned by the closed-form
/// fixtures.  This test guards only against the "panic on small problems"
/// regression documented in eigensolve.rs FAER_MIN_DIM comment.
///
/// NOTE: fast debug runtime (measured ~0.107 s, task 4055) depends on the root
/// Cargo.toml profile overrides ([profile.dev.package."*"] opt-level=3 and
/// [profile.dev.package.reify-solver-elastic] opt-level=2).  If this test
/// hangs (127 solves × unoptimised faer ≈ 300 s each), check those first.
#[test]
fn shift_invert_no_panic_at_min_dim_boundaries() {
    for n in 2_usize..=128 {
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

// ---------------------------------------------------------------------------
// Generic lanczos_shift_invert: modal-style test with non-identity mass matrix.
//
// Fixture D — 80-DOF Laplacian K + scaled identity mass M = 2.0·I.
// Closed-form: λ_k = (1 − cos(kπ/81)) for k=1..5.
// n=80 > 64 = 2·FAER_MIN_DIM so the Lanczos path actually runs.
// M ≠ I is necessary: a buggy impl that ignores M would recover 2·λ_k instead.
// ---------------------------------------------------------------------------

/// Expected eigenvalues for K = tridiag(-1,2,-1) n=80, M = 2·I.
/// Kφ = λMφ  →  λ_k = λ_k^{Laplacian}/2 = (1 − cos(kπ/81)).
fn fixture_d_expected_5() -> [f64; 5] {
    let n = 80usize;
    std::array::from_fn(|i| {
        let k = (i + 1) as f64;
        1.0 - f64::cos(k * std::f64::consts::PI / (n as f64 + 1.0))
    })
}

#[test]
fn lanczos_shift_invert_recovers_modal_eigenpairs_on_uniform_mass_laplacian() {
    let n = 80usize;

    // K = tridiag(-1, 2, -1) — 80×80 Dirichlet Laplacian (SPD).
    let mut k_trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        k_trips.push(Triplet::new(i, i, 2.0));
        if i > 0 { k_trips.push(Triplet::new(i, i - 1, -1.0)); }
        if i + 1 < n { k_trips.push(Triplet::new(i, i + 1, -1.0)); }
    }
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();

    // M = 2.0·I — uniform mass scaling (non-identity to exercise the M slot).
    let m_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 2.0)).collect();
    let m = SparseRowMat::try_new_from_triplets(n, n, &m_trips).unwrap();

    // Factor K.
    let llt = k.sp_cholesky(Side::Lower).expect("K must be SPD");

    // Build generic operator pair.
    let k_op = SparseStiffnessOp { llt: &llt, n };
    let m_op = SparseMetricOp { m: m.as_ref() };

    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };

    // Call the generic Lanczos entry point.
    let result = lanczos_shift_invert(&k_op, &m_op, opts);

    // (a) Must converge.
    assert!(
        result.converged,
        "lanczos_shift_invert must converge on 80-DOF Laplacian + 2·I mass"
    );
    // (b) Must exercise the Lanczos path (n_converged > 0).
    assert!(
        result.n_converged >= 5,
        "lanczos_shift_invert at n=80 must run Lanczos (n_converged >= 5); got {}",
        result.n_converged
    );
    // (c) Must return exactly n_modes=5 eigenvalues.
    assert_eq!(
        result.eigenvalues.len(),
        5,
        "must return 5 eigenvalues"
    );
    // (d) Eigenvector shape n×n_modes.
    assert_eq!(result.eigenvectors.nrows(), n, "eigenvectors must have n rows");
    assert_eq!(result.eigenvectors.ncols(), 5, "eigenvectors must have n_modes cols");

    // (e) Eigenvalues must match closed-form λ_k = (1 − cos(kπ/81)) to 1e-8.
    let expected = fixture_d_expected_5();
    for (i, (&got, &exp)) in result.eigenvalues.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-8,
            "eigenvalue[{}]: got {:.15}, expected {:.15}, diff = {:.3e}",
            i, got, exp, (got - exp).abs(),
        );
    }
}

// ---------------------------------------------------------------------------
// Step-3 tests: contract-guard #[should_panic] tests for lanczos_shift_invert.
//
// These tests pin the generic entry-point's own panic contract (distinct from
// the wrapper-level guards on `solve_eigen_shift_invert` above).
// ---------------------------------------------------------------------------

/// (a) k_op.n() ≠ m_op.n() is rejected at the generic entry point with a
/// message containing "dimension".
#[test]
#[should_panic(expected = "dimension")]
fn lanczos_shift_invert_panics_on_dimension_mismatch() {
    let n_k = 80usize;
    let n_m = 79usize; // intentional mismatch

    // K = tridiag(-1,2,-1) 80×80, factored.
    let mut k_trips = Vec::with_capacity(3 * n_k - 2);
    for i in 0..n_k {
        k_trips.push(Triplet::new(i, i, 2.0));
        if i > 0 { k_trips.push(Triplet::new(i, i - 1, -1.0)); }
        if i + 1 < n_k { k_trips.push(Triplet::new(i, i + 1, -1.0)); }
    }
    let k = SparseRowMat::try_new_from_triplets(n_k, n_k, &k_trips).unwrap();
    let llt = k.sp_cholesky(Side::Lower).expect("K must be SPD");
    let k_op = SparseStiffnessOp { llt: &llt, n: n_k };

    // M = I (79×79) — dimension deliberately mismatched with k_op.n()=80.
    let m_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n_m).map(|i| Triplet::new(i, i, 1.0)).collect();
    let m = SparseRowMat::try_new_from_triplets(n_m, n_m, &m_trips).unwrap();
    let m_op = SparseMetricOp { m: m.as_ref() };

    // Must panic: "lanczos_shift_invert: dimension mismatch — ..."
    // Use explicit valid opts so that if Default ever changes to an invalid
    // value, the dimension assert (which runs last) still fires rather than
    // an earlier guard giving a confusing failure message.
    let _ = lanczos_shift_invert(
        &k_op,
        &m_op,
        EigenSolverOptions { n_modes: 5, tol: 1e-10, max_iters: 1000, sigma: 0.0 },
    );
}

/// (b) n_modes=0 is rejected at the generic entry point with a message
/// containing "n_modes".
#[test]
#[should_panic(expected = "n_modes")]
fn lanczos_shift_invert_panics_on_zero_n_modes() {
    let n = 80usize;

    // Valid 80-DOF pair (same as Fixture D construction).
    let mut k_trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        k_trips.push(Triplet::new(i, i, 2.0));
        if i > 0 { k_trips.push(Triplet::new(i, i - 1, -1.0)); }
        if i + 1 < n { k_trips.push(Triplet::new(i, i + 1, -1.0)); }
    }
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();
    let llt = k.sp_cholesky(Side::Lower).expect("K must be SPD");
    let k_op = SparseStiffnessOp { llt: &llt, n };

    let m_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    let m = SparseRowMat::try_new_from_triplets(n, n, &m_trips).unwrap();
    let m_op = SparseMetricOp { m: m.as_ref() };

    // Must panic: "EigenSolverOptions.n_modes = 0 is invalid; must be >= 1"
    let opts = EigenSolverOptions { n_modes: 0, ..EigenSolverOptions::default() };
    let _ = lanczos_shift_invert(&k_op, &m_op, opts);
}

/// (c) tol=NaN is rejected at the generic entry point with a message
/// containing "tol".
#[test]
#[should_panic(expected = "tol")]
fn lanczos_shift_invert_panics_on_non_finite_tol() {
    let n = 80usize;

    let mut k_trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        k_trips.push(Triplet::new(i, i, 2.0));
        if i > 0 { k_trips.push(Triplet::new(i, i - 1, -1.0)); }
        if i + 1 < n { k_trips.push(Triplet::new(i, i + 1, -1.0)); }
    }
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();
    let llt = k.sp_cholesky(Side::Lower).expect("K must be SPD");
    let k_op = SparseStiffnessOp { llt: &llt, n };

    let m_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    let m = SparseRowMat::try_new_from_triplets(n, n, &m_trips).unwrap();
    let m_op = SparseMetricOp { m: m.as_ref() };

    // Must panic: "EigenSolverOptions.tol = NaN must be a finite positive value"
    let opts = EigenSolverOptions { n_modes: 5, tol: f64::NAN, max_iters: 1000, sigma: 0.0 };
    let _ = lanczos_shift_invert(&k_op, &m_op, opts);
}

/// (d) max_iters=0 is rejected at the generic entry point with a message
/// containing "max_iters".
#[test]
#[should_panic(expected = "max_iters")]
fn lanczos_shift_invert_panics_on_zero_max_iters() {
    let n = 80usize;

    let mut k_trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        k_trips.push(Triplet::new(i, i, 2.0));
        if i > 0 { k_trips.push(Triplet::new(i, i - 1, -1.0)); }
        if i + 1 < n { k_trips.push(Triplet::new(i, i + 1, -1.0)); }
    }
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();
    let llt = k.sp_cholesky(Side::Lower).expect("K must be SPD");
    let k_op = SparseStiffnessOp { llt: &llt, n };

    let m_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    let m = SparseRowMat::try_new_from_triplets(n, n, &m_trips).unwrap();
    let m_op = SparseMetricOp { m: m.as_ref() };

    // Must panic: "EigenSolverOptions.max_iters = 0 is invalid; must be >= 1"
    let opts = EigenSolverOptions { n_modes: 5, tol: 1e-10, max_iters: 0, sigma: 0.0 };
    let _ = lanczos_shift_invert(&k_op, &m_op, opts);
}
