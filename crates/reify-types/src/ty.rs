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
    /// Named enum type.
    Enum(String),
    /// Homogeneous list type.
    List(Box<Type>),
    /// Homogeneous set type.
    Set(Box<Type>),
    /// Homogeneous map type (key, value).
    Map(Box<Type>, Box<Type>),
    /// Optional type.
    Option(Box<Type>),
    /// Function type.
    Function { params: Vec<Type>, return_type: Box<Type> },
    /// Unresolved type parameter reference (e.g., `T` in a generic definition).
    TypeParam(String),
    /// Concrete structure name reference at an instantiation site
    /// (e.g., `Bolt` in `Box<Bolt>()`). Distinct from TypeParam which
    /// represents unresolved type variables needing substitution.
    StructureRef(String),
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

    /// Returns the inner name for name-carrying variants without allocating.
    /// Used for registry lookups instead of Display formatting.
    pub fn as_name(&self) -> Option<&str> {
        match self {
            Type::TypeParam(name) | Type::StructureRef(name) | Type::Enum(name) => Some(name),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_enum_display() {
        assert_eq!(format!("{}", Type::Enum("Color".into())), "Enum(Color)");
    }

    #[test]
    fn type_list_display() {
        assert_eq!(format!("{}", Type::List(Box::new(Type::Int))), "List<Int>");
    }

    #[test]
    fn type_set_display() {
        assert_eq!(
            format!("{}", Type::Set(Box::new(Type::String))),
            "Set<String>"
        );
    }

    #[test]
    fn type_map_display() {
        assert_eq!(
            format!(
                "{}",
                Type::Map(Box::new(Type::String), Box::new(Type::Real))
            ),
            "Map<String, Real>"
        );
    }

    #[test]
    fn type_option_display() {
        assert_eq!(
            format!("{}", Type::Option(Box::new(Type::Int))),
            "Option<Int>"
        );
    }

    #[test]
    fn type_function_display() {
        let func = Type::Function {
            params: vec![Type::Int],
            return_type: Box::new(Type::Real),
        };
        assert_eq!(format!("{}", func), "Function(Int) -> Real");
    }

    #[test]
    fn type_function_multi_params_display() {
        let func = Type::Function {
            params: vec![Type::Int, Type::String],
            return_type: Box::new(Type::Bool),
        };
        assert_eq!(format!("{}", func), "Function(Int, String) -> Bool");
    }

    #[test]
    fn type_param_display() {
        assert_eq!(format!("{}", Type::TypeParam("T".into())), "T");
        assert_eq!(format!("{}", Type::TypeParam("Element".into())), "Element");
    }

    #[test]
    fn type_param_not_numeric() {
        assert!(!Type::TypeParam("T".into()).is_numeric());
    }

    #[test]
    fn type_new_variants_not_numeric() {
        assert!(!Type::Enum("X".into()).is_numeric());
        assert!(!Type::List(Box::new(Type::Int)).is_numeric());
        assert!(!Type::Set(Box::new(Type::Int)).is_numeric());
        assert!(!Type::Map(Box::new(Type::Int), Box::new(Type::Int)).is_numeric());
        assert!(!Type::Option(Box::new(Type::Int)).is_numeric());
        let func = Type::Function {
            params: vec![],
            return_type: Box::new(Type::Int),
        };
        assert!(!func.is_numeric());
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
            Type::Enum(name) => write!(f, "Enum({})", name),
            Type::List(inner) => write!(f, "List<{}>", inner),
            Type::Set(inner) => write!(f, "Set<{}>", inner),
            Type::Map(k, v) => write!(f, "Map<{}, {}>", k, v),
            Type::Option(inner) => write!(f, "Option<{}>", inner),
            Type::Function { params, return_type } => {
                let params_str: Vec<String> = params.iter().map(|p| format!("{}", p)).collect();
                write!(f, "Function({}) -> {}", params_str.join(", "), return_type)
            }
            Type::TypeParam(name) => write!(f, "{}", name),
            Type::StructureRef(name) => write!(f, "{}", name),
        }
    }
}
