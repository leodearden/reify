//! Beam-flexure PRB constructors (Howell §5): cantilever beam (revolute) and
//! fixed-fixed beam (transverse prismatic).
//!
//! Scaffold stub — the constructor arms land in the γ implementation steps.

use reify_ir::Value;

/// Evaluate a beam-flexure constructor by name.
///
/// Returns `None` for every name until the constructor arms land, so all
/// flexure names fall through to `Value::Undef` in `eval_builtin`.
pub(crate) fn eval_beam(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod tests {}
