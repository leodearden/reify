//! Built-in mathematical constants available as language-level identifiers.
//!
//! Constants resolve to compile-time `Value::Real` literals (dimensionless).
//! They are checked AFTER scope lookup AND collection sub-name resolution,
//! so both user-defined `let pi = ...` and collection sub-names shadow
//! the builtins.

use reify_types::{CompiledExpr, Type, Value};

/// If `name` is a built-in mathematical constant, return the corresponding
/// `CompiledExpr` literal. Returns `None` for unrecognized names.
pub(crate) fn resolve_builtin_constant(name: &str) -> Option<CompiledExpr> {
    match name {
        "pi" => Some(CompiledExpr::literal(
            Value::Real(std::f64::consts::PI),
            Type::Real,
        )),
        "tau" => Some(CompiledExpr::literal(
            Value::Real(std::f64::consts::TAU),
            Type::Real,
        )),
        _ => None,
    }
}
