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
        // Register as a purpose param so expr.rs can look up the param root for the
        // per-param `{purpose}::{param}` entity stamp (task-2181 β, contract C1).
        scope.register_purpose_param(&param.name);
        // Deprecation check: warn if the referenced entity kind is @deprecated.
        if let Some(template) = template_registry.get(&param.entity_kind)
            && let Some(msg) = deprecation_message(&template.annotations)
        {
            let kind_label: &'static str = template.entity_kind.as_label();
            emit_deprecation_warning(kind_label, &param.entity_kind, msg, param.span, diagnostics);
        }
    }

    // Multi-StructureRef purpose params now compile under the per-param
    // `purpose::param` stamp scheme (task-2181 β, PRD §4.1 contract C1).
    // Activation-time remap is single-bound-entity here (one entity_ref applied
    // to every param stamp); task γ adds `activate_purpose_with_bindings` for
    // per-param binding maps.

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
            reify_ast::MemberDecl::Fn(_) => {
                // Associated fn compilation deferred to task δ/ζ.
                // Trait fns are not valid in purpose bodies (grammar-enforced),
                // so this arm is unreachable in practice at γ.
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

#[cfg(test)]
mod tests {
    //! Unit tests for `compile_trait` associated-function handling (task 3939 δ).
    //!
    //! These pin the producer contract for trait associated functions:
    //!   * a bodyless `fn req(self) -> Real` (FnDef.body = None) compiles to a
    //!     `RequirementKind::Fn(sig)` requirement, and
    //!   * a default-providing `fn area(self) -> Real { 3.14 }` (body = Some)
    //!     compiles to a `DefaultKind::Fn(fn_def)` default.
    //!
    //! `compile_trait` is `pub(crate)`, so these tests must live in-crate.
    use super::*;

    fn span() -> reify_core::SourceSpan {
        reify_core::SourceSpan::new(0, 0)
    }

    fn named_type(name: &str) -> reify_ast::TypeExpr {
        reify_ast::TypeExpr {
            kind: reify_ast::TypeExprKind::Named {
                name: name.to_string(),
                type_args: vec![],
            },
            span: span(),
        }
    }

    /// The implicit `self` receiver param: `is_self == true` with the sentinel
    /// `self` named type (per decl.rs:818-823).
    fn self_param() -> reify_ast::FnParam {
        reify_ast::FnParam {
            name: "self".to_string(),
            is_self: true,
            type_expr: named_type("self"),
            default: None,
            span: span(),
        }
    }

    /// Build an `FnDef` member, with `body` controlling required (None) vs
    /// default-providing (Some).
    fn fn_def(
        name: &str,
        params: Vec<reify_ast::FnParam>,
        return_type: Option<reify_ast::TypeExpr>,
        body: Option<reify_ast::FnBody>,
    ) -> reify_ast::FnDef {
        reify_ast::FnDef {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            params,
            return_type,
            body,
            span: span(),
            content_hash: reify_core::ContentHash::of_str(name),
            annotations: vec![],
        }
    }

    /// Wrap members in a `TraitDecl` named `"T"`.
    fn trait_decl(members: Vec<reify_ast::MemberDecl>) -> reify_ast::TraitDecl {
        reify_ast::TraitDecl {
            name: "T".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            refinements: vec![],
            members,
            span: span(),
            content_hash: reify_core::ContentHash::of_str("T"),
            pragmas: vec![],
            annotations: vec![],
        }
    }

    /// Run `compile_trait` with empty enum/alias/name registries.
    fn run(decl: &reify_ast::TraitDecl) -> (CompiledTrait, Vec<Diagnostic>) {
        let enums: Vec<reify_ir::EnumDef> = vec![];
        let alias_registry = TypeAliasRegistry::new();
        let structure_names = HashSet::new();
        let trait_names = HashSet::new();
        let mut diagnostics = Vec::new();
        let compiled = compile_trait(
            decl,
            &enums,
            &alias_registry,
            &structure_names,
            &trait_names,
            &mut diagnostics,
        );
        (compiled, diagnostics)
    }

    // (a) Bodyless `fn req(self) -> Real` → RequirementKind::Fn(sig).
    #[test]
    fn bodyless_assoc_fn_becomes_required_fn() {
        let decl = trait_decl(vec![reify_ast::MemberDecl::Fn(fn_def(
            "req",
            vec![self_param()],
            Some(named_type("Real")),
            None, // bodyless → required
        ))]);
        let (compiled, _diags) = run(&decl);

        let sig = compiled
            .required_members
            .iter()
            .find_map(|r| match &r.kind {
                RequirementKind::Fn(sig) => Some(sig.clone()),
                _ => None,
            })
            .expect("expected a RequirementKind::Fn requirement for the bodyless fn");

        assert_eq!(sig.name, "req");
        assert!(sig.has_self, "self receiver should set has_self = true");
        assert!(
            sig.params.is_empty(),
            "the self receiver must be excluded from params, got: {:?}",
            sig.params
        );
        assert_eq!(sig.return_type, Type::Real);
        // A required (bodyless) fn must NOT also appear as a default.
        assert!(
            !compiled
                .defaults
                .iter()
                .any(|d| matches!(d.kind, DefaultKind::Fn(_))),
            "a bodyless required fn must not produce a DefaultKind::Fn"
        );
    }

    // (b) `fn area(self) -> Real { 3.14 }` → DefaultKind::Fn(fn_def).
    #[test]
    fn assoc_fn_with_body_becomes_default_fn() {
        let body = reify_ast::FnBody {
            let_bindings: vec![],
            result_expr: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 3.14,
                    is_real: true,
                },
                span: span(),
            },
        };
        let decl = trait_decl(vec![reify_ast::MemberDecl::Fn(fn_def(
            "area",
            vec![self_param()],
            Some(named_type("Real")),
            Some(body), // has body → default-providing
        ))]);
        let (compiled, _diags) = run(&decl);

        let default_fn_def = compiled
            .defaults
            .iter()
            .find_map(|d| match &d.kind {
                DefaultKind::Fn(fd) => Some(fd.clone()),
                _ => None,
            })
            .expect("expected a DefaultKind::Fn default for the fn with a body");

        assert_eq!(default_fn_def.name, "area");
        // A default-providing fn must NOT also appear as a requirement.
        assert!(
            !compiled
                .required_members
                .iter()
                .any(|r| matches!(r.kind, RequirementKind::Fn(_))),
            "a default-providing fn must not produce a RequirementKind::Fn"
        );
    }
}
