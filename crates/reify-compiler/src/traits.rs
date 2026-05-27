use super::*;

/// Resolve a trait-member type annotation (`param x : T` or `let x : T = ...`).
///
/// Control flow:
///   1. Early-reject `DimensionalOp` with the historical "unexpected dimensional
///      expression" wording (the resolver silently returns None for it; pinned by
///      `type_expr_kind_dispatch_tests::dim_op_in_trait_param_emits_diagnostic`).
///   2. Early-reject `IntegerLiteral` — `resolve_type_expr_with_aliases` pushes its
///      own "integer literal `N` is only allowed as a type argument of Tensor or
///      Matrix" diagnostic and returns None; without an early skip we would emit a
///      second, less-useful "unknown type name" cascade.
///   3. Otherwise call `resolve_type_expr_with_aliases` (handles parameterized
///      builtins `Option<T>`/`List<T>`/`Set<T>`/`Map<K,V>`, parametric aliases,
///      structures, traits). On `None`, fall back to enum lookup, then the
///      "unresolved type in trait" diagnostic.
///
/// All error paths return `Type::Real` for downstream error-recovery so subsequent
/// trait machinery has a concrete type to work with.
#[allow(clippy::too_many_arguments)]
fn resolve_trait_member_type_annotation(
    type_expr: &reify_ast::TypeExpr,
    trait_decl: &reify_ast::TraitDecl,
    enum_defs: &[reify_ir::EnumDef],
    empty_params: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Type {
    match &type_expr.kind {
        reify_ast::TypeExprKind::DimensionalOp { .. } => {
            diagnostics.push(
                Diagnostic::error(format!(
                    "unresolved type in trait '{}': {}",
                    trait_decl.name, type_expr
                ))
                .with_code(DiagnosticCode::UnresolvedType)
                .with_label(DiagnosticLabel::new(
                    type_expr.span,
                    "unexpected dimensional expression",
                )),
            );
            return Type::Real;
        }
        reify_ast::TypeExprKind::IntegerLiteral(_) => {
            // Let the resolver emit its specific diagnostic by calling it once for
            // its side effect, then return Real without adding a cascade.
            let _ = resolve_type_expr_with_aliases(
                type_expr,
                empty_params,
                alias_registry,
                diagnostics,
                structure_names,
                trait_names,
            );
            return Type::Real;
        }
        _ => {}
    }
    match resolve_type_expr_with_aliases(
        type_expr,
        empty_params,
        alias_registry,
        diagnostics,
        structure_names,
        trait_names,
    ) {
        Some(t) => t,
        None => {
            if let reify_ast::TypeExprKind::Named { name, type_args } = &type_expr.kind
                && let Some(t) = resolve_enum_type(name, enum_defs)
            {
                if !type_args.is_empty() {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "enum `{}` does not accept type arguments",
                            name
                        ))
                        .with_label(DiagnosticLabel::new(
                            type_expr.span,
                            "enum types are not generic",
                        )),
                    );
                }
                t
            } else {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "unresolved type in trait '{}': {}",
                        trait_decl.name, type_expr
                    ))
                    .with_code(DiagnosticCode::UnresolvedType)
                    .with_label(DiagnosticLabel::new(type_expr.span, "unknown type name")),
                );
                Type::Real
            }
        }
    }
}

pub(crate) fn compile_trait(
    trait_decl: &reify_ast::TraitDecl,
    enum_defs: &[reify_ir::EnumDef],
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledTrait {
    let empty_params = HashSet::new();
    let mut required_members = Vec::new();
    let mut defaults = Vec::new();

    for member in &trait_decl.members {
        match member {
            reify_ast::MemberDecl::Param(param) => {
                let ty = if let Some(type_expr) = &param.type_expr {
                    resolve_trait_member_type_annotation(
                        type_expr,
                        trait_decl,
                        enum_defs,
                        &empty_params,
                        alias_registry,
                        structure_names,
                        trait_names,
                        diagnostics,
                    )
                } else {
                    Type::Real
                };

                if param.default.is_some() {
                    // Param with default → trait default
                    defaults.push(TraitDefault {
                        name: Some(param.name.clone()),
                        kind: DefaultKind::Param {
                            cell_type: ty,
                            default_decl: param.clone(),
                        },
                        span: param.span,
                    });
                } else {
                    // Param without default → requirement
                    required_members.push(TraitRequirement {
                        name: param.name.clone(),
                        kind: RequirementKind::Param(ty),
                        span: param.span,
                    });
                }
            }
            reify_ast::MemberDecl::Let(let_decl) => {
                // Let bindings always have a value expression → default.
                // Resolve the annotation type when present; None when absent.
                // Shares the trait Param resolver so let-typed annotations support the
                // same parameterized builtins (`Option<T>`, `List<T>`, etc.) and produce
                // identical diagnostics for unresolved / DimensionalOp / IntegerLiteral
                // type expressions.
                let cell_type = let_decl.type_expr.as_ref().map(|type_expr| {
                    resolve_trait_member_type_annotation(
                        type_expr,
                        trait_decl,
                        enum_defs,
                        &empty_params,
                        alias_registry,
                        structure_names,
                        trait_names,
                        diagnostics,
                    )
                });
                defaults.push(TraitDefault {
                    name: Some(let_decl.name.clone()),
                    kind: DefaultKind::Let {
                        cell_type,
                        let_decl: let_decl.clone(),
                    },
                    span: let_decl.span,
                });
            }
            reify_ast::MemberDecl::Constraint(constraint_decl) => {
                if let Some(label) = &constraint_decl.label {
                    // Labeled constraint with expression in trait → default
                    // (override detection uses label matching at injection site)
                    defaults.push(TraitDefault {
                        name: Some(label.clone()),
                        kind: DefaultKind::Constraint(constraint_decl.clone()),
                        span: constraint_decl.span,
                    });
                } else {
                    // Unlabeled constraint → always injected as default
                    defaults.push(TraitDefault {
                        name: None,
                        kind: DefaultKind::Constraint(constraint_decl.clone()),
                        span: constraint_decl.span,
                    });
                }
            }
            reify_ast::MemberDecl::Sub(sub_decl) => {
                required_members.push(TraitRequirement {
                    name: sub_decl.name.clone(),
                    kind: RequirementKind::Sub(sub_decl.structure_name.clone()),
                    span: sub_decl.span,
                });
            }
            _ => {
                // Minimize, Maximize, GuardedGroup, AssociatedType — skip for now
            }
        }
    }

    let content_hash = trait_decl.content_hash;

    // Convert parsed type parameters to compiled TypeParam structs
    let type_params = convert_type_params(&trait_decl.type_params);

    let annotations = lower_annotations(&trait_decl.annotations, diagnostics);
    validate_annotations(&annotations, "trait", diagnostics);
    validate_pragmas(&trait_decl.pragmas, "trait", diagnostics);

    CompiledTrait {
        name: trait_decl.name.clone(),
        is_pub: trait_decl.is_pub,
        doc: trait_decl.doc.clone(),
        type_params,
        refinements: trait_decl
            .refinements
            .iter()
            .map(|r| r.name.clone())
            .collect(),
        required_members,
        defaults,
        content_hash,
        annotations,
        pragmas: trait_decl.pragmas.clone(),
    }
}

/// Compile a parsed purpose declaration into a CompiledPurpose.
pub(crate) fn compile_purpose(
    purpose_def: &reify_ast::PurposeDef,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    template_registry: &HashMap<String, &TopologyTemplate>,
    unit_registry: &UnitRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledPurpose {
    let purpose_name = &purpose_def.name;

    // Create a compilation scope for the purpose body.
    // Purpose params are registered so their members can be referenced.
    let mut scope = CompilationScope::new(purpose_name);
    scope.set_unit_registry(unit_registry);
    scope.set_template_registry(template_registry);

    // Register purpose params as identifiers in scope.
    // Each param binds an entity reference (e.g., `subject : Structure`).
    // Use StructureRef so member access resolves correctly against the entity type.
    for param in &purpose_def.params {
        scope.register(&param.name, Type::StructureRef(param.entity_kind.clone()));
        // Deprecation check: warn if the referenced entity kind is @deprecated.
        if let Some(template) = template_registry.get(&param.entity_kind)
            && let Some(msg) = deprecation_message(&template.annotations)
        {
            let kind_label: &'static str = template.entity_kind.as_label();
            emit_deprecation_warning(kind_label, &param.entity_kind, msg, param.span, diagnostics);
        }
    }

    // task-2201: Reject multi-StructureRef-param purposes with a clear diagnostic.
    //
    // The ValueCellId stamping at expr.rs (~line 1222) uses `scope.entity_name`
    // (= purpose_name) for ALL purpose-subject member refs:
    //   `ValueCellId::new(&id.entity, member)` where `id.entity == purpose_name`.
    // This means `a.mass` and `b.mass` in a two-param purpose would both compile
    // to `ValueRef(check, mass)` — per-param identity is lost. The single-entity
    // `remap_entity` call in reify-types/src/expr.rs:660 has no way to distinguish
    // which param a given ref came from, so there is no safe forward path today.
    //
    // Approach 2 (encoding `format!("{}::{}", purpose_name, param_name)` and
    // redesigning `activate_purpose` to take a per-param mapping) is deferred
    // until a real multi-param use case appears. See esc-2181-18 S3 for design.
    //
    // NOTE: Do NOT return early here — the existing accumulate-and-continue
    // pattern (let-binding/guarded-block arms below) is preserved so that
    // `phase_purposes` (compile_builder/post_passes.rs:107-121) always receives
    // a `CompiledPurpose` entry for every `PurposeDef`.
    if purpose_def.params.len() > 1 {
        diagnostics.push(
            Diagnostic::error(format!(
                "multi-StructureRef purpose params not supported; binding scheme TBD: \
                 purpose '{}' has {} StructureRef params (task-2201)",
                purpose_name,
                purpose_def.params.len()
            ))
            .with_label(DiagnosticLabel::new(
                purpose_def.params[1].span,
                // "first extra" is self-explanatory for 3+ params too, where params[2..] are
                // unhighlighted — the message text already says "has N StructureRef params".
                "first extra StructureRef param".to_string(),
            )),
        );
    }

    let mut constraints = Vec::new();
    let mut constraint_index = 0u32;
    let mut objective = None;

    for member in &purpose_def.members {
        match member {
            reify_ast::MemberDecl::Constraint(constraint) => {
                let compiled_expr =
                    compile_expr(&constraint.expr, &scope, enum_defs, functions, diagnostics);
                let id = ConstraintNodeId::new(purpose_name, constraint_index);
                constraints.push(CompiledConstraint {
                    id,
                    label: constraint.label.clone(),
                    expr: compiled_expr,
                    span: constraint.span,
                    domain: None,
                    optimized_target: None,
                });
                constraint_index += 1;
            }
            reify_ast::MemberDecl::Minimize(min_decl) => {
                let compiled_expr =
                    compile_expr(&min_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective = Some(OptimizationObjective::Minimize(compiled_expr));
            }
            reify_ast::MemberDecl::Maximize(max_decl) => {
                let compiled_expr =
                    compile_expr(&max_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective = Some(OptimizationObjective::Maximize(compiled_expr));
            }
            reify_ast::MemberDecl::Let(let_decl) => {
                // Let bindings in purpose bodies are not yet supported:
                // CompiledPurpose has no storage for let expressions, and
                // activate_purpose only injects constraints. Any constraint
                // referencing a let-bound name would produce a ValueCellId
                // with no backing node in the eval graph.
                diagnostics.push(
                    Diagnostic::error(format!(
                        "let bindings in purpose bodies are not yet supported: '{}'",
                        let_decl.name
                    ))
                    .with_code(DiagnosticCode::PurposeLetUnsupported)
                    .with_label(DiagnosticLabel::new(
                        let_decl.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::GuardedGroup(g) => {
                diagnostics.push(
                    Diagnostic::error(
                        "guarded blocks in purpose bodies are not yet supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        g.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::Param(p) => {
                diagnostics.push(
                    Diagnostic::error(
                        "param declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        p.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::Sub(s) => {
                diagnostics.push(
                    Diagnostic::error(
                        "sub declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        s.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::Port(p) => {
                diagnostics.push(
                    Diagnostic::error(
                        "port declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        p.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::Connect(c) => {
                diagnostics.push(
                    Diagnostic::error(
                        "connect declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        c.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::Chain(c) => {
                diagnostics.push(
                    Diagnostic::error(
                        "chain declarations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        c.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::AssociatedType(a) => {
                diagnostics.push(
                    Diagnostic::error(
                        "associated type declarations in purpose bodies are not supported"
                            .to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        a.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::MetaBlock(m) => {
                diagnostics.push(
                    Diagnostic::error(
                        "meta blocks in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        m.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::ConstraintInst(ci) => {
                diagnostics.push(
                    Diagnostic::error(
                        "constraint instantiations in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        ci.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::ForallConnect(f) => {
                diagnostics.push(
                    Diagnostic::error(
                        "forall connect/chain statements in purpose bodies are not supported"
                            .to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        f.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::ForallConstraint(f) => {
                diagnostics.push(
                    Diagnostic::error(
                        "forall constraint statements in purpose bodies are not supported"
                            .to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        f.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
            reify_ast::MemberDecl::MatchArmDeclGroup(m) => {
                diagnostics.push(
                    Diagnostic::error(
                        "match-arm decl groups in purpose bodies are not supported".to_string(),
                    )
                    .with_label(DiagnosticLabel::new(
                        m.span,
                        "unsupported in purpose".to_string(),
                    )),
                );
            }
        }
    }

    let params: Vec<CompiledPurposeParam> = purpose_def
        .params
        .iter()
        .map(|p| CompiledPurposeParam {
            name: p.name.clone(),
            entity_kind: p.entity_kind.clone(),
        })
        .collect();

    // Resolve reflective schema queries for each purpose param.
    // Look up the bound entity's TopologyTemplate and extract relevant ValueCellIds.
    let mut resolved_queries = Vec::new();
    for param in &params {
        if let Some(template) = template_registry.get(&param.entity_kind) {
            // Resolve "params" query: all Param and Auto value cells
            let param_ids: Vec<ValueCellId> = template
                .value_cells
                .iter()
                .filter(|vc| matches!(vc.kind, ValueCellKind::Param | ValueCellKind::Auto { .. }))
                .map(|vc| vc.id.clone())
                .collect();
            if !param_ids.is_empty() {
                resolved_queries.push(ResolvedSchemaQuery {
                    param_name: param.name.clone(),
                    query_kind: "params".to_string(),
                    resolved_ids: param_ids,
                });
            }
        }
    }

    let annotations = lower_annotations(&purpose_def.annotations, diagnostics);
    validate_annotations(&annotations, "purpose", diagnostics);
    validate_pragmas(&purpose_def.pragmas, "purpose", diagnostics);

    CompiledPurpose {
        name: purpose_def.name.clone(),
        is_pub: purpose_def.is_pub,
        params,
        constraints,
        objective,
        resolved_queries,
        content_hash: purpose_def.content_hash,
        annotations,
        pragmas: purpose_def.pragmas.clone(),
    }
}
