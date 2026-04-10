use super::*;

/// Shared reference to entity definition fields (used by both StructureDef and OccurrenceDef).
pub(crate) struct EntityDefRef<'a> {
    pub(crate) name: &'a str,
    pub(crate) is_pub: bool,
    pub(crate) type_params: &'a [reify_syntax::TypeParamDecl],
    pub(crate) trait_bounds: &'a [reify_syntax::TraitBoundRef],
    pub(crate) members: &'a [reify_syntax::MemberDecl],
    pub(crate) annotations: &'a [reify_syntax::Annotation],
    pub(crate) pragmas: &'a [reify_syntax::Pragma],
    pub(crate) span: SourceSpan,
    #[allow(dead_code)]
    pub(crate) content_hash: ContentHash,
}

impl<'a> From<&'a reify_syntax::StructureDef> for EntityDefRef<'a> {
    fn from(s: &'a reify_syntax::StructureDef) -> Self {
        EntityDefRef {
            name: &s.name,
            is_pub: s.is_pub,
            type_params: &s.type_params,
            trait_bounds: &s.trait_bounds,
            members: &s.members,
            annotations: &s.annotations,
            pragmas: &s.pragmas,
            span: s.span,
            content_hash: s.content_hash,
        }
    }
}

impl<'a> From<&'a reify_syntax::OccurrenceDef> for EntityDefRef<'a> {
    fn from(o: &'a reify_syntax::OccurrenceDef) -> Self {
        EntityDefRef {
            name: &o.name,
            is_pub: o.is_pub,
            type_params: &o.type_params,
            trait_bounds: &o.trait_bounds,
            members: &o.members,
            annotations: &o.annotations,
            pragmas: &o.pragmas,
            span: o.span,
            content_hash: o.content_hash,
        }
    }
}


/// Substitute constraint parameter references in an AST expression.
///
/// Recursively walks `expr` and replaces every `ExprKind::Ident(name)` where
/// `name` is a key in `bindings` with the corresponding bound expression.
/// Lambda and quantifier bodies respect lexical shadowing — when a binder
/// introduces a name that overlaps a constraint param, the inner name takes
/// precedence and substitution is suppressed for that name inside the body.
pub(crate) fn substitute_expr(
    expr: &reify_syntax::Expr,
    bindings: &HashMap<String, reify_syntax::Expr>,
) -> reify_syntax::Expr {
    use reify_syntax::{Expr, ExprKind, MatchArm};
    let span = expr.span;
    let new_kind = match &expr.kind {
        // Leaf variants — no sub-expressions to recurse into.
        ExprKind::NumberLiteral(n) => ExprKind::NumberLiteral(*n),
        ExprKind::QuantityLiteral { value, unit } => ExprKind::QuantityLiteral {
            value: *value,
            unit: unit.clone(),
        },
        ExprKind::StringLiteral(s) => ExprKind::StringLiteral(s.clone()),
        ExprKind::BoolLiteral(b) => ExprKind::BoolLiteral(*b),
        ExprKind::Auto { free } => ExprKind::Auto { free: *free },
        ExprKind::EnumAccess { type_name, variant } => ExprKind::EnumAccess {
            type_name: type_name.clone(),
            variant: variant.clone(),
        },

        // Identifier — the substitution point.
        ExprKind::Ident(name) => {
            if let Some(replacement) = bindings.get(name) {
                return replacement.clone();
            }
            ExprKind::Ident(name.clone())
        }

        // Compound variants — recurse into sub-expressions.
        ExprKind::BinOp { op, left, right } => ExprKind::BinOp {
            op: op.clone(),
            left: Box::new(substitute_expr(left, bindings)),
            right: Box::new(substitute_expr(right, bindings)),
        },
        ExprKind::UnOp { op, operand } => ExprKind::UnOp {
            op: op.clone(),
            operand: Box::new(substitute_expr(operand, bindings)),
        },
        ExprKind::FunctionCall { name, args } => ExprKind::FunctionCall {
            name: name.clone(),
            args: args.iter().map(|a| substitute_expr(a, bindings)).collect(),
        },
        ExprKind::MemberAccess { object, member } => ExprKind::MemberAccess {
            object: Box::new(substitute_expr(object, bindings)),
            member: member.clone(),
        },
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => ExprKind::Conditional {
            condition: Box::new(substitute_expr(condition, bindings)),
            then_branch: Box::new(substitute_expr(then_branch, bindings)),
            else_branch: Box::new(substitute_expr(else_branch, bindings)),
        },
        ExprKind::ListLiteral(items) => {
            ExprKind::ListLiteral(items.iter().map(|i| substitute_expr(i, bindings)).collect())
        }
        ExprKind::SetLiteral(items) => {
            ExprKind::SetLiteral(items.iter().map(|i| substitute_expr(i, bindings)).collect())
        }
        ExprKind::MapLiteral(pairs) => ExprKind::MapLiteral(
            pairs
                .iter()
                .map(|(k, v)| (substitute_expr(k, bindings), substitute_expr(v, bindings)))
                .collect(),
        ),
        ExprKind::IndexAccess { object, index } => ExprKind::IndexAccess {
            object: Box::new(substitute_expr(object, bindings)),
            index: Box::new(substitute_expr(index, bindings)),
        },
        ExprKind::Match { discriminant, arms } => ExprKind::Match {
            discriminant: Box::new(substitute_expr(discriminant, bindings)),
            arms: arms
                .iter()
                .map(|arm| MatchArm {
                    patterns: arm.patterns.clone(),
                    body: substitute_expr(&arm.body, bindings),
                    span: arm.span,
                })
                .collect(),
        },
        // Lambda — remove params that shadow constraint param names to respect scoping.
        ExprKind::Lambda { params, body } => {
            let shadowed: std::collections::HashSet<&str> =
                params.iter().map(|p| p.name.as_str()).collect();
            let inner_bindings: HashMap<String, Expr> = bindings
                .iter()
                .filter(|(k, _)| !shadowed.contains(k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            ExprKind::Lambda {
                params: params.clone(),
                body: Box::new(substitute_expr(body, &inner_bindings)),
            }
        }
        // Quantifier — the bound variable shadows constraint params in the predicate.
        ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
        } => {
            // The collection expression is evaluated in the outer scope.
            let sub_collection = substitute_expr(collection, bindings);
            // The predicate is evaluated with the variable shadowing any same-named binding.
            let inner_bindings: HashMap<String, Expr> = bindings
                .iter()
                .filter(|(k, _)| k.as_str() != variable.as_str())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            ExprKind::Quantifier {
                kind: *kind,
                variable: variable.clone(),
                collection: Box::new(sub_collection),
                predicate: Box::new(substitute_expr(predicate, &inner_bindings)),
            }
        }
        ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => ExprKind::AdHocSelector {
            base: Box::new(substitute_expr(base, bindings)),
            selector: selector.clone(),
            args: args.iter().map(|a| substitute_expr(a, bindings)).collect(),
        },
        ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => ExprKind::Range {
            lower: lower
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, bindings))),
            upper: upper
                .as_ref()
                .map(|e| Box::new(substitute_expr(e, bindings))),
            lower_inclusive: *lower_inclusive,
            upper_inclusive: *upper_inclusive,
        },
        ExprKind::QualifiedAccess { qualifier, member } => ExprKind::QualifiedAccess {
            qualifier: Box::new(substitute_expr(qualifier, bindings)),
            member: member.clone(),
        },
        ExprKind::InstanceQualifiedAccess { object, qualified } => {
            ExprKind::InstanceQualifiedAccess {
                object: Box::new(substitute_expr(object, bindings)),
                qualified: Box::new(substitute_expr(qualified, bindings)),
            }
        }
    };
    Expr {
        kind: new_kind,
        span,
    }
}

/// Compile a single entity definition (structure or occurrence) into a topology template.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_entity(
    structure: &EntityDefRef<'_>,
    entity_kind: EntityKind,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    trait_registry: &HashMap<String, &CompiledTrait>,
    field_registry: &HashMap<String, &CompiledField>,
    constraint_def_registry: &HashMap<String, &reify_syntax::ConstraintDef>,
    unit_registry: &UnitRegistry,
    alias_registry: &TypeAliasRegistry,
    pending_bound_checks: &mut Vec<PendingBoundCheck>,
    diagnostics: &mut Vec<Diagnostic>,
    compiled_templates: &[TopologyTemplate],
) -> TopologyTemplate {
    let entity_name = structure.name;
    let mut scope = CompilationScope::new(entity_name);
    scope.set_unit_registry(unit_registry);
    scope.is_entity_scope = true;

    // Populate trait member index for qualified access resolution.
    for (trait_name, compiled_trait) in trait_registry {
        let mut members: HashSet<String> = compiled_trait
            .required_members
            .iter()
            .map(|m| m.name.clone())
            .collect();
        for default in &compiled_trait.defaults {
            if let Some(n) = &default.name {
                members.insert(n.clone());
            }
        }
        scope.trait_members.insert(trait_name.clone(), members);
    }

    let mut value_cells = Vec::new();
    let mut constraints = Vec::new();
    let mut sub_components: Vec<SubComponentDecl> = Vec::new();
    let mut ports: Vec<CompiledPort> = Vec::new();
    let mut port_names: HashMap<String, SourceSpan> = HashMap::new();
    let mut duplicate_port_names: HashSet<String> = HashSet::new();
    let mut guarded_groups: Vec<CompiledGuardedGroup> = Vec::new();
    let mut structure_controlling: HashSet<ValueCellId> = HashSet::new();
    let mut connections: Vec<CompiledConnection> = Vec::new();
    let mut objective: Option<OptimizationObjective> = None;
    let mut first_meta_span: Option<SourceSpan> = None;
    let mut constraint_index: u32 = 0;
    let mut guard_index: u32 = 0;
    let mut connector_index: u32 = 0;

    // Collect type parameter names for this structure so we can resolve
    // member types like `param contents : T` to Type::TypeParam("T").
    let type_param_names: HashSet<String> = structure
        .type_params
        .iter()
        .map(|tp| tp.name.clone())
        .collect();

    // Register field names into the scope so expressions can reference fields
    // (e.g., `sample(my_field, point)`). Fields use the FIELD_ENTITY_PREFIX.
    for (field_name, field) in field_registry {
        let field_id = ValueCellId::new(FIELD_ENTITY_PREFIX, field_name);
        let field_type = Type::Field {
            domain: Box::new(field.domain_type.clone()),
            codomain: Box::new(field.codomain_type.clone()),
        };
        scope
            .names
            .insert(field_name.clone(), (field_id, field_type, None));
    }

    // First pass: register all param and let names into the scope so they can
    // reference each other (forward references within the structure).
    // We need types for the scope, so we resolve types in this pass as well.
    for member in structure.members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let ty = if let Some(type_expr) = &param.type_expr {
                    match resolve_type_expr_with_aliases(
                        type_expr,
                        &type_param_names,
                        alias_registry,
                        diagnostics,
                    ) {
                        Some(t) => t,
                        None => {
                            // Check if it's an enum type defined in the same module or prelude
                            if enum_defs.iter().any(|e| e.name == type_expr.name) {
                                Type::Enum(type_expr.name.clone())
                            } else {
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "unresolved type: {}",
                                        type_expr.name
                                    ))
                                    .with_label(
                                        DiagnosticLabel::new(type_expr.span, "unknown type name"),
                                    ),
                                );
                                Type::Real // fallback
                            }
                        }
                    }
                } else {
                    // Infer type from default expression if available
                    Type::Real
                };
                scope.register(&param.name, ty);
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                // For lets, we need to infer the type from the expression.
                // Geometry lets produce realizations (not value cells) but still
                // need to be registered in scope so subsequent lets can reference them.
                if is_geometry_let(&let_decl.value, functions) {
                    scope.register(&let_decl.name, Type::Geometry);
                } else {
                    // We'll register with a placeholder type; the actual type will
                    // be determined when we compile the expression. For now, use Real.
                    // We'll update this after the expression is compiled.
                    scope.register(&let_decl.name, Type::Real);
                }
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                register_guarded_names(&g.members, &mut scope, functions, diagnostics);
                register_guarded_names(&g.else_members, &mut scope, functions, diagnostics);
            }
            reify_syntax::MemberDecl::Port(port_decl) => {
                if let Some(first_span) = port_names.get(&port_decl.name) {
                    // Duplicate port name — emit error and skip registration
                    diagnostics.push(
                        Diagnostic::error(format!("duplicate port name '{}'", port_decl.name))
                            .with_label(DiagnosticLabel::new(
                                port_decl.span,
                                "duplicate defined here",
                            ))
                            .with_label(DiagnosticLabel::new(*first_span, "first defined here")),
                    );
                    duplicate_port_names.insert(port_decl.name.clone());
                    continue;
                }
                port_names.insert(port_decl.name.clone(), port_decl.span);
                scope.port_names.insert(port_decl.name.clone());
                // Register port body members with composite names: port_name.member_name
                for port_member in &port_decl.members {
                    match port_member {
                        reify_syntax::MemberDecl::Param(param) => {
                            let composite_name = format!("{}.{}", port_decl.name, param.name);
                            let ty = if let Some(type_expr) = &param.type_expr {
                                resolve_type_name(&type_expr.name).unwrap_or_else(|| {
                                    diagnostics.push(
                                        Diagnostic::error(format!(
                                            "unresolved type name '{}' in port parameter",
                                            type_expr.name
                                        ))
                                        .with_label(DiagnosticLabel::new(
                                            type_expr.span,
                                            "unknown type",
                                        )),
                                    );
                                    Type::Real
                                })
                            } else {
                                Type::Real
                            };
                            let id = ValueCellId::new(entity_name, &composite_name);
                            scope.names.insert(composite_name, (id, ty, None));
                        }
                        reify_syntax::MemberDecl::Let(let_decl) => {
                            let composite_name = format!("{}.{}", port_decl.name, let_decl.name);
                            let id = ValueCellId::new(entity_name, &composite_name);
                            scope.names.insert(composite_name, (id, Type::Real, None));
                        }
                        _ => {}
                    }
                }
            }
            reify_syntax::MemberDecl::Sub(sub) => {
                // Register sub-component type info for instance qualified access.
                scope
                    .sub_component_types
                    .insert(sub.name.clone(), sub.structure_name.clone());
                if let Some(child_tmpl) = compiled_templates
                    .iter()
                    .find(|t| t.name == sub.structure_name)
                {
                    // Deprecation check: warn if the referenced structure is @deprecated.
                    if let Some(msg) = deprecation_message(&child_tmpl.annotations) {
                        emit_deprecation_warning(
                            "structure",
                            &sub.structure_name,
                            &msg,
                            sub.span,
                            diagnostics,
                        );
                    }
                    scope
                        .sub_structure_traits
                        .insert(sub.structure_name.clone(), child_tmpl.trait_bounds.clone());
                }
                // Populate sub_member_types for ALL subs (for self.sub.member resolution).
                if let Some(child_tmpl) = compiled_templates
                    .iter()
                    .find(|t| t.name == sub.structure_name)
                {
                    let member_types: HashMap<String, Type> = child_tmpl
                        .value_cells
                        .iter()
                        .map(|vc| (vc.id.member.clone(), vc.cell_type.clone()))
                        .collect();
                    scope
                        .sub_member_types
                        .insert(sub.name.clone(), member_types);
                }
                if sub.is_collection {
                    scope.collection_sub_names.insert(sub.name.clone());
                    // Populate member types from already-compiled child template
                    if let Some(child_tmpl) = compiled_templates
                        .iter()
                        .find(|t| t.name == sub.structure_name)
                    {
                        let member_types: HashMap<String, Type> = child_tmpl
                            .value_cells
                            .iter()
                            .map(|vc| (vc.id.member.clone(), vc.cell_type.clone()))
                            .collect();
                        scope
                            .collection_sub_member_types
                            .insert(sub.name.clone(), member_types);
                    }
                }
            }
            reify_syntax::MemberDecl::MetaBlock(meta) => {
                if let Some(first_span) = first_meta_span {
                    diagnostics.push(
                        Diagnostic::error("duplicate meta block".to_string())
                            .with_label(DiagnosticLabel::new(meta.span, "duplicate defined here"))
                            .with_label(DiagnosticLabel::new(first_span, "first defined here")),
                    );
                } else {
                    first_meta_span = Some(meta.span);
                    for (key, value) in &meta.entries {
                        scope.meta_entries.insert(key.clone(), value.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // Trait conformance checking: verify structure satisfies all trait bounds.
    if !structure.trait_bounds.is_empty() {
        check_trait_conformance(
            structure,
            trait_registry,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            enum_defs,
            functions,
            alias_registry,
            diagnostics,
        );

        // Deprecation check: warn for each trait bound that references a @deprecated trait.
        for trait_bound in structure.trait_bounds {
            if let Some(compiled_trait) = trait_registry.get(&trait_bound.name)
                && let Some(msg) = deprecation_message(&compiled_trait.annotations)
            {
                emit_deprecation_warning(
                    "trait",
                    &trait_bound.name,
                    &msg,
                    trait_bound.span,
                    diagnostics,
                );
            }
        }

        // Defer type argument checking on parameterized trait bounds (e.g., Container<Bolt>)
        // to the post-compilation pass so forward references are resolved correctly.
        for trait_bound in structure.trait_bounds {
            if !trait_bound.type_args.is_empty()
                && let Some(compiled_trait) = trait_registry.get(&trait_bound.name)
                && !compiled_trait.type_params.is_empty()
            {
                let resolved_args: Vec<Type> = trait_bound
                    .type_args
                    .iter()
                    .map(|ta| {
                        resolve_type_name(&ta.name).unwrap_or_else(|| {
                            if type_param_names.contains(&ta.name) {
                                Type::TypeParam(ta.name.clone())
                            } else {
                                Type::StructureRef(ta.name.clone())
                            }
                        })
                    })
                    .collect();
                // TraitConformance: type_params are known now from the compiled
                // trait, so they're carried directly in the enum variant.
                pending_bound_checks.push(PendingBoundCheck::TraitConformance {
                    type_params: compiled_trait.type_params.clone(),
                    type_args: resolved_args,
                    target_name: trait_bound.name.clone(),
                    span: trait_bound.span,
                });
            }
        }
    }

    // Second pass: compile all members.
    for member in structure.members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let id = ValueCellId::new(entity_name, &param.name);
                let cell_type = scope
                    .resolve(&param.name)
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or_else(|| {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "internal compiler error: unresolved name '{}' in pass 2",
                                param.name
                            ))
                            .with_label(DiagnosticLabel::new(
                                param.span,
                                "ICE: name should have been registered in pass 1",
                            )),
                        );
                        Type::Real
                    });

                // Check if the default is ExprKind::Auto and extract the free flag
                let auto_free: Option<bool> =
                    param.default.as_ref().and_then(|expr| {
                        if let reify_syntax::ExprKind::Auto { free } = &expr.kind {
                            Some(*free)
                        } else {
                            None
                        }
                    });

                let decl = if let Some(free) = auto_free {
                    ValueCellDecl {
                        id,
                        kind: ValueCellKind::Auto { free },
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr: None,
                        span: param.span,
                    }
                } else {
                    let default_expr = param.default.as_ref().map(|expr| {
                        let mut compiled =
                            compile_expr(expr, &scope, enum_defs, functions, diagnostics);
                        // If the default is OptionNone and the param type is Option<T>,
                        // override the OptionNone's result_type to match the declared type.
                        if matches!(&compiled.kind, CompiledExprKind::OptionNone)
                            && matches!(&cell_type, Type::Option(_))
                        {
                            compiled = CompiledExpr::option_none(cell_type.clone());
                        }
                        compiled
                    });

                    ValueCellDecl {
                        id,
                        kind: ValueCellKind::Param,
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr,
                        span: param.span,
                    }
                };

                if let Some(wc) = &param.where_clause {
                    compile_per_decl_guard(
                        entity_name,
                        wc,
                        decl,
                        &mut scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        &mut guarded_groups,
                        &mut structure_controlling,
                        &mut guard_index,
                    );
                } else {
                    value_cells.push(decl);
                }
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                // Skip geometry-producing function calls
                if is_geometry_let(&let_decl.value, functions) {
                    continue;
                }

                let mut compiled_expr =
                    compile_expr(&let_decl.value, &scope, enum_defs, functions, diagnostics);
                // If the value is `none` and the let has a typed annotation like
                // `Option<Length>`, override the OptionNone's result_type to match
                // the declared type — mirroring the param-default fixup at lines
                // 5179-5185.
                if matches!(&compiled_expr.kind, CompiledExprKind::OptionNone)
                    && let Some(type_expr) = &let_decl.type_expr
                    && let Some(resolved) = resolve_type_expr_with_aliases(
                        type_expr,
                        &type_param_names,
                        alias_registry,
                        diagnostics,
                    )
                    && matches!(&resolved, Type::Option(_))
                {
                    compiled_expr = CompiledExpr::option_none(resolved);
                }
                let cell_type = compiled_expr.result_type.clone();
                let id = ValueCellId::new(entity_name, &let_decl.name);

                // Update the scope with the inferred type
                scope.register(&let_decl.name, cell_type.clone());

                let visibility = if let_decl.is_pub {
                    Visibility::Public
                } else {
                    Visibility::Private
                };

                let decl = ValueCellDecl {
                    id,
                    kind: ValueCellKind::Let,
                    visibility,
                    cell_type,
                    default_expr: Some(compiled_expr),
                    span: let_decl.span,
                };

                if let Some(wc) = &let_decl.where_clause {
                    compile_per_decl_guard(
                        entity_name,
                        wc,
                        decl,
                        &mut scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        &mut guarded_groups,
                        &mut structure_controlling,
                        &mut guard_index,
                    );
                } else {
                    value_cells.push(decl);
                }
            }
            reify_syntax::MemberDecl::Constraint(constraint) => {
                // Detect collection count constraint pattern:
                //   `collection_name.count == expr`  or  `expr == collection_name.count`
                if let Some((coll_name, count_expr)) =
                    extract_count_constraint(&constraint.expr, &scope.collection_sub_names)
                {
                    let compiled_rhs =
                        compile_expr(count_expr, &scope, enum_defs, functions, diagnostics);
                    let count_member = format!("__count_{}", coll_name);
                    let count_id = ValueCellId::new(entity_name, &count_member);
                    value_cells.push(ValueCellDecl {
                        id: count_id.clone(),
                        kind: ValueCellKind::Let,
                        visibility: Visibility::Private,
                        cell_type: Type::Int,
                        default_expr: Some(compiled_rhs),
                        span: constraint.span,
                    });
                    structure_controlling.insert(count_id.clone());
                    // Store count_cell on the matching SubComponentDecl
                    if let Some(sub) = sub_components.iter_mut().find(|s| s.name == coll_name) {
                        sub.count_cell = Some(count_id);
                    }
                } else {
                    let compiled_expr =
                        compile_expr(&constraint.expr, &scope, enum_defs, functions, diagnostics);

                    // Check that the constraint expression produces Bool
                    if compiled_expr.result_type != Type::Bool {
                        diagnostics.push(
                            Diagnostic::warning(format!(
                                "constraint expression has type {}, expected Bool",
                                compiled_expr.result_type,
                            ))
                            .with_label(DiagnosticLabel::new(
                                constraint.expr.span,
                                "expected Bool",
                            )),
                        );
                    }

                    let id = ConstraintNodeId::new(entity_name, constraint_index);
                    let cc = CompiledConstraint {
                        id,
                        label: constraint.label.clone(),
                        expr: compiled_expr,
                        span: constraint.span,
                        domain: None,
                    };
                    constraint_index += 1;

                    if let Some(wc) = &constraint.where_clause {
                        compile_per_decl_constraint_guard(
                            entity_name,
                            wc,
                            cc,
                            &mut scope,
                            enum_defs,
                            functions,
                            diagnostics,
                            &mut guarded_groups,
                            &mut structure_controlling,
                            &mut guard_index,
                        );
                    } else {
                        constraints.push(cc);
                    }
                }
            }
            reify_syntax::MemberDecl::Sub(sub) => {
                let compiled_args: Vec<(String, CompiledExpr)> = sub
                    .args
                    .iter()
                    .map(|(name, expr)| {
                        (
                            name.clone(),
                            compile_expr(expr, &scope, enum_defs, functions, diagnostics),
                        )
                    })
                    .collect();

                // Resolve type arguments to Type values.
                let resolved_type_args: Vec<Type> = sub
                    .type_args
                    .iter()
                    .map(|ta| {
                        resolve_type_name(&ta.name).unwrap_or_else(|| {
                            if type_param_names.contains(&ta.name) {
                                Type::TypeParam(ta.name.clone())
                            } else {
                                Type::StructureRef(ta.name.clone())
                            }
                        })
                    })
                    .collect();

                // SubComponent: defer bound checking to the post-compilation
                // pass so forward-referenced structures are available in the
                // registry. type_params are resolved from the target template
                // during the post-pass. Always push — even with empty
                // type_args, the target may have type params requiring defaults.
                {
                    pending_bound_checks.push(PendingBoundCheck::SubComponent {
                        type_args: resolved_type_args.clone(),
                        target_name: sub.structure_name.clone(),
                        span: sub.span,
                    });
                }

                // Compile the sub's where_clause into guard_expr (used by termination check).
                // If compilation emits any diagnostics (errors), store None rather than
                // Some(broken_expr). This prevents the termination check from seeing a guard
                // that has no ValueRefs (because the identifier failed to resolve) and then
                // emitting a spurious "guard references no Int/Bool param" error on top of the
                // real compilation error.
                let sub_guard_expr = sub.where_clause.as_ref().and_then(|wc| {
                    let diag_count_before = diagnostics.len();
                    let compiled =
                        compile_expr(&wc.condition, &scope, enum_defs, functions, diagnostics);
                    if diagnostics.len() > diag_count_before {
                        // Guard compilation emitted diagnostics — guard is unusable for
                        // termination analysis. Return None to avoid cascading errors.
                        None
                    } else {
                        Some(compiled)
                    }
                });

                sub_components.push(SubComponentDecl {
                    name: sub.name.clone(),
                    structure_name: sub.structure_name.clone(),
                    visibility: Visibility::Public,
                    args: compiled_args,
                    type_args: resolved_type_args,
                    is_collection: sub.is_collection,
                    count_cell: None,
                    guard_expr: sub_guard_expr,
                    span: sub.span,
                    content_hash: sub.content_hash,
                });
            }
            reify_syntax::MemberDecl::Minimize(min_decl) => {
                let compiled_expr =
                    compile_expr(&min_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective = Some(OptimizationObjective::Minimize(compiled_expr));
            }
            reify_syntax::MemberDecl::Maximize(max_decl) => {
                let compiled_expr =
                    compile_expr(&max_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective = Some(OptimizationObjective::Maximize(compiled_expr));
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                compile_block_guard(
                    entity_name,
                    g,
                    None, // no outer guard
                    &mut scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    &mut guarded_groups,
                    &mut structure_controlling,
                    &mut guard_index,
                    &mut constraint_index,
                );
            }
            reify_syntax::MemberDecl::AssociatedType(_) => {
                // Associated type compilation deferred to a later milestone.
            }
            reify_syntax::MemberDecl::Port(port_decl) => {
                // Skip duplicate port names (already reported in first pass).
                // The first occurrence is compiled; subsequent duplicates are skipped.
                if duplicate_port_names.contains(&port_decl.name)
                    && !port_names
                        .get(&port_decl.name)
                        .is_some_and(|&span| span == port_decl.span)
                {
                    continue;
                }
                let direction = port_decl
                    .direction
                    .unwrap_or(reify_types::PortDirection::Bidi);

                // Verify port type_name exists in the trait registry
                if !trait_registry.contains_key(&port_decl.type_name) {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "unknown port type '{}' — no trait with this name found in current module",
                            port_decl.type_name
                        ))
                        .with_label(DiagnosticLabel::new(
                            port_decl.span,
                            "unknown port type",
                        )),
                    );
                }

                let mut port_members = Vec::new();
                let mut port_constraints = Vec::new();

                for port_member in &port_decl.members {
                    match port_member {
                        reify_syntax::MemberDecl::Param(param) => {
                            let composite_name = format!("{}.{}", port_decl.name, param.name);
                            let id = ValueCellId::new(entity_name, &composite_name);
                            let cell_type = scope
                                .resolve(&composite_name)
                                .map(|(_, ty)| ty.clone())
                                .unwrap_or_else(|| {
                                    diagnostics.push(
                                        Diagnostic::error(format!(
                                            "internal compiler error: unresolved name '{}' in pass 2",
                                            composite_name
                                        ))
                                        .with_label(DiagnosticLabel::new(
                                            param.span,
                                            "ICE: name should have been registered in pass 1",
                                        )),
                                    );
                                    Type::Real
                                });

                            let auto_free: Option<bool> =
                                param.default.as_ref().and_then(|expr| {
                                    if let reify_syntax::ExprKind::Auto { free } = &expr.kind {
                                        Some(*free)
                                    } else {
                                        None
                                    }
                                });

                            let decl = if let Some(free) = auto_free {
                                ValueCellDecl {
                                    id,
                                    kind: ValueCellKind::Auto { free },
                                    visibility: Visibility::Public,
                                    cell_type,
                                    default_expr: None,
                                    span: param.span,
                                }
                            } else {
                                let default_expr = param.default.as_ref().map(|expr| {
                                    compile_expr(expr, &scope, enum_defs, functions, diagnostics)
                                });

                                ValueCellDecl {
                                    id,
                                    kind: ValueCellKind::Param,
                                    visibility: Visibility::Public,
                                    cell_type,
                                    default_expr,
                                    span: param.span,
                                }
                            };
                            port_members.push(decl);
                        }
                        reify_syntax::MemberDecl::Let(let_decl) => {
                            let composite_name = format!("{}.{}", port_decl.name, let_decl.name);
                            let compiled_expr = compile_expr(
                                &let_decl.value,
                                &scope,
                                enum_defs,
                                functions,
                                diagnostics,
                            );
                            let cell_type = compiled_expr.result_type.clone();
                            let id = ValueCellId::new(entity_name, &composite_name);

                            scope
                                .names
                                .insert(composite_name, (id.clone(), cell_type.clone(), None));

                            let visibility = if let_decl.is_pub {
                                Visibility::Public
                            } else {
                                Visibility::Private
                            };

                            port_members.push(ValueCellDecl {
                                id,
                                kind: ValueCellKind::Let,
                                visibility,
                                cell_type,
                                default_expr: Some(compiled_expr),
                                span: let_decl.span,
                            });
                        }
                        reify_syntax::MemberDecl::Constraint(constraint) => {
                            let compiled_expr = compile_expr(
                                &constraint.expr,
                                &scope,
                                enum_defs,
                                functions,
                                diagnostics,
                            );
                            let id = ConstraintNodeId::new(entity_name, constraint_index);
                            port_constraints.push(CompiledConstraint {
                                id,
                                label: constraint.label.clone(),
                                expr: compiled_expr,
                                span: constraint.span,
                                domain: None,
                            });
                            constraint_index += 1;
                        }
                        _ => {}
                    }
                }

                let frame_expr = port_decl
                    .frame_expr
                    .as_ref()
                    .map(|expr| compile_expr(expr, &scope, enum_defs, functions, diagnostics));

                ports.push(CompiledPort {
                    name: port_decl.name.clone(),
                    direction,
                    type_name: port_decl.type_name.clone(),
                    members: port_members,
                    constraints: port_constraints,
                    frame_expr,
                });
            }
            reify_syntax::MemberDecl::Connect(connect_decl) => {
                let ctx = ConnectContext {
                    entity_name,
                    ports: &ports,
                    scope: &scope,
                    enum_defs,
                    functions,
                };
                let mut acc = ConnectAccumulator {
                    constraints: &mut constraints,
                    constraint_index: &mut constraint_index,
                    connections: &mut connections,
                    sub_components: &mut sub_components,
                    connector_index: &mut connector_index,
                };
                compile_connection(
                    &ctx,
                    &ConnectInput {
                        left_expr: &connect_decl.left.expr,
                        operator: connect_decl.operator,
                        right_expr: &connect_decl.right.expr,
                        connector_type: connect_decl.connector_type.as_deref(),
                        params: &connect_decl.params,
                        port_mappings: &connect_decl.port_mappings,
                        span: connect_decl.span,
                    },
                    diagnostics,
                    &mut acc,
                );
            }
            reify_syntax::MemberDecl::Chain(chain_decl) => {
                if chain_decl.elements.len() < 2 {
                    diagnostics.push(
                        Diagnostic::error("chain statement requires at least two elements")
                            .with_label(DiagnosticLabel::new(chain_decl.span, "too few elements")),
                    );
                }
                let ctx = ConnectContext {
                    entity_name,
                    ports: &ports,
                    scope: &scope,
                    enum_defs,
                    functions,
                };
                // Desugar chain into pairwise Forward connections
                for pair in chain_decl.elements.windows(2) {
                    let mut acc = ConnectAccumulator {
                        constraints: &mut constraints,
                        constraint_index: &mut constraint_index,
                        connections: &mut connections,
                        sub_components: &mut sub_components,
                        connector_index: &mut connector_index,
                    };
                    compile_connection(
                        &ctx,
                        &ConnectInput {
                            left_expr: &pair[0],
                            operator: reify_syntax::ConnectOp::Forward,
                            right_expr: &pair[1],
                            connector_type: None,
                            params: &[],
                            port_mappings: &[],
                            span: chain_decl.span,
                        },
                        diagnostics,
                        &mut acc,
                    );
                }
            }
            reify_syntax::MemberDecl::MetaBlock(_) => {
                // Meta blocks are collected in the first pass; skip in second pass.
            }
            reify_syntax::MemberDecl::ConstraintInst(ci) => {
                // Look up the constraint definition.
                let def = match constraint_def_registry.get(&ci.name) {
                    Some(d) => *d,
                    None => {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "unknown constraint definition: {}",
                                ci.name
                            ))
                            .with_label(DiagnosticLabel::new(
                                ci.span,
                                format!("no constraint def named '{}'", ci.name),
                            )),
                        );
                        continue;
                    }
                };

                // Build name → Expr bindings map from the named args.
                let arg_map: HashMap<String, reify_syntax::Expr> = ci
                    .args
                    .iter()
                    .map(|(name, expr)| (name.clone(), expr.clone()))
                    .collect();

                // Validate: check for unknown argument names.
                let param_names: std::collections::HashSet<&str> =
                    def.params.iter().map(|p| p.name.as_str()).collect();
                for (arg_name, _) in &ci.args {
                    if !param_names.contains(arg_name.as_str()) {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "unknown argument '{}' in constraint instantiation of '{}'",
                                arg_name, ci.name
                            ))
                            .with_label(DiagnosticLabel::new(
                                ci.span,
                                format!("'{}' is not a parameter of '{}'", arg_name, ci.name),
                            )),
                        );
                    }
                }

                // Validate: check for missing required arguments.
                let mut has_validation_error = false;
                for param in &def.params {
                    if !arg_map.contains_key(&param.name) && param.default.is_none() {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "missing argument '{}' in constraint instantiation of '{}'",
                                param.name, ci.name
                            ))
                            .with_label(DiagnosticLabel::new(
                                ci.span,
                                format!("argument '{}' is required", param.name),
                            )),
                        );
                        has_validation_error = true;
                    }
                }
                if has_validation_error {
                    continue;
                }

                // For each predicate in the constraint def, substitute params with args
                // and compile the resulting expression in the calling entity's scope.
                for (pred_idx, predicate) in def.predicates.iter().enumerate() {
                    let substituted = substitute_expr(predicate, &arg_map);
                    let compiled_expr =
                        compile_expr(&substituted, &scope, enum_defs, functions, diagnostics);

                    let id = ConstraintNodeId::new(entity_name, constraint_index);
                    let cc = CompiledConstraint {
                        id,
                        label: Some(format!("{}[{}]", ci.name, pred_idx)),
                        expr: compiled_expr,
                        span: ci.span,
                        domain: None,
                    };
                    constraint_index += 1;

                    if let Some(wc) = &ci.where_clause {
                        compile_per_decl_constraint_guard(
                            entity_name,
                            wc,
                            cc,
                            &mut scope,
                            enum_defs,
                            functions,
                            diagnostics,
                            &mut guarded_groups,
                            &mut structure_controlling,
                            &mut guard_index,
                        );
                    } else {
                        constraints.push(cc);
                    }
                }
            }
        }
    }

    // Third pass: compile geometry let bindings into realizations.
    let mut realizations = Vec::new();
    let mut realization_index: u32 = 0;

    for member in structure.members {
        if let reify_syntax::MemberDecl::Let(let_decl) = member
            && is_geometry_let(&let_decl.value, functions)
            && let Some(ops) = compile_geometry_call(
                &let_decl.value,
                &scope,
                enum_defs,
                functions,
                diagnostics,
                0,
            )
        {
            realizations.push(RealizationDecl {
                id: RealizationNodeId::new(entity_name, realization_index),
                operations: ops,
                span: SourceSpan::new(0, 0),
            });
            realization_index += 1;
        }
    }

    // Build a content-sensitive hash by combining the name with all compiled content.
    let content_hash = {
        let name_hash = ContentHash::of_str(entity_name);

        // Value cell default expression hashes (sentinel ContentHash(0) for None)
        let vc_hashes = value_cells.iter().map(|vc| {
            vc.default_expr
                .as_ref()
                .map(|e| e.content_hash)
                .unwrap_or(ContentHash(0))
        });

        // Constraint expression hashes
        let constraint_hashes = constraints.iter().map(|c| c.expr.content_hash);

        // Sub-component content hashes
        let sub_hashes = sub_components.iter().map(|s| s.content_hash);

        // Guarded group hashes: include guard_expr + all member/constraint/else content
        let guard_hashes = guarded_groups.iter().flat_map(|g| {
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

        // Port member hashes (including identity fields for incremental invalidation)
        let port_hashes = ports.iter().flat_map(|p| {
            // Port identity fields: name, direction, type_name
            std::iter::once(ContentHash::of_str(&p.name))
                .chain(std::iter::once(ContentHash::of(&[p.direction as u8])))
                .chain(std::iter::once(ContentHash::of_str(&p.type_name)))
                // Port member default_expr hashes
                .chain(p.members.iter().map(|m| {
                    m.default_expr
                        .as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or(ContentHash(0))
                }))
                .chain(p.constraints.iter().map(|c| c.expr.content_hash))
                // Frame expression hash
                .chain(std::iter::once(
                    p.frame_expr
                        .as_ref()
                        .map(|e| e.content_hash)
                        .unwrap_or(ContentHash(0)),
                ))
        });

        // Connection identity hashes: left_port, operator, right_port, port_mappings, connector_sub
        let connection_hashes = connections.iter().flat_map(|c| {
            std::iter::once(ContentHash::of_str(&c.left_port))
                .chain(std::iter::once(ContentHash::of(&[c.operator.as_u8()])))
                .chain(std::iter::once(ContentHash::of_str(&c.right_port)))
                .chain(
                    c.port_mappings
                        .iter()
                        .flat_map(|(l, r)| [ContentHash::of_str(l), ContentHash::of_str(r)]),
                )
                .chain(std::iter::once(
                    c.connector_sub
                        .as_ref()
                        .map(|s| ContentHash::of_str(s))
                        .unwrap_or(ContentHash(0)),
                ))
        });

        let all_hashes = std::iter::once(name_hash)
            .chain(vc_hashes)
            .chain(constraint_hashes)
            .chain(sub_hashes)
            .chain(guard_hashes)
            .chain(port_hashes)
            .chain(connection_hashes);

        ContentHash::combine_all(all_hashes)
    };

    let visibility = if structure.is_pub {
        Visibility::Public
    } else {
        Visibility::Private
    };

    // Reference safety: detect unguarded references to guarded members.
    {
        let mut guarded_cell_map: HashMap<ValueCellId, ValueCellId> = HashMap::new();
        for group in &guarded_groups {
            for m in &group.members {
                guarded_cell_map.insert(m.id.clone(), group.guard_value_cell.clone());
            }
            for m in &group.else_members {
                guarded_cell_map.insert(m.id.clone(), group.guard_value_cell.clone());
            }
        }

        // Build parent_guard chain for nested guard ancestor checking.
        // Maps guard_value_cell -> parent_guard (None for top-level guards).
        let guard_parent_map: HashMap<ValueCellId, Option<ValueCellId>> = guarded_groups
            .iter()
            .map(|g| (g.guard_value_cell.clone(), g.parent_guard.clone()))
            .collect();

        // Check if ref_guard is an ancestor of current_guard in the parent chain.
        // Returns true if ref_guard == current_guard OR if ref_guard appears
        // in the ancestor chain of current_guard (via parent_guard links).
        let is_ancestor_guard = |ref_guard: &ValueCellId, current_guard: &ValueCellId| -> bool {
            if ref_guard == current_guard {
                return true;
            }
            let mut cursor = guard_parent_map.get(current_guard).and_then(|p| p.as_ref());
            while let Some(ancestor) = cursor {
                if ancestor == ref_guard {
                    return true;
                }
                cursor = guard_parent_map.get(ancestor).and_then(|p| p.as_ref());
            }
            false
        };

        for vc in &value_cells {
            if let Some(expr) = &vc.default_expr {
                for ref_id in expr.collect_value_refs() {
                    if guarded_cell_map.contains_key(&ref_id) {
                        diagnostics.push(
                            Diagnostic::warning(format!(
                                "unguarded reference to guarded cell '{}'",
                                ref_id.member,
                            ))
                            .with_label(DiagnosticLabel::new(
                                vc.span,
                                "references a conditionally-active member",
                            )),
                        );
                    }
                }
            }
        }
        for c in &constraints {
            for ref_id in c.expr.collect_value_refs() {
                if guarded_cell_map.contains_key(&ref_id) {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "unguarded reference to guarded cell '{}'",
                            ref_id.member,
                        ))
                        .with_label(DiagnosticLabel::new(
                            c.span,
                            "constraint references a conditionally-active member",
                        )),
                    );
                }
            }
        }
        for group in &guarded_groups {
            for m in &group.members {
                if let Some(expr) = &m.default_expr {
                    for ref_id in expr.collect_value_refs() {
                        if let Some(ref_guard) = guarded_cell_map.get(&ref_id)
                            && !is_ancestor_guard(ref_guard, &group.guard_value_cell)
                        {
                            diagnostics.push(
                                Diagnostic::warning(format!(
                                    "reference to differently-guarded cell '{}'",
                                    ref_id.member,
                                ))
                                .with_label(DiagnosticLabel::new(
                                    m.span,
                                    "referenced member under a different guard",
                                )),
                            );
                        }
                    }
                }
            }
            for m in &group.else_members {
                if let Some(expr) = &m.default_expr {
                    for ref_id in expr.collect_value_refs() {
                        if let Some(ref_guard) = guarded_cell_map.get(&ref_id)
                            && !is_ancestor_guard(ref_guard, &group.guard_value_cell)
                        {
                            diagnostics.push(
                                Diagnostic::warning(format!(
                                    "reference to differently-guarded cell '{}'",
                                    ref_id.member,
                                ))
                                .with_label(DiagnosticLabel::new(
                                    m.span,
                                    "referenced member under a different guard",
                                )),
                            );
                        }
                    }
                }
            }
            for c in &group.constraints {
                for ref_id in c.expr.collect_value_refs() {
                    if let Some(ref_guard) = guarded_cell_map.get(&ref_id)
                        && !is_ancestor_guard(ref_guard, &group.guard_value_cell)
                    {
                        diagnostics.push(
                            Diagnostic::warning(format!(
                                "reference to differently-guarded cell '{}'",
                                ref_id.member,
                            ))
                            .with_label(DiagnosticLabel::new(
                                c.span,
                                "constraint references member under a different guard",
                            )),
                        );
                    }
                }
            }
            for c in &group.else_constraints {
                for ref_id in c.expr.collect_value_refs() {
                    if let Some(ref_guard) = guarded_cell_map.get(&ref_id)
                        && !is_ancestor_guard(ref_guard, &group.guard_value_cell)
                    {
                        diagnostics.push(
                            Diagnostic::warning(format!(
                                "reference to differently-guarded cell '{}'",
                                ref_id.member,
                            ))
                            .with_label(DiagnosticLabel::new(
                                c.span,
                                "constraint references member under a different guard",
                            )),
                        );
                    }
                }
            }
        }
    }

    // Reconciliation sweep: backfill count_cell for collection sub-components
    // whose count constraint was processed before the sub declaration.
    // Match __count_{name} cells in value_cells to sub_components where count_cell is None.
    for vc in &value_cells {
        if let Some(coll_name) = vc.id.member.strip_prefix("__count_")
            && let Some(sub) = sub_components
                .iter_mut()
                .find(|s| s.name == coll_name && s.count_cell.is_none())
        {
            sub.count_cell = Some(vc.id.clone());
        }
    }

    // Convert parsed type parameters to compiled TypeParam structs
    let type_params = convert_type_params(structure.type_params);

    let trait_bounds: Vec<String> = structure
        .trait_bounds
        .iter()
        .map(|tb| tb.name.clone())
        .collect();

    // Port direction validation for occurrences: warn if missing in/out ports.
    if entity_kind == EntityKind::Occurrence {
        let has_in = ports
            .iter()
            .any(|p| p.direction == reify_types::PortDirection::In);
        let has_out = ports
            .iter()
            .any(|p| p.direction == reify_types::PortDirection::Out);
        if !has_in {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "occurrence '{}' has no input port; occurrences typically consume input structures",
                    entity_name
                ))
                .with_label(DiagnosticLabel::new(structure.span, "occurrence defined here")),
            );
        }
        if !has_out {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "occurrence '{}' has no output port; occurrences typically produce output structures",
                    entity_name
                ))
                .with_label(DiagnosticLabel::new(structure.span, "occurrence defined here")),
            );
        }
    }

    let context = match entity_kind {
        EntityKind::Structure => "structure",
        EntityKind::Occurrence => "occurrence",
    };
    let annotations = lower_annotations(structure.annotations, diagnostics);
    validate_annotations(&annotations, context, diagnostics);
    validate_pragmas(structure.pragmas, context, diagnostics);
    let is_test = annotations.iter().any(|a| a.name == "test");

    TopologyTemplate {
        name: entity_name.to_string(),
        entity_kind,
        visibility,
        type_params,
        trait_bounds,
        value_cells,
        constraints,
        realizations,
        sub_components,
        ports,
        connections,
        guarded_groups,
        structure_controlling,
        objective,
        meta: scope.meta_entries.clone(),
        content_hash,
        is_recursive: false,
        is_test,
        annotations,
        pragmas: structure.pragmas.to_vec(),
    }
}

/// A deferred bound check to be executed after all structures are compiled.
/// This ensures forward references are resolved correctly.
///
/// Two distinct paths produce pending bound checks:
/// - **SubComponent**: a `sub x = Foo<Bar>()` instantiation where type_params
///   are not yet known (resolved from the template registry in the post-pass).
/// - **TraitConformance**: a `structure def X : Trait<Arg>` declaration where
///   type_params are already known from the compiled trait definition.
pub(crate) enum PendingBoundCheck {
    /// Deferred check for a sub-component instantiation of a generic structure.
    /// The type_params are resolved from the template registry during the
    /// post-compilation pass, since the target structure may not yet be compiled.
    SubComponent {
        type_args: Vec<Type>,
        target_name: String,
        span: SourceSpan,
    },
    /// Deferred check for trait conformance with type arguments.
    /// The type_params are known at construction time from the compiled trait.
    TraitConformance {
        type_params: Vec<reify_types::TypeParam>,
        type_args: Vec<Type>,
        target_name: String,
        span: SourceSpan,
    },
}

/// Check that type arguments satisfy the bounds on type parameters.
///
/// For each type param with bounds, verify that the corresponding type arg
/// declares conformance to all required traits. Forwarded type params
/// (Type::TypeParam) are skipped — their bounds are enforced at the concrete
/// instantiation site.
/// When type_args are fewer than type_params, fill in defaults from TypeParam.default.
/// If a type_param has no default and no arg is provided, emit an error.
/// If type_args exceed type_params, emit an arity error.
pub(crate) fn check_type_param_bounds(
    type_params: &[reify_types::TypeParam],
    type_args: &[Type],
    target_structure_name: &str,
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    diagnostics: &mut Vec<Diagnostic>,
    span: SourceSpan,
) {
    // Check arity: too many type args
    if type_args.len() > type_params.len() {
        diagnostics.push(
            Diagnostic::error(format!(
                "too many type arguments for '{}': expected {}, got {}",
                target_structure_name,
                type_params.len(),
                type_args.len()
            ))
            .with_label(DiagnosticLabel::new(
                span,
                format!(
                    "'{}' declares {} type parameter(s)",
                    target_structure_name,
                    type_params.len()
                ),
            )),
        );
    }

    for (i, tp) in type_params.iter().enumerate() {
        let effective_arg: &Type = if let Some(arg) = type_args.get(i) {
            arg
        } else if let Some(ref default_type) = tp.default {
            default_type
        } else {
            diagnostics.push(
                Diagnostic::error(format!(
                    "missing type argument for type parameter '{}' of '{}' (no default provided)",
                    tp.name, target_structure_name
                ))
                .with_label(DiagnosticLabel::new(
                    span,
                    format!(
                        "'{}' requires a type argument for '{}'",
                        target_structure_name, tp.name
                    ),
                )),
            );
            continue;
        };

        // Skip bound checking for forwarded type params — bounds are
        // enforced at the concrete instantiation site.
        if matches!(effective_arg, Type::TypeParam(_)) {
            continue;
        }

        let arg_name = match effective_arg.as_name() {
            Some(name) => name,
            None => continue, // builtin types don't need bound checking
        };

        let arg_template = template_registry.get(arg_name);

        for bound in &tp.bounds {
            let bound_name = &bound.trait_ref.name;
            let satisfies = if let Some(tmpl) = arg_template {
                satisfies_trait_bound(&tmpl.trait_bounds, bound_name, trait_registry)
            } else {
                false
            };

            if !satisfies {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "type argument '{}' does not satisfy bound '{}' on type parameter '{}' of '{}'",
                        arg_name, bound_name, tp.name, target_structure_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        span,
                        format!("'{}' does not implement '{}'", arg_name, bound_name),
                    )),
                );
            }
        }
    }
}

/// Check whether a structure's declared trait bounds satisfy a required trait,
/// walking refinement chains transitively.
///
/// Returns true if any of the `structure_trait_bounds` equals `required_trait`
/// or refines it (directly or transitively) through the `trait_registry`.
pub(crate) fn satisfies_trait_bound(
    structure_trait_bounds: &[String],
    required_trait: &str,
    trait_registry: &HashMap<String, &CompiledTrait>,
) -> bool {
    for bound in structure_trait_bounds {
        let mut visited = HashSet::new();
        if trait_satisfies(bound, required_trait, trait_registry, &mut visited) {
            return true;
        }
    }
    false
}

/// Recursively check if `trait_name` equals or refines `required_trait`.
pub(crate) fn trait_satisfies(
    trait_name: &str,
    required_trait: &str,
    trait_registry: &HashMap<String, &CompiledTrait>,
    visited: &mut HashSet<String>,
) -> bool {
    if trait_name == required_trait {
        return true;
    }
    if !visited.insert(trait_name.to_string()) {
        return false; // cycle detected
    }
    if let Some(compiled_trait) = trait_registry.get(trait_name) {
        for refinement in &compiled_trait.refinements {
            if trait_satisfies(refinement, required_trait, trait_registry, visited) {
                return true;
            }
        }
    }
    false
}

