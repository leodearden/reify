//! Trajectory stdlib module — `piecewise_polynomial` ctor and evaluator
//! intrinsics (evaluate_profile / _dot / _ddot, profile_duration).
//!
//! PRD: docs/prds/v0_3/trajectory-input-shaping.md §4.1, §11 Phase 1 β.

use reify_types::Value;

mod spline;

/// Evaluate a trajectory stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names
/// so that `eval_builtin` can fall through to the next module or `Value::Undef`.
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
