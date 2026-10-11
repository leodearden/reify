//! A solve session: one system, its tier decision, and the tier's state
//! across solves — a factor built by the first solve that needs it, or
//! nothing for Jacobi-PCG.

use std::sync::Arc;

use faer::sparse::SparseColMat;

use super::control::{SolveControl, SolvePhase};
use super::direct::{self, DirectTier, Symbolic};
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

impl SpdSolver {
    /// Decides the tier. Auto and Direct run the symbolic analysis the
    /// decision needs; no numeric work happens until a non-zero solve.
    pub fn prepare(
        system: Arc<SpdSystem>,
        choice: LinearSolverChoice,
        policy: &SolvePolicy,
        ctl: &mut SolveControl<'_>,
    ) -> Result<Self, SolveStop> {
        if choice == LinearSolverChoice::Iterative {
            return Ok(Self {
                decision: choose_strategy(choice, None, policy)?,
                system,
                estimate: None,
                tier: Tier::Jacobi,
            });
        }
        ctl.enter(SolvePhase::Analyse)?;
        let (symbolic, csc) = analyse(&system)?;
        let estimate = symbolic.estimate();
        let decision = choose_strategy(choice, Some(&estimate), policy)?;
        let tier = match decision.strategy {
            Strategy::Direct => Tier::Direct(Box::new(DirectTier::new(symbolic, csc))),
            Strategy::Pcg(PreconditionerKind::Jacobi) => Tier::Jacobi,
            Strategy::Pcg(PreconditionerKind::SmoothedAggregation) => {
                unreachable!("choose_strategy names SmoothedAggregation only once its tier exists")
            }
        };
        Ok(Self {
            system,
            decision,
            estimate: Some(estimate),
            tier,
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

/// The symbolic analysis of `system`, with the column-major copy of `K` the
/// numeric factorisation reads.
fn analyse(system: &SpdSystem) -> Result<(Symbolic, SparseColMat<usize, f64>), SolveFailure> {
    let k = system.matrix();
    let out_of_memory = || SolveFailure::OutOfMemory {
        requested: column_major_bytes(k.ncols(), k.compute_nnz()),
        phase: SolvePhase::Analyse,
    };
    let csc = k.to_col_major().map_err(|_| out_of_memory())?;
    let symbolic = direct::analyse(csc.as_ref().symbolic()).map_err(|_| out_of_memory())?;
    Ok((symbolic, csc))
}

/// `8·(n+1) + 16·nnz`: the column pointers, and a row index and a value per
/// stored entry.
fn column_major_bytes(ncols: usize, nnz: usize) -> u64 {
    (ncols as u64 + 1)
        .saturating_mul(8)
        .saturating_add((nnz as u64).saturating_mul(16))
}
