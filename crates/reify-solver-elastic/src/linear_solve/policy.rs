//! The tier policy: which engine solves a system, decided from the caller's
//! choice, the symbolic factor estimate and the engine's budgets — and from
//! nothing else (no free memory, no load, no ledger state).

use super::outcome::SolveFailure;

const GIB: u64 = 1024 * 1024 * 1024;

/// The tier every iterative decision names. γ flips this one line to
/// `SmoothedAggregation`.
const ITERATIVE_TIER: Strategy = Strategy::Pcg(PreconditionerKind::Jacobi);

/// The caller's tier request (`ElasticOptions.linear_solver`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinearSolverChoice {
    Auto,
    Direct,
    Iterative,
}

/// Engine-configured budgets, in bytes of the predicted factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolvePolicy {
    /// Auto picks Direct only when the factor fits in this many bytes.
    pub auto_direct_budget: u64,
    /// An explicit Direct request fails when the factor exceeds this.
    pub direct_ceiling: u64,
    /// Auto picks Direct only when the predicted factorisation flops fit
    /// in this; `None` leaves the work term off.
    pub auto_direct_work_budget: Option<f64>,
}

impl Default for SolvePolicy {
    fn default() -> Self {
        Self {
            auto_direct_budget: 2 * GIB,
            direct_ceiling: 8 * GIB,
            auto_direct_work_budget: None,
        }
    }
}

/// What the symbolic analysis predicts the numeric factorisation costs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FactorEstimate {
    pub n_dofs: usize,
    /// Stored entries of the factor `L`.
    pub len_val: u64,
    /// `8·len_val` plus the numeric factorisation's scratch.
    pub factor_bytes: u64,
    pub predicted_flops: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreconditionerKind {
    Jacobi,
    SmoothedAggregation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Strategy {
    Direct,
    Pcg(PreconditionerKind),
}

/// Why [`choose_strategy`] picked its strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrategyReason {
    AutoWithinBudget {
        predicted_bytes: u64,
        budget: u64,
    },
    AutoOverBudget {
        predicted_bytes: u64,
        budget: u64,
    },
    AutoOverWorkBudget {
        predicted_flops: f64,
        budget: f64,
    },
    /// The caller named the tier.
    Requested,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrategyDecision {
    pub strategy: Strategy,
    pub reason: StrategyReason,
}

/// The one home of the tier policy (PRD §5 C4).
///
/// `Auto` and `Direct` need the symbolic analysis' `estimate`; calling them
/// without one is a contract violation and panics. `Iterative` ignores it.
pub fn choose_strategy(
    choice: LinearSolverChoice,
    estimate: Option<&FactorEstimate>,
    policy: &SolvePolicy,
) -> Result<StrategyDecision, SolveFailure> {
    let required = || {
        estimate.unwrap_or_else(|| {
            panic!("choose_strategy({choice:?}): {choice:?} requires the symbolic FactorEstimate")
        })
    };
    match choice {
        LinearSolverChoice::Iterative => Ok(decision(ITERATIVE_TIER, StrategyReason::Requested)),
        LinearSolverChoice::Direct => {
            let estimate = required();
            if estimate.factor_bytes <= policy.direct_ceiling {
                Ok(decision(Strategy::Direct, StrategyReason::Requested))
            } else {
                Err(SolveFailure::FactorExceedsCeiling {
                    predicted: estimate.factor_bytes,
                    ceiling: policy.direct_ceiling,
                })
            }
        }
        LinearSolverChoice::Auto => Ok(choose_auto(required(), policy)),
    }
}

fn choose_auto(estimate: &FactorEstimate, policy: &SolvePolicy) -> StrategyDecision {
    let predicted_bytes = estimate.factor_bytes;
    let budget = policy.auto_direct_budget;
    if predicted_bytes > budget {
        return decision(
            ITERATIVE_TIER,
            StrategyReason::AutoOverBudget {
                predicted_bytes,
                budget,
            },
        );
    }
    if let Some(work_budget) = policy.auto_direct_work_budget
        && estimate.predicted_flops > work_budget
    {
        return decision(
            ITERATIVE_TIER,
            StrategyReason::AutoOverWorkBudget {
                predicted_flops: estimate.predicted_flops,
                budget: work_budget,
            },
        );
    }
    decision(
        Strategy::Direct,
        StrategyReason::AutoWithinBudget {
            predicted_bytes,
            budget,
        },
    )
}

fn decision(strategy: Strategy, reason: StrategyReason) -> StrategyDecision {
    StrategyDecision { strategy, reason }
}
