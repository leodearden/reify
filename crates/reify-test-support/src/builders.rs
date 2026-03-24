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
        Value::Enum { .. } | Value::List(_) | Value::Set(_) | Value::Map(_) | Value::Option(_) | Value::Lambda { .. } | Value::Field { .. } => {
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
    CompiledTrait, DefaultKind, EntityKind, RealizationDecl, RequirementKind, SubComponentDecl,
    TopologyTemplate, TraitDefault, TraitRequirement, ValueCellDecl, ValueCellKind,
};
use reify_types::{ConstraintNodeId, RealizationNodeId, TypeParam};

// --- Constraint expression helpers ---

/// Create a range constraint: two `CompiledConstraint`s for `member > min_expr` and `member < max_expr`.
///
/// Returns a `Vec` of two constraints with indices 0 and 1. Callers can add them to a
/// `TopologyTemplateBuilder` via `.constraint(entity, idx, None, expr)`.
pub fn range_constraint(
    entity: &str,
    member: &str,
    cell_type: Type,
    min_expr: CompiledExpr,
    max_expr: CompiledExpr,
) -> Vec<CompiledConstraint> {
    let ref_expr = value_ref_typed(entity, member, cell_type);
    let lower = CompiledConstraint {
        id: ConstraintNodeId::new(entity, 0),
        label: None,
        expr: gt(ref_expr.clone(), min_expr),
        span: SourceSpan::new(0, 0),
        domain: None,
    };
    let upper = CompiledConstraint {
        id: ConstraintNodeId::new(entity, 1),
        label: None,
        expr: lt(ref_expr, max_expr),
        span: SourceSpan::new(0, 0),
        domain: None,
    };
    vec![lower, upper]
}

/// Create an equality constraint expression: `member == target_expr`.
///
/// Returns a single `CompiledExpr` with `Type::Bool` result. Add to a
/// `TopologyTemplateBuilder` via `.constraint(entity, idx, None, expr)`.
pub fn equality_constraint(
    entity: &str,
    member: &str,
    cell_type: Type,
    target_expr: CompiledExpr,
) -> CompiledExpr {
    let ref_expr = value_ref_typed(entity, member, cell_type);
    eq(ref_expr, target_expr)
}

/// Builder for `TopologyTemplate`.
pub struct TopologyTemplateBuilder {
    name: String,
    entity_kind: EntityKind,
    visibility: reify_compiler::Visibility,
    type_params: Vec<TypeParam>,
    trait_bounds: Vec<String>,
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
            entity_kind: EntityKind::Structure,
            visibility: reify_compiler::Visibility::Private,
            type_params: Vec::new(),
            trait_bounds: Vec::new(),
            value_cells: Vec::new(),
            constraints: Vec::new(),
            realizations: Vec::new(),
            sub_components: Vec::new(),
            guarded_groups: Vec::new(),
            structure_controlling: HashSet::new(),
            objective: None,
        }
    }

    /// Declare a trait bound this structure conforms to.
    pub fn trait_bound(mut self, name: impl Into<String>) -> Self {
        self.trait_bounds.push(name.into());
        self
    }

    /// Add a type parameter to this structure.
    pub fn type_param(mut self, param: TypeParam) -> Self {
        self.type_params.push(param);
        self
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
            domain: None,
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
            type_args: Vec::new(),
            is_collection: false,
            count_cell: None,
            span: SourceSpan::new(0, 0),
        });
        self
    }

    /// Add a collection sub-component (`sub name : List<T>`) with a count cell.
    pub fn collection_sub_component(
        mut self,
        name: impl Into<String>,
        structure_name: impl Into<String>,
        count_cell: ValueCellId,
    ) -> Self {
        let name = name.into();
        let structure_name = structure_name.into();
        self.sub_components.push(SubComponentDecl {
            content_hash: ContentHash::of_str(&format!("sub {} : List<{}>", name, structure_name)),
            name,
            structure_name,
            visibility: reify_compiler::Visibility::Public,
            args: Vec::new(),
            type_args: Vec::new(),
            is_collection: true,
            count_cell: Some(count_cell),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    /// Add a ValueCellId to the structure_controlling set.
    pub fn structure_controlling_cell(mut self, id: ValueCellId) -> Self {
        self.structure_controlling.insert(id);
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
            entity_kind: self.entity_kind,
            visibility: self.visibility,
            type_params: self.type_params,
            trait_bounds: self.trait_bounds,
            value_cells: self.value_cells,
            constraints: self.constraints,
            realizations: self.realizations,
            sub_components: self.sub_components,
            ports: Vec::new(),
            connections: Vec::new(),
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

    // step-1: failing test for TraitDefBuilder minimal
    #[test]
    fn trait_def_builder_minimal() {
        let ct = TraitDefBuilder::new("Rigid").build();
        assert_eq!(ct.name, "Rigid");
        assert!(!ct.is_pub);
        assert!(ct.required_members.is_empty());
        assert!(ct.defaults.is_empty());
        assert!(ct.refinements.is_empty());
        assert!(ct.type_params.is_empty());
        // content_hash should be non-zero (derived from name)
        assert_ne!(ct.content_hash, reify_types::ContentHash(0));
    }

    // step-3: failing tests for TraitDefBuilder members
    #[test]
    fn trait_def_builder_with_requirement() {
        let ct = TraitDefBuilder::new("Rigid")
            .requirement("mass", RequirementKind::Param(Type::Scalar {
                dimension: DimensionVector::LENGTH, // reuse LENGTH for test simplicity
            }))
            .build();
        assert_eq!(ct.required_members.len(), 1);
        assert_eq!(ct.required_members[0].name, "mass");
        assert!(matches!(&ct.required_members[0].kind, RequirementKind::Param(_)));
    }

    #[test]
    fn trait_def_builder_with_refinement() {
        let ct = TraitDefBuilder::new("StronglyRigid")
            .refinement("Rigid")
            .build();
        assert_eq!(ct.refinements.len(), 1);
        assert_eq!(ct.refinements[0], "Rigid");
    }

    #[test]
    fn trait_def_builder_with_type_param() {
        use reify_types::{TraitBound, TraitRef};
        let param = TypeParam {
            name: "T".to_string(),
            bounds: vec![TraitBound {
                trait_ref: TraitRef {
                    name: "Rigid".to_string(),
                    type_args: vec![],
                },
            }],
            default: None,
        };
        let ct = TraitDefBuilder::new("Container")
            .type_param(param)
            .build();
        assert_eq!(ct.type_params.len(), 1);
        assert_eq!(ct.type_params[0].name, "T");
        assert_eq!(ct.type_params[0].bounds.len(), 1);
        assert_eq!(ct.type_params[0].bounds[0].trait_ref.name, "Rigid");
    }

    #[test]
    fn trait_def_builder_is_pub() {
        let ct = TraitDefBuilder::new("Rigid").is_pub().build();
        assert!(ct.is_pub);
    }

    #[test]
    fn trait_def_builder_content_hash_differs_by_name() {
        let ct1 = TraitDefBuilder::new("Rigid").build();
        let ct2 = TraitDefBuilder::new("Flexible").build();
        assert_ne!(ct1.content_hash, ct2.content_hash);
    }

    // step-5: failing tests for TopologyTemplateBuilder trait extensions
    #[test]
    fn topology_with_trait_bound() {
        let template = TopologyTemplateBuilder::new("Bolt")
            .trait_bound("Rigid")
            .build();
        assert_eq!(template.trait_bounds.len(), 1);
        assert_eq!(template.trait_bounds[0], "Rigid");
    }

    #[test]
    fn topology_with_multiple_trait_bounds() {
        let template = TopologyTemplateBuilder::new("Bolt")
            .trait_bound("Rigid")
            .trait_bound("Fastener")
            .build();
        assert_eq!(template.trait_bounds.len(), 2);
        assert!(template.trait_bounds.contains(&"Rigid".to_string()));
        assert!(template.trait_bounds.contains(&"Fastener".to_string()));
    }

    #[test]
    fn topology_with_type_param() {
        use reify_types::{TraitBound, TraitRef};
        let param = TypeParam {
            name: "T".to_string(),
            bounds: vec![TraitBound {
                trait_ref: TraitRef {
                    name: "Rigid".to_string(),
                    type_args: vec![],
                },
            }],
            default: None,
        };
        let template = TopologyTemplateBuilder::new("Container")
            .type_param(param)
            .build();
        assert_eq!(template.type_params.len(), 1);
        assert_eq!(template.type_params[0].name, "T");
        assert_eq!(template.type_params[0].bounds[0].trait_ref.name, "Rigid");
    }
}

/// Builder for `CompiledTrait`.
///
/// Follows the same fluent pattern as `TopologyTemplateBuilder`.
pub struct TraitDefBuilder {
    name: String,
    is_pub: bool,
    type_params: Vec<TypeParam>,
    refinements: Vec<String>,
    required_members: Vec<TraitRequirement>,
    defaults: Vec<TraitDefault>,
}

impl TraitDefBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_pub: false,
            type_params: Vec::new(),
            refinements: Vec::new(),
            required_members: Vec::new(),
            defaults: Vec::new(),
        }
    }

    pub fn is_pub(mut self) -> Self {
        self.is_pub = true;
        self
    }

    pub fn refinement(mut self, trait_name: impl Into<String>) -> Self {
        self.refinements.push(trait_name.into());
        self
    }

    pub fn type_param(mut self, param: TypeParam) -> Self {
        self.type_params.push(param);
        self
    }

    pub fn requirement(mut self, name: impl Into<String>, kind: RequirementKind) -> Self {
        self.required_members.push(TraitRequirement {
            name: name.into(),
            kind,
            span: reify_types::SourceSpan::new(0, 0),
        });
        self
    }

    pub fn default(mut self, name: Option<impl Into<String>>, kind: DefaultKind) -> Self {
        self.defaults.push(TraitDefault {
            name: name.map(|n| n.into()),
            kind,
            span: reify_types::SourceSpan::new(0, 0),
        });
        self
    }

    pub fn build(self) -> CompiledTrait {
        let content_hash = {
            let name_hash = ContentHash::of_str(&self.name);
            let req_hashes = self
                .required_members
                .iter()
                .map(|r| ContentHash::of_str(&r.name));
            let ref_hashes = self
                .refinements
                .iter()
                .map(|r| ContentHash::of_str(r));
            let all_hashes = std::iter::once(name_hash)
                .chain(req_hashes)
                .chain(ref_hashes);
            ContentHash::combine_all(all_hashes)
        };

        CompiledTrait {
            name: self.name,
            is_pub: self.is_pub,
            type_params: self.type_params,
            refinements: self.refinements,
            required_members: self.required_members,
            defaults: self.defaults,
            content_hash,
        }
    }
}

/// Builder for `CompiledModule`.
pub struct CompiledModuleBuilder {
    path: reify_types::ModulePath,
    imports: Vec<CompiledImport>,
    functions: Vec<reify_types::CompiledFunction>,
    trait_defs: Vec<CompiledTrait>,
    templates: Vec<TopologyTemplate>,
    diagnostics: Vec<reify_types::Diagnostic>,
}

impl CompiledModuleBuilder {
    pub fn new(path: reify_types::ModulePath) -> Self {
        Self {
            path,
            imports: Vec::new(),
            functions: Vec::new(),
            trait_defs: Vec::new(),
            templates: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn trait_def(mut self, t: CompiledTrait) -> Self {
        self.trait_defs.push(t);
        self
    }

    pub fn function(mut self, f: reify_types::CompiledFunction) -> Self {
        self.functions.push(f);
        self
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

            let function_hashes = self.functions.iter().map(|f| f.content_hash);

            let trait_def_hashes = self.trait_defs.iter().map(|t| t.content_hash);

            let all_hashes = std::iter::once(path_hash)
                .chain(template_hashes)
                .chain(import_hashes)
                .chain(function_hashes)
                .chain(trait_def_hashes);

            ContentHash::combine_all(all_hashes)
        };

        CompiledModule {
            path: self.path,
            imports: self.imports,
            enum_defs: Vec::new(),
            functions: self.functions,
            trait_defs: self.trait_defs,
            fields: Vec::new(),
            compiled_purposes: Vec::new(),
            templates: self.templates,
            diagnostics: self.diagnostics,
            content_hash,
        }
    }
}

#[cfg(test)]
mod module_builder_tests {
    use super::*;
    use reify_types::ModulePath;

    // step-7: failing test for CompiledModuleBuilder trait_def method
    #[test]
    fn module_builder_with_trait_def() {
        let ct = TraitDefBuilder::new("Rigid").build();
        let module = CompiledModuleBuilder::new(ModulePath::single("test"))
            .trait_def(ct)
            .build();
        assert_eq!(module.trait_defs.len(), 1);
        assert_eq!(module.trait_defs[0].name, "Rigid");
    }

    // step-26: failing test — content_hash must differ when trait_defs differ
    #[test]
    fn module_builder_trait_defs_affect_content_hash() {
        let path = ModulePath::single("test");
        let module_no_traits = CompiledModuleBuilder::new(path.clone()).build();
        let ct = TraitDefBuilder::new("Rigid").build();
        let module_with_trait = CompiledModuleBuilder::new(path).trait_def(ct).build();
        assert_ne!(
            module_no_traits.content_hash,
            module_with_trait.content_hash,
            "modules differing only in trait_defs must produce distinct content_hashes"
        );
    }

    // step-13: failing tests for constraint expression helpers
    #[test]
    fn range_constraint_returns_two_constraints() {
        let entity = "Beam";
        let constraints = range_constraint(
            entity,
            "width",
            Type::length(),
            literal(Value::Scalar { si_value: 0.01, dimension: DimensionVector::LENGTH }),
            literal(Value::Scalar { si_value: 0.5, dimension: DimensionVector::LENGTH }),
        );
        // range_constraint produces: [width > min, width < max]
        assert_eq!(constraints.len(), 2);
        // Both should be CompiledConstraint with Bool result type
        assert_eq!(constraints[0].expr.result_type, Type::Bool);
        assert_eq!(constraints[1].expr.result_type, Type::Bool);
    }

    #[test]
    fn equality_constraint_returns_one_expr() {
        let entity = "Beam";
        let expr = equality_constraint(
            entity,
            "ratio",
            Type::Real,
            literal(Value::Real(2.0)),
        );
        // equality_constraint: ratio == 2.0 → Bool
        assert_eq!(expr.result_type, Type::Bool);
    }
}
