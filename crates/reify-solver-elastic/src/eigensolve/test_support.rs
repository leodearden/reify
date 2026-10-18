//! Fixtures shared by the eigensolve module's own tests, the crate's
//! integration tests, and `reify-eval`'s in-crate modal tests.
//!
//! `#[doc(hidden)] pub` rather than `#[cfg(test)]` for the same reason
//! [`crate::assembly::test_support`] is: an integration test compiles against
//! the built library, so a `#[cfg(test)]` item is invisible to it and every
//! consumer ends up with its own copy. The closed form in particular was
//! maintained in four places before this seam existed.

use faer::sparse::{SparseRowMat, Triplet};

/// The `n`-DOF 1-D Dirichlet Laplacian pencil: `K = tridiag(−1, 2, −1)`
/// (symmetric positive definite) with `B = I`.
///
/// Its spectrum is closed-form ([`laplacian_lambda`]), which is what lets a
/// test place σ EXACTLY on an eigenvalue in f64 rather than near one to
/// within whatever tolerance some prior solve happened to reach.
///
/// `n > 64` is the threshold above which [`super::routes_to_dense_fallback`]
/// is false and the real Lanczos path runs.
pub fn laplacian_pencil(n: usize) -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
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
    (
        SparseRowMat::try_new_from_triplets(n, n, &k_trips).unwrap(),
        identity(n),
    )
}

/// `I` (`n`×`n`), as a sparse row matrix.
pub fn identity(n: usize) -> SparseRowMat<usize, f64> {
    let trips: Vec<Triplet<usize, usize, f64>> = (0..n).map(|i| Triplet::new(i, i, 1.0)).collect();
    SparseRowMat::try_new_from_triplets(n, n, &trips).unwrap()
}

/// Closed-form `λ_k = 2(1 − cos(kπ/(n+1)))` of [`laplacian_pencil`],
/// 1-INDEXED (λ₁ is the first mode, not λ₀), matching the formula.
pub fn laplacian_lambda(n: usize, k: usize) -> f64 {
    assert!(
        (1..=n).contains(&k),
        "an n = {n} pencil has modes k = 1..={n}, not {k}",
    );
    2.0 * (1.0 - f64::cos(k as f64 * std::f64::consts::PI / (n as f64 + 1.0)))
}

/// The `m` smallest closed-form eigenvalues of [`laplacian_pencil`],
/// ascending.
pub fn laplacian_lambdas<const M: usize>(n: usize) -> [f64; M] {
    std::array::from_fn(|i| laplacian_lambda(n, i + 1))
}

/// An SPD pencil whose `B` is NOT a multiple of the identity:
/// `K = tridiag(−1, 2, −1)` with `B = diag(1 + i/n)` for `i = 0..n`.
///
/// The smallest stand-in for a modal pencil with a non-uniform mass. It is
/// the case where `(K − σB)⁻¹B` is not Euclidean-symmetric, so a Lanczos that
/// assumes it is returns pairs that are not eigenpairs. No closed form:
/// `solve_eigen_dense` is the reference.
pub fn graded_diagonal_b_pencil(n: usize) -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    let trips: Vec<Triplet<usize, usize, f64>> = (0..n)
        .map(|i| Triplet::new(i, i, 1.0 + i as f64 / n as f64))
        .collect();
    (
        laplacian_pencil(n).0,
        SparseRowMat::try_new_from_triplets(n, n, &trips).unwrap(),
    )
}

/// A pencil with SPD `K` and symmetric INDEFINITE `B`:
/// `K = tridiag(−1, 2, −1)`, and `B` tridiagonal with diagonal `+1.0` for
/// `i < n/2`, `−0.5` for `i ≥ n/2`, and off-diagonals `0.2`.
///
/// The stand-in for buckling's `B = −K_g`, whose spectrum has both signs.
/// No closed form: `solve_eigen_dense` is the reference.
pub fn indefinite_b_pencil(n: usize) -> (SparseRowMat<usize, f64>, SparseRowMat<usize, f64>) {
    let mut b_trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        let diagonal = if i < n / 2 { 1.0 } else { -0.5 };
        b_trips.push(Triplet::new(i, i, diagonal));
        if i > 0 {
            b_trips.push(Triplet::new(i, i - 1, 0.2));
        }
        if i + 1 < n {
            b_trips.push(Triplet::new(i, i + 1, 0.2));
        }
    }
    (
        laplacian_pencil(n).0,
        SparseRowMat::try_new_from_triplets(n, n, &b_trips).unwrap(),
    )
}
