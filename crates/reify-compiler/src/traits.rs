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

/// Derive the exact-match [`CompiledAssocFnSig`] for a trait associated function.
///
/// The leading `is_self` receiver (sentinel `self` named type, decl.rs:818-823)
/// is recorded as `has_self` and excluded from `params`; every other param's
/// `type_expr` and the `return_type` resolve through the same
/// [`resolve_trait_member_type_annotation`] path the rest of `compile_trait`
/// uses (so unresolved/DimensionalOp/IntegerLiteral annotations produce the
/// same diagnostics). A missing return type defaults to `Type::Real`, matching
/// `compile_function`'s convention. Added by task 3939 δ.
#[allow(clippy::too_many_arguments)]
fn assoc_fn_sig(
    fn_def: &reify_ast::FnDef,
    trait_decl: &reify_ast::TraitDecl,
    enum_defs: &[reify_ir::EnumDef],
    empty_params: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledAssocFnSig {
    let mut has_self = false;
    let mut params = Vec::new();
    for p in &fn_def.params {
        if p.is_self {
            // The self receiver's sentinel `self` type is not resolved here;
            // it is mapped to the concrete conformer type during conformance /
            // dispatch (tasks δ/ζ). Record self-ness and skip.
            has_self = true;
            continue;
        }
        let ty = resolve_trait_member_type_annotation(
            &p.type_expr,
            trait_decl,
            enum_defs,
            empty_params,
            alias_registry,
            structure_names,
            trait_names,
            diagnostics,
        );
        params.push(ty);
    }
    let return_type = match &fn_def.return_type {
        Some(te) => resolve_trait_member_type_annotation(
            te,
            trait_decl,
            enum_defs,
            empty_params,
            alias_registry,
            structure_names,
            trait_names,
            diagnostics,
        ),
        None => Type::Real,
    };
    CompiledAssocFnSig {
        name: fn_def.name.clone(),
        has_self,
        params,
        return_type,
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
            reify_ast::MemberDecl::Fn(fn_def) => {
                // Associated function (task 3939 δ). A bodyless fn is a required
                // member the conformer must provide; a fn with a body is a
                // default-providing member injected when not overridden.
                let sig = assoc_fn_sig(
                    fn_def,
                    trait_decl,
                    enum_defs,
                    &empty_params,
                    alias_registry,
                    structure_names,
                    trait_names,
                    diagnostics,
                );
                if fn_def.body.is_none() {
                    required_members.push(TraitRequirement {
                        name: fn_def.name.clone(),
                        kind: RequirementKind::Fn(sig),
                        span: fn_def.span,
                    });
                } else {
                    defaults.push(TraitDefault {
                        name: Some(fn_def.name.clone()),
                        kind: DefaultKind::Fn(fn_def.clone()),
                        span: fn_def.span,
                    });
                }
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
    let mut objective_terms: Vec<ObjectiveTerm> = Vec::new();
    let mut lets = Vec::new();

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
                objective_terms.push(ObjectiveTerm::new(ObjectiveSense::Minimize, compiled_expr));
            }
            reify_ast::MemberDecl::Maximize(max_decl) => {
                let compiled_expr =
                    compile_expr(&max_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective_terms.push(ObjectiveTerm::new(ObjectiveSense::Maximize, compiled_expr));
            }
            reify_ast::MemberDecl::Let(let_decl) => {
                // Compile the let expression in the current scope (purpose params
                // + earlier lets are visible). Any forward reference to a name not
                // yet registered produces an unknown-identifier diagnostic via the
                // normal scope.resolve path — no special-casing needed.
                // Mirrors entity-body let semantics (ordered, no forward refs).
                let expr = compile_expr(&let_decl.value, &scope, enum_defs, functions, diagnostics);
                let cell_id = ValueCellId::new(purpose_name.as_str(), let_decl.name.as_str());
                lets.push(CompiledPurposeLet {
                    name: let_decl.name.clone(),
                    cell_id,
                    expr: expr.clone(),
                    span: let_decl.span,
                });
                // Register AFTER compiling the expr so the let name is not visible
                // to its own initialiser (ordered semantics, no forward refs).
                scope.register(&let_decl.name, expr.result_type);
            }
            reify_ast::MemberDecl::GuardedGroup(g) => {
                // Task ε (4012): lower guarded blocks to implication constraints at
                // compile time.  The activated graph must be deterministic (purposes are
                // graph-level), so guards CANNOT branch graph shape at runtime.
                //
                // Lowering:
                //   where C { constraint A }  →  inject `C implies A`
                //   else { constraint B }     →  inject `(not C) implies B`
                //   where C { let x = … }     →  append let to CompiledPurpose.lets and
                //                                register the name in scope
                //
                // The condition is compiled once; cloned for each where-arm implication
                // and negated+cloned for each else-arm implication, ensuring per-param β
                // stamping is applied identically across all arms.
                let cond = compile_expr(
                    &g.condition,
                    &scope,
                    enum_defs,
                    functions,
                    diagnostics,
                );

                // Helper closure: emit an implication constraint from an already-compiled
                // body expression.
                let emit_implies =
                    |antecedent: CompiledExpr,
                     body: CompiledExpr,
                     label: Option<String>,
                     span: SourceSpan,
                     constraints: &mut Vec<CompiledConstraint>,
                     idx: &mut u32| {
                        let implied_expr =
                            CompiledExpr::binop(BinOp::Implies, antecedent, body, Type::Bool);
                        let id = ConstraintNodeId::new(purpose_name, *idx);
                        constraints.push(CompiledConstraint {
                            id,
                            label,
                            expr: implied_expr,
                            span,
                            domain: None,
                            optimized_target: None,
                        });
                        *idx += 1;
                    };

                // Process both where-arm (g.members) and else-arm (g.else_members) through
                // the same per-member logic.  The only difference between the two arms is
                // the constraint antecedent:
                //   where-arm: C as-is
                //   else-arm:  (not C)
                // Encoding this as a `(members, negate)` pair eliminates duplication of
                // the Let and unsupported arms across the two loops.
                let arms: [(&[reify_ast::MemberDecl], bool); 2] = [
                    (g.members.as_slice(), false),     // ── where-arm ──
                    (g.else_members.as_slice(), true),  // ── else-arm  ──
                ];
                for (members, negate) in &arms {
                    for m in *members {
                        match m {
                            reify_ast::MemberDecl::Constraint(c) => {
                                let antecedent = if *negate {
                                    CompiledExpr::unop(UnOp::Not, cond.clone(), Type::Bool)
                                } else {
                                    cond.clone()
                                };
                                let body = compile_expr(
                                    &c.expr,
                                    &scope,
                                    enum_defs,
                                    functions,
                                    diagnostics,
                                );
                                emit_implies(
                                    antecedent,
                                    body,
                                    c.label.clone(),
                                    c.span,
                                    &mut constraints,
                                    &mut constraint_index,
                                );
                            }
                            reify_ast::MemberDecl::Let(let_decl) => {
                                // Guard-scoped lets: mirror the top-level Let arm (traits.rs:399-416).
                                // Name is registered in scope after compiling so no forward refs are
                                // possible within the same block (ordered semantics).
                                //
                                // NOTE (design coherence): the let *value* is always evaluated
                                // unconditionally — it is appended to CompiledPurpose.lets and
                                // injected at activation time regardless of whether the guard
                                // condition C holds.  Constraints referencing the let rely on the
                                // `C implies (let_ref op ...)` wrapper for vacuity when C is false;
                                // the let cell is evaluated but its value is never directly
                                // asserted without the implication guard.  See test
                                // `guarded_let_undef_value_when_cond_false_no_spurious_violation`
                                // in purpose_activation.rs for the no-spurious-violation pin.
                                //
                                // NOTE (name scoping): the let name leaks past the guard block
                                // into the enclosing purpose scope because CompilationScope has no
                                // cheap unregister; this is an accepted v1 limitation (task ε
                                // design decisions).
                                //
                                // NOTE (duplicate cell_ids): a let with the same name in both the
                                // where-arm and else-arm (or shadowing a top-level let) produces
                                // two CompiledPurposeLet entries with the same cell_id.  The
                                // injection loop in engine_purposes.rs seeds the same ValueCellId
                                // twice; the second write wins (last-writer-wins in
                                // snapshot.values).  This is the accepted v1 behaviour; see
                                // `guarded_duplicate_let_name_in_arms_produces_two_lets_entries`
                                // in purpose_compile_tests.rs for the pinning test.
                                let expr = compile_expr(
                                    &let_decl.value,
                                    &scope,
                                    enum_defs,
                                    functions,
                                    diagnostics,
                                );
                                let cell_id = ValueCellId::new(
                                    purpose_name.as_str(),
                                    let_decl.name.as_str(),
                                );
                                lets.push(CompiledPurposeLet {
                                    name: let_decl.name.clone(),
                                    cell_id,
                                    expr: expr.clone(),
                                    span: let_decl.span,
                                });
                                scope.register(&let_decl.name, expr.result_type);
                            }
                            unsupported => {
                                // Any member kind that is not Constraint or Let is unsupported
                                // inside a purpose guarded block; emit the same pattern used by
                                // the sibling top-level reject arms.
                                let (msg, span) = unsupported_purpose_member_info(unsupported);
                                diagnostics.push(
                                    Diagnostic::error(msg)
                                        .with_label(DiagnosticLabel::new(
                                            span,
                                            "unsupported in purpose".to_string(),
                                        )),
                                );
                            }
                        }
                    }
                }
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
    // task-4137: emit "params" first (preserves [0]-positional readers), then
    // also emit "geometric_params" and "material_params" when non-empty.
    let mut resolved_queries = Vec::new();
    for param in &params {
        if let Some(template) = template_registry.get(&param.entity_kind) {
            let param_auto_cells: Vec<_> = template
                .value_cells
                .iter()
                .filter(|vc| matches!(vc.kind, ValueCellKind::Param | ValueCellKind::Auto { .. }))
                .collect();

            // Resolve "params" query: all Param and Auto value cells (pushed first,
            // preserving [0]-positional readers that pre-date task-4137).
            let param_ids: Vec<ValueCellId> = param_auto_cells
                .iter()
                .map(|vc| vc.id.clone())
                .collect();
            if !param_ids.is_empty() {
                resolved_queries.push(ResolvedSchemaQuery {
                    param_name: param.name.clone(),
                    query_kind: "params".to_string(),
                    resolved_ids: param_ids,
                });
            }

            // Resolve "geometric_params" query: Param/Auto cells with a nonzero
            // Length (slot 0) or Angle (slot 7) dimension exponent (task-4137).
            let geo_ids: Vec<ValueCellId> = param_auto_cells
                .iter()
                .filter(|vc| is_geometric_param_type(&vc.cell_type))
                .map(|vc| vc.id.clone())
                .collect();
            if !geo_ids.is_empty() {
                resolved_queries.push(ResolvedSchemaQuery {
                    param_name: param.name.clone(),
                    query_kind: "geometric_params".to_string(),
                    resolved_ids: geo_ids,
                });
            }

            // Resolve "material_params" query: Param/Auto cells typed as
            // StructureRef("Material") or TraitObject("Material") (task-4137).
            let mat_ids: Vec<ValueCellId> = param_auto_cells
                .iter()
                .filter(|vc| is_material_param_type(&vc.cell_type))
                .map(|vc| vc.id.clone())
                .collect();
            if !mat_ids.is_empty() {
                resolved_queries.push(ResolvedSchemaQuery {
                    param_name: param.name.clone(),
                    query_kind: "material_params".to_string(),
                    resolved_ids: mat_ids,
                });
            }
        }
    }

    let annotations = lower_annotations(&purpose_def.annotations, diagnostics);
    validate_annotations(&annotations, "purpose", diagnostics);
    validate_pragmas(&purpose_def.pragmas, "purpose", diagnostics);

    let objective = if objective_terms.is_empty() {
        None
    } else {
        Some(ObjectiveSet { terms: objective_terms, combination: ObjectiveCombination::WeightedSum })
    };

    CompiledPurpose {
        name: purpose_def.name.clone(),
        is_pub: purpose_def.is_pub,
        params,
        lets,
        constraints,
        objective,
        resolved_queries,
        content_hash: purpose_def.content_hash,
        annotations,
        pragmas: purpose_def.pragmas.clone(),
    }
}

/// Return `(diagnostic_message, span)` for an unsupported [`reify_ast::MemberDecl`] found
/// inside a purpose-body guarded block (task ε).  Mirrors the wording used by the top-level
/// reject arms in `compile_purpose` so all "not supported in purpose" errors look identical.
///
/// Panics (unreachable!) for `Constraint` and `Let` — those are the two supported kinds that
/// callers must have already handled before reaching this helper.
fn unsupported_purpose_member_info(m: &reify_ast::MemberDecl) -> (String, SourceSpan) {
    use reify_ast::MemberDecl;
    match m {
        MemberDecl::Param(p) => (
            "param declarations in purpose bodies are not supported".to_string(),
            p.span,
        ),
        MemberDecl::Sub(s) => (
            "sub declarations in purpose bodies are not supported".to_string(),
            s.span,
        ),
        MemberDecl::Port(p) => (
            "port declarations in purpose bodies are not supported".to_string(),
            p.span,
        ),
        MemberDecl::Connect(c) => (
            "connect declarations in purpose bodies are not supported".to_string(),
            c.span,
        ),
        MemberDecl::Chain(c) => (
            "chain declarations in purpose bodies are not supported".to_string(),
            c.span,
        ),
        MemberDecl::AssociatedType(a) => (
            "associated type declarations in purpose bodies are not supported".to_string(),
            a.span,
        ),
        MemberDecl::MetaBlock(mb) => (
            "meta blocks in purpose bodies are not supported".to_string(),
            mb.span,
        ),
        MemberDecl::ConstraintInst(ci) => (
            "constraint instantiations in purpose bodies are not supported".to_string(),
            ci.span,
        ),
        MemberDecl::ForallConnect(f) => (
            "forall connect/chain statements in purpose bodies are not supported".to_string(),
            f.span,
        ),
        MemberDecl::ForallConstraint(f) => (
            "forall constraint statements in purpose bodies are not supported".to_string(),
            f.span,
        ),
        MemberDecl::MatchArmDeclGroup(mg) => (
            "match-arm decl groups in purpose bodies are not supported".to_string(),
            mg.span,
        ),
        MemberDecl::Minimize(min) => (
            "minimize declarations in purpose bodies are not supported inside guarded blocks"
                .to_string(),
            min.span,
        ),
        MemberDecl::Maximize(max) => (
            "maximize declarations in purpose bodies are not supported inside guarded blocks"
                .to_string(),
            max.span,
        ),
        MemberDecl::GuardedGroup(g) => (
            "nested guarded blocks in purpose bodies are not supported".to_string(),
            g.span,
        ),
        MemberDecl::Fn(f) => (
            "fn declarations in purpose bodies are not supported".to_string(),
            f.span,
        ),
        MemberDecl::Constraint(_) | MemberDecl::Let(_) => {
            unreachable!(
                "unsupported_purpose_member_info called with a Constraint or Let — \
                 these are supported and must be handled by the caller"
            )
        }
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

    // (b) `fn area(self) -> Real { 3.5 }` → DefaultKind::Fn(fn_def).
    // (Body value is irrelevant to this test — it only asserts the member becomes
    // a default-providing fn. Kept off `3.14…` to avoid clippy::approx_constant.)
    #[test]
    fn assoc_fn_with_body_becomes_default_fn() {
        let body = reify_ast::FnBody {
            let_bindings: vec![],
            result_expr: reify_ast::Expr {
                kind: reify_ast::ExprKind::NumberLiteral {
                    value: 3.5,
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

    /// Run `compile_trait` with a custom `structure_names` set so that named
    /// type expressions resolve to `Type::StructureRef`.
    fn run_with_structures(
        decl: &reify_ast::TraitDecl,
        structure_names: HashSet<String>,
    ) -> (CompiledTrait, Vec<Diagnostic>) {
        let enums: Vec<reify_ir::EnumDef> = vec![];
        let alias_registry = TypeAliasRegistry::new();
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

    /// Build an `AssociatedTypeDecl` member.
    fn assoc_type_decl(
        name: &str,
        default_type: Option<reify_ast::TypeExpr>,
    ) -> reify_ast::AssociatedTypeDecl {
        reify_ast::AssociatedTypeDecl {
            name: name.to_string(),
            default_type,
            span: span(),
            content_hash: reify_core::ContentHash::of_str(name),
        }
    }

    // (c) Bodyless `type Material` → RequirementKind::AssocType(None).
    //     `type Color = Red` → DefaultKind::AssocType(Type::StructureRef("Red")).
    //
    // RED until step-2 wires the AssociatedType arm in compile_trait.
    // Currently the `_ =>` wildcard at traits.rs:295-297 silently skips
    // AssociatedType members, so neither is produced.
    #[test]
    fn assoc_type_without_default_becomes_requirement() {
        let decl = trait_decl(vec![
            // `type Material` — no default → required
            reify_ast::MemberDecl::AssociatedType(assoc_type_decl("Material", None)),
            // `type Color = Red` — default → DefaultKind::AssocType
            reify_ast::MemberDecl::AssociatedType(assoc_type_decl(
                "Color",
                Some(named_type("Red")),
            )),
        ]);

        let structure_names: HashSet<String> = std::iter::once("Red".to_string()).collect();
        let (compiled, diags) = run_with_structures(&decl, structure_names);

        // Zero diagnostics — both members should compile cleanly.
        assert!(
            diags.is_empty(),
            "expected no diagnostics, got: {:?}",
            diags
        );

        // `type Material` → exactly one AssocType requirement named "Material".
        let material_req = compiled
            .required_members
            .iter()
            .find(|r| r.name == "Material")
            .expect("expected a required_member named 'Material'");
        assert!(
            matches!(material_req.kind, RequirementKind::AssocType(None)),
            "expected RequirementKind::AssocType(None), got {:?}",
            material_req.kind
        );

        // `type Color = Red` → exactly one AssocType default named "Color" resolving to StructureRef("Red").
        let color_default = compiled
            .defaults
            .iter()
            .find(|d| d.name.as_deref() == Some("Color"))
            .expect("expected a default named 'Color'");
        assert!(
            matches!(&color_default.kind, DefaultKind::AssocType(ty) if *ty == Type::StructureRef("Red".to_string())),
            "expected DefaultKind::AssocType(Type::StructureRef(\"Red\")), got {:?}",
            color_default.kind
        );

        // `type Material` must NOT appear as a default.
        assert!(
            !compiled.defaults.iter().any(|d| d.name.as_deref() == Some("Material")),
            "a bodyless AssociatedType must not produce a default"
        );

        // `type Color = Red` must NOT appear as a required_member.
        assert!(
            !compiled.required_members.iter().any(|r| r.name == "Color"),
            "a default-providing AssociatedType must not produce a requirement"
        );
    }
}
