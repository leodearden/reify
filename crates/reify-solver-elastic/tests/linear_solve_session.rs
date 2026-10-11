//! The `linear_solve` seam's Cholesky driver, observed through the
//! feature-gated fault probe (PRD
//! `docs/prds/v0_6/elastic-static-solver-performance.md` §5 C2).
//!
//! `SplitCholesky` runs on the seam's one driver: every factorisation it makes
//! is visible to an installed `FaultProbe` — one symbolic and one numeric call,
//! and exactly the two fallible reservations, in order — and the probe can
//! force faer's symbolic variant or fail a reservation.

use faer::linalg::solvers::SolveCore;
use faer::sparse::linalg::LltError;
use faer::sparse::linalg::cholesky::{
    SymbolicCholesky, SymbolicCholeskyRaw, factorize_symbolic_cholesky,
};
use faer::sparse::{FaerError, SparseRowMat, Triplet};
use faer::{Conj, Mat, Par, Side};
use reify_solver_elastic::linear_solve::fault_injection::{
    FaultProbe, Reservation, ReservationSite, SymbolicVariant,
};
use reify_solver_elastic::split_cholesky::SplitCholesky;

// ---------------------------------------------------------------------------
// Fixtures and checks (copied from tests/split_cholesky.rs per the
// per-test-file helper convention).
// ---------------------------------------------------------------------------

/// Relative agreement required between `G⁻ᵀG⁻¹R` and faer's `A⁻¹R`.
const REL_TOL: f64 = 1e-12;

/// `tridiag(−1, 2, −1)`, `n`×`n` — faer factors it SIMPLICIALLY.
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

/// `n = 300`, bandwidth `w = 100`, diagonal `4·w`, off-diagonal `−1/d` at
/// distance `d ≤ w` — faer factors it SUPERNODALLY.
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

/// faer's PUBLIC symbolic Cholesky analysis of `a`, with faer's defaults.
fn faer_symbolic_cholesky(a: &SparseRowMat<usize, f64>) -> SymbolicCholesky<usize> {
    let a_csc = a.to_col_major().unwrap();
    factorize_symbolic_cholesky(
        a_csc.as_ref().symbolic(),
        Side::Lower,
        Default::default(),
        Default::default(),
    )
    .unwrap()
}

fn faer_variant(a: &SparseRowMat<usize, f64>) -> SymbolicVariant {
    match faer_symbolic_cholesky(a).raw() {
        SymbolicCholeskyRaw::Simplicial(_) => SymbolicVariant::Simplicial,
        SymbolicCholeskyRaw::Supernodal(_) => SymbolicVariant::Supernodal,
    }
}

fn two_column_rhs(n: usize) -> Mat<f64> {
    Mat::from_fn(n, 2, |i, j| match j {
        0 => ((i * 7919) % 101) as f64 / 101.0 - 0.5,
        _ => ((i * 104_729 + 13) % 97) as f64 / 97.0 - 0.5,
    })
}

/// `G⁻ᵀ·(G⁻¹·R)` equals faer's own `A⁻¹·R`.
fn assert_halves_compose_to_the_inverse(
    g: &SplitCholesky,
    a: &SparseRowMat<usize, f64>,
    label: &str,
) {
    let r = two_column_rhs(a.nrows());
    let mut halves = r.clone();
    g.solve_factor_in_place(halves.as_mut());
    g.solve_factor_transpose_in_place(halves.as_mut());

    let llt = a.sp_cholesky(Side::Lower).expect("fixture is SPD");
    let mut composed = r.clone();
    SolveCore::<f64>::solve_in_place_with_conj(&llt, Conj::No, composed.as_mut());

    let rel = (&halves - &composed).norm_l2() / composed.norm_l2();
    assert!(
        rel <= REL_TOL,
        "{label}: ‖G⁻ᵀG⁻¹R − A⁻¹R‖/‖A⁻¹R‖ = {rel:.3e} > {REL_TOL:.0e}",
    );
}

fn assert_out_of_memory(result: Result<SplitCholesky, LltError>) {
    match result {
        Err(LltError::Generic(FaerError::OutOfMemory)) => {}
        Err(other) => panic!("expected LltError::Generic(OutOfMemory), got {other:?}"),
        Ok(_) => panic!("a failed reservation must not produce a factor"),
    }
}

// ---------------------------------------------------------------------------
// SplitCholesky runs on the shared driver.
// ---------------------------------------------------------------------------

#[test]
fn split_cholesky_counts_one_symbolic_one_numeric_and_both_reservations() {
    let a = tridiagonal(80);
    let faer = faer_symbolic_cholesky(&a);
    let len_val = faer.len_val() as u64;
    let scratch = faer
        .factorize_numeric_llt_scratch::<f64>(Par::Seq, Default::default())
        .size_bytes() as u64;
    assert!(scratch > 0, "precondition: faer needs numeric scratch");

    let probe = FaultProbe::new();
    let _g = probe.install();
    SplitCholesky::try_new(&a).expect("fixture is SPD");

    assert_eq!(probe.symbolic_calls(), 1);
    assert_eq!(probe.numeric_calls(), 1);
    assert_eq!(probe.symbolic_variants(), [SymbolicVariant::Simplicial]);
    assert_eq!(
        probe.reservations(),
        [
            Reservation {
                site: ReservationSite::FactorValues,
                bytes: 8 * len_val,
            },
            Reservation {
                site: ReservationSite::NumericScratch,
                bytes: scratch,
            },
        ]
    );
}

#[test]
fn split_cholesky_records_the_supernodal_variant_faer_picks() {
    let probe = FaultProbe::new();
    let _g = probe.install();
    SplitCholesky::try_new(&supernodal_banded()).expect("fixture is SPD");
    assert_eq!(probe.symbolic_variants(), [SymbolicVariant::Supernodal]);
}

#[test]
fn forcing_supernodal_on_a_simplicial_fixture_still_composes() {
    let a = tridiagonal(80);
    assert_eq!(
        faer_variant(&a),
        SymbolicVariant::Simplicial,
        "precondition: faer alone picks simplicial"
    );
    let probe = FaultProbe::new();
    probe.force_symbolic_variant(SymbolicVariant::Supernodal);
    let _g = probe.install();
    let g = SplitCholesky::try_new(&a).expect("fixture is SPD");
    assert_eq!(probe.symbolic_variants(), [SymbolicVariant::Supernodal]);
    assert_halves_compose_to_the_inverse(&g, &a, "forced supernodal");
}

#[test]
fn forcing_simplicial_on_a_supernodal_fixture_still_composes() {
    let a = supernodal_banded();
    assert_eq!(
        faer_variant(&a),
        SymbolicVariant::Supernodal,
        "precondition: faer alone picks supernodal"
    );
    let probe = FaultProbe::new();
    probe.force_symbolic_variant(SymbolicVariant::Simplicial);
    let _g = probe.install();
    let g = SplitCholesky::try_new(&a).expect("fixture is SPD");
    assert_eq!(probe.symbolic_variants(), [SymbolicVariant::Simplicial]);
    assert_halves_compose_to_the_inverse(&g, &a, "forced simplicial");
}

#[test]
fn failed_factor_values_reservation_is_out_of_memory_before_numeric() {
    let a = tridiagonal(80);
    let probe = FaultProbe::new();
    probe.fail_reservation(ReservationSite::FactorValues);
    let _g = probe.install();
    assert_out_of_memory(SplitCholesky::try_new(&a));
    assert_eq!(probe.numeric_calls(), 0);
    let sites: Vec<_> = probe.reservations().iter().map(|r| r.site).collect();
    assert_eq!(sites, [ReservationSite::FactorValues]);
}

#[test]
fn failed_numeric_scratch_reservation_is_out_of_memory_before_numeric() {
    let a = tridiagonal(80);
    let probe = FaultProbe::new();
    probe.fail_reservation(ReservationSite::NumericScratch);
    let _g = probe.install();
    assert_out_of_memory(SplitCholesky::try_new(&a));
    assert_eq!(probe.numeric_calls(), 0);
    let sites: Vec<_> = probe.reservations().iter().map(|r| r.site).collect();
    assert_eq!(
        sites,
        [
            ReservationSite::FactorValues,
            ReservationSite::NumericScratch
        ]
    );
}

#[test]
fn probe_sees_only_its_own_thread_and_only_while_installed() {
    let a = tridiagonal(80);
    let probe = FaultProbe::new();
    let guard = probe.install();
    std::thread::scope(|s| {
        s.spawn(|| {
            SplitCholesky::try_new(&a)
                .map(|_| ())
                .expect("fixture is SPD")
        });
    });
    assert_eq!(probe.symbolic_calls(), 0, "another thread's factorisation");
    assert_eq!(probe.numeric_calls(), 0, "another thread's factorisation");
    assert!(
        probe.reservations().is_empty(),
        "another thread's factorisation"
    );

    drop(guard);
    SplitCholesky::try_new(&a).expect("fixture is SPD");
    assert_eq!(probe.symbolic_calls(), 0, "after the guard dropped");
    assert_eq!(probe.numeric_calls(), 0, "after the guard dropped");
    assert!(probe.reservations().is_empty(), "after the guard dropped");
    assert!(
        probe.symbolic_variants().is_empty(),
        "after the guard dropped"
    );
}
