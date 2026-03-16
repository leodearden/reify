use crate::dimension::DimensionVector;

/// Types in the Reify type system (M1 subset).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// Boolean value.
    Bool,
    /// Arbitrary-precision integer.
    Int,
    /// IEEE 754 double-precision float (dimensionless).
    Real,
    /// UTF-8 string.
    String,
    /// Dimensioned scalar (e.g., 80mm has dimension LENGTH).
    Scalar { dimension: DimensionVector },
}

impl Type {
    /// Shorthand for a length scalar.
    pub fn length() -> Self {
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }
    }

    /// Shorthand for an angle scalar.
    pub fn angle() -> Self {
        Type::Scalar {
            dimension: DimensionVector::ANGLE,
        }
    }

    /// Shorthand for a dimensionless scalar.
    pub fn dimensionless_scalar() -> Self {
        Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }
    }

    /// Is this type a numeric type (Int, Real, or Scalar)?
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Real | Type::Scalar { .. })
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Bool => write!(f, "Bool"),
            Type::Int => write!(f, "Int"),
            Type::Real => write!(f, "Real"),
            Type::String => write!(f, "String"),
            Type::Scalar { dimension } => {
                if dimension.is_dimensionless() {
                    write!(f, "Scalar")
                } else {
                    write!(f, "Scalar[{}]", dimension)
                }
            }
        }
    }
}
