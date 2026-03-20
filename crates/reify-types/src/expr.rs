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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::ContentHash;
    use crate::identity::ValueCellId;

    // Helper to make a simple Conditional expression manually.
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

    // Helper to make a FunctionCall expression.
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
        assert_eq!(count, 1, "walk on Literal should visit exactly 1 node");
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
        let mut refs = Vec::new();
        expr.walk(&mut |node| {
            count += 1;
            if let CompiledExprKind::ValueRef(vid) = &node.kind {
                refs.push(vid.clone());
            }
        });
        assert_eq!(count, 3, "BinOp + 2 children = 3 nodes");
        assert_eq!(refs.len(), 2, "should collect both ValueCellIds");
    }

    #[test]
    fn walk_traverses_function_call_args() {
        let arg1 = CompiledExpr::literal(Value::Int(1), Type::Int);
        let arg2 = CompiledExpr::literal(Value::Int(2), Type::Int);
        let expr = make_function_call("foo", vec![arg1, arg2], Type::Int);

        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 3, "FunctionCall + 2 args = 3 nodes");
    }

    #[test]
    fn walk_traverses_conditional_branches() {
        let cond = CompiledExpr::literal(Value::Bool(true), Type::Bool);
        let then_br = CompiledExpr::literal(Value::Int(1), Type::Int);
        let else_br = CompiledExpr::literal(Value::Int(2), Type::Int);
        let expr = make_conditional(cond, then_br, else_br, Type::Int);

        let mut count = 0;
        expr.walk(&mut |_| count += 1);
        assert_eq!(count, 4, "Conditional + condition + then + else = 4 nodes");
    }

    #[test]
    fn walk_traverses_deeply_nested() {
        // Conditional containing BinOp containing ValueRefs
        let a = CompiledExpr::value_ref(ValueCellId::new("P", "a"), Type::length());
        let b = CompiledExpr::value_ref(ValueCellId::new("P", "b"), Type::length());
        let condition = CompiledExpr::binop(BinOp::Gt, a, b, Type::Bool);

        let c = CompiledExpr::value_ref(ValueCellId::new("P", "c"), Type::length());
        let one_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.001,
                dimension: crate::DimensionVector::LENGTH,
            },
            Type::length(),
        );
        let then_br = CompiledExpr::binop(BinOp::Gt, c, one_mm, Type::Bool);

        let d = CompiledExpr::value_ref(ValueCellId::new("P", "d"), Type::length());
        let two_mm = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.002,
                dimension: crate::DimensionVector::LENGTH,
            },
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
        assert_eq!(refs.len(), 4, "should collect all 4 ValueCellIds from all levels");
        let expected = vec![
            ValueCellId::new("P", "a"),
            ValueCellId::new("P", "b"),
            ValueCellId::new("P", "c"),
            ValueCellId::new("P", "d"),
        ];
        for id in &expected {
            assert!(refs.contains(id), "missing {:?}", id);
        }
    }
}
