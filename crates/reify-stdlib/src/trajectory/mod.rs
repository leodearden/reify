//! Trajectory stdlib module — `piecewise_polynomial` ctor and evaluator
//! intrinsics (evaluate_profile / _dot / _ddot, profile_duration).
//!
//! PRD: docs/prds/v0_3/trajectory-input-shaping.md §4.1, §11 Phase 1 β.

use reify_ir::Value;

mod gcode_import;
mod spline;

/// Evaluate a trajectory stdlib function by name.
///
/// Returns `Some(Value)` for known function names, or `None` for unknown names
/// so that `eval_builtin` can fall through to the next module.
///
/// Phase β: all recognized names unconditionally return `Some(Value::Undef)`.
/// The pure-Rust spline math is implemented in the `spline` submodule but is
/// not yet wired to the Value API.  Full marshalling (parsing a
/// `PiecewisePolynomialProfile` from `Value::StructureInstance`, dispatching on
/// the `BoundaryCondition` SIR type-tag, emitting `Value::List<Value::Real>`
/// per joint) is deferred to a later phase (γ/η/θ per the β PRD scope
/// boundary).  Callers that see `Value::Undef` here should treat it as a
/// "not yet implemented" stub, not a computation result.
pub(crate) fn eval_trajectory(name: &str, _args: &[Value]) -> Option<Value> {
    match name {
        "piecewise_polynomial"
        | "evaluate_profile"
        | "evaluate_profile_dot"
        | "evaluate_profile_ddot"
        | "profile_duration" => Some(Value::Undef),
        _ => None,
    }
}
