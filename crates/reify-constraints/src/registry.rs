//! Solver registry for multi-domain constraint dispatch.
//!
//! Combines classification + decomposition to dispatch sub-problems
//! to domain-specific solvers.

use reify_types::{ConstraintSolver, ResolutionProblem, SolveResult};

/// A registry that dispatches constraint sub-problems to domain-specific solvers.
pub struct SolverRegistry {
    /// The fallback solver (used for all domains until specialized solvers are registered).
    fallback: Box<dyn ConstraintSolver>,
}

impl SolverRegistry {
    /// Create a new solver registry with a fallback solver.
    pub fn new(fallback: Box<dyn ConstraintSolver>) -> Self {
        Self { fallback }
    }
}

impl ConstraintSolver for SolverRegistry {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult {
        // Delegate to fallback until full implementation
        self.fallback.solve(problem)
    }
}

// Safety: SolverRegistry is Send + Sync because it only contains
// Box<dyn ConstraintSolver> which requires Send + Sync.
unsafe impl Send for SolverRegistry {}
unsafe impl Sync for SolverRegistry {}
