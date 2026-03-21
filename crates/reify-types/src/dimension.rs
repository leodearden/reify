use std::fmt;
use std::ops::{Add, Neg, Sub};

use crate::ContentHash;

/// Rational number as i16/i16 for dimension exponents.
/// Uses i16 to prevent overflow when multiplying exponents (max i8 * i8 = 16,129 < i16::MAX).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rational {
    pub num: i16,
    pub den: i16,
}

impl Rational {
    pub const ZERO: Rational = Rational { num: 0, den: 1 };
    pub const ONE: Rational = Rational { num: 1, den: 1 };

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
        self.num == 0
    }

    pub fn is_integer(self) -> bool {
        self.den == 1 || self.num % self.den == 0
    }

    pub fn as_i8(self) -> Option<i8> {
        let val = if self.den == 1 {
            self.num
        } else if self.num % self.den == 0 {
            self.num / self.den
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
            self.num * rhs.den + rhs.num * self.den,
            self.den * rhs.den,
        )
    }
}

impl Sub for Rational {
    type Output = Rational;
    fn sub(self, rhs: Rational) -> Rational {
        Rational::new(
            self.num * rhs.den - rhs.num * self.den,
            self.den * rhs.den,
        )
    }
}

impl Neg for Rational {
    type Output = Rational;
    fn neg(self) -> Rational {
        Rational {
            num: -self.num,
            den: self.den,
        }
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
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

/// Dimension vector: 9 rational exponents for the SI base dimensions.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DimensionVector(pub [Rational; 9]);

impl DimensionVector {
    pub const DIMENSIONLESS: DimensionVector = DimensionVector([Rational::ZERO; 9]);
    pub const LENGTH: DimensionVector = DimensionVector::basis(0);
    pub const MASS: DimensionVector = DimensionVector::basis(1);
    pub const TIME: DimensionVector = DimensionVector::basis(2);
    pub const CURRENT: DimensionVector = DimensionVector::basis(3);
    pub const TEMPERATURE: DimensionVector = DimensionVector::basis(4);
    pub const ANGLE: DimensionVector = DimensionVector::basis(7);

    /// Common derived dimensions.
    pub const AREA: DimensionVector = DimensionVector::basis_n(0, 2);
    pub const VOLUME: DimensionVector = DimensionVector::basis_n(0, 3);

    const fn basis(index: usize) -> DimensionVector {
        let mut v = [Rational::ZERO; 9];
        v[index] = Rational::ONE;
        DimensionVector(v)
    }

    const fn basis_n(index: usize, n: i16) -> DimensionVector {
        let mut v = [Rational::ZERO; 9];
        v[index] = Rational { num: n, den: 1 };
        DimensionVector(v)
    }

    pub fn is_dimensionless(&self) -> bool {
        self.0.iter().all(|r| r.is_zero())
    }

    /// Multiply dimensions (add exponents).
    pub fn mul(&self, other: &DimensionVector) -> DimensionVector {
        let mut result = [Rational::ZERO; 9];
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = self.0[i] + other.0[i];
        }
        DimensionVector(result)
    }

    /// Divide dimensions (subtract exponents).
    pub fn div(&self, other: &DimensionVector) -> DimensionVector {
        let mut result = [Rational::ZERO; 9];
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
        let mut result = [Rational::ZERO; 9];
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = Rational::new(self.0[i].num, self.0[i].den * n);
        }
        DimensionVector(result)
    }

    /// Raise to an integer power (multiply all exponents).
    pub fn pow(&self, n: i8) -> DimensionVector {
        let mut result = [Rational::ZERO; 9];
        let n = n as i16;
        let nr = Rational { num: n, den: 1 };
        for (i, slot) in result.iter_mut().enumerate() {
            *slot = Rational::new(self.0[i].num * nr.num, self.0[i].den * nr.den);
        }
        DimensionVector(result)
    }

    pub fn content_hash(&self) -> ContentHash {
        let mut buf = [0u8; 36]; // 9 * 4 bytes (2 bytes per i16 field)
        for (i, r) in self.0.iter().enumerate() {
            let num_bytes = r.num.to_le_bytes();
            let den_bytes = r.den.to_le_bytes();
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
    let mut v = [Rational::ZERO; 9];
    v[0] = Rational::ONE; // length
    v[1] = Rational::ONE; // mass
    v[2] = Rational {
        num: -2,
        den: 1,
    }; // time^-2
    DimensionVector(v)
};

impl fmt::Display for DimensionVector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names = ["m", "kg", "s", "A", "K", "mol", "cd", "rad", "sr"];
        let mut parts = Vec::new();
        for (i, r) in self.0.iter().enumerate() {
            if !r.is_zero() {
                if *r == Rational::ONE {
                    parts.push(names[i].to_string());
                } else {
                    parts.push(format!("{}^{}", names[i], r));
                }
            }
        }
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
        assert_eq!(Rational::new(1, 2) + Rational::new(1, 3), Rational::new(5, 6));
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
        let velocity = DimensionVector::LENGTH.div(&DimensionVector {
            0: {
                let mut v = [Rational::ZERO; 9];
                v[2] = Rational::ONE; // time
                v
            },
        });
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
        for i in 1..9 {
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
        assert_eq!(result.0[0], Rational { num: 128, den: 1 });
    }

    #[test]
    fn root_overflow_does_not_silently_wrap() {
        // Rational exponent {1, 64} with root(3) → {1, 192}.
        // With i8, den 64*3=192 overflows i8::MAX.
        let mut v = [Rational::ZERO; 9];
        v[0] = Rational { num: 1, den: 64 };
        let dv = DimensionVector(v);
        let result = dv.root(3);
        assert_eq!(result.0[0], Rational { num: 1, den: 192 });
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
        let r = Rational { num: -128, den: 1 };
        assert_eq!(-r, Rational { num: 128, den: 1 });
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
        assert_eq!(powered.0[0], Rational { num: 100, den: 1 });
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
        let r = Rational { num: 200, den: 1 };
        assert_eq!(format!("{}", r), "200");
        let r2 = Rational::new(300, 2);
        assert_eq!(format!("{}", r2), "150");
    }
}
