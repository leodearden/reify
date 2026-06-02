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
// STUB (pre-2): empty until populated in step-10. `#[allow(dead_code)]`
// because `mod math_signatures` is private and nothing references this yet —
// the first reference (`is_math_typed_fn` + the units.rs test module) lands in
// steps 9/10, where this allow is removed.
#[allow(dead_code)]
pub const MATH_CONSTRUCTION_NAMES: &[&str] = &[];

/// Is `name` a math-linalg construction builtin? Name-only classification,
/// mirroring `units::is_geometry_query`.
//
// STUB (pre-2): always `false` until step-10. `#[allow(dead_code)]` because it
// is not yet referenced from production code — the `expr.rs` arm that calls it
// is wired in step-14, where this allow is removed.
#[allow(dead_code)]
pub(crate) fn is_math_typed_fn(_name: &str) -> bool {
    false
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
}
