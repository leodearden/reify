//! Whether an engine resolves `auto` parameters, named at construction.
//!
//! Every `Engine` constructor takes a [`SolverPosture`] as its leading
//! argument, so no construction path is solver-free by omission
//! (`docs/prds/v0_6/solver-driver-parity.md` §4.1 I1). A solving engine always
//! starts from the production solver registry, never a bare solver (§4.2 I2).

use reify_constraints::SolverRegistry;
use reify_ir::{ConstraintSolver, ResolutionProfile};

/// How an engine is wired to resolve `auto` parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverPosture {
    /// Resolve through the production solver registry under this profile.
    Solve(ResolutionProfile),
    /// Resolve nothing: every `auto` parameter stays unresolved.
    SolverFree,
}

/// A solving engine's profile and the solver that serves it, owned together so
/// an engine holds a solver iff it holds a profile.
pub(crate) struct SolveStage {
    profile: ResolutionProfile,
    solver: Box<dyn ConstraintSolver>,
}

impl SolveStage {
    /// The stage a `posture` calls for: the production registry under its
    /// profile, or none for [`SolverPosture::SolverFree`].
    pub(crate) fn for_posture(posture: SolverPosture) -> Option<Self> {
        match posture {
            SolverPosture::Solve(profile) => Some(Self {
                profile,
                solver: Box::new(SolverRegistry::production()),
            }),
            SolverPosture::SolverFree => None,
        }
    }

    pub(crate) fn profile(&self) -> ResolutionProfile {
        self.profile
    }

    pub(crate) fn solver(&self) -> &dyn ConstraintSolver {
        self.solver.as_ref()
    }

    pub(crate) fn replace_solver(&mut self, solver: Box<dyn ConstraintSolver>) {
        self.solver = solver;
    }
}
