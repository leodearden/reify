use crate::hash::ContentHash;
use crate::identity::ValueCellId;
use crate::ty::Type;
use crate::value::Value;

/// A compiled expression tree — fully resolved, ready for evaluation.
/// Shared by reify-eval and reify-constraints (via reify-expr).
#[derive(Debug, Clone)]
pub struct CompiledExpr {
    pub kind: CompiledExprKind,
    pub result_type: Type,
    pub content_hash: ContentHash,
}

/// The kinds of compiled expression nodes.
#[derive(Debug, Clone)]
pub enum CompiledExprKind {
    /// Literal value.
    Literal(Value),
    /// Reference to a value cell.
    ValueRef(ValueCellId),
    /// Binary operation.
    BinOp {
        op: BinOp,
        left: Box<CompiledExpr>,
        right: Box<CompiledExpr>,
    },
    /// Unary operation.
    UnOp {
        op: UnOp,
        operand: Box<CompiledExpr>,
    },
    /// Call to a resolved function (stdlib or built-in).
    FunctionCall {
        function: ResolvedFunction,
        args: Vec<CompiledExpr>,
    },
    /// Conditional expression: if cond then a else b.
    Conditional {
        condition: Box<CompiledExpr>,
        then_branch: Box<CompiledExpr>,
        else_branch: Box<CompiledExpr>,
    },
    /// Match expression: match discriminant { pattern => body, ... }
    Match {
        discriminant: Box<CompiledExpr>,
        arms: Vec<CompiledMatchArm>,
    },
    /// Call to a user-defined function.
    UserFunctionCall {
        function_name: String,
        args: Vec<CompiledExpr>,
    },
    /// Lambda expression: |params| body with captured outer-scope references.
    Lambda {
        params: Vec<(String, Option<Type>)>,
        param_ids: Vec<ValueCellId>,
        body: Box<CompiledExpr>,
        captures: Vec<ValueCellId>,
    },
}

/// A compiled match arm.
#[derive(Debug, Clone)]
pub struct CompiledMatchArm {
    pub patterns: Vec<String>,
    pub body: CompiledExpr,
}

/// Binary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// Unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnOp {
    Neg,
    Not,
}

/// A fully resolved function reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedFunction {
    pub name: String,
    /// Unique identifier for dispatch (e.g., "std::math::sin").
    pub qualified_name: String,
}

/// A compiled user-defined function.
#[derive(Debug, Clone)]
pub struct CompiledFunction {
    pub name: String,
    pub is_pub: bool,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: CompiledFnBody,
    pub content_hash: ContentHash,
}

/// A compiled function body: let bindings followed by a result expression.
#[derive(Debug, Clone)]
pub struct CompiledFnBody {
    pub let_bindings: Vec<(String, CompiledExpr)>,
    pub result_expr: CompiledExpr,
}

impl CompiledExpr {
    /// Create a literal expression.
    pub fn literal(value: Value, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[0]).combine(value.content_hash());
        CompiledExpr {
            kind: CompiledExprKind::Literal(value),
            result_type,
            content_hash,
        }
    }

    /// Create a value reference expression.
    pub fn value_ref(id: ValueCellId, result_type: Type) -> Self {
        let content_hash =
            ContentHash::of(&[1]).combine(ContentHash::of_str(&format!("{}", id)));
        CompiledExpr {
            kind: CompiledExprKind::ValueRef(id),
            result_type,
            content_hash,
        }
    }

    /// Create a binary operation expression.
    pub fn binop(op: BinOp, left: CompiledExpr, right: CompiledExpr, result_type: Type) -> Self {
        let content_hash = ContentHash::of(&[2, op as u8])
            .combine(left.content_hash)
            .combine(right.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::BinOp {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
            result_type,
            content_hash,
        }
    }

    /// Recursively walk the expression tree, calling `f` on each node (pre-order).
    ///
    /// This is the canonical traversal for `CompiledExprKind`. All callers
    /// that need to visit expression nodes should use this method rather than
    /// implementing their own match on `CompiledExprKind`. This ensures that
    /// when new variants are added, only this single method needs updating.
    pub fn walk(&self, f: &mut impl FnMut(&CompiledExpr)) {
        f(self);
        match &self.kind {
            CompiledExprKind::Literal(_) => {}
            CompiledExprKind::ValueRef(_) => {}
            CompiledExprKind::BinOp { left, right, .. } => {
                left.walk(f);
                right.walk(f);
            }
            CompiledExprKind::UnOp { operand, .. } => {
                operand.walk(f);
            }
            CompiledExprKind::FunctionCall { args, .. } => {
                for arg in args {
                    arg.walk(f);
                }
            }
            CompiledExprKind::Conditional {
                condition,
                then_branch,
                else_branch,
            } => {
                condition.walk(f);
                then_branch.walk(f);
                else_branch.walk(f);
            }
            CompiledExprKind::Match {
                discriminant,
                arms,
            } => {
                discriminant.walk(f);
                for arm in arms {
                    arm.body.walk(f);
                }
            }
            CompiledExprKind::UserFunctionCall { args, .. } => {
                for arg in args {
                    arg.walk(f);
                }
            }
            CompiledExprKind::Lambda { body, .. } => {
                body.walk(f);
            }
        }
    }

    /// Create a unary operation expression.
    pub fn unop(op: UnOp, operand: CompiledExpr, result_type: Type) -> Self {
        let content_hash =
            ContentHash::of(&[3, op as u8]).combine(operand.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::UnOp {
                op,
                operand: Box::new(operand),
            },
            result_type,
            content_hash,
        }
    }

    /// Collect all ValueRef ValueCellIds from this expression tree.
    ///
    /// For Lambda nodes, emits `captures` only — does NOT recurse into body.
    /// This is the correct behavior for dependency tracking: a lambda's
    /// dependencies are its captures, not the refs inside its body.
    pub fn collect_value_refs(&self) -> Vec<ValueCellId> {
        let mut refs = Vec::new();
        self.collect_value_refs_inner(&mut refs);
        refs
    }

    fn collect_value_refs_inner(&self, refs: &mut Vec<ValueCellId>) {
        match &self.kind {
            CompiledExprKind::ValueRef(id) => refs.push(id.clone()),
            CompiledExprKind::Literal(_) => {}
            CompiledExprKind::BinOp { left, right, .. } => {
                left.collect_value_refs_inner(refs);
                right.collect_value_refs_inner(refs);
            }
            CompiledExprKind::UnOp { operand, .. } => {
                operand.collect_value_refs_inner(refs);
            }
            CompiledExprKind::FunctionCall { args, .. } => {
                for arg in args {
                    arg.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::Conditional { condition, then_branch, else_branch } => {
                condition.collect_value_refs_inner(refs);
                then_branch.collect_value_refs_inner(refs);
                else_branch.collect_value_refs_inner(refs);
            }
            CompiledExprKind::Match { discriminant, arms } => {
                discriminant.collect_value_refs_inner(refs);
                for arm in arms {
                    arm.body.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::UserFunctionCall { args, .. } => {
                for arg in args {
                    arg.collect_value_refs_inner(refs);
                }
            }
            CompiledExprKind::Lambda { captures, .. } => {
                for cap in captures {
                    refs.push(cap.clone());
                }
            }
        }
    }

    /// Create a lambda expression.
    pub fn lambda(
        params: Vec<(String, Option<Type>)>,
        param_ids: Vec<ValueCellId>,
        body: CompiledExpr,
        captures: Vec<ValueCellId>,
        result_type: Type,
    ) -> Self {
        let mut content_hash = ContentHash::of(&[7]).combine(body.content_hash);
        for (name, ty) in &params {
            content_hash = content_hash.combine(ContentHash::of_str(name));
            if let Some(t) = ty {
                content_hash = content_hash.combine(ContentHash::of_str(&format!("{:?}", t)));
            }
        }
        for id in &param_ids {
            content_hash = content_hash.combine(ContentHash::of_str(&format!("{}", id)));
        }
        for cap in &captures {
            content_hash = content_hash.combine(ContentHash::of_str(&format!("{}", cap)));
        }
        CompiledExpr {
            kind: CompiledExprKind::Lambda {
                params,
                param_ids,
                body: Box::new(body),
                captures,
            },
            result_type,
            content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::ContentHash;
    use crate::identity::ValueCellId;

    fn make_conditional(
        condition: CompiledExpr,
        then_branch: CompiledExpr,
        else_branch: CompiledExpr,
        result_type: Type,
    ) -> CompiledExpr {
        let hash = ContentHash::of(&[5])
            .combine(condition.content_hash)
            .combine(then_branch.content_hash)
            .combine(else_branch.content_hash);
        CompiledExpr {
            kind: CompiledExprKind::Conditional {
                condition: Box::new(condition),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            result_type,
            content_hash: hash,
        }
    }

    fn make_function_call(name: &str, args: Vec<CompiledExpr>, result_type: Type) -> CompiledExpr {
        let hash = ContentHash::of(name.as_bytes());
        CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: name.to_string(),
                    qualified_name: format!("std::{}", name),
                },
                args,
            },
            result_type,
            content_hash: hash,
        }
    }

    #[test]
    fn walk_visits_literal() {
        let expr = CompiledExpr::literal(Value::Int(42), Type::Int);
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 1);
    }

    #[test]
    fn walk_collects_value_ref() {
        let id = ValueCellId::new("Part", "x");
        let expr = CompiledExpr::value_ref(id.clone(), Type::length());
        let mut refs = Vec::new();
        expr.walk(&mut |node| {
            if let CompiledExprKind::ValueRef(vid) = &node.kind {
                refs.push(vid.clone());
            }
        });
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0], id);
    }

    #[test]
    fn walk_traverses_binop_children() {
        let a = CompiledExpr::value_ref(ValueCellId::new("P", "a"), Type::length());
        let b = CompiledExpr::value_ref(ValueCellId::new("P", "b"), Type::length());
        let expr = CompiledExpr::binop(BinOp::Gt, a, b, Type::Bool);
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 3);
    }

    #[test]
    fn walk_traverses_function_call_args() {
        let arg1 = CompiledExpr::literal(Value::Int(1), Type::Int);
        let arg2 = CompiledExpr::literal(Value::Int(2), Type::Int);
        let expr = make_function_call("foo", vec![arg1, arg2], Type::Int);
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 3);
    }

    #[test]
    fn walk_traverses_conditional_branches() {
        let cond = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let then_br = CompiledExpr::literal(Value::Int(1), Type::Int);
        let else_br = CompiledExpr::literal(Value::Int(2), Type::Int);
        let expr = make_conditional(cond, then_br, else_br, Type::Int);
        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 4);
    }

    #[test]
    fn walk_traverses_deeply_nested() {
        let a = CompiledExpr::value_ref(ValueCellId::new("P", "a"), Type::length());
        let b = CompiledExpr::value_ref(ValueCellId::new("P", "b"), Type::length());
        let condition = CompiledExpr::binop(BinOp::Gt, a, b, Type::Bool);
        let c = CompiledExpr::value_ref(ValueCellId::new("P", "c"), Type::length());
        let one_mm = CompiledExpr::literal(
            Value::Scalar { si_value: 0.001, dimension: crate::DimensionVector::LENGTH },
            Type::length(),
        );
        let then_br = CompiledExpr::binop(BinOp::Gt, c, one_mm, Type::Bool);
        let d = CompiledExpr::value_ref(ValueCellId::new("P", "d"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar { si_value: 0.002, dimension: crate::DimensionVector::LENGTH },
            Type::length(),
        );
        let else_br = CompiledExpr::binop(BinOp::Gt, d, two_mm, Type::Bool);
        let expr = make_conditional(condition, then_br, else_br, Type::Bool);
        let mut refs = Vec::new();
        expr.walk(&mut |node| {
            if let CompiledExprKind::ValueRef(vid) = &node.kind {
                refs.push(vid.clone());
            }
        });
        assert_eq!(refs.len(), 4);
    }
}
