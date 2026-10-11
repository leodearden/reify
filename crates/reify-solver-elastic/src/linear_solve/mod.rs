//! The tiered SPD linear-solve seam: one entry point that solves `K·u = f`
//! for a post-boundary-condition stiffness `K`, by a direct sparse Cholesky
//! or by preconditioned CG. Contract: PRD
//! `docs/prds/v0_6/elastic-static-solver-performance.md` §5 C1–C5.

mod control;
pub(crate) mod direct;
pub mod fault_injection;
mod outcome;
mod pcg;
mod policy;
mod session;
mod system;

pub use control::{SolveControl, SolveEvent, SolvePhase};
pub use outcome::{
    FactorReuse, NpdSite, Solution, SolveFailure, SolveLimits, SolveReport, SolveStop,
};
pub use policy::{
    FactorEstimate, LinearSolverChoice, PreconditionerKind, SolvePolicy, Strategy,
    StrategyDecision, StrategyReason, choose_strategy,
};
pub use session::SpdSolver;
pub use system::{SpdSystem, SystemInvalid};
