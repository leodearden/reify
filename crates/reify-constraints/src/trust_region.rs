//! Trust-region Gauss–Newton over a [`LeastSquaresProblem`] (PRD
//! `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.7).
//!
//! The problem states its unknowns and residual rows in SCALED units: a row is
//! satisfied when `|r̃ᵢ| ≤ 1`, and one unit of an unknown is its nominal scale. So
//! every radius, norm and tolerance here is dimensionless, and a model rescaled
//! by a power of two runs bit-for-bit the same iterations.
//!
//! Each iterate is factorized once ([`RowSpaceQr`] of `J̃`), which yields the
//! minimum-norm Gauss–Newton step, and — at a converged point — the rank and the
//! null space. The step is Powell's dogleg between that step and the Cauchy
//! point; both lie in `J̃`'s row space, so a direction no row sees (a gauge
//! freedom) never moves. The run ends on one of three tests:
//!
//! 1. `max|r̃ᵢ| ≤ 1` — converged;
//! 2. `‖P r̃‖ ≤ RANGE_COSINE_TOLERANCE·‖r̃‖`, where `P` projects onto the range of
//!    `J̃` — a stationary non-root that no descent probe can leave: certified
//!    [`Infeasible`](SolveFailureCause::Infeasible);
//! 3. the scaled step (or the radius) collapses below
//!    `STEP_TOLERANCE·(‖x̃‖ + STEP_TOLERANCE)` — classified by 1 and 2, else
//!    [`Stalled`](SolveFailureCause::Stalled);
//!
//! or on the iteration budget ([`BudgetExhausted`](SolveFailureCause::BudgetExhausted)).
//!
//! A stationary point is only first-order evidence: the crest of a bounded row
//! (a sine of a misalignment at its maximum) is stationary too. So before
//! certifying, the solve probes a fixed distance along every coordinate axis and
//! every null-space direction; a probe that lowers `‖r̃‖²` becomes the next
//! iterate (one iteration of the budget) and the search resumes from it.

use reify_ir::{
    BudgetExhaustion, InfeasibilityCertificate, ResidualSource, RowBlame, SolveFailureCause,
    SolveStall,
};

use crate::linalg::{RankTolerance, RowSpaceQr, dot, mat_t_vec, mat_vec, norm};

/// The initial trust radius: one nominal scale.
pub const INITIAL_RADIUS: f64 = 1.0;
/// The largest trust radius, in nominal scales.
pub const MAX_RADIUS: f64 = 1e3;
/// A trial step is accepted iff its gain ratio `ρ = actual / predicted` exceeds
/// this.
pub const ACCEPT_RATIO: f64 = 0.1;
/// Below this gain ratio the radius contracts by [`CONTRACT_FACTOR`].
pub const CONTRACT_BELOW: f64 = 0.25;
/// The radius contraction factor.
pub const CONTRACT_FACTOR: f64 = 0.25;
/// Above this gain ratio, on a step that reached the boundary, the radius grows
/// by [`EXPAND_FACTOR`] (up to [`MAX_RADIUS`]).
pub const EXPAND_ABOVE: f64 = 0.75;
/// The radius expansion factor.
pub const EXPAND_FACTOR: f64 = 2.0;
/// The bar of the infeasibility certificate: the cosine between `r̃` and the
/// range of `J̃` (`‖P r̃‖ / ‖r̃‖`, `P` the orthogonal projector onto that range)
/// is at most this. The cosine is zero iff the gradient `J̃ᵀr̃` vanishes, and is
/// blind to column scaling and to how stiff the satisfied rows are.
pub const RANGE_COSINE_TOLERANCE: f64 = 1e-8;
/// The relative scaled-step bar of the stall test (`√ε`).
pub const STEP_TOLERANCE: f64 = 1.490_116_119_384_765_6e-8;
/// How far, in nominal scales, a descent probe moves from a stationary point.
const PROBE_LENGTH: f64 = 0.1;
/// A probe escapes a stationary point iff it lowers `‖r̃‖²` by more than this
/// fraction: orders above the rounding of `‖r̃‖²` (~1e-15 relative), orders below
/// the ~`PROBE_LENGTH²/2` relative drop at the crest of a bounded row.
const PROBE_DECREASE: f64 = 1e-6;

/// How many trial steps a solve may take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolveBudget {
    /// The most trial steps (accepted or rejected) before the solve gives up.
    pub max_iterations: u32,
}

impl Default for SolveBudget {
    fn default() -> Self {
        Self {
            max_iterations: 200,
        }
    }
}

/// A nonlinear least-squares problem in scaled coordinates.
pub trait LeastSquaresProblem {
    /// The number of unknowns.
    fn dim(&self) -> usize;
    /// The scaled residual `r̃(x)`.
    fn residual(&self, x: &[f64]) -> Vec<f64>;
    /// The scaled Jacobian `∂r̃/∂x` at `x`, one row per residual row.
    fn jacobian(&self, x: &[f64]) -> Vec<Vec<f64>>;
    /// Re-centre `x` on its chart without changing the point it represents (e.g.
    /// wrap an angle). Applied to the start and after every accepted step.
    fn normalize(&self, _x: &mut [f64]) {}
    /// What produced residual row `row`.
    fn row_source(&self, row: usize) -> ResidualSource;
}

/// A converged solve's rank and null space, from the factorization at the
/// converged point.
#[derive(Debug, Clone, PartialEq)]
pub struct Converged {
    /// The rank of `J̃` at the solution.
    pub rank: usize,
    /// An orthonormal basis (in scaled coordinates) of the directions no row
    /// sees at the solution.
    pub null_space: Vec<Vec<f64>>,
}

/// What one [`solve`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct TrustRegionRun {
    /// The final iterate (the solution when `outcome` is `Ok`).
    pub x: Vec<f64>,
    /// Trial steps taken, accepted or rejected (`0` when the start converged).
    pub iterations: u32,
    /// The scaled norm of each trial step, in order.
    pub step_norms: Vec<f64>,
    /// Converged, or why not.
    pub outcome: Result<Converged, SolveFailureCause>,
}

/// Solve `problem` from `x0` by trust-region Gauss–Newton within `budget`,
/// judging rank with `rank_tolerance`.
pub fn solve(
    problem: &impl LeastSquaresProblem,
    x0: &[f64],
    budget: SolveBudget,
    rank_tolerance: RankTolerance,
) -> TrustRegionRun {
    let mut x = x0.to_vec();
    problem.normalize(&mut x);
    let r = problem.residual(&x);
    let mut iterate = Iterate::at(problem, x, r, rank_tolerance);
    let mut radius = INITIAL_RADIUS;
    let mut iterations = 0_u32;
    let mut step_norms = Vec::new();

    loop {
        match iterate.verdict(problem) {
            Verdict::Converged(converged) => {
                return iterate.finish(iterations, step_norms, Ok(converged));
            }
            Verdict::Stationary(certificate) => {
                let Some((probe_x, probe_r)) = iterate.descent_probe(problem) else {
                    let cause = SolveFailureCause::Infeasible(certificate);
                    return iterate.finish(iterations, step_norms, Err(cause));
                };
                if iterations >= budget.max_iterations {
                    return iterate.exhausted(iterations, budget, step_norms);
                }
                iterations += 1;
                step_norms.push(PROBE_LENGTH);
                iterate = Iterate::at(problem, probe_x, probe_r, rank_tolerance);
                radius = INITIAL_RADIUS;
                continue;
            }
            Verdict::Open => {}
        }
        if iterations >= budget.max_iterations {
            return iterate.exhausted(iterations, budget, step_norms);
        }

        let step = iterate.dogleg(radius);
        let step_norm = norm(&step);
        iterations += 1;
        step_norms.push(step_norm);

        let trial: Vec<f64> = iterate.x.iter().zip(&step).map(|(x, d)| x - d).collect();
        let r_trial = problem.residual(&trial);
        let rho = iterate.gain_ratio(&step, &r_trial);

        if rho < CONTRACT_BELOW {
            radius *= CONTRACT_FACTOR;
        } else if rho > EXPAND_ABOVE && step_norm >= radius * (1.0 - 1e-9) {
            radius = (radius * EXPAND_FACTOR).min(MAX_RADIUS);
        }

        let collapse = STEP_TOLERANCE * (norm(&iterate.x) + STEP_TOLERANCE);
        if rho > ACCEPT_RATIO {
            let mut accepted = trial.clone();
            problem.normalize(&mut accepted);
            let r_accepted = if accepted == trial {
                r_trial
            } else {
                problem.residual(&accepted)
            };
            iterate = Iterate::at(problem, accepted, r_accepted, rank_tolerance);
            if step_norm > collapse {
                continue;
            }
        } else if radius > collapse {
            continue;
        }
        // Test 3 fired. A converged or stationary iterate is resolved at the top.
        if let Verdict::Open = iterate.verdict(problem) {
            let cause = SolveFailureCause::Stalled(SolveStall {
                iterations,
                step_norm,
                range_cosine: iterate.range_cosine(),
                max_scaled_residual: max_abs(&iterate.r),
            });
            return iterate.finish(iterations, step_norms, Err(cause));
        }
    }
}

/// What tests 1 and 2 make of one iterate.
enum Verdict {
    /// Every row is within its bar.
    Converged(Converged),
    /// A stationary non-root, pending the descent probe.
    Stationary(InfeasibilityCertificate),
    /// Neither: the search goes on.
    Open,
}

/// One accepted point with its factorization.
struct Iterate {
    x: Vec<f64>,
    r: Vec<f64>,
    jacobian: Vec<Vec<f64>>,
    qr: RowSpaceQr,
    /// The minimum-norm Gauss–Newton step `δ` (`x ← x − δ`).
    gauss_newton: Vec<f64>,
    /// `g = J̃ᵀr̃`, half the gradient of `‖r̃‖²`.
    gradient: Vec<f64>,
}

impl Iterate {
    fn at(
        problem: &impl LeastSquaresProblem,
        x: Vec<f64>,
        r: Vec<f64>,
        rank_tolerance: RankTolerance,
    ) -> Self {
        let jacobian = problem.jacobian(&x);
        assert_eq!(
            jacobian.len(),
            r.len(),
            "the Jacobian must have one row per residual row"
        );
        let mut qr = RowSpaceQr::new(problem.dim(), rank_tolerance);
        qr.push_rows(&jacobian);
        let gauss_newton = qr.least_squares(&r);
        let gradient = mat_t_vec(&jacobian, &r, problem.dim());
        Self {
            x,
            r,
            jacobian,
            qr,
            gauss_newton,
            gradient,
        }
    }

    /// Tests 1 and 2: converged, stationary, or neither.
    fn verdict(&self, problem: &impl LeastSquaresProblem) -> Verdict {
        if max_abs(&self.r) <= 1.0 {
            return Verdict::Converged(Converged {
                rank: self.qr.rank(),
                null_space: self.qr.null_space(),
            });
        }
        let range_cosine = self.range_cosine();
        if range_cosine > RANGE_COSINE_TOLERANCE {
            return Verdict::Open;
        }
        Verdict::Stationary(InfeasibilityCertificate {
            gradient_norm: norm(&self.gradient),
            range_cosine,
            max_scaled_residual: max_abs(&self.r),
            blame: self.blame(problem),
        })
    }

    /// The lowest-cost point [`PROBE_LENGTH`] away along a coordinate axis or a
    /// null-space direction, with its residual — iff it lowers `‖r̃‖²` by more
    /// than [`PROBE_DECREASE`] of itself.
    fn descent_probe(&self, problem: &impl LeastSquaresProblem) -> Option<(Vec<f64>, Vec<f64>)> {
        let dim = problem.dim();
        let axes = (0..dim).map(|j| {
            let mut e = vec![0.0; dim];
            e[j] = 1.0;
            e
        });
        let cost = dot(&self.r, &self.r);
        axes.chain(self.qr.null_space())
            .flat_map(|d| [scaled(&d, PROBE_LENGTH), scaled(&d, -PROBE_LENGTH)])
            .filter_map(|offset| {
                let mut x: Vec<f64> = self.x.iter().zip(&offset).map(|(x, o)| x + o).collect();
                problem.normalize(&mut x);
                let r = problem.residual(&x);
                let usable = r.len() == self.r.len() && r.iter().all(|v| v.is_finite());
                usable.then(|| (dot(&r, &r), x, r))
            })
            .filter(|(probe_cost, _, _)| *probe_cost < cost * (1.0 - PROBE_DECREASE))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, x, r)| (x, r))
    }

    /// Stop on the iteration budget at this iterate.
    fn exhausted(
        self,
        iterations: u32,
        budget: SolveBudget,
        step_norms: Vec<f64>,
    ) -> TrustRegionRun {
        let cause = SolveFailureCause::BudgetExhausted(BudgetExhaustion {
            iterations,
            budget: budget.max_iterations,
            step_norms: step_norms.clone(),
            max_scaled_residual: max_abs(&self.r),
        });
        self.finish(iterations, step_norms, Err(cause))
    }

    fn finish(
        self,
        iterations: u32,
        step_norms: Vec<f64>,
        outcome: Result<Converged, SolveFailureCause>,
    ) -> TrustRegionRun {
        TrustRegionRun {
            x: self.x,
            iterations,
            step_norms,
            outcome,
        }
    }

    /// `‖J̃·δ_gn‖ / ‖r̃‖`; `0` when `r̃ = 0`. The minimum-norm Gauss–Newton step
    /// solves `J̃δ ≈ r̃` in the least-squares sense, so `J̃·δ_gn` is `r̃` projected
    /// onto the range of `J̃`, and this is the cosine between the two.
    fn range_cosine(&self) -> f64 {
        let residual_norm = norm(&self.r);
        if residual_norm > 0.0 {
            norm(&mat_vec(&self.jacobian, &self.gauss_newton)) / residual_norm
        } else {
            0.0
        }
    }

    /// The rows above their bar, largest `|r̃ᵢ|` first.
    fn blame(&self, problem: &impl LeastSquaresProblem) -> Vec<RowBlame> {
        let mut blame: Vec<RowBlame> = self
            .r
            .iter()
            .enumerate()
            .filter(|(_, ri)| ri.abs() > 1.0)
            .map(|(row, &scaled_residual)| RowBlame {
                source: problem.row_source(row),
                row,
                scaled_residual,
            })
            .collect();
        blame.sort_by(|a, b| b.scaled_residual.abs().total_cmp(&a.scaled_residual.abs()));
        blame
    }

    /// Powell's dogleg step within `radius`.
    fn dogleg(&self, radius: f64) -> Vec<f64> {
        let gauss_newton_norm = norm(&self.gauss_newton);
        if gauss_newton_norm <= radius {
            return self.gauss_newton.clone();
        }
        let g = &self.gradient;
        let g_norm = norm(g);
        let jg = mat_vec(&self.jacobian, g);
        let cauchy_length = g_norm * g_norm / dot(&jg, &jg) * g_norm;
        if cauchy_length.is_nan() || cauchy_length >= radius {
            return scaled(g, radius / g_norm);
        }
        let cauchy = scaled(g, cauchy_length / g_norm);
        let d: Vec<f64> = self
            .gauss_newton
            .iter()
            .zip(&cauchy)
            .map(|(n, c)| n - c)
            .collect();
        let (a, b, c) = (
            dot(&d, &d),
            2.0 * dot(&cauchy, &d),
            dot(&cauchy, &cauchy) - radius * radius,
        );
        let root = (b * b - 4.0 * a * c).max(0.0).sqrt();
        let tau = if b >= 0.0 {
            -2.0 * c / (b + root)
        } else {
            (root - b) / (2.0 * a)
        };
        cauchy.iter().zip(&d).map(|(c, d)| c + tau * d).collect()
    }

    /// `ρ = (‖r̃‖² − ‖r̃(x − δ)‖²) / (‖r̃‖² − ‖r̃ − J̃δ‖²)`; `−∞` when the model
    /// predicts no decrease or the trial residual is unusable.
    fn gain_ratio(&self, step: &[f64], r_trial: &[f64]) -> f64 {
        if r_trial.len() != self.r.len() {
            return f64::NEG_INFINITY;
        }
        let jd = mat_vec(&self.jacobian, step);
        let predicted = 2.0 * dot(&self.r, &jd) - dot(&jd, &jd);
        let actual: f64 = self
            .r
            .iter()
            .zip(r_trial)
            .map(|(r, t)| (r - t) * (r + t))
            .sum();
        let rho = actual / predicted;
        if predicted > 0.0 && !rho.is_nan() {
            rho
        } else {
            f64::NEG_INFINITY
        }
    }
}

fn scaled(v: &[f64], s: f64) -> Vec<f64> {
    v.iter().map(|x| x * s).collect()
}

fn max_abs(r: &[f64]) -> f64 {
    r.iter().fold(0.0_f64, |m, v| m.max(v.abs()))
}
