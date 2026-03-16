use reify_types::{
    BinOp, CompiledExpr, ContentHash, DimensionVector, SourceSpan, Type, UnOp, Value, ValueCellId,
};

// --- Expression builders ---

/// Create a literal expression from a value, inferring the type.
pub fn literal(v: Value) -> CompiledExpr {
    let ty = match &v {
        Value::Bool(_) => Type::Bool,
        Value::Int(_) => Type::Int,
        Value::Real(_) => Type::Real,
        Value::String(_) => Type::String,
        Value::Scalar { dimension, .. } => Type::Scalar {
            dimension: *dimension,
        },
        Value::Undef => Type::Bool, // arbitrary for undef
    };
    CompiledExpr::literal(v, ty)
}

/// Create a value reference expression.
pub fn value_ref(entity: &str, member: &str) -> CompiledExpr {
    // Default to length type; callers can use value_ref_typed for specifics
    CompiledExpr::value_ref(
        ValueCellId::new(entity, member),
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    )
}

/// Create a value reference expression with an explicit type.
pub fn value_ref_typed(entity: &str, member: &str, ty: Type) -> CompiledExpr {
    CompiledExpr::value_ref(ValueCellId::new(entity, member), ty)
}

/// Create a binary operation expression.
pub fn binop(op: BinOp, left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    let result_type = infer_binop_type(op, &left.result_type, &right.result_type);
    CompiledExpr::binop(op, left, right, result_type)
}

/// Create a > comparison.
pub fn gt(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Gt, left, right, Type::Bool)
}

/// Create a < comparison.
pub fn lt(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Lt, left, right, Type::Bool)
}

/// Create a >= comparison.
pub fn ge(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Ge, left, right, Type::Bool)
}

/// Create a <= comparison.
pub fn le(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Le, left, right, Type::Bool)
}

/// Create an == comparison.
pub fn eq(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Eq, left, right, Type::Bool)
}

/// Create a != comparison.
pub fn ne(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Ne, left, right, Type::Bool)
}

/// Create an AND expression.
pub fn and(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::And, left, right, Type::Bool)
}

/// Create an OR expression.
pub fn or(left: CompiledExpr, right: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Or, left, right, Type::Bool)
}

/// Create a NOT expression.
pub fn not(operand: CompiledExpr) -> CompiledExpr {
    CompiledExpr::unop(UnOp::Not, operand, Type::Bool)
}

/// Create a negation expression.
pub fn neg(operand: CompiledExpr) -> CompiledExpr {
    let ty = operand.result_type.clone();
    CompiledExpr::unop(UnOp::Neg, operand, ty)
}

fn infer_binop_type(op: BinOp, left: &Type, right: &Type) -> Type {
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        | BinOp::And | BinOp::Or => Type::Bool,
        BinOp::Add | BinOp::Sub => left.clone(), // same dimension required
        BinOp::Mul => match (left, right) {
            (
                Type::Scalar { dimension: ld },
                Type::Scalar { dimension: rd },
            ) => Type::Scalar {
                dimension: ld.mul(rd),
            },
            (Type::Scalar { .. }, _) | (_, Type::Scalar { .. }) => left.clone(),
            (Type::Real, _) | (_, Type::Real) => Type::Real,
            _ => Type::Int,
        },
        BinOp::Div => match (left, right) {
            (
                Type::Scalar { dimension: ld },
                Type::Scalar { dimension: rd },
            ) => {
                let result = ld.div(rd);
                if result.is_dimensionless() {
                    Type::Real
                } else {
                    Type::Scalar { dimension: result }
                }
            }
            (Type::Scalar { .. }, _) => left.clone(),
            (Type::Real, _) | (_, Type::Real) => Type::Real,
            _ => Type::Int,
        },
        BinOp::Mod => left.clone(),
        BinOp::Pow => left.clone(), // simplified
    }
}

// --- Topology builders ---

use reify_compiler::{
    CompiledConstraint, CompiledModule, RealizationDecl, TopologyTemplate, ValueCellDecl,
    ValueCellKind,
};
use reify_types::ConstraintNodeId;

/// Builder for `TopologyTemplate`.
pub struct TopologyTemplateBuilder {
    name: String,
    value_cells: Vec<ValueCellDecl>,
    constraints: Vec<CompiledConstraint>,
    realizations: Vec<RealizationDecl>,
}

impl TopologyTemplateBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value_cells: Vec::new(),
            constraints: Vec::new(),
            realizations: Vec::new(),
        }
    }

    pub fn param(
        mut self,
        entity: &str,
        member: &str,
        cell_type: Type,
        default: Option<CompiledExpr>,
    ) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Param,
            cell_type,
            default_expr: default,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn let_binding(
        mut self,
        entity: &str,
        member: &str,
        cell_type: Type,
        expr: CompiledExpr,
    ) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Let,
            cell_type,
            default_expr: Some(expr),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn constraint(
        mut self,
        entity: &str,
        index: u32,
        label: Option<&str>,
        expr: CompiledExpr,
    ) -> Self {
        self.constraints.push(CompiledConstraint {
            id: ConstraintNodeId::new(entity, index),
            label: label.map(String::from),
            expr,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn build(self) -> TopologyTemplate {
        let content_hash = ContentHash::of_str(&self.name);
        TopologyTemplate {
            name: self.name,
            value_cells: self.value_cells,
            constraints: self.constraints,
            realizations: self.realizations,
            content_hash,
        }
    }
}

/// Builder for `CompiledModule`.
pub struct CompiledModuleBuilder {
    path: reify_types::ModulePath,
    templates: Vec<TopologyTemplate>,
    diagnostics: Vec<reify_types::Diagnostic>,
}

impl CompiledModuleBuilder {
    pub fn new(path: reify_types::ModulePath) -> Self {
        Self {
            path,
            templates: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn template(mut self, template: TopologyTemplate) -> Self {
        self.templates.push(template);
        self
    }

    pub fn diagnostic(mut self, diag: reify_types::Diagnostic) -> Self {
        self.diagnostics.push(diag);
        self
    }

    pub fn build(self) -> CompiledModule {
        let content_hash = ContentHash::of_str(&format!("{}", self.path));
        CompiledModule {
            path: self.path,
            templates: self.templates,
            diagnostics: self.diagnostics,
            content_hash,
        }
    }
}
