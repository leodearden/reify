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
//! (C3) and [`assert_implementations_agree`] (the BT2 body: C2 multiset
//! agreement, C3 order, C5 provenance). Adding β's σ≠0 coverage is a one-line
//! instantiation of the latter, not a new harness.
//!
//! BT2 has no test of its own in leaf α. Its definition is "dense and Lanczos at
//! the same σ≠0 return the same eigenvalue multiset", and α cannot instantiate
//! that — the Lanczos path does not honor σ until β. Its σ=0 arm therefore runs
//! inside BT1, on the two results BT1 has already solved, rather than under a
//! name that promises a shift it does not apply.
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
//! - **Fixture D** — 5×5 indefinite diagonal pair (K = diag(−2, −0.5, 1, 3, 4),
//!   B = I), spectrum = K's diagonal. n=5, so **dense path only**. A and C are
//!   both positive-definite, and a positive-definite pencil cannot reach the
//!   negative half of the contract at all; D exists for exactly that half.
//! - **Fixture E** — 80-DOF 1D Laplacian again (K = tridiag(-1,2,-1)) but with
//!   B = I **plus** symmetric corner entries at (0,79)/(79,0), so B's sparsity
//!   pattern is NOT a subset of K's. n=80, so **Lanczos**, enforced the same way
//!   (`n_converged > 0`). The pattern difference is the whole fixture: on C
//!   (B = I ⊆ K) the union of the two patterns equals K's own, so a `K − 0·B`
//!   assembly is indistinguishable from K and a σ=0 identity test passes
//!   vacuously. On E the union is strictly larger and the extra entries induce
//!   real Cholesky fill, so a σ=0 solve wrongly routed through the shifted
//!   assembly is observable.
//!
//! Fixtures A and C and the 1e-12 / 1e-8 tolerances are ported from the landed
//! `crates/reify-solver-elastic/tests/eigensolve_synthetic.rs`, where they are
//! already measured against this same `gevd_real` / `partial_self_adjoint_eigen`
//! pair (`dense_recovers_known_spectrum_on_5x5_diagonal_pair` and
//! `shift_invert_and_dense_agree_on_80dof_synthetic_pair`). The tolerances here
//! are therefore a measured precedent, not a guess.
//!
//! # Gate residency
//!
//! This binary is gate-resident from the commit that adds it, deliberately:
//! `REIFY_HEAVY_NEXTEST_FILTER` excludes only `determinism`,
//! `analytical_validation` and `modal_benchmarks` from this package, and no
//! exclusion is added for this one — these are small analytic pencils, and
//! excluding them would remove the contract harness from the merge gate that β
//! (#7259) is held to. β adds its σ≠0 arms HERE rather than in a new binary,
//! for that reason: a separate binary would need its own filter and override
//! entries, and would put the two halves of one contract on different gates.
//!
//! It needs no `.config/nextest.toml` override, and that is MEASURED rather
//! than assumed — re-measured by β after adding five tests, rather than
//! presumed to survive on α's margin. Under the repo nextest config the slowest
//! single test is **2.449 s debug** (`dense_and_lanczos_agree_at_nonzero_sigma`,
//! four 80-DOF solves), whole binary **2.461 s over 15 tests**, against the
//! `[profile.default]` per-test ceiling of
//! `slow-timeout = { period = "120s", terminate-after = 10 }` = 1200 s. No
//! `[[profile.default.overrides]]` block matches `binary(eigensolve_shift_contract)`,
//! so that default ceiling is what applies, leaving a **~490x margin** on the
//! debug figure — taken under ordinary lane contention, and ample against the
//! worst contention multiplier this repo has recorded. No override is added.
//!
//! (α's figure for comparison, same conditions: 1.495 s slowest / 1.513 s over
//! 10 tests. β's five new arms roughly double the wall clock and leave the
//! order of magnitude of the margin unchanged.)

use faer::Side;
use faer::Mat;
use faer::sparse::{SparseRowMat, Triplet};
use reify_solver_elastic::eigensolve::{
    EigenSolverOptions, EigenSolverResult, ShiftInvertFailure, SparseFactorRef, SparseMetricOp,
    SparseStiffnessOp, lanczos_shift_invert, solve_eigen_dense, solve_eigen_shift_invert,
    try_solve_eigen_shift_invert,
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

/// Fixture D: K = diag(−2, −0.5, 1, 3, 4), B = I (5×5). Dense path only.
///
/// The INDEFINITE fixture. A and C are both strictly positive-definite pencils,
/// so neither can reach the negative-λ half of the contract: the `sigma < λ < 0`
/// disjunct of the C5 predicate, and C3's absolute-value order (which only
/// differs from a signed order when both signs are present). B = I makes the
/// pencil's eigenvalues exactly K's diagonal, so the arithmetic stays closed-form.
///
/// Not hypothetical: `buckling_kernel` assembles a `neg_sigma` geometric
/// stiffness for the reversed-load case, which is precisely an indefinite pencil.
fn fixture_d() -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    let diag = fixture_d_spectrum();
    let n = diag.len();
    let k_trips: Vec<Triplet<usize, usize, f64>> = diag
        .iter()
        .enumerate()
        .map(|(i, &d)| Triplet::new(i, i, d))
        .collect();
    let b_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();
    let b = SparseRowMat::try_new_from_triplets(n, n, &b_trips).unwrap();
    (k, b)
}

/// Fixture D's spectrum, in the diagonal's own order (NOT a contract order).
fn fixture_d_spectrum() -> [f64; 5] {
    [-2.0, -0.5, 1.0, 3.0, 4.0]
}

/// Fixture E: K = tridiag(-1,2,-1) (80×80), B = I **plus corner entries** at
/// (0,79) and (79,0). n=80 > 64 so Lanczos runs.
///
/// The PATTERN-UNION fixture, and the corner entries are the entire point.
/// Fixture C's B = I is a sparsity-pattern SUBSET of K, so the union of the two
/// patterns equals K's own and a `K − 0·B` assembly would be indistinguishable
/// from K itself — a σ=0 identity test on fixture C therefore passes VACUOUSLY,
/// whether or not σ=0 is really special-cased.
///
/// Here the union is strictly larger than K's pattern: (0,79) and (79,0) are
/// present in B and absent from K. A `K − 0·B` routed through the shifted
/// assembly would store them as EXPLICIT ZEROS, which changes the Cholesky
/// fill-in (the corners connect the two ends of the tridiagonal band, inducing
/// real fill) and therefore the rounding. So a σ=0 solve that wrongly went
/// through the shifted path is observable here as a bit-level difference, which
/// is what makes `sigma_zero_factors_k_itself_not_k_minus_zero_b` a genuine
/// tripwire rather than a tautology.
///
/// K is unchanged from fixture C and so still SPD, which is what keeps the σ=0
/// Cholesky succeeding. B stays symmetric, and with the corner entries at 1.0
/// it is I + (e₀e₇₉ᵀ + e₇₉e₀ᵀ), whose eigenvalues are {2, 0, 1, …, 1} — still
/// positive semi-definite, so no closed form is claimed for the pencil and none
/// is needed: this fixture's only job is the pattern difference.
fn fixture_e() -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
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
    let mut b_trips: Vec<Triplet<usize, usize, f64>> =
        (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    // The two off-pattern entries: present in B, absent from K.
    b_trips.push(Triplet::new(0, n - 1, 1.0));
    b_trips.push(Triplet::new(n - 1, 0, 1.0));
    let k = SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap();
    let b = SparseRowMat::try_new_from_triplets(n, n, &b_trips).unwrap();
    (k, b)
}

/// Closed-form `λ_k = 2(1 − cos(kπ/81))` of the 80-DOF Laplacian, k = 1..=80.
///
/// 1-INDEXED, matching the formula and the way the modes are named throughout
/// this file (λ₁ is the first mode, not λ₀).
fn fixture_c_lambda(k: usize) -> f64 {
    assert!((1..=80).contains(&k), "fixture C has modes k = 1..=80, not {k}");
    2.0 * (1.0 - f64::cos(k as f64 * std::f64::consts::PI / 81.0))
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
/// Set agreement is a genuine **multiset** comparison: both eigenvalue vectors
/// are sorted into scratch copies by signed λ and compared element-wise within
/// `tol`. That is order-insensitive, so it tests C2 (which eigenvalues came
/// back) without silently re-testing C3 (what order they came back in) — C3 is
/// asserted separately on each result below.
///
/// A nearest-match scan in both directions would be weaker and is deliberately
/// not used: it is a Hausdorff-style check, and Hausdorff distance is blind to
/// multiplicity. `{1, 1, 2}` and `{1, 2, 2}` have the same length and every
/// element of each has an exact counterpart in the other, so a bidirectional
/// scan accepts them. Repeated and near-repeated eigenvalues are exactly where
/// a shift-invert selection bug duplicates a mode, and β (#7259) instantiates
/// this function as its σ≠0 acceptance criterion — so the check has to be able
/// to see that.
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

    // C2 — same multiset, order-insensitive.
    let sorted = |v: &[f64]| -> Vec<f64> {
        let mut s = v.to_vec();
        s.sort_by(f64::total_cmp);
        s
    };
    let (sa, sb) = (sorted(&res_a.eigenvalues), sorted(&res_b.eigenvalues));
    for (i, (&x, &y)) in sa.iter().zip(sb.iter()).enumerate() {
        assert!(
            (x - y).abs() < tol,
            "{ctx}: C2 violated — sorted eigenvalue[{i}] is {x:.15} in implementation a \
             but {y:.15} in b, {:.3e} apart ≥ tol = {tol:.3e}; a = {:?}, b = {:?} \
             (sorted: {sa:?} vs {sb:?})",
            (x - y).abs(),
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

    // BT2 at σ=0, on the two results already in hand.
    //
    // BT2 proper is "dense and Lanczos at the same σ≠0 return the same
    // eigenvalue multiset", and no σ≠0 arm can exist in leaf α: the Lanczos path
    // does not honor σ until β (#7259) lands the `K − σB` assembly and the
    // Cholesky-then-LU dispatch. Its σ=0 arm belongs here rather than in a test
    // of its own named for a shift it does not apply — and re-solving this
    // 80-DOF pencil both ways a second time to assert a strict superset of what
    // is already asserted above would be pure gate cost.
    //
    // β adds the real BT2 by calling `assert_implementations_agree` with a
    // non-zero σ. The acceptance criterion is that function, not a new one
    // written there. The 1e-8 tolerance is the one the landed
    // `shift_invert_and_dense_agree_on_80dof_synthetic_pair` already measures
    // for this exact pair.
    assert_implementations_agree(&dense_c, &lanczos_c, 1e-8, "BT2 fixture C at σ=0");
}

// ---------------------------------------------------------------------------
// Eigenvector residual helper (ported from eigensolve_synthetic.rs)
//
// Pins the C3 column permutation to the eigenvalue permutation: an
// implementation that reorders `eigenvalues` without reordering the matching
// eigenvector columns passes an eigenvalue-only assertion and fails this one.
// ---------------------------------------------------------------------------

/// `y = M · x` for a `SparseRowMat` (CSR), by iterating stored entries.
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

/// Assert `‖K φ_i − λ_i B φ_i‖ / ‖K φ_i‖ < tol` for every returned mode.
fn assert_eigen_residuals(
    k: &SparseRowMat<usize, f64>,
    b: &SparseRowMat<usize, f64>,
    eigenvalues: &[f64],
    eigenvectors: &Mat<f64>,
    tol: f64,
    ctx: &str,
) {
    let n = k.nrows();
    assert_eq!(
        eigenvectors.nrows(),
        n,
        "{ctx}: eigenvector row count mismatch"
    );
    assert_eq!(
        eigenvectors.ncols(),
        eigenvalues.len(),
        "{ctx}: eigenvector column count must match eigenvalue count",
    );
    for (i, &lam) in eigenvalues.iter().enumerate() {
        let phi: Vec<f64> = (0..n).map(|row| eigenvectors[(row, i)]).collect();
        let k_phi = csr_matvec(k, &phi);
        let b_phi = csr_matvec(b, &phi);
        let resid: Vec<f64> = k_phi
            .iter()
            .zip(b_phi.iter())
            .map(|(k_, b_)| k_ - lam * b_)
            .collect();
        let rel = l2_norm(&resid) / l2_norm(&k_phi).max(f64::MIN_POSITIVE);
        assert!(
            rel < tol,
            "{ctx}: mode[{i}] (λ={lam}) — ‖Kφ − λBφ‖/‖Kφ‖ = {rel:.3e} ≥ tol = {tol:.3e}; \
             the eigenvector column does not belong to this eigenvalue, so the C3 \
             column permutation has drifted from the eigenvalue permutation",
        );
    }
}

// ---------------------------------------------------------------------------
// BT3 — selection really moved (C2), and did NOT drag the order with it (C3)
// ---------------------------------------------------------------------------

/// **BT3.** At a σ above λ₁ the returned set differs from the σ=0 set.
///
/// Fixture A, n_modes=2, σ=0.6. Distances |λ − σ| over the exact spectrum
/// {0.2, 0.25, 1/3, 0.5, 1.0} are {0.4, 0.35, 0.267, 0.1, 0.4}, so C2 selects
/// the nearest pair {0.5, 1/3} — while σ=0 would select {0.2, 0.25}.
///
/// C2 and C3 are asserted SEPARATELY here, because they are two different rules
/// and no implementation may conflate them (PRD §5.2). The selected pair is
/// presented in ascending-|λ| order `[1/3, 0.5]`, **not** in proximity order
/// `[0.5, 1/3]` — a comparator that sorted the output by |λ − σ| would return
/// the right *set* and still fail this test, which is the point.
///
/// The residual assertion pins the eigenvector columns to the same permutation,
/// so C3 covers the whole result rather than just the eigenvalue vector.
#[test]
fn selection_really_moved_above_lambda_one() {
    let (k, b) = fixture_a();
    let base = EigenSolverOptions {
        n_modes: 2,
        tol: 1e-12,
        max_iters: 1000,
        sigma: 0.0,
    };

    let unshifted = solve_eigen_dense(&k, &b, base.clone());
    assert_matches_closed_form(
        &unshifted.eigenvalues,
        &[0.2, 0.25],
        1e-12,
        "BT3 baseline at σ=0",
    );

    let shifted = solve_eigen_dense(&k, &b, EigenSolverOptions { sigma: 0.6, ..base });

    // C2 — the |λ−σ|-nearest pair came back.
    assert_matches_closed_form(
        &shifted.eigenvalues,
        &[1.0 / 3.0, 0.5],
        1e-12,
        "BT3 selection at σ=0.6",
    );
    assert_eq!(
        shifted.shift, 0.6,
        "BT3: the σ used must be reported as 0.6"
    );

    // C2 — and it is genuinely a DIFFERENT set from the σ=0 one.
    for &lam in &shifted.eigenvalues {
        assert!(
            !unshifted
                .eigenvalues
                .iter()
                .any(|&u| (u - lam).abs() < 1e-12),
            "BT3: σ=0.6 returned λ = {lam:.15}, which is also in the σ=0 set {:?} — \
             the shift did not move the selection",
            unshifted.eigenvalues,
        );
    }

    // C3 — presentation order is ascending |λ|, NOT proximity to σ.
    assert_order_ascending_by_abs_lambda(&shifted.eigenvalues, "BT3 order at σ=0.6");
    assert_eq!(
        shifted.eigenvectors.ncols(),
        2,
        "BT3: eigenvector matrix must have one column per returned mode",
    );
    assert_eigen_residuals(
        &k,
        &b,
        &shifted.eigenvalues,
        &shifted.eigenvectors,
        1e-8,
        "BT3 at σ=0.6",
    );
}

// ---------------------------------------------------------------------------
// BT5 — σ placed exactly on a known eigenvalue
// ---------------------------------------------------------------------------

/// **BT5.** σ placed exactly on a known eigenvalue of a small analytic pencil.
///
/// On the DENSE path this is **not a failure**: contract clause C6 is satisfied
/// VACUOUSLY there because no `K − σB` is ever formed — σ is a sort key, not a
/// factorization — so σ on an eigenvalue is a well-posed selection. What must
/// hold is that the solve completes, every returned value is finite, and the
/// eigenvalue σ sits on is the nearest one and therefore comes back.
///
/// Fixture A, n_modes=2, σ=0.5 (exactly the eigenvalue 1/2). Distances are
/// {0.5: 0, 1/3: 0.167, 0.25: 0.25, 0.2: 0.3, 1.0: 0.5}, so C2 selects
/// {0.5, 1/3}, presented in ascending-|λ| order as `[1/3, 0.5]`.
///
/// β (#7259) adds the Lanczos arm against this same fixture, where `K − σB` IS
/// formed and the C6 **typed failure** (`DiagnosticCode::ShiftAtEigenvalue`,
/// carrying σ) is the required behaviour — never a panic, never a silently
/// perturbed solve, never a garbage spectrum.
#[test]
fn shift_at_an_eigenvalue_is_not_a_failure_on_the_dense_path() {
    let (k, b) = fixture_a();
    let result = solve_eigen_dense(
        &k,
        &b,
        EigenSolverOptions {
            n_modes: 2,
            tol: 1e-12,
            max_iters: 1000,
            sigma: 0.5,
        },
    );

    assert_matches_closed_form(
        &result.eigenvalues,
        &[1.0 / 3.0, 0.5],
        1e-12,
        "BT5 at σ=0.5 (exactly on an eigenvalue)",
    );
    assert_order_ascending_by_abs_lambda(&result.eigenvalues, "BT5 at σ=0.5");
    assert_eq!(result.shift, 0.5, "BT5: the σ used must be reported as 0.5");

    for (i, &lam) in result.eigenvalues.iter().enumerate() {
        assert!(
            lam.is_finite(),
            "BT5: eigenvalue[{i}] = {lam} is not finite — σ on an eigenvalue must not \
             produce NaN or inf on the dense path",
        );
    }
    assert!(
        result
            .eigenvalues
            .iter()
            .any(|&lam| (lam - 0.5).abs() < 1e-12),
        "BT5: the eigenvalue σ sits on (0.5) is the nearest one and must be returned; \
         got {:?}",
        result.eigenvalues,
    );
    assert_eigen_residuals(
        &k,
        &b,
        &result.eigenvalues,
        &result.eigenvectors,
        1e-8,
        "BT5 at σ=0.5",
    );
}

// ---------------------------------------------------------------------------
// BT4 — provenance is honest in BOTH directions (C5)
//
// All three cases are on Fixture A, on the dense path, where the answer is
// EXACT: `gevd_real` yields the whole spectrum, so "does an eigenvalue lie
// strictly between 0 and σ and fail to come back?" is decidable, not estimated.
// ---------------------------------------------------------------------------

/// **BT4(a).** σ below λ₁ ⟹ nothing was skipped, and `modes[0]` is still the
/// genuine first mode.
///
/// Fixture A, n_modes=2, σ=0.1 — below λ₁=0.2, so the open interval (0, 0.1)
/// contains no eigenvalue at all. The selected pair is still {0.2, 0.25}.
///
/// The `eigenvalues[0] == ` the σ=0 first mode assertion is the exact condition
/// ε (#7262) keys its `critical_load` / `safety_factor_buckling` /
/// `first_frequency` refusal on: when this holds the helpers are correct and
/// must NOT refuse.
#[test]
fn provenance_below_lambda_one_reports_nothing_skipped() {
    let (k, b) = fixture_a();
    let base = EigenSolverOptions {
        n_modes: 2,
        tol: 1e-12,
        max_iters: 1000,
        sigma: 0.0,
    };

    let unshifted = solve_eigen_dense(&k, &b, base.clone());
    let shifted = solve_eigen_dense(&k, &b, EigenSolverOptions { sigma: 0.1, ..base });

    assert_matches_closed_form(&shifted.eigenvalues, &[0.2, 0.25], 1e-12, "BT4(a) at σ=0.1");
    assert_eq!(
        shifted.shift, 0.1,
        "BT4(a): the σ used must be reported as 0.1"
    );
    assert!(
        !shifted.shift_skipped_modes,
        "BT4(a): C5 violated — σ=0.1 lies below λ₁=0.2, so the open interval (0, 0.1) \
         contains no eigenvalue and nothing can have been skipped; got true",
    );
    assert!(
        (shifted.eigenvalues[0] - unshifted.eigenvalues[0]).abs() < 1e-12,
        "BT4(a): modes[0] at σ=0.1 is {:.15} but the σ=0 first mode is {:.15} — a shift \
         below λ₁ must leave the first mode intact",
        shifted.eigenvalues[0],
        unshifted.eigenvalues[0],
    );
}

/// **BT4(b).** σ above λ₁ ⟹ modes WERE skipped.
///
/// Fixture A, n_modes=2, σ=0.6. The selected set is {1/3, 0.5}, so 0.2 and 0.25
/// both lie strictly between 0 and σ and are both absent — `modes[0]` is
/// 1/3, which is NOT the first mode, and a helper that reported it as one would
/// be wrong in the unconservative direction.
#[test]
fn provenance_above_lambda_one_reports_skipped() {
    let (k, b) = fixture_a();
    let result = solve_eigen_dense(
        &k,
        &b,
        EigenSolverOptions {
            n_modes: 2,
            tol: 1e-12,
            max_iters: 1000,
            sigma: 0.6,
        },
    );

    assert_matches_closed_form(
        &result.eigenvalues,
        &[1.0 / 3.0, 0.5],
        1e-12,
        "BT4(b) at σ=0.6",
    );
    assert_eq!(
        result.shift, 0.6,
        "BT4(b): the σ used must be reported as 0.6"
    );
    assert!(
        result.shift_skipped_modes,
        "BT4(b): C5 violated — 0.2 and 0.25 lie strictly between 0 and σ=0.6 and are \
         absent from the returned set {:?}; got false",
        result.eigenvalues,
    );
}

/// **BT4(c).** The sharp case: C5 counts ABSENCE from the returned set, not
/// position relative to σ.
///
/// Fixture A, n_modes=3, σ=0.3. Distances are
/// {1/3: 0.033, 0.25: 0.05, 0.2: 0.1, 0.5: 0.2, 1.0: 0.7}, so the selected set
/// is {1/3, 0.25, 0.2}, presented as [0.2, 0.25, 1/3].
///
/// 0.2 and 0.25 DO lie strictly below σ=0.3 — and `shift_skipped_modes` must
/// still be **false**, because both are RETURNED. This is the case that
/// separates C5's "absent from the returned set" from a naive "any eigenvalue
/// below σ" count, and it is the defect most likely to be coded by mistake: the
/// naive version reports `true` here and would make ε (#7262) refuse a result
/// whose first mode is perfectly present.
#[test]
fn provenance_counts_absence_not_position() {
    let (k, b) = fixture_a();
    let result = solve_eigen_dense(
        &k,
        &b,
        EigenSolverOptions {
            n_modes: 3,
            tol: 1e-12,
            max_iters: 1000,
            sigma: 0.3,
        },
    );

    assert_matches_closed_form(
        &result.eigenvalues,
        &[0.2, 0.25, 1.0 / 3.0],
        1e-12,
        "BT4(c) at σ=0.3",
    );
    assert_order_ascending_by_abs_lambda(&result.eigenvalues, "BT4(c) at σ=0.3");
    assert_eq!(
        result.shift, 0.3,
        "BT4(c): the σ used must be reported as 0.3"
    );
    assert!(
        !result.shift_skipped_modes,
        "BT4(c): C5 violated — 0.2 and 0.25 lie below σ=0.3 but are both RETURNED in \
         {:?}, so nothing was skipped; reporting true here means the implementation is \
         counting position relative to σ instead of absence from the returned set",
        result.eigenvalues,
    );
}

// ---------------------------------------------------------------------------
// BT4(d)/(e) + C3 — the negative half of the contract, on Fixture D
//
// Fixtures A and C are strictly positive pencils, so between them they cannot
// execute the `sigma < λ < 0` disjunct of the C5 predicate at all, and cannot
// tell C3's absolute-value order apart from a signed one. Both claims are made
// explicitly in the implementation's own rustdoc, so both are pinned here.
// ---------------------------------------------------------------------------

/// **C3 on an indefinite pencil.** Presentation order is ascending `|λ|`, which
/// is NOT ascending λ once both signs are present.
///
/// Fixture D, n_modes=3, σ=0. Distances from zero are
/// {−0.5: 0.5, 1: 1, −2: 2, 3: 3, 4: 4}, so C2 selects {−0.5, 1, −2} and C3
/// presents them as `[−0.5, 1, −2]` — the +1 ahead of the −2, which is the
/// module rustdoc's "λ=−2 still sorts before λ=+3" convention in the one
/// direction a positive-definite fixture can never show. A signed sort would
/// return `[−2, −0.5, 1]` and pass every other test in this file.
#[test]
fn order_is_by_absolute_value_not_signed_value() {
    let (k, b) = fixture_d();
    let result = solve_eigen_dense(
        &k,
        &b,
        EigenSolverOptions {
            n_modes: 3,
            tol: 1e-12,
            max_iters: 1000,
            sigma: 0.0,
        },
    );

    assert_matches_closed_form(
        &result.eigenvalues,
        &[-0.5, 1.0, -2.0],
        1e-12,
        "C3 on indefinite Fixture D at σ=0",
    );
    assert_order_ascending_by_abs_lambda(&result.eigenvalues, "C3 on indefinite Fixture D");
    assert_eigen_residuals(
        &k,
        &b,
        &result.eigenvalues,
        &result.eigenvectors,
        1e-8,
        "C3 on indefinite Fixture D at σ=0",
    );
}

/// **BT4(d).** A NEGATIVE σ with a skipped mode on the negative side.
///
/// Fixture D, n_modes=1, σ=−2.5. Distances are
/// {−2: 0.5, −0.5: 2, 1: 3.5, 3: 5.5, 4: 6.5}, so C2 selects {−2}. Both −2 and
/// −0.5 lie strictly between σ=−2.5 and 0, and −0.5 did NOT come back — so
/// `shift_skipped_modes` must be `true`.
///
/// This is the only case in the suite that executes the `sigma < λ < 0` half of
/// the C5 predicate. Without it that branch is dead code under test, while the
/// helper's rustdoc claims a negative shift on the reversed-load buckling side
/// is handled by the same rule.
#[test]
fn provenance_reports_skipped_on_the_negative_side_of_zero() {
    let (k, b) = fixture_d();
    let result = solve_eigen_dense(
        &k,
        &b,
        EigenSolverOptions {
            n_modes: 1,
            tol: 1e-12,
            max_iters: 1000,
            sigma: -2.5,
        },
    );

    assert_matches_closed_form(&result.eigenvalues, &[-2.0], 1e-12, "BT4(d) at σ=−2.5");
    assert_eq!(
        result.shift, -2.5,
        "BT4(d): the σ used must be reported as −2.5"
    );
    assert!(
        result.shift_skipped_modes,
        "BT4(d): C5 violated — λ=−0.5 lies strictly between σ=−2.5 and 0 and is absent \
         from the returned set {:?}; got false, which means the predicate only looks at \
         the positive side of zero",
        result.eigenvalues,
    );
}

/// **BT4(e).** A negative σ with nothing skipped — the other direction, so
/// BT4(d) cannot be passed by a predicate that simply returns `true` whenever
/// σ < 0.
///
/// Fixture D, n_modes=2, σ=−0.25. Distances are
/// {−0.5: 0.25, 1: 1.25, −2: 1.75, 3: 3.25, 4: 4.25}, so C2 selects {−0.5, 1},
/// presented as `[−0.5, 1]`. The open interval (−0.25, 0) contains no eigenvalue
/// at all — −0.5 is outside it, on the far side of σ — so nothing can have been
/// skipped.
#[test]
fn provenance_reports_nothing_skipped_for_a_negative_shift_above_lambda_one() {
    let (k, b) = fixture_d();
    let result = solve_eigen_dense(
        &k,
        &b,
        EigenSolverOptions {
            n_modes: 2,
            tol: 1e-12,
            max_iters: 1000,
            sigma: -0.25,
        },
    );

    assert_matches_closed_form(
        &result.eigenvalues,
        &[-0.5, 1.0],
        1e-12,
        "BT4(e) at σ=−0.25",
    );
    assert_order_ascending_by_abs_lambda(&result.eigenvalues, "BT4(e) at σ=−0.25");
    assert_eq!(
        result.shift, -0.25,
        "BT4(e): the σ used must be reported as −0.25"
    );
    assert!(
        !result.shift_skipped_modes,
        "BT4(e): C5 violated — the open interval (−0.25, 0) contains no eigenvalue of \
         {:?}, so nothing can have been skipped; got true",
        fixture_d_spectrum(),
    );
}

// ---------------------------------------------------------------------------
// Equidistant σ — the tie-break determinism the selection helper claims
// ---------------------------------------------------------------------------

/// **Determinism at an equidistant σ.** With σ placed between two eigenvalues
/// and only one slot to fill, the same call must resolve the same way every
/// time.
///
/// Fixture A, n_modes=1, σ=0.225 — nominally midway between λ=0.2 and λ=0.25.
/// `select_nearest_to_shift` is documented as a STABLE sort keyed via
/// `total_cmp`, so equidistant candidates resolve from `gevd`'s own index order
/// rather than from whichever the comparator happened to visit first; the
/// crate's determinism suite depends on that.
///
/// What is pinned is REPEATABILITY, not which of the two wins: at this σ the two
/// distances agree only to within a ULP or so (0.2 and 0.225 are not exactly
/// representable in binary), so asserting a specific winner would be asserting
/// a rounding detail. Repeatability is the property the helper actually claims,
/// and it is checked bit-for-bit — on the eigenvector column too, since a
/// selection that flipped would carry its column with it. The membership
/// assertion keeps the test from passing vacuously on some third eigenvalue.
#[test]
fn equidistant_shift_selects_deterministically() {
    let (k, b) = fixture_a();
    let opts = EigenSolverOptions {
        n_modes: 1,
        tol: 1e-12,
        max_iters: 1000,
        sigma: 0.225,
    };

    let first = solve_eigen_dense(&k, &b, opts.clone());
    assert_eq!(
        first.eigenvalues.len(),
        1,
        "equidistant σ: expected exactly one mode, got {:?}",
        first.eigenvalues,
    );
    let lam = first.eigenvalues[0];
    assert!(
        (lam - 0.2).abs() < 1e-12 || (lam - 0.25).abs() < 1e-12,
        "equidistant σ=0.225: the selected eigenvalue must be one of the two nearest \
         candidates (0.2, 0.25); got {lam:.15}",
    );

    for run in 1..4 {
        let again = solve_eigen_dense(&k, &b, opts.clone());
        assert_eq!(
            again.eigenvalues, first.eigenvalues,
            "equidistant σ=0.225: run {run} selected {:?} but the first run selected \
             {:?} — the tie-break is not deterministic, which breaks the stable-sort \
             guarantee the determinism suite relies on",
            again.eigenvalues, first.eigenvalues,
        );
        let col_first: Vec<f64> = (0..k.nrows()).map(|r| first.eigenvectors[(r, 0)]).collect();
        let col_again: Vec<f64> = (0..k.nrows()).map(|r| again.eigenvectors[(r, 0)]).collect();
        assert_eq!(
            col_again, col_first,
            "equidistant σ=0.225: run {run} returned a different eigenvector column for \
             the same eigenvalue — the column permutation is not deterministic",
        );
    }
}

// ---------------------------------------------------------------------------
// C1 structural tripwire — σ=0 factors K ITSELF, never K − 0·B
// ---------------------------------------------------------------------------

/// **C1, structurally.** At σ=0 the sparse path must factor the row-major `K`
/// verbatim — NOT a `K − 0·B` assembly that happens to be numerically equal.
///
/// # Why this cannot be a tolerance test
///
/// `K − 0·B` is not a no-op at the sparsity level. An assembly over the UNION of
/// the two patterns stores an entry wherever EITHER operand has one, so every
/// off-pattern entry of B arrives as an EXPLICIT ZERO. Explicit zeros change the
/// symbolic factorization — different fill-in, a different elimination tree,
/// therefore a different summation order and different rounding. The result is
/// *close*, which is exactly the problem: it would drift the four pinned σ=0
/// golden suites (the `euler_column_pin_pin` BC families at their 0.09–0.11
/// bounds, `buckling_smoke`, `buckling_persistent_cache_round_trip`, and the
/// modal goldens) by an amount no tolerance here would catch.
///
/// So the assertion is BIT-IDENTITY against a reference route the test builds
/// itself out of `k.sp_cholesky(Side::Lower)` on the row-major K. No baseline
/// constants are hardcoded anywhere — the reference is constructed, not recorded
/// — and the claim is the narrow one the `SparseStiffnessOp` adapter already
/// makes in the source: the same faer calls, with the same arguments, in the
/// same order, produce the same bits. It is NOT a claim that iterative
/// convergence is byte-reproducible in general.
///
/// # This test is GREEN the day it lands, deliberately
///
/// It is written BEFORE the σ≠0 work rather than after, because its job is to go
/// RED the moment that work erodes the σ=0 branch. If it ever fails, the σ=0
/// guard in `try_solve_eigen_shift_invert` is wrong — **fix the guard, never
/// re-baseline this test.**
///
/// Fixture E, not C, for the reason spelled out on `fixture_e`: C's B = I is a
/// pattern subset of K, so on C the union assembly and K itself are the same
/// matrix and this test would pass vacuously.
#[test]
fn sigma_zero_factors_k_itself_not_k_minus_zero_b() {
    let (k, b) = fixture_e();
    let n = k.nrows();
    let opts = EigenSolverOptions {
        n_modes: 4,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };

    let via_entry_point = try_solve_eigen_shift_invert(&k, &b, opts.clone())
        .expect("fixture E's K is the SPD tridiagonal Laplacian; σ=0 must succeed");

    // Enforce the Lanczos path claim (PRD §5.5 trap), exactly as BT1 does.
    assert!(
        via_entry_point.n_converged > 0,
        "fixture E must exercise Lanczos (n_converged > 0); got 0, which means routing \
         fell through to the dense fallback and this tripwire no longer guards the \
         sparse σ=0 branch at all",
    );

    // The reference route, built here rather than recorded: factor the ROW-MAJOR
    // K directly and drive the generic core with it.
    let llt = k
        .sp_cholesky(Side::Lower)
        .expect("fixture E's K is SPD, so its Cholesky must succeed");
    let k_op = SparseStiffnessOp {
        factor: SparseFactorRef::Cholesky(&llt),
        n,
    };
    let m_op = SparseMetricOp { m: b.as_ref() };
    let reference = lanczos_shift_invert(&k_op, &m_op, opts);

    assert_eq!(
        via_entry_point.eigenvalues, reference.eigenvalues,
        "C1 violated STRUCTURALLY: at σ=0 the entry point returned {:?} but factoring \
         the row-major K itself returns {:?}. Equal-to-a-tolerance is not enough here — \
         a difference in these bits means σ=0 is being routed through a K−0·B assembly \
         whose explicit zeros changed the Cholesky fill-in, which silently moves every \
         pinned σ=0 golden. Fix the σ=0 guard; do NOT re-baseline.",
        via_entry_point.eigenvalues, reference.eigenvalues,
    );

    // The eigenVECTORS too: an eigenvalue-only check passes on a result whose
    // columns were permuted or re-converged differently.
    assert_eq!(
        via_entry_point.eigenvectors.nrows(),
        reference.eigenvectors.nrows(),
        "C1: eigenvector row counts differ between the entry point and the reference",
    );
    assert_eq!(
        via_entry_point.eigenvectors.ncols(),
        reference.eigenvectors.ncols(),
        "C1: eigenvector column counts differ between the entry point and the reference",
    );
    for col in 0..reference.eigenvectors.ncols() {
        for row in 0..reference.eigenvectors.nrows() {
            let got = via_entry_point.eigenvectors[(row, col)];
            let want = reference.eigenvectors[(row, col)];
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "C1 violated STRUCTURALLY: eigenvector[{row}][{col}] is {got:.17e} from the \
                 entry point but {want:.17e} from factoring the row-major K itself — σ=0 \
                 is not on today's exact path. Fix the σ=0 guard; do NOT re-baseline.",
            );
        }
    }

    // Provenance is part of the identity: σ=0 reports the σ it used and an
    // ESTABLISHED `false` (the open interval strictly between 0 and 0 is empty).
    assert_eq!(
        via_entry_point.shift, 0.0,
        "C1: a σ=0 solve must report shift == 0.0",
    );
    assert!(
        !via_entry_point.shift_skipped_modes,
        "C1: at σ=0 nothing can be skipped — the interval strictly between 0 and 0 is empty",
    );
}

// ---------------------------------------------------------------------------
// BT3 on the Lanczos path — C2 selection, C3 order, C4 back-shift at σ≠0
// ---------------------------------------------------------------------------

/// σ for the Lanczos σ≠0 arms: midway between λ₉ and λ₁₀ of fixture C.
///
/// Placed ABOVE several modes on purpose. A σ below λ₁ would leave the nearest
/// set equal to the bottom of the spectrum, so a path that ignored σ entirely
/// would still return the right eigenvalues and C2 would go untested.
///
/// `K − σB` is indefinite here (nine eigenvalues lie below σ), which is exactly
/// the case that forces the Cholesky-then-LU fallback rather than exercising
/// only the Cholesky arm.
fn sigma_between_lambda_9_and_10() -> f64 {
    0.5 * (fixture_c_lambda(9) + fixture_c_lambda(10))
}

/// **BT3, Lanczos.** At a σ above several modes the shift-invert path selects
/// the |λ − σ|-nearest set (C2), presents it ascending by |λ| (C3), returns it
/// in the ORIGINAL λ space (C4), and reports the σ it actually used.
///
/// C2 and C3 are asserted separately and must not be conflated: the nearest set
/// here is {λ₉, λ₁₀}, and presentation order is by |λ| — which for this pencil
/// coincides with ascending λ, so the order assertion is about the RULE, not
/// about this fixture's arithmetic.
///
/// The "different from the σ=0 answer" assertion is what makes the selection
/// claim non-vacuous: without it a path that quietly ignored σ would still
/// satisfy every closed-form comparison, because it would return λ₁ and λ₂ and
/// those are genuinely eigenvalues of this pencil.
///
/// Nothing is asserted here about `shift_skipped_modes` — step-6 owns C5, and
/// pinning it now would make that step's RED vacuous.
#[test]
fn lanczos_selects_the_nearest_set_and_back_shifts_at_nonzero_sigma() {
    let (k, b) = fixture_c();
    let sigma = sigma_between_lambda_9_and_10();
    let opts = EigenSolverOptions {
        n_modes: 2,
        tol: 1e-10,
        max_iters: 1000,
        sigma,
    };

    let got = solve_eigen_shift_invert(&k, &b, opts.clone());

    // Enforce the Lanczos path claim (PRD §5.5 trap) rather than asserting it
    // in prose: n_converged is 0 on the dense fallback.
    assert!(
        got.n_converged > 0,
        "BT3 Lanczos must exercise Lanczos (n_converged > 0); got 0, which means routing \
         fell through to the dense fallback and this test says nothing about the \
         shift-invert implementation",
    );

    // C5's own field is the honest report of which σ was solved at.
    assert_eq!(
        got.shift, sigma,
        "BT3 Lanczos: the result must report the σ it actually used ({sigma}), not {}",
        got.shift,
    );

    // C2 — the nearest set, against the closed form.
    assert_matches_closed_form(
        &got.eigenvalues,
        &[fixture_c_lambda(9), fixture_c_lambda(10)],
        1e-8,
        "BT3 Lanczos C2 at σ between λ₉ and λ₁₀",
    );

    // C2, non-vacuously — the answer really moved off the bottom of the spectrum.
    let at_zero = solve_eigen_shift_invert(
        &k,
        &b,
        EigenSolverOptions {
            sigma: 0.0,
            ..opts.clone()
        },
    );
    assert_matches_closed_form(
        &at_zero.eigenvalues,
        &[fixture_c_lambda(1), fixture_c_lambda(2)],
        1e-8,
        "BT3 Lanczos control: σ=0 must still return the bottom of the spectrum",
    );
    for (i, (&shifted, &unshifted)) in got
        .eigenvalues
        .iter()
        .zip(at_zero.eigenvalues.iter())
        .enumerate()
    {
        assert!(
            (shifted - unshifted).abs() > 1e-3,
            "BT3 Lanczos: eigenvalue[{i}] is {shifted} at σ={sigma} and {unshifted} at σ=0 — \
             the shifted solve returned the SAME set as the unshifted one, so σ was ignored",
        );
    }

    // C3 — presentation order, asserted separately from selection.
    assert_order_ascending_by_abs_lambda(&got.eigenvalues, "BT3 Lanczos C3 at σ≠0");

    // C4 — back-shifted into the original λ space, with the eigenvector columns
    // pinned to the same permutation as the eigenvalues. A result still in μ
    // space, or back-shifted with the wrong sign, fails here even though the
    // closed-form check above could be passed by a coincidence.
    assert_eigen_residuals(
        &k,
        &b,
        &got.eigenvalues,
        &got.eigenvectors,
        1e-8,
        "BT3 Lanczos C4 at σ≠0",
    );
}

// ---------------------------------------------------------------------------
// BT4 — C5 provenance on the Lanczos path
// ---------------------------------------------------------------------------

/// σ below λ₁ of fixture C: half the first eigenvalue.
///
/// `K − σB` is still positive definite here, so the shifted Cholesky succeeds
/// and its success ESTABLISHES `shift_skipped_modes == false` by Sylvester's law
/// of inertia — the evidence C5 requires, rather than an assumption.
fn sigma_below_lambda_1() -> f64 {
    0.5 * fixture_c_lambda(1)
}

/// **BT4, Lanczos.** The C5 discriminator must answer `false` where `false` is
/// established and `true` where modes really were passed.
///
/// BOTH arms are load-bearing and neither is redundant: the first alone is
/// passed by a predicate that always returns `false`, the second alone by one
/// that always returns `true`. Only together do they pin a discriminator that
/// actually discriminates — and today's `conservative_shift_provenance` (which
/// is `sigma != 0.0`) is exactly the always-`true` predicate the second arm
/// cannot catch.
#[test]
fn lanczos_provenance_distinguishes_below_and_above_lambda_one() {
    let (k, b) = fixture_c();
    let base = EigenSolverOptions {
        n_modes: 2,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };

    let unshifted = solve_eigen_shift_invert(&k, &b, base.clone());

    // (a) σ below λ₁ — nothing can have been skipped, and the shifted Cholesky
    // proves it.
    let below = solve_eigen_shift_invert(
        &k,
        &b,
        EigenSolverOptions {
            sigma: sigma_below_lambda_1(),
            ..base.clone()
        },
    );
    assert!(
        below.n_converged > 0,
        "BT4(a) Lanczos must exercise Lanczos (n_converged > 0); got 0",
    );
    assert!(
        !below.shift_skipped_modes,
        "BT4(a) Lanczos: C5 violated — σ = {} lies below λ₁ = {}, so K − σB is positive \
         definite, its Cholesky succeeds, and by Sylvester's law of inertia no eigenvalue \
         lies between 0 and σ. `false` is ESTABLISHED here, not assumed; got true",
        sigma_below_lambda_1(),
        fixture_c_lambda(1),
    );
    assert!(
        (below.eigenvalues[0] - unshifted.eigenvalues[0]).abs() < 1e-9,
        "BT4(a) Lanczos: modes[0] at σ={} is {:.15} but the σ=0 first mode is {:.15} — a \
         shift below λ₁ must leave the first mode intact",
        sigma_below_lambda_1(),
        below.eigenvalues[0],
        unshifted.eigenvalues[0],
    );

    // (b) σ above several modes — eight eigenvalues lie strictly between 0 and σ
    // and none of them comes back, so the honest answer is `true`.
    let above = solve_eigen_shift_invert(
        &k,
        &b,
        EigenSolverOptions {
            sigma: sigma_between_lambda_9_and_10(),
            ..base
        },
    );
    assert!(
        above.n_converged > 0,
        "BT4(b) Lanczos must exercise Lanczos (n_converged > 0); got 0",
    );
    assert!(
        above.shift_skipped_modes,
        "BT4(b) Lanczos: C5 violated — σ = {} lies above λ₁…λ₈ and the returned set is \
         {:?}, so modes WERE passed; got false",
        sigma_between_lambda_9_and_10(),
        above.eigenvalues,
    );
}

// ---------------------------------------------------------------------------
// BT2 — cross-implementation agreement at σ≠0 (the acceptance criterion)
// ---------------------------------------------------------------------------

/// **BT2.** Dense and Lanczos, same pencil, same σ≠0, same answer.
///
/// This is the acceptance criterion for the Lanczos shift implementation, and it
/// is an INSTANTIATION of [`assert_implementations_agree`] — the body α wrote
/// for exactly this — not a new harness.
///
/// # Why these two σ and not others: the C5 predicates are not the same predicate
///
/// The two paths decide `shift_skipped_modes` by different means, and
/// [`assert_implementations_agree`] compares the answers:
///
/// - **Dense is ABSENCE-based.** It has the whole spectrum, so it asks "does
///   some eigenvalue lie between 0 and σ *and not come back*?"
/// - **Lanczos is POSITION-based.** It has no spectrum, only which
///   factorization succeeded, so by Sylvester it asks "does some eigenvalue lie
///   between 0 and σ?" — with no absence clause available to it.
///
/// Cholesky SUCCESS implies nothing lies in the interval, which implies the
/// absence-based answer is also `false`, so the two always coincide below λ₁.
/// Cholesky FAILURE only implies something lies in the interval; that something
/// may still have been RETURNED, and in that configuration dense says `false`
/// while Lanczos says `true`. Lanczos OVER-reports, which is the conservative
/// direction C5 permits ("`false` only when established") — a fixture-design
/// constraint on this test, **not a defect in either side, and not something to
/// be "fixed" by weakening one of them.** `provenance_counts_absence_not_position`
/// is the dense-path test that pins that very configuration.
///
/// So the above-λ₁ σ here is chosen well above the bottom of the spectrum with a
/// small `n_modes` — σ between λ₉ and λ₁₀, n_modes=2 — so the selected window
/// genuinely does not reach back down past σ and the two predicates agree.
#[test]
fn dense_and_lanczos_agree_at_nonzero_sigma() {
    let (k, b) = fixture_c();
    let base = EigenSolverOptions {
        n_modes: 2,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };

    for (sigma, label) in [
        (sigma_below_lambda_1(), "σ below λ₁"),
        (sigma_between_lambda_9_and_10(), "σ between λ₉ and λ₁₀"),
    ] {
        let opts = EigenSolverOptions {
            sigma,
            ..base.clone()
        };
        let dense = solve_eigen_dense(&k, &b, opts.clone());
        let lanczos = solve_eigen_shift_invert(&k, &b, opts);

        assert!(
            lanczos.n_converged > 0,
            "BT2 at {label}: must exercise Lanczos (n_converged > 0); got 0, so this is a \
             dense-vs-dense comparison and proves nothing",
        );
        assert_implementations_agree(
            &dense,
            &lanczos,
            1e-8,
            &format!("BT2 dense vs Lanczos at {label} (σ = {sigma})"),
        );
    }
}

// ---------------------------------------------------------------------------
// BT5 — C6 singular shift on the Lanczos path
// ---------------------------------------------------------------------------

/// **BT5, Lanczos.** A σ sitting exactly ON an eigenvalue is a TYPED failure
/// carrying σ — not a panic, not `Err(KNotSpd)`, and above all not a
/// finite-looking spectrum.
///
/// α's BT5 covers only the dense path and says so: its fixture A is 5×5 and
/// routes to the dense fallback, where no `K − σB` is ever formed and σ on an
/// eigenvalue is simply a selection key. The Lanczos arm needs an n>64 fixture,
/// so this uses fixture C, whose closed-form spectrum supplies an eigenvalue to
/// sit exactly on.
///
/// # Both directions, because either alone is passed by a broken guard
///
/// (a) alone is passed by a guard that fires at every σ≠0; (b) alone by one that
/// never fires. Together they pin a guard that discriminates.
///
/// # Why part (b) of the PRD §5.3 detection is not optional
///
/// faer's `LuError::SymbolicSingular` reports STRUCTURAL rank deficiency only —
/// no pivot exists anywhere in the pattern. It cannot fire on this fixture:
/// `K − σB` keeps a full diagonal for every finite σ, so the symbolic structure
/// is full-rank however close σ gets to an eigenvalue. Partial-pivot LU on a
/// numerically tiny pivot returns `Ok`, and part A alone would let σ-on-an-
/// eigenvalue through as plausible numbers. Hence the post-factorization guard.
///
/// # No threshold constant is pinned here
///
/// The assertions are behavioural — typed failure / no typed failure — and the
/// threshold is DERIVED in the implementation from the pencil's own scale. It
/// is not restated here, because a test that hardcoded it would have to be
/// re-baselined every time the derivation was corrected, which is exactly the
/// pressure that turns a guard into a rubber stamp. Achievability was checked
/// and clears by ~11 orders in both directions: this pencil's λ-space resolution
/// floor is n·ε·‖K−σB‖_∞/‖B‖_∞ ≈ 80 · 2.22e-16 · 3.99 ≈ 7.1e-14, the
/// σ-on-eigenvalue case collapses to |λ−σ| ~ 1e-16, and the healthy case sits at
/// |λ−σ| ≈ 1.4e-2.
#[test]
fn lanczos_reports_a_singular_shift_as_a_typed_failure() {
    let (k, b) = fixture_c();
    let base = EigenSolverOptions {
        n_modes: 2,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    };

    // (a) σ exactly on λ₃, computed in f64 so it really is the same value the
    // pencil has.
    let sigma_on_eigenvalue = fixture_c_lambda(3);
    let got = try_solve_eigen_shift_invert(
        &k,
        &b,
        EigenSolverOptions {
            sigma: sigma_on_eigenvalue,
            ..base.clone()
        },
    );
    match got {
        Err(ShiftInvertFailure::ShiftAtEigenvalue { sigma }) => {
            // Carrying σ is what makes this a TYPED failure rather than a bare
            // error: a caller can name the offending shift without re-deriving
            // it from its own options.
            assert_eq!(
                sigma, sigma_on_eigenvalue,
                "BT5(a) Lanczos: the failure must carry the σ that was passed \
                 ({sigma_on_eigenvalue}), not {sigma}",
            );
        }
        Err(ShiftInvertFailure::KNotSpd) => panic!(
            "BT5(a) Lanczos: σ = {sigma_on_eigenvalue} sits on λ₃ of an SPD K — this is a \
             singular SHIFT, and reporting it as a non-SPD K would send an author to check \
             boundary conditions for a problem that is entirely about where σ was put",
        ),
        Ok(result) => panic!(
            "BT5(a) Lanczos: σ = {sigma_on_eigenvalue} sits exactly on λ₃, so K − σB is \
             singular and there is no shift-invert operator to apply; got a finite-looking \
             spectrum {:?} instead of a typed failure. A plausible answer to an unanswerable \
             question is the silent-substitution class C6 exists to close.",
            result.eigenvalues,
        ),
    }

    // (b) the healthy σ from BT3 on the SAME fixture still succeeds, so the
    // guard cannot be one that fires unconditionally at every σ≠0.
    let healthy = sigma_between_lambda_9_and_10();
    let ok = try_solve_eigen_shift_invert(
        &k,
        &b,
        EigenSolverOptions {
            sigma: healthy,
            ..base
        },
    );
    match ok {
        Ok(result) => {
            assert!(
                result.n_converged > 0,
                "BT5(b) Lanczos must exercise Lanczos (n_converged > 0); got 0",
            );
        }
        Err(e) => panic!(
            "BT5(b) Lanczos: σ = {healthy} is nowhere near an eigenvalue of this pencil \
             (the nearest is ~1.4e-2 away, eleven orders above the resolution floor), so \
             it must solve; got {e:?} — the guard fires unconditionally and has made every \
             shifted solve unreachable",
        ),
    }
}
