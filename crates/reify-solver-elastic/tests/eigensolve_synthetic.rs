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
use reify_solver_elastic::eigensolve::test_support::{
    graded_diagonal_b_pencil, indefinite_b_pencil, laplacian_lambdas, laplacian_pencil,
};
use reify_solver_elastic::eigensolve::{
    EigenSolverOptions, EigenSolverResult, solve_eigen_dense, solve_eigen_shift_invert,
};
use reify_solver_elastic::{
    LanczosMetric, SparseFactorRef, SparseMetricOp, SparseStiffnessOp, SplitCholesky,
    lanczos_shift_invert, lanczos_shift_invert_in_metric,
};

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

/// Fixture B: K = tridiag(-1, 2, -1) (50×50 Dirichlet Laplacian), B = I.
const FIXTURE_B_N: usize = 50;

/// Fixture B, from the crate's shared test-support seam — one definition of the
/// pencil and its closed form across every site that drives them.
fn fixture_b() -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    laplacian_pencil(FIXTURE_B_N)
}

/// Closed-form smallest 5 eigenvalues of fixture B (Kφ = λBφ = λφ).
fn fixture_b_expected_5() -> [f64; 5] {
    laplacian_lambdas(FIXTURE_B_N)
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

/// Closed-form smallest 5 eigenvalues of fixture C (Kφ = λBφ = λφ).
fn fixture_c_expected_5() -> [f64; 5] {
    laplacian_lambdas(FIXTURE_C_N)
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

/// Fixture C: K = tridiag(-1,2,-1) (80×80), B = I, from the shared seam.
/// n=80 > 64 so faer's effective_max_dim = min(max(32,64,10),80) = 64 < 80:
/// partial_self_adjoint_eigen runs the Lanczos loop without the dense fallback.
const FIXTURE_C_N: usize = 80;

fn fixture_c() -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    laplacian_pencil(FIXTURE_C_N)
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
    assert_eq!(
        result.residual_check_failures, 0,
        "non-convergence is a shortfall, not a residual-check failure",
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
    let k_op = SparseStiffnessOp { factor: SparseFactorRef::Cholesky(&llt), n };
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
    let k_op = SparseStiffnessOp {
        factor: SparseFactorRef::Cholesky(&llt),
        n: n_k,
    };

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
    let k_op = SparseStiffnessOp { factor: SparseFactorRef::Cholesky(&llt), n };

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
    let k_op = SparseStiffnessOp { factor: SparseFactorRef::Cholesky(&llt), n };

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
    let k_op = SparseStiffnessOp { factor: SparseFactorRef::Cholesky(&llt), n };

    let m_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    let m = SparseRowMat::try_new_from_triplets(n, n, &m_trips).unwrap();
    let m_op = SparseMetricOp { m: m.as_ref() };

    // Must panic: "EigenSolverOptions.max_iters = 0 is invalid; must be >= 1"
    let opts = EigenSolverOptions { n_modes: 5, tol: 1e-10, max_iters: 0, sigma: 0.0 };
    let _ = lanczos_shift_invert(&k_op, &m_op, opts);
}

// ---------------------------------------------------------------------------
// Dense QZ on an indefinite-B pencil (#7602 concern 4)
// ---------------------------------------------------------------------------

/// The dense QZ path completes on the 136-DOF indefinite-B pencil.
///
/// A regression tripwire for a BUILD-PROFILE fault, not an algorithmic one:
/// faer's generic QZ (`gevd_real`) is monomorphised in this crate, and its
/// aggressive-early-deflation step relies on `usize` wrapping arithmetic that
/// is correct in release but panics "attempt to subtract with overflow" under
/// overflow-checks. This pencil reaches that step; the root `Cargo.toml` dev
/// profile for `reify-solver-elastic` is what keeps it green.
///
/// The same pencil is the dense reference for the indefinite-B Lanczos tests,
/// so a dense path that cannot solve it leaves those tests without a baseline.
#[test]
fn dense_solve_completes_on_the_indefinite_136dof_pencil() {
    let (k, b) = indefinite_b_pencil(136);
    let opts = EigenSolverOptions {
        n_modes: 3,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };
    let result = solve_eigen_dense(&k, &b, opts);

    assert!(result.converged, "dense QZ must return all 3 requested modes");
    assert_eq!(result.eigenvalues.len(), 3, "must return exactly 3 eigenvalues");
    assert_eigen_residuals(
        &k,
        &b,
        &result.eigenvalues,
        &result.eigenvectors,
        1e-10,
        "dense 136 indefinite",
    );
}

// ---------------------------------------------------------------------------
// Cholesky-symmetrized Lanczos core, driven directly (#7602)
//
// For a B that is not a multiple of the identity, `(K − σB)⁻¹B` is not
// Euclidean-symmetric, so the core runs on S = G⁻¹[B + (σ − τ)B(K − σB)⁻¹B]G⁻ᵀ
// for an SPD W = K − τB = G·Gᵀ instead. These drive both metric arms against
// the dense QZ reference at the same σ.
// ---------------------------------------------------------------------------

/// `K − σB` for two TRIDIAGONAL operands, as a sparse row matrix over the
/// tridiagonal pattern.
fn shifted_tridiagonal_pencil(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    sigma: f64,
) -> SparseRowMat<usize, f64> {
    let n = k.nrows();
    let (k_dense, b_dense) = (k.to_dense(), b.to_dense());
    let mut trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        for j in i.saturating_sub(1)..(i + 2).min(n) {
            trips.push(Triplet::new(i, j, k_dense[(i, j)] - sigma * b_dense[(i, j)]));
        }
    }
    SparseRowMat::try_new_from_triplets(n, n, &trips).unwrap()
}

fn metric_opts(n_modes: usize, sigma: f64) -> EigenSolverOptions {
    EigenSolverOptions {
        n_modes,
        tol: 1e-10,
        max_iters: 1000,
        sigma,
    }
}

/// The eigenvalues of `got` and `want` agree as MULTISETS: each sorted pair
/// `(g, w)` is within `tolerance(g, w)`.
fn assert_same_eigenvalue_multiset(
    got: &[f64],
    want: &[f64],
    tolerance: impl Fn(f64, f64) -> f64,
    label: &str,
) {
    assert_eq!(got.len(), want.len(), "{label}: eigenvalue count {got:?} vs {want:?}");
    let sorted = |v: &[f64]| {
        let mut v = v.to_vec();
        v.sort_by(f64::total_cmp);
        v
    };
    for (g, w) in sorted(got).into_iter().zip(sorted(want)) {
        let bound = tolerance(g, w);
        assert!(
            (g - w).abs() <= bound,
            "{label}: λ = {g} vs dense {w}, gap {:.3e} > bound {bound:.3e}; \
             got {got:?}, dense {want:?}",
            (g - w).abs(),
        );
    }
}

/// Relative agreement to `rel_tol`.
fn relative(rel_tol: f64) -> impl Fn(f64, f64) -> f64 {
    move |g, w| rel_tol * g.abs().max(w.abs())
}

/// Shift-invert accuracy scales with `|λ − σ|` (λ = σ + 1/μ), and a pure
/// relative bound is ill-posed where a pencil has λ = 0 exactly, so σ-ladder
/// comparisons use `1e-9·max(|a|, |b|, |a − σ|, 1e-12)`.
fn mixed_near_shift(sigma: f64) -> impl Fn(f64, f64) -> f64 {
    move |g, w| 1e-9 * g.abs().max(w.abs()).max((g - sigma).abs()).max(1e-12)
}

/// A direct solve through the metric core: the pencil, the options, and what
/// came back.
struct MetricCoreSolve {
    label: &'static str,
    k: SparseRowMat<usize, f64>,
    b: SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
    result: EigenSolverResult,
}

/// Drive the metric core's `ShiftedPencil` arm with `w = K − σB` (σ = 0 ⇒ K).
fn shifted_pencil_metric_solve(
    label: &'static str,
    (k, b): (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>),
    w: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> MetricCoreSolve {
    let g = SplitCholesky::try_new(w).expect("the metric must be SPD");
    let result = lanczos_shift_invert_in_metric(
        LanczosMetric::ShiftedPencil(&g),
        &SparseMetricOp { m: b.as_ref() },
        opts.clone(),
    );
    MetricCoreSolve { label, k, b, opts, result }
}

/// Drive the metric core's `Stiffness` arm: `W = K`, and the indefinite
/// `K − σB` applied through LU.
fn stiffness_metric_solve(
    label: &'static str,
    (k, b): (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>),
    opts: EigenSolverOptions,
) -> MetricCoreSolve {
    let lu = shifted_tridiagonal_pencil(&k, &b, opts.sigma).sp_lu().unwrap();
    let g_k = SplitCholesky::try_new(&k).expect("K is SPD");
    let shifted_inverse = SparseStiffnessOp {
        factor: SparseFactorRef::Lu(&lu),
        n: k.nrows(),
    };
    let result = lanczos_shift_invert_in_metric(
        LanczosMetric::Stiffness {
            k_factor: &g_k,
            shifted_inverse: &shifted_inverse,
        },
        &SparseMetricOp { m: b.as_ref() },
        opts.clone(),
    );
    MetricCoreSolve { label, k, b, opts, result }
}

/// σ = 0: `W = K` itself, `S = G⁻¹BG⁻ᵀ`.
fn graded_metric_at_sigma_zero() -> MetricCoreSolve {
    let pencil = graded_diagonal_b_pencil(80);
    let k = pencil.0.clone();
    shifted_pencil_metric_solve("graded σ=0 ShiftedPencil", pencil, &k, metric_opts(2, 0.0))
}

/// σ = 5e-4, below λ₁ ≈ 1.002e-3: `K − σB` is SPD and is the metric.
fn graded_metric_below_lambda_one() -> MetricCoreSolve {
    let pencil = graded_diagonal_b_pencil(80);
    let opts = metric_opts(2, 5e-4);
    let w = shifted_tridiagonal_pencil(&pencil.0, &pencil.1, opts.sigma);
    shifted_pencil_metric_solve("graded σ=5e-4 ShiftedPencil", pencil, &w, opts)
}

/// σ = 0.5, above many modes: `K − σB` is indefinite, so `W = K`.
fn graded_metric_above_a_mode() -> MetricCoreSolve {
    stiffness_metric_solve(
        "graded σ=0.5 Stiffness",
        graded_diagonal_b_pencil(80),
        metric_opts(2, 0.5),
    )
}

/// The indefinite-B pencil at σ = 0.02, above its smallest positive mode
/// (≈ 6.46e-4), through the `Stiffness` arm.
fn indefinite_metric_above_a_mode() -> MetricCoreSolve {
    stiffness_metric_solve(
        "indefinite σ=0.02 Stiffness",
        indefinite_b_pencil(136),
        metric_opts(3, 0.02),
    )
}

/// Every contract a metric-Lanczos result owes, measured against dense QZ.
fn assert_metric_lanczos_matches_dense(solve: &MetricCoreSolve) {
    let MetricCoreSolve { label, k, b, opts, result } = solve;
    let n_modes = opts.n_modes;
    assert!(
        result.n_converged >= n_modes,
        "{label}: n_converged = {} < {n_modes} — the Lanczos did not genuinely run",
        result.n_converged,
    );
    assert!(result.converged, "{label}: must converge");
    assert_eq!(result.shift, opts.sigma, "{label}: the shift used must be σ");

    let dense = solve_eigen_dense(k, b, opts.clone());
    assert_same_eigenvalue_multiset(&result.eigenvalues, &dense.eigenvalues, relative(1e-9), label);
    assert_eigen_residuals(k, b, &result.eigenvalues, &result.eigenvectors, 1e-10, label);

    for col in 0..result.eigenvectors.ncols() {
        let norm = result.eigenvectors.col(col).norm_l2();
        assert!(
            (norm - 1.0).abs() <= 1e-12,
            "{label}: eigenvector column {col} has Euclidean norm {norm}, not 1",
        );
    }
}

#[test]
fn metric_lanczos_shifted_pencil_at_sigma_zero_matches_dense_on_graded_b() {
    assert_metric_lanczos_matches_dense(&graded_metric_at_sigma_zero());
}

#[test]
fn metric_lanczos_shifted_pencil_below_lambda_one_matches_dense_on_graded_b() {
    assert_metric_lanczos_matches_dense(&graded_metric_below_lambda_one());
}

#[test]
fn metric_lanczos_stiffness_arm_above_a_mode_matches_dense_on_graded_b() {
    assert_metric_lanczos_matches_dense(&graded_metric_above_a_mode());
}

#[test]
fn metric_lanczos_stiffness_arm_matches_dense_on_indefinite_b() {
    assert_metric_lanczos_matches_dense(&indefinite_metric_above_a_mode());
}

// ---------------------------------------------------------------------------
// The sparse entry point dispatches B ≠ cI to the symmetrized operator (#7602)
// ---------------------------------------------------------------------------

/// SPD, non-identity B: σ from 0 to well inside the spectrum (n_modes = 2).
const GRADED_SIGMA_LADDER: [f64; 6] = [0.0, 5e-4, 0.05, 0.5, 1.5, 2.5];
/// Indefinite B (the buckling stand-in): shifts of both signs (n_modes = 3).
const INDEFINITE_SIGMA_LADDER: [f64; 6] = [0.0, 1e-3, 0.02, -0.02, 0.2, -0.3];

/// `solve_eigen_shift_invert` at each σ of `sigmas`, with the options used.
fn ladder_solves(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    n_modes: usize,
    sigmas: &[f64],
) -> Vec<(EigenSolverOptions, EigenSolverResult)> {
    sigmas
        .iter()
        .map(|&sigma| {
            let opts = metric_opts(n_modes, sigma);
            let result = solve_eigen_shift_invert(k, b, opts.clone());
            (opts, result)
        })
        .collect()
}

/// Hold every solve of a σ ladder to the dense reference at the same σ.
fn assert_shift_invert_matches_dense_across(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    n_modes: usize,
    sigmas: &[f64],
    label: &str,
) {
    for (opts, lanczos) in ladder_solves(k, b, n_modes, sigmas) {
        let sigma = opts.sigma;
        let ctx = format!("{label} σ={sigma}");
        assert!(
            lanczos.n_converged > 0,
            "{ctx}: must exercise Lanczos (n_converged > 0)",
        );
        assert!(lanczos.converged, "{ctx}: must converge");
        let dense = solve_eigen_dense(k, b, opts);
        assert_same_eigenvalue_multiset(
            &lanczos.eigenvalues,
            &dense.eigenvalues,
            mixed_near_shift(sigma),
            &ctx,
        );
        assert_eigen_residuals(k, b, &lanczos.eigenvalues, &lanczos.eigenvectors, 1e-10, &ctx);
    }
}

#[test]
fn shift_invert_matches_dense_on_graded_spd_b_across_a_sigma_ladder() {
    let (k, b) = graded_diagonal_b_pencil(80);
    assert_shift_invert_matches_dense_across(&k, &b, 2, &GRADED_SIGMA_LADDER, "graded");
}

#[test]
fn shift_invert_matches_dense_on_indefinite_b_across_a_sigma_ladder() {
    let (k, b) = indefinite_b_pencil(136);
    assert_shift_invert_matches_dense_across(&k, &b, 3, &INDEFINITE_SIGMA_LADDER, "indefinite");
}

// ---------------------------------------------------------------------------
// Post-solve residual verification (#7602): a non-eigenpair never reports
// `converged = true`
// ---------------------------------------------------------------------------

/// The Euclidean core driven with a Cholesky of the row-major K.
fn euclidean_core_solve(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    opts: EigenSolverOptions,
) -> EigenSolverResult {
    let llt = k.sp_cholesky(Side::Lower).expect("K is SPD");
    lanczos_shift_invert(
        &SparseStiffnessOp {
            factor: SparseFactorRef::Cholesky(&llt),
            n: k.nrows(),
        },
        &SparseMetricOp { m: b.as_ref() },
        opts,
    )
}

/// The DEFECT CLASS itself: the Euclidean core handed a B ≠ cI returns Ritz
/// pairs that are not eigenpairs (measured ‖Sy − μy‖ = 9.1e1, relative 0.36),
/// and must say so rather than label them converged. The pairs are still
/// RETURNED, so C2 selection is not silently changed.
#[test]
fn euclidean_core_misused_on_a_non_identity_b_reports_unverified_pairs() {
    let (k, b) = graded_diagonal_b_pencil(80);
    let misused = euclidean_core_solve(&k, &b, metric_opts(2, 0.0));
    assert_eq!(misused.eigenvalues.len(), 2, "the pairs must still be returned");
    assert_eq!(
        misused.residual_check_failures, 2,
        "both returned pairs fail the post-solve residual check",
    );
    assert!(!misused.converged, "unverified pairs must not report converged");

    let (k_c, b_c) = fixture_c();
    let control = euclidean_core_solve(&k_c, &b_c, metric_opts(2, 0.0));
    assert!(control.converged, "B = I: the Euclidean core is valid and converges");
    assert_eq!(control.residual_check_failures, 0, "B = I: every pair verifies");
}

/// Every Lanczos path this file drives verifies its pairs: both σ ladders
/// through the sparse entry point, and every direct metric-core solve.
#[test]
fn every_lanczos_path_verifies_its_pairs() {
    let graded = graded_diagonal_b_pencil(80);
    let indefinite = indefinite_b_pencil(136);
    let ladders = ladder_solves(&graded.0, &graded.1, 2, &GRADED_SIGMA_LADDER)
        .into_iter()
        .map(|solve| ("graded", solve))
        .chain(
            ladder_solves(&indefinite.0, &indefinite.1, 3, &INDEFINITE_SIGMA_LADDER)
                .into_iter()
                .map(|solve| ("indefinite", solve)),
        );
    for (label, (opts, result)) in ladders {
        assert!(result.n_converged > 0, "{label} σ={}: must run Lanczos", opts.sigma);
        assert_eq!(
            result.residual_check_failures, 0,
            "{label} σ={}: every returned pair must verify",
            opts.sigma,
        );
    }
    for solve in [
        graded_metric_at_sigma_zero(),
        graded_metric_below_lambda_one(),
        graded_metric_above_a_mode(),
        indefinite_metric_above_a_mode(),
    ] {
        assert_eq!(
            solve.result.residual_check_failures, 0,
            "{}: every returned pair must verify",
            solve.label,
        );
    }
}

/// QZ computes the spectrum directly, so the dense path has no Ritz pairs to
/// verify.
#[test]
fn dense_path_reports_no_residual_check_failures() {
    let (k, b) = graded_diagonal_b_pencil(80);
    let result = solve_eigen_dense(&k, &b, metric_opts(2, 0.0));
    assert_eq!(result.residual_check_failures, 0);
}
