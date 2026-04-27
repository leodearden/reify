use std::fmt;
use std::ops::{Add, Neg, Sub};

use crate::ContentHash;

/// Rational number as i16/i16 for dimension exponents.
/// Uses i16 to prevent overflow when multiplying exponents (max i8 * i8 = 16,129 < i16::MAX).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rational {
    num: i16,
    den: i16,
}

impl Rational {
    pub const ZERO: Rational = Rational::new(0, 1);
    pub const ONE: Rational = Rational::new(1, 1);

    pub const fn new(num: i16, den: i16) -> Self {
        assert!(den != 0, "denominator must not be zero");
        let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i16;
        let sign = if den < 0 { -1 } else { 1 };
        Rational {
            num: sign * num / g,
            den: sign * den / g,
        }
    }

    pub const fn num(&self) -> i16 {
        self.num
    }

    pub const fn den(&self) -> i16 {
        self.den
    }

    pub fn is_zero(self) -> bool {
        self.num() == 0
    }

    pub fn is_integer(self) -> bool {
        self.den() == 1 || self.num() % self.den() == 0
    }

    pub fn as_i8(self) -> Option<i8> {
        let val = if self.den() == 1 {
            self.num()
        } else if self.num() % self.den() == 0 {
            self.num() / self.den()
        } else {
            return None;
        };
        i8::try_from(val).ok()
    }
}

impl Add for Rational {
    type Output = Rational;
    fn add(self, rhs: Rational) -> Rational {
        Rational::new(
            self.num() * rhs.den() + rhs.num() * self.den(),
            self.den() * rhs.den(),
        )
    }
}

impl Sub for Rational {
    type Output = Rational;
    fn sub(self, rhs: Rational) -> Rational {
        Rational::new(
            self.num() * rhs.den() - rhs.num() * self.den(),
            self.den() * rhs.den(),
        )
    }
}

impl Neg for Rational {
    type Output = Rational;
    fn neg(self) -> Rational {
        Rational::new(-self.num(), self.den())
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den() == 1 {
            write!(f, "{}", self.num())
        } else {
            write!(f, "{}/{}", self.num(), self.den())
        }
    }
}

const fn gcd(mut a: u16, mut b: u16) -> u16 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Dimension vector: 10 rational exponents for the SI base dimensions plus Money.
///
/// Index mapping:
/// 0: Length (m)
/// 1: Mass (kg)
/// 2: Time (s)
/// 3: Electric current (A)
/// 4: Temperature (K)
/// 5: Amount of substance (mol)
/// 6: Luminous intensity (cd)
/// 7: Angle (rad, treated as dimension for engineering use)
/// 8: Solid angle (sr)
/// 9: Money (USD)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DimensionVector(pub [Rational; 10]);

impl DimensionVector {
    pub const DIMENSIONLESS: DimensionVector = DimensionVector([Rational::ZERO; 10]);
    pub const LENGTH: DimensionVector = DimensionVector::basis(0);
    pub const MASS: DimensionVector = DimensionVector::basis(1);
    pub const TIME: DimensionVector = DimensionVector::basis(2);
    pub const CURRENT: DimensionVector = DimensionVector::basis(3);
    pub const TEMPERATURE: DimensionVector = DimensionVector::basis(4);
    pub const AMOUNT_OF_SUBSTANCE: DimensionVector = DimensionVector::basis(5);
    pub const LUMINOUS_INTENSITY: DimensionVector = DimensionVector::basis(6);
    pub const ANGLE: DimensionVector = DimensionVector::basis(7);
    pub const SOLID_ANGLE: DimensionVector = DimensionVector::basis(8);
    pub const MONEY: DimensionVector = DimensionVector::basis(9);

    /// Common derived dimensions.
    pub const AREA: DimensionVector = DimensionVector::basis_n(0, 2);
    pub const VOLUME: DimensionVector = DimensionVector::basis_n(0, 3);

    // Derived SI dimensions — built at const-eval time via direct exponent arrays.
    //
    // Index layout reminder (see struct doc):
    //   0:Length  1:Mass  2:Time  3:Current  4:Temperature
    //   5:AmountOfSubstance  6:LuminousIntensity  7:Angle  8:SolidAngle  9:Money

    /// Frequency: s⁻¹
    pub const FREQUENCY: DimensionVector = DimensionVector::from_exps(&[(2, -1)]);
    /// Force: kg·m·s⁻² (same as module-scope `FORCE` — kept in parallel for ergonomics).
    pub const FORCE: DimensionVector =
        DimensionVector::from_exps(&[(0, 1), (1, 1), (2, -2)]);
    /// Energy: kg·m²·s⁻²
    pub const ENERGY: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -2)]);
    /// Power: kg·m²·s⁻³
    pub const POWER: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -3)]);
    /// Pressure: kg·m⁻¹·s⁻²
    pub const PRESSURE: DimensionVector =
        DimensionVector::from_exps(&[(0, -1), (1, 1), (2, -2)]);
    /// Voltage (electric potential): kg·m²·s⁻³·A⁻¹
    pub const VOLTAGE: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -3), (3, -1)]);
    /// Charge: s·A
    pub const CHARGE: DimensionVector = DimensionVector::from_exps(&[(2, 1), (3, 1)]);
    /// Capacitance: kg⁻¹·m⁻²·s⁴·A²
    pub const CAPACITANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, -2), (1, -1), (2, 4), (3, 2)]);
    /// Resistance: kg·m²·s⁻³·A⁻²
    pub const RESISTANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -3), (3, -2)]);
    /// Conductance: kg⁻¹·m⁻²·s³·A²
    pub const CONDUCTANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, -2), (1, -1), (2, 3), (3, 2)]);
    /// Inductance: kg·m²·s⁻²·A⁻²
    pub const INDUCTANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -2), (3, -2)]);
    /// Magnetic flux: kg·m²·s⁻²·A⁻¹
    pub const MAGNETIC_FLUX: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (1, 1), (2, -2), (3, -1)]);
    /// Magnetic flux density: kg·s⁻²·A⁻¹
    pub const MAGNETIC_FLUX_DENSITY: DimensionVector =
        DimensionVector::from_exps(&[(1, 1), (2, -2), (3, -1)]);
    /// Luminous flux: cd·sr
    pub const LUMINOUS_FLUX: DimensionVector =
        DimensionVector::from_exps(&[(6, 1), (8, 1)]);
    /// Illuminance: cd·sr·m⁻²
    pub const ILLUMINANCE: DimensionVector =
        DimensionVector::from_exps(&[(0, -2), (6, 1), (8, 1)]);
    /// Absorbed dose of ionising radiation: m²·s⁻² (also Sievert for equivalent dose).
    pub const ABSORBED_DOSE: DimensionVector =
        DimensionVector::from_exps(&[(0, 2), (2, -2)]);
    /// Angular velocity: rad·s⁻¹
    pub const ANGULAR_VELOCITY: DimensionVector =
        DimensionVector::from_exps(&[(2, -1), (7, 1)]);
    /// Dynamic viscosity: kg·m⁻¹·s⁻¹
    pub const DYNAMIC_VISCOSITY: DimensionVector =
        DimensionVector::from_exps(&[(0, -1), (1, 1), (2, -1)]);

    const fn basis(index: usize) -> DimensionVector {
        let mut v = [Rational::ZERO; 10];
        v[index] = Rational::ONE;
        DimensionVector(v)
    }

    const fn basis_n(index: usize, n: i16) -> DimensionVector {
        let mut v = [Rational::ZERO; 10];
        v[index] = Rational::new(n, 1);
        DimensionVector(v)
    }

    /// Build a `DimensionVector` from `(index, integer_exponent)` pairs at
    /// const-eval time. Intended for concise declaration of derived-dimension
    /// constants (e.g. `ENERGY`, `VOLTAGE`).
    const fn from_exps(entries: &[(usize, i16)]) -> DimensionVector {
        let mut v = [Rational::ZERO; 10];
        let mut i = 0;
        while i < entries.len() {
            let (idx, e) = entries[i];
            v[idx] = Rational::new(e, 1);
            i += 1;
        }
        DimensionVector(v)
    }

    /// Return the canonical user-facing name for this dimension, if it matches
    /// exactly one of the named singleton constants.
    ///
    /// Performs a linear scan over [`NAMED_DIMENSIONS`], the single source-of-truth
    /// table shared with `resolve_dimension_type` in
    /// `crates/reify-compiler/src/type_resolution.rs`.
    ///
    /// Returns `Some("Money")`, `Some("Force")`, etc. for each named singleton in the table.
    ///
    /// Returns `None` for:
    /// - `DIMENSIONLESS` — intentionally excluded from the table; callers handle it specially.
    /// - Composite/derived dimensions that are not in the named set (e.g. `MONEY/MASS`).
    pub fn canonical_name(&self) -> Option<&'static str> {
        NAMED_DIMENSIONS
            .iter()
            .find(|(dim, _)| *dim == *self)
            .map(|(_, name)| *name)
    }

    pub fn is_dimensionless(&self) -> bool {
        self.0.iter().all(|r| r.is_zero())
    }

    /// Multiply dimensions (add exponents).
    pub fn mul(&self, other: &DimensionVector) -> DimensionVector {
        let mut result = [Rational::ZERO; 10];
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = self.0[i] + other.0[i];
        }
        DimensionVector(result)
    }

    /// Divide dimensions (subtract exponents).
    pub fn div(&self, other: &DimensionVector) -> DimensionVector {
        let mut result = [Rational::ZERO; 10];
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = self.0[i] - other.0[i];
        }
        DimensionVector(result)
    }

    /// Take the nth root (divide all exponents by n).
    ///
    /// E.g., `AREA.root(2)` produces LENGTH (halves exponent from 2 to 1).
    /// Fractional exponents are representable via `Rational`.
    pub fn root(&self, n: i8) -> DimensionVector {
        assert!(n != 0, "root degree must not be zero");
        let n = n as i16;
        let mut result = [Rational::ZERO; 10];
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = Rational::new(self.0[i].num(), self.0[i].den() * n);
        }
        DimensionVector(result)
    }

    /// Raise to an integer power (multiply all exponents).
    pub fn pow(&self, n: i8) -> DimensionVector {
        let mut result = [Rational::ZERO; 10];
        let n = n as i16;
        let nr = Rational::new(n, 1);
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = Rational::new(self.0[i].num() * nr.num(), self.0[i].den() * nr.den());
        }
        DimensionVector(result)
    }

    /// Convert an SI value to the standard engineering display unit for this dimension.
    ///
    /// Returns `(converted_value, unit_label)`. For example, LENGTH converts
    /// metres to millimetres: `to_display_units(0.08)` → `(80.0, "mm")`.
    pub fn to_display_units(&self, si_value: f64) -> (f64, &'static str) {
        if *self == DimensionVector::LENGTH {
            (si_value * 1000.0, "mm")
        } else if *self == DimensionVector::ANGLE {
            (si_value * 180.0 / std::f64::consts::PI, "deg")
        } else if *self == DimensionVector::AREA {
            (si_value * 1e6, "mm\u{00B2}")
        } else if *self == DimensionVector::VOLUME {
            (si_value * 1e9, "mm\u{00B3}")
        } else if *self == DimensionVector::MONEY {
            (si_value, "USD")
        } else if self.is_dimensionless() {
            (si_value, "")
        } else {
            (si_value, "SI")
        }
    }

    pub fn content_hash(&self) -> ContentHash {
        let mut buf = [0u8; 40]; // 10 * 4 bytes (2 bytes per i16 field)
        for (i, r) in self.0.iter().enumerate() {
            let num_bytes = r.num().to_le_bytes();
            let den_bytes = r.den().to_le_bytes();
            buf[i * 4] = num_bytes[0];
            buf[i * 4 + 1] = num_bytes[1];
            buf[i * 4 + 2] = den_bytes[0];
            buf[i * 4 + 3] = den_bytes[1];
        }
        ContentHash::of(&buf)
    }
}

/// FORCE dimension: kg·m·s⁻²
pub const FORCE: DimensionVector = {
    let mut v = [Rational::ZERO; 10];
    v[0] = Rational::ONE; // length
    v[1] = Rational::ONE; // mass
    v[2] = Rational::new(-2, 1); // time^-2
    DimensionVector(v)
};

/// Single source-of-truth mapping from named singleton `DimensionVector` constants to their
/// canonical PascalCase user-facing names.
///
/// This table drives both:
/// - [`DimensionVector::canonical_name`] — dimension → name direction (linear scan forward).
/// - `resolve_dimension_type` in `crates/reify-compiler/src/type_resolution.rs` — name → dimension
///   direction (linear scan backward).
///
/// **`DIMENSIONLESS` is intentionally excluded.** `canonical_name` returns `None` for
/// `DIMENSIONLESS` via the search-miss path (the existing contract), while `resolve_dimension_type`
/// special-cases `"Dimensionless" => DimensionVector::DIMENSIONLESS` as a separate fallback arm.
///
/// The slice contains exactly 30 entries, one per named singleton, in the same order as the
/// original `canonical_name` match arms (LENGTH .. DYNAMIC_VISCOSITY).
pub static NAMED_DIMENSIONS: &[(DimensionVector, &str)] = &[
    (DimensionVector::LENGTH, "Length"),
    (DimensionVector::MASS, "Mass"),
    (DimensionVector::TIME, "Time"),
    (DimensionVector::CURRENT, "Current"),
    (DimensionVector::TEMPERATURE, "Temperature"),
    (DimensionVector::AMOUNT_OF_SUBSTANCE, "AmountOfSubstance"),
    (DimensionVector::LUMINOUS_INTENSITY, "LuminousIntensity"),
    (DimensionVector::ANGLE, "Angle"),
    (DimensionVector::SOLID_ANGLE, "SolidAngle"),
    (DimensionVector::MONEY, "Money"),
    (DimensionVector::AREA, "Area"),
    (DimensionVector::VOLUME, "Volume"),
    (DimensionVector::FREQUENCY, "Frequency"),
    (DimensionVector::FORCE, "Force"),
    (DimensionVector::ENERGY, "Energy"),
    (DimensionVector::POWER, "Power"),
    (DimensionVector::PRESSURE, "Pressure"),
    (DimensionVector::VOLTAGE, "Voltage"),
    (DimensionVector::CHARGE, "Charge"),
    (DimensionVector::CAPACITANCE, "Capacitance"),
    (DimensionVector::RESISTANCE, "Resistance"),
    (DimensionVector::CONDUCTANCE, "Conductance"),
    (DimensionVector::INDUCTANCE, "Inductance"),
    (DimensionVector::MAGNETIC_FLUX, "MagneticFlux"),
    (DimensionVector::MAGNETIC_FLUX_DENSITY, "MagneticFluxDensity"),
    (DimensionVector::LUMINOUS_FLUX, "LuminousFlux"),
    (DimensionVector::ILLUMINANCE, "Illuminance"),
    (DimensionVector::ABSORBED_DOSE, "AbsorbedDose"),
    (DimensionVector::ANGULAR_VELOCITY, "AngularVelocity"),
    (DimensionVector::DYNAMIC_VISCOSITY, "DynamicViscosity"),
];

impl fmt::Display for DimensionVector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = ["m", "kg", "s", "A", "K", "mol", "cd", "rad", "sr", "USD"];
        // Emit positive-exponent slots first, then negative-exponent slots,
        // preserving index order within each group. This produces conventional
        // notation like "USD·kg^-1" rather than "kg^-1·USD".
        let mut numerator = Vec::new();
        let mut denominator = Vec::new();
        for (i, r) in self.0.iter().enumerate() {
            if !r.is_zero() {
                let part = if *r == Rational::ONE {
                    names[i].to_string()
                } else {
                    format!("{}^{}", names[i], r)
                };
                if r.num() > 0 {
                    numerator.push(part);
                } else {
                    denominator.push(part);
                }
            }
        }
        let parts: Vec<String> = numerator.into_iter().chain(denominator).collect();
        if parts.is_empty() {
            write!(f, "dimensionless")
        } else {
            write!(f, "{}", parts.join("·"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_getters() {
        let r = Rational::new(3, 4);
        assert_eq!(r.num(), 3);
        assert_eq!(r.den(), 4);
    }

    #[test]
    fn rational_const_new_normalization() {
        const R: Rational = Rational::new(6, 4);
        assert_eq!(R.num(), 3);
        assert_eq!(R.den(), 2);
    }

    #[test]
    fn rational_normalization_via_getters() {
        let r = Rational::new(2, 4);
        assert_eq!(r.num(), 1);
        assert_eq!(r.den(), 2);
    }

    #[test]
    fn rational_arithmetic() {
        assert_eq!(
            Rational::new(1, 2) + Rational::new(1, 3),
            Rational::new(5, 6)
        );
        assert_eq!(Rational::new(1, 1) - Rational::new(1, 1), Rational::ZERO);
        assert_eq!(-Rational::new(3, 4), Rational::new(-3, 4));
    }

    #[test]
    fn rational_normalization() {
        assert_eq!(Rational::new(2, 4), Rational::new(1, 2));
        assert_eq!(Rational::new(-2, -4), Rational::new(1, 2));
        assert_eq!(Rational::new(2, -4), Rational::new(-1, 2));
    }

    #[test]
    fn dimension_mul() {
        let area = DimensionVector::LENGTH.mul(&DimensionVector::LENGTH);
        assert_eq!(area, DimensionVector::AREA);
    }

    #[test]
    fn dimension_div() {
        let velocity = DimensionVector::LENGTH.div(&DimensionVector({
            let mut v = [Rational::ZERO; 10];
            v[2] = Rational::ONE; // time
            v
        }));
        assert_eq!(velocity.0[0], Rational::ONE); // m
        assert_eq!(velocity.0[2], Rational::new(-1, 1)); // s^-1
    }

    #[test]
    fn dimension_pow() {
        let volume = DimensionVector::LENGTH.pow(3);
        assert_eq!(volume, DimensionVector::VOLUME);
    }

    #[test]
    fn dimensionless_check() {
        assert!(DimensionVector::DIMENSIONLESS.is_dimensionless());
        assert!(!DimensionVector::LENGTH.is_dimensionless());
    }

    #[test]
    fn force_dimension() {
        // F = m·a = kg·m·s⁻²
        let f = DimensionVector::MASS
            .mul(&DimensionVector::LENGTH)
            .mul(&DimensionVector::TIME.pow(-2));
        assert_eq!(f, FORCE);
    }

    #[test]
    fn all_new_derived_dimensions_have_correct_exponents() {
        // Helper: build a DimensionVector from (index, exponent) pairs.
        let make = |entries: &[(usize, i16)]| {
            let mut v = [Rational::ZERO; 10];
            for (i, e) in entries {
                v[*i] = Rational::new(*e, 1);
            }
            DimensionVector(v)
        };

        // Base-dimension constants that must exist.
        assert_eq!(DimensionVector::AMOUNT_OF_SUBSTANCE, make(&[(5, 1)]));
        assert_eq!(DimensionVector::LUMINOUS_INTENSITY, make(&[(6, 1)]));
        assert_eq!(DimensionVector::SOLID_ANGLE, make(&[(8, 1)]));

        // Derived-dimension constants.
        // Frequency: s⁻¹
        assert_eq!(DimensionVector::FREQUENCY, make(&[(2, -1)]));
        // Energy: kg·m²·s⁻²
        assert_eq!(DimensionVector::ENERGY, make(&[(0, 2), (1, 1), (2, -2)]));
        // Power: kg·m²·s⁻³
        assert_eq!(DimensionVector::POWER, make(&[(0, 2), (1, 1), (2, -3)]));
        // Pressure: kg·m⁻¹·s⁻²
        assert_eq!(DimensionVector::PRESSURE, make(&[(0, -1), (1, 1), (2, -2)]));
        // Voltage: kg·m²·s⁻³·A⁻¹
        assert_eq!(
            DimensionVector::VOLTAGE,
            make(&[(0, 2), (1, 1), (2, -3), (3, -1)])
        );
        // Charge: s·A
        assert_eq!(DimensionVector::CHARGE, make(&[(2, 1), (3, 1)]));
        // Capacitance: kg⁻¹·m⁻²·s⁴·A²
        assert_eq!(
            DimensionVector::CAPACITANCE,
            make(&[(0, -2), (1, -1), (2, 4), (3, 2)])
        );
        // Resistance: kg·m²·s⁻³·A⁻²
        assert_eq!(
            DimensionVector::RESISTANCE,
            make(&[(0, 2), (1, 1), (2, -3), (3, -2)])
        );
        // Conductance: kg⁻¹·m⁻²·s³·A²
        assert_eq!(
            DimensionVector::CONDUCTANCE,
            make(&[(0, -2), (1, -1), (2, 3), (3, 2)])
        );
        // Inductance: kg·m²·s⁻²·A⁻²
        assert_eq!(
            DimensionVector::INDUCTANCE,
            make(&[(0, 2), (1, 1), (2, -2), (3, -2)])
        );
        // Magnetic flux: kg·m²·s⁻²·A⁻¹
        assert_eq!(
            DimensionVector::MAGNETIC_FLUX,
            make(&[(0, 2), (1, 1), (2, -2), (3, -1)])
        );
        // Magnetic flux density: kg·s⁻²·A⁻¹
        assert_eq!(
            DimensionVector::MAGNETIC_FLUX_DENSITY,
            make(&[(1, 1), (2, -2), (3, -1)])
        );
        // Luminous flux: cd·sr
        assert_eq!(DimensionVector::LUMINOUS_FLUX, make(&[(6, 1), (8, 1)]));
        // Illuminance: cd·sr·m⁻²
        assert_eq!(
            DimensionVector::ILLUMINANCE,
            make(&[(0, -2), (6, 1), (8, 1)])
        );
        // Absorbed dose: m²·s⁻²
        assert_eq!(DimensionVector::ABSORBED_DOSE, make(&[(0, 2), (2, -2)]));
        // Angular velocity: rad·s⁻¹
        assert_eq!(
            DimensionVector::ANGULAR_VELOCITY,
            make(&[(2, -1), (7, 1)])
        );
        // Dynamic viscosity: kg·m⁻¹·s⁻¹
        assert_eq!(
            DimensionVector::DYNAMIC_VISCOSITY,
            make(&[(0, -1), (1, 1), (2, -1)])
        );
    }

    #[test]
    fn content_hash_determinism() {
        assert_eq!(
            DimensionVector::LENGTH.content_hash(),
            DimensionVector::LENGTH.content_hash()
        );
    }

    #[test]
    fn root_area_to_length() {
        // root(2) of AREA [2,0,...] → LENGTH [1,0,...]
        let result = DimensionVector::AREA.root(2);
        assert_eq!(result, DimensionVector::LENGTH);
    }

    #[test]
    fn root_length_4_to_length_2() {
        // root(2) of LENGTH^4 → LENGTH^2
        let len4 = DimensionVector::LENGTH.pow(4);
        let result = len4.root(2);
        assert_eq!(result, DimensionVector::AREA); // LENGTH^2 == AREA
    }

    #[test]
    fn root_length_to_fractional_exponent() {
        // root(2) of LENGTH → LENGTH^(1/2)
        let result = DimensionVector::LENGTH.root(2);
        assert_eq!(result.0[0], Rational::new(1, 2));
        // all other exponents should be zero
        for i in 1..10 {
            assert_eq!(result.0[i], Rational::ZERO);
        }
    }

    #[test]
    fn root_dimensionless_stays_dimensionless() {
        let result = DimensionVector::DIMENSIONLESS.root(2);
        assert_eq!(result, DimensionVector::DIMENSIONLESS);
    }

    #[test]
    fn pow_overflow_does_not_silently_wrap() {
        // LENGTH^64 raised to power 2 should give exponent 128.
        // With i8, 64*2=128 overflows i8::MAX (127).
        let len64 = DimensionVector::basis_n(0, 64);
        let result = len64.pow(2);
        assert_eq!(result.0[0], Rational::new(128, 1));
    }

    #[test]
    fn root_overflow_does_not_silently_wrap() {
        // Rational exponent {1, 64} with root(3) → {1, 192}.
        // With i8, den 64*3=192 overflows i8::MAX.
        let mut v = [Rational::ZERO; 10];
        v[0] = Rational::new(1, 64);
        let dv = DimensionVector(v);
        let result = dv.root(3);
        assert_eq!(result.0[0], Rational::new(1, 192));
    }

    #[test]
    fn rational_add_beyond_i8_range() {
        // 100 + 100 = 200, which overflows i8::MAX (127).
        let a = Rational::new(100, 1);
        let b = Rational::new(100, 1);
        assert_eq!(a + b, Rational::new(200, 1));
    }

    #[test]
    fn rational_neg_at_i8_boundary() {
        // Negating -128 in i8 overflows (no positive 128 in i8).
        // With i16, -(-128) = 128 works fine.
        let r = Rational::new(-128, 1);
        assert_eq!(-r, Rational::new(128, 1));
    }

    #[test]
    fn rational_sub_beyond_i8_range() {
        // 50 - (-100) = 150, but cross-multiplication in Sub produces
        // 50*1 - (-100)*1 = 150 which overflows i8::MAX.
        let a = Rational::new(50, 1);
        let b = Rational::new(-100, 1);
        assert_eq!(a - b, Rational::new(150, 1));
    }

    #[test]
    fn pow_large_exponent_round_trip() {
        // LENGTH^100 then root(100) should recover LENGTH.
        let powered = DimensionVector::LENGTH.pow(100);
        assert_eq!(powered.0[0], Rational::new(100, 1));
        let rooted = powered.root(100);
        assert_eq!(rooted, DimensionVector::LENGTH);
    }

    #[test]
    fn content_hash_determinism_with_wide_values() {
        // DimensionVector with exponents > 127 must hash deterministically.
        let dv = DimensionVector::basis_n(0, 200);
        let h1 = dv.content_hash();
        let h2 = dv.content_hash();
        assert_eq!(h1, h2);
        // Hash should differ from a different wide exponent.
        let dv2 = DimensionVector::basis_n(0, 201);
        assert_ne!(h1, dv2.content_hash());
    }

    #[test]
    fn rational_display_wide_values() {
        // Display formatting must show correct values beyond old i8 range.
        let r = Rational::new(200, 1);
        assert_eq!(format!("{}", r), "200");
        let r2 = Rational::new(300, 2);
        assert_eq!(format!("{}", r2), "150");
    }

    #[test]
    fn pressure_display_sorts_positive_before_negative_exponents() {
        // PRESSURE = kg·m⁻¹·s⁻² (index order: m=-1, kg=1, s=-2).
        // The Display impl emits positive-exponent slots first (kg), then
        // negative-exponent slots in index order (m^-1, s^-2).  This test
        // locks in that ordering for a pre-existing dimension so any future
        // revert to raw index order is caught immediately.
        assert_eq!(
            format!("{}", DimensionVector::PRESSURE),
            "kg\u{00B7}m^-1\u{00B7}s^-2"
        );
    }

    #[test]
    fn money_display_outputs_usd() {
        assert_eq!(format!("{}", DimensionVector::MONEY), "USD");
    }

    #[test]
    fn money_display_with_squared_exponent() {
        assert_eq!(format!("{}", DimensionVector::MONEY.pow(2)), "USD^2");
    }

    #[test]
    fn money_per_mass_display_renders_compositely() {
        // CostPerMass: MONEY / MASS → "USD·kg^-1"
        assert_eq!(
            format!("{}", DimensionVector::MONEY.div(&DimensionVector::MASS)),
            "USD\u{00B7}kg^-1"
        );
    }

    #[test]
    fn money_dimensionless_after_self_cancel_displays_dimensionless() {
        assert_eq!(
            format!("{}", DimensionVector::MONEY.div(&DimensionVector::MONEY)),
            "dimensionless"
        );
    }

    #[test]
    fn money_mul_with_mass_keeps_both_slots() {
        let result = DimensionVector::MONEY.mul(&DimensionVector::MASS);
        assert_eq!(result.0[9], Rational::ONE, "slot 9 (Money) should be ONE");
        assert_eq!(result.0[1], Rational::ONE, "slot 1 (Mass) should be ONE");
    }

    #[test]
    fn money_div_by_mass_produces_cost_per_mass() {
        // CostPerMass: MONEY/MASS → slot 9 = +1, slot 1 = -1, all others = 0
        let make = |entries: &[(usize, i16)]| {
            let mut v = [Rational::ZERO; 10];
            for (i, e) in entries {
                v[*i] = Rational::new(*e, 1);
            }
            DimensionVector(v)
        };
        assert_eq!(
            DimensionVector::MONEY.div(&DimensionVector::MASS),
            make(&[(1, -1), (9, 1)])
        );
    }

    #[test]
    fn money_pow_2_doubles_slot_9() {
        assert_eq!(DimensionVector::MONEY.pow(2).0[9], Rational::new(2, 1));
    }

    #[test]
    fn money_root_2_halves_slot_9() {
        assert_eq!(DimensionVector::MONEY.root(2).0[9], Rational::new(1, 2));
    }

    #[test]
    fn money_div_by_money_is_dimensionless() {
        assert!(DimensionVector::MONEY.div(&DimensionVector::MONEY).is_dimensionless());
    }

    #[test]
    fn money_does_not_leak_into_unrelated_arithmetic() {
        // Slot 9 must stay zero when Money is not involved.
        let result = DimensionVector::LENGTH.mul(&DimensionVector::MASS);
        assert_eq!(
            result.0[9],
            Rational::ZERO,
            "slot 9 should be zero for non-Money arithmetic"
        );
    }

    #[test]
    fn money_content_hash_is_deterministic() {
        assert_eq!(
            DimensionVector::MONEY.content_hash(),
            DimensionVector::MONEY.content_hash()
        );
    }

    #[test]
    fn money_content_hash_differs_from_other_base_dimensions() {
        let money_hash = DimensionVector::MONEY.content_hash();
        assert_ne!(money_hash, DimensionVector::LENGTH.content_hash());
        assert_ne!(money_hash, DimensionVector::MASS.content_hash());
        assert_ne!(money_hash, DimensionVector::TIME.content_hash());
        assert_ne!(money_hash, DimensionVector::CURRENT.content_hash());
        assert_ne!(money_hash, DimensionVector::TEMPERATURE.content_hash());
        assert_ne!(money_hash, DimensionVector::AMOUNT_OF_SUBSTANCE.content_hash());
        assert_ne!(money_hash, DimensionVector::LUMINOUS_INTENSITY.content_hash());
        assert_ne!(money_hash, DimensionVector::ANGLE.content_hash());
        assert_ne!(money_hash, DimensionVector::SOLID_ANGLE.content_hash());
        assert_ne!(money_hash, DimensionVector::DIMENSIONLESS.content_hash());
        assert_ne!(money_hash, DimensionVector::MONEY.pow(2).content_hash());
        assert_ne!(
            money_hash,
            DimensionVector::MONEY.div(&DimensionVector::MASS).content_hash()
        );
    }

    // --- canonical_name tests (step-1) ---

    #[test]
    fn canonical_name_money_returns_money() {
        assert_eq!(DimensionVector::MONEY.canonical_name(), Some("Money"));
    }

    #[test]
    fn canonical_name_force_returns_force() {
        assert_eq!(DimensionVector::FORCE.canonical_name(), Some("Force"));
    }

    #[test]
    fn canonical_name_length_returns_length() {
        assert_eq!(DimensionVector::LENGTH.canonical_name(), Some("Length"));
    }

    #[test]
    fn canonical_name_mass_returns_mass() {
        assert_eq!(DimensionVector::MASS.canonical_name(), Some("Mass"));
    }

    #[test]
    fn canonical_name_composite_returns_none() {
        // MONEY / MASS is a composite dimension — no single canonical name.
        assert_eq!(DimensionVector::MONEY.div(&DimensionVector::MASS).canonical_name(), None);
    }

    #[test]
    fn canonical_name_dimensionless_returns_none() {
        assert_eq!(DimensionVector::DIMENSIONLESS.canonical_name(), None);
    }

    /// Full coverage: every named singleton round-trips through `canonical_name`.
    ///
    /// The test derives its loop from [`super::NAMED_DIMENSIONS`] — the single source-of-truth
    /// table shared with `resolve_dimension_type`. Adding a new named dimension only requires
    /// updating `NAMED_DIMENSIONS`; this test and both consuming functions automatically stay in
    /// sync.
    #[test]
    fn canonical_name_covers_all_named_singletons() {
        for &(dim, expected) in super::NAMED_DIMENSIONS {
            assert_eq!(
                dim.canonical_name(),
                Some(expected),
                "canonical_name() mismatch for {:?}: expected {:?}",
                dim,
                expected,
            );
        }
        // DIMENSIONLESS is intentionally not named (self-explanatory to users).
        assert_eq!(DimensionVector::DIMENSIONLESS.canonical_name(), None);
    }

    #[test]
    fn content_hash_buffer_covers_slot_9() {
        // Verify that slot-9 bytes actually feed into the digest.
        let b7 = DimensionVector::basis_n(9, 7);
        let b8 = DimensionVector::basis_n(9, 8);
        let b0_7 = DimensionVector::basis_n(0, 7);
        // Different exponent in same slot → different hash
        assert_ne!(b7.content_hash(), b8.content_hash());
        // Same exponent in different slot → different hash
        assert_ne!(b7.content_hash(), b0_7.content_hash());
    }

    #[test]
    fn to_display_units_recognises_money() {
        let (value, unit) = DimensionVector::MONEY.to_display_units(25.0);
        assert_eq!(value, 25.0, "Money value should pass through unchanged");
        assert_eq!(unit, "USD", "Money unit label should be USD, not SI fallback");
        assert_ne!(unit, "SI", "Money must NOT fall through to the SI fallback");
    }

    #[test]
    fn to_display_units_keeps_si_fallback_for_unknown_composed_dim() {
        // A composed Money/Length dimension is not the canonical MONEY singleton.
        // Guard: Money must NOT leak into a composite label (the real regression
        // risk). We do not pin the exact fallback string so future improvements
        // (e.g. assigning "USD/m" to this dimension) don't falsely fail.
        let cost_per_length = DimensionVector::MONEY.div(&DimensionVector::LENGTH);
        let (_, unit) = cost_per_length.to_display_units(1.0);
        assert_ne!(unit, "USD", "bare 'USD' label must not appear for a composed Money/Length dimension");
    }

    #[test]
    fn money_constant_populates_slot_9() {
        // Slot 9 must be ONE; all other slots must be ZERO.
        assert_eq!(DimensionVector::MONEY.0[9], Rational::ONE);
        for i in 0..9 {
            assert_eq!(
                DimensionVector::MONEY.0[i],
                Rational::ZERO,
                "slot {} should be zero",
                i
            );
        }
        // MONEY is not DIMENSIONLESS and not SOLID_ANGLE.
        assert_ne!(DimensionVector::MONEY, DimensionVector::DIMENSIONLESS);
        assert_ne!(DimensionVector::MONEY, DimensionVector::SOLID_ANGLE);
    }

    /// Verify `NAMED_DIMENSIONS` is a complete, self-consistent table.
    ///
    /// (a) The table must be non-empty.
    /// (b) For every `(dim, name)` entry the round-trip `dim.canonical_name() == Some(name)` holds.
    /// (c) `DIMENSIONLESS.canonical_name()` is still `None` (intentionally excluded from the table).
    ///
    /// The table length is intentionally not asserted as a magic number — the round-trip loop
    /// and the DIMENSIONLESS check are the meaningful coverage; a length constant would just
    /// need updating whenever a new named singleton is added.
    #[test]
    fn named_dimensions_table_round_trips_canonical_name() {
        assert!(
            !super::NAMED_DIMENSIONS.is_empty(),
            "NAMED_DIMENSIONS must not be empty"
        );
        for &(dim, expected_name) in super::NAMED_DIMENSIONS {
            assert_eq!(
                dim.canonical_name(),
                Some(expected_name),
                "round-trip failed for {:?}: canonical_name() should return {:?}",
                dim,
                expected_name,
            );
        }
        assert_eq!(
            DimensionVector::DIMENSIONLESS.canonical_name(),
            None,
            "DIMENSIONLESS must remain absent from NAMED_DIMENSIONS (canonical_name returns None)"
        );
    }
}
