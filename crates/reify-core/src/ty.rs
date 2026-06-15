//! Types in the Reify type system.
//!
//! ## Point / Vector quantity-slot convention
//!
//! When `Point3<Q>` / `Vector3<Q>` surface syntax is resolved by the parametric
//! resolver (`resolve_parameterized_builtin_type` and the substitution path
//! `resolve_parameterized_builtin_type_with_subst` in
//! `crates/reify-compiler/src/type_resolution.rs`), the `quantity` slot of
//! `Type::Point` / `Type::Vector` is **always** `Type::Scalar { dimension: … }`.
//! The resolver treats `Q` as a *dimension expression* — accepting forms like
//! `Length`, `kg*m/s^2` — and wraps the resolved [`DimensionVector`] in
//! `Type::Scalar`.  This convention is asserted by the integration tests in
//! `crates/reify-compiler/tests/parametric_vector_point_resolution_tests.rs`.
//!
//! The variant itself does **not** enforce `Type::Scalar` in the `quantity` slot.
//! `Value::Point::infer_type()` / `Value::Vector::infer_type()` in
//! `crates/reify-types/src/value.rs` may produce `Type::dimensionless_scalar()` or `Type::Int`
//! quantities for unit-less component vectors; tests in this crate also construct
//! those forms directly.  `is_scalar_like_leaf` in
//! `crates/reify-compiler/src/type_compat.rs` treats `Type::dimensionless_scalar()`, `Type::Int`, and
//! `Type::Scalar { .. }` symmetrically for compat-rule firing, so the looseness is
//! intentional.
//!
//! Accepting any `Type` in the `Q` position (mirroring `Tensor` / `Matrix`, which
//! use `resolve_type_expr_with_aliases`) was considered and deferred (task-2767); the
//! dimension-expression form is the established surface-syntax contract.

use crate::dimension::DimensionVector;

/// Identifies which geometry entity kind a selector targets.
///
/// Used by [`Type::Selector`] and [`crate::value::SelectorValue`] to enforce
/// kind-closure at the constructor boundary (K1 invariant, PRD §4.3).
///
/// Dimensionality mapping (D2/§4.1): Face=2, Edge=1, Body=3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectorKind {
    /// Selects 2-manifold faces (dimensionality = 2).
    Face,
    /// Selects 1-manifold edges (dimensionality = 1).
    Edge,
    /// Selects volumetric bodies (dimensionality = 3).
    Body,
}

impl SelectorKind {
    /// Topological dimensionality of the selected entity kind.
    ///
    /// - `Face` → 2 (2-manifold surface)
    /// - `Edge` → 1 (1-manifold curve)
    /// - `Body` → 3 (volumetric solid)
    pub fn dimensionality(&self) -> usize {
        match self {
            SelectorKind::Face => 2,
            SelectorKind::Edge => 1,
            SelectorKind::Body => 3,
        }
    }
}

impl std::fmt::Display for SelectorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectorKind::Face => write!(f, "FaceSelector"),
            SelectorKind::Edge => write!(f, "EdgeSelector"),
            SelectorKind::Body => write!(f, "BodySelector"),
        }
    }
}

/// Types in the Reify type system (M1 subset).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    /// Boolean value.
    Bool,
    /// Arbitrary-precision integer.
    Int,
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
    /// Keyed sub-collection kind — members addressed by author-assigned String
    /// key; distinct from Map (values) and List (positional).
    ///
    /// A `Keyed<T>` sub is structural: it lowers to a `SubComponentDecl`, never
    /// held in a value cell. Introduced in task 3930 β (PRD keyed-collection-identity.md).
    Keyed(Box<Type>),
    /// Optional type.
    Option(Box<Type>),
    /// Function type.
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
    /// Unresolved type parameter reference (e.g., `T` in a generic definition).
    TypeParam(String),
    /// Concrete structure name reference at an instantiation site
    /// (e.g., `Bolt` in `Box<Bolt>()`). Distinct from TypeParam which
    /// represents unresolved type variables needing substitution.
    StructureRef(String),
    /// Nominal reference to a trait name used as a type
    /// (e.g., `Material` in `param material : Material`).
    /// Semantically: "any value whose structure conforms to this trait."
    /// Distinct from `StructureRef` (which denotes a concrete struct) and
    /// `TypeParam` (which denotes an unresolved type variable). Call-site
    /// conformance enforcement for trait-typed params is deferred; this
    /// variant currently records the declaration only.
    TraitObject(String),
    /// Field type: a mapping from domain to codomain (e.g., Field<Point3, Scalar>).
    Field {
        domain: Box<Type>,
        codomain: Box<Type>,
    },
    /// Geometry handle (a reference to a realization, not a scalar value).
    Geometry,
    /// N-dimensional point with a quantity type (e.g., `Point3<Scalar[m]>`).
    ///
    /// See the *Point / Vector quantity-slot convention* section in the module
    /// docs for the parametric-resolver convention, de-facto contract, and
    /// compat treatment.
    Point { n: usize, quantity: Box<Type> },
    /// N-dimensional vector with a quantity type (e.g., `Vector3<Scalar[m]>`).
    ///
    /// See the *Point / Vector quantity-slot convention* section in the module
    /// docs for the parametric-resolver convention, de-facto contract, and
    /// compat treatment.
    Vector { n: usize, quantity: Box<Type> },
    /// Rank-r tensor with n elements per dimension and a quantity type (e.g., Tensor2x3<Scalar[m]>).
    Tensor {
        rank: usize,
        n: usize,
        quantity: Box<Type>,
    },
    /// Complex number type with a quantity type (e.g., Complex<Scalar[Ω]>).
    Complex(Box<Type>),
    /// Orientation in N-dimensional space (unit quaternion for N=3, angle for N=2).
    Orientation(usize),
    /// Coordinate frame in N-dimensional space: an origin point + a basis orientation.
    Frame(usize),
    /// Rigid-body transformation in N-dimensional space: a rotation (Orientation) + translation (Vector).
    Transform(usize),
    /// General (non-rigid) affine map in N-dimensional space: a linear part + translation.
    ///
    /// Unlike `Transform(usize)` (rigid: rotation+translation), the linear part may scale/shear.
    /// Stored as inline arrays `linear: [[f64;3];3]` + `translation: [f64;3]` in `Value::AffineMap`.
    AffineMap(usize),
    /// Range over a comparable element type (e.g., Range<Int>, Range<Scalar[m]>).
    Range(Box<Type>),
    /// 3D plane: an origin point and a unit normal vector.
    Plane,
    /// 3D axis (ray): an origin point and a unit direction vector.
    Axis,
    /// Dimensionless 3D unit vector; distinct from `Vector3<Length>` and `Orientation`.
    ///
    /// A pure direction (assumed unit-normalized) carrying no length dimension and
    /// no chirality/handedness — unlike `Orientation(3)` (a full rotation) or a
    /// `Vector3<Length>` (a dimensioned displacement). Produced by datum
    /// projections such as `axis.dir`, `plane.normal`, and `frame.x/.y/.z`
    /// (geometric-relations β).
    Direction,
    /// 3D axis-aligned bounding box defined by min and max corner points.
    BoundingBox,
    /// A dimensioned scalar whose dimension is the named dimension-param
    /// (e.g. `Q` in `fn g<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q>`).
    ///
    /// Compile-time/signature-only — erased before eval (D7/D1). No
    /// `Value::ScalarParam` exists; this variant is only produced inside
    /// dimension-kinded generic fn signatures by `resolve_parameterized_builtin_type`.
    ScalarParam(String),
    /// User-facing m×n matrix type (e.g., Matrix3x2<Scalar[m]>).
    ///
    /// Semantically, evaluation treats matrices as rank-2 tensors; `Type::Matrix` preserves
    /// distinct row (`m`) and column (`n`) dimensions while `Type::Tensor` uses a single
    /// `n` for all dimensions.
    Matrix {
        m: usize,
        n: usize,
        quantity: Box<Type>,
    },
    /// Sentinel for a type-inference failure ("poison value").
    ///
    /// Operations that see a `Type::Error` operand must propagate `Type::Error`
    /// (not fall back to `Type::dimensionless_scalar()`) so that a single root-cause error does
    /// not trigger cascading type-mismatch diagnostics downstream. Producers
    /// that emit an error diagnostic should pair it with a `Type::Error`
    /// result type; consumer sites (binary operators, index access, member
    /// access, quantifiers, etc.) guard on `is_error()` and short-circuit.
    Error,
    /// Topology selector: a first-class value that identifies a subset of
    /// geometry entities of a given kind (face, edge, or body).
    ///
    /// The `SelectorKind` parameter encodes which entity dimension the selector
    /// targets, enforcing kind-closure at the constructor boundary (K1 invariant,
    /// PRD §4.3). All operations on a selector (union, intersect, difference)
    /// must produce a result of the same kind.
    ///
    /// Introduced in task 4116 α.
    Selector(SelectorKind),
    /// Kind-agnostic topology selector: a param/field annotation that accepts a
    /// `Selector` value of ANY concrete kind (Face, Edge, or Body — and Vertex
    /// once A1 lands).
    ///
    /// Used as the declared type for FEA boundary-condition targets
    /// (`FixedSupport.target : Selector`) where the kind of the selected
    /// geometry is not constrained at the type level (PRD §4.2/§11.1,
    /// task 4369 / A2).  Single-kind selector params (`Type::Selector(k)`) keep
    /// exact-kind checking; only params declared with the bare `Selector`
    /// annotation resolve to this variant.
    ///
    /// There is no `Value::AnySelector`; at runtime the cell always holds a
    /// concrete `Value::Selector(sv)` whose kind is checked by
    /// `value_type_kind_matches` (which accepts any `sv.kind` against this
    /// cell type).
    AnySelector,
    /// Compile-time-only union of mutually-exclusive arm types from a
    /// `match`-block decl cluster (PRD `match-block-decls.md` §6.4).
    ///
    /// When `self.head` (or `bolt.head` from outside) refers to a
    /// `GuardedDeclGroup` whose arms each declare a sub of a different
    /// concrete structure (e.g. `Hex => sub head : HexHead;
    /// Socket => sub head : SocketHead`), the static type at the reference
    /// site is `Type::Union(vec![HexHead, SocketHead])`. Narrowing under a
    /// matching arm-guard collapses the union to a single arm-type.
    ///
    /// Compile-time only: rejected by `is_representable_cell_type` in
    /// `reify-eval` (engine_eval.rs). A `Value::Union` does not exist; at
    /// runtime exactly one arm is active and its concrete `StructureRef`-
    /// typed cell holds the value.
    Union(Vec<Type>),
    /// A geometric **relation** directive: a degree-of-freedom-removal
    /// directive between datums, carrying **no truth value** (distinct from
    /// `Bool`, which asserts truth).
    ///
    /// Produced by the relation vocabulary (`coincident`/`concentric`/`flush`/
    /// `offset`/`parallel`/`perpendicular`/… — geometric-relations γ, task
    /// 4383). A relation call type-checks to `Type::Relation` but is an
    /// **Undef-backed** compile-time directive: there is no `Value::Relation`,
    /// so relation calls evaluate to `Value::Undef` until ζ supplies the
    /// relate-solve (the geometry-query Phase-1 precedent). Inside a
    /// `relate { … }` block a relation removes degrees of freedom rather than
    /// asserting a truth — `relate { coincident(a, b) }` drives `a`/`b` into
    /// coincidence. Admitted by `is_representable_cell_type` alongside
    /// `StructureRef`/`TraitObject`.
    Relation,
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
        Type::Vector {
            n: 2,
            quantity: Box::new(quantity),
        }
    }

    /// Shorthand for a 3D vector with a given quantity type.
    pub fn vec3(quantity: Type) -> Self {
        Type::Vector {
            n: 3,
            quantity: Box::new(quantity),
        }
    }

    /// Shorthand for a 2D point with a given quantity type.
    pub fn point2(quantity: Type) -> Self {
        Type::Point {
            n: 2,
            quantity: Box::new(quantity),
        }
    }

    /// Shorthand for a 3D point with a given quantity type.
    pub fn point3(quantity: Type) -> Self {
        Type::Point {
            n: 3,
            quantity: Box::new(quantity),
        }
    }

    /// Shorthand for a rank-r tensor with n elements per dimension and a given quantity type.
    pub fn tensor(rank: usize, n: usize, quantity: Type) -> Self {
        Type::Tensor {
            rank,
            n,
            quantity: Box::new(quantity),
        }
    }

    /// Shorthand for a complex number type with a given quantity type.
    pub fn complex(q: Type) -> Self {
        Type::Complex(Box::new(q))
    }

    /// Shorthand for an orientation in N-dimensional space.
    pub fn orientation(n: usize) -> Self {
        Type::Orientation(n)
    }

    /// Shorthand for a coordinate frame in N-dimensional space.
    pub fn frame(n: usize) -> Self {
        Type::Frame(n)
    }

    /// Shorthand for a rigid-body transformation in N-dimensional space.
    pub fn transform(n: usize) -> Self {
        Type::Transform(n)
    }

    /// Shorthand for a general (non-rigid) affine map in N-dimensional space.
    pub fn affine_map(n: usize) -> Self {
        Type::AffineMap(n)
    }

    /// Shorthand for a range over a given element type.
    pub fn range(inner: Type) -> Self {
        Type::Range(Box::new(inner))
    }

    /// Shorthand for a 3D plane type.
    pub fn plane() -> Self {
        Type::Plane
    }

    /// Shorthand for a 3D axis type.
    pub fn axis() -> Self {
        Type::Axis
    }

    /// Shorthand for a dimensionless 3D unit-vector (direction) type.
    pub fn direction() -> Self {
        Type::Direction
    }

    /// Shorthand for the geometric-relation directive type (γ): a DOF-removal
    /// directive carrying no truth value, distinct from `Bool`.
    pub fn relation() -> Self {
        Type::Relation
    }

    /// Shorthand for a 3D bounding box type.
    pub fn bounding_box() -> Self {
        Type::BoundingBox
    }

    /// Shorthand for a topology selector type of the given kind.
    pub fn selector(kind: SelectorKind) -> Self {
        Type::Selector(kind)
    }

    /// Shorthand for an m×n matrix with a given quantity type.
    pub fn matrix(m: usize, n: usize, quantity: Type) -> Self {
        Type::Matrix {
            m,
            n,
            quantity: Box::new(quantity),
        }
    }

    /// Is this type a numeric type (Int, Real, or Scalar)?
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int | Type::Scalar { .. })
    }

    /// Is this the poison-value sentinel `Type::Error`?
    ///
    /// Consumer sites should short-circuit to `Type::Error` when any operand's
    /// type returns `true` here, to prevent cascading diagnostics.
    ///
    /// # Top-level-only contract
    ///
    /// **This method checks ONLY the top-level `Type::Error` variant.  It does
    /// NOT recurse into inner type parameters.**  Compound types that carry a
    /// poisoned inner type return `false`:
    ///
    /// - `List<Error>`  → `false`
    /// - `Option<Error>` → `false`
    /// - `Set<Error>` → `false`
    /// - `Map<K, Error>` (error in value position) → `false`
    /// - `Map<Error, V>` (error in key position) → `false`
    /// - Any other `Box<Type>`-bearing variant (`Range`, `Complex`, `Field`,
    ///   `Point`, `Vector`, `Tensor`, `Matrix`) with an `Error` inner type
    ///   → `false`
    ///
    /// This boundary is intentional in the current implementation: the anti-
    /// cascade contract (task-448) covers only the top-level sentinel, and
    /// extending it to a recursive check requires simultaneous updates at every
    /// consumer guard site.
    ///
    /// # Known gap: nested-error cascade
    ///
    /// A reachable cascade on current code is:
    ///
    /// ```text
    /// trait T { let x : List<Real> = [self.unsupported] }
    /// structure S : T {}
    /// ```
    ///
    /// Here `self.unsupported` emits "unknown member 'unsupported' on self" and
    /// returns `Type::Error`; the list literal infers its element type from the
    /// first element and wraps it to `List<Error>`.  The trait-let injection
    /// pass at `conformance.rs:521-531` calls
    /// `type_compatible(List<Real>, List<Error>)`, whose top-level `is_error()`
    /// guard trips on neither operand, no rule arm matches, and it returns
    /// `false` — emitting a second "type mismatch for trait let 'x'" cascade on
    /// top of the root-cause diagnostic.
    ///
    /// This cascade is pinned as a regression test at:
    /// `crates/reify-compiler/tests/type_error_propagation_tests.rs`
    /// `::nested_compound_error_cascades_through_trait_let_annotation`
    ///
    /// # Follow-up plan
    ///
    /// When a future contributor needs deep detection, introduce a
    /// `contains_error()` recursive helper on `Type` and **update every
    /// consumer listed in the variant-doc at lines 80–88** (binary ops, index
    /// access, member access, quantifiers, plus `type_compatible` /
    /// `implicitly_converts_to` in `type_compat.rs`) in the **same commit**.
    /// At that point, also flip the cascade assertion in the regression test
    /// from "IS present" to "is NOT present", and update this follow-up section.
    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    /// Returns the inner name for name-carrying variants without allocating.
    /// Used for registry lookups instead of Display formatting.
    pub fn as_name(&self) -> Option<&str> {
        match self {
            Type::TypeParam(name)
            | Type::StructureRef(name)
            | Type::TraitObject(name)
            | Type::Enum(name) => Some(name),
            _ => None,
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Bool => write!(f, "Bool"),
            Type::Int => write!(f, "Int"),
            Type::String => write!(f, "String"),
            Type::Scalar { dimension } => {
                if dimension.is_dimensionless() {
                    write!(f, "Real")
                } else {
                    write!(f, "Scalar[{}]", dimension)
                }
            }
            Type::Enum(name) => write!(f, "Enum({})", name),
            Type::List(inner) => write!(f, "List<{}>", inner),
            Type::Set(inner) => write!(f, "Set<{}>", inner),
            Type::Map(k, v) => write!(f, "Map<{}, {}>", k, v),
            Type::Keyed(inner) => write!(f, "Keyed<{}>", inner),
            Type::Option(inner) => write!(f, "Option<{}>", inner),
            Type::Function {
                params,
                return_type,
            } => {
                let params_str: Vec<String> = params.iter().map(|p| format!("{}", p)).collect();
                write!(f, "Function({}) -> {}", params_str.join(", "), return_type)
            }
            Type::TypeParam(name) => write!(f, "{}", name),
            Type::StructureRef(name) => write!(f, "{}", name),
            Type::TraitObject(name) => write!(f, "{}", name),
            Type::Field { domain, codomain } => write!(f, "Field<{}, {}>", domain, codomain),
            Type::Geometry => write!(f, "Geometry"),
            Type::Point { n, quantity } => write!(f, "Point{}<{}>", n, quantity),
            Type::Vector { n, quantity } => write!(f, "Vector{}<{}>", n, quantity),
            Type::Tensor { rank, n, quantity } => write!(f, "Tensor{}x{}<{}>", rank, n, quantity),
            Type::Complex(inner) => write!(f, "Complex<{}>", inner),
            Type::Orientation(n) => write!(f, "Orientation{}", n),
            Type::Frame(n) => write!(f, "Frame{}", n),
            Type::Transform(n) => write!(f, "Transform{}", n),
            Type::AffineMap(n) => write!(f, "AffineMap{}", n),
            Type::Range(inner) => write!(f, "Range<{}>", inner),
            Type::Plane => write!(f, "Plane"),
            Type::Axis => write!(f, "Axis"),
            Type::Direction => write!(f, "Direction"),
            Type::Relation => write!(f, "Relation"),
            Type::BoundingBox => write!(f, "BoundingBox"),
            Type::ScalarParam(name) => write!(f, "Scalar<{}>", name),
            Type::Matrix { m, n, quantity } => write!(f, "Matrix{}x{}<{}>", m, n, quantity),
            Type::Selector(kind) => write!(f, "{}", kind),
            Type::AnySelector => write!(f, "Selector"),
            Type::Error => write!(f, "<error>"),
            Type::Union(arms) => write!(
                f,
                "Union<{}>",
                arms.iter()
                    .map(|a| format!("{}", a))
                    .collect::<Vec<_>>()
                    .join(" | ")
            ),
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
                Type::Map(Box::new(Type::String), Box::new(Type::dimensionless_scalar()))
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
            return_type: Box::new(Type::dimensionless_scalar()),
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
        assert_eq!(
            format!("{}", Type::StructureRef("Bracket".into())),
            "Bracket"
        );
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
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        };
        // Display
        assert_eq!(format!("{}", field_ty), "Field<Real, Real>");
        // Equality
        let field_ty2 = Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        };
        assert_eq!(field_ty, field_ty2);
        // Not numeric
        assert!(!field_ty.is_numeric());
    }

    #[test]
    fn type_point_vector_not_numeric() {
        assert!(!Type::point2(Type::length()).is_numeric());
        assert!(!Type::point3(Type::dimensionless_scalar()).is_numeric());
        assert!(!Type::vec2(Type::length()).is_numeric());
        assert!(!Type::vec3(Type::dimensionless_scalar()).is_numeric());
    }

    #[test]
    fn type_point_vector_eq_and_hash() {
        use std::collections::HashMap;

        let p3_len = Type::point3(Type::length());
        let p3_len2 = Type::point3(Type::length());
        let p2_len = Type::point2(Type::length());
        let p3_real = Type::point3(Type::dimensionless_scalar());
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
        let v3_real = Type::vec3(Type::dimensionless_scalar());
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
            Type::Vector {
                n: 2,
                quantity: Box::new(Type::length())
            }
        );
        assert_eq!(
            Type::vec3(Type::dimensionless_scalar()),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar())
            }
        );
    }

    #[test]
    fn type_point_factory_methods() {
        assert_eq!(
            Type::point2(Type::length()),
            Type::Point {
                n: 2,
                quantity: Box::new(Type::length())
            }
        );
        assert_eq!(
            Type::point3(Type::dimensionless_scalar()),
            Type::Point {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar())
            }
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
            format!("{}", Type::tensor(1, 4, Type::dimensionless_scalar())),
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
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::length())
            }
        );
        // rank-1 tensor, 4 elements, quantity = Real
        assert_eq!(
            Type::tensor(1, 4, Type::dimensionless_scalar()),
            Type::Tensor {
                rank: 1,
                n: 4,
                quantity: Box::new(Type::dimensionless_scalar())
            }
        );
    }

    #[test]
    fn type_tensor_eq_and_hash() {
        use std::collections::HashMap;

        let t2_3_len = Type::tensor(2, 3, Type::length());
        let t2_3_len2 = Type::tensor(2, 3, Type::length());
        let t1_3_len = Type::tensor(1, 3, Type::length());
        let t2_4_len = Type::tensor(2, 4, Type::length());
        let t2_3_real = Type::tensor(2, 3, Type::dimensionless_scalar());
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
        assert!(!Type::tensor(2, 2, Type::dimensionless_scalar()).is_numeric());
    }

    // ── Complex tests (step-1) ────────────────────────────────────────────────

    #[test]
    fn type_complex_display_real() {
        assert_eq!(format!("{}", Type::complex(Type::dimensionless_scalar())), "Complex<Real>");
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
            format!("{}", Type::complex(Type::complex(Type::dimensionless_scalar()))),
            "Complex<Complex<Real>>"
        );
    }

    #[test]
    fn type_complex_factory_eq_variant() {
        assert_eq!(
            Type::complex(Type::dimensionless_scalar()),
            Type::Complex(Box::new(Type::dimensionless_scalar()))
        );
        assert_eq!(Type::complex(Type::Int), Type::Complex(Box::new(Type::Int)));
    }

    #[test]
    fn type_complex_eq_and_hash() {
        use std::collections::HashMap;

        let c_real = Type::complex(Type::dimensionless_scalar());
        let c_real2 = Type::complex(Type::dimensionless_scalar());
        let c_int = Type::complex(Type::Int);

        // Equal complex types are equal
        assert_eq!(c_real, c_real2);
        // Different inner types are not equal
        assert_ne!(c_real, c_int);
        // complex(Real) != Real
        assert_ne!(c_real, Type::dimensionless_scalar());

        // Hash consistency: equal values produce equal hashes via HashMap
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(c_real.clone(), "c_real");
        assert_eq!(map.get(&c_real2), Some(&"c_real"));
        assert_eq!(map.get(&c_int), None);
    }

    #[test]
    fn type_complex_not_numeric() {
        assert!(!Type::complex(Type::dimensionless_scalar()).is_numeric());
        assert!(!Type::complex(Type::Int).is_numeric());
    }

    #[test]
    fn type_complex_as_name_none() {
        assert_eq!(Type::complex(Type::dimensionless_scalar()).as_name(), None);
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
            quantity: Box::new(Type::dimensionless_scalar()),
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
        assert_ne!(o3a, Type::dimensionless_scalar());

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
        let r_real = Type::Range(Box::new(Type::dimensionless_scalar()));
        // Distinct inner types
        assert_ne!(r_int, r_real);
        // Same inner type equal
        assert_eq!(
            Type::Range(Box::new(Type::Int)),
            Type::Range(Box::new(Type::Int))
        );
    }

    #[test]
    fn type_range_display_int() {
        assert_eq!(
            format!("{}", Type::Range(Box::new(Type::Int))),
            "Range<Int>"
        );
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
        assert_eq!(
            format!("{}", Type::Range(Box::new(Type::dimensionless_scalar()))),
            "Range<Real>"
        );
    }

    #[test]
    fn type_range_factory() {
        assert_eq!(Type::range(Type::Int), Type::Range(Box::new(Type::Int)));
        assert_eq!(Type::range(Type::dimensionless_scalar()), Type::Range(Box::new(Type::dimensionless_scalar())));
    }

    #[test]
    fn type_range_eq_and_hash() {
        use std::collections::HashMap;

        let r_int_a = Type::range(Type::Int);
        let r_int_b = Type::range(Type::Int);
        let r_real = Type::range(Type::dimensionless_scalar());

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
        assert!(!Type::range(Type::dimensionless_scalar()).is_numeric());
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
            Type::Matrix {
                m: 3,
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar())
            },
            Type::Matrix {
                m: 3,
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar())
            },
        );
        // Different m — not equal
        assert_ne!(
            Type::Matrix {
                m: 3,
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar())
            },
            Type::Matrix {
                m: 2,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar())
            },
        );
        // Different n — not equal
        assert_ne!(
            Type::Matrix {
                m: 3,
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar())
            },
            Type::Matrix {
                m: 3,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar())
            },
        );
        // Different quantity — not equal
        assert_ne!(
            Type::Matrix {
                m: 3,
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar())
            },
            Type::Matrix {
                m: 3,
                n: 2,
                quantity: Box::new(Type::Int)
            },
        );
        // Matrix != Tensor with same n/quantity
        assert_ne!(
            Type::Matrix {
                m: 2,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar())
            },
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar())
            },
        );
    }

    #[test]
    fn type_matrix_display() {
        // (d) Display: Matrix{m}x{n}<{quantity}>
        assert_eq!(
            format!(
                "{}",
                Type::Matrix {
                    m: 3,
                    n: 2,
                    quantity: Box::new(Type::dimensionless_scalar())
                }
            ),
            "Matrix3x2<Real>"
        );
        assert_eq!(
            format!(
                "{}",
                Type::Matrix {
                    m: 4,
                    n: 4,
                    quantity: Box::new(Type::length())
                }
            ),
            "Matrix4x4<Scalar[m]>"
        );
    }

    #[test]
    fn type_matrix_factory() {
        // (b) Type::matrix(m, n, q) factory method
        assert_eq!(
            Type::matrix(3, 2, Type::dimensionless_scalar()),
            Type::Matrix {
                m: 3,
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar())
            },
        );
        assert_eq!(
            Type::matrix(1, 1, Type::Int),
            Type::Matrix {
                m: 1,
                n: 1,
                quantity: Box::new(Type::Int)
            },
        );
    }

    #[test]
    fn type_matrix_eq_and_hash() {
        use std::collections::HashMap;
        // (c) hash consistency: same key retrieves value
        let m_a = Type::matrix(3, 2, Type::dimensionless_scalar());
        let m_b = Type::matrix(3, 2, Type::dimensionless_scalar());
        let m_other = Type::matrix(2, 3, Type::dimensionless_scalar());

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
        assert!(!Type::matrix(3, 2, Type::dimensionless_scalar()).is_numeric());
        assert!(!Type::matrix(1, 1, Type::Int).is_numeric());
    }

    #[test]
    fn type_matrix_as_name_none() {
        // (f) as_name returns None
        assert_eq!(Type::matrix(3, 2, Type::dimensionless_scalar()).as_name(), None);
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
            quantity: Box::new(Type::dimensionless_scalar()),
        };
        assert_eq!(format!("{}", p2_real), "Point2<Real>");

        let p1_int = Type::Point {
            n: 1,
            quantity: Box::new(Type::Int),
        };
        assert_eq!(format!("{}", p1_int), "Point1<Int>");
    }

    // ── Frame tests (step-1) ─────────────────────────────────────────────────

    #[test]
    fn type_frame_construction() {
        let f3 = Type::Frame(3);
        let f2 = Type::Frame(2);
        // Same dimension equal
        assert_eq!(Type::Frame(3), Type::Frame(3));
        // Distinct dimensions not equal
        assert_ne!(f3, f2);
    }

    #[test]
    fn type_frame_display() {
        assert_eq!(format!("{}", Type::Frame(3)), "Frame3");
        assert_eq!(format!("{}", Type::Frame(2)), "Frame2");
    }

    #[test]
    fn type_frame_factory() {
        assert_eq!(Type::frame(3), Type::Frame(3));
        assert_eq!(Type::frame(2), Type::Frame(2));
    }

    #[test]
    fn type_frame_eq_and_hash() {
        use std::collections::HashMap;

        let f3a = Type::Frame(3);
        let f3b = Type::Frame(3);
        let f2 = Type::Frame(2);

        assert_eq!(f3a, f3b);
        assert_ne!(f3a, f2);
        // Frame != other types
        assert_ne!(f3a, Type::dimensionless_scalar());

        // Hash consistency
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(f3a.clone(), "f3");
        assert_eq!(map.get(&f3b), Some(&"f3"));
        assert_eq!(map.get(&f2), None);
    }

    #[test]
    fn type_frame_not_numeric() {
        assert!(!Type::Frame(3).is_numeric());
        assert!(!Type::Frame(2).is_numeric());
    }

    #[test]
    fn type_frame_as_name_none() {
        assert_eq!(Type::Frame(3).as_name(), None);
    }

    #[test]
    fn type_frame_ne_orientation() {
        // Frame(3) and Orientation(3) are distinct types
        assert_ne!(Type::Frame(3), Type::Orientation(3));
    }

    // ── Transform tests (step-1) ─────────────────────────────────────────────

    #[test]
    fn type_transform_construction() {
        let t3 = Type::Transform(3);
        let t2 = Type::Transform(2);
        // Same dimension equal
        assert_eq!(Type::Transform(3), Type::Transform(3));
        // Distinct dimensions not equal
        assert_ne!(t3, t2);
    }

    #[test]
    fn type_transform_display() {
        assert_eq!(format!("{}", Type::Transform(3)), "Transform3");
        assert_eq!(format!("{}", Type::Transform(2)), "Transform2");
    }

    #[test]
    fn type_transform_factory() {
        assert_eq!(Type::transform(3), Type::Transform(3));
        assert_eq!(Type::transform(2), Type::Transform(2));
    }

    #[test]
    fn type_transform_eq_and_hash() {
        use std::collections::HashMap;

        let t3a = Type::Transform(3);
        let t3b = Type::Transform(3);
        let t2 = Type::Transform(2);

        assert_eq!(t3a, t3b);
        assert_ne!(t3a, t2);
        // Transform != other types
        assert_ne!(t3a, Type::dimensionless_scalar());

        // Hash consistency
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(t3a.clone(), "t3");
        assert_eq!(map.get(&t3b), Some(&"t3"));
        assert_eq!(map.get(&t2), None);
    }

    #[test]
    fn type_transform_not_numeric() {
        assert!(!Type::Transform(3).is_numeric());
        assert!(!Type::Transform(2).is_numeric());
    }

    #[test]
    fn type_transform_as_name_none() {
        assert_eq!(Type::Transform(3).as_name(), None);
    }

    #[test]
    fn type_transform_ne_frame() {
        // Transform(3) and Frame(3) are distinct types
        assert_ne!(Type::Transform(3), Type::Frame(3));
    }

    #[test]
    fn type_transform_ne_orientation() {
        // Transform(3) and Orientation(3) are distinct types
        assert_ne!(Type::Transform(3), Type::Orientation(3));
    }

    // ── Plane tests (pre-1) ──────────────────────────────────────────────────

    #[test]
    fn type_plane_construction_and_equality() {
        assert_eq!(Type::Plane, Type::Plane);
        assert_ne!(Type::Plane, Type::Axis);
        assert_ne!(Type::Plane, Type::BoundingBox);
        assert_ne!(Type::Plane, Type::dimensionless_scalar());
    }

    #[test]
    fn type_plane_display() {
        assert_eq!(format!("{}", Type::Plane), "Plane");
    }

    #[test]
    fn type_plane_factory() {
        assert_eq!(Type::plane(), Type::Plane);
    }

    #[test]
    fn type_plane_eq_and_hash() {
        use std::collections::HashMap;
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(Type::Plane, "plane");
        assert_eq!(map.get(&Type::Plane), Some(&"plane"));
        assert_eq!(map.get(&Type::Axis), None);
    }

    #[test]
    fn type_plane_not_numeric() {
        assert!(!Type::Plane.is_numeric());
    }

    #[test]
    fn type_plane_as_name_none() {
        assert_eq!(Type::Plane.as_name(), None);
    }

    // ── Axis tests (pre-1) ───────────────────────────────────────────────────

    #[test]
    fn type_axis_construction_and_equality() {
        assert_eq!(Type::Axis, Type::Axis);
        assert_ne!(Type::Axis, Type::Plane);
        assert_ne!(Type::Axis, Type::BoundingBox);
        assert_ne!(Type::Axis, Type::dimensionless_scalar());
    }

    #[test]
    fn type_axis_display() {
        assert_eq!(format!("{}", Type::Axis), "Axis");
    }

    #[test]
    fn type_axis_factory() {
        assert_eq!(Type::axis(), Type::Axis);
    }

    #[test]
    fn type_axis_not_numeric() {
        assert!(!Type::Axis.is_numeric());
    }

    #[test]
    fn type_axis_as_name_none() {
        assert_eq!(Type::Axis.as_name(), None);
    }

    // ── Direction tests (β: First-class Direction type) ──────────────────────

    #[test]
    fn type_direction_construction_and_equality() {
        assert_eq!(Type::Direction, Type::Direction);
        assert_ne!(Type::Direction, Type::Axis);
        assert_ne!(Type::Direction, Type::Plane);
        assert_ne!(Type::Direction, Type::dimensionless_scalar());
    }

    #[test]
    fn type_direction_display() {
        assert_eq!(format!("{}", Type::Direction), "Direction");
    }

    #[test]
    fn type_direction_factory() {
        assert_eq!(Type::direction(), Type::Direction);
    }

    #[test]
    fn type_direction_eq_and_hash() {
        use std::collections::HashMap;
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(Type::Direction, "direction");
        assert_eq!(map.get(&Type::Direction), Some(&"direction"));
        assert_eq!(map.get(&Type::Axis), None);
    }

    #[test]
    fn type_direction_not_numeric() {
        assert!(!Type::Direction.is_numeric());
    }

    #[test]
    fn type_direction_as_name_none() {
        assert_eq!(Type::Direction.as_name(), None);
    }

    /// The locked β property: `Direction` is a DISTINCT type — it is neither a
    /// `Vector3<Length>` nor an `Orientation(3)`.
    #[test]
    fn type_direction_distinct_from_vector3_and_orientation() {
        assert_ne!(Type::Direction, Type::vec3(Type::length()));
        assert_ne!(Type::Direction, Type::Orientation(3));
    }

    // ── BoundingBox tests (pre-1) ────────────────────────────────────────────

    #[test]
    fn type_bounding_box_construction_and_equality() {
        assert_eq!(Type::BoundingBox, Type::BoundingBox);
        assert_ne!(Type::BoundingBox, Type::Plane);
        assert_ne!(Type::BoundingBox, Type::Axis);
        assert_ne!(Type::BoundingBox, Type::dimensionless_scalar());
    }

    #[test]
    fn type_bounding_box_display() {
        assert_eq!(format!("{}", Type::BoundingBox), "BoundingBox");
    }

    #[test]
    fn type_bounding_box_factory() {
        assert_eq!(Type::bounding_box(), Type::BoundingBox);
    }

    #[test]
    fn type_bounding_box_not_numeric() {
        assert!(!Type::BoundingBox.is_numeric());
    }

    #[test]
    fn type_bounding_box_as_name_none() {
        assert_eq!(Type::BoundingBox.as_name(), None);
    }

    // ── Error tests (task-448) ───────────────────────────────────────────────

    #[test]
    fn type_error_construction_and_equality() {
        // (a) Construction and equality
        assert_eq!(Type::Error, Type::Error);
        assert_ne!(Type::Error, Type::dimensionless_scalar());
        assert_ne!(Type::Error, Type::Int);
    }

    #[test]
    fn type_error_is_error_true() {
        // (b) Type::Error.is_error() returns true
        assert!(Type::Error.is_error());
    }

    #[test]
    fn type_error_is_error_false_for_others() {
        // (c) Other types return false from is_error()
        assert!(!Type::dimensionless_scalar().is_error());
        assert!(!Type::Int.is_error());
        assert!(!Type::List(Box::new(Type::Int)).is_error());
    }

    #[test]
    fn type_error_not_numeric() {
        // (d) Type::Error.is_numeric() returns false
        assert!(!Type::Error.is_numeric());
    }

    #[test]
    fn type_error_as_name_none() {
        // (e) Type::Error.as_name() returns None
        assert_eq!(Type::Error.as_name(), None);
    }

    #[test]
    fn type_error_display() {
        // (f) Display is "<error>"
        assert_eq!(format!("{}", Type::Error), "<error>");
    }

    // ── task-1913: is_error() top-level-only boundary pins ───────────────────
    // These tests DOCUMENT and PIN the fact that `is_error()` returns `false`
    // for compound types that contain `Type::Error` as an inner type parameter.
    // They are INTENTIONALLY written so that they PASS on current code (where
    // `is_error()` is top-level-only) and would FAIL if `is_error()` were
    // changed to recurse. Paired with the integration regression test at:
    //   crates/reify-compiler/tests/type_error_propagation_tests.rs
    //   ::nested_compound_error_cascades_through_trait_let_annotation
    //
    // If you are implementing a recursive `contains_error()` helper (option (a)
    // from the task-1913 design), you need to update both these tests and all
    // consumer guard sites (`type_compat.rs`, `expr.rs`, `conformance.rs`)
    // in a single coordinated change. See the `is_error()` doc comment for the
    // full follow-up plan.

    #[test]
    fn type_error_is_error_false_for_list_of_error() {
        // top-level-only boundary; see `contains_error` follow-up
        assert!(
            !Type::List(Box::new(Type::Error)).is_error(),
            "is_error() must return false for List<Error>: \
             top-level-only boundary; nested errors are not yet detected"
        );
    }

    #[test]
    fn type_error_is_error_false_for_option_of_error() {
        // top-level-only boundary; see `contains_error` follow-up
        assert!(
            !Type::Option(Box::new(Type::Error)).is_error(),
            "is_error() must return false for Option<Error>: \
             top-level-only boundary; nested errors are not yet detected"
        );
    }

    #[test]
    fn type_error_is_error_false_for_set_of_error() {
        // top-level-only boundary; see `contains_error` follow-up
        assert!(
            !Type::Set(Box::new(Type::Error)).is_error(),
            "is_error() must return false for Set<Error>: \
             top-level-only boundary; nested errors are not yet detected"
        );
    }

    #[test]
    fn type_error_is_error_false_for_map_value_of_error() {
        // top-level-only boundary; see `contains_error` follow-up
        assert!(
            !Type::Map(Box::new(Type::Int), Box::new(Type::Error)).is_error(),
            "is_error() must return false for Map<Int, Error>: \
             top-level-only boundary; nested errors are not yet detected"
        );
    }

    #[test]
    fn type_error_is_error_false_for_map_key_of_error() {
        // top-level-only boundary; see `contains_error` follow-up
        assert!(
            !Type::Map(Box::new(Type::Error), Box::new(Type::Int)).is_error(),
            "is_error() must return false for Map<Error, Int>: \
             top-level-only boundary; nested errors are not yet detected"
        );
    }

    // ── TraitObject tests (task-1874) ───────────────────────────────────────
    // The `Type::TraitObject(String)` variant represents a nominal reference to
    // a trait name used as a type (e.g. `param material : Material`). It mirrors
    // the `StructureRef`/`TypeParam`/`Enum` naming-variant pattern: Display emits
    // the bare name, `as_name` returns `Some(name)`, `is_numeric` returns false.

    #[test]
    fn type_trait_object_display() {
        assert_eq!(
            format!("{}", Type::TraitObject("Material".into())),
            "Material"
        );
        assert_eq!(format!("{}", Type::TraitObject("Rigid".into())), "Rigid");
    }

    #[test]
    fn type_trait_object_as_name_some() {
        assert_eq!(
            Type::TraitObject("Material".into()).as_name(),
            Some("Material")
        );
    }

    #[test]
    fn type_trait_object_not_numeric() {
        assert!(!Type::TraitObject("Material".into()).is_numeric());
    }

    #[test]
    fn type_trait_object_not_error() {
        assert!(!Type::TraitObject("Material".into()).is_error());
    }

    #[test]
    fn type_trait_object_eq_and_hash() {
        use std::collections::HashMap;

        let t_a = Type::TraitObject("Material".into());
        let t_b = Type::TraitObject("Material".into());
        let t_other = Type::TraitObject("Rigid".into());

        // Equality: same name equal, different name not equal
        assert_eq!(t_a, t_b);
        assert_ne!(t_a, t_other);

        // Distinctness: TraitObject("Material") != StructureRef("Material"),
        // because trait-typed values have different semantics than concrete
        // structure references.
        assert_ne!(t_a, Type::StructureRef("Material".into()));

        // Hash consistency via HashMap
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(t_a.clone(), "material");
        assert_eq!(map.get(&t_b), Some(&"material"));
        assert_eq!(map.get(&t_other), None);
    }

    // ── Union tests (task-2373) ─────────────────────────────────────────────
    // `Type::Union(Vec<Type>)` is a compile-time-only union over the arm-types
    // of a `match`-block decl cluster. See PRD `match-block-decls.md` §6.4.

    #[test]
    fn type_union_display_pipe_separated() {
        let union = Type::Union(vec![
            Type::StructureRef("HexHead".into()),
            Type::StructureRef("SocketHead".into()),
        ]);
        assert_eq!(format!("{}", union), "Union<HexHead | SocketHead>");
    }

    // ── AffineMap tests (step-1 RED / task 3958 α) ───────────────────────────

    #[test]
    fn type_affine_map_construction() {
        // Same dimension equals itself
        assert_eq!(Type::AffineMap(3), Type::AffineMap(3));
        // Distinct dimensions are not equal
        assert_ne!(Type::AffineMap(3), Type::AffineMap(2));
    }

    #[test]
    fn type_affine_map_display() {
        assert_eq!(format!("{}", Type::AffineMap(3)), "AffineMap3");
        assert_eq!(format!("{}", Type::AffineMap(2)), "AffineMap2");
    }

    #[test]
    fn type_affine_map_factory() {
        assert_eq!(Type::affine_map(3), Type::AffineMap(3));
        assert_eq!(Type::affine_map(2), Type::AffineMap(2));
    }

    #[test]
    fn type_affine_map_eq_and_hash() {
        use std::collections::HashMap;

        let a3a = Type::AffineMap(3);
        let a3b = Type::AffineMap(3);
        let a2 = Type::AffineMap(2);

        assert_eq!(a3a, a3b);
        assert_ne!(a3a, a2);
        // AffineMap != other types
        assert_ne!(a3a, Type::dimensionless_scalar());

        // Hash consistency
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(a3a.clone(), "a3");
        assert_eq!(map.get(&a3b), Some(&"a3"));
        assert_eq!(map.get(&a2), None);
    }

    #[test]
    fn type_affine_map_not_numeric() {
        assert!(!Type::AffineMap(3).is_numeric());
        assert!(!Type::AffineMap(2).is_numeric());
    }

    #[test]
    fn type_affine_map_as_name_none() {
        assert_eq!(Type::AffineMap(3).as_name(), None);
    }

    #[test]
    fn type_affine_map_ne_transform() {
        // AffineMap(3) and Transform(3) are semantically distinct (non-rigid vs rigid)
        assert_ne!(Type::AffineMap(3), Type::Transform(3));
    }

    #[test]
    fn type_affine_map_ne_frame() {
        assert_ne!(Type::AffineMap(3), Type::Frame(3));
    }

    #[test]
    fn type_affine_map_ne_orientation() {
        assert_ne!(Type::AffineMap(3), Type::Orientation(3));
    }

    // ── SelectorKind + Type::Selector tests (step-1 RED / task 4116 α) ─────────

    #[test]
    fn selector_kind_display() {
        // (a) Display: Face=>"FaceSelector", Edge=>"EdgeSelector", Body=>"BodySelector"
        assert_eq!(format!("{}", SelectorKind::Face), "FaceSelector");
        assert_eq!(format!("{}", SelectorKind::Edge), "EdgeSelector");
        assert_eq!(format!("{}", SelectorKind::Body), "BodySelector");
    }

    #[test]
    fn selector_kind_dimensionality() {
        // (b) Face=>2, Edge=>1, Body=>3 (per D2/§4.1)
        assert_eq!(SelectorKind::Face.dimensionality(), 2);
        assert_eq!(SelectorKind::Edge.dimensionality(), 1);
        assert_eq!(SelectorKind::Body.dimensionality(), 3);
    }

    #[test]
    fn selector_kind_eq_and_hash() {
        use std::collections::HashMap;

        // (c) eq/inequality
        assert_eq!(SelectorKind::Face, SelectorKind::Face);
        assert_ne!(SelectorKind::Face, SelectorKind::Edge);
        assert_ne!(SelectorKind::Face, SelectorKind::Body);
        assert_ne!(SelectorKind::Edge, SelectorKind::Body);

        // HashMap round-trip (derives Eq+Hash)
        let mut map: HashMap<SelectorKind, &str> = HashMap::new();
        map.insert(SelectorKind::Face, "face");
        map.insert(SelectorKind::Edge, "edge");
        assert_eq!(map.get(&SelectorKind::Face), Some(&"face"));
        assert_eq!(map.get(&SelectorKind::Edge), Some(&"edge"));
        assert_eq!(map.get(&SelectorKind::Body), None);
    }

    #[test]
    fn type_selector_construction_and_equality() {
        // (d) Type::Selector construction + equality
        assert_eq!(Type::Selector(SelectorKind::Face), Type::Selector(SelectorKind::Face));
        assert_ne!(Type::Selector(SelectorKind::Face), Type::Selector(SelectorKind::Edge));
        assert_ne!(Type::Selector(SelectorKind::Face), Type::Selector(SelectorKind::Body));
        assert_ne!(Type::Selector(SelectorKind::Edge), Type::Selector(SelectorKind::Body));
        // Factory: Type::selector(kind) == Type::Selector(kind)
        assert_eq!(Type::selector(SelectorKind::Face), Type::Selector(SelectorKind::Face));
        assert_eq!(Type::selector(SelectorKind::Edge), Type::Selector(SelectorKind::Edge));
        assert_eq!(Type::selector(SelectorKind::Body), Type::Selector(SelectorKind::Body));
    }

    #[test]
    fn type_selector_display() {
        // (e) Display delegates to SelectorKind::Display
        assert_eq!(format!("{}", Type::Selector(SelectorKind::Face)), "FaceSelector");
        assert_eq!(format!("{}", Type::Selector(SelectorKind::Edge)), "EdgeSelector");
        assert_eq!(format!("{}", Type::Selector(SelectorKind::Body)), "BodySelector");
    }

    #[test]
    fn type_selector_not_numeric() {
        // (f) is_numeric() returns false
        assert!(!Type::Selector(SelectorKind::Face).is_numeric());
        assert!(!Type::Selector(SelectorKind::Edge).is_numeric());
        assert!(!Type::Selector(SelectorKind::Body).is_numeric());
    }

    #[test]
    fn type_selector_as_name_none() {
        // (f) as_name() returns None
        assert_eq!(Type::Selector(SelectorKind::Face).as_name(), None);
        assert_eq!(Type::Selector(SelectorKind::Edge).as_name(), None);
        assert_eq!(Type::Selector(SelectorKind::Body).as_name(), None);
    }

    #[test]
    fn type_selector_eq_and_hash() {
        use std::collections::HashMap;

        let sf_a = Type::Selector(SelectorKind::Face);
        let sf_b = Type::Selector(SelectorKind::Face);
        let se = Type::Selector(SelectorKind::Edge);

        assert_eq!(sf_a, sf_b);
        assert_ne!(sf_a, se);
        // Selector(Face) != Real
        assert_ne!(sf_a, Type::dimensionless_scalar());

        // Hash consistency
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(sf_a.clone(), "face_sel");
        assert_eq!(map.get(&sf_b), Some(&"face_sel"));
        assert_eq!(map.get(&se), None);
    }

    #[test]
    fn type_selector_ne_other_types() {
        // Selector kind != non-Selector types
        assert_ne!(Type::Selector(SelectorKind::Face), Type::Plane);
        assert_ne!(Type::Selector(SelectorKind::Face), Type::Geometry);
        assert_ne!(Type::Selector(SelectorKind::Body), Type::dimensionless_scalar());
    }

    // ── Keyed tests (step-1 RED / task 3930 β) ───────────────────────────────
    // `Type::Keyed(Box<Type>)` is the keyed sub-collection kind — members
    // addressed by an author-assigned String key. Distinct from `Map` (value
    // collection) and `List` (positional). See PRD keyed-collection-identity.md.

    #[test]
    fn type_keyed_display() {
        // (a) Display: Keyed<{inner}>
        assert_eq!(
            format!(
                "{}",
                Type::Keyed(Box::new(Type::StructureRef("Vent".into())))
            ),
            "Keyed<Vent>"
        );
        assert_eq!(format!("{}", Type::Keyed(Box::new(Type::Int))), "Keyed<Int>");
    }

    #[test]
    fn type_keyed_distinct_from_map_list_set() {
        // (b) Keyed<Int> is a distinct kind from List<Int>, Map<String,Int>, Set<Int>
        let keyed_int = Type::Keyed(Box::new(Type::Int));
        assert_ne!(keyed_int, Type::List(Box::new(Type::Int)));
        assert_ne!(
            keyed_int,
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
        assert_ne!(keyed_int, Type::Set(Box::new(Type::Int)));
    }

    #[test]
    fn type_keyed_fall_through_predicates() {
        // (c) Falls through is_numeric (false), is_error (false), as_name (None)
        assert!(!Type::Keyed(Box::new(Type::Int)).is_numeric());
        assert!(!Type::Keyed(Box::new(Type::Error)).is_error());
        assert_eq!(Type::Keyed(Box::new(Type::Int)).as_name(), None);
    }

    #[test]
    fn type_keyed_eq_and_hash() {
        use std::collections::HashMap;

        let k_int_a = Type::Keyed(Box::new(Type::Int));
        let k_int_b = Type::Keyed(Box::new(Type::Int));
        let k_real = Type::Keyed(Box::new(Type::dimensionless_scalar()));

        // Same inner type equal; different inner not equal
        assert_eq!(k_int_a, k_int_b);
        assert_ne!(k_int_a, k_real);
        // Keyed(Int) != Int
        assert_ne!(k_int_a, Type::Int);

        // Hash consistency via HashMap insert+lookup (mirrors type_list / point tests)
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(k_int_a.clone(), "k_int");
        assert_eq!(map.get(&k_int_b), Some(&"k_int"));
        assert_eq!(map.get(&k_real), None);
    }

    // Step-1 RED: dimensionless scalar Display prints "Real" (fails today: prints "Scalar").
    #[test]
    fn dimensionless_scalar_displays_real() {
        assert_eq!(format!("{}", Type::dimensionless_scalar()), "Real");
    }

    // Step-1 guard: dimensioned scalars are not affected by the Display change.
    #[test]
    fn dimensioned_scalar_display_unchanged() {
        assert_eq!(format!("{}", Type::length()), "Scalar[m]");
        assert_eq!(format!("{}", Type::angle()), "Scalar[rad]");
    }

    // ── ScalarParam tests (task 4234 ε: dimension-kinded params) ─────────────
    // `Type::ScalarParam(String)` — a dimensioned scalar whose dimension is the
    // named dimension-param; compile-time/signature-only, erased before eval.

    #[test]
    fn type_scalar_param_display() {
        // (a) Display renders Scalar<Q>
        assert_eq!(
            format!("{}", Type::ScalarParam("Q".into())),
            "Scalar<Q>"
        );
        assert_eq!(
            format!("{}", Type::ScalarParam("Length".into())),
            "Scalar<Length>"
        );
    }

    #[test]
    fn type_scalar_param_eq_and_hash() {
        use std::collections::HashMap;

        // (b) Same name → equal; different name → not equal
        let sp_q_a = Type::ScalarParam("Q".into());
        let sp_q_b = Type::ScalarParam("Q".into());
        let sp_r = Type::ScalarParam("R".into());

        assert_eq!(sp_q_a, sp_q_b);
        assert_ne!(sp_q_a, sp_r);

        // Hash consistency via HashMap round-trip
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(sp_q_a.clone(), "q_param");
        assert_eq!(map.get(&sp_q_b), Some(&"q_param"));
        assert_eq!(map.get(&sp_r), None);
    }

    #[test]
    fn type_scalar_param_distinctness() {
        // (c) ScalarParam("Q") != concrete Scalar variants, != TypeParam("Q")
        let sp_q = Type::ScalarParam("Q".into());
        assert_ne!(sp_q, Type::Scalar { dimension: DimensionVector::LENGTH });
        assert_ne!(sp_q, Type::dimensionless_scalar());
        assert_ne!(sp_q, Type::TypeParam("Q".into()));
    }

    #[test]
    fn type_scalar_param_not_numeric_not_error() {
        // (d) is_numeric()==false, is_error()==false, as_name()==None
        let sp_q = Type::ScalarParam("Q".into());
        assert!(!sp_q.is_numeric());
        assert!(!sp_q.is_error());
        assert_eq!(sp_q.as_name(), None);
    }

    // ── Relation tests (γ: geometric-relations Relation directive type) ───────
    // `Type::Relation` is a DOF-removal directive (geometric-relations γ, task
    // 4383): it carries NO truth value, so it is distinct from `Bool`, and it is
    // distinct from every datum type (Axis/Plane/Direction/Frame). RED until the
    // variant lands in step-2 (compile failure is the documented RED state).

    #[test]
    fn type_relation_construction_and_equality() {
        // Equal to itself.
        assert_eq!(Type::Relation, Type::Relation);
        // Distinct from Bool — a Relation is a directive, not a truth value.
        assert_ne!(Type::Relation, Type::Bool);
        // Distinct from every datum type.
        assert_ne!(Type::Relation, Type::Axis);
        assert_ne!(Type::Relation, Type::Plane);
        assert_ne!(Type::Relation, Type::Direction);
        assert_ne!(Type::Relation, Type::Frame(3));
        // Distinct from a plain scalar.
        assert_ne!(Type::Relation, Type::dimensionless_scalar());
    }

    #[test]
    fn type_relation_display() {
        // Display renders exactly "Relation".
        assert_eq!(format!("{}", Type::Relation), "Relation");
    }

    #[test]
    fn type_relation_factory() {
        assert_eq!(Type::relation(), Type::Relation);
    }

    #[test]
    fn type_relation_eq_and_hash() {
        use std::collections::HashMap;
        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(Type::Relation, "relation");
        assert_eq!(map.get(&Type::Relation), Some(&"relation"));
        assert_eq!(map.get(&Type::Bool), None);
    }

    #[test]
    fn type_relation_not_numeric() {
        // A directive type is not numeric.
        assert!(!Type::Relation.is_numeric());
    }

    #[test]
    fn type_relation_not_error() {
        assert!(!Type::Relation.is_error());
    }

    #[test]
    fn type_relation_as_name_none() {
        // No nominal name (not a name-carrying variant).
        assert_eq!(Type::Relation.as_name(), None);
    }

    // ── Applied / Projection tests (step-1 RED / task 4602 β) ────────────────
    // Type::Applied { name, args } and Type::Projection { base, member } do NOT
    // exist until step-2. These tests fail to COMPILE until then — compile
    // failure IS the RED signal, consistent with every prior variant addition
    // (Type::Tuple/task-3924, Type::AffineMap/task-3958, etc.).

    #[test]
    fn type_applied_display_single_arg() {
        assert_eq!(
            format!(
                "{}",
                Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())])
            ),
            "Coupling<Prismatic>"
        );
    }

    #[test]
    fn type_applied_display_multi_arg() {
        assert_eq!(
            format!(
                "{}",
                Type::applied(
                    "Coupling",
                    vec![
                        Type::StructureRef("Prismatic".into()),
                        Type::StructureRef("Revolute".into()),
                    ]
                )
            ),
            "Coupling<Prismatic, Revolute>"
        );
    }

    #[test]
    fn type_projection_display_structure_ref_base() {
        assert_eq!(
            format!(
                "{}",
                Type::projection(Type::StructureRef("Prismatic".into()), "MotionValue")
            ),
            "Prismatic::MotionValue"
        );
    }

    #[test]
    fn type_projection_display_type_param_base() {
        assert_eq!(
            format!("{}", Type::projection(Type::TypeParam("P".into()), "MotionValue")),
            "P::MotionValue"
        );
    }

    #[test]
    fn type_projection_display_applied_base() {
        assert_eq!(
            format!(
                "{}",
                Type::projection(
                    Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]),
                    "MotionValue"
                )
            ),
            "Coupling<Prismatic>::MotionValue"
        );
    }

    #[test]
    fn type_applied_factory_eq_variant() {
        let name = "Coupling".to_string();
        let args = vec![Type::StructureRef("Prismatic".into())];
        assert_eq!(
            Type::applied("Coupling", args.clone()),
            Type::Applied {
                name: name.clone(),
                args: args.clone(),
            }
        );
    }

    #[test]
    fn type_projection_factory_eq_variant() {
        let base = Type::StructureRef("Prismatic".into());
        let member = "MotionValue".to_string();
        assert_eq!(
            Type::projection(base.clone(), "MotionValue"),
            Type::Projection {
                base: Box::new(base),
                member,
            }
        );
    }

    #[test]
    fn type_applied_eq_same() {
        let a = Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]);
        let b = Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]);
        assert_eq!(a, b);
    }

    #[test]
    fn type_applied_ne_different_args() {
        let a = Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]);
        let b = Type::applied("Coupling", vec![Type::StructureRef("Revolute".into())]);
        assert_ne!(a, b);
    }

    #[test]
    fn type_applied_ne_different_name() {
        let a = Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]);
        let b = Type::applied("Other", vec![Type::StructureRef("Prismatic".into())]);
        assert_ne!(a, b);
    }

    #[test]
    fn type_applied_ne_structure_ref() {
        let a = Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]);
        let b = Type::StructureRef("Coupling".into());
        assert_ne!(a, b);
    }

    #[test]
    fn type_projection_ne_different_member() {
        let a = Type::projection(Type::StructureRef("Prismatic".into()), "MotionValue");
        let b = Type::projection(Type::StructureRef("Prismatic".into()), "Other");
        assert_ne!(a, b);
    }

    #[test]
    fn type_projection_ne_different_base() {
        let a = Type::projection(Type::StructureRef("Prismatic".into()), "MotionValue");
        let b = Type::projection(Type::StructureRef("Revolute".into()), "MotionValue");
        assert_ne!(a, b);
    }

    #[test]
    fn type_applied_and_projection_hash_roundtrip() {
        use std::collections::HashMap;

        let applied = Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]);
        let applied2 = Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]);
        let projection =
            Type::projection(Type::StructureRef("Prismatic".into()), "MotionValue");
        let projection2 =
            Type::projection(Type::StructureRef("Prismatic".into()), "MotionValue");

        let mut map: HashMap<Type, &str> = HashMap::new();
        map.insert(applied.clone(), "applied");
        assert_eq!(map.get(&applied2), Some(&"applied"));

        map.insert(projection.clone(), "projection");
        assert_eq!(map.get(&projection2), Some(&"projection"));
    }

    #[test]
    fn type_applied_not_numeric() {
        assert!(
            !Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]).is_numeric()
        );
    }

    #[test]
    fn type_projection_not_numeric() {
        assert!(
            !Type::projection(Type::StructureRef("Prismatic".into()), "MotionValue").is_numeric()
        );
    }

    #[test]
    fn type_applied_as_name_none() {
        assert_eq!(
            Type::applied("Coupling", vec![Type::StructureRef("Prismatic".into())]).as_name(),
            None
        );
    }

    #[test]
    fn type_projection_as_name_none() {
        assert_eq!(
            Type::projection(Type::StructureRef("Prismatic".into()), "MotionValue").as_name(),
            None
        );
    }
}
