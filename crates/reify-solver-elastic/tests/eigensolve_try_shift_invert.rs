//! Synthetic behavioural tests for [`try_solve_eigen_shift_invert`] — the
//! non-panicking sibling of `solve_eigen_shift_invert` (task 6663).
//!
//! `eigensolve_synthetic.rs` covers the panicking entry point's routing and
//! numerics on Fixtures A/B/C. This file pins the `try_` variant's own TWO
//! contract clauses, which are otherwise exercised only indirectly, from
//! `reify-eval`'s `modal_ops` in another crate:
//!
//!   1. **`None` means EXACTLY "`K` is not SPD".** A stiffness matrix carrying a
//!      free (zero-stiffness) DOF — the shape an under-constrained modal model
//!      assembles — returns `None` instead of panicking in `sp_cholesky`.
//!   2. **`Some` is bit-identical to `solve_eigen_shift_invert`.** On SPD `K` the
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
use reify_solver_elastic::eigensolve::{
    EigenSolverOptions, solve_eigen_shift_invert, try_solve_eigen_shift_invert,
};

const N: usize = 80;

/// `K = tridiag(-1, 2, -1)` (N×N Dirichlet Laplacian) — symmetric positive
/// definite, so `sp_cholesky` succeeds.
fn spd_laplacian() -> SparseRowMat<usize, f64> {
    let mut trips = Vec::with_capacity(3 * N - 2);
    for i in 0..N {
        trips.push(Triplet::new(i, i, 2.0));
        if i > 0 {
            trips.push(Triplet::new(i, i - 1, -1.0));
        }
        if i + 1 < N {
            trips.push(Triplet::new(i, i + 1, -1.0));
        }
    }
    SparseRowMat::try_new_from_triplets(N, N, &trips).unwrap()
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
    let trips: Vec<Triplet<usize, usize, f64>> =
        (0..N).map(|i| Triplet::new(i, i, 1.0)).collect();
    SparseRowMat::try_new_from_triplets(N, N, &trips).unwrap()
}

fn opts() -> EigenSolverOptions {
    EigenSolverOptions {
        n_modes: 5,
        tol: 1e-10,
        max_iters: 1000,
        sigma: 0.0,
    }
}

/// Closed-form smallest 5 eigenvalues of the N-DOF Dirichlet Laplacian:
/// `λ_k = 2(1 − cos(kπ/(N+1)))`.
fn expected_5() -> [f64; 5] {
    std::array::from_fn(|i| {
        let k = (i + 1) as f64;
        2.0 * (1.0 - f64::cos(k * std::f64::consts::PI / (N as f64 + 1.0)))
    })
}

// ---------------------------------------------------------------------------
// Clause 1: a non-SPD K returns None instead of panicking.
// ---------------------------------------------------------------------------

/// A singular `K` (one zero-stiffness DOF) must return `None`, not panic.
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

    assert!(
        try_solve_eigen_shift_invert(&k, &b, opts()).is_none(),
        "a K with a zero-stiffness DOF is not SPD; try_ must report that as None",
    );
}

/// The panicking sibling really does panic on the same input — so `None` above
/// is a genuine behavioural difference, not a fixture that both entry points
/// happen to tolerate.
#[test]
#[should_panic(expected = "K must be SPD")]
fn shift_invert_still_panics_on_the_same_singular_k() {
    let k = singular_laplacian(N / 2);
    let b = identity();
    let _ = solve_eigen_shift_invert(&k, &b, opts());
}

// ---------------------------------------------------------------------------
// Clause 2: on SPD K the healthy path is unchanged.
// ---------------------------------------------------------------------------

/// On an SPD `K` the `try_` variant returns `Some` whose spectrum is identical,
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
        .expect("the Dirichlet Laplacian is SPD; try_ must return Some");
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
