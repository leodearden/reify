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
    /// Field type: a mapping from domain to codomain (e.g., Field<Point3, Scalar>).
    Field { domain: Box<Type>, codomain: Box<Type> },
    /// Geometry handle (a reference to a realization, not a scalar value).
    Geometry,
    /// N-dimensional point with a quantity type (e.g., Point3<Scalar[m]>).
    Point { n: usize, quantity: Box<Type> },
    /// N-dimensional vector with a quantity type (e.g., Vector3<Scalar[m]>).
    Vector { n: usize, quantity: Box<Type> },
    /// Rank-r tensor with n elements per dimension and a quantity type (e.g., Tensor2x3<Scalar[m]>).
    Tensor { rank: usize, n: usize, quantity: Box<Type> },
    /// Complex number type with a quantity type (e.g., Complex<Scalar[Ω]>).
    Complex(Box<Type>),
    /// Orientation in N-dimensional space (unit quaternion for N=3, angle for N=2).
    Orientation(usize),
    /// Range over a comparable element type (e.g., Range<Int>, Range<Scalar[m]>).
    Range(Box<Type>),
    /// m-by-n matrix with a quantity type (e.g., Matrix3x2<Scalar[m]>).
    Matrix { m: usize, n: usize, quantity: Box<Type> },
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

    /// Shorthand for a 2D vector with a given quantity type.
    pub fn vec2(quantity: Type) -> Self {
        Type::Vector { n: 2, quantity: Box::new(quantity) }
    }

    /// Shorthand for a 3D vector with a given quantity type.
    pub fn vec3(quantity: Type) -> Self {
        Type::Vector { n: 3, quantity: Box::new(quantity) }
    }

    /// Shorthand for a 2D point with a given quantity type.
    pub fn point2(quantity: Type) -> Self {
        Type::Point { n: 2, quantity: Box::new(quantity) }
    }

    /// Shorthand for a 3D point with a given quantity type.
    pub fn point3(quantity: Type) -> Self {
        Type::Point { n: 3, quantity: Box::new(quantity) }
    }

    /// Shorthand for a rank-r tensor with n elements per dimension and a given quantity type.
    pub fn tensor(rank: usize, n: usize, quantity: Type) -> Self {
        Type::Tensor { rank, n, quantity: Box::new(quantity) }
    }

    /// Shorthand for a complex number type with a given quantity type.
    pub fn complex(q: Type) -> Self {
        Type::Complex(Box::new(q))
    }

    /// Shorthand for an orientation in N-dimensional space.
    pub fn orientation(n: usize) -> Self {
        Type::Orientation(n)
    }

    /// Shorthand for a range over a given element type.
    pub fn range(inner: Type) -> Self {
        Type::Range(Box::new(inner))
    }

    /// Shorthand for an m×n matrix with a given quantity type.
    pub fn matrix(m: usize, n: usize, quantity: Type) -> Self {
        Type::Matrix { m, n, quantity: Box::new(quantity) }
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
    fn type_structure_ref_display() {
        assert_eq!(format!("{}", Type::StructureRef("Bolt".into())), "Bolt");
        assert_eq!(format!("{}", Type::StructureRef("Bracket".into())), "Bracket");
    }

    #[test]
    fn type_structure_ref_not_numeric() {
        assert!(!Type::StructureRef("Bolt".into()).is_numeric());
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

    #[test]
    fn type_field_variant() {
        let field_ty = Type::Field {
            domain: Box::new(Type::Real),
            codomain: Box::new(Type::Real),
        };
        // Display
        assert_eq!(format!("{}", field_ty), "Field<Real, Real>");
        // Equality
        let field_ty2 = Type::Field {
            domain: Box::new(Type::Real),
            codomain: Box::new(Type::Real),
        };
        assert_eq!(field_ty, field_ty2);
        // Not numeric
        assert!(!field_ty.is_numeric());
    }

    #[test]
    fn type_point_vector_not_numeric() {
        assert!(!Type::point2(Type::length()).is_numeric());
        assert!(!Type::point3(Type::Real).is_numeric());
        assert!(!Type::vec2(Type::length()).is_numeric());
        assert!(!Type::vec3(Type::Real).is_numeric());
    }

    #[test]
    fn type_point_vector_eq_and_hash() {
        use std::collections::HashMap;

        let p3_len = Type::point3(Type::length());
        let p3_len2 = Type::point3(Type::length());
        let p2_len = Type::point2(Type::length());
        let p3_real = Type::point3(Type::Real);
        let v3_len = Type::vec3(Type::length());

        // (a) Point3<Length> == Point3<Length>
        assert_eq!(p3_len, p3_len2);

        // (b) Point3<Length> != Point2<Length>
        assert_ne!(p3_len, p2_len);

        // (c) Point3<Length> != Point3<Real>
        assert_ne!(p3_len, p3_real);

        // (d) Point3<Length> != Vector3<Length>
        assert_ne!(p3_len, v3_len);

        // (e) Vector assertions
        let v3_len_a = Type::vec3(Type::length());
        let v3_len_b = Type::vec3(Type::length());
        let v2_len = Type::vec2(Type::length());
        let v3_real = Type::vec3(Type::Real);
        assert_eq!(v3_len_a, v3_len_b);
        assert_ne!(v3_len_a, v2_len);
        assert_ne!(v3_len_a, v3_real);

        // (f) Hash consistency: equal types produce equal hashes via HashMap insert+lookup
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(p3_len.clone(), "p3_len");
        assert_eq!(map.get(&p3_len2), Some(&"p3_len"));
        map.insert(v3_len.clone(), "v3_len");
        assert_eq!(map.get(&v3_len_a), Some(&"v3_len"));
    }

    #[test]
    fn type_vec_factory_methods() {
        assert_eq!(
            Type::vec2(Type::length()),
            Type::Vector { n: 2, quantity: Box::new(Type::length()) }
        );
        assert_eq!(
            Type::vec3(Type::Real),
            Type::Vector { n: 3, quantity: Box::new(Type::Real) }
        );
    }

    #[test]
    fn type_point_factory_methods() {
        assert_eq!(
            Type::point2(Type::length()),
            Type::Point { n: 2, quantity: Box::new(Type::length()) }
        );
        assert_eq!(
            Type::point3(Type::Real),
            Type::Point { n: 3, quantity: Box::new(Type::Real) }
        );
    }

    #[test]
    fn type_tensor_display() {
        // Tensor{rank}x{n}<{quantity}>
        assert_eq!(
            format!("{}", Type::tensor(2, 3, Type::length())),
            "Tensor2x3<Scalar[m]>"
        );
        assert_eq!(
            format!("{}", Type::tensor(1, 4, Type::Real)),
            "Tensor1x4<Real>"
        );
        assert_eq!(
            format!("{}", Type::tensor(3, 2, Type::Int)),
            "Tensor3x2<Int>"
        );
    }

    #[test]
    fn type_tensor_factory_method() {
        // rank-2 tensor, 3 elements per level, quantity = length scalar
        assert_eq!(
            Type::tensor(2, 3, Type::length()),
            Type::Tensor { rank: 2, n: 3, quantity: Box::new(Type::length()) }
        );
        // rank-1 tensor, 4 elements, quantity = Real
        assert_eq!(
            Type::tensor(1, 4, Type::Real),
            Type::Tensor { rank: 1, n: 4, quantity: Box::new(Type::Real) }
        );
    }

    #[test]
    fn type_tensor_eq_and_hash() {
        use std::collections::HashMap;

        let t2_3_len = Type::tensor(2, 3, Type::length());
        let t2_3_len2 = Type::tensor(2, 3, Type::length());
        let t1_3_len = Type::tensor(1, 3, Type::length());
        let t2_4_len = Type::tensor(2, 4, Type::length());
        let t2_3_real = Type::tensor(2, 3, Type::Real);
        let v3_len = Type::vec3(Type::length());
        let p3_len = Type::point3(Type::length());

        // (a) Same rank/n/quantity => equal
        assert_eq!(t2_3_len, t2_3_len2);

        // (b) Different rank => not equal
        assert_ne!(t2_3_len, t1_3_len);

        // (c) Different n => not equal
        assert_ne!(t2_3_len, t2_4_len);

        // (d) Different quantity => not equal
        assert_ne!(t2_3_len, t2_3_real);

        // (e) Tensor != Vector/Point with same n/quantity
        assert_ne!(t2_3_len, v3_len);
        assert_ne!(t2_3_len, p3_len);

        // (f) Hash consistency via HashMap
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(t2_3_len.clone(), "t2_3_len");
        assert_eq!(map.get(&t2_3_len2), Some(&"t2_3_len"));
    }

    #[test]
    fn type_tensor_not_numeric() {
        assert!(!Type::tensor(1, 3, Type::length()).is_numeric());
        assert!(!Type::tensor(2, 2, Type::Real).is_numeric());
    }

    // ── Complex tests (step-1) ────────────────────────────────────────────────

    #[test]
    fn type_complex_display_real() {
        assert_eq!(format!("{}", Type::complex(Type::Real)), "Complex<Real>");
    }

    #[test]
    fn type_complex_display_scalar() {
        assert_eq!(
            format!("{}", Type::complex(Type::length())),
            "Complex<Scalar[m]>"
        );
    }

    #[test]
    fn type_complex_display_nested() {
        assert_eq!(
            format!("{}", Type::complex(Type::complex(Type::Real))),
            "Complex<Complex<Real>>"
        );
    }

    #[test]
    fn type_complex_factory_eq_variant() {
        assert_eq!(
            Type::complex(Type::Real),
            Type::Complex(Box::new(Type::Real))
        );
        assert_eq!(
            Type::complex(Type::Int),
            Type::Complex(Box::new(Type::Int))
        );
    }

    #[test]
    fn type_complex_eq_and_hash() {
        use std::collections::HashMap;

        let c_real = Type::complex(Type::Real);
        let c_real2 = Type::complex(Type::Real);
        let c_int = Type::complex(Type::Int);

        // Equal complex types are equal
        assert_eq!(c_real, c_real2);
        // Different inner types are not equal
        assert_ne!(c_real, c_int);
        // complex(Real) != Real
        assert_ne!(c_real, Type::Real);

        // Hash consistency: equal values produce equal hashes via HashMap
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(c_real.clone(), "c_real");
        assert_eq!(map.get(&c_real2), Some(&"c_real"));
        assert_eq!(map.get(&c_int), None);
    }

    #[test]
    fn type_complex_not_numeric() {
        assert!(!Type::complex(Type::Real).is_numeric());
        assert!(!Type::complex(Type::Int).is_numeric());
    }

    #[test]
    fn type_complex_as_name_none() {
        assert_eq!(Type::complex(Type::Real).as_name(), None);
    }

    #[test]
    fn type_vector_display() {
        let v3_length = Type::Vector {
            n: 3,
            quantity: Box::new(Type::length()),
        };
        assert_eq!(format!("{}", v3_length), "Vector3<Scalar[m]>");

        let v2_real = Type::Vector {
            n: 2,
            quantity: Box::new(Type::Real),
        };
        assert_eq!(format!("{}", v2_real), "Vector2<Real>");
    }

    // ── Orientation tests (step-1) ──────────────────────────────────────────

    #[test]
    fn type_orientation_construction() {
        let o3 = Type::Orientation(3);
        let o2 = Type::Orientation(2);
        // Distinct dimensions
        assert_ne!(o3, o2);
        // Same dimension equal
        assert_eq!(Type::Orientation(3), Type::Orientation(3));
    }

    #[test]
    fn type_orientation_display() {
        assert_eq!(format!("{}", Type::Orientation(3)), "Orientation3");
        assert_eq!(format!("{}", Type::Orientation(2)), "Orientation2");
    }

    #[test]
    fn type_orientation_factory() {
        assert_eq!(Type::orientation(3), Type::Orientation(3));
        assert_eq!(Type::orientation(2), Type::Orientation(2));
    }

    #[test]
    fn type_orientation_eq_and_hash() {
        use std::collections::HashMap;

        let o3a = Type::Orientation(3);
        let o3b = Type::Orientation(3);
        let o2 = Type::Orientation(2);

        assert_eq!(o3a, o3b);
        assert_ne!(o3a, o2);
        // Orientation != other types
        assert_ne!(o3a, Type::Real);

        // Hash consistency
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(o3a.clone(), "o3");
        assert_eq!(map.get(&o3b), Some(&"o3"));
        assert_eq!(map.get(&o2), None);
    }

    #[test]
    fn type_orientation_not_numeric() {
        assert!(!Type::Orientation(3).is_numeric());
        assert!(!Type::Orientation(2).is_numeric());
    }

    #[test]
    fn type_orientation_as_name_none() {
        assert_eq!(Type::Orientation(3).as_name(), None);
    }

    // ── Range tests (step-1) ─────────────────────────────────────────────────

    #[test]
    fn type_range_construction() {
        let r_int = Type::Range(Box::new(Type::Int));
        let r_real = Type::Range(Box::new(Type::Real));
        // Distinct inner types
        assert_ne!(r_int, r_real);
        // Same inner type equal
        assert_eq!(Type::Range(Box::new(Type::Int)), Type::Range(Box::new(Type::Int)));
    }

    #[test]
    fn type_range_display_int() {
        assert_eq!(format!("{}", Type::Range(Box::new(Type::Int))), "Range<Int>");
    }

    #[test]
    fn type_range_display_scalar() {
        assert_eq!(
            format!("{}", Type::Range(Box::new(Type::length()))),
            "Range<Scalar[m]>"
        );
    }

    #[test]
    fn type_range_display_real() {
        assert_eq!(format!("{}", Type::Range(Box::new(Type::Real))), "Range<Real>");
    }

    #[test]
    fn type_range_factory() {
        assert_eq!(Type::range(Type::Int), Type::Range(Box::new(Type::Int)));
        assert_eq!(Type::range(Type::Real), Type::Range(Box::new(Type::Real)));
    }

    #[test]
    fn type_range_eq_and_hash() {
        use std::collections::HashMap;

        let r_int_a = Type::range(Type::Int);
        let r_int_b = Type::range(Type::Int);
        let r_real = Type::range(Type::Real);

        assert_eq!(r_int_a, r_int_b);
        assert_ne!(r_int_a, r_real);
        // Range(Int) != Int
        assert_ne!(r_int_a, Type::Int);

        // Hash consistency
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(r_int_a.clone(), "r_int");
        assert_eq!(map.get(&r_int_b), Some(&"r_int"));
        assert_eq!(map.get(&r_real), None);
    }

    #[test]
    fn type_range_not_numeric() {
        assert!(!Type::range(Type::Int).is_numeric());
        assert!(!Type::range(Type::Real).is_numeric());
        assert!(!Type::range(Type::length()).is_numeric());
    }

    #[test]
    fn type_range_as_name_none() {
        assert_eq!(Type::range(Type::Int).as_name(), None);
    }

    // ── Matrix tests (step-1) ────────────────────────────────────────────────

    #[test]
    fn type_matrix_construction_and_equality() {
        // (a) Same dimensions and quantity are equal
        assert_eq!(
            Type::Matrix { m: 3, n: 2, quantity: Box::new(Type::Real) },
            Type::Matrix { m: 3, n: 2, quantity: Box::new(Type::Real) },
        );
        // Different m — not equal
        assert_ne!(
            Type::Matrix { m: 3, n: 2, quantity: Box::new(Type::Real) },
            Type::Matrix { m: 2, n: 3, quantity: Box::new(Type::Real) },
        );
        // Different n — not equal
        assert_ne!(
            Type::Matrix { m: 3, n: 2, quantity: Box::new(Type::Real) },
            Type::Matrix { m: 3, n: 3, quantity: Box::new(Type::Real) },
        );
        // Different quantity — not equal
        assert_ne!(
            Type::Matrix { m: 3, n: 2, quantity: Box::new(Type::Real) },
            Type::Matrix { m: 3, n: 2, quantity: Box::new(Type::Int) },
        );
        // Matrix != Tensor with same n/quantity
        assert_ne!(
            Type::Matrix { m: 2, n: 3, quantity: Box::new(Type::Real) },
            Type::Tensor { rank: 2, n: 3, quantity: Box::new(Type::Real) },
        );
    }

    #[test]
    fn type_matrix_display() {
        // (d) Display: Matrix{m}x{n}<{quantity}>
        assert_eq!(
            format!("{}", Type::Matrix { m: 3, n: 2, quantity: Box::new(Type::Real) }),
            "Matrix3x2<Real>"
        );
        assert_eq!(
            format!("{}", Type::Matrix { m: 4, n: 4, quantity: Box::new(Type::length()) }),
            "Matrix4x4<Scalar[m]>"
        );
    }

    #[test]
    fn type_matrix_factory() {
        // (b) Type::matrix(m, n, q) factory method
        assert_eq!(
            Type::matrix(3, 2, Type::Real),
            Type::Matrix { m: 3, n: 2, quantity: Box::new(Type::Real) },
        );
        assert_eq!(
            Type::matrix(1, 1, Type::Int),
            Type::Matrix { m: 1, n: 1, quantity: Box::new(Type::Int) },
        );
    }

    #[test]
    fn type_matrix_eq_and_hash() {
        use std::collections::HashMap;
        // (c) hash consistency: same key retrieves value
        let m_a = Type::matrix(3, 2, Type::Real);
        let m_b = Type::matrix(3, 2, Type::Real);
        let m_other = Type::matrix(2, 3, Type::Real);

        assert_eq!(m_a, m_b);
        assert_ne!(m_a, m_other);

        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(m_a.clone(), "mat");
        assert_eq!(map.get(&m_b), Some(&"mat"));
        assert_eq!(map.get(&m_other), None);
    }

    #[test]
    fn type_matrix_not_numeric() {
        // (e) is_numeric returns false
        assert!(!Type::matrix(3, 2, Type::Real).is_numeric());
        assert!(!Type::matrix(1, 1, Type::Int).is_numeric());
    }

    #[test]
    fn type_matrix_as_name_none() {
        // (f) as_name returns None
        assert_eq!(Type::matrix(3, 2, Type::Real).as_name(), None);
    }

    #[test]
    fn type_point_display() {
        let p3_length = Type::Point {
            n: 3,
            quantity: Box::new(Type::length()),
        };
        assert_eq!(format!("{}", p3_length), "Point3<Scalar[m]>");

        let p2_real = Type::Point {
            n: 2,
            quantity: Box::new(Type::Real),
        };
        assert_eq!(format!("{}", p2_real), "Point2<Real>");

        let p1_int = Type::Point {
            n: 1,
            quantity: Box::new(Type::Int),
        };
        assert_eq!(format!("{}", p1_int), "Point1<Int>");
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
            Type::Field { domain, codomain } => write!(f, "Field<{}, {}>", domain, codomain),
            Type::Geometry => write!(f, "Geometry"),
            Type::Point { n, quantity } => write!(f, "Point{}<{}>", n, quantity),
            Type::Vector { n, quantity } => write!(f, "Vector{}<{}>", n, quantity),
            Type::Tensor { rank, n, quantity } => write!(f, "Tensor{}x{}<{}>", rank, n, quantity),
            Type::Complex(inner) => write!(f, "Complex<{}>", inner),
            Type::Orientation(n) => write!(f, "Orientation{}", n),
            Type::Range(inner) => write!(f, "Range<{}>", inner),
            Type::Matrix { m, n, quantity } => write!(f, "Matrix{}x{}<{}>", m, n, quantity),
        }
    }
}
