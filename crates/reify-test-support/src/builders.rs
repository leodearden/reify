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
        Value::Enum { .. } | Value::List(_) | Value::Set(_) | Value::Map(_) | Value::Option(_) | Value::Lambda { .. } => {
            panic!("literal() not yet implemented for M5 type: {:?}. Use CompiledExpr::literal(value, type) directly.", v)
        }
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

use std::collections::HashSet;

use reify_compiler::{
    CompiledConstraint, CompiledGeometryOp, CompiledGuardedGroup, CompiledImport, CompiledModule,
    RealizationDecl, SubComponentDecl, TopologyTemplate, ValueCellDecl, ValueCellKind,
};
use reify_types::{ConstraintNodeId, RealizationNodeId};

/// Builder for `TopologyTemplate`.
pub struct TopologyTemplateBuilder {
    name: String,
    visibility: reify_compiler::Visibility,
    value_cells: Vec<ValueCellDecl>,
    constraints: Vec<CompiledConstraint>,
    realizations: Vec<RealizationDecl>,
    sub_components: Vec<SubComponentDecl>,
    guarded_groups: Vec<CompiledGuardedGroup>,
    structure_controlling: HashSet<ValueCellId>,
    objective: Option<reify_types::OptimizationObjective>,
}

impl TopologyTemplateBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visibility: reify_compiler::Visibility::Private,
            value_cells: Vec::new(),
            constraints: Vec::new(),
            realizations: Vec::new(),
            sub_components: Vec::new(),
            guarded_groups: Vec::new(),
            structure_controlling: HashSet::new(),
            objective: None,
        }
    }

    pub fn visibility(mut self, vis: reify_compiler::Visibility) -> Self {
        self.visibility = vis;
        self
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
            visibility: reify_compiler::Visibility::Public,
            cell_type,
            default_expr: default,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn auto_param(
        mut self,
        entity: &str,
        member: &str,
        cell_type: Type,
    ) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Auto,
            visibility: reify_compiler::Visibility::Public,
            cell_type,
            default_expr: None,
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
            visibility: reify_compiler::Visibility::Private,
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

    pub fn realization(
        mut self,
        entity: &str,
        index: u32,
        operations: Vec<CompiledGeometryOp>,
    ) -> Self {
        self.realizations.push(RealizationDecl {
            id: RealizationNodeId::new(entity, index),
            operations,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn objective(mut self, obj: reify_types::OptimizationObjective) -> Self {
        self.objective = Some(obj);
        self
    }

    pub fn sub_component(
        mut self,
        name: impl Into<String>,
        structure_name: impl Into<String>,
        args: Vec<(String, CompiledExpr)>,
    ) -> Self {
        let name = name.into();
        let structure_name = structure_name.into();
        self.sub_components.push(SubComponentDecl {
            content_hash: ContentHash::of_str(&format!("sub {} = {}", name, structure_name)),
            name,
            structure_name,
            visibility: reify_compiler::Visibility::Public,
            args,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn guarded_group(
        mut self,
        guard_expr: CompiledExpr,
        guard_value_cell: ValueCellId,
        members: Vec<ValueCellDecl>,
        constraints: Vec<CompiledConstraint>,
        else_members: Vec<ValueCellDecl>,
        else_constraints: Vec<CompiledConstraint>,
    ) -> Self {
        self.structure_controlling.insert(guard_value_cell.clone());
        self.guarded_groups.push(CompiledGuardedGroup {
            guard_expr,
            guard_value_cell,
            members,
            constraints,
            else_members,
            else_constraints,
            parent_guard: None,
        });
        self
    }

    pub fn build(self) -> TopologyTemplate {
        // Build a content-sensitive hash matching compile_structure() logic.
        let content_hash = {
            let name_hash = ContentHash::of_str(&self.name);

            let vc_hashes = self.value_cells.iter().map(|vc| {
                vc.default_expr
                    .as_ref()
                    .map(|e| e.content_hash)
                    .unwrap_or(ContentHash(0))
            });

            let constraint_hashes = self.constraints.iter().map(|c| c.expr.content_hash);

            let sub_hashes = self.sub_components.iter().map(|s| s.content_hash);

            let guard_hashes = self.guarded_groups.iter().flat_map(|g| {
                std::iter::once(g.guard_expr.content_hash)
                    .chain(g.members.iter().map(|m| {
                        m.default_expr
                            .as_ref()
                            .map(|e| e.content_hash)
                            .unwrap_or(ContentHash(0))
                    }))
                    .chain(g.constraints.iter().map(|c| c.expr.content_hash))
                    .chain(g.else_members.iter().map(|m| {
                        m.default_expr
                            .as_ref()
                            .map(|e| e.content_hash)
                            .unwrap_or(ContentHash(0))
                    }))
                    .chain(g.else_constraints.iter().map(|c| c.expr.content_hash))
            });

            let all_hashes = std::iter::once(name_hash)
                .chain(vc_hashes)
                .chain(constraint_hashes)
                .chain(sub_hashes)
                .chain(guard_hashes);

            ContentHash::combine_all(all_hashes)
        };

        TopologyTemplate {
            name: self.name,
            visibility: self.visibility,
            value_cells: self.value_cells,
            constraints: self.constraints,
            realizations: self.realizations,
            sub_components: self.sub_components,
            ports: Vec::new(),
            guarded_groups: self.guarded_groups,
            structure_controlling: self.structure_controlling,
            objective: self.objective,
            content_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    #[should_panic(expected = "literal() not yet implemented for M5 type")]
    fn literal_panics_on_enum_value() {
        literal(Value::Enum {
            type_name: "X".into(),
            variant: "Y".into(),
        });
    }

    #[test]
    #[should_panic(expected = "literal() not yet implemented for M5 type")]
    fn literal_panics_on_list_value() {
        literal(Value::List(vec![]));
    }

    #[test]
    #[should_panic(expected = "literal() not yet implemented for M5 type")]
    fn literal_panics_on_set_value() {
        literal(Value::Set(BTreeSet::new()));
    }

    #[test]
    #[should_panic(expected = "literal() not yet implemented for M5 type")]
    fn literal_panics_on_map_value() {
        literal(Value::Map(BTreeMap::new()));
    }

    #[test]
    #[should_panic(expected = "literal() not yet implemented for M5 type")]
    fn literal_panics_on_option_value() {
        literal(Value::Option(None));
    }

    #[test]
    fn auto_param_builder() {
        let template = TopologyTemplateBuilder::new("T")
            .auto_param("T", "x", Type::length())
            .build();

        assert_eq!(template.value_cells.len(), 1);
        let cell = &template.value_cells[0];
        assert_eq!(cell.id, ValueCellId::new("T", "x"));
        assert_eq!(cell.kind, ValueCellKind::Auto);
        assert!(cell.default_expr.is_none());
        assert_eq!(cell.cell_type, Type::length());
    }
}

/// Builder for `CompiledModule`.
pub struct CompiledModuleBuilder {
    path: reify_types::ModulePath,
    imports: Vec<CompiledImport>,
    templates: Vec<TopologyTemplate>,
    diagnostics: Vec<reify_types::Diagnostic>,
}

impl CompiledModuleBuilder {
    pub fn new(path: reify_types::ModulePath) -> Self {
        Self {
            path,
            imports: Vec::new(),
            templates: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn import(mut self, path: impl Into<String>) -> Self {
        self.imports.push(CompiledImport {
            path: path.into(),
            kind: reify_syntax::ImportKind::Module,
            is_pub: false,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn import_with(
        mut self,
        path: impl Into<String>,
        kind: reify_syntax::ImportKind,
        is_pub: bool,
    ) -> Self {
        self.imports.push(CompiledImport {
            path: path.into(),
            kind,
            is_pub,
            span: SourceSpan::new(0, 0),
        });
        self
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
        // Build a content-sensitive hash matching compile() logic.
        let content_hash = {
            let path_hash = ContentHash::of_str(&format!("{}", self.path));

            let template_hashes = self.templates.iter().map(|t| t.content_hash);

            let import_hashes = self.imports.iter().map(|i| ContentHash::of_str(&i.path));

            let all_hashes = std::iter::once(path_hash)
                .chain(template_hashes)
                .chain(import_hashes);

            ContentHash::combine_all(all_hashes)
        };

        CompiledModule {
            path: self.path,
            imports: self.imports,
            enum_defs: Vec::new(),
            functions: Vec::new(),
            trait_defs: Vec::new(),
            templates: self.templates,
            diagnostics: self.diagnostics,
            content_hash,
        }
    }
}
