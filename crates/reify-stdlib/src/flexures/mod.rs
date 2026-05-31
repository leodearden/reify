//! Compliant-joint / flexure constructors (Compliant-Joints PRD, v0.3).
//!
//! This module hosts the Howell-style pseudo-rigid-body (PRB) flexure
//! constructors. Each builtin returns a joint `Value::Map` (kind
//! `"revolute"` / `"prismatic"`) extended with the flexure-specific keys
//! (`spring_rate`, `damping`, `neutral`, `pivot`) so a flexure plugs into the
//! mechanism / sweep / snapshot engines exactly like a plain joint — the
//! engines dispatch on the `kind` string and ignore the extra keys (PRD §8.2).
//!
//! Dispatch mirrors the other stdlib modules: [`eval_flexures`] returns
//! `Some(Value)` for a recognised flexure name (including `Some(Value::Undef)`
//! on validation failure) and `None` for any unknown name, so `eval_builtin`
//! can fall through to the next module.

use reify_ir::Value;

mod beam;

/// Evaluate a flexure stdlib function by name.
///
/// Returns `Some(Value)` for known flexure constructors (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_flexures(name: &str, args: &[Value]) -> Option<Value> {
    beam::eval_beam(name, args)
}
