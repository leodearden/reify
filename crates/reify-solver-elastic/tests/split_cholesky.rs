//! Behaviour tests for [`SplitCholesky`]: a sparse Cholesky factor `A = G·Gᵀ`
//! that applies `G⁻¹` and `G⁻ᵀ` SEPARATELY.
//!
//! faer's own `Llt` exposes only the composed solve `A⁻¹ = G⁻ᵀG⁻¹`; a
//! Cholesky-symmetrized Lanczos needs the two halves on either side of an
//! operator. These tests pin the two halves by what they must compose to,
//! never by the factor's internal layout:
//!
//! - `G⁻¹·A·G⁻ᵀ = I` (the symmetric reduction the Lanczos relies on), and
//! - `G⁻ᵀ·G⁻¹ = A⁻¹` (agreement with faer's own composed solve),
//!
//! each on a fixture that faer factors SIMPLICIALLY and one it factors
//! SUPERNODALLY. Which arm a fixture takes is asserted through faer's public
//! symbolic API, so neither fixture can silently drift onto the other arm.

use faer::linalg::solvers::SolveCore;
use faer::sparse::linalg::LltError;
use faer::sparse::linalg::cholesky::{SymbolicCholeskyRaw, factorize_symbolic_cholesky};
use faer::sparse::{SparseRowMat, Triplet};
use faer::{Conj, Mat, Side};
use reify_solver_elastic::split_cholesky::SplitCholesky;

/// Relative agreement required between the two sides of every identity here.
/// Measured 3.8e-16 to 5.8e-16 on both factor arms.
const REL_TOL: f64 = 1e-12;

/// `tridiag(−1, 2, −1)`, `n`×`n` — small enough that faer factors it
/// SIMPLICIALLY.
fn tridiagonal(n: usize) -> SparseRowMat<usize, f64> {
    let mut trips = Vec::with_capacity(3 * n - 2);
    for i in 0..n {
        trips.push(Triplet::new(i, i, 2.0));
        if i > 0 {
            trips.push(Triplet::new(i, i - 1, -1.0));
        }
        if i + 1 < n {
            trips.push(Triplet::new(i, i + 1, -1.0));
        }
    }
    SparseRowMat::try_new_from_triplets(n, n, &trips).unwrap()
}

/// A banded SPD matrix dense enough per column that faer factors it
/// SUPERNODALLY: `n = 300`, bandwidth `w = 100`, diagonal `4·w`, and
/// off-diagonal `−1/d` at distance `d = |i − j| ≤ w` (strictly diagonally
/// dominant, hence SPD).
fn supernodal_banded() -> SparseRowMat<usize, f64> {
    let n: usize = 300;
    let w: usize = 100;
    let mut trips = Vec::new();
    for i in 0..n {
        for j in i.saturating_sub(w)..(i + w + 1).min(n) {
            let value = if i == j {
                4.0 * w as f64
            } else {
                -1.0 / i.abs_diff(j) as f64
            };
            trips.push(Triplet::new(i, j, value));
        }
    }
    SparseRowMat::try_new_from_triplets(n, n, &trips).unwrap()
}

/// Whether faer's PUBLIC symbolic analysis picks the supernodal arm for `a`.
fn faer_factors_supernodally(a: &SparseRowMat<usize, f64>) -> bool {
    let a_csc = a.to_col_major().unwrap();
    let symbolic = factorize_symbolic_cholesky(
        a_csc.as_ref().symbolic(),
        Side::Lower,
        Default::default(),
        Default::default(),
    )
    .unwrap();
    matches!(symbolic.raw(), SymbolicCholeskyRaw::Supernodal(_))
}

/// A deterministic two-column right-hand side with no structure a triangular
/// solve could accidentally respect.
fn two_column_rhs(n: usize) -> Mat<f64> {
    Mat::from_fn(n, 2, |i, j| match j {
        0 => ((i * 7919) % 101) as f64 / 101.0 - 0.5,
        _ => ((i * 104_729 + 13) % 97) as f64 / 97.0 - 0.5,
    })
}

/// `A·X` for a CSR `A`, column by column, by walking stored entries — an
/// independent matvec, not faer's.
fn csr_times(a: &SparseRowMat<usize, f64>, x: &Mat<f64>) -> Mat<f64> {
    let a_ref = a.as_ref();
    let symbolic = a_ref.symbolic();
    Mat::from_fn(a.nrows(), x.ncols(), |i, j| {
        symbolic
            .col_idx_of_row_raw(i)
            .iter()
            .zip(a_ref.val_of_row(i))
            .map(|(&col, &value)| value * x[(col, j)])
            .sum()
    })
}

fn relative_difference(got: &Mat<f64>, want: &Mat<f64>) -> f64 {
    (got - want).norm_l2() / want.norm_l2()
}

/// (a) `G⁻¹·A·G⁻ᵀ·X = X`.
fn assert_symmetric_reduction_is_identity(a: &SparseRowMat<usize, f64>, label: &str) {
    let g = SplitCholesky::try_new(a).expect("fixture is SPD");
    let x = two_column_rhs(a.nrows());

    let mut y = x.clone();
    g.solve_factor_transpose_in_place(y.as_mut());
    let mut z = csr_times(a, &y);
    g.solve_factor_in_place(z.as_mut());

    let rel = relative_difference(&z, &x);
    assert!(
        rel <= REL_TOL,
        "{label}: ‖G⁻¹·A·G⁻ᵀ·X − X‖/‖X‖ = {rel:.3e} > {REL_TOL:.0e}",
    );
}

/// (b) `G⁻ᵀ·(G⁻¹·R)` equals faer's own `A⁻¹·R`.
fn assert_halves_compose_to_the_inverse(a: &SparseRowMat<usize, f64>, label: &str) {
    let g = SplitCholesky::try_new(a).expect("fixture is SPD");
    let r = two_column_rhs(a.nrows());

    let mut halves = r.clone();
    g.solve_factor_in_place(halves.as_mut());
    g.solve_factor_transpose_in_place(halves.as_mut());

    let llt = a.sp_cholesky(Side::Lower).expect("fixture is SPD");
    let mut composed = r.clone();
    SolveCore::<f64>::solve_in_place_with_conj(&llt, Conj::No, composed.as_mut());

    let rel = relative_difference(&halves, &composed);
    assert!(
        rel <= REL_TOL,
        "{label}: ‖G⁻ᵀG⁻¹R − A⁻¹R‖/‖A⁻¹R‖ = {rel:.3e} > {REL_TOL:.0e}",
    );
}

#[test]
fn simplicial_fixture_takes_the_simplicial_arm() {
    assert!(
        !faer_factors_supernodally(&tridiagonal(80)),
        "the tridiagonal fixture must pin faer's SIMPLICIAL arm",
    );
}

#[test]
fn supernodal_fixture_takes_the_supernodal_arm() {
    assert!(
        faer_factors_supernodally(&supernodal_banded()),
        "the banded fixture must pin faer's SUPERNODAL arm",
    );
}

#[test]
fn simplicial_symmetric_reduction_is_the_identity() {
    assert_symmetric_reduction_is_identity(&tridiagonal(80), "simplicial");
}

#[test]
fn simplicial_halves_compose_to_the_inverse() {
    assert_halves_compose_to_the_inverse(&tridiagonal(80), "simplicial");
}

#[test]
fn supernodal_symmetric_reduction_is_the_identity() {
    assert_symmetric_reduction_is_identity(&supernodal_banded(), "supernodal");
}

#[test]
fn supernodal_halves_compose_to_the_inverse() {
    assert_halves_compose_to_the_inverse(&supernodal_banded(), "supernodal");
}

/// (d) An indefinite matrix is a NUMERIC failure — the same discrimination
/// `sp_cholesky` makes — never a panic and never a resource error.
#[test]
fn indefinite_matrix_is_a_numeric_failure() {
    let n = 10;
    let mut trips = Vec::new();
    for i in 0..n {
        trips.push(Triplet::new(i, i, if i % 2 == 0 { 1.0 } else { -1.0 }));
        if i > 0 {
            trips.push(Triplet::new(i, i - 1, 0.1));
        }
        if i + 1 < n {
            trips.push(Triplet::new(i, i + 1, 0.1));
        }
    }
    let a = SparseRowMat::try_new_from_triplets(n, n, &trips).unwrap();
    match SplitCholesky::try_new(&a) {
        Err(LltError::Numeric(_)) => {}
        Err(other) => panic!("expected LltError::Numeric, got {other:?}"),
        Ok(_) => panic!("an indefinite matrix must not factor"),
    }
}

/// (e) `n()` is the matrix dimension.
#[test]
fn dimension_is_the_matrix_dimension() {
    assert_eq!(SplitCholesky::try_new(&tridiagonal(80)).unwrap().n(), 80);
    assert_eq!(
        SplitCholesky::try_new(&supernodal_banded()).unwrap().n(),
        300
    );
}
