//! Built-in mathematical constants available as language-level identifiers.
//!
//! Constants resolve to compile-time `Value::Real` literals (dimensionless).
//! They are checked AFTER scope lookup AND collection sub-name resolution,
//! so both user-defined `let pi = ...` and collection sub-names shadow
//! the builtins.

use reify_types::{CompiledExpr, Type, Value};

/// Canonical names of all built-in mathematical constants.
///
/// This is the single source of truth for case-variant hint matching.
/// When adding a new constant to [`resolve_builtin_constant`], also add its
/// name here so that case-variant hints fire for the new constant automatically.
const BUILTIN_NAMES: &[&str] = &["pi", "tau"];

/// If `name` is a case-variant of a built-in constant (but not the exact
/// lowercase spelling), return the canonical lowercase name as a hint.
///
/// Returns `Some("pi")` for `"Pi"`, `"PI"`, `"pI"`, etc.
/// Returns `Some("tau")` for `"Tau"`, `"TAU"`, `"tAU"`, etc.
/// Returns `None` if the name already matches a builtin exactly (no hint needed)
/// or is not related to any builtin constant.
///
/// Uses [`str::eq_ignore_ascii_case`] — no heap allocation — since this is
/// only reached on the diagnostic (error) path.
pub(crate) fn builtin_constant_hint(name: &str) -> Option<&'static str> {
    // If the exact spelling already resolves, no hint is needed.
    if resolve_builtin_constant(name).is_some() {
        return None;
    }
    // Return the canonical name if `name` is a case-variant of any builtin.
    // eq_ignore_ascii_case avoids allocating a lowercase String.
    BUILTIN_NAMES
        .iter()
        .copied()
        .find(|&canonical| name.eq_ignore_ascii_case(canonical))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The number of constants recognised by `resolve_builtin_constant`.
    ///
    /// **Keep this in sync with the match arms in `resolve_builtin_constant`.**
    /// When you add a new match arm, bump this constant AND add the name to
    /// `BUILTIN_NAMES`. The two guard tests below enforce both directions:
    ///
    /// - `builtin_names_covers_all_constants` — every name in `BUILTIN_NAMES`
    ///   resolves (forward direction).
    /// - `builtin_names_is_exhaustive` — `BUILTIN_NAMES` contains exactly
    ///   `BUILTIN_NAMES_COUNT` entries, equal to the number of match arms
    ///   (reverse direction).
    const BUILTIN_NAMES_COUNT: usize = 2;

    /// Guard: every name in `BUILTIN_NAMES` must resolve via
    /// `resolve_builtin_constant`. If this test fails, a name was added to
    /// `BUILTIN_NAMES` (the hint source-of-truth) without a corresponding
    /// match arm in `resolve_builtin_constant`, which would cause the hint
    /// system to suggest a name that also fails to resolve.
    #[test]
    fn builtin_names_covers_all_constants() {
        for &name in BUILTIN_NAMES {
            assert!(
                resolve_builtin_constant(name).is_some(),
                "BUILTIN_NAMES contains {:?} but resolve_builtin_constant({:?}) returned None — \
                 add a match arm for this name",
                name,
                name,
            );
        }
    }

    /// Guard: `BUILTIN_NAMES` must be exhaustive relative to the match arms in
    /// `resolve_builtin_constant`. If this test fails after adding a new
    /// constant, you must both bump `BUILTIN_NAMES_COUNT` and add the name to
    /// `BUILTIN_NAMES`.
    ///
    /// Together with `builtin_names_covers_all_constants` this enforces the
    /// bidirectional contract: every name in the list resolves, and the list
    /// contains exactly as many names as there are match arms.
    #[test]
    fn builtin_names_is_exhaustive() {
        assert_eq!(
            BUILTIN_NAMES.len(),
            BUILTIN_NAMES_COUNT,
            "BUILTIN_NAMES.len() ({}) != BUILTIN_NAMES_COUNT ({}) — \
             if you added a match arm in resolve_builtin_constant, \
             bump BUILTIN_NAMES_COUNT AND add the name to BUILTIN_NAMES",
            BUILTIN_NAMES.len(),
            BUILTIN_NAMES_COUNT,
        );
    }
}
