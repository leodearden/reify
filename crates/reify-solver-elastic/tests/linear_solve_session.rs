//! The `linear_solve` seam's session contract and its Cholesky driver (PRD
//! `docs/prds/v0_6/elastic-static-solver-performance.md` §5 C1, C2, C5).
//!
//! - `SplitCholesky` runs on the seam's one driver: every factorisation it
//!   makes is visible to an installed `FaultProbe` — one symbolic and one
//!   numeric call, and exactly the two fallible reservations, in order — and
//!   the probe can force faer's symbolic variant or fail a reservation.
//! - `SpdSolver` sessions on both tiers: the tier decision, the lazy factor,
//!   the zero-RHS rule, the true residual on every tier, typed stops instead
//!   of panics, phases and cooperative cancel, and agreement of the tiers.
//! - The hooks a detached factorisation worker (ι) relies on: carrying the
//!   probe to another thread, holding the numeric step at a barrier, and
//!   panicking in it. Every wait is a blocking handshake with no timeout.
//!
//! No row asserts a wall-clock time or a committed bit pattern; bit equality
//! is compared within one run.

use std::num::NonZeroUsize;
use std::ops::ControlFlow;
use std::sync::{Arc, OnceLock};

use faer::linalg::solvers::SolveCore;
use faer::sparse::linalg::cholesky::{
    CholeskySymbolicParams, SymbolicCholesky, SymbolicCholeskyRaw, factorize_symbolic_cholesky,
};
use faer::sparse::linalg::{LltError, SupernodalThreshold};
use faer::sparse::{FaerError, SparseRowMat, Triplet};
use faer::{Conj, Mat, Par, Side};
use reify_solver_elastic::linear_solve::fault_injection::{
    FaultProbe, INJECTED_NUMERIC_PANIC, NumericBarrier, Reservation, ReservationSite,
    SymbolicVariant,
};
use reify_solver_elastic::linear_solve::{
    FactorEstimate, FactorReuse, LinearSolverChoice, NpdSite, PreconditionerKind, Solution,
    SolveControl, SolveEvent, SolveFailure, SolveLimits, SolvePhase, SolvePolicy, SolveStop,
    SpdSolver, SpdSystem, Strategy, StrategyDecision, StrategyReason,
};
use reify_solver_elastic::split_cholesky::SplitCholesky;
use reify_solver_elastic::{
    AssemblyElement, DirichletBc, ElementOrder, ElementStiffness, IsotropicElastic,
    apply_dirichlet_row_elimination, assemble_stiffness, element_stiffness,
};

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
fn an_injected_symbolic_failure_reaches_split_cholesky_unchanged() {
    for error in [FaerError::OutOfMemory, FaerError::IndexOverflow] {
        let probe = FaultProbe::new();
        probe.fail_symbolic(error);
        let _g = probe.install();
        match SplitCholesky::try_new(&tridiagonal(80)) {
            Err(LltError::Generic(seen)) => assert_eq!(seen, error),
            Err(other) => panic!("expected LltError::Generic({error:?}), got {other:?}"),
            Ok(_) => panic!("a failed symbolic analysis must not produce a factor"),
        }
        assert_eq!(probe.symbolic_calls(), 0, "{error:?}");
        assert!(probe.reservations().is_empty(), "{error:?}");
    }
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

// ---------------------------------------------------------------------------
// Session fixtures.
// ---------------------------------------------------------------------------

const LIMITS: SolveLimits = SolveLimits {
    tolerance: 1e-6,
    max_iter: 10_000,
};

const GIB: u64 = 1024 * 1024 * 1024;

const JACOBI: Strategy = Strategy::Pcg(PreconditionerKind::Jacobi);

/// Split a hex cell into 6 tetrahedra via the Kuhn triangulation (copied from
/// `tests/determinism.rs` per the per-test-file helper convention).
fn kuhn_split_hex_to_six_tets(c: [usize; 8]) -> [[usize; 4]; 6] {
    [
        [c[0], c[1], c[2], c[6]],
        [c[0], c[1], c[5], c[6]],
        [c[0], c[3], c[2], c[6]],
        [c[0], c[3], c[7], c[6]],
        [c[0], c[4], c[5], c[6]],
        [c[0], c[4], c[7], c[6]],
    ]
}

/// A structured P1 tet mesh of `[0,lx]×[0,ly]×[0,lz]` with `nx×ny×nz` hex
/// cells, each Kuhn-split into 6 tets (copied from `tests/determinism.rs`).
fn box_p1_mesh(
    lx: f64,
    ly: f64,
    lz: f64,
    nx: usize,
    ny: usize,
    nz: usize,
) -> (Vec<[f64; 3]>, Vec<[usize; 4]>) {
    let (nnx, nny, nnz) = (nx + 1, ny + 1, nz + 1);
    let mut nodes = Vec::with_capacity(nnx * nny * nnz);
    for iz in 0..nnz {
        for iy in 0..nny {
            for ix in 0..nnx {
                nodes.push([
                    ix as f64 * lx / nx as f64,
                    iy as f64 * ly / ny as f64,
                    iz as f64 * lz / nz as f64,
                ]);
            }
        }
    }
    let node_idx = |ix: usize, iy: usize, iz: usize| iz * nny * nnx + iy * nnx + ix;
    let mut connectivity = Vec::with_capacity(6 * nx * ny * nz);
    for iz in 0..nz {
        for iy in 0..ny {
            for ix in 0..nx {
                let c = [
                    node_idx(ix, iy, iz),
                    node_idx(ix + 1, iy, iz),
                    node_idx(ix + 1, iy + 1, iz),
                    node_idx(ix, iy + 1, iz),
                    node_idx(ix, iy, iz + 1),
                    node_idx(ix + 1, iy, iz + 1),
                    node_idx(ix + 1, iy + 1, iz + 1),
                    node_idx(ix, iy + 1, iz + 1),
                ];
                connectivity.extend(kuhn_split_hex_to_six_tets(c));
            }
        }
    }
    (nodes, connectivity)
}

/// F1: the engine's smoke cantilever after boundary conditions — a
/// 1.0×0.1×0.1 m steel beam on a 60×1×6 Kuhn-split hex grid, the x=0 face
/// clamped and −1000 N in z spread equally over the x=L face. 2,562 DOF.
fn build_cantilever() -> (SparseRowMat<usize, f64>, Vec<f64>) {
    const L: f64 = 1.0;
    const NX: usize = 60;
    let steel = IsotropicElastic {
        youngs_modulus: 205e9,
        poisson_ratio: 0.29,
    };
    let (nodes, conns) = box_p1_mesh(L, 0.1, 0.1, NX, 1, 6);
    let stiffness: Vec<ElementStiffness> = conns
        .iter()
        .map(|conn| element_stiffness(ElementOrder::P1, &conn.map(|i| nodes[i]), &steel))
        .collect();
    let elements: Vec<AssemblyElement<'_>> = conns
        .iter()
        .zip(&stiffness)
        .enumerate()
        .map(|(id, (conn, k_e))| AssemblyElement {
            id,
            connectivity: conn.as_slice(),
            k_e,
        })
        .collect();
    let ControlFlow::Continue(mut k) =
        assemble_stiffness(nodes.len(), &elements, NonZeroUsize::MIN, &mut || {
            ControlFlow::Continue(())
        })
    else {
        unreachable!("a poll that never breaks cannot cancel assembly");
    };

    let tol = 0.5 * L / NX as f64;
    let on_face = |x: f64| -> Vec<usize> {
        (0..nodes.len())
            .filter(|&n| (nodes[n][0] - x).abs() < tol)
            .collect()
    };
    let clamp: Vec<DirichletBc> = on_face(0.0)
        .into_iter()
        .flat_map(|n| {
            (0..3).map(move |c| DirichletBc {
                dof: 3 * n + c,
                value: 0.0,
            })
        })
        .collect();
    let tip = on_face(L);
    let mut f = vec![0.0; 3 * nodes.len()];
    for &n in &tip {
        f[3 * n + 2] -= 1000.0 / tip.len() as f64;
    }
    apply_dirichlet_row_elimination(&mut k, &mut f, &clamp);
    (k, f)
}

fn cantilever() -> (Arc<SpdSystem>, Vec<f64>) {
    static F1: OnceLock<(SparseRowMat<usize, f64>, Vec<f64>)> = OnceLock::new();
    let (k, f) = F1.get_or_init(build_cantilever);
    let system = system(k.clone());
    assert_eq!(
        system.n_dofs(),
        2562,
        "F1 is the 2,562-DOF smoke cantilever"
    );
    (system, f.clone())
}

/// F2 (BT4): the identity with the indefinite block `[[1,2],[2,1]]` at
/// `(BT4_A, BT4_B)` — its pivots are 1 then 1 − 2·2 = −3 — plus the SPD pair
/// `[[4,1],[1,4]]` at `BT4_SPD_PAIR`. With the indefinite block alone faer's
/// postordered AMD leaves the later block DOF at its own position, where a
/// swapped permutation direction would name the same DOF; the SPD pair
/// straddles the block and shifts it.
const BT4_N: usize = 12;
const BT4_A: usize = 3;
const BT4_B: usize = 7;
const BT4_SPD_PAIR: (usize, usize) = (1, 10);

fn indefinite_block() -> SparseRowMat<usize, f64> {
    let (c, d) = BT4_SPD_PAIR;
    let mut trips: Vec<Triplet<usize, usize, f64>> = (0..BT4_N)
        .map(|i| Triplet::new(i, i, if i == c || i == d { 4.0 } else { 1.0 }))
        .collect();
    for (i, j, value) in [(BT4_A, BT4_B, 2.0), (c, d, 1.0)] {
        trips.push(Triplet::new(i, j, value));
        trips.push(Triplet::new(j, i, value));
    }
    SparseRowMat::try_new_from_triplets(BT4_N, BT4_N, &trips).unwrap()
}

/// F3: the `supernodal_banded` shape (eigenvalues in [389, 411], κ ≤ 1.06)
/// with every strict-upper entry replaced by `f64::next_up` of itself, so
/// Direct (lower triangle) and PCG (whole matrix) read different bits.
fn nudged_banded() -> SparseRowMat<usize, f64> {
    let n: usize = 300;
    let w: usize = 100;
    let mut trips = Vec::new();
    for i in 0..n {
        for j in i.saturating_sub(w)..(i + w + 1).min(n) {
            let value = match i.cmp(&j) {
                std::cmp::Ordering::Equal => 4.0 * w as f64,
                std::cmp::Ordering::Greater => -1.0 / i.abs_diff(j) as f64,
                std::cmp::Ordering::Less => (-1.0 / i.abs_diff(j) as f64).next_up(),
            };
            trips.push(Triplet::new(i, j, value));
        }
    }
    SparseRowMat::try_new_from_triplets(n, n, &trips).unwrap()
}

/// F4: dense 8×8, diagonal 9, off-diagonal 1.
fn dense8() -> SparseRowMat<usize, f64> {
    let trips: Vec<_> = (0..8)
        .flat_map(|i| (0..8).map(move |j| Triplet::new(i, j, if i == j { 9.0 } else { 1.0 })))
        .collect();
    SparseRowMat::try_new_from_triplets(8, 8, &trips).unwrap()
}

fn system(k: SparseRowMat<usize, f64>) -> Arc<SpdSystem> {
    Arc::new(SpdSystem::new(k).expect("fixture is a valid SpdSystem"))
}

/// A deterministic RHS with no structure a solver could accidentally respect.
fn scattered_rhs(n: usize) -> Vec<f64> {
    (0..n)
        .map(|i| ((i * 7919) % 101) as f64 / 101.0 - 0.5)
        .collect()
}

fn unit_pair(n: usize, a: usize, b: usize, sign_b: f64) -> Vec<f64> {
    let mut rhs = vec![0.0; n];
    rhs[a] = 1.0;
    rhs[b] = sign_b;
    rhs
}

fn bits(u: &[f64]) -> Vec<u64> {
    u.iter().map(|x| x.to_bits()).collect()
}

/// Runs `body` under a control that records every event and returns
/// `Break(())` on the first event `stop` matches.
fn recorded<R>(
    stop: impl Fn(&SolveEvent) -> bool,
    body: impl FnOnce(&mut SolveControl<'_>) -> R,
) -> (R, Vec<SolveEvent>) {
    let mut events = Vec::new();
    let mut on_event = |event: &SolveEvent| {
        events.push(*event);
        if stop(event) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let result = body(&mut SolveControl::new(&mut on_event));
    (result, events)
}

fn quiet<R>(body: impl FnOnce(&mut SolveControl<'_>) -> R) -> R {
    recorded(|_| false, body).0
}

fn prepare(
    system: &Arc<SpdSystem>,
    choice: LinearSolverChoice,
    policy: &SolvePolicy,
) -> Result<SpdSolver, SolveStop> {
    quiet(|ctl| SpdSolver::prepare(Arc::clone(system), choice, policy, ctl))
}

fn prepared(system: &Arc<SpdSystem>, choice: LinearSolverChoice) -> SpdSolver {
    prepare(system, choice, &SolvePolicy::default())
        .unwrap_or_else(|stop| panic!("{choice:?} prepare: {stop}"))
}

fn solve(
    solver: &mut SpdSolver,
    rhs: &[f64],
    limits: SolveLimits,
    warm: Option<&[f64]>,
) -> Result<Solution, SolveStop> {
    quiet(|ctl| solver.solve(rhs, limits, warm, ctl))
}

/// F1's predicted factor bytes, read from Auto's decision under the default
/// policy.
fn cantilever_factor_bytes() -> u64 {
    let (system, _) = cantilever();
    match *prepared(&system, LinearSolverChoice::Auto).decision() {
        StrategyDecision {
            strategy: Strategy::Direct,
            reason:
                StrategyReason::AutoWithinBudget {
                    predicted_bytes,
                    budget,
                },
        } => {
            assert_eq!(budget, 2 * GIB);
            predicted_bytes
        }
        other => panic!("F1 under the default policy must go Direct within budget: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Session contract.
// ---------------------------------------------------------------------------

#[test]
fn bt1_direct_solves_the_cantilever_to_a_backward_stable_residual() {
    let fb = cantilever_factor_bytes();
    let (system, f) = cantilever();
    let mut solver = prepared(&system, LinearSolverChoice::Direct);
    assert_eq!(
        *solver.decision(),
        StrategyDecision {
            strategy: Strategy::Direct,
            reason: StrategyReason::Requested
        }
    );
    let report = solve(&mut solver, &f, LIMITS, None)
        .expect("Direct solve")
        .report;
    assert_eq!(report.decision, *solver.decision());
    assert_eq!(report.n_dofs, 2562);
    assert_eq!(report.iterations, None);
    assert_eq!(report.reuse, FactorReuse::None);
    let estimate = report.factor.expect("Direct ran the symbolic analysis");
    assert_eq!(estimate.n_dofs, 2562);
    assert_eq!(estimate.factor_bytes, fb, "one estimate per system");
    eprintln!(
        "BT1: Direct true_rel_residual on F1 = {:.3e}",
        report.true_rel_residual
    );
    assert!(
        report.true_rel_residual <= 1e-10,
        "BT1: true_rel_residual = {:.3e} > 1e-10",
        report.true_rel_residual
    );
}

#[test]
fn bt2_auto_compares_the_factor_bytes_with_the_budget() {
    let fb = cantilever_factor_bytes();
    let (system, f) = cantilever();
    let at_budget = prepare(
        &system,
        LinearSolverChoice::Auto,
        &SolvePolicy {
            auto_direct_budget: fb,
            ..SolvePolicy::default()
        },
    )
    .expect("prepare");
    assert_eq!(
        *at_budget.decision(),
        StrategyDecision {
            strategy: Strategy::Direct,
            reason: StrategyReason::AutoWithinBudget {
                predicted_bytes: fb,
                budget: fb
            }
        }
    );

    let mut over = prepare(
        &system,
        LinearSolverChoice::Auto,
        &SolvePolicy {
            auto_direct_budget: fb - 1,
            ..SolvePolicy::default()
        },
    )
    .expect("prepare");
    assert_eq!(
        *over.decision(),
        StrategyDecision {
            strategy: JACOBI,
            reason: StrategyReason::AutoOverBudget {
                predicted_bytes: fb,
                budget: fb - 1
            }
        }
    );
    let report = solve(&mut over, &f, LIMITS, None)
        .expect("Auto→Pcg converges")
        .report;
    assert!(
        matches!(report.iterations, Some(k) if k >= 1),
        "{:?}",
        report.iterations
    );
    assert_eq!(
        report.factor.map(|e| e.factor_bytes),
        Some(fb),
        "Auto ran the symbolic analysis"
    );
}

#[test]
fn bt3_direct_over_the_ceiling_fails_before_any_numeric_allocation() {
    let fb = cantilever_factor_bytes();
    let (system, _) = cantilever();
    let ceiling = |direct_ceiling| SolvePolicy {
        direct_ceiling,
        ..SolvePolicy::default()
    };
    let probe = FaultProbe::new();
    let _g = probe.install();
    assert_eq!(
        prepare(&system, LinearSolverChoice::Direct, &ceiling(fb - 1)).err(),
        Some(SolveStop::Failed(SolveFailure::FactorExceedsCeiling {
            predicted: fb,
            ceiling: fb - 1
        }))
    );
    assert_eq!(probe.symbolic_calls(), 1);
    assert!(
        probe.reservations().is_empty(),
        "{:?}",
        probe.reservations()
    );
    assert_eq!(probe.numeric_calls(), 0);
    assert!(prepare(&system, LinearSolverChoice::Direct, &ceiling(fb)).is_ok());
}

fn bt4_pivot_dof(variant: SymbolicVariant) -> usize {
    let probe = FaultProbe::new();
    probe.force_symbolic_variant(variant);
    let _g = probe.install();
    let mut solver = prepared(&system(indefinite_block()), LinearSolverChoice::Direct);
    assert_eq!(probe.symbolic_variants(), [variant]);
    let rhs = unit_pair(BT4_N, BT4_A, BT4_B, 1.0);
    match solve(&mut solver, &rhs, LIMITS, None) {
        Err(SolveStop::Failed(SolveFailure::NotPositiveDefinite {
            site: NpdSite::Pivot { dof },
        })) => dof,
        other => panic!("{variant:?}: expected a non-positive pivot, got {other:?}"),
    }
}

#[test]
fn bt4_non_positive_pivot_names_the_same_dof_on_both_variants() {
    let faer = faer_symbolic_cholesky(&indefinite_block());
    let (forward, inverse) = faer.perm().expect("AMD reorders F2").arrays();
    assert_ne!(forward, inverse, "precondition: not an involution");
    let pivot_position = inverse[BT4_A].max(inverse[BT4_B]);
    let swapped = inverse[pivot_position];
    assert!(
        swapped != BT4_A && swapped != BT4_B,
        "precondition: the wrong array names dof {swapped}, inside the block"
    );

    let simplicial = bt4_pivot_dof(SymbolicVariant::Simplicial);
    let supernodal = bt4_pivot_dof(SymbolicVariant::Supernodal);
    assert!([BT4_A, BT4_B].contains(&simplicial), "dof {simplicial}");
    assert_eq!(simplicial, supernodal);
    assert_eq!(simplicial, forward[pivot_position]);
}

fn dense_estimate(variant: SymbolicVariant) -> (FactorEstimate, Vec<Reservation>) {
    let probe = FaultProbe::new();
    probe.force_symbolic_variant(variant);
    let _g = probe.install();
    let mut solver = prepared(&system(dense8()), LinearSolverChoice::Direct);
    let report = solve(&mut solver, &[1.0; 8], LIMITS, None)
        .expect("F4 is SPD")
        .report;
    assert_eq!(probe.symbolic_variants(), [variant]);
    (
        report.factor.expect("Direct estimate"),
        probe.reservations(),
    )
}

fn assert_reservations_partition_the_estimate(
    estimate: &FactorEstimate,
    reservations: &[Reservation],
) {
    let values = 8 * estimate.len_val;
    assert_eq!(
        reservations,
        [
            Reservation {
                site: ReservationSite::FactorValues,
                bytes: values
            },
            Reservation {
                site: ReservationSite::NumericScratch,
                bytes: estimate.factor_bytes - values
            },
        ]
    );
}

#[test]
fn estimate_is_exact_on_a_dense_simplicial_factor() {
    let (estimate, reservations) = dense_estimate(SymbolicVariant::Simplicial);
    assert_eq!(estimate.n_dofs, 8);
    assert_eq!(estimate.len_val, 36);
    assert_eq!(estimate.predicted_flops, 156.0);
    assert_reservations_partition_the_estimate(&estimate, &reservations);
}

#[test]
fn estimate_is_exact_on_a_dense_supernodal_factor() {
    let csc = dense8().to_col_major().unwrap();
    let forced = factorize_symbolic_cholesky(
        csc.as_ref().symbolic(),
        Side::Lower,
        Default::default(),
        CholeskySymbolicParams {
            supernodal_flop_ratio_threshold: SupernodalThreshold::FORCE_SUPERNODAL,
            ..Default::default()
        },
    )
    .unwrap();
    let SymbolicCholeskyRaw::Supernodal(s) = forced.raw() else {
        panic!("precondition: FORCE_SUPERNODAL is supernodal");
    };
    assert_eq!(s.n_supernodes(), 1, "precondition: one supernode");
    assert!(
        s.supernode(0).pattern().is_empty(),
        "precondition: no below-pattern"
    );

    let (estimate, reservations) = dense_estimate(SymbolicVariant::Supernodal);
    assert_eq!(estimate.len_val, 64);
    assert_eq!(estimate.predicted_flops, 352.0);
    assert_reservations_partition_the_estimate(&estimate, &reservations);
}

#[test]
fn bt4b_a_zero_rhs_returns_zero_without_touching_a_tier() {
    let (system, f) = cantilever();
    let zero = vec![0.0; system.n_dofs()];
    let probe = FaultProbe::new();
    let _g = probe.install();
    let mut sessions = Vec::new();
    for (choice, iterations) in [
        (LinearSolverChoice::Direct, None),
        (LinearSolverChoice::Auto, None),
        (LinearSolverChoice::Iterative, Some(0)),
    ] {
        let mut solver = prepared(&system, choice);
        for _ in 0..2 {
            let (result, events) =
                recorded(|_| false, |ctl| solver.solve(&zero, LIMITS, None, ctl));
            let solution = result.unwrap_or_else(|stop| panic!("{choice:?}: {stop}"));
            assert!(events.is_empty(), "{choice:?}: {events:?}");
            assert!(solution.u.iter().all(|&x| x == 0.0), "{choice:?}");
            assert_eq!(solution.report.true_rel_residual, 0.0, "{choice:?}");
            assert_eq!(solution.report.iterations, iterations, "{choice:?}");
            assert_eq!(solution.report.reuse, FactorReuse::None, "{choice:?}");
        }
        sessions.push(solver);
    }
    assert_eq!(probe.numeric_calls(), 0, "zero solves never factor");
    solve(&mut sessions[0], &f, LIMITS, None).expect("Direct solve");
    assert_eq!(probe.numeric_calls(), 1, "the first non-zero solve factors");
}

#[test]
fn bt5_a_second_rhs_reuses_the_session_factor() {
    let (system, f) = cantilever();
    let g: Vec<f64> = f
        .iter()
        .zip(scattered_rhs(f.len()))
        .map(|(fi, s)| fi + s)
        .collect();
    let fresh = solve(
        &mut prepared(&system, LinearSolverChoice::Direct),
        &g,
        LIMITS,
        None,
    )
    .expect("fresh session")
    .u;

    let probe = FaultProbe::new();
    let _g = probe.install();
    let mut solver = prepared(&system, LinearSolverChoice::Direct);
    let first = solve(&mut solver, &f, LIMITS, None).expect("first RHS");
    let second = solve(&mut solver, &g, LIMITS, None).expect("second RHS");
    assert_eq!(probe.symbolic_calls(), 1);
    assert_eq!(probe.numeric_calls(), 1);
    assert_eq!(first.report.reuse, FactorReuse::None);
    assert_eq!(second.report.reuse, FactorReuse::SessionFactor);
    assert_eq!(bits(&second.u), bits(&fresh));
}

#[test]
fn direct_above_the_residual_tolerance_is_a_typed_stop() {
    let (system, f) = cantilever();
    let mut solver = prepared(&system, LinearSolverChoice::Direct);
    let limits = SolveLimits {
        tolerance: 1e-300,
        ..LIMITS
    };
    match solve(&mut solver, &f, limits, None) {
        Err(SolveStop::ResidualAboveTolerance {
            true_rel_residual,
            tolerance,
        }) => {
            assert_eq!(tolerance, 1e-300);
            assert!(
                true_rel_residual.is_finite() && true_rel_residual > 0.0,
                "{true_rel_residual}"
            );
        }
        other => panic!("expected ResidualAboveTolerance, got {other:?}"),
    }
}

#[test]
fn direct_ignores_the_warm_start() {
    let (system, f) = cantilever();
    let garbage: Vec<f64> = (0..f.len()).map(|i| (i as f64).sin() * 1e3).collect();
    let mut solver = prepared(&system, LinearSolverChoice::Direct);
    let cold = solve(&mut solver, &f, LIMITS, None).expect("cold").u;
    let warm = solve(&mut solver, &f, LIMITS, Some(&garbage))
        .expect("warm")
        .u;
    assert_eq!(bits(&cold), bits(&warm));
}

#[test]
fn direct_phases_are_analyse_then_factor_substitute_verify_then_substitute_verify() {
    let (prepared, events) = recorded(
        |_| false,
        |ctl| {
            SpdSolver::prepare(
                system(dense8()),
                LinearSolverChoice::Direct,
                &SolvePolicy::default(),
                ctl,
            )
        },
    );
    assert_eq!(events, [SolveEvent::Phase(SolvePhase::Analyse)]);
    let mut solver = prepared.expect("prepare");
    let phases = |solver: &mut SpdSolver| {
        let (result, events) =
            recorded(|_| false, |ctl| solver.solve(&[1.0; 8], LIMITS, None, ctl));
        result.expect("F4 solve");
        events
    };
    assert_eq!(
        phases(&mut solver),
        [
            SolveEvent::Phase(SolvePhase::Factor),
            SolveEvent::Phase(SolvePhase::Substitute),
            SolveEvent::Phase(SolvePhase::Verify),
        ]
    );
    assert_eq!(
        phases(&mut solver),
        [
            SolveEvent::Phase(SolvePhase::Substitute),
            SolveEvent::Phase(SolvePhase::Verify),
        ]
    );
}

#[test]
fn iterative_phases_report_every_iteration_with_its_relative_residual() {
    let banded = system(nudged_banded());
    let (prepared, events) = recorded(
        |_| false,
        |ctl| {
            SpdSolver::prepare(
                Arc::clone(&banded),
                LinearSolverChoice::Iterative,
                &SolvePolicy::default(),
                ctl,
            )
        },
    );
    assert!(events.is_empty(), "{events:?}");
    let mut solver = prepared.expect("prepare");
    let (result, events) = recorded(
        |_| false,
        |ctl| solver.solve(&scattered_rhs(300), LIMITS, None, ctl),
    );
    let k = result
        .expect("F3 converges")
        .report
        .iterations
        .expect("PCG iterations") as usize;

    assert_eq!(
        events.first(),
        Some(&SolveEvent::Phase(SolvePhase::Iterate))
    );
    assert_eq!(events.last(), Some(&SolveEvent::Phase(SolvePhase::Verify)));
    let iterations: Vec<(u32, f64)> = events[1..events.len() - 1]
        .iter()
        .map(|event| match *event {
            SolveEvent::Iteration { iter, residual } => (iter, residual),
            other => panic!("expected only Iteration events between the phases: {other:?}"),
        })
        .collect();
    assert_eq!(iterations.len(), k);
    assert!(
        iterations
            .iter()
            .enumerate()
            .all(|(i, &(iter, r))| iter as usize == i + 1 && r.is_finite())
    );
    let last = iterations.last().expect("k ≥ 1").1;
    assert!(last < LIMITS.tolerance, "last relative residual {last:e}");
}

#[test]
fn auto_going_iterative_still_emits_the_analyse_phase() {
    let policy = SolvePolicy {
        auto_direct_budget: 0,
        ..SolvePolicy::default()
    };
    let (prepared, events) = recorded(
        |_| false,
        |ctl| SpdSolver::prepare(system(dense8()), LinearSolverChoice::Auto, &policy, ctl),
    );
    assert_eq!(prepared.expect("prepare").decision().strategy, JACOBI);
    assert_eq!(events, [SolveEvent::Phase(SolvePhase::Analyse)]);
}

#[test]
fn breaking_on_analyse_cancels_before_the_symbolic_analysis() {
    let probe = FaultProbe::new();
    let _g = probe.install();
    let (result, _) = recorded(
        |event| *event == SolveEvent::Phase(SolvePhase::Analyse),
        |ctl| {
            SpdSolver::prepare(
                system(dense8()),
                LinearSolverChoice::Direct,
                &SolvePolicy::default(),
                ctl,
            )
        },
    );
    assert_eq!(
        result.err(),
        Some(SolveStop::Cancelled {
            phase: SolvePhase::Analyse
        })
    );
    assert_eq!(probe.symbolic_calls(), 0);
}

#[test]
fn breaking_on_factor_cancels_before_any_reservation_and_leaves_the_session_reusable() {
    let probe = FaultProbe::new();
    let _g = probe.install();
    let mut solver = prepared(&system(dense8()), LinearSolverChoice::Direct);
    let (result, _) = recorded(
        |event| *event == SolveEvent::Phase(SolvePhase::Factor),
        |ctl| solver.solve(&[1.0; 8], LIMITS, None, ctl),
    );
    assert_eq!(
        result.err(),
        Some(SolveStop::Cancelled {
            phase: SolvePhase::Factor
        })
    );
    assert_eq!(probe.numeric_calls(), 0);
    assert!(
        probe.reservations().is_empty(),
        "{:?}",
        probe.reservations()
    );

    solve(&mut solver, &[1.0; 8], LIMITS, None).expect("the same session solves");
    assert_eq!(probe.numeric_calls(), 1);
}

#[test]
fn breaking_on_substitute_or_verify_cancels_in_that_phase() {
    for phase in [SolvePhase::Substitute, SolvePhase::Verify] {
        let mut solver = prepared(&system(dense8()), LinearSolverChoice::Direct);
        let (result, _) = recorded(
            |event| *event == SolveEvent::Phase(phase),
            |ctl| solver.solve(&[1.0; 8], LIMITS, None, ctl),
        );
        assert_eq!(result.err(), Some(SolveStop::Cancelled { phase }));
    }
}

#[test]
fn breaking_on_iterate_or_verify_cancels_the_iterative_tier_in_that_phase() {
    for phase in [SolvePhase::Iterate, SolvePhase::Verify] {
        let mut solver = prepared(&system(nudged_banded()), LinearSolverChoice::Iterative);
        let (result, events) = recorded(
            |event| *event == SolveEvent::Phase(phase),
            |ctl| solver.solve(&scattered_rhs(300), LIMITS, None, ctl),
        );
        assert_eq!(result.err(), Some(SolveStop::Cancelled { phase }));
        let iterations = events
            .iter()
            .filter(|event| matches!(event, SolveEvent::Iteration { .. }))
            .count();
        match phase {
            SolvePhase::Iterate => assert_eq!(events, [SolveEvent::Phase(phase)]),
            _ => {
                assert!(iterations >= 1, "PCG ran before Verify: {events:?}");
                assert_eq!(events.last(), Some(&SolveEvent::Phase(phase)));
            }
        }
    }
}

#[test]
fn breaking_on_an_iteration_cancels_the_iterate_phase() {
    let mut solver = prepared(&system(nudged_banded()), LinearSolverChoice::Iterative);
    let (result, _) = recorded(
        |event| matches!(event, SolveEvent::Iteration { iter: 1, .. }),
        |ctl| solver.solve(&scattered_rhs(300), LIMITS, None, ctl),
    );
    assert_eq!(
        result.err(),
        Some(SolveStop::Cancelled {
            phase: SolvePhase::Iterate
        })
    );
}

#[test]
fn jacobi_pcg_converges_on_the_cantilever_without_a_factor_estimate() {
    let (system, f) = cantilever();
    let mut solver = prepared(&system, LinearSolverChoice::Iterative);
    let report = solve(&mut solver, &f, LIMITS, None)
        .expect("PCG converges")
        .report;
    let k = report.iterations.expect("PCG iterations");
    eprintln!(
        "F1 Jacobi-PCG: {k} iterations, true_rel_residual = {:.3e}",
        report.true_rel_residual
    );
    assert!(k >= 1);
    assert_eq!(report.factor, None);
}

#[test]
fn jacobi_pcg_at_its_cap_is_not_converged_with_the_true_residual() {
    let (system, f) = cantilever();
    let mut solver = prepared(&system, LinearSolverChoice::Iterative);
    let limits = SolveLimits {
        max_iter: 1,
        ..LIMITS
    };
    match solve(&mut solver, &f, limits, None) {
        Err(SolveStop::NotConverged {
            iterations,
            cap,
            true_rel_residual,
            tolerance,
        }) => {
            assert_eq!((iterations, cap), (1, 1));
            assert_eq!(tolerance, 1e-6);
            assert!(
                true_rel_residual.is_finite() && true_rel_residual > 0.0,
                "{true_rel_residual}"
            );
        }
        other => panic!("expected NotConverged, got {other:?}"),
    }
}

#[test]
fn jacobi_pcg_on_negative_curvature_is_a_typed_failure() {
    let mut solver = prepared(&system(indefinite_block()), LinearSolverChoice::Iterative);
    let rhs = unit_pair(BT4_N, BT4_A, BT4_B, -1.0);
    assert_eq!(
        solve(&mut solver, &rhs, LIMITS, None).err(),
        Some(SolveStop::Failed(SolveFailure::NotPositiveDefinite {
            site: NpdSite::Curvature { iteration: 1 }
        }))
    );
}

#[test]
fn direct_and_pcg_agree_on_a_well_conditioned_system() {
    let banded = system(nudged_banded());
    let rhs = scattered_rhs(300);
    let limits = SolveLimits {
        tolerance: 1e-10,
        ..LIMITS
    };
    let direct = solve(
        &mut prepared(&banded, LinearSolverChoice::Direct),
        &rhs,
        limits,
        None,
    )
    .expect("Direct")
    .u;
    let pcg = solve(
        &mut prepared(&banded, LinearSolverChoice::Iterative),
        &rhs,
        limits,
        None,
    )
    .expect("PCG")
    .u;
    let norm = |v: &mut dyn Iterator<Item = f64>| v.map(|x| x * x).sum::<f64>().sqrt();
    let rel = norm(&mut pcg.iter().zip(direct.iter()).map(|(p, d)| p - d))
        / norm(&mut direct.iter().copied());
    eprintln!("F3 tier agreement ‖u_pcg − u_dir‖/‖u_dir‖ = {rel:.3e}");
    assert!(rel <= 1e-8, "tier agreement {rel:.3e} > 1e-8");
}

#[test]
fn jacobi_pcg_warm_started_at_the_solution_returns_it_unchanged() {
    let banded = system(nudged_banded());
    let rhs = scattered_rhs(300);
    let direct = solve(
        &mut prepared(&banded, LinearSolverChoice::Direct),
        &rhs,
        LIMITS,
        None,
    )
    .expect("Direct")
    .u;
    let warm = solve(
        &mut prepared(&banded, LinearSolverChoice::Iterative),
        &rhs,
        LIMITS,
        Some(&direct),
    )
    .expect("warm PCG");
    assert_eq!(warm.report.iterations, Some(0));
    assert_eq!(bits(&warm.u), bits(&direct));
}

#[test]
fn a_failed_factor_reservation_is_out_of_memory_in_the_factor_phase() {
    let (estimate, _) = dense_estimate(SymbolicVariant::Simplicial);
    let values = 8 * estimate.len_val;
    for (site, requested) in [
        (ReservationSite::FactorValues, values),
        (
            ReservationSite::NumericScratch,
            estimate.factor_bytes - values,
        ),
    ] {
        let probe = FaultProbe::new();
        probe.force_symbolic_variant(SymbolicVariant::Simplicial);
        probe.fail_reservation(site);
        let _g = probe.install();
        let mut solver = prepared(&system(dense8()), LinearSolverChoice::Direct);
        assert_eq!(
            solve(&mut solver, &[1.0; 8], LIMITS, None).err(),
            Some(SolveStop::Failed(SolveFailure::OutOfMemory {
                requested,
                phase: SolvePhase::Factor
            })),
            "{site:?}"
        );
        assert_eq!(probe.numeric_calls(), 0, "{site:?}");
    }
}

#[test]
fn a_failed_symbolic_analysis_names_its_real_cause() {
    let (n, nnz) = (8, 64);
    let column_major_bytes =
        ((n + 1) * size_of::<usize>() + nnz * (size_of::<usize>() + size_of::<f64>())) as u64;
    for (error, failure) in [
        (
            FaerError::OutOfMemory,
            SolveFailure::OutOfMemory {
                requested: column_major_bytes,
                phase: SolvePhase::Analyse,
            },
        ),
        (FaerError::IndexOverflow, SolveFailure::IndexOverflow),
    ] {
        for choice in [LinearSolverChoice::Auto, LinearSolverChoice::Direct] {
            let probe = FaultProbe::new();
            probe.fail_symbolic(error);
            let _g = probe.install();
            assert_eq!(
                prepare(&system(dense8()), choice, &SolvePolicy::default()).err(),
                Some(SolveStop::Failed(failure)),
                "{error:?} under {choice:?}"
            );
            assert_eq!(probe.symbolic_calls(), 0, "{error:?} under {choice:?}");
        }
    }
}

#[test]
#[should_panic(expected = "rhs.len()")]
fn a_wrong_length_rhs_is_a_contract_violation() {
    let mut solver = prepared(&system(dense8()), LinearSolverChoice::Direct);
    let _ = solve(&mut solver, &[1.0; 7], LIMITS, None);
}

#[test]
#[should_panic(expected = "rhs[3] = NaN must be finite")]
fn a_non_finite_rhs_is_a_contract_violation() {
    let mut solver = prepared(&system(dense8()), LinearSolverChoice::Iterative);
    let mut rhs = [1.0; 8];
    rhs[3] = f64::NAN;
    let _ = solve(&mut solver, &rhs, LIMITS, None);
}

#[test]
#[should_panic(expected = "warm[5] = inf must be finite")]
fn a_non_finite_warm_start_is_a_contract_violation() {
    let mut solver = prepared(&system(dense8()), LinearSolverChoice::Iterative);
    let mut warm = [0.0; 8];
    warm[5] = f64::INFINITY;
    let _ = solve(&mut solver, &[1.0; 8], LIMITS, Some(&warm));
}

// ---------------------------------------------------------------------------
// Hooks for a detached factorisation worker (ι).
// ---------------------------------------------------------------------------

fn reservation_sites(probe: &FaultProbe) -> Vec<ReservationSite> {
    probe.reservations().iter().map(|r| r.site).collect()
}

const BOTH_SITES: [ReservationSite; 2] = [
    ReservationSite::FactorValues,
    ReservationSite::NumericScratch,
];

#[test]
fn current_is_the_probe_installed_on_this_thread() {
    assert!(FaultProbe::current().is_none(), "no probe installed");
    let probe = FaultProbe::new();
    let guard = probe.install();
    let current = FaultProbe::current().expect("installed probe");
    let inner = current.install();
    SplitCholesky::try_new(&tridiagonal(80)).expect("fixture is SPD");
    assert_eq!(probe.numeric_calls(), 1, "counted through the shared state");
    drop(inner);
    drop(guard);
    assert!(
        FaultProbe::current().is_none(),
        "the guard restored no probe"
    );
}

#[test]
fn a_probe_carried_to_another_thread_counts_its_factorisation() {
    let probe = FaultProbe::new();
    let _g = probe.install();
    let carried = FaultProbe::current().expect("installed probe");
    std::thread::scope(|s| {
        s.spawn(move || {
            let _g = carried.install();
            SplitCholesky::try_new(&tridiagonal(80))
                .map(|_| ())
                .expect("fixture is SPD");
        });
    });
    assert_eq!(probe.symbolic_calls(), 1);
    assert_eq!(probe.numeric_calls(), 1);
}

#[test]
fn a_barrier_holds_the_numeric_step_after_both_reservations() {
    let probe = FaultProbe::new();
    let barrier: NumericBarrier = probe.hold_numeric_at_barrier();
    let worker_probe = probe.clone();
    let worker = std::thread::spawn(move || {
        let _g = worker_probe.install();
        let mut solver = prepared(&system(dense8()), LinearSolverChoice::Direct);
        solve(&mut solver, &[1.0; 8], LIMITS, None)
    });

    barrier.wait_until_reached();
    assert_eq!(probe.numeric_calls(), 1);
    assert_eq!(reservation_sites(&probe), BOTH_SITES);
    assert!(!worker.is_finished(), "the worker is held at the barrier");

    barrier.release();
    let solution = worker.join().expect("worker").expect("F4 solve");
    assert!(solution.report.true_rel_residual <= LIMITS.tolerance);
}

#[test]
fn an_armed_panic_fires_in_the_numeric_step_after_both_reservations() {
    let probe = FaultProbe::new();
    let _g = probe.install();
    let mut solver = prepared(&system(dense8()), LinearSolverChoice::Direct);
    probe.panic_in_numeric();
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        solve(&mut solver, &[1.0; 8], LIMITS, None)
    }))
    .expect_err("the armed hook panics");
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or_default();
    assert!(
        message.contains(INJECTED_NUMERIC_PANIC),
        "payload: {message:?}"
    );
    assert_eq!(probe.numeric_calls(), 1);
    assert_eq!(reservation_sites(&probe), BOTH_SITES);
}
