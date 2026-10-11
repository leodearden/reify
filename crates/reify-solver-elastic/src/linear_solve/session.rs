//! A solve session: one system, its tier decision, and the tier's state
//! across solves — a factor built by the first solve that needs it, or
//! nothing for Jacobi-PCG.

use std::sync::Arc;

use faer::sparse::{FaerError, SparseColMat};

use super::control::{SolveControl, SolvePhase};
use super::direct::{self, DirectTier, F64_BYTES, Symbolic};
use super::outcome::{FactorReuse, Solution, SolveFailure, SolveLimits, SolveReport, SolveStop};
use super::pcg::{self, PcgOutcome};
use super::policy::{
    FactorEstimate, LinearSolverChoice, PreconditionerKind, SolvePolicy, Strategy,
    StrategyDecision, choose_strategy,
};
use super::system::SpdSystem;
use crate::solver::norm2_squared;

pub struct SpdSolver {
    system: Arc<SpdSystem>,
    decision: StrategyDecision,
    estimate: Option<FactorEstimate>,
    tier: Tier,
}

enum Tier {
    Direct(Box<DirectTier>),
    Jacobi,
}

/// What `prepare`'s symbolic analysis produced.
struct Analysis {
    symbolic: Symbolic,
    /// The column-major copy of `K` the numeric factorisation reads.
    csc: SparseColMat<usize, f64>,
    estimate: FactorEstimate,
}

impl Tier {
    /// The tier `strategy` names — the decision is the only source of the
    /// tier on every path — built from `prepare`'s analysis, if one ran.
    fn for_strategy(strategy: Strategy, analysis: Option<Analysis>) -> Self {
        match strategy {
            Strategy::Direct => {
                let Analysis { symbolic, csc, .. } =
                    analysis.expect("choose_strategy names Direct only from a symbolic estimate");
                Self::Direct(Box::new(DirectTier::new(symbolic, csc)))
            }
            Strategy::Pcg(PreconditionerKind::Jacobi) => Self::Jacobi,
            Strategy::Pcg(PreconditionerKind::SmoothedAggregation) => {
                unreachable!("choose_strategy names SmoothedAggregation only once its tier exists")
            }
        }
    }
}

impl SpdSolver {
    /// Decides the tier. Auto and Direct run the symbolic analysis the
    /// decision needs; no numeric work happens until a non-zero solve.
    pub fn prepare(
        system: Arc<SpdSystem>,
        choice: LinearSolverChoice,
        policy: &SolvePolicy,
        ctl: &mut SolveControl<'_>,
    ) -> Result<Self, SolveStop> {
        let analysis = match choice {
            LinearSolverChoice::Iterative => None,
            LinearSolverChoice::Auto | LinearSolverChoice::Direct => {
                ctl.enter(SolvePhase::Analyse)?;
                Some(analyse(&system)?)
            }
        };
        let estimate = analysis.as_ref().map(|analysis| analysis.estimate);
        let decision = choose_strategy(choice, estimate.as_ref(), policy)?;
        Ok(Self {
            system,
            decision,
            estimate,
            tier: Tier::for_strategy(decision.strategy, analysis),
        })
    }

    pub fn decision(&self) -> &StrategyDecision {
        &self.decision
    }

    /// Solves `K·u = rhs`. A zero `rhs` returns `u = 0` without touching the
    /// tier. `warm` seeds PCG and is ignored by Direct.
    ///
    /// # Panics
    /// On a contract violation: `rhs` or `warm` not sized to the system, a
    /// tolerance that is not finite and positive, or `max_iter == 0` on PCG.
    pub fn solve(
        &mut self,
        rhs: &[f64],
        limits: SolveLimits,
        warm: Option<&[f64]>,
        ctl: &mut SolveControl<'_>,
    ) -> Result<Solution, SolveStop> {
        let n = self.system.n_dofs();
        assert_eq!(
            rhs.len(),
            n,
            "SpdSolver::solve: rhs.len() = {} but the system has {n} dofs",
            rhs.len()
        );
        assert!(
            limits.tolerance.is_finite() && limits.tolerance > 0.0,
            "SpdSolver::solve: tolerance = {} must be finite and positive",
            limits.tolerance
        );
        if let Some(warm) = warm {
            assert_eq!(
                warm.len(),
                n,
                "SpdSolver::solve: warm.len() = {} but the system has {n} dofs",
                warm.len()
            );
        }
        if norm2_squared(rhs) == 0.0 {
            let iterations = match self.tier {
                Tier::Direct(_) => None,
                Tier::Jacobi => Some(0),
            };
            return Ok(self.solution(vec![0.0; n], iterations, 0.0, FactorReuse::None));
        }
        match &mut self.tier {
            Tier::Direct(direct) => {
                let (u, reuse) = direct.solve(rhs, ctl)?;
                ctl.enter(SolvePhase::Verify)?;
                let true_rel_residual = self.system.relative_residual(rhs, &u);
                if true_rel_residual <= limits.tolerance {
                    Ok(self.solution(u, None, true_rel_residual, reuse))
                } else {
                    Err(SolveStop::ResidualAboveTolerance {
                        true_rel_residual,
                        tolerance: limits.tolerance,
                    })
                }
            }
            Tier::Jacobi => {
                assert!(
                    limits.max_iter >= 1,
                    "SpdSolver::solve: max_iter = 0 on the iterative tier; must be >= 1"
                );
                let outcome = pcg::solve(&self.system, rhs, limits, warm, ctl)?;
                ctl.enter(SolvePhase::Verify)?;
                match outcome {
                    PcgOutcome::Converged { u, iterations } => {
                        let true_rel_residual = self.system.relative_residual(rhs, &u);
                        Ok(
                            self.solution(
                                u,
                                Some(iterations),
                                true_rel_residual,
                                FactorReuse::None,
                            ),
                        )
                    }
                    PcgOutcome::CapReached { u, cap } => Err(SolveStop::NotConverged {
                        iterations: cap,
                        cap,
                        true_rel_residual: self.system.relative_residual(rhs, &u),
                        tolerance: limits.tolerance,
                    }),
                }
            }
        }
    }

    fn solution(
        &self,
        u: Vec<f64>,
        iterations: Option<u32>,
        true_rel_residual: f64,
        reuse: FactorReuse,
    ) -> Solution {
        Solution {
            u: Arc::new(u),
            report: SolveReport {
                decision: self.decision,
                n_dofs: self.system.n_dofs(),
                factor: self.estimate,
                iterations,
                true_rel_residual,
                reuse,
            },
        }
    }
}

const INDEX_BYTES: u64 = size_of::<usize>() as u64;

/// The symbolic analysis of `system`.
fn analyse(system: &SpdSystem) -> Result<Analysis, SolveFailure> {
    let k = system.matrix();
    let failure = |error| analyse_failure(error, column_major_bytes(k.ncols(), k.compute_nnz()));
    let csc = k.to_col_major().map_err(failure)?;
    let symbolic = direct::analyse(csc.as_ref().symbolic()).map_err(failure)?;
    Ok(Analysis {
        estimate: symbolic.estimate(),
        symbolic,
        csc,
    })
}

/// The typed failure for a `FaerError` of the analysis, whose reservations
/// are bounded by the column-major copy's `column_major_bytes`.
fn analyse_failure(error: FaerError, column_major_bytes: u64) -> SolveFailure {
    match error {
        FaerError::OutOfMemory => SolveFailure::OutOfMemory {
            requested: column_major_bytes,
            phase: SolvePhase::Analyse,
        },
        FaerError::IndexOverflow => SolveFailure::IndexOverflow,
        other => unreachable!("faer 0.24's FaerError has no variant {other:?}"),
    }
}

/// The column-major copy's size: `n+1` column pointers, and a row index and
/// a value per stored entry.
fn column_major_bytes(ncols: usize, nnz: usize) -> u64 {
    (ncols as u64 + 1)
        .saturating_mul(INDEX_BYTES)
        .saturating_add((nnz as u64).saturating_mul(INDEX_BYTES + F64_BYTES))
}
