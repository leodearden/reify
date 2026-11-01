//! Why a continuous solve failed — the typed cause behind a failed solve's
//! diagnostic (PRD `docs/prds/v0_6/geometry-algebra-solver-unification.md` §7.5).
//!
//! A solver produces one [`SolveFailureCause`]; every renderer reads the same
//! value, so the cause → [`DiagnosticCode`] policy ([`SolveFailureCause::code`])
//! and the numeric clause ([`Display`](std::fmt::Display)) each have one home.
//! All figures are in the solve's SCALED units: a scaled residual of `1` is a row
//! exactly at its own tolerance.

use std::fmt;

use reify_core::DiagnosticCode;

/// Why a continuous solve stopped without converging.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SolveFailureCause {
    /// Stopped at a stationary point of `‖r̃‖²` that is not a root — proven
    /// infeasible to first order.
    Infeasible(InfeasibilityCertificate),
    /// Used the whole iteration budget, neither converged nor certified
    /// infeasible.
    BudgetExhausted(BudgetExhaustion),
    /// The step collapsed at a point that is neither a root nor stationary (e.g.
    /// a kink) — a search failure, not a proof.
    Stalled(SolveStall),
}

impl SolveFailureCause {
    /// The diagnostic code every renderer of this cause uses.
    pub fn code(&self) -> DiagnosticCode {
        match self {
            Self::Infeasible(_) => DiagnosticCode::SolverInfeasible,
            Self::BudgetExhausted(_) => DiagnosticCode::SolverBudgetExhausted,
            Self::Stalled(_) => DiagnosticCode::ConstraintUnsatisfiable,
        }
    }
}

impl fmt::Display for SolveFailureCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Infeasible(cert) => cert.fmt(f),
            Self::BudgetExhausted(exhaustion) => exhaustion.fmt(f),
            Self::Stalled(stall) => stall.fmt(f),
        }
    }
}

/// The evidence that a solve stopped at a stationary non-root.
#[derive(Debug, Clone, PartialEq)]
pub struct InfeasibilityCertificate {
    /// `‖J̃ᵀr̃‖` at the stopping point.
    pub gradient_norm: f64,
    /// `‖J̃ᵀr̃‖ / (‖J̃‖_F·‖r̃‖)` — the cosine between `r̃` and the range of `J̃`;
    /// near zero only at a genuine stationary point.
    pub relative_gradient: f64,
    /// `max|r̃ᵢ|` at the stopping point (`> 1`).
    pub max_scaled_residual: f64,
    /// The rows still above their tolerance, largest `|r̃ᵢ|` first.
    pub blame: Vec<RowBlame>,
}

impl fmt::Display for InfeasibilityCertificate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the solve reached a least-squares minimum that is not a root: \
             ‖J̃ᵀr̃‖ = {:.3e} (relative gradient {:.2e}), largest scaled residual \
             {:.3e} over {} violated row(s)",
            self.gradient_norm,
            self.relative_gradient,
            self.max_scaled_residual,
            self.blame.len()
        )
    }
}

/// One residual row implicated in an infeasibility.
#[derive(Debug, Clone, PartialEq)]
pub struct RowBlame {
    /// What produced the row.
    pub source: ResidualSource,
    /// The row's index in the solve's residual vector.
    pub row: usize,
    /// The row's scaled residual `r̃ᵢ` at the stopping point.
    pub scaled_residual: f64,
}

/// What produced a residual row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResidualSource {
    /// A relation, by its index in the slice of relations the solve was given.
    Relation(usize),
}

/// The record of a solve that ran out of iterations.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetExhaustion {
    /// Trial steps taken, accepted or rejected.
    pub iterations: u32,
    /// The iteration budget.
    pub budget: u32,
    /// The scaled norm of each trial step, in order.
    pub step_norms: Vec<f64>,
    /// `max|r̃ᵢ|` at the last iterate.
    pub max_scaled_residual: f64,
}

impl fmt::Display for BudgetExhaustion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the solve used {} of {} iterations without converging",
            self.iterations, self.budget
        )?;
        if let (Some(first), Some(last)) = (self.step_norms.first(), self.step_norms.last()) {
            write!(
                f,
                "; step norms {first:.3e} … {last:.3e} (last/first {:.2e})",
                last / first
            )?;
        }
        write!(
            f,
            "; largest scaled residual {:.3e}",
            self.max_scaled_residual
        )
    }
}

/// The record of a solve whose step collapsed without a stationarity
/// certificate.
#[derive(Debug, Clone, PartialEq)]
pub struct SolveStall {
    /// Trial steps taken, accepted or rejected.
    pub iterations: u32,
    /// The scaled norm of the last trial step.
    pub step_norm: f64,
    /// The relative gradient at the stopping point (above the certificate's
    /// tolerance).
    pub relative_gradient: f64,
    /// `max|r̃ᵢ|` at the stopping point.
    pub max_scaled_residual: f64,
}

impl fmt::Display for SolveStall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the solve stalled after {} iterations: step norm {:.3e} at a point \
             that is not stationary (relative gradient {:.2e}); largest scaled \
             residual {:.3e}",
            self.iterations, self.step_norm, self.relative_gradient, self.max_scaled_residual
        )
    }
}
