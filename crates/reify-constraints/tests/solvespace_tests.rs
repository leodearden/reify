//! Tests for SolveSpaceSolver — geometric constraint solving via libslvs FFI.

use reify_constraints::SolveSpaceSolver;
use reify_types::ConstraintSolver;

/// SolveSpaceSolver must be Send + Sync (required by ConstraintSolver trait).
#[test]
fn solvespace_solver_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SolveSpaceSolver>();
}

/// SolveSpaceSolver can be boxed as a trait object for ConstraintSolver.
#[test]
fn solvespace_solver_as_trait_object() {
    let solver = SolveSpaceSolver;
    let _boxed: Box<dyn ConstraintSolver> = Box::new(solver);
}
