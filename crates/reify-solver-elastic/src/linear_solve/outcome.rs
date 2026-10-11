//! What a solve returns: its solution and report, or how it stopped.

use std::fmt;
use std::sync::Arc;

use super::control::SolvePhase;
use super::policy::{FactorEstimate, StrategyDecision};

/// `tolerance` is the PCG stopping tolerance on the relative recurrence
/// residual, or Direct's bound on the true relative residual; `max_iter`
/// caps PCG.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolveLimits {
    pub tolerance: f64,
    pub max_iter: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solution {
    pub u: Arc<Vec<f64>>,
    pub report: SolveReport,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolveReport {
    pub decision: StrategyDecision,
    pub n_dofs: usize,
    /// `Some` when the session ran the symbolic analysis.
    pub factor: Option<FactorEstimate>,
    /// PCG iterations; `None` for Direct.
    pub iterations: Option<u32>,
    /// `‖f − K·u‖/‖f‖` on every tier; 0 for a zero right-hand side.
    pub true_rel_residual: f64,
    pub reuse: FactorReuse,
}

/// Which earlier work a solve reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactorReuse {
    None,
    /// The factor an earlier solve of the same session created.
    SessionFactor,
    /// A numeric factor carried across evaluations.
    CachedAcrossEvaluations,
    /// A symbolic analysis carried across evaluations; numeric re-run.
    SymbolicAcrossEvaluations,
}

/// A solve that returned no solution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolveStop {
    /// The control callback broke on an event of `phase`.
    Cancelled {
        phase: SolvePhase,
    },
    /// PCG used its whole iteration cap.
    NotConverged {
        iterations: u32,
        cap: u32,
        true_rel_residual: f64,
        tolerance: f64,
    },
    /// Direct's true residual is above the tolerance.
    ResidualAboveTolerance {
        true_rel_residual: f64,
        tolerance: f64,
    },
    Failed(SolveFailure),
}

/// A solve that cannot proceed on this system under this policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolveFailure {
    NotPositiveDefinite {
        site: NpdSite,
    },
    /// An explicit Direct request whose predicted factor exceeds
    /// `SolvePolicy::direct_ceiling`; raised before any numeric allocation.
    FactorExceedsCeiling {
        predicted: u64,
        ceiling: u64,
    },
    /// A reservation of `requested` bytes failed. In `Analyse` the figure is
    /// the column-major copy of `K` (`8·(n+1) + 16·nnz`), which bounds the
    /// same O(n + nnz) as faer's unexposed symbolic workspace.
    OutOfMemory {
        requested: u64,
        phase: SolvePhase,
    },
}

/// Where a non-positive-definite `K` was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpdSite {
    /// Direct: faer's non-positive pivot, mapped back to its DOF.
    Pivot { dof: usize },
    /// PCG: `p·Kp ≤ 0` at 1-based `iteration`.
    Curvature { iteration: u32 },
}

/// A count as reported on the wire: saturating at `u32::MAX`, never a panic.
pub(crate) fn reported_count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<SolveFailure> for SolveStop {
    fn from(failure: SolveFailure) -> Self {
        Self::Failed(failure)
    }
}

impl fmt::Display for SolveStop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled { phase } => write!(f, "solve cancelled in the {phase:?} phase"),
            Self::NotConverged {
                iterations,
                cap,
                true_rel_residual,
                tolerance,
            } => write!(
                f,
                "PCG did not converge in {iterations} of {cap} iterations: true relative \
                 residual {true_rel_residual:e} against tolerance {tolerance:e}"
            ),
            Self::ResidualAboveTolerance {
                true_rel_residual,
                tolerance,
            } => write!(
                f,
                "direct solve's true relative residual {true_rel_residual:e} is above the \
                 tolerance {tolerance:e}"
            ),
            Self::Failed(failure) => failure.fmt(f),
        }
    }
}

impl fmt::Display for SolveFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPositiveDefinite {
                site: NpdSite::Pivot { dof },
            } => write!(
                f,
                "stiffness is not positive definite: non-positive Cholesky pivot at dof {dof}"
            ),
            Self::NotPositiveDefinite {
                site: NpdSite::Curvature { iteration },
            } => write!(
                f,
                "stiffness is not positive definite: PCG curvature p·Kp ≤ 0 at iteration \
                 {iteration}"
            ),
            Self::FactorExceedsCeiling { predicted, ceiling } => write!(
                f,
                "direct factor needs {predicted} bytes, over the {ceiling}-byte direct ceiling"
            ),
            Self::OutOfMemory { requested, phase } => write!(
                f,
                "out of memory reserving {requested} bytes in the {phase:?} phase"
            ),
        }
    }
}

impl std::error::Error for SolveStop {}

impl std::error::Error for SolveFailure {}
