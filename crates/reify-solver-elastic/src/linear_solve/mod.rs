//! The tiered SPD linear-solve seam: one entry point that solves `K·u = f`
//! for a post-boundary-condition stiffness `K`, by a direct sparse Cholesky
//! or by preconditioned CG. Contract: PRD
//! `docs/prds/v0_6/elastic-static-solver-performance.md` §5 C1–C4.

pub(crate) mod direct;
pub mod fault_injection;
mod outcome;
mod policy;
mod system;

pub use outcome::SolveFailure;
pub use policy::{
    FactorEstimate, LinearSolverChoice, PreconditionerKind, SolvePolicy, Strategy,
    StrategyDecision, StrategyReason, choose_strategy,
};
pub use system::{SpdSystem, SystemInvalid};
