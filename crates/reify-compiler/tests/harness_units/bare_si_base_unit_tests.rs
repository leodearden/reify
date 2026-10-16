//! Bare `A`, `mol` and `cd` are declared in `stdlib/units.ri` (task #6515).
//!
//! si_units.rs generates the PREFIXED forms of these three SI bases (`mA`,
//! `kmol`, `mcd`, …) but has no empty prefix, so without a hand declaration
//! the bare symbols reached only the built-in fallback table. That fallback
//! answers a bare literal like `1A`, but a compound unit expression is
//! resolved against the unit registry alone, so `1A^2`, `2mol/m^3` and
//! `5cd/m^2` failed with `unknown unit`. These tests pin the declarations and
//! the compound resolution they unlock.
//!
//! Cross-file references here name SYMBOLS, never line numbers.

use crate::common::{
    assert_eq_rel, assert_simple_unit, stdlib_let_si_value, stdlib_param_si_value,
};
use reify_core::DimensionVector;

#[test]
fn bare_si_base_units_are_declared_in_std_units() {
    assert_simple_unit("A", DimensionVector::CURRENT, 1.0, 1e-12);
    assert_simple_unit("mol", DimensionVector::AMOUNT_OF_SUBSTANCE, 1.0, 1e-12);
    assert_simple_unit("cd", DimensionVector::LUMINOUS_INTENSITY, 1.0, 1e-12);
}

#[test]
fn compound_literals_naming_bare_si_bases_resolve_under_the_stdlib() {
    let (v, d) = stdlib_let_si_value("3A^2");
    assert_eq_rel(v, 3.0, 1e-12, "3A^2");
    assert_eq!(d, DimensionVector::CURRENT.mul(&DimensionVector::CURRENT));

    let (v, d) = stdlib_param_si_value("Voltage", "5kg*m^2/s^3/A");
    assert_eq_rel(v, 5.0, 1e-12, "5kg*m^2/s^3/A");
    assert_eq!(d, DimensionVector::VOLTAGE);

    let (v, d) = stdlib_param_si_value("Charge", "2A*s");
    assert_eq_rel(v, 2.0, 1e-12, "2A*s");
    assert_eq!(d, DimensionVector::CHARGE);

    let (v, d) = stdlib_let_si_value("2mol/m^3");
    assert_eq_rel(v, 2.0, 1e-12, "2mol/m^3");
    assert_eq!(
        d,
        DimensionVector::AMOUNT_OF_SUBSTANCE.div(&DimensionVector::VOLUME)
    );

    let (v, d) = stdlib_let_si_value("5cd/m^2");
    assert_eq_rel(v, 5.0, 1e-12, "5cd/m^2");
    assert_eq!(
        d,
        DimensionVector::LUMINOUS_INTENSITY.div(&DimensionVector::AREA)
    );
}
