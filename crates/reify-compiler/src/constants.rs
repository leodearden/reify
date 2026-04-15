//! Built-in mathematical constants available as language-level identifiers.
//!
//! Constants resolve to compile-time `Value::Real` literals (dimensionless).
//! They are checked AFTER scope lookup AND collection sub-name resolution,
//! so both user-defined `let pi = ...` and collection sub-names shadow
//! the builtins.

use reify_types::{CompiledExpr, Type, Value};

/// If `name` is a case-variant of a built-in constant (but not the exact
/// lowercase spelling), return the canonical lowercase name as a hint.
///
/// Returns `Some("pi")` for `"Pi"`, `"PI"`, `"pI"`, etc.
/// Returns `Some("tau")` for `"Tau"`, `"TAU"`, `"tAU"`, etc.
/// Returns `None` if the name already matches a builtin exactly (no hint needed)
/// or is not related to any builtin constant.
pub(crate) fn builtin_constant_hint(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    // Only hint when the lowercased version matches AND the original doesn't
    // (if the original matched, resolve_builtin_constant would have already succeeded).
    match lower.as_str() {
        "pi" if name != "pi" => Some("pi"),
        "tau" if name != "tau" => Some("tau"),
        _ => None,
    }
}

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
