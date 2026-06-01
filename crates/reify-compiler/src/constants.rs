//! Built-in mathematical constants available as language-level identifiers.
//!
//! Constants resolve to compile-time `Value::Real` literals (dimensionless).
//! They are checked AFTER scope lookup AND collection sub-name resolution,
//! so both user-defined `let pi = ...` and collection sub-names shadow
//! the builtins.

use reify_core::Type;
use reify_ir::{CompiledExpr, Value};

/// Canonical names of all built-in mathematical constants.
///
/// This is the single source of truth for case-variant hint matching.
/// When adding a new constant to [`resolve_builtin_constant`], also add its
/// name here so that case-variant hints fire for the new constant automatically.
const BUILTIN_NAMES: &[&str] = &["pi", "tau", "e"];

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
        // e = Euler's number, dimensionless Real
        "e" => Some(CompiledExpr::literal(
            Value::Real(std::f64::consts::E),
            Type::Real,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guard: every name in `BUILTIN_NAMES` must resolve via
    /// `resolve_builtin_constant`. If this test fails, a name was added to
    /// `BUILTIN_NAMES` (the hint source-of-truth) without a corresponding
    /// match arm in `resolve_builtin_constant`, which would cause the hint
    /// system to suggest a name that also fails to resolve.
    ///
    /// Bidirectional exhaustiveness contract:
    ///
    /// - `builtin_names_covers_all_constants` (forward) — every name in
    ///   `BUILTIN_NAMES` resolves via `resolve_builtin_constant`.
    /// - `builtin_names_no_unlisted_resolvers` (reverse) — no plausible
    ///   constant name resolves unless it is also listed in `BUILTIN_NAMES`.
    ///   The probe set covers all `std::f64::consts` equivalents and common
    ///   mathematical/physical names; truly novel names outside the probe set
    ///   are not caught automatically.
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

    /// Guard (reverse direction): no plausible mathematical constant name
    /// should resolve via `resolve_builtin_constant` unless it is also listed
    /// in `BUILTIN_NAMES`.
    ///
    /// This test probes a comprehensive superset of names — all
    /// `std::f64::consts` lowercase equivalents plus common mathematical and
    /// physical constant names — and asserts that any name returning `Some`
    /// from `resolve_builtin_constant` is also present in `BUILTIN_NAMES`.
    ///
    /// If this test fails, a match arm was added to `resolve_builtin_constant`
    /// without adding the name to `BUILTIN_NAMES`; the hint system would then
    /// fail to suggest that constant for case-variant misspellings.
    ///
    /// **Limitation:** names that are entirely novel (outside this probe set)
    /// are not caught. The probe set covers all `std::f64::consts` variants and
    /// the most common mathematical/physical constants, which encompasses the
    /// realistic namespace of plausible additions.
    #[test]
    fn builtin_names_no_unlisted_resolvers() {
        // All std::f64::consts lowercase equivalents plus common
        // mathematical/physical names and the current builtins (pi, tau).
        const PROBE: &[&str] = &[
            // current builtins
            "pi",
            "tau",
            // std::f64::consts equivalents
            "e",
            "frac_1_pi",
            "frac_1_sqrt_2",
            "frac_2_pi",
            "frac_2_sqrt_pi",
            "frac_pi_2",
            "frac_pi_3",
            "frac_pi_4",
            "frac_pi_6",
            "frac_pi_8",
            "ln_2",
            "ln_10",
            "log2_e",
            "log2_10",
            "log10_2",
            "log10_e",
            "sqrt_2",
            // common mathematical / physical names
            "phi",
            "golden_ratio",
            "euler",
            "avogadro",
            "boltzmann",
            "planck",
            "speed_of_light",
            "gravity",
            "infinity",
            "nan",
            // If you add a constant outside this list, add its name here too.
        ];

        for &name in PROBE {
            if resolve_builtin_constant(name).is_some() {
                assert!(
                    BUILTIN_NAMES.contains(&name),
                    "resolve_builtin_constant({:?}) returned Some but {:?} is not in \
                     BUILTIN_NAMES — add the name to BUILTIN_NAMES so the hint system \
                     can suggest it for case-variant misspellings",
                    name,
                    name,
                );
            }
        }
    }
}
