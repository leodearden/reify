//! Synthetic behavioural tests for [`try_solve_eigen_shift_invert`] — the
//! non-panicking sibling of `solve_eigen_shift_invert` (task 6663).
//!
//! `eigensolve_synthetic.rs` covers the panicking entry point's routing and
//! numerics on Fixtures A/B/C. This file pins the `try_` variant's own TWO
//! contract clauses, which are otherwise exercised only indirectly, from
//! `reify-eval`'s `modal_ops` in another crate:
//!
//!   1. **`Err(KNotSpd)` means EXACTLY "`K` is not SPD".** A stiffness matrix carrying a
//!      free (zero-stiffness) DOF — the shape an under-constrained modal model
//!      assembles — returns `Err(KNotSpd)` instead of panicking in `sp_cholesky`.
//!      "EXACTLY" is enforced by matching faer's error rather than `.ok()?`-ing
//!      it: only `LltError::Numeric` (non-positive pivot) becomes `Err(KNotSpd)`, while
//!      `LltError::Generic` (`OutOfMemory` / `IndexOverflow`) still panics, so a
//!      resource failure on a large mesh cannot be reported to the user as
//!      `W_ModalRigidBodyMode: K_free is singular`. The `Generic` arm is NOT
//!      pinned by a test here: faer offers no hook to inject an allocation or
//!      index-overflow failure, and provoking a real OOM in a merge-gate test is
//!      not a trade worth making. The arm is a one-line `panic!` immediately
//!      beside the `Err(KNotSpd)` it is distinguished from.
//!   2. **`Ok` is bit-identical to `solve_eigen_shift_invert`.** On SPD `K` the
//!      two entry points stay numerically interchangeable, so the healthy path
//!      returns the same numbers and callers can swap one for the other freely.
//!
//! The implementation also factors `K` exactly once and reuses that
//! factorization, rather than probing with a throwaway `sp_cholesky` and then
//! calling the panicking entry point. That property is deliberately NOT claimed
//! as a pinned clause here: `sp_cholesky` is deterministic, so the probe-then-
//! refactor shape would return bit-identical numbers and pass clause 2
//! unchanged. Pinning it would take a counting/instrumented factorization; this
//! file pins the observable contract only.
//!
//! # Fixture
//!
//! An 80-DOF 1-D Dirichlet Laplacian pair (`K = tridiag(-1, 2, -1)`, `B = I`),
//! matching `eigensolve_synthetic.rs`'s Fixture C. n = 80 > 64 = 2·faer MIN_DIM,
//! so the Krylov window fits and the REAL Lanczos path runs — the dense-fallback
//! branch is not what is being measured here.
//! Closed form: `λ_k = 2(1 − cos(kπ/81))`, k = 1..80.

use faer::sparse::{SparseRowMat, Triplet};
use reify_solver_elastic::eigensolve::test_support::{
    identity as identity_of, laplacian_lambda, laplacian_lambdas, laplacian_pencil,
};
use reify_solver_elastic::eigensolve::{
    EigenSolverOptions, EigenSolverResult, ShiftInvertFailure, solve_eigen_shift_invert,
    try_solve_eigen_shift_invert,
};

const N: usize = 80;

/// `K = tridiag(-1, 2, -1)` (N×N Dirichlet Laplacian) — symmetric positive
/// definite, so `sp_cholesky` succeeds. From the crate's shared test-support
/// seam, which owns the pencil and its closed form.
fn spd_laplacian() -> SparseRowMat<usize, f64> {
    laplacian_pencil(N).0
}

/// The same Laplacian with row/column `free` zeroed out: DOF `free` carries no
/// stiffness at all.
///
/// This is exactly the shape an under-constrained assembled FE system has — a
/// DOF that no element and no Dirichlet BC restrains — and it makes `K`
/// EXACTLY singular (the `free`-th unit vector spans a null direction), so the
/// Cholesky pivot there is exactly `0.0` rather than a rounding-dependent small
/// positive number. Deterministic across platforms, which a semi-definite
/// free-free (Neumann) Laplacian would not be.
fn singular_laplacian(free: usize) -> SparseRowMat<usize, f64> {
    let mut trips = Vec::with_capacity(3 * N - 2);
    for i in 0..N {
        if i == free {
            // Explicit structural zero on the diagonal: keeps every row present
            // (no empty row for the symbolic factorization to special-case)
            // while leaving the pivot at exactly 0.0.
            trips.push(Triplet::new(i, i, 0.0));
            continue;
        }
        trips.push(Triplet::new(i, i, 2.0));
        if i > 0 && i - 1 != free {
            trips.push(Triplet::new(i, i - 1, -1.0));
        }
        if i + 1 < N && i + 1 != free {
            trips.push(Triplet::new(i, i + 1, -1.0));
        }
    }
    SparseRowMat::try_new_from_triplets(N, N, &trips).unwrap()
}

/// `B = I` (N×N).
fn identity() -> SparseRowMat<usize, f64> {
    identity_of(N)
}

fn opts() -> EigenSolverOptions {
    EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    }
}

/// Closed-form smallest 5 eigenvalues of the N-DOF Dirichlet Laplacian.
fn expected_5() -> [f64; 5] {
    laplacian_lambdas(N)
}

// ---------------------------------------------------------------------------
// Clause 1: a non-SPD K returns Err(KNotSpd) instead of panicking.
// ---------------------------------------------------------------------------

/// A singular `K` (one zero-stiffness DOF) must return `Err(KNotSpd)`, not panic.
///
/// This is the `LltError::Numeric` (non-positive pivot) arm — the ONLY one that
/// maps to `Err(KNotSpd)`; see this file's module doc for why the `Generic` arm
/// is unpinned.
///
/// `solve_eigen_shift_invert` on this same pair panics — that is its documented
/// contract ("K is not SPD → panic with descriptive message"), and it is what
/// forced task 6663's `try_` sibling into existence: `modal_ops` can legitimately
/// be handed a rigid-body-bearing `K_free` by an under-constrained user model,
/// where a panic is a crash rather than a diagnostic.
#[test]
fn try_shift_invert_returns_none_on_singular_k() {
    let k = singular_laplacian(N / 2);
    let b = identity();

    let got = try_solve_eigen_shift_invert(&k, &b, opts());
    assert!(
        matches!(got, Err(ShiftInvertFailure::KNotSpd)),
        "a K with a zero-stiffness DOF is not SPD; try_ must report that as \
         Err(KNotSpd), not as any other arm of the failure channel",
    );
}

/// The panicking sibling really does panic on the same input — so `Err(KNotSpd)`
/// above is a genuine behavioural difference, not a fixture that both entry
/// points happen to tolerate.
#[test]
#[should_panic(expected = "K must be SPD")]
fn shift_invert_still_panics_on_the_same_singular_k() {
    let k = singular_laplacian(N / 2);
    let b = identity();
    let _ = solve_eigen_shift_invert(&k, &b, opts());
}

// ---------------------------------------------------------------------------
// Clause 1b: the two domain failures are DISTINCT and non-interchangeable —
// as typed values, and on the panicking sibling as messages. The rationale is
// stated once, on `ShiftInvertFailure::ShiftAtEigenvalue`.
// ---------------------------------------------------------------------------

/// `sigma` placed exactly on an eigenvalue of the SPD Laplacian. k=3 is well
/// inside the spectrum, far from both ends.
fn sigma_on_an_eigenvalue() -> f64 {
    laplacian_lambda(N, 3)
}

/// **The control.** The surviving contract: a non-SPD `K` at sigma=0 still
/// panics with "K must be SPD".
///
/// Already pinned by `shift_invert_still_panics_on_the_same_singular_k` above;
/// re-asserted here as the control for the negative half of the next test, so
/// the pair reads as one claim rather than two unrelated ones.
#[test]
#[should_panic(expected = "K must be SPD")]
fn panicking_sibling_names_a_non_spd_k() {
    let k = singular_laplacian(N / 2);
    let b = identity();
    let _ = solve_eigen_shift_invert(&k, &b, opts());
}

/// A singular SHIFT panics with a message that names the shift and the
/// offending sigma.
///
/// The value is checked by catching the panic rather than by
/// `#[should_panic(expected = ...)]`, because the load-bearing half of this
/// claim is NEGATIVE — the message must NOT contain "K must be SPD" — and
/// `should_panic` can only assert a substring is present, never that one is
/// absent.
#[test]
fn panicking_sibling_names_the_shift_not_the_stiffness() {
    let k = spd_laplacian();
    let b = identity();
    let sigma = sigma_on_an_eigenvalue();

    let outcome = std::panic::catch_unwind(|| {
        // Discarded rather than returned: `EigenSolverResult` is not `Debug`,
        // and what this test needs from the Ok side is only that it did not
        // happen.
        let _ = solve_eigen_shift_invert(
            &k,
            &b,
            EigenSolverOptions {
                sigma,
                ..opts()
            },
        );
    });
    let panicked = outcome.expect_err(
        "sigma sits exactly on an eigenvalue, so K - sigma*B is singular and the panicking \
         entry point must panic",
    );

    let message = panicked
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panicked.downcast_ref::<&str>().copied())
        .unwrap_or("<non-string panic payload>")
        .to_string();

    let lower = message.to_lowercase();

    // The NEGATIVE half, and the point of the whole test.
    assert!(
        !message.contains("K must be SPD"),
        "a singular SHIFT must not be reported with the non-SPD wording — wrong fault, \
         wrong remedy (see ShiftInvertFailure::ShiftAtEigenvalue). Got: {message}",
    );
    // Not a placeholder, either: an "internal error" tells the author nothing
    // they can act on and is indistinguishable from a bug in the solver.
    assert!(
        !lower.contains("internal error"),
        "a sigma landing on an eigenvalue is a DOMAIN fault with a remedy the author can \
         apply, not an internal error. Got: {message}",
    );

    // The POSITIVE half: diagnosis, fault domain, and the offending value.
    assert!(
        lower.contains("singular"),
        "the panic must say that K - sigma*B is SINGULAR — that is the diagnosis the \
         remedy follows from. Got: {message}",
    );
    assert!(
        lower.contains("shift"),
        "the panic must name the SHIFT as the fault. Got: {message}",
    );
    assert!(
        message.contains(&format!("{sigma}")),
        "the panic must name the offending sigma ({sigma}), following the \
         named-offending-value convention the rest of the module uses. Got: {message}",
    );
}

/// At the `try_` level the two failures are distinguishable as VALUES, on the
/// same two fixtures — so the widened channel is proved to carry the
/// distinction rather than merely to have room for it.
#[test]
fn try_level_distinguishes_a_non_spd_k_from_a_singular_shift() {
    let b = identity();

    // `EigenSolverResult` is not `Debug`, so each outcome is reduced to its
    // failure — which is the only part these assertions are about.
    let failure_of = |r: Result<EigenSolverResult, ShiftInvertFailure>| r.err();

    let not_spd = failure_of(try_solve_eigen_shift_invert(
        &singular_laplacian(N / 2),
        &b,
        opts(),
    ));
    assert_eq!(
        not_spd,
        Some(ShiftInvertFailure::KNotSpd),
        "a K with a zero-stiffness DOF must report Err(KNotSpd)",
    );

    let sigma = sigma_on_an_eigenvalue();
    let singular_shift = failure_of(try_solve_eigen_shift_invert(
        &spd_laplacian(),
        &b,
        EigenSolverOptions {
            sigma,
            ..opts()
        },
    ));
    assert_eq!(
        singular_shift,
        Some(ShiftInvertFailure::ShiftAtEigenvalue { sigma }),
        "an SPD K with sigma on an eigenvalue must report ShiftAtEigenvalue carrying that \
         sigma — a DIFFERENT value from KNotSpd above, on fixtures that differ only in \
         which fault they carry",
    );
    assert_ne!(
        not_spd, singular_shift,
        "the two domain failures must be distinguishable as values, not merely have room \
         in the channel to be",
    );
}

// ---------------------------------------------------------------------------
// Clause 2: on SPD K the healthy path is unchanged.
// ---------------------------------------------------------------------------

/// On an SPD `K` the `try_` variant returns `Ok` whose spectrum is identical,
/// element-for-element, to `solve_eigen_shift_invert`'s — and both match the
/// closed form.
///
/// Exact (`==`) equality is asserted deliberately, but note what it does and does
/// NOT pin. It DOES pin that the two entry points stay numerically interchangeable
/// on SPD K: any change that makes `try_` return a merely-close spectrum (a
/// different shift, a different tolerance, a re-ordered assembly) reds here rather
/// than drifting silently past an epsilon. It does NOT pin the "factors K exactly
/// once" property — `sp_cholesky` is deterministic, so an implementation that
/// probed with one factorization and then re-factored would produce bit-identical
/// output and pass unchanged. Detecting that would need an instrumented or
/// counting factorization, not an output comparison.
///
/// The closed-form check keeps the test meaningful (rather than tautological) if
/// the delegation is ever restructured.
#[test]
fn try_shift_invert_matches_the_panicking_entry_point_on_spd_k() {
    let k = spd_laplacian();
    let b = identity();

    let tried = try_solve_eigen_shift_invert(&k, &b, opts())
        .expect("the Dirichlet Laplacian is SPD; try_ must return Ok");
    let direct = solve_eigen_shift_invert(&k, &b, opts());

    assert_eq!(
        tried.eigenvalues.len(),
        direct.eigenvalues.len(),
        "try_ and the panicking entry point must return the same mode count",
    );
    for (i, (&got, &exp)) in tried
        .eigenvalues
        .iter()
        .zip(direct.eigenvalues.iter())
        .enumerate()
    {
        assert_eq!(
            got, exp,
            "eigenvalue[{i}]: try_ returned {got:.17e}, solve_ returned {exp:.17e} \
             — on SPD K the two entry points must stay numerically interchangeable",
        );
    }
    assert_eq!(
        tried.converged, direct.converged,
        "converged flag must match between try_ and the panicking entry point",
    );
    assert_eq!(
        tried.n_converged, direct.n_converged,
        "n_converged must match between try_ and the panicking entry point",
    );

    // Lanczos really ran (n = 80 > 64), so this is not the dense-fallback branch.
    assert!(
        tried.converged,
        "shift-invert Lanczos must converge on the {N}-DOF Laplacian at n_modes=5",
    );

    for (i, (&got, &exp)) in tried.eigenvalues.iter().zip(expected_5().iter()).enumerate() {
        assert!(
            (got - exp).abs() < 1e-8,
            "eigenvalue[{i}]: got {got:.15}, closed form {exp:.15}, diff = {:.3e}",
            (got - exp).abs(),
        );
    }
}
