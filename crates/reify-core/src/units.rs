//! Built-in unit-symbol → SI conversion table.
//!
//! Extracted from `reify-compiler::units::unit_to_scalar` (task #4535) so that
//! reify-stdlib's runtime `parse_length`/`parse_length_r` and reify-compiler's
//! quantity-literal handling share exactly one physical table instead of
//! risking two independently-diverging copies. Returns pure core types
//! (`f64` SI scale factor + [`crate::DimensionVector`]) — no `Value`
//! coupling — so this stays a leaf module that both compiler and stdlib can
//! depend on without inverting the crate DAG (see the B1 invariant note in
//! `lib.rs`).

use crate::DimensionVector;

/// Look up a built-in unit symbol's SI conversion factor and dimension.
///
/// Returns `Some((factor, dimension))` such that `si_value = value * factor`,
/// or `None` if `unit` is not one of the hardcoded built-in symbols.
///
/// This is the single source of truth copied verbatim (values and all) from
/// the pre-extraction `reify-compiler::units::unit_to_scalar` match arms;
/// that function now delegates here. Does not resolve user-declared units
/// (e.g. `km`, `ft`, `thou`) — those live only in the compiler's per-module
/// `UnitRegistry`, which has no equivalent at this layer.
pub fn unit_symbol_to_si(unit: &str) -> Option<(f64, DimensionVector)> {
    match unit {
        "mm" => Some((0.001, DimensionVector::LENGTH)),
        "cm" => Some((0.01, DimensionVector::LENGTH)),
        "m" => Some((1.0, DimensionVector::LENGTH)),
        "in" => Some((0.0254, DimensionVector::LENGTH)),
        "deg" => Some((std::f64::consts::PI / 180.0, DimensionVector::ANGLE)),
        "rad" => Some((1.0, DimensionVector::ANGLE)),
        "kg" => Some((1.0, DimensionVector::MASS)),
        "g" => Some((0.001, DimensionVector::MASS)),
        "s" => Some((1.0, DimensionVector::TIME)),
        // Kelvin needs a hardcoded fallback because `std.units` itself uses
        // `1K` in `BOLTZMANN_CONSTANT()`s body — fn bodies in std.units load
        // with no unit_registry seeded, so the K declared at units.ri can't
        // satisfy the same file's own quantity literals. Mirrors the kg/s/m
        // self-bootstrap entries above.
        "K" => Some((1.0, DimensionVector::TEMPERATURE)),
        // Bare SI base units completing the standard set (factor 1.0).
        // A/mol/cd are the SI bases for Current/AmountOfSubstance/LuminousIntensity;
        // they need the same hardcoded fallback as kg/s/K so that stdlib fn bodies
        // and other unseeded-registry scopes can resolve these unit literals
        // (PRD §2.2 / decision D5).
        "A" => Some((1.0, DimensionVector::CURRENT)),
        "mol" => Some((1.0, DimensionVector::AMOUNT_OF_SUBSTANCE)),
        "cd" => Some((1.0, DimensionVector::LUMINOUS_INTENSITY)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mm_converts_to_length_with_milli_factor() {
        let (factor, dim) = unit_symbol_to_si("mm").expect("mm should be recognized");
        assert!((factor - 0.001).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::LENGTH);
    }

    #[test]
    fn cm_converts_to_length_with_centi_factor() {
        let (factor, dim) = unit_symbol_to_si("cm").expect("cm should be recognized");
        assert!((factor - 0.01).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::LENGTH);
    }

    #[test]
    fn m_converts_to_length_with_unit_factor() {
        let (factor, dim) = unit_symbol_to_si("m").expect("m should be recognized");
        assert!((factor - 1.0).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::LENGTH);
    }

    #[test]
    fn in_converts_to_length_with_inch_factor() {
        let (factor, dim) = unit_symbol_to_si("in").expect("in should be recognized");
        assert!((factor - 0.0254).abs() < 1e-9);
        assert_eq!(dim, DimensionVector::LENGTH);
    }

    #[test]
    fn kg_converts_to_mass_with_unit_factor() {
        let (factor, dim) = unit_symbol_to_si("kg").expect("kg should be recognized");
        assert!((factor - 1.0).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::MASS);
    }

    #[test]
    fn deg_converts_to_angle_with_pi_over_180_factor() {
        let (factor, dim) = unit_symbol_to_si("deg").expect("deg should be recognized");
        assert!((factor - std::f64::consts::PI / 180.0).abs() < 1e-12);
        assert_eq!(dim, DimensionVector::ANGLE);
    }

    #[test]
    fn bogus_unit_is_unrecognized() {
        assert_eq!(unit_symbol_to_si("bogus"), None);
    }
}
