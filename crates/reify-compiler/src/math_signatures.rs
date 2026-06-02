//! Compiler signatures for the math-linalg **construction** builtins
//! (math-linalg α, task 4179) — the frozen §3 contract.
//!
//! Holds the single source of truth for the construction-builtin name family
//! ([`MATH_CONSTRUCTION_NAMES`]), the name-only classification predicate
//! ([`is_math_typed_fn`], mirroring `units::is_geometry_query`), and the
//! shape-dependent result-type resolver ([`math_fn_result_type`]).
//!
//! Unlike the name-only `geometry_query_result_type`, `math_fn_result_type`
//! takes `&[CompiledExpr]` because the construction builtins' return *shape*
//! (the `n` of a `Vector{n}` / `Tensor{rank,n}`) is recovered from the COMPILED
//! ARGUMENT STRUCTURE — list length from a `CompiledExprKind::ListLiteral`, the
//! literal value from `CompiledExprKind::Literal(Value::Int)` — since
//! `Type::List` carries no length.
//!
//! Wired into `expr.rs::resolve_function_overload`'s `NoUserFunctions` ladder
//! (the `is_math_typed_fn` arm) in step-14. The family is pinned disjoint from
//! the geometry / dynamics families by the `units.rs` disjointness tests.

use reify_core::Type;
use reify_ir::CompiledExpr;

/// The complete set of math-linalg **construction** builtin names recognised
/// by the compiler. Single source of truth — imported into the `units.rs` test
/// module to pin disjointness from the geometry / dynamics families.
///
/// Case-sensitive: Reify function names are snake_case.
//
// `#[allow(dead_code)]` until step-14: the units.rs test module references this
// slice (`#[cfg(test)]`), and `is_math_typed_fn` reads it, but neither is
// reachable from a non-test build until the `expr.rs` arm is wired in step-14 —
// the allow is removed there once the family is production-live. (β extends this
// slice with the linear-algebra operation names later.)
#[allow(dead_code)]
pub const MATH_CONSTRUCTION_NAMES: &[&str] = &["vec", "matrix", "diag", "identity"];

/// Is `name` a math-linalg construction builtin? Name-only classification,
/// mirroring `units::is_geometry_query` (a `.contains` over the single-source-of-
/// truth [`MATH_CONSTRUCTION_NAMES`] slice). Case-sensitive.
//
// `#[allow(dead_code)]` until step-14: the only production caller is the
// `expr.rs::resolve_function_overload` arm wired in step-14 (test code aside);
// the allow is removed there.
#[allow(dead_code)]
pub(crate) fn is_math_typed_fn(name: &str) -> bool {
    MATH_CONSTRUCTION_NAMES.contains(&name)
}

/// Result type for a math-linalg construction builtin, derived from the
/// compiled argument structure.
//
// STUB (pre-2): always `Type::Real` until step-12. `#[allow(dead_code)]`
// because it is not yet referenced from production code — the `expr.rs` arm
// that calls it is wired in step-14, where this allow is removed.
#[allow(dead_code)]
pub(crate) fn math_fn_result_type(_name: &str, _args: &[CompiledExpr]) -> Type {
    Type::Real
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four construction-builtin names, frozen by the §3 contract. Local
    /// fixture so a drift in `MATH_CONSTRUCTION_NAMES` is caught against an
    /// independent list rather than against itself.
    const EXPECTED_NAMES: [&str; 4] = ["vec", "matrix", "diag", "identity"];

    // ── Name-family contract (step-9 RED / step-10 GREEN) ────────────────────

    /// `is_math_typed_fn` recognises every construction-builtin name.
    #[test]
    fn is_math_typed_fn_recognises_all_construction_names() {
        for name in EXPECTED_NAMES {
            assert!(
                is_math_typed_fn(name),
                "is_math_typed_fn({name:?}) must be true (math-linalg α §3 contract)"
            );
        }
    }

    /// `is_math_typed_fn` rejects names from the other builtin families, the
    /// empty name, an unrelated name, and a sibling math name that α does NOT
    /// register (`determinant` is a β operation, not an α constructor).
    #[test]
    fn is_math_typed_fn_rejects_other_family_and_unknown_names() {
        // Geometry-query family (`units::GEOMETRY_QUERY_NAMES`).
        assert!(
            !is_math_typed_fn("volume"),
            "must reject geometry-query 'volume'"
        );
        // Dynamics-query family (`units::DYNAMICS_QUERY_NAMES`).
        assert!(
            !is_math_typed_fn("body_mass_props"),
            "must reject dynamics-query 'body_mass_props'"
        );
        // A linear-algebra OPERATION (task β), deliberately NOT an α constructor.
        assert!(
            !is_math_typed_fn("determinant"),
            "must reject 'determinant' — a β operation, not an α construction builtin"
        );
        // Empty / unrelated.
        assert!(!is_math_typed_fn(""), "must reject empty name");
        assert!(
            !is_math_typed_fn("does_not_exist"),
            "must reject unrelated name"
        );
    }

    /// Case-sensitivity invariant: Reify function names are snake_case, so the
    /// PascalCase forms must not match (mirrors `is_geometry_query_is_case_sensitive`).
    #[test]
    fn is_math_typed_fn_is_case_sensitive() {
        assert!(!is_math_typed_fn("Vec"));
        assert!(!is_math_typed_fn("Matrix"));
        assert!(!is_math_typed_fn("Diag"));
        assert!(!is_math_typed_fn("Identity"));
    }

    /// `MATH_CONSTRUCTION_NAMES` is exactly the four construction names —
    /// membership both ways plus an exact count (so neither a missing nor an
    /// extra name slips through).
    #[test]
    fn math_construction_names_are_exactly_the_four() {
        assert_eq!(
            MATH_CONSTRUCTION_NAMES.len(),
            EXPECTED_NAMES.len(),
            "MATH_CONSTRUCTION_NAMES must hold exactly {} names, got {:?}",
            EXPECTED_NAMES.len(),
            MATH_CONSTRUCTION_NAMES
        );
        for name in EXPECTED_NAMES {
            assert!(
                MATH_CONSTRUCTION_NAMES.contains(&name),
                "MATH_CONSTRUCTION_NAMES must contain {name:?}"
            );
        }
    }

    // ── Result-type resolution (step-11 RED / step-12 GREEN) ─────────────────

    use reify_core::DimensionVector;
    use reify_core::identity::ValueCellId;
    use reify_ir::Value;

    /// A dimensionless `Real` element expression (`result_type = Type::Real`).
    fn real_elem(v: f64) -> CompiledExpr {
        CompiledExpr::literal(Value::Real(v), Type::Real)
    }

    /// A `Scalar<Length>` element expression.
    fn length_elem(v: f64) -> CompiledExpr {
        CompiledExpr::literal(
            Value::Scalar {
                si_value: v,
                dimension: DimensionVector::LENGTH,
            },
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        )
    }

    /// A `ListLiteral` of `elems` whose own `result_type` is `List(elem_ty)`.
    /// `math_fn_result_type` reads N + quantity from the ELEMENT structure, not
    /// from this outer result_type, so its exact value is immaterial — set
    /// realistically anyway.
    fn list_lit(elems: Vec<CompiledExpr>, elem_ty: Type) -> CompiledExpr {
        CompiledExpr::list_literal(elems, Type::List(Box::new(elem_ty)))
    }

    /// (a) `vec` over a 3-element dimensionless `ListLiteral` →
    /// `Vector{n:3, quantity:Real}`.
    #[test]
    fn vec_result_type_dimensionless_is_vector_n3_real() {
        let arg = list_lit(vec![real_elem(1.0), real_elem(2.0), real_elem(3.0)], Type::Real);
        assert_eq!(
            math_fn_result_type("vec", &[arg]),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// (b) `vec` over `Scalar<Length>` elements → `Vector{n:2, quantity:Scalar<Length>}`.
    #[test]
    fn vec_result_type_length_preserves_quantity() {
        let len_ty = Type::Scalar {
            dimension: DimensionVector::LENGTH,
        };
        let arg = list_lit(vec![length_elem(1.0), length_elem(2.0)], len_ty.clone());
        assert_eq!(
            math_fn_result_type("vec", &[arg]),
            Type::Vector {
                n: 2,
                quantity: Box::new(len_ty)
            }
        );
    }

    /// (c) `matrix` over a depth-2 2×2 `ListLiteral` → `Tensor{rank:2, n:2, quantity:Real}`.
    #[test]
    fn matrix_result_type_2x2_is_tensor_rank2_n2_real() {
        let row0 = list_lit(vec![real_elem(1.0), real_elem(2.0)], Type::Real);
        let row1 = list_lit(vec![real_elem(3.0), real_elem(4.0)], Type::Real);
        let arg = list_lit(vec![row0, row1], Type::List(Box::new(Type::Real)));
        assert_eq!(
            math_fn_result_type("matrix", &[arg]),
            Type::Tensor {
                rank: 2,
                n: 2,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// (d) `diag` over a 3-element `ListLiteral` → `Tensor{rank:2, n:3, quantity:Real}`.
    #[test]
    fn diag_result_type_is_tensor_rank2_n3_real() {
        let arg = list_lit(vec![real_elem(3.0), real_elem(5.0), real_elem(7.0)], Type::Real);
        assert_eq!(
            math_fn_result_type("diag", &[arg]),
            Type::Tensor {
                rank: 2,
                n: 3,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// (e) `identity` over `Literal(Value::Int(4))` → `Tensor{rank:2, n:4, quantity:Real}`
    /// (dimensionless).
    #[test]
    fn identity_result_type_is_tensor_rank2_n4_real() {
        let arg = CompiledExpr::literal(Value::Int(4), Type::Int);
        assert_eq!(
            math_fn_result_type("identity", &[arg]),
            Type::Tensor {
                rank: 2,
                n: 4,
                quantity: Box::new(Type::Real)
            }
        );
    }

    /// (f) DEGRADE (locks D7): a non-literal `vec` arg — a `ValueRef` typed
    /// `List(Real)` whose length is NOT statically known — must STILL resolve to
    /// a `Type::Vector{..}` variant (quantity recovered from the `List` element),
    /// NEVER the first-arg `Type::List`. Falling through to `List` would make the
    /// eval'd `Value::Vector` fail `value_type_kind_matches` at runtime.
    #[test]
    fn vec_result_type_non_literal_arg_degrades_to_vector_not_list() {
        let arg = CompiledExpr::value_ref(
            ValueCellId::new("S", "x"),
            Type::List(Box::new(Type::Real)),
        );
        let result = math_fn_result_type("vec", &[arg]);
        assert!(
            !matches!(result, Type::List(_)),
            "non-literal vec arg must NOT degrade to Type::List (D7), got {result:?}"
        );
        match result {
            Type::Vector { quantity, .. } => assert_eq!(
                *quantity,
                Type::Real,
                "degraded Vector quantity should be recovered from the List element"
            ),
            other => panic!("expected a Type::Vector variant, got {other:?}"),
        }
    }
}
