use super::*;

pub(crate) fn collect_body_refs(expr: &CompiledExpr) -> Vec<ValueCellId> {
    let mut refs = Vec::new();
    collect_body_refs_inner(expr, &mut refs);
    refs
}

pub(crate) fn collect_body_refs_inner(expr: &CompiledExpr, refs: &mut Vec<ValueCellId>) {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) => refs.push(id.clone()),
        CompiledExprKind::BinOp { left, right, .. } => {
            collect_body_refs_inner(left, refs);
            collect_body_refs_inner(right, refs);
        }
        CompiledExprKind::UnOp { operand, .. } => {
            collect_body_refs_inner(operand, refs);
        }
        CompiledExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_body_refs_inner(arg, refs);
            }
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_body_refs_inner(condition, refs);
            collect_body_refs_inner(then_branch, refs);
            collect_body_refs_inner(else_branch, refs);
        }
        CompiledExprKind::Match { discriminant, arms } => {
            collect_body_refs_inner(discriminant, refs);
            for arm in arms {
                collect_body_refs_inner(&arm.body, refs);
            }
        }
        CompiledExprKind::UserFunctionCall { args, .. } => {
            for arg in args {
                collect_body_refs_inner(arg, refs);
            }
        }
        CompiledExprKind::Lambda { body, .. } => {
            collect_body_refs_inner(body, refs);
        }
        CompiledExprKind::Quantifier {
            variable_id,
            collection,
            predicate,
            ..
        } => {
            collect_body_refs_inner(collection, refs);
            // Filter out the quantifier's bound variable from predicate refs,
            // mirroring collect_value_refs_inner in reify-types/src/expr.rs.
            let mut pred_refs = Vec::new();
            collect_body_refs_inner(predicate, &mut pred_refs);
            for r in pred_refs {
                if r != *variable_id {
                    refs.push(r);
                }
            }
        }
        CompiledExprKind::Literal(_) => {}
        CompiledExprKind::ListLiteral(elements) => {
            for elem in elements {
                collect_body_refs_inner(elem, refs);
            }
        }
        CompiledExprKind::ReflectiveCellList(elements) => {
            for elem in elements {
                collect_body_refs_inner(elem, refs);
            }
        }
        CompiledExprKind::SetLiteral(elements) => {
            for elem in elements {
                collect_body_refs_inner(elem, refs);
            }
        }
        CompiledExprKind::MapLiteral(entries) => {
            for (key, val) in entries {
                collect_body_refs_inner(key, refs);
                collect_body_refs_inner(val, refs);
            }
        }
        CompiledExprKind::IndexAccess { object, index } => {
            collect_body_refs_inner(object, refs);
            collect_body_refs_inner(index, refs);
        }
        CompiledExprKind::MethodCall { object, args, .. } => {
            collect_body_refs_inner(object, refs);
            for arg in args {
                collect_body_refs_inner(arg, refs);
            }
        }
        CompiledExprKind::OptionSome(inner) => {
            collect_body_refs_inner(inner, refs);
        }
        CompiledExprKind::OptionNone => {}
        CompiledExprKind::MetaAccess { .. } => {}
        CompiledExprKind::DeterminacyPredicate { cell, .. } => {
            refs.push(cell.clone());
        }
        CompiledExprKind::RangeConstructor { lower, upper, .. } => {
            if let Some(lo) = lower {
                collect_body_refs_inner(lo, refs);
            }
            if let Some(hi) = upper {
                collect_body_refs_inner(hi, refs);
            }
        }
        CompiledExprKind::AdHocSelector { base, args, .. } => {
            collect_body_refs_inner(base, refs);
            for arg in args {
                collect_body_refs_inner(arg, refs);
            }
        }
        // Reflective-aggregation placeholder (task-2289): carries no concrete
        // ValueCellId — activation expands it before any dependency-tracking
        // pass runs.
        CompiledExprKind::PurposeReflectiveAggregation { .. } => {}
    }
}

/// Register names from guarded group members in the compilation scope (pass 1).
/// Recursively handles nested guarded groups.
#[allow(clippy::too_many_arguments)]
pub(crate) fn register_guarded_names<'a>(
    members: &'a [reify_syntax::MemberDecl],
    scope: &mut CompilationScope,
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    known_geometry_lets: &mut HashSet<&'a str>,
) {
    for member in members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let ty = if let Some(type_expr) = &param.type_expr {
                    resolve_type_expr_with_aliases(
                        type_expr,
                        type_param_names,
                        alias_registry,
                        diagnostics,
                        structure_names,
                        trait_names,
                    )
                    .unwrap_or_else(|| {
                        diagnostics.push(
                            Diagnostic::error(format!("unresolved type: {}", type_expr))
                                .with_label(DiagnosticLabel::new(
                                    type_expr.span,
                                    "unknown type name",
                                )),
                        );
                        Type::Real
                    })
                } else {
                    Type::Real
                };
                // Solid-typed params with a geometry-call default are treated
                // symmetrically to geometry lets (mirrors entity.rs pre-pass, step-4).
                if is_solid_geometry_param(
                    &ty,
                    param.default.as_ref(),
                    functions,
                    known_geometry_lets,
                ) {
                    scope.has_geometry = true;
                    known_geometry_lets.insert(param.name.as_str());
                }
                scope.register(&param.name, ty);
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                if is_geometry_let(&let_decl.value, functions, known_geometry_lets) {
                    scope.register(&let_decl.name, Type::Geometry);
                    known_geometry_lets.insert(let_decl.name.as_str());
                } else {
                    scope.register(&let_decl.name, Type::Real);
                }
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                // `known_geometry_lets` is intentionally shared across both branches,
                // consistent with how `scope` is shared: names registered in the
                // if-branch are visible when processing the else-branch. As a result,
                // an Ident alias in the else-branch may be classified as a geometry let
                // if the aliased name appeared in the if-branch. Fixing this would
                // require snapshotting both `scope` and `known_geometry_lets` atomically
                // for each branch — a larger change that is deferred until needed.
                register_guarded_names(
                    &g.members,
                    scope,
                    functions,
                    diagnostics,
                    type_param_names,
                    alias_registry,
                    structure_names,
                    trait_names,
                    known_geometry_lets,
                );
                register_guarded_names(
                    &g.else_members,
                    scope,
                    functions,
                    diagnostics,
                    type_param_names,
                    alias_registry,
                    structure_names,
                    trait_names,
                    known_geometry_lets,
                );
            }
            _ => {}
        }
    }
}

/// Compile a block-level `where` guard into a CompiledGuardedGroup.
///
/// Creates a synthetic guard ValueCell and compiles all members within the block.
/// If `outer_guard` is Some, the guard expression becomes AND(outer_guard, inner_condition).
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_block_guard(
    entity_name: &str,
    g: &reify_syntax::GuardedGroupDecl,
    outer_guard: Option<&ValueCellId>,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
    constraint_index: &mut u32,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    known_geometry_lets: &HashSet<&str>,
) {
    let inner_condition = compile_expr(&g.condition, scope, enum_defs, functions, diagnostics);

    // If there's an outer guard, conjoin: guard = outer && inner
    let guard_expr = if let Some(outer_id) = outer_guard {
        let outer_ref = CompiledExpr::value_ref(outer_id.clone(), Type::Bool);
        CompiledExpr::binop(BinOp::And, outer_ref, inner_condition, Type::Bool)
    } else {
        inner_condition
    };

    let guard_cell_id = ValueCellId::new(entity_name, format!("__guard_{}", guard_index));
    *guard_index += 1;
    structure_controlling.insert(guard_cell_id.clone());

    let mut members = Vec::new();
    let mut group_constraints = Vec::new();

    // Compile main members
    compile_guarded_members(
        entity_name,
        &g.members,
        &guard_cell_id,
        scope,
        enum_defs,
        functions,
        diagnostics,
        &mut members,
        &mut group_constraints,
        guarded_groups,
        structure_controlling,
        guard_index,
        constraint_index,
        type_param_names,
        alias_registry,
        structure_names,
        trait_names,
        known_geometry_lets,
    );

    let mut else_members = Vec::new();
    let mut else_constraints = Vec::new();

    // Compile else members
    if !g.else_members.is_empty() {
        compile_guarded_members(
            entity_name,
            &g.else_members,
            &guard_cell_id,
            scope,
            enum_defs,
            functions,
            diagnostics,
            &mut else_members,
            &mut else_constraints,
            guarded_groups,
            structure_controlling,
            guard_index,
            constraint_index,
            type_param_names,
            alias_registry,
            structure_names,
            trait_names,
            known_geometry_lets,
        );
    }

    // Update scope to mark all members and else_members as guarded
    for m in &members {
        scope.register_guarded(&m.id.member, m.cell_type.clone(), guard_cell_id.clone());
    }
    for m in &else_members {
        scope.register_guarded(&m.id.member, m.cell_type.clone(), guard_cell_id.clone());
    }

    guarded_groups.push(CompiledGuardedGroup {
        guard_expr,
        guard_value_cell: guard_cell_id,
        members,
        constraints: group_constraints,
        else_members,
        else_constraints,
        parent_guard: outer_guard.cloned(),
    });
}

/// Compile members within a guarded block into ValueCellDecls and CompiledConstraints.
/// Handles nested GuardedGroupDecls recursively.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_guarded_members(
    entity_name: &str,
    ast_members: &[reify_syntax::MemberDecl],
    current_guard: &ValueCellId,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    members: &mut Vec<ValueCellDecl>,
    group_constraints: &mut Vec<CompiledConstraint>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
    constraint_index: &mut u32,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    known_geometry_lets: &HashSet<&str>,
) {
    let guard_ctx = Some(current_guard);
    for member in ast_members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let id = ValueCellId::new(entity_name, &param.name);
                let cell_type = scope
                    .resolve(&param.name)
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or_else(|| emit_ice_unresolved(UnresolvedKind::GuardedMember, &param.name, param.span, diagnostics));

                // Solid-typed params with a geometry-call default are lowered as
                // realizations (not scalar cells) — mirrors entity.rs main loop (step-6).
                if is_solid_geometry_param(
                    &cell_type,
                    param.default.as_ref(),
                    functions,
                    known_geometry_lets,
                ) {
                    continue;
                }

                let auto_free = param.default.as_ref().and_then(extract_auto_free);

                // Lower and validate annotations on this guarded param
                let lowered_annotations = lower_annotations(&param.annotations, diagnostics);
                validate_annotations(&lowered_annotations, "param", diagnostics);
                let solver_hints = extract_solver_hints(&lowered_annotations, diagnostics);
                validate_solver_hint_collections(&solver_hints, scope, functions, diagnostics);

                let decl = if let Some(free) = auto_free {
                    ValueCellDecl {
                        id,
                        kind: ValueCellKind::Auto { free },
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr: None,
                        solver_hints,
                        span: param.span,
                    }
                } else {
                    let default_expr = param.default.as_ref().map(|expr| {
                        let mut lc = 0u32;
                        let mut compiled = compile_expr_guarded(
                            expr,
                            scope,
                            enum_defs,
                            functions,
                            diagnostics,
                            guard_ctx,
                            &mut lc,
                        );
                        fixup_option_none_for_param(&mut compiled, &cell_type);
                        compiled
                    });
                    ValueCellDecl {
                        id,
                        kind: ValueCellKind::Param,
                        visibility: Visibility::Public,
                        cell_type,
                        default_expr,
                        solver_hints,
                        span: param.span,
                    }
                };
                members.push(decl);
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                if is_geometry_let(&let_decl.value, functions, known_geometry_lets) {
                    continue;
                }
                let mut compiled_expr = {
                    let mut lc = 0u32;
                    compile_expr_guarded(
                        &let_decl.value,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        guard_ctx,
                        &mut lc,
                    )
                };
                fixup_option_none_for_let(
                    &mut compiled_expr,
                    let_decl.type_expr.as_ref(),
                    type_param_names,
                    alias_registry,
                    structure_names,
                    trait_names,
                    diagnostics,
                );
                let cell_type = compiled_expr.result_type.clone();
                let id = ValueCellId::new(entity_name, &let_decl.name);

                let visibility = if let_decl.is_pub {
                    Visibility::Public
                } else {
                    Visibility::Private
                };

                // Lower and validate annotations on this guarded let
                let lowered_annotations = lower_annotations(&let_decl.annotations, diagnostics);
                validate_annotations(&lowered_annotations, "let", diagnostics);
                let solver_hints = extract_solver_hints(&lowered_annotations, diagnostics);
                validate_solver_hint_collections(&solver_hints, scope, functions, diagnostics);

                members.push(ValueCellDecl {
                    id,
                    kind: ValueCellKind::Let,
                    visibility,
                    cell_type,
                    default_expr: Some(compiled_expr),
                    solver_hints,
                    span: let_decl.span,
                });
            }
            reify_syntax::MemberDecl::Constraint(constraint) => {
                let compiled_expr = {
                    let mut lc = 0u32;
                    compile_expr_guarded(
                        &constraint.expr,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        guard_ctx,
                        &mut lc,
                    )
                };
                if compiled_expr.result_type != Type::Bool {
                    diagnostics.push(
                        Diagnostic::warning(format!(
                            "constraint expression has type {}, expected Bool",
                            compiled_expr.result_type,
                        ))
                        .with_label(DiagnosticLabel::new(constraint.expr.span, "expected Bool")),
                    );
                }
                let id = ConstraintNodeId::new(entity_name, *constraint_index);
                group_constraints.push(CompiledConstraint {
                    id,
                    label: constraint.label.clone(),
                    expr: compiled_expr,
                    span: constraint.span,
                    domain: None,
                    optimized_target: None,
                });
                *constraint_index += 1;
            }
            reify_syntax::MemberDecl::GuardedGroup(nested) => {
                // Nested guard: compile with current guard as outer
                compile_block_guard(
                    entity_name,
                    nested,
                    Some(current_guard),
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    guarded_groups,
                    structure_controlling,
                    guard_index,
                    constraint_index,
                    type_param_names,
                    alias_registry,
                    structure_names,
                    trait_names,
                    known_geometry_lets,
                );
            }
            reify_syntax::MemberDecl::Sub(s) => {
                diagnostics.push(
                    Diagnostic::error("sub declarations in guarded blocks are not yet supported")
                        .with_label(DiagnosticLabel::new(s.span, "not yet supported")),
                );
            }
            reify_syntax::MemberDecl::Minimize(m) => {
                diagnostics.push(
                    Diagnostic::error(
                        "minimize declarations in guarded blocks are not yet supported",
                    )
                    .with_label(DiagnosticLabel::new(m.span, "not yet supported")),
                );
            }
            reify_syntax::MemberDecl::Maximize(m) => {
                diagnostics.push(
                    Diagnostic::error(
                        "maximize declarations in guarded blocks are not yet supported",
                    )
                    .with_label(DiagnosticLabel::new(m.span, "not yet supported")),
                );
            }
            reify_syntax::MemberDecl::ForallConnect(f) => {
                diagnostics.push(
                    Diagnostic::error(
                        "forall connect/chain statements in guarded blocks are not yet supported",
                    )
                    .with_label(DiagnosticLabel::new(f.span, "not yet supported")),
                );
            }
            reify_syntax::MemberDecl::ForallConstraint(f) => {
                diagnostics.push(
                    Diagnostic::error(
                        "forall constraint statements in guarded blocks are not yet supported",
                    )
                    .with_label(DiagnosticLabel::new(f.span, "not yet supported")),
                );
            }
            reify_syntax::MemberDecl::Port(_)
            | reify_syntax::MemberDecl::Connect(_)
            | reify_syntax::MemberDecl::Chain(_)
            | reify_syntax::MemberDecl::AssociatedType(_)
            | reify_syntax::MemberDecl::MetaBlock(_)
            | reify_syntax::MemberDecl::ConstraintInst(_)
            // task 2372: match-arm decl group members inside a where{} guard are
            // handled in the parent compile_entity loop, not here.
            | reify_syntax::MemberDecl::MatchArmDeclGroup(_) => {
                // Not yet handled inside guarded blocks. Enumerated explicitly so
                // adding a new MemberDecl variant produces a `non-exhaustive match`
                // compile error here, forcing an intentional decision about how the
                // new variant behaves under a `where { }` guard. If a new variant
                // should be silently dropped inside a guard, add it to this arm; if
                // it should emit a diagnostic (like Sub/Minimize/Maximize above),
                // add a dedicated arm.
            }
        }
    }
}

/// Compile a per-declaration `where` clause into a single-member CompiledGuardedGroup.
///
/// Creates a synthetic guard ValueCell (Bool, Let kind) with the guard condition as
/// its default expression, and wraps the member in a CompiledGuardedGroup.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_per_decl_guard(
    entity_name: &str,
    wc: &reify_syntax::WhereClause,
    member_decl: ValueCellDecl,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
) {
    let guard_expr = compile_expr(&wc.condition, scope, enum_defs, functions, diagnostics);
    let guard_cell_id = ValueCellId::new(entity_name, format!("__guard_{}", guard_index));
    *guard_index += 1;

    // Update scope to mark this member as guarded (for reference safety checking)
    let member_name = member_decl.id.member.clone();
    let member_type = member_decl.cell_type.clone();

    structure_controlling.insert(guard_cell_id.clone());
    guarded_groups.push(CompiledGuardedGroup {
        guard_expr,
        guard_value_cell: guard_cell_id.clone(),
        members: vec![member_decl],
        constraints: vec![],
        else_members: vec![],
        else_constraints: vec![],
        parent_guard: None,
    });

    scope.register_guarded(&member_name, member_type, guard_cell_id);
}

/// Compile a per-declaration `where` clause for a constraint into a single-constraint
/// CompiledGuardedGroup.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_per_decl_constraint_guard(
    entity_name: &str,
    wc: &reify_syntax::WhereClause,
    constraint: CompiledConstraint,
    scope: &mut CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
) {
    let guard_expr = compile_expr(&wc.condition, scope, enum_defs, functions, diagnostics);
    let guard_cell_id = ValueCellId::new(entity_name, format!("__guard_{}", guard_index));
    *guard_index += 1;

    structure_controlling.insert(guard_cell_id.clone());
    guarded_groups.push(CompiledGuardedGroup {
        guard_expr,
        guard_value_cell: guard_cell_id,
        members: vec![],
        constraints: vec![constraint],
        else_members: vec![],
        else_constraints: vec![],
        parent_guard: None,
    });
}
