use std::fmt;
use std::ops::{Add, Neg, Sub};

use crate::ContentHash;

/// Rational number as i8/i8 for dimension exponents.
/// Sufficient for all physical dimension exponents (e.g., m^(1/2) for √area).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rational {
    pub num: i8,
    pub den: i8,
}

impl Rational {
    pub const ZERO: Rational = Rational { num: 0, den: 1 };
    pub const ONE: Rational = Rational { num: 1, den: 1 };

    pub fn new(num: i8, den: i8) -> Self {
        assert!(den != 0, "denominator must not be zero");
        let g = gcd(num.unsigned_abs(), den.unsigned_abs()) as i8;
        let sign = if den < 0 { -1 } else { 1 };
        Rational {
            num: sign * num / g,
            den: sign * den / g,
        }
    }

    pub fn is_zero(self) -> bool {
        self.num == 0
    }

    pub fn is_integer(self) -> bool {
        self.den == 1 || self.num % self.den == 0
    }

    pub fn as_i8(self) -> Option<i8> {
        if self.den == 1 {
            Some(self.num)
        } else if self.num % self.den == 0 {
            Some(self.num / self.den)
        } else {
            None
        }
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

fn gcd(mut a: u8, mut b: u8) -> u8 {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    const fn basis_n(index: usize, n: i8) -> DimensionVector {
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
        for i in 0..9 {
            result[i] = self.0[i] + other.0[i];
        }
        DimensionVector(result)
    }

    /// Divide dimensions (subtract exponents).
    pub fn div(&self, other: &DimensionVector) -> DimensionVector {
        let mut result = [Rational::ZERO; 9];
        for i in 0..9 {
            result[i] = self.0[i] - other.0[i];
        }
        DimensionVector(result)
    }

    /// Raise to an integer power (multiply all exponents).
    pub fn pow(&self, n: i8) -> DimensionVector {
        let mut result = [Rational::ZERO; 9];
        let nr = Rational { num: n, den: 1 };
        for i in 0..9 {
            result[i] = Rational::new(self.0[i].num * nr.num, self.0[i].den * nr.den);
        }
        DimensionVector(result)
    }

    pub fn content_hash(&self) -> ContentHash {
        let mut buf = [0u8; 18]; // 9 * 2 bytes
        for (i, r) in self.0.iter().enumerate() {
            buf[i * 2] = r.num as u8;
            buf[i * 2 + 1] = r.den as u8;
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
}
