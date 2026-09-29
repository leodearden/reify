//! Shared ANGLE `CompiledExpr` literal constructor for the `reify-eval`
//! integration-test binaries.
//!
//! Consumers include it with
//! `#[path = "../common/angle_expr.rs"] mod angle_expr;`, following the
//! `common/differential.rs` precedent already in this tree.
//!
//! `reify_test_support::values` (beside `mm()`) is the natural long-term home
//! for this, and for the per-file LENGTH literal constructors that still shadow
//! one another; consolidating there is follow-up work.

/// Build a `CompiledExpr` literal from an ANGLE-dimensioned scalar (SI radians).
///
/// The angle-bearing args of `rotate` / `rotate_around` / `revolve` / `arc`
/// require a dimensioned Angle since PRD 3 leaf γ — a bare literal in those
/// positions is now rejected at eval, exactly as a bare length already was.
pub fn angle_literal(radians: f64) -> reify_ir::CompiledExpr {
    reify_ir::CompiledExpr::literal(reify_ir::Value::angle(radians), reify_core::Type::angle())
}
