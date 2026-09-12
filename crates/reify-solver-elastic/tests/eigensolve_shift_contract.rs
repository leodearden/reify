//! Boundary tests for the PRD §6 shift contract (C1–C6).
//!
//! PRD reference: `docs/prds/v0_6/shift-invert-eigensolve.md` §6 (the contract
//! and its two-way boundary tests BT1–BT5).
//!
//! # This is a harness, not a test of one implementation
//!
//! The contract is defined once and implemented **twice** — on the dense QZ path
//! (`solve_eigen_dense`) and on the shift-invert Lanczos path
//! (`solve_eigen_shift_invert`). Both implementations are held to the assertions
//! in this file. Leaf α (#7258) lands the dense implementation and the harness;
//! leaf β (#7259) makes the Lanczos path honor σ and instantiates the σ≠0 arms
//! against **these same functions** rather than inventing its own acceptance.
//!
//! The reusable, solver-agnostic pieces are [`assert_order_ascending_by_abs_lambda`]
//! (C3) and [`assert_implementations_agree`] (the BT2 body: C2 set agreement, C3
//! order, C5 provenance). Adding β's σ≠0 coverage is a one-line instantiation of
//! the latter, not a new harness.
//!
//! # Which solver path each fixture exercises (PRD §5.5 trap)
//!
//! A fixture small enough to be quick may exercise *only* the dense path and
//! never touch Lanczos at all, so every fixture here names its path — and the
//! Lanczos claim is **enforced** (via `n_converged > 0`), not merely asserted in
//! prose:
//!
//! - **Fixture A** — 5×5 diagonal pair (K = I, B = diag(1,2,3,4,5)), closed-form
//!   λ_i = 1/b_i = [0.2, 0.25, 1/3, 0.5, 1.0]. n=5 ≤ 64, so this fixture reaches
//!   the **dense path only**; it is called through `solve_eigen_dense` directly.
//!   Its exact rational spectrum is what makes the σ arithmetic checkable in
//!   closed form.
//! - **Fixture C** — 80-DOF 1D Laplacian (K = tridiag(-1,2,-1), B = I),
//!   closed-form λ_k = 2(1 − cos(kπ/81)). n=80 > 2·FAER_MIN_DIM = 64, so
//!   `solve_eigen_shift_invert` genuinely runs **Lanczos** here rather than
//!   falling back to dense — which is what makes it the cross-implementation
//!   fixture.
//!
//! Both fixtures and the 1e-12 / 1e-8 tolerances are ported from the landed
//! `crates/reify-solver-elastic/tests/eigensolve_synthetic.rs`, where they are
//! already measured against this same `gevd_real` / `partial_self_adjoint_eigen`
//! pair (`dense_recovers_known_spectrum_on_5x5_diagonal_pair` and
//! `shift_invert_and_dense_agree_on_80dof_synthetic_pair`). The tolerances here
//! are therefore a measured precedent, not a guess.

use faer::sparse::{SparseRowMat, Triplet};
use reify_solver_elastic::eigensolve::{
    EigenSolverOptions, EigenSolverResult, solve_eigen_dense, solve_eigen_shift_invert,
};

// ---------------------------------------------------------------------------
// Fixtures (ported from eigensolve_synthetic.rs — see module docs)
// ---------------------------------------------------------------------------

/// Fixture A: K = I (5×5 identity), B = diag(1,2,3,4,5). Dense path only.
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

/// Closed-form Fixture-A spectrum in ascending |λ|.
/// `Kφ = λBφ` → `Iφ = λ·diag(b_i)φ` → `λ_i = 1/b_i`.
fn fixture_a_expected() -> [f64; 5] {
    [0.2, 0.25, 1.0 / 3.0, 0.5, 1.0]
}

/// Fixture C: K = tridiag(-1,2,-1) (80×80), B = I. n=80 > 64 so Lanczos runs.
fn fixture_c() -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    let n = 80usize;
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

/// Closed-form smallest 5 eigenvalues of the 80-DOF Laplacian:
/// `λ_k = 2(1 − cos(kπ/81))` for k=1..=5.
fn fixture_c_expected_5() -> [f64; 5] {
    let n = 80usize;
    std::array::from_fn(|i| {
        let k = (i + 1) as f64;
        2.0 * (1.0 - f64::cos(k * std::f64::consts::PI / (n as f64 + 1.0)))
    })
}

// ---------------------------------------------------------------------------
// Solver-agnostic contract harness (β #7259 instantiates these at σ≠0)
// ---------------------------------------------------------------------------

/// **C3 — Order.** `eigenvalues` must be ascending by |λ|.
///
/// Absolute, not signed, which is today's convention and is preserved unchanged:
/// with negative eigenvalues present λ=−2 still sorts before λ=+3. This is a
/// *presentation* rule and is asserted separately from the C2 *selection* rule
/// everywhere in this file — the two must never be conflated.
fn assert_order_ascending_by_abs_lambda(eigenvalues: &[f64], ctx: &str) {
    for (i, w) in eigenvalues.windows(2).enumerate() {
        assert!(
            w[0].abs() <= w[1].abs(),
            "{ctx}: C3 violated — |λ[{i}]| = {:.15} > |λ[{}]| = {:.15} (eigenvalues must \
             be ascending by |λ|); full set = {eigenvalues:?}",
            w[0].abs(),
            i + 1,
            w[1].abs(),
        );
    }
}

/// **BT2 — cross-implementation agreement.** The reusable body: two
/// implementations solving the same pencil at the same σ must return the same
/// eigenvalue *set* to `tol`, the same C3 order, and the same C5 provenance
/// boolean.
///
/// Set agreement is checked as a **bidirectional nearest-match**, deliberately
/// order-insensitive, so it tests C2 (which eigenvalues came back) without
/// silently re-testing C3 (what order they came back in). C3 is then asserted
/// separately on each result. One direction alone would accept a multiset
/// mismatch such as `{1, 1}` vs `{1, 2}`; both directions reject it.
fn assert_implementations_agree(
    res_a: &EigenSolverResult,
    res_b: &EigenSolverResult,
    tol: f64,
    ctx: &str,
) {
    assert_eq!(
        res_a.eigenvalues.len(),
        res_b.eigenvalues.len(),
        "{ctx}: implementations returned different mode counts — a: {:?}, b: {:?}",
        res_a.eigenvalues,
        res_b.eigenvalues,
    );

    // C2 — same set, order-insensitive, both directions.
    let nearest = |lam: f64, other: &[f64]| -> f64 {
        other
            .iter()
            .map(|&m| (lam - m).abs())
            .fold(f64::INFINITY, f64::min)
    };
    for &lam in &res_a.eigenvalues {
        let d = nearest(lam, &res_b.eigenvalues);
        assert!(
            d < tol,
            "{ctx}: C2 violated — λ = {lam:.15} from implementation a has no counterpart \
             within {tol:.3e} in b (nearest is {d:.3e} away); a = {:?}, b = {:?}",
            res_a.eigenvalues,
            res_b.eigenvalues,
        );
    }
    for &lam in &res_b.eigenvalues {
        let d = nearest(lam, &res_a.eigenvalues);
        assert!(
            d < tol,
            "{ctx}: C2 violated — λ = {lam:.15} from implementation b has no counterpart \
             within {tol:.3e} in a (nearest is {d:.3e} away); a = {:?}, b = {:?}",
            res_a.eigenvalues,
            res_b.eigenvalues,
        );
    }

    // C3 — each side independently ordered ascending by |λ|.
    assert_order_ascending_by_abs_lambda(&res_a.eigenvalues, &format!("{ctx} (implementation a)"));
    assert_order_ascending_by_abs_lambda(&res_b.eigenvalues, &format!("{ctx} (implementation b)"));

    // C5 — same provenance, and the same σ was actually used by both.
    assert_eq!(
        res_a.shift, res_b.shift,
        "{ctx}: implementations disagree on the σ used — a: {}, b: {}",
        res_a.shift, res_b.shift,
    );
    assert_eq!(
        res_a.shift_skipped_modes, res_b.shift_skipped_modes,
        "{ctx}: C5 violated — implementations disagree on skipped-mode provenance \
         (a: {}, b: {}) at σ = {}",
        res_a.shift_skipped_modes, res_b.shift_skipped_modes, res_a.shift,
    );
}

/// Assert a spectrum matches a closed-form expectation element-wise.
fn assert_matches_closed_form(got: &[f64], expected: &[f64], tol: f64, ctx: &str) {
    assert_eq!(
        got.len(),
        expected.len(),
        "{ctx}: expected {} eigenvalues, got {} ({got:?})",
        expected.len(),
        got.len(),
    );
    for (i, (&g, &e)) in got.iter().zip(expected.iter()).enumerate() {
        assert!(
            (g - e).abs() < tol,
            "{ctx}: eigenvalue[{i}] = {g:.15}, expected {e:.15}, diff = {:.3e} ≥ tol = {tol:.3e}",
            (g - e).abs(),
        );
    }
}

// ---------------------------------------------------------------------------
// BT1 — σ=0 is the identity, on both implementations (C1)
// ---------------------------------------------------------------------------

/// **BT1.** At σ=0 both implementations reproduce the pre-PRD spectrum and order.
///
/// C1 is *structural*, not a tolerance: `λ − 0.0 == λ` bit-exactly in IEEE-754
/// for every finite λ, so the σ-aware selection at σ=0 is literally today's sort.
/// The numerical tolerances below are therefore pinning the fixtures' closed-form
/// spectra (which is what makes a regression legible), not C1 itself.
///
/// Both implementations must also report `shift == 0.0` and
/// `shift_skipped_modes == false` — and at σ=0 that `false` is **established**,
/// not assumed: the open interval strictly between 0 and 0 is empty, so no
/// eigenvalue can lie in it.
#[test]
fn sigma_zero_is_the_identity_on_both_implementations() {
    let opts_5 = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-12,
        max_iters: 1000,
        sigma: 0.0,
    };

    // Fixture A — dense path only (n=5 ≤ 64).
    let (ka, ba) = fixture_a();
    let dense_a = solve_eigen_dense(&ka, &ba, opts_5.clone());
    assert_matches_closed_form(
        &dense_a.eigenvalues,
        &fixture_a_expected(),
        1e-12,
        "BT1 fixture A dense at σ=0",
    );
    assert_order_ascending_by_abs_lambda(&dense_a.eigenvalues, "BT1 fixture A dense at σ=0");
    assert_eq!(dense_a.shift, 0.0, "BT1 fixture A dense: shift must be 0.0");
    assert!(
        !dense_a.shift_skipped_modes,
        "BT1 fixture A dense: at σ=0 nothing can be skipped — the interval strictly \
         between 0 and 0 is empty",
    );

    // Fixture C — both paths (n=80 > 64, so Lanczos genuinely runs).
    let (kc, bc) = fixture_c();
    let opts_c = EigenSolverOptions {
        tol: 1e-10,
        ..opts_5
    };
    let dense_c = solve_eigen_dense(&kc, &bc, opts_c.clone());
    let lanczos_c = solve_eigen_shift_invert(&kc, &bc, opts_c);

    // Enforce the path claim rather than asserting it in prose (PRD §5.5 trap):
    // n_converged reflects faer's info.n_converged_eigen, which is 0 on the dense
    // fallback. If this ever trips, Lanczos coverage has silently been lost.
    assert!(
        lanczos_c.n_converged > 0,
        "BT1 fixture C must exercise Lanczos (n_converged > 0); got 0, which means \
         routing fell through to the dense fallback and the cross-implementation \
         claim of this file no longer holds",
    );

    let expected_c = fixture_c_expected_5();
    assert_matches_closed_form(
        &dense_c.eigenvalues,
        &expected_c,
        1e-8,
        "BT1 fixture C dense at σ=0",
    );
    assert_matches_closed_form(
        &lanczos_c.eigenvalues,
        &expected_c,
        1e-8,
        "BT1 fixture C Lanczos at σ=0",
    );
    assert_order_ascending_by_abs_lambda(&dense_c.eigenvalues, "BT1 fixture C dense at σ=0");
    assert_order_ascending_by_abs_lambda(&lanczos_c.eigenvalues, "BT1 fixture C Lanczos at σ=0");

    for (label, res) in [("dense", &dense_c), ("Lanczos", &lanczos_c)] {
        assert_eq!(res.shift, 0.0, "BT1 fixture C {label}: shift must be 0.0");
        assert!(
            !res.shift_skipped_modes,
            "BT1 fixture C {label}: at σ=0 nothing can be skipped — the interval \
             strictly between 0 and 0 is empty",
        );
    }
}

// ---------------------------------------------------------------------------
// BT2 — cross-implementation agreement (C2 + C3 + C5)
// ---------------------------------------------------------------------------

/// **BT2.** Dense and Lanczos, on a pencil sized to be solvable both ways, return
/// the same eigenvalue set to solver tolerance, the same C3 order and the same C5
/// provenance boolean.
///
/// Instantiated here at σ=0, which is the only shift both implementations honor
/// in leaf α: the Lanczos path does not honor σ until β (#7259) lands the
/// `K − σB` assembly and the Cholesky-then-LU dispatch. β adds its σ≠0 arms by
/// calling [`assert_implementations_agree`] with a non-zero σ — the acceptance
/// criterion is this function, not a new one written there.
///
/// The 1e-8 tolerance is the one the landed
/// `shift_invert_and_dense_agree_on_80dof_synthetic_pair` already measures for
/// this exact pair.
#[test]
fn implementations_agree_at_shift() {
    let (k, b) = fixture_c();
    let opts = EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };
    let dense = solve_eigen_dense(&k, &b, opts.clone());
    let lanczos = solve_eigen_shift_invert(&k, &b, opts);

    assert!(
        lanczos.n_converged > 0,
        "BT2 must exercise Lanczos (n_converged > 0); got 0, which means routing fell \
         through to the dense fallback and this test would be comparing dense to dense",
    );

    assert_implementations_agree(&dense, &lanczos, 1e-8, "BT2 fixture C at σ=0");
}
