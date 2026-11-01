//! The trust-region Gauss–Newton core and its typed termination causes — task
//! #6675 (solver-unification η), PRD
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.5 / §7.7.
//!
//! Every problem here states its residual already in SCALED units, so the
//! convergence bar is `max|r| ≤ 1`: each textbook residual is multiplied by
//! `S = 1e8`, making the bar mean ~1e-8 accuracy.

use std::f64::consts::PI;

use reify_constraints::linalg::RankTolerance;
use reify_constraints::trust_region::{
    LeastSquaresProblem, RANGE_COSINE_TOLERANCE, SolveBudget, TrustRegionRun, solve,
};
use reify_ir::{ResidualSource, SolveFailureCause};

const S: f64 = 1e8;

const RANK: RankTolerance = RankTolerance {
    independence: 1e-8,
    zero_row: 1e-9,
};

fn max_abs(r: &[f64]) -> f64 {
    r.iter().fold(0.0_f64, |m, v| m.max(v.abs()))
}

fn run(problem: &impl LeastSquaresProblem, x0: &[f64]) -> TrustRegionRun {
    solve(problem, x0, SolveBudget::default(), RANK)
}

/// Rosenbrock's least-squares form, `[10(x₂ − x₁²), 1 − x₁]`.
struct Rosenbrock;

impl LeastSquaresProblem for Rosenbrock {
    fn dim(&self) -> usize {
        2
    }
    fn residual(&self, x: &[f64]) -> Vec<f64> {
        vec![S * 10.0 * (x[1] - x[0] * x[0]), S * (1.0 - x[0])]
    }
    fn jacobian(&self, x: &[f64]) -> Vec<Vec<f64>> {
        vec![vec![-S * 20.0 * x[0], S * 10.0], vec![-S, 0.0]]
    }
    fn row_source(&self, row: usize) -> ResidualSource {
        ResidualSource::Relation(row)
    }
}

const ROSENBROCK_START: [f64; 2] = [-1.2, 1.0];

#[test]
fn rosenbrock_converges_to_its_root_with_full_rank() {
    let run = run(&Rosenbrock, &ROSENBROCK_START);
    let converged = run.outcome.as_ref().expect("Rosenbrock has a root");
    assert!(max_abs(&Rosenbrock.residual(&run.x)) <= 1.0, "{run:?}");
    assert!((run.x[0] - 1.0).abs() <= 1e-6 && (run.x[1] - 1.0).abs() <= 1e-6);
    assert_eq!(converged.rank, 2);
    assert!(converged.null_space.is_empty());
    assert_eq!(run.step_norms.len() as u32, run.iterations);
}

/// Three rows in one unknown, `[x − 1, x − 1, x + 2]`: the least-squares minimum
/// is `x = 0`, where the residual is `[−1, −1, 2]`·S and no root exists.
struct Inconsistent;

impl LeastSquaresProblem for Inconsistent {
    fn dim(&self) -> usize {
        1
    }
    fn residual(&self, x: &[f64]) -> Vec<f64> {
        vec![S * (x[0] - 1.0), S * (x[0] - 1.0), S * (x[0] + 2.0)]
    }
    fn jacobian(&self, _x: &[f64]) -> Vec<Vec<f64>> {
        vec![vec![S], vec![S], vec![S]]
    }
    fn row_source(&self, row: usize) -> ResidualSource {
        ResidualSource::Relation(10 + row)
    }
}

#[test]
fn a_stationary_non_root_is_certified_infeasible_with_blame() {
    let run = run(&Inconsistent, &[3.0]);
    let Err(SolveFailureCause::Infeasible(cert)) = &run.outcome else {
        panic!("expected a certified infeasibility, got {run:?}");
    };
    assert!(
        cert.range_cosine <= RANGE_COSINE_TOLERANCE,
        "range cosine {} above the published tolerance",
        cert.range_cosine
    );
    assert!(cert.gradient_norm >= 0.0);
    assert!(
        (cert.max_scaled_residual - 2.0 * S).abs() <= 1e-9 * S,
        "the LS-minimum residual is 2S: {cert:?}"
    );

    let sources: Vec<ResidualSource> = cert.blame.iter().map(|b| b.source.clone()).collect();
    assert_eq!(
        sources[0],
        ResidualSource::Relation(12),
        "largest |r̃| first"
    );
    assert_eq!(cert.blame[0].row, 2);
    for expected in [10, 11, 12] {
        assert!(
            sources.contains(&ResidualSource::Relation(expected)),
            "row {expected} must be blamed: {cert:?}"
        );
    }
    assert!(
        cert.blame
            .windows(2)
            .all(|w| w[0].scaled_residual.abs() >= w[1].scaled_residual.abs()),
        "{cert:?}"
    );
}

#[test]
fn an_exhausted_budget_is_reported_as_such() {
    let run = solve(
        &Rosenbrock,
        &ROSENBROCK_START,
        SolveBudget { max_iterations: 2 },
        RANK,
    );
    let Err(SolveFailureCause::BudgetExhausted(exhaustion)) = &run.outcome else {
        panic!("expected budget exhaustion, got {run:?}");
    };
    assert_eq!(exhaustion.iterations, 2);
    assert_eq!(exhaustion.budget, 2);
    assert_eq!(exhaustion.step_norms.len(), 2);
    assert!(exhaustion.max_scaled_residual > 1.0, "{exhaustion:?}");
    assert_eq!(run.iterations, 2);
}

/// `[|x| + 1]`: no root, and a kink minimum at 0 where the gradient never
/// vanishes, so no stationarity certificate is possible.
struct Kink;

impl LeastSquaresProblem for Kink {
    fn dim(&self) -> usize {
        1
    }
    fn residual(&self, x: &[f64]) -> Vec<f64> {
        vec![S * (x[0].abs() + 1.0)]
    }
    fn jacobian(&self, x: &[f64]) -> Vec<Vec<f64>> {
        vec![vec![S * x[0].signum()]]
    }
    fn row_source(&self, row: usize) -> ResidualSource {
        ResidualSource::Relation(row)
    }
}

#[test]
fn a_kink_stall_is_neither_infeasible_nor_budget_exhaustion() {
    let run = run(&Kink, &[0.5]);
    assert!(
        matches!(run.outcome, Err(SolveFailureCause::Stalled(_))),
        "{run:?}"
    );
}

/// `[x₁ − 1]` in two unknowns: `x₂` is a gauge direction no row sees.
struct Gauge;

impl LeastSquaresProblem for Gauge {
    fn dim(&self) -> usize {
        2
    }
    fn residual(&self, x: &[f64]) -> Vec<f64> {
        vec![S * (x[0] - 1.0)]
    }
    fn jacobian(&self, _x: &[f64]) -> Vec<Vec<f64>> {
        vec![vec![S, 0.0]]
    }
    fn row_source(&self, row: usize) -> ResidualSource {
        ResidualSource::Relation(row)
    }
}

#[test]
fn a_gauge_direction_never_moves_and_is_the_null_space() {
    let run = run(&Gauge, &[0.0, 5.0]);
    let converged = run.outcome.as_ref().expect("the gauge problem has roots");
    assert_eq!(run.x[1], 5.0, "the null direction stays at its seed");
    assert!(max_abs(&Gauge.residual(&run.x)) <= 1.0);
    assert_eq!(converged.rank, 1);
    assert_eq!(converged.null_space.len(), 1);
    let z = &converged.null_space[0];
    assert!(
        z[0].abs() <= 1e-12 && (z[1].abs() - 1.0).abs() <= 1e-12,
        "{z:?}"
    );
}

/// A point on the unit circle at angle `x` against the point at 0.5 rad; the
/// chart wraps the angle into (−π, π].
struct Angle;

impl LeastSquaresProblem for Angle {
    fn dim(&self) -> usize {
        1
    }
    fn residual(&self, x: &[f64]) -> Vec<f64> {
        vec![
            S * (x[0].cos() - 0.5_f64.cos()),
            S * (x[0].sin() - 0.5_f64.sin()),
        ]
    }
    fn jacobian(&self, x: &[f64]) -> Vec<Vec<f64>> {
        vec![vec![-S * x[0].sin()], vec![S * x[0].cos()]]
    }
    fn normalize(&self, x: &mut [f64]) {
        x[0] -= 2.0 * PI * (x[0] / (2.0 * PI)).round();
    }
    fn row_source(&self, row: usize) -> ResidualSource {
        ResidualSource::Relation(row)
    }
}

#[test]
fn the_normalize_chart_keeps_the_converged_point_in_range() {
    let run = run(&Angle, &[0.8 + 2.0 * PI]);
    assert!(run.outcome.is_ok(), "{run:?}");
    assert!(-PI < run.x[0] && run.x[0] <= PI, "{run:?}");
    assert!(max_abs(&Angle.residual(&run.x)) <= 1.0, "{run:?}");
}

/// A satisfied stiff row beside a violated soft one, `[1e9·x₀, x₁ − 10]`:
/// linear, full rank and consistent, so its root `(0, 10)` must be found. The
/// stiff row's Jacobian entry must not hide the soft row's gradient from the
/// stationarity test.
struct DisparateRows;

impl LeastSquaresProblem for DisparateRows {
    fn dim(&self) -> usize {
        2
    }
    fn residual(&self, x: &[f64]) -> Vec<f64> {
        vec![1e9 * x[0], x[1] - 10.0]
    }
    fn jacobian(&self, _x: &[f64]) -> Vec<Vec<f64>> {
        vec![vec![1e9, 0.0], vec![0.0, 1.0]]
    }
    fn row_source(&self, row: usize) -> ResidualSource {
        ResidualSource::Relation(row)
    }
}

#[test]
fn a_satisfied_stiff_row_does_not_hide_a_violated_soft_row() {
    let run = run(&DisparateRows, &[0.0, 0.0]);
    assert!(run.outcome.is_ok(), "{run:?}");
    assert!(max_abs(&DisparateRows.residual(&run.x)) <= 1.0, "{run:?}");
}

/// As [`DisparateRows`], but the stiff row couples both unknowns,
/// `[1e9·(x₀ + x₁), x₁ − 10]`, so no single column's gradient ratio is large
/// either: only the cosine between `r̃` and the range of `J̃` sees the soft row.
struct CoupledStiffRows;

impl LeastSquaresProblem for CoupledStiffRows {
    fn dim(&self) -> usize {
        2
    }
    fn residual(&self, x: &[f64]) -> Vec<f64> {
        vec![1e9 * (x[0] + x[1]), x[1] - 10.0]
    }
    fn jacobian(&self, _x: &[f64]) -> Vec<Vec<f64>> {
        vec![vec![1e9, 1e9], vec![0.0, 1.0]]
    }
    fn row_source(&self, row: usize) -> ResidualSource {
        ResidualSource::Relation(row)
    }
}

#[test]
fn a_coupled_stiff_row_does_not_hide_a_violated_soft_row() {
    let run = run(&CoupledStiffRows, &[0.0, 0.0]);
    assert!(run.outcome.is_ok(), "{run:?}");
    assert!(
        max_abs(&CoupledStiffRows.residual(&run.x)) <= 1.0,
        "{run:?}"
    );
}

/// `[100·cos x]`, seeded at its crest `x = 0`: the row is violated (`r̃ = 100`)
/// and its Jacobian vanishes, so the point is stationary to first order — yet a
/// root sits at `π/2`. A stationary MAXIMUM is not an infeasibility.
struct Crest;

impl LeastSquaresProblem for Crest {
    fn dim(&self) -> usize {
        1
    }
    fn residual(&self, x: &[f64]) -> Vec<f64> {
        vec![100.0 * x[0].cos()]
    }
    fn jacobian(&self, x: &[f64]) -> Vec<Vec<f64>> {
        vec![vec![-100.0 * x[0].sin()]]
    }
    fn row_source(&self, row: usize) -> ResidualSource {
        ResidualSource::Relation(row)
    }
}

#[test]
fn a_stationary_crest_is_left_not_certified_infeasible() {
    let run = run(&Crest, &[0.0]);
    assert!(run.outcome.is_ok(), "{run:?}");
    assert!(max_abs(&Crest.residual(&run.x)) <= 1.0, "{run:?}");
}
