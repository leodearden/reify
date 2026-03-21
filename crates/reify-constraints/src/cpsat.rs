//! CP-SAT solver: pure-Rust backtracking constraint solver for logical/discrete constraints.
//!
//! Handles boolean SAT, enum constraints, integer constraints, implications,
//! cardinality, and all-different via forward-checking backtracking search
//! with eval_expr as the constraint checker.

/// A discrete constraint solver using backtracking search with forward-checking.
///
/// Named CpSatSolver to match the OR-Tools CP-SAT interface from the task spec,
/// but implemented as a pure-Rust backtracking solver suitable for v0.1 problem sizes.
pub struct CpSatSolver;
