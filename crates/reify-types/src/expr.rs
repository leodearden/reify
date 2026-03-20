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
