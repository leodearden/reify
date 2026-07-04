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

#[cfg(test)]
mod tests {
    use crate::DimensionVector;

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
