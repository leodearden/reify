//! SolveSpace geometric constraint solver integration.
//!
//! Implements `ConstraintSolver` using the SolveSpace libslvs C library
//! via hand-written FFI bindings. Creates a fresh solver system per call
//! (stateless), making it trivially Send + Sync.

use reify_types::{ConstraintSolver, ResolutionProblem, SolveResult};

/// Geometric constraint solver backed by SolveSpace's libslvs.
///
/// Solves geometric constraints (point distances, angles, parallelism,
/// coincidence, etc.) by mapping Reify's `ResolutionProblem` to libslvs
/// entities and constraints, solving, then reading back results.
///
/// A fresh `Slvs_System` is created per `solve()` call — no internal
/// mutable state — so this type is trivially `Send + Sync`.
pub struct SolveSpaceSolver;

impl ConstraintSolver for SolveSpaceSolver {
    fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
        SolveResult::NoProgress {
            reason: "SolveSpace solver not yet implemented".to_string(),
        }
    }
}
