use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, ContentHash, DimensionVector, ResolvedFunction,
    SourceSpan, Type, UnOp, Value, ValueCellId,
};

/// Infer the Type of a Value for use in literal() and collection builders.
fn infer_value_type(v: &Value) -> Type {
    match v {
        Value::Bool(_) => Type::Bool,
        Value::Int(_) => Type::Int,
        Value::Real(_) => Type::Real,
        Value::String(_) => Type::String,
        Value::Scalar { dimension, .. } => Type::Scalar { dimension: *dimension },
        Value::Enum { type_name, .. } => Type::Enum(type_name.clone()),
        Value::List(items) => {
            let elem_ty = items.first().map(infer_value_type).unwrap_or(Type::Int);
            Type::List(Box::new(elem_ty))
        }
        Value::Set(items) => {
            let elem_ty = items.iter().next().map(infer_value_type).unwrap_or(Type::Int);
            Type::Set(Box::new(elem_ty))
        }
        Value::Map(m) => {
            let (k_ty, v_ty) = m
                .iter()
                .next()
                .map(|(k, v)| (infer_value_type(k), infer_value_type(v)))
                .unwrap_or((Type::String, Type::Int));
            Type::Map(Box::new(k_ty), Box::new(v_ty))
        }
        Value::Option(Some(inner)) => Type::Option(Box::new(infer_value_type(inner))),
        Value::Option(None) => Type::Option(Box::new(Type::Bool)),
        Value::Lambda { params, body, .. } => {
            let param_types = params.iter().map(|_| Type::Real).collect();
            Type::Function {
                params: param_types,
                return_type: Box::new(body.result_type.clone()),
            }
        }
        Value::Field { domain_type, codomain_type, .. } => Type::Field {
            domain: Box::new(domain_type.clone()),
            codomain: Box::new(codomain_type.clone()),
        },
        Value::Tensor(_) => {
            panic!("literal() cannot infer Tensor type (rank/n/quantity). Use CompiledExpr::literal(value, type) directly.")
        }
        Value::Point(_) | Value::Vector(_) => {
            panic!("literal() cannot infer Point/Vector type (n/quantity). Use CompiledExpr::literal(value, type) directly.")
        }
        Value::Matrix(_) => {
            panic!("literal() cannot infer Matrix type (m/n/quantity). Use CompiledExpr::literal(value, type) directly.")
        }
        Value::Complex { dimension, .. } => Type::complex(Type::Scalar { dimension: *dimension }),
        Value::Orientation { .. } => Type::Orientation(3),
        Value::Frame { .. } => Type::Frame(3),
        Value::Transform { .. } => Type::Transform(3),
        Value::Range { lower, upper, .. } => {
            let elem_ty = lower
                .as_ref()
                .map(|v| infer_value_type(v))
                .or_else(|| upper.as_ref().map(|v| infer_value_type(v)))
                .unwrap_or_else(|| panic!("literal() cannot infer Range element type for fully unbounded range. Use CompiledExpr::literal(value, type) directly."));
            Type::Range(Box::new(elem_ty))
        }
        Value::Undef => Type::Bool,
    }
}

// --- Expression builders ---

/// Create a literal expression from a value, inferring the type.
///
/// Supports all Value variants including M5 types (Enum, List, Set, Map, Option,
/// Lambda, Field). For empty collections, element type defaults to Int/Bool.
pub fn literal(v: Value) -> CompiledExpr {
    let ty = infer_value_type(&v);
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

/// Create a list literal expression, inferring element type from the first element.
///
/// Panics if `elements` is empty — use `CompiledExpr::list_literal` directly for empty lists.
pub fn list_expr(elements: Vec<CompiledExpr>) -> CompiledExpr {
    assert!(!elements.is_empty(), "list_expr: use CompiledExpr::list_literal for empty lists");
    let elem_ty = elements[0].result_type.clone();
    let result_type = Type::List(Box::new(elem_ty));
    CompiledExpr::list_literal(elements, result_type)
}

/// Create a set literal expression, inferring element type from the first element.
///
/// Panics if `elements` is empty — use `CompiledExpr::set_literal` directly for empty sets.
pub fn set_expr(elements: Vec<CompiledExpr>) -> CompiledExpr {
    assert!(!elements.is_empty(), "set_expr: use CompiledExpr::set_literal for empty sets");
    let elem_ty = elements[0].result_type.clone();
    let result_type = Type::Set(Box::new(elem_ty));
    CompiledExpr::set_literal(elements, result_type)
}

/// Create a map literal expression, inferring key/value types from the first entry.
///
/// Panics if `entries` is empty — use `CompiledExpr::map_literal` directly for empty maps.
pub fn map_expr(entries: Vec<(CompiledExpr, CompiledExpr)>) -> CompiledExpr {
    assert!(!entries.is_empty(), "map_expr: use CompiledExpr::map_literal for empty maps");
    let key_ty = entries[0].0.result_type.clone();
    let val_ty = entries[0].1.result_type.clone();
    let result_type = Type::Map(Box::new(key_ty), Box::new(val_ty));
    CompiledExpr::map_literal(entries, result_type)
}

/// Create an `option_some` expression wrapping `inner`, inferring `Type::Option(inner.result_type)`.
pub fn option_some_expr(inner: CompiledExpr) -> CompiledExpr {
    let result_type = Type::Option(Box::new(inner.result_type.clone()));
    CompiledExpr::option_some(inner, result_type)
}

/// Create an `option_none` expression for the given inner type, producing `Type::Option(inner_type)`.
pub fn option_none_expr(inner_type: Type) -> CompiledExpr {
    let result_type = Type::Option(Box::new(inner_type));
    CompiledExpr::option_none(result_type)
}

/// Create a conditional expression. Result type is taken from `then_branch`.
pub fn conditional_expr(
    condition: CompiledExpr,
    then_branch: CompiledExpr,
    else_branch: CompiledExpr,
) -> CompiledExpr {
    let result_type = then_branch.result_type.clone();
    let content_hash = ContentHash::of(&[4])
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
        content_hash,
    }
}

/// Create a standard function call expression with a fully-qualified function name.
pub fn fn_call(
    name: &str,
    qualified_name: &str,
    args: Vec<CompiledExpr>,
    result_type: Type,
) -> CompiledExpr {
    let mut content_hash = ContentHash::of(&[5]).combine(ContentHash::of_str(name));
    for arg in &args {
        content_hash = content_hash.combine(arg.content_hash);
    }
    CompiledExpr {
        kind: CompiledExprKind::FunctionCall {
            function: ResolvedFunction {
                name: name.to_string(),
                qualified_name: qualified_name.to_string(),
            },
            args,
        },
        result_type,
        content_hash,
    }
}

/// Create a user-defined function call expression.
pub fn user_fn_call(
    function_name: &str,
    args: Vec<CompiledExpr>,
    result_type: Type,
) -> CompiledExpr {
    let mut content_hash = ContentHash::of(&[6]).combine(ContentHash::of_str(function_name));
    for arg in &args {
        content_hash = content_hash.combine(arg.content_hash);
    }
    CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: function_name.to_string(),
            args,
        },
        result_type,
        content_hash,
    }
}

/// Create a method call expression.
pub fn method_call_expr(
    object: CompiledExpr,
    method: &str,
    args: Vec<CompiledExpr>,
    result_type: Type,
) -> CompiledExpr {
    CompiledExpr::method_call(object, method.to_string(), args, result_type)
}

/// Create a field `sample` call: `std::field::sample(field, point) -> result_type`.
pub fn sample_call(field: CompiledExpr, point: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call("sample", "std::field::sample", vec![field, point], result_type)
}

/// Create a field `gradient` call: `std::field::gradient(field) -> result_type`.
pub fn gradient_call(field: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call("gradient", "std::field::gradient", vec![field], result_type)
}

/// Create a field `divergence` call: `std::field::divergence(field) -> result_type`.
pub fn divergence_call(field: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call("divergence", "std::field::divergence", vec![field], result_type)
}

/// Create a field `curl` call: `std::field::curl(field) -> result_type`.
pub fn curl_call(field: CompiledExpr, result_type: Type) -> CompiledExpr {
    fn_call("curl", "std::field::curl", vec![field], result_type)
}

/// Create a lambda expression with named parameters.
///
/// Generates param IDs with `ValueCellId::new("__lambda", name)` for each parameter.
pub fn lambda_expr(params: Vec<(&str, Type)>, body: CompiledExpr) -> CompiledExpr {
    let param_types: Vec<Type> = params.iter().map(|(_, ty)| ty.clone()).collect();
    let return_type = body.result_type.clone();
    let result_type = Type::Function {
        params: param_types,
        return_type: Box::new(return_type),
    };
    let param_ids: Vec<ValueCellId> = params
        .iter()
        .map(|(name, _)| ValueCellId::new("__lambda", *name))
        .collect();
    let compiled_params: Vec<(String, Option<Type>)> = params
        .into_iter()
        .map(|(name, ty)| (name.to_string(), Some(ty)))
        .collect();
    CompiledExpr::lambda(compiled_params, param_ids, body, vec![], result_type)
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

// --- Topology builders ---

use std::collections::HashSet;

use reify_compiler::{
    CompiledConstraint, CompiledField, CompiledFieldSource, CompiledGeometryOp,
    CompiledGuardedGroup, CompiledImport, CompiledModule, CompiledPurpose, CompiledPurposeParam,
    CompiledTrait, DefaultKind, EntityKind, RealizationDecl, RequirementKind,
    ResolvedSchemaQuery, SubComponentDecl, TopologyTemplate, TraitDefault, TraitRequirement,
    ValueCellDecl, ValueCellKind,
};
use reify_types::{ConstraintNodeId, RealizationNodeId, TypeParam};

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

    /// Spec-aligned alias for `auto_param`. Creates a ValueCellKind::Auto cell (a "free" parameter
    /// with no default, determined by the solver).
    pub fn free_param(self, entity: &str, member: &str, cell_type: Type) -> Self {
        self.auto_param(entity, member, cell_type)
    }

    /// Create a ValueCellKind::Param cell with no default expression.
    pub fn param_no_default(mut self, entity: &str, member: &str, cell_type: Type) -> Self {
        self.value_cells.push(ValueCellDecl {
            id: ValueCellId::new(entity, member),
            kind: ValueCellKind::Param,
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
            is_recursive: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::CompiledExprKind;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn literal_enum_produces_enum_type() {
        let expr = literal(Value::Enum {
            type_name: "Color".into(),
            variant: "Red".into(),
        });
        assert_eq!(expr.result_type, Type::Enum("Color".to_string()));
        assert!(matches!(expr.kind, CompiledExprKind::Literal(Value::Enum { .. })));
    }

    #[test]
    fn literal_list_produces_list_type() {
        let expr = literal(Value::List(vec![Value::Int(1), Value::Int(2)]));
        assert_eq!(expr.result_type, Type::List(Box::new(Type::Int)));
        assert!(matches!(expr.kind, CompiledExprKind::Literal(Value::List(_))));
    }

    #[test]
    fn literal_set_produces_set_type() {
        let mut s = BTreeSet::new();
        s.insert(Value::Int(1));
        let expr = literal(Value::Set(s));
        assert_eq!(expr.result_type, Type::Set(Box::new(Type::Int)));
    }

    #[test]
    fn literal_map_produces_map_type() {
        let mut m = BTreeMap::new();
        m.insert(Value::String("k".into()), Value::Int(1));
        let expr = literal(Value::Map(m));
        assert_eq!(
            expr.result_type,
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
    }

    #[test]
    fn literal_option_some_produces_option_type() {
        let expr = literal(Value::Option(Some(Box::new(Value::Int(1)))));
        assert_eq!(expr.result_type, Type::Option(Box::new(Type::Int)));
    }

    #[test]
    fn literal_option_none_produces_option_bool_fallback() {
        let expr = literal(Value::Option(None));
        assert_eq!(expr.result_type, Type::Option(Box::new(Type::Bool)));
    }

    #[test]
    fn literal_empty_list_uses_int_fallback() {
        let expr = literal(Value::List(vec![]));
        assert_eq!(expr.result_type, Type::List(Box::new(Type::Int)));
    }

    #[test]
    #[should_panic(expected = "literal() cannot infer Range element type")]
    fn literal_fully_unbounded_range_panics() {
        let _ = literal(Value::Range {
            lower: None,
            upper: None,
            lower_inclusive: false,
            upper_inclusive: false,
        });
    }

    #[test]
    fn literal_range_with_lower_bound_infers_type() {
        let expr = literal(Value::Range {
            lower: Some(Box::new(Value::Int(1))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        });
        assert_eq!(expr.result_type, Type::Range(Box::new(Type::Int)));
    }

    #[test]
    fn literal_range_with_upper_bound_only_infers_type() {
        let expr = literal(Value::Range {
            lower: None,
            upper: Some(Box::new(Value::Real(5.0))),
            lower_inclusive: false,
            upper_inclusive: true,
        });
        assert_eq!(expr.result_type, Type::Range(Box::new(Type::Real)));
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

    // --- Collection expression builder tests (step-5) ---

    #[test]
    fn list_expr_produces_list_literal_with_correct_type() {
        let e1 = literal(Value::Int(1));
        let e2 = literal(Value::Int(2));
        let expr = list_expr(vec![e1, e2]);
        assert_eq!(expr.result_type, Type::List(Box::new(Type::Int)));
        assert!(matches!(expr.kind, CompiledExprKind::ListLiteral(_)));
    }

    #[test]
    fn set_expr_produces_set_literal_with_correct_type() {
        let e1 = literal(Value::Int(1));
        let expr = set_expr(vec![e1]);
        assert_eq!(expr.result_type, Type::Set(Box::new(Type::Int)));
        assert!(matches!(expr.kind, CompiledExprKind::SetLiteral(_)));
    }

    #[test]
    fn map_expr_produces_map_literal_with_correct_type() {
        let k = literal(Value::String("key".into()));
        let v = literal(Value::Int(99));
        let expr = map_expr(vec![(k, v)]);
        assert_eq!(
            expr.result_type,
            Type::Map(Box::new(Type::String), Box::new(Type::Int))
        );
        assert!(matches!(expr.kind, CompiledExprKind::MapLiteral(_)));
    }

    // --- conditional_expr, fn_call, user_fn_call tests (step-7) ---

    #[test]
    fn conditional_expr_uses_then_branch_type() {
        let cond = literal(Value::Bool(true));
        let then_b = literal(Value::Int(1));
        let else_b = literal(Value::Int(2));
        let expr = conditional_expr(cond, then_b, else_b);
        assert_eq!(expr.result_type, Type::Int);
        assert!(matches!(expr.kind, CompiledExprKind::Conditional { .. }));
    }

    #[test]
    fn fn_call_produces_function_call_with_resolved_function() {
        let arg = literal(Value::Real(1.0));
        let expr = fn_call("sin", "std::math::sin", vec![arg], Type::Real);
        assert_eq!(expr.result_type, Type::Real);
        if let CompiledExprKind::FunctionCall { function, args } = &expr.kind {
            assert_eq!(function.name, "sin");
            assert_eq!(function.qualified_name, "std::math::sin");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected FunctionCall kind");
        }
    }

    #[test]
    fn user_fn_call_produces_user_function_call() {
        let arg = literal(Value::Int(1));
        let expr = user_fn_call("my_func", vec![arg], Type::Int);
        assert_eq!(expr.result_type, Type::Int);
        if let CompiledExprKind::UserFunctionCall { function_name, args } = &expr.kind {
            assert_eq!(function_name, "my_func");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected UserFunctionCall kind");
        }
    }

    // --- method_call_expr and lambda_expr tests (step-9) ---

    #[test]
    fn method_call_expr_produces_method_call_kind() {
        let obj = list_expr(vec![literal(Value::Int(1))]);
        let expr = method_call_expr(obj, "count", vec![], Type::Int);
        assert_eq!(expr.result_type, Type::Int);
        if let CompiledExprKind::MethodCall { method, args, .. } = &expr.kind {
            assert_eq!(method, "count");
            assert!(args.is_empty());
        } else {
            panic!("expected MethodCall kind");
        }
    }

    #[test]
    fn lambda_expr_produces_lambda_with_function_type() {
        let body = literal(Value::Real(1.0));
        let expr = lambda_expr(vec![("x", Type::Real)], body);
        assert_eq!(
            expr.result_type,
            Type::Function {
                params: vec![Type::Real],
                return_type: Box::new(Type::Real),
            }
        );
        if let CompiledExprKind::Lambda { params, param_ids, .. } = &expr.kind {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].0, "x");
            assert_eq!(param_ids.len(), 1);
            assert_eq!(param_ids[0], ValueCellId::new("__lambda", "x"));
        } else {
            panic!("expected Lambda kind");
        }
    }

    // --- Field operation expression helpers tests (step-11) ---

    #[test]
    fn sample_call_produces_function_call_with_std_field_sample() {
        let field_e = literal(Value::Real(0.0)); // dummy field expr
        let point_e = literal(Value::Real(1.0));
        let expr = sample_call(field_e, point_e, Type::Real);
        if let CompiledExprKind::FunctionCall { function, args } = &expr.kind {
            assert_eq!(function.qualified_name, "std::field::sample");
            assert_eq!(args.len(), 2);
        } else {
            panic!("expected FunctionCall kind for sample_call");
        }
        assert_eq!(expr.result_type, Type::Real);
    }

    #[test]
    fn gradient_call_produces_function_call_with_std_field_gradient() {
        let field_e = literal(Value::Real(0.0));
        let expr = gradient_call(field_e, Type::Real);
        if let CompiledExprKind::FunctionCall { function, args } = &expr.kind {
            assert_eq!(function.qualified_name, "std::field::gradient");
            assert_eq!(args.len(), 1);
        } else {
            panic!("expected FunctionCall kind for gradient_call");
        }
    }

    #[test]
    fn divergence_call_produces_function_call_with_std_field_divergence() {
        let field_e = literal(Value::Real(0.0));
        let expr = divergence_call(field_e, Type::Real);
        if let CompiledExprKind::FunctionCall { function, .. } = &expr.kind {
            assert_eq!(function.qualified_name, "std::field::divergence");
        } else {
            panic!("expected FunctionCall kind for divergence_call");
        }
    }

    #[test]
    fn curl_call_produces_function_call_with_std_field_curl() {
        let field_e = literal(Value::Real(0.0));
        let expr = curl_call(field_e, Type::Real);
        if let CompiledExprKind::FunctionCall { function, .. } = &expr.kind {
            assert_eq!(function.qualified_name, "std::field::curl");
        } else {
            panic!("expected FunctionCall kind for curl_call");
        }
    }

    // --- CompiledFieldBuilder tests (step-13) ---

    #[test]
    fn compiled_field_builder_analytical_produces_field() {
        use reify_compiler::CompiledFieldSource;
        let body = literal(Value::Real(1.0));
        let field = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .analytical(body)
            .build();
        assert_eq!(field.name, "temp");
        assert!(!field.is_pub);
        assert_eq!(field.domain_type, Type::Geometry);
        assert_eq!(field.codomain_type, Type::Real);
        assert!(matches!(field.source, CompiledFieldSource::Analytical { .. }));
        assert_ne!(field.content_hash, ContentHash(0));
    }

    #[test]
    fn compiled_field_builder_public_sampled() {
        use reify_compiler::CompiledFieldSource;
        let field = CompiledFieldBuilder::new("vel", Type::Geometry, Type::Real)
            .public()
            .sampled(vec![("resolution", literal(Value::Int(32)))])
            .build();
        assert!(field.is_pub);
        assert!(matches!(field.source, CompiledFieldSource::Sampled { .. }));
        assert_ne!(field.content_hash, ContentHash(0));
    }

    #[test]
    fn compiled_field_builder_composed() {
        use reify_compiler::CompiledFieldSource;
        let body = literal(Value::Real(0.0));
        let field = CompiledFieldBuilder::new("composed_f", Type::Geometry, Type::Real)
            .composed(body)
            .build();
        assert!(matches!(field.source, CompiledFieldSource::Composed { .. }));
    }

    #[test]
    fn compiled_field_builder_imported() {
        use reify_compiler::CompiledFieldSource;
        let field = CompiledFieldBuilder::new("ext", Type::Geometry, Type::Real)
            .imported()
            .build();
        assert!(matches!(field.source, CompiledFieldSource::Imported));
        assert_ne!(field.content_hash, ContentHash(0));
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

    pub fn add_default(mut self, name: Option<impl Into<String>>, kind: DefaultKind) -> Self {
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
                .map(|r| ContentHash::of_str(&format!("{}:{:?}", r.name, std::mem::discriminant(&r.kind))));
            let ref_hashes = self
                .refinements
                .iter()
                .map(|r| ContentHash::of_str(r));
            let type_param_hashes = self
                .type_params
                .iter()
                .map(|p| ContentHash::of_str(&p.name));
            let default_hashes = self
                .defaults
                .iter()
                .map(|d| ContentHash::of_str(d.name.as_deref().unwrap_or("")));
            let all_hashes = std::iter::once(name_hash)
                .chain(req_hashes)
                .chain(ref_hashes)
                .chain(type_param_hashes)
                .chain(default_hashes);
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

// --- Tests for TraitDefBuilder ---

#[cfg(test)]
mod trait_def_builder_tests {
    use super::*;
    use reify_compiler::{DefaultKind, RequirementKind};
    use reify_types::{ContentHash, DimensionVector, SourceSpan, TraitBound, TraitRef, TypeParam};

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

    #[test]
    fn trait_def_builder_with_default() {
        let ct = TraitDefBuilder::new("Rigid")
            .add_default(
                Some("mass_positive"),
                DefaultKind::Constraint(reify_syntax::ConstraintDecl {
                    label: Some("mass_positive".to_string()),
                    expr: reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::BoolLiteral(true),
                        span: SourceSpan::new(0, 0),
                    },
                    where_clause: None,
                    span: SourceSpan::new(0, 0),
                    content_hash: ContentHash::of_str("true"),
                }),
            )
            .build();
        assert_eq!(ct.defaults.len(), 1);
        assert_eq!(ct.defaults[0].name.as_deref(), Some("mass_positive"));
    }

    #[test]
    fn trait_def_content_hash_differs_by_type_param() {
        let ct1 = TraitDefBuilder::new("Container").build();
        let ct2 = TraitDefBuilder::new("Container")
            .type_param(TypeParam {
                name: "T".to_string(),
                bounds: vec![TraitBound {
                    trait_ref: TraitRef {
                        name: "Rigid".to_string(),
                        type_args: vec![],
                    },
                }],
                default: None,
            })
            .build();
        assert_ne!(
            ct1.content_hash, ct2.content_hash,
            "traits differing only in type_params must produce distinct content_hashes"
        );
    }

    #[test]
    fn trait_def_content_hash_differs_by_default() {
        let ct1 = TraitDefBuilder::new("Rigid").build();
        let ct2 = TraitDefBuilder::new("Rigid")
            .add_default(
                Some("mass_positive"),
                DefaultKind::Constraint(reify_syntax::ConstraintDecl {
                    label: Some("mass_positive".to_string()),
                    expr: reify_syntax::Expr {
                        kind: reify_syntax::ExprKind::BoolLiteral(true),
                        span: SourceSpan::new(0, 0),
                    },
                    where_clause: None,
                    span: SourceSpan::new(0, 0),
                    content_hash: ContentHash::of_str("true"),
                }),
            )
            .build();
        assert_ne!(
            ct1.content_hash, ct2.content_hash,
            "traits differing only in defaults must produce distinct content_hashes"
        );
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
    fields: Vec<CompiledField>,
    enum_defs: Vec<reify_types::EnumDef>,
    compiled_purposes: Vec<CompiledPurpose>,
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
            fields: Vec::new(),
            enum_defs: Vec::new(),
            compiled_purposes: Vec::new(),
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

    pub fn field(mut self, f: CompiledField) -> Self {
        self.fields.push(f);
        self
    }

    pub fn enum_def(mut self, e: reify_types::EnumDef) -> Self {
        self.enum_defs.push(e);
        self
    }

    pub fn compiled_purpose(mut self, p: CompiledPurpose) -> Self {
        self.compiled_purposes.push(p);
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

            let field_hashes = self.fields.iter().map(|f| f.content_hash);

            let purpose_hashes = self.compiled_purposes.iter().map(|p| p.content_hash);

            let enum_hashes =
                self.enum_defs.iter().map(|e| ContentHash::of_str(&e.name));

            let all_hashes = std::iter::once(path_hash)
                .chain(template_hashes)
                .chain(import_hashes)
                .chain(function_hashes)
                .chain(trait_def_hashes)
                .chain(field_hashes)
                .chain(purpose_hashes)
                .chain(enum_hashes);

            ContentHash::combine_all(all_hashes)
        };

        CompiledModule {
            path: self.path,
            imports: self.imports,
            enum_defs: self.enum_defs,
            functions: self.functions,
            trait_defs: self.trait_defs,
            fields: self.fields,
            compiled_purposes: self.compiled_purposes,
            templates: self.templates,
            diagnostics: self.diagnostics,
            content_hash,
        }
    }
}

// --- CompiledFieldBuilder ---

/// Builder for `CompiledField`.
pub struct CompiledFieldBuilder {
    name: String,
    is_pub: bool,
    domain_type: Type,
    codomain_type: Type,
    source: Option<CompiledFieldSource>,
}

impl CompiledFieldBuilder {
    pub fn new(name: impl Into<String>, domain_type: Type, codomain_type: Type) -> Self {
        Self {
            name: name.into(),
            is_pub: false,
            domain_type,
            codomain_type,
            source: None,
        }
    }

    pub fn public(mut self) -> Self {
        self.is_pub = true;
        self
    }

    /// Set source to `Analytical { expr }`.
    pub fn analytical(mut self, expr: CompiledExpr) -> Self {
        self.source = Some(CompiledFieldSource::Analytical { expr });
        self
    }

    /// Set source to `Sampled { config }`.
    pub fn sampled(mut self, config: Vec<(&str, CompiledExpr)>) -> Self {
        let config = config.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        self.source = Some(CompiledFieldSource::Sampled { config });
        self
    }

    /// Set source to `Composed { expr }`.
    pub fn composed(mut self, expr: CompiledExpr) -> Self {
        self.source = Some(CompiledFieldSource::Composed { expr });
        self
    }

    /// Set source to `Imported`.
    pub fn imported(mut self) -> Self {
        self.source = Some(CompiledFieldSource::Imported);
        self
    }

    pub fn build(self) -> CompiledField {
        let source = self.source.expect("CompiledFieldBuilder: source must be set before build()");
        let content_hash = ContentHash::of_str(&self.name)
            .combine(ContentHash::of(&[99])); // distinguish from zero
        CompiledField {
            name: self.name,
            is_pub: self.is_pub,
            domain_type: self.domain_type,
            codomain_type: self.codomain_type,
            source,
            content_hash,
        }
    }
}

// --- CompiledPurposeBuilder (step-16) ---

/// Builder for `CompiledPurpose`.
pub struct CompiledPurposeBuilder {
    name: String,
    is_pub: bool,
    params: Vec<CompiledPurposeParam>,
    constraints: Vec<CompiledConstraint>,
    objective: Option<reify_types::OptimizationObjective>,
    resolved_queries: Vec<ResolvedSchemaQuery>,
}

impl CompiledPurposeBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_pub: false,
            params: Vec::new(),
            constraints: Vec::new(),
            objective: None,
            resolved_queries: Vec::new(),
        }
    }

    pub fn public(mut self) -> Self {
        self.is_pub = true;
        self
    }

    pub fn param(mut self, name: impl Into<String>, entity_kind: impl Into<String>) -> Self {
        self.params.push(CompiledPurposeParam {
            name: name.into(),
            entity_kind: entity_kind.into(),
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

    pub fn objective(mut self, obj: reify_types::OptimizationObjective) -> Self {
        self.objective = Some(obj);
        self
    }

    pub fn schema_query(
        mut self,
        param_name: impl Into<String>,
        query_kind: impl Into<String>,
        resolved_ids: Vec<ValueCellId>,
    ) -> Self {
        self.resolved_queries.push(ResolvedSchemaQuery {
            param_name: param_name.into(),
            query_kind: query_kind.into(),
            resolved_ids,
        });
        self
    }

    pub fn build(self) -> CompiledPurpose {
        let name_hash = ContentHash::of_str(&self.name);
        let constraint_hashes = self.constraints.iter().map(|c| c.expr.content_hash);
        let query_hashes = self
            .resolved_queries
            .iter()
            .map(|q| ContentHash::of_str(&format!("{}.{}", q.param_name, q.query_kind)));
        let content_hash = std::iter::once(name_hash)
            .chain(constraint_hashes)
            .chain(query_hashes)
            .fold(ContentHash::of(&[0x50]), |acc, h| acc.combine(h));

        CompiledPurpose {
            name: self.name,
            is_pub: self.is_pub,
            params: self.params,
            constraints: self.constraints,
            objective: self.objective,
            resolved_queries: self.resolved_queries,
            content_hash,
        }
    }
}

// --- CompiledTraitBuilder (step-18) ---

/// Builder for `CompiledTrait`.
pub struct CompiledTraitBuilder {
    name: String,
    is_pub: bool,
    type_params: Vec<reify_types::TypeParam>,
    refinements: Vec<String>,
    required_members: Vec<TraitRequirement>,
    defaults: Vec<reify_compiler::TraitDefault>,
}

impl CompiledTraitBuilder {
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

    pub fn public(mut self) -> Self {
        self.is_pub = true;
        self
    }

    pub fn refinement(mut self, name: impl Into<String>) -> Self {
        self.refinements.push(name.into());
        self
    }

    pub fn require_param(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.required_members.push(TraitRequirement {
            name: name.into(),
            kind: RequirementKind::Param(ty),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn require_let(mut self, name: impl Into<String>, ty: Type) -> Self {
        self.required_members.push(TraitRequirement {
            name: name.into(),
            kind: RequirementKind::Let(ty),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn require_sub(mut self, name: impl Into<String>, structure: impl Into<String>) -> Self {
        self.required_members.push(TraitRequirement {
            name: name.into(),
            kind: RequirementKind::Sub(structure.into()),
            span: SourceSpan::new(0, 0),
        });
        self
    }

    pub fn build(self) -> CompiledTrait {
        let name_hash = ContentHash::of_str(&self.name);
        let member_hashes = self.required_members.iter().map(|m| ContentHash::of_str(&m.name));
        let content_hash = std::iter::once(name_hash)
            .chain(member_hashes)
            .fold(ContentHash::of(&[0x54]), |acc, h| acc.combine(h));

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

// --- Tests for CompiledPurposeBuilder (step-15) ---

#[cfg(test)]
mod purpose_builder_tests {
    use super::*;
    use reify_types::OptimizationObjective;

    #[test]
    fn purpose_builder_basic_param_and_constraint() {
        use reify_compiler::CompiledPurpose;
        let constraint_expr = literal(Value::Bool(true));
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("mfg_ready")
            .param("subject", "Structure")
            .constraint("subject", 0, Some("thick_enough"), constraint_expr)
            .build();
        assert_eq!(purpose.name, "mfg_ready");
        assert!(!purpose.is_pub);
        assert_eq!(purpose.params.len(), 1);
        assert_eq!(purpose.params[0].name, "subject");
        assert_eq!(purpose.params[0].entity_kind, "Structure");
        assert_eq!(purpose.constraints.len(), 1);
        assert_eq!(purpose.constraints[0].label.as_deref(), Some("thick_enough"));
        assert_ne!(purpose.content_hash, ContentHash(0));
    }

    #[test]
    fn purpose_builder_public() {
        use reify_compiler::CompiledPurpose;
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("opt_ready")
            .public()
            .build();
        assert!(purpose.is_pub);
    }

    #[test]
    fn purpose_builder_with_objective() {
        use reify_compiler::CompiledPurpose;
        let obj_expr = literal(Value::Real(1.0));
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("minimize_mass")
            .param("subject", "Structure")
            .objective(OptimizationObjective::Minimize(obj_expr))
            .build();
        assert!(purpose.objective.is_some());
        assert_eq!(purpose.resolved_queries.len(), 0);
        assert_ne!(purpose.content_hash, ContentHash(0));
    }

    #[test]
    fn purpose_builder_with_schema_query() {
        use reify_compiler::CompiledPurpose;
        let vcid = ValueCellId::new("subject", "thickness");
        let purpose: CompiledPurpose = CompiledPurposeBuilder::new("mfg_ready")
            .param("subject", "Structure")
            .schema_query("subject", "params", vec![vcid.clone()])
            .build();
        assert_eq!(purpose.resolved_queries.len(), 1);
        assert_eq!(purpose.resolved_queries[0].param_name, "subject");
        assert_eq!(purpose.resolved_queries[0].query_kind, "params");
        assert_eq!(purpose.resolved_queries[0].resolved_ids.len(), 1);
        assert_eq!(purpose.resolved_queries[0].resolved_ids[0], vcid);
    }
}

// --- Tests for CompiledTraitBuilder (step-17) ---

#[cfg(test)]
mod trait_builder_tests {
    use super::*;
    use reify_compiler::{CompiledTrait, RequirementKind};

    #[test]
    fn trait_builder_require_param_produces_required_member() {
        let t: CompiledTrait = CompiledTraitBuilder::new("Rigid")
            .require_param("thickness", Type::length())
            .build();
        assert_eq!(t.name, "Rigid");
        assert!(!t.is_pub);
        assert_eq!(t.required_members.len(), 1);
        assert_eq!(t.required_members[0].name, "thickness");
        if let RequirementKind::Param(ty) = &t.required_members[0].kind {
            assert_eq!(*ty, Type::length());
        } else {
            panic!("expected RequirementKind::Param");
        }
        assert_ne!(t.content_hash, ContentHash(0));
    }

    #[test]
    fn trait_builder_public() {
        let t: CompiledTrait = CompiledTraitBuilder::new("Rigid")
            .public()
            .build();
        assert!(t.is_pub);
    }

    #[test]
    fn trait_builder_refinement_and_multiple_requirements() {
        let t: CompiledTrait = CompiledTraitBuilder::new("RigidMount")
            .refinement("Rigid")
            .require_let("vol", Type::Real)
            .require_sub("mount", "MountPoint")
            .build();
        assert_eq!(t.refinements.len(), 1);
        assert_eq!(t.refinements[0], "Rigid");
        assert_eq!(t.required_members.len(), 2);
        assert!(matches!(&t.required_members[0].kind, RequirementKind::Let(_)));
        assert!(matches!(&t.required_members[1].kind, RequirementKind::Sub(s) if s == "MountPoint"));
        assert_ne!(t.content_hash, ContentHash(0));
    }

    #[test]
    fn trait_builder_defaults_initially_empty() {
        let t: CompiledTrait = CompiledTraitBuilder::new("Bounded").build();
        assert_eq!(t.defaults.len(), 0);
        assert_eq!(t.type_params.len(), 0);
    }
}

// --- Tests for constraint expression helpers ---

#[cfg(test)]
mod constraint_helper_tests {
    use super::*;

    #[test]
    fn equality_constraint_returns_single_bool_expr() {
        let exprs = equality_constraint(
            "Beam",
            "ratio",
            Type::Real,
            literal(Value::Real(2.0)),
        );
        assert_eq!(exprs.len(), 1, "equality_constraint should return exactly 1 expr");
        assert_eq!(exprs[0].result_type, Type::Bool, "expr should be Bool");
        assert!(
            matches!(&exprs[0].kind, CompiledExprKind::BinOp { op: BinOp::Eq, .. }),
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
        assert_eq!(all_exprs.len(), 4, "should have 4 constraint expressions total");
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
        assert_eq!(exprs.len(), 2, "range_constraint should return exactly 2 exprs");
        assert_eq!(exprs[0].result_type, Type::Bool, "first expr should be Bool");
        assert_eq!(exprs[1].result_type, Type::Bool, "second expr should be Bool");
        // First expr should be a Gt comparison, second a Lt comparison
        assert!(
            matches!(&exprs[0].kind, CompiledExprKind::BinOp { op: BinOp::Gt, .. }),
            "first expr should be Gt"
        );
        assert!(
            matches!(&exprs[1].kind, CompiledExprKind::BinOp { op: BinOp::Lt, .. }),
            "second expr should be Lt"
        );
    }
}

// --- Tests for extended CompiledModuleBuilder (step-19) ---

#[cfg(test)]
mod module_builder_extension_tests {
    use super::*;
    use crate::celsius;
    use crate::kelvin;
    use crate::type_alias_module;
    use reify_types::{EnumDef, ModulePath};

    fn module_path() -> ModulePath {
        ModulePath::new(vec!["test".to_string()])
    }

    #[test]
    fn module_builder_with_trait_def() {
        let t = CompiledTraitBuilder::new("Rigid")
            .require_param("thickness", Type::length())
            .build();
        let module = CompiledModuleBuilder::new(module_path())
            .trait_def(t)
            .build();
        assert_eq!(module.trait_defs.len(), 1);
        assert_eq!(module.trait_defs[0].name, "Rigid");
    }

    // step-26: failing test — content_hash must differ when trait_defs differ
    #[test]
    fn module_builder_trait_defs_affect_content_hash() {
        let module_no_traits = CompiledModuleBuilder::new(module_path()).build();
        let ct = TraitDefBuilder::new("Rigid").build();
        let module_with_trait = CompiledModuleBuilder::new(module_path()).trait_def(ct).build();
        assert_ne!(
            module_no_traits.content_hash,
            module_with_trait.content_hash,
            "modules differing only in trait_defs must produce distinct content_hashes"
        );
    }

    #[test]
    fn module_builder_with_field() {
        let body = literal(Value::Real(1.0));
        let f = CompiledFieldBuilder::new("temp", Type::Geometry, Type::Real)
            .analytical(body)
            .build();
        let module = CompiledModuleBuilder::new(module_path())
            .field(f)
            .build();
        assert_eq!(module.fields.len(), 1);
        assert_eq!(module.fields[0].name, "temp");
    }

    #[test]
    fn module_builder_with_enum_def() {
        let e = EnumDef { name: "Color".to_string(), variants: vec!["Red".to_string(), "Blue".to_string()] };
        let module = CompiledModuleBuilder::new(module_path())
            .enum_def(e)
            .build();
        assert_eq!(module.enum_defs.len(), 1);
        assert_eq!(module.enum_defs[0].name, "Color");
        assert_eq!(module.enum_defs[0].variants.len(), 2);
    }

    #[test]
    fn module_builder_with_compiled_purpose() {
        let p = CompiledPurposeBuilder::new("mfg_ready")
            .param("subject", "Structure")
            .build();
        let module = CompiledModuleBuilder::new(module_path())
            .compiled_purpose(p)
            .build();
        assert_eq!(module.compiled_purposes.len(), 1);
        assert_eq!(module.compiled_purposes[0].name, "mfg_ready");
    }

    #[test]
    fn module_builder_hash_changes_with_new_fields() {
        let empty_module = CompiledModuleBuilder::new(module_path()).build();
        let t = CompiledTraitBuilder::new("Rigid").build();
        let with_trait = CompiledModuleBuilder::new(module_path()).trait_def(t).build();
        assert_ne!(empty_module.content_hash, with_trait.content_hash);
    }

    // step-5: failing tests for option expression helpers
    #[test]
    fn option_some_expr_wraps_inner_with_option_type() {
        let inner = literal(Value::Int(42));
        let expr = option_some_expr(inner);
        assert_eq!(expr.result_type, Type::Option(Box::new(Type::Int)));
        assert!(matches!(expr.kind, CompiledExprKind::OptionSome(_)));
    }

    #[test]
    fn option_none_expr_produces_option_none_kind() {
        let expr = option_none_expr(Type::Real);
        assert_eq!(expr.result_type, Type::Option(Box::new(Type::Real)));
        assert!(matches!(expr.kind, CompiledExprKind::OptionNone));
    }

    #[test]
    fn option_some_expr_content_hash_is_nonzero() {
        let inner = literal(Value::Int(42));
        let expr = option_some_expr(inner);
        assert_ne!(expr.content_hash, reify_types::ContentHash::of(&[]));
    }

    // step-7: failing tests for free_param and param_no_default
    #[test]
    fn free_param_creates_auto_kind_cell() {
        use reify_compiler::ValueCellKind;
        let template = TopologyTemplateBuilder::new("S")
            .free_param("S", "x", Type::Scalar { dimension: DimensionVector::LENGTH })
            .build();
        let cell = &template.value_cells[0];
        assert_eq!(cell.kind, ValueCellKind::Auto);
        assert!(cell.default_expr.is_none());
        assert_eq!(cell.visibility, reify_compiler::Visibility::Public);
    }

    #[test]
    fn free_param_is_equivalent_to_auto_param() {
        let ty = Type::Scalar { dimension: DimensionVector::LENGTH };
        let via_free = TopologyTemplateBuilder::new("S")
            .free_param("S", "x", ty.clone())
            .build();
        let via_auto = TopologyTemplateBuilder::new("S")
            .auto_param("S", "x", ty.clone())
            .build();
        assert_eq!(via_free.value_cells[0].kind, via_auto.value_cells[0].kind);
        assert_eq!(via_free.value_cells[0].cell_type, via_auto.value_cells[0].cell_type);
        // Both should have no default expression
        assert!(via_free.value_cells[0].default_expr.is_none());
        assert!(via_auto.value_cells[0].default_expr.is_none());
    }

    #[test]
    fn param_no_default_creates_param_kind_without_default() {
        use reify_compiler::ValueCellKind;
        let template = TopologyTemplateBuilder::new("S")
            .param_no_default("S", "y", Type::Int)
            .build();
        let cell = &template.value_cells[0];
        assert_eq!(cell.kind, ValueCellKind::Param);
        assert!(cell.default_expr.is_none());
        assert_eq!(cell.visibility, reify_compiler::Visibility::Public);
    }

    // step-9: failing test for type_alias_module fixture
    #[test]
    fn type_alias_module_fixture_returns_compiled_module() {
        let module = type_alias_module();
        // Should have at least one template
        assert!(!module.templates.is_empty(), "should have at least one topology template");
        // Module should have a valid content_hash (non-zero)
        assert_ne!(module.content_hash, reify_types::ContentHash::of(&[]));
        // The template should have temperature-dimensioned params
        let template = &module.templates[0];
        let has_temperature_param = template.value_cells.iter().any(|c| {
            matches!(
                c.cell_type,
                Type::Scalar { dimension } if dimension == DimensionVector::TEMPERATURE
            )
        });
        assert!(has_temperature_param, "HeatExchanger should have a TEMPERATURE param");
    }

    // step-11: integration test composing all new helpers
    #[test]
    fn integration_reactor_module_with_all_new_helpers() {
        // Compose a Reactor module using free_param, celsius() default, range constraint,
        // option_some_expr/option_none_expr, param_no_default
        let e = "Reactor";
        let temp_type = Type::Scalar { dimension: DimensionVector::TEMPERATURE };

        // param with celsius(25.0) as default
        let temp_default = literal(celsius(25.0));

        // Range constraint: temperature in [kelvin(273.15), kelvin(773.15)]
        let temp_ref = value_ref_typed(e, "max_temp", temp_type.clone());
        let lower_bound = literal(kelvin(273.15));
        let upper_bound = literal(kelvin(773.15));
        let range_constraint_expr = and(
            ge(temp_ref.clone(), lower_bound),
            le(temp_ref.clone(), upper_bound),
        );

        // option_some_expr and option_none_expr usage in a conditional
        let opt_temp = option_some_expr(temp_ref.clone());
        let opt_none = option_none_expr(temp_type.clone());
        let cond = conditional_expr(literal(Value::Bool(true)), opt_temp, opt_none);
        assert_eq!(cond.result_type, Type::Option(Box::new(temp_type.clone())));

        let template = TopologyTemplateBuilder::new(e)
            .free_param(e, "operating_temp", temp_type.clone())
            .param(e, "max_temp", temp_type.clone(), Some(temp_default))
            .param_no_default(e, "set_point", temp_type.clone())
            .constraint(e, 0, Some("temp_range"), range_constraint_expr)
            .build();

        let module = CompiledModuleBuilder::new(module_path())
            .template(template)
            .build();

        assert_eq!(module.templates.len(), 1);
        let tmpl = &module.templates[0];
        // free_param + param + param_no_default = 3 cells
        assert_eq!(tmpl.value_cells.len(), 3);
        // 1 constraint
        assert_eq!(tmpl.constraints.len(), 1);
        assert_ne!(module.content_hash, reify_types::ContentHash::of(&[]));
    }
}
