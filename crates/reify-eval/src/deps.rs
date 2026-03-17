use reify_types::{CompiledExpr, CompiledExprKind, ValueCellId};

/// Tracks which value cells a node read during evaluation.
///
/// This is a minimal stub for task 12 (content-hash caching).
/// Task 11 will replace this with a full dependency tracing implementation.
#[derive(Debug, Clone, Default)]
pub struct DependencyTrace {
    pub reads: Vec<ValueCellId>,
}

/// Extract a dependency trace from a compiled expression by collecting all ValueRef ids.
pub fn extract_dependency_trace(expr: &CompiledExpr) -> DependencyTrace {
    let mut reads = Vec::new();
    collect_value_refs(expr, &mut reads);
    DependencyTrace { reads }
}

/// Recursively collect all ValueRef ids from a compiled expression tree.
pub fn collect_value_refs(expr: &CompiledExpr, out: &mut Vec<ValueCellId>) {
    match &expr.kind {
        CompiledExprKind::Literal(_) => {}
        CompiledExprKind::ValueRef(id) => {
            out.push(id.clone());
        }
        CompiledExprKind::BinOp { left, right, .. } => {
            collect_value_refs(left, out);
            collect_value_refs(right, out);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            collect_value_refs(operand, out);
        }
        CompiledExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_value_refs(arg, out);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_value_refs(condition, out);
            collect_value_refs(then_branch, out);
            collect_value_refs(else_branch, out);
        }
    }
}

/// Extract dependency ValueCellIds from a CompiledGeometryOp's argument expressions.
///
/// Walks all expression arguments in Primitive, Modify, and Transform ops.
/// Boolean ops have no expression arguments (just geometry refs).
pub fn extract_realization_dependencies(
    ops: &[reify_compiler::CompiledGeometryOp],
) -> DependencyTrace {
    let mut reads = Vec::new();
    for op in ops {
        match op {
            reify_compiler::CompiledGeometryOp::Primitive { args, .. } => {
                for (_, expr) in args {
                    collect_value_refs(expr, &mut reads);
                }
            }
            reify_compiler::CompiledGeometryOp::Boolean { .. } => {
                // Boolean ops reference geometry handles, not value cells
            }
            reify_compiler::CompiledGeometryOp::Modify { args, .. } => {
                for (_, expr) in args {
                    collect_value_refs(expr, &mut reads);
                }
            }
            reify_compiler::CompiledGeometryOp::Transform { args, .. } => {
                for (_, expr) in args {
                    collect_value_refs(expr, &mut reads);
                }
            }
        }
    }
    DependencyTrace { reads }
}
