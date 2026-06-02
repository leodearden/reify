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
    // Name-family contract tests land in step-9 (RED) → step-10 (GREEN);
    // result-type tests land in step-11 (RED) → step-12 (GREEN). Placeholder
    // module so the crate compiles with the math_signatures.rs scaffold.
}
