use reify_core::Type;
use reify_ir::CompiledExpr;

use super::expr::{eq, gt, lt, value_ref_typed};

// --- Constraint expression helpers ---

/// Build a pair of range-check expressions for an entity member.
///
/// Returns `vec![member > min_expr, member < max_expr]` — exactly two `CompiledExpr`
/// values, both with `result_type == Type::Bool`.  Callers wrap each expression into
/// a `CompiledConstraint` via `TopologyTemplateBuilder::constraint(entity, idx, label, expr)`
/// using their own chosen indices, so no `ConstraintNodeId` is ever hardcoded here.
///
/// This is safe to call multiple times for the same entity (e.g., once for `width`,
/// once for `height`) because no index is allocated inside this function.
pub fn range_constraint(
    entity: &str,
    member: &str,
    cell_type: Type,
    min_expr: CompiledExpr,
    max_expr: CompiledExpr,
) -> Vec<CompiledExpr> {
    let member_ref = value_ref_typed(entity, member, cell_type);
    vec![gt(member_ref.clone(), min_expr), lt(member_ref, max_expr)]
}

/// Build a single equality-check expression for an entity member.
///
/// Returns `vec![member == target_expr]` — exactly one `CompiledExpr` with
/// `result_type == Type::Bool`.  Return type matches `range_constraint` so callers
/// can iterate over results uniformly.
pub fn equality_constraint(
    entity: &str,
    member: &str,
    cell_type: Type,
    target_expr: CompiledExpr,
) -> Vec<CompiledExpr> {
    let member_ref = value_ref_typed(entity, member, cell_type);
    vec![eq(member_ref, target_expr)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builders::literal;
    use reify_ir::{BinOp, CompiledExprKind, Value};

    #[test]
    fn equality_constraint_returns_single_bool_expr() {
        let exprs = equality_constraint("Beam", "ratio", Type::Real, literal(Value::Real(2.0)));
        assert_eq!(
            exprs.len(),
            1,
            "equality_constraint should return exactly 1 expr"
        );
        assert_eq!(exprs[0].result_type, Type::Bool, "expr should be Bool");
        assert!(
            matches!(
                &exprs[0].kind,
                CompiledExprKind::BinOp { op: BinOp::Eq, .. }
            ),
            "expr should be Eq"
        );
    }

    #[test]
    fn range_constraint_composable_for_multiple_members() {
        // Call range_constraint twice for different members of the same entity.
        // All 4 resulting expressions should be valid Bool expressions.
        // This proves the API is safe for repeated calls (core fix for S1).
        let width_exprs = range_constraint(
            "Beam",
            "width",
            Type::length(),
            literal(crate::mm(10.0)),
            literal(crate::mm(500.0)),
        );
        let height_exprs = range_constraint(
            "Beam",
            "height",
            Type::length(),
            literal(crate::mm(10.0)),
            literal(crate::mm(1000.0)),
        );
        let all_exprs: Vec<_> = width_exprs.into_iter().chain(height_exprs).collect();
        assert_eq!(
            all_exprs.len(),
            4,
            "should have 4 constraint expressions total"
        );
        for expr in &all_exprs {
            assert_eq!(expr.result_type, Type::Bool, "all exprs should be Bool");
        }
    }

    #[test]
    fn range_constraint_returns_two_bool_exprs() {
        let exprs = range_constraint(
            "Beam",
            "width",
            Type::length(),
            literal(crate::mm(10.0)),
            literal(crate::mm(500.0)),
        );
        assert_eq!(
            exprs.len(),
            2,
            "range_constraint should return exactly 2 exprs"
        );
        assert_eq!(
            exprs[0].result_type,
            Type::Bool,
            "first expr should be Bool"
        );
        assert_eq!(
            exprs[1].result_type,
            Type::Bool,
            "second expr should be Bool"
        );
        // First expr should be a Gt comparison, second a Lt comparison
        assert!(
            matches!(
                &exprs[0].kind,
                CompiledExprKind::BinOp { op: BinOp::Gt, .. }
            ),
            "first expr should be Gt"
        );
        assert!(
            matches!(
                &exprs[1].kind,
                CompiledExprKind::BinOp { op: BinOp::Lt, .. }
            ),
            "second expr should be Lt"
        );
    }
}
