use super::*;

pub(crate) fn collect_body_refs(expr: &CompiledExpr) -> Vec<ValueCellId> {
    let mut refs = Vec::new();
    collect_body_refs_inner(expr, &mut refs);
    refs
}

pub(crate) fn collect_body_refs_inner(expr: &CompiledExpr, refs: &mut Vec<ValueCellId>) {
    match &expr.kind {
        CompiledExprKind::ValueRef(id) | CompiledExprKind::CrossSubGeometryRef(id) => {
            refs.push(id.clone())
        }
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
        // task 3540: recurse into the ctor's supplied args + captured
        // defaults so guarded-group ref collection stays complete.
        CompiledExprKind::StructureInstanceCtor {
            ordered_args,
            defaults,
            ..
        } => {
            for (_, arg) in ordered_args {
                collect_body_refs_inner(arg, refs);
            }
            for (_, def) in defaults {
                collect_body_refs_inner(def, refs);
            }
        }
        // task 4118 (γ): recurse into the wrapped selector so guarded-group
        // ref collection sees through the Selector→List<Geometry> coercion.
        CompiledExprKind::ResolveSelector { selector } => {
            collect_body_refs_inner(selector, refs);
        }
    }
}

/// Register names from guarded group members in the compilation scope (pass 1).
/// Recursively handles nested guarded groups.
#[allow(clippy::too_many_arguments)]
pub(crate) fn register_guarded_names<'a>(
    members: &'a [reify_ast::MemberDecl],
    scope: &mut CompilationScope,
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    known_geometry_lets: &mut HashSet<&'a str>,
    known_selector_lets: &mut HashSet<&'a str>,
) {
    for member in members {
        match member {
            reify_ast::MemberDecl::Param(param) => {
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
                                .with_code(DiagnosticCode::UnresolvedType)
                                .with_label(DiagnosticLabel::new(
                                    type_expr.span,
                                    "unknown type name",
                                )),
                        );
                        Type::dimensionless_scalar()
                    })
                } else {
                    Type::dimensionless_scalar()
                };
                // Solid-typed params with a geometry-call default are treated
                // symmetrically to geometry lets (mirrors entity.rs pre-pass).
                // (is_solid_geometry_param inlined here — retired in GHR-γ, task 3605)
                if ty == Type::Geometry
                    && param
                        .default
                        .as_ref()
                        .map(|e| is_geometry_let(e, functions, known_geometry_lets, known_selector_lets))
                        .unwrap_or(false)
                {
                    scope.has_geometry = true;
                    known_geometry_lets.insert(param.name.as_str());
                }
                scope.register(&param.name, ty);
            }
            reify_ast::MemberDecl::Let(let_decl) => {
                if is_geometry_let(&let_decl.value, functions, known_geometry_lets, known_selector_lets) {
                    scope.register(&let_decl.name, Type::Geometry);
                    known_geometry_lets.insert(let_decl.name.as_str());
                } else {
                    scope.register(&let_decl.name, Type::dimensionless_scalar());
                    // Track selector lets so subsequent all-ident compositions
                    // are correctly classified. Mirrors entity.rs pre-pass. (task 4527)
                    if is_selector_expr(&let_decl.value, functions, known_selector_lets) {
                        known_selector_lets.insert(let_decl.name.as_str());
                    }
                }
            }
            reify_ast::MemberDecl::GuardedGroup(g) => {
                // `known_geometry_lets` is intentionally shared across both branches,
                // consistent with how `scope` is shared: names registered in the
                // if-branch are visible when processing the else-branch. As a result,
                // an Ident alias in the else-branch may be classified as a geometry let
                // if the aliased name appeared in the if-branch. Fixing this would
                // require snapshotting both `scope` and `known_geometry_lets` atomically
                // for each branch — a larger change that is deferred until needed.
                // The same sharing applies to `known_selector_lets`. (task 4527)
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
                    known_selector_lets,
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
                    known_selector_lets,
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
    g: &reify_ast::GuardedGroupDecl,
    outer_guard: Option<&ValueCellId>,
    scope: &mut CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
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
    known_selector_lets: &HashSet<&str>,
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
        known_selector_lets,
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
            known_selector_lets,
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
    ast_members: &[reify_ast::MemberDecl],
    current_guard: &ValueCellId,
    scope: &mut CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
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
    known_selector_lets: &HashSet<&str>,
) {
    let guard_ctx = Some(current_guard);
    for member in ast_members {
        match member {
            reify_ast::MemberDecl::Param(param) => {
                let id = ValueCellId::new(entity_name, &param.name);
                let cell_type = scope
                    .resolve(&param.name)
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or_else(|| emit_ice_unresolved(UnresolvedKind::GuardedMember, &param.name, param.span, diagnostics));

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
                        is_aux: false,
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
                        is_aux: false,
                        cell_type,
                        default_expr,
                        solver_hints,
                        span: param.span,
                    }
                };
                members.push(decl);
            }
            reify_ast::MemberDecl::Let(let_decl) => {
                if is_geometry_let(&let_decl.value, functions, known_geometry_lets, known_selector_lets) {
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
                    is_aux: let_decl.is_aux,
                    cell_type,
                    default_expr: Some(compiled_expr),
                    solver_hints,
                    span: let_decl.span,
                });
            }
            reify_ast::MemberDecl::Constraint(constraint) => {
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
            reify_ast::MemberDecl::GuardedGroup(nested) => {
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
                    known_selector_lets,
                );
            }
            reify_ast::MemberDecl::Sub(s) => {
                diagnostics.push(
                    Diagnostic::error("sub declarations in guarded blocks are not yet supported")
                        .with_label(DiagnosticLabel::new(s.span, "not yet supported")),
                );
            }
            reify_ast::MemberDecl::Minimize(m) => {
                diagnostics.push(
                    Diagnostic::error(
                        "minimize declarations in guarded blocks are not yet supported",
                    )
                    .with_label(DiagnosticLabel::new(m.span, "not yet supported")),
                );
            }
            reify_ast::MemberDecl::Maximize(m) => {
                diagnostics.push(
                    Diagnostic::error(
                        "maximize declarations in guarded blocks are not yet supported",
                    )
                    .with_label(DiagnosticLabel::new(m.span, "not yet supported")),
                );
            }
            reify_ast::MemberDecl::ForallConnect(f) => {
                diagnostics.push(
                    Diagnostic::error(
                        "forall connect/chain statements in guarded blocks are not yet supported",
                    )
                    .with_label(DiagnosticLabel::new(f.span, "not yet supported")),
                );
            }
            reify_ast::MemberDecl::ForallConstraint(f) => {
                diagnostics.push(
                    Diagnostic::error(
                        "forall constraint statements in guarded blocks are not yet supported",
                    )
                    .with_label(DiagnosticLabel::new(f.span, "not yet supported")),
                );
            }
            reify_ast::MemberDecl::Port(_)
            | reify_ast::MemberDecl::Connect(_)
            | reify_ast::MemberDecl::Chain(_)
            | reify_ast::MemberDecl::AssociatedType(_)
            // Trait fn members inside a where{} guard: deferred to task δ/ζ.
            | reify_ast::MemberDecl::Fn(_)
            | reify_ast::MemberDecl::MetaBlock(_)
            | reify_ast::MemberDecl::ConstraintInst(_)
            // task 2372: match-arm decl group members inside a where{} guard are
            // handled in the parent compile_entity loop, not here.
            | reify_ast::MemberDecl::MatchArmDeclGroup(_) => {
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
    wc: &reify_ast::WhereClause,
    member_decl: ValueCellDecl,
    scope: &mut CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
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
    wc: &reify_ast::WhereClause,
    constraint: CompiledConstraint,
    scope: &mut CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
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

/// Structurally narrow a `GuardedDeclGroup`'s arms based on the active guard
/// at a reference site (task 2373).
///
/// Returns the subset of `arms` whose `guard_value_cell` is reachable from
/// `current_guard` via the `parent_chain` (mirroring the `is_ancestor_guard`
/// walk in entity.rs:1731-1739). When `current_guard` is `None`, all arms
/// are returned (no narrowing — the full union).
///
/// If `current_guard` is `Some` but no arm is reachable, falls back to the
/// full arm set conservatively (no implication established → conservative
/// full union). This mirrors the v0.1 narrowing-is-structural decision: the
/// only existing implication mechanism is the parent-chain walk; we never
/// over-narrow.
///
/// In v0.1, surface syntax does not produce a `current_guard` matching an
/// arm's cell, so callers in `expr.rs` always pass `None`. The helper is
/// exercised by direct unit tests pinning the contract for future tasks.
//
// `dead_code` is allowed here because v0.1's `expr.rs` MemberAccess hookup
// (task 2373 step-8) always returns the full union — narrowing under arm
// guards is a contract pinned by the inline unit tests below for future
// tasks (e.g., when surface syntax for narrowing-on-decl is introduced).
#[allow(dead_code)]
pub(crate) fn narrow_arms_under_guard<'a>(
    arms: &'a [GuardedDeclArm],
    current_guard: Option<&ValueCellId>,
    parent_chain: &HashMap<ValueCellId, Option<ValueCellId>>,
) -> Vec<&'a GuardedDeclArm> {
    let Some(current) = current_guard else {
        return arms.iter().collect();
    };
    // Walk parent chain from current upward, collecting any arm whose cell
    // appears (including current itself).
    let mut active = Vec::new();
    for arm in arms {
        if &arm.guard_value_cell == current {
            active.push(arm);
            continue;
        }
        let mut cursor = parent_chain.get(current).and_then(|p| p.as_ref());
        while let Some(ancestor) = cursor {
            if ancestor == &arm.guard_value_cell {
                active.push(arm);
                break;
            }
            cursor = parent_chain.get(ancestor).and_then(|p| p.as_ref());
        }
    }
    if active.is_empty() {
        // No implication established — return the full arm set conservatively.
        return arms.iter().collect();
    }
    active
}

#[cfg(test)]
mod narrow_arms_under_guard_tests {
    use super::*;
    use reify_ir::Value;

    fn make_arm(guard_member: &str, arm_struct: &str) -> GuardedDeclArm {
        GuardedDeclArm {
            guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
            guard_value_cell: ValueCellId::new("Bolt", guard_member),
            arm_type: Type::StructureRef(arm_struct.to_string()),
        }
    }

    /// Step-15 case 1: `current_guard == arm[0].guard_value_cell` → `[arm[0]]`.
    #[test]
    fn narrow_arms_collapses_to_arm_when_guard_matches_arm_cell() {
        let arms = vec![
            make_arm("__guard_0", "HexHead"),
            make_arm("__guard_1", "SocketHead"),
        ];
        let parent_chain: HashMap<ValueCellId, Option<ValueCellId>> = HashMap::new();
        let current = arms[0].guard_value_cell.clone();
        let result = narrow_arms_under_guard(&arms, Some(&current), &parent_chain);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].guard_value_cell, arms[0].guard_value_cell);
    }

    /// Step-15 case 2: `current_guard == None` → all arms.
    #[test]
    fn narrow_arms_returns_all_when_no_guard() {
        let arms = vec![
            make_arm("__guard_0", "HexHead"),
            make_arm("__guard_1", "SocketHead"),
        ];
        let parent_chain: HashMap<ValueCellId, Option<ValueCellId>> = HashMap::new();
        let result = narrow_arms_under_guard(&arms, None, &parent_chain);
        assert_eq!(result.len(), 2);
    }

    /// Step-15 case 3: non-matching guard whose parent chain reaches
    /// `arm[1].guard_value_cell` → `[arm[1]]`.
    #[test]
    fn narrow_arms_walks_parent_chain_to_match_arm() {
        let arms = vec![
            make_arm("__guard_0", "HexHead"),
            make_arm("__guard_1", "SocketHead"),
        ];
        let nested = ValueCellId::new("Bolt", "__guard_2");
        // nested's parent is arm[1].guard_value_cell (which is itself a "top-level" guard for the
        // parent_chain — its own entry is None).
        let mut parent_chain: HashMap<ValueCellId, Option<ValueCellId>> = HashMap::new();
        parent_chain.insert(nested.clone(), Some(arms[1].guard_value_cell.clone()));
        parent_chain.insert(arms[1].guard_value_cell.clone(), None);
        parent_chain.insert(arms[0].guard_value_cell.clone(), None);
        let result = narrow_arms_under_guard(&arms, Some(&nested), &parent_chain);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].guard_value_cell, arms[1].guard_value_cell);
    }

    /// Amendment for review suggestion 4: integration-style proof that the
    /// `parent_chain` shape consumed by `narrow_arms_under_guard` matches
    /// what `entity.rs` actually constructs at lines ~1757-1760
    /// (`guard_parent_map: HashMap<ValueCellId, Option<ValueCellId>>` built
    /// from `guarded_groups.iter().map(|g| (g.guard_value_cell.clone(),
    /// g.parent_guard.clone())).collect()`).
    ///
    /// This builds a `Vec<CompiledGuardedGroup>` mirroring how
    /// `compile_match_arm_decl_group` and `compile_block_guard` populate the
    /// reference-safety sweep, then transforms it via the *same expression*
    /// used in entity.rs to produce the parent_chain. Two `GuardedDeclArm`
    /// entries are produced with cell IDs matching their group entries; one
    /// outer (non-arm) guard is also added with the inner-arm's parent set
    /// to the outer guard. We assert that narrowing under the outer guard
    /// reaches both arms (because the outer guard's chain crosses neither
    /// arm cell), and narrowing under one arm cell reaches just that arm.
    #[test]
    fn narrow_arms_under_real_entity_parent_chain_shape() {
        use reify_ir::CompiledExpr;

        let outer_guard = ValueCellId::new("Bolt", "__guard_outer");
        let arm0_guard = ValueCellId::new("Bolt", "__guard_0");
        let arm1_guard = ValueCellId::new("Bolt", "__guard_1");

        // Build CompiledGuardedGroup entries the same way
        // compile_match_arm_decl_group / compile_block_guard do, then derive
        // the parent_chain via the entity.rs:1757-1760 expression.
        let groups: Vec<crate::types::CompiledGuardedGroup> = vec![
            crate::types::CompiledGuardedGroup {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: outer_guard.clone(),
                members: vec![],
                constraints: vec![],
                else_members: vec![],
                else_constraints: vec![],
                parent_guard: None,
            },
            crate::types::CompiledGuardedGroup {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: arm0_guard.clone(),
                members: vec![],
                constraints: vec![],
                else_members: vec![],
                else_constraints: vec![],
                parent_guard: Some(outer_guard.clone()),
            },
            crate::types::CompiledGuardedGroup {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: arm1_guard.clone(),
                members: vec![],
                constraints: vec![],
                else_members: vec![],
                else_constraints: vec![],
                parent_guard: Some(outer_guard.clone()),
            },
        ];

        // Mirror entity.rs:1757-1760 verbatim.
        let parent_chain: HashMap<ValueCellId, Option<ValueCellId>> = groups
            .iter()
            .map(|g| (g.guard_value_cell.clone(), g.parent_guard.clone()))
            .collect();

        let arms = vec![
            GuardedDeclArm {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: arm0_guard.clone(),
                arm_type: Type::StructureRef("HexHead".to_string()),
            },
            GuardedDeclArm {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: arm1_guard.clone(),
                arm_type: Type::StructureRef("SocketHead".to_string()),
            },
        ];

        // Case A: current_guard == arm0_guard → narrows to arm[0] only.
        let result = narrow_arms_under_guard(&arms, Some(&arm0_guard), &parent_chain);
        assert_eq!(
            result.len(),
            1,
            "narrowing under arm[0]'s own guard cell must collapse to that single arm"
        );
        assert_eq!(result[0].guard_value_cell, arm0_guard);

        // Case B: current_guard == outer_guard → no implication established
        // (outer is parent OF the arms, not reachable FROM the arms via the
        // parent chain — the helper walks current→ancestors), so the helper
        // returns the conservative full arm set.
        let result = narrow_arms_under_guard(&arms, Some(&outer_guard), &parent_chain);
        assert_eq!(
            result.len(),
            2,
            "narrowing under an outer (parent-of-arms) guard must not establish \
             implication and falls back to the full arm set"
        );
    }
}
