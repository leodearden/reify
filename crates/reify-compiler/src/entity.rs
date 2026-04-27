use super::*;
use std::collections::BTreeMap;
use std::collections::HashSet;
use crate::compile_builder::hash::hash_pragma;

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
/// Match arms recurse into the body with the full set of bindings — arm
/// patterns are structural (enum variants, literals) and do not introduce
/// shadowing. If pattern bindings are introduced in the future (e.g.
/// `x @ Pattern` or destructuring), arm-level shadowing suppression must be
/// added here. Conditional branches (`if/then/else`) are traversed
/// transparently; substitution applies to condition, then-branch, and
/// else-branch alike.
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
///
/// # Two-pass compilation
///
/// The member list is walked twice:
///
/// **Pass 1** (pre-pass, the `known_geometry_lets` loop): registers every
/// param, let, port, sub-component, and guarded-group name into
/// `CompilationScope` with a best-effort type, and simultaneously builds the
/// `known_geometry_lets: HashSet<&str>` accumulator. No expression is compiled
/// in this pass — only name-to-type bindings are established.
///
/// **Pass 2** (main member loop, after the pre-pass): compiles expressions
/// with the scope already fully populated. Because every name is registered
/// before any expression is compiled, expressions may reference a param or let
/// declared *later* in the member list — true forward references within the
/// entity body. This is behaviourally pinned by
/// `let_type_disambiguation_tests::unannotated_let_resolves_forward_reference_to_annotated_let`
/// and `unannotated_let_resolves_forward_reference_to_annotated_param`.
///
/// # Ordering caveat: `known_geometry_lets`
///
/// Unlike scope name resolution (order-free by design), the
/// `known_geometry_lets` accumulator is built **incrementally** during pass 1.
/// When a let's value expression is an `Ident`, `is_geometry_let` can only
/// classify it as a geometry let if the aliased name is already in the set at
/// the moment that member is visited. An alias that appears before its referent
/// in member order is therefore **not** classified as a geometry let, even
/// though the referent will be inserted shortly after. This conservative
/// behaviour is intentional and is pinned by
/// `let_scope_tests::cyclic_ident_alias_does_not_crash`, whose inline comment
/// notes "the forward-pass incremental set never adds either to
/// known_geometry_lets". Forward alias chains that are ordered correctly
/// (referent before alias) do propagate transitively.
///
/// Guarded groups follow the same two-pass + incremental-classification pattern
/// via `register_guarded_names` and `compile_guarded_members` (guards.rs).
///
/// # Shadowing
///
/// `CompilationScope::register` (`scope.rs`) uses `HashMap::insert`, so a
/// later same-named registration overwrites the earlier entry. `known_geometry_lets`
/// being a `HashSet` follows the same idempotent-add convention (a name that is
/// already geometry stays geometry; duplicate registration is harmless).
///
/// The separate shadow rule for the `functions: &[CompiledFunction]` parameter
/// — user functions first, prelude appended without duplicates — is applied
/// upstream by `merge_prelude_functions` (`lib.rs`). `is_geometry_let` queries
/// `functions` via `.iter().any(…)` and is therefore order-independent with
/// respect to that slice.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_entity(
    structure: &EntityDefRef<'_>,
    entity_kind: EntityKind,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    trait_registry: &HashMap<String, &CompiledTrait>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    field_registry: &HashMap<String, &CompiledField>,
    constraint_def_registry: &HashMap<String, &CompiledConstraintDef>,
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

    // First pass: register all param and let names (and ports, subs, guarded
    // groups) into the scope so pass 2 expressions can reference any name in the
    // entity body, regardless of declaration order (true forward references).
    // Types are resolved here as well so the scope entries are usable in pass 2.
    //
    // `known_geometry_lets` tracks which let names resolve to geometry (either a
    // direct geometry function call or an Ident alias to an already-known geometry
    // let). It is built incrementally — each Let is classified using only the
    // names already in the set at that point in the walk. An Ident alias that
    // appears *before* its referent is therefore not classified as geometry, even
    // though the referent will be inserted on the next visit. This ordering
    // constraint is the dual of the forward-reference freedom enjoyed by pass 2:
    // scope name resolution is order-free (whole-pass pre-registration), while
    // geometry-let classification is order-sensitive (incremental accumulation).
    // Pinned by `let_scope_tests::cyclic_ident_alias_does_not_crash`.
    let mut known_geometry_lets: HashSet<&str> = HashSet::new();
    for member in structure.members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let ty = if let Some(type_expr) = &param.type_expr {
                    match resolve_type_expr_with_aliases(
                        type_expr,
                        &type_param_names,
                        alias_registry,
                        diagnostics,
                        structure_names,
                        trait_names,
                    ) {
                        Some(t) => t,
                        None => {
                            // Check if it's an enum type defined in the same module or prelude
                            if let reify_syntax::TypeExprKind::Named { name, type_args } =
                                &type_expr.kind
                                && let Some(t) = resolve_enum_type(name, enum_defs)
                            {
                                // Reify enums are non-parametric. Emit a user-facing diagnostic
                                // if type_args are present so the error is visible in release builds too.
                                if !type_args.is_empty() {
                                    diagnostics.push(
                                        Diagnostic::error(format!(
                                            "enum `{}` does not accept type arguments",
                                            name
                                        ))
                                        .with_label(
                                            DiagnosticLabel::new(
                                                type_expr.span,
                                                "enum types are not generic",
                                            ),
                                        ),
                                    );
                                }
                                t
                            } else {
                                diagnostics.push(
                                    Diagnostic::error(format!("unresolved type: {}", type_expr))
                                        .with_label(DiagnosticLabel::new(
                                            type_expr.span,
                                            "unknown type name",
                                        )),
                                );
                                Type::Real // fallback
                            }
                        }
                    }
                } else {
                    // Infer type from default expression if available
                    Type::Real
                };
                // Solid-typed params with a geometry-call default are treated
                // symmetrically to geometry lets: register as Type::Geometry,
                // mark scope as having geometry, and track in known_geometry_lets
                // so subsequent members can reference this param as a geometry source.
                if is_solid_geometry_param(
                    &ty,
                    param.default.as_ref(),
                    functions,
                    &known_geometry_lets,
                ) {
                    scope.has_geometry = true;
                    known_geometry_lets.insert(param.name.as_str());
                }
                scope.register(&param.name, ty);
            }
            reify_syntax::MemberDecl::Let(let_decl) => {
                // For lets, we need to infer the type from the expression.
                // Geometry lets produce realizations (not value cells) but still
                // need to be registered in scope so subsequent lets can reference them.
                if is_geometry_let(&let_decl.value, functions, &known_geometry_lets) {
                    scope.has_geometry = true;
                    scope.register(&let_decl.name, Type::Geometry);
                    known_geometry_lets.insert(let_decl.name.as_str());
                } else {
                    // We'll register with a placeholder type; the actual type will
                    // be determined when we compile the expression. For now, use Real.
                    // We'll update this after the expression is compiled.
                    scope.register(&let_decl.name, Type::Real);
                }
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                // `known_geometry_lets` is intentionally shared across both branches
                // (consistent with the same pattern in register_guarded_names/guards.rs).
                register_guarded_names(
                    &g.members,
                    &mut scope,
                    functions,
                    diagnostics,
                    &type_param_names,
                    alias_registry,
                    structure_names,
                    trait_names,
                    &mut known_geometry_lets,
                );
                register_guarded_names(
                    &g.else_members,
                    &mut scope,
                    functions,
                    diagnostics,
                    &type_param_names,
                    alias_registry,
                    structure_names,
                    trait_names,
                    &mut known_geometry_lets,
                );
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
                                resolve_type_expr_with_aliases(
                                    type_expr,
                                    &type_param_names,
                                    alias_registry,
                                    diagnostics,
                                    structure_names,
                                    trait_names,
                                )
                                .unwrap_or_else(|| {
                                    diagnostics.push(
                                        Diagnostic::error(format!(
                                            "unresolved type name '{}' in port parameter",
                                            type_expr
                                        ))
                                        .with_label(
                                            DiagnosticLabel::new(type_expr.span, "unknown type"),
                                        ),
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
                // Single lookup: handle deprecation, sub_structure_traits, and
                // sub_member_types in one pass over compiled_templates.
                if let Some(child_tmpl) = find_template(compiled_templates, &sub.structure_name) {
                    // Deprecation check: warn if the referenced structure is @deprecated.
                    if let Some(msg) = deprecation_message(&child_tmpl.annotations) {
                        emit_deprecation_warning(
                            "structure",
                            &sub.structure_name,
                            msg,
                            sub.span,
                            diagnostics,
                        );
                    }
                    scope
                        .sub_structure_traits
                        .insert(sub.structure_name.clone(), child_tmpl.trait_bounds.clone());
                    // Populate sub_member_types for self.sub.member resolution.
                    let member_types: BTreeMap<String, Type> = child_tmpl
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
                    scope.has_meta_block = true;
                    let mut seen_meta_keys: HashSet<&str> = HashSet::new();
                    for (key, value) in &meta.entries {
                        if !seen_meta_keys.insert(key.as_str()) {
                            diagnostics.push(
                                Diagnostic::error(format!("duplicate meta key '{}'", key))
                                    .with_label(DiagnosticLabel::new(
                                        meta.span,
                                        format!("duplicate key '{}' in this meta block", key),
                                    )),
                            );
                        } else {
                            scope.meta_entries.insert(key.clone(), value.clone());
                        }
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
            structure_names,
            trait_names,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            enum_defs,
            functions,
            alias_registry,
            diagnostics,
        );

        // Trait-bound checks: deprecation warning and parameterized type-argument deferral.
        // One registry lookup per bound handles both checks.
        for trait_bound in structure.trait_bounds {
            let compiled_trait = trait_registry.get(&trait_bound.name);
            // Deprecation check: warn if the referenced trait is @deprecated.
            if let Some(ct) = compiled_trait
                && let Some(msg) = deprecation_message(&ct.annotations)
            {
                emit_deprecation_warning(
                    "trait",
                    &trait_bound.name,
                    msg,
                    trait_bound.span,
                    diagnostics,
                );
            }
            // Geometry-marker escape hatch: if the bound names one of the seven stdlib
            // geometry-conformance marker traits, emit W_TRAIT_USER_ASSERTED. The
            // declaration is treated as a user assertion that bypasses any future runtime
            // conformance check (PRD geometry-traits.md task 6 / W_TRAIT_USER_ASSERTED).
            // Detection is name-based (case-sensitive) — see design decision §1 of task 2321.
            if crate::geometry_traits_inference::is_geometry_marker_trait(&trait_bound.name) {
                diagnostics.push(
                    Diagnostic::warning(format!(
                        "geometry trait '{}' on '{}' is treated as a user assertion; runtime conformance check is suppressed",
                        trait_bound.name, structure.name
                    ))
                    .with_code(DiagnosticCode::TraitUserAsserted)
                    .with_label(DiagnosticLabel::new(
                        trait_bound.span,
                        "user-asserted geometry trait",
                    )),
                );
            }
            // Defer type argument checking on parameterized trait bounds (e.g., Container<Bolt>)
            // to the post-compilation pass so forward references are resolved correctly.
            if !trait_bound.type_args.is_empty()
                && let Some(ct) = compiled_trait
                && !ct.type_params.is_empty()
            {
                let resolved_args: Vec<Type> = trait_bound
                    .type_args
                    .iter()
                    .map(|ta| {
                        if let reify_syntax::TypeExprKind::Named { name, .. } = &ta.kind {
                            resolve_type_name(name).unwrap_or_else(|| {
                                if type_param_names.contains(name) {
                                    Type::TypeParam(name.clone())
                                } else {
                                    Type::StructureRef(name.clone())
                                }
                            })
                        } else {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unexpected dimensional expression in type argument: {}",
                                    ta
                                ))
                                .with_label(DiagnosticLabel::new(
                                    ta.span,
                                    "unexpected dimensional expression in type argument",
                                )),
                            );
                            Type::Real
                        }
                    })
                    .collect();
                // TraitConformance: type_params are known now from the compiled
                // trait, so they're carried directly in the enum variant.
                pending_bound_checks.push(PendingBoundCheck::TraitConformance {
                    type_params: ct.type_params.clone(),
                    type_args: resolved_args,
                    target_name: trait_bound.name.clone(),
                    span: trait_bound.span,
                });
            }
        }
    }

    // Second pass: compile all members.
    // Track per-constraint-def instantiation counts within this entity so each
    // instantiation gets a unique inst_idx in the label (e.g. `MinWall#0[0]`
    // and `MinWall#1[0]` for two distinct instantiations of MinWall). Scoped
    // per-entity so labels are stable and locally-interpretable (see task 845).
    let mut constraint_inst_counts: HashMap<String, usize> = HashMap::new();
    for member in structure.members {
        match member {
            reify_syntax::MemberDecl::Param(param) => {
                let id = ValueCellId::new(entity_name, &param.name);
                let cell_type = scope
                    .resolve(&param.name)
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or_else(|| emit_ice_unresolved(UnresolvedKind::Name, &param.name, param.span, diagnostics));

                // Solid-typed params with a geometry-call default are lowered as
                // realizations (third pass), not as scalar ValueCellDecls.
                // Symmetric with the geometry-let early-continue in the Let branch.
                if is_solid_geometry_param(
                    &cell_type,
                    param.default.as_ref(),
                    functions,
                    &known_geometry_lets,
                ) {
                    continue;
                }

                let auto_free = param.default.as_ref().and_then(extract_auto_free);

                // Lower and validate annotations on this param
                let lowered_annotations = lower_annotations(&param.annotations, diagnostics);
                validate_annotations(&lowered_annotations, "param", diagnostics);
                let solver_hints = extract_solver_hints(&lowered_annotations, diagnostics);

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
                        let mut compiled =
                            compile_expr(expr, &scope, enum_defs, functions, diagnostics);
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
                // Skip geometry-producing function calls (and ident aliases to them)
                if is_geometry_let(&let_decl.value, functions, &known_geometry_lets) {
                    continue;
                }

                let mut compiled_expr =
                    compile_expr(&let_decl.value, &scope, enum_defs, functions, diagnostics);
                fixup_option_none_for_let(
                    &mut compiled_expr,
                    let_decl.type_expr.as_ref(),
                    &type_param_names,
                    alias_registry,
                    structure_names,
                    trait_names,
                    diagnostics,
                );
                let cell_type = compiled_expr.result_type.clone();
                let id = ValueCellId::new(entity_name, &let_decl.name);

                // Update the scope with the inferred type
                scope.register(&let_decl.name, cell_type.clone());

                let visibility = if let_decl.is_pub {
                    Visibility::Public
                } else {
                    Visibility::Private
                };

                // Lower and validate annotations on this let
                let lowered_annotations = lower_annotations(&let_decl.annotations, diagnostics);
                validate_annotations(&lowered_annotations, "let", diagnostics);
                let solver_hints = extract_solver_hints(&lowered_annotations, diagnostics);

                let decl = ValueCellDecl {
                    id,
                    kind: ValueCellKind::Let,
                    visibility,
                    cell_type,
                    default_expr: Some(compiled_expr),
                    solver_hints,
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
                        solver_hints: Vec::new(),
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
                        optimized_target: None,
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
                        if let reify_syntax::TypeExprKind::Named { name, .. } = &ta.kind {
                            resolve_type_name(name).unwrap_or_else(|| {
                                if type_param_names.contains(name) {
                                    Type::TypeParam(name.clone())
                                } else {
                                    Type::StructureRef(name.clone())
                                }
                            })
                        } else {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unexpected dimensional expression in type argument: {}",
                                    ta
                                ))
                                .with_label(DiagnosticLabel::new(
                                    ta.span,
                                    "unexpected dimensional expression in type argument",
                                )),
                            );
                            Type::Real
                        }
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

                // TraitArgConformance: defer one check per named arg so that
                // forward-referenced target structures (and their param types)
                // are available in the template registry during the post-pass.
                // Zip sub.args (carries the original Expr with its source span)
                // with compiled_args so we can use per-arg spans in diagnostics.
                // The full CompiledExpr is stored so the conformance walker can
                // recurse into nested OptionSome / ListLiteral / SetLiteral /
                // MapLiteral nodes.
                //
                // Cost note (task 2280): `compiled_arg.clone()` below is O(literal-tree-size)
                // per arg.  See the `PendingBoundCheck::TraitArgConformance` doc-comment below
                // for the Rc/arena trade-off analysis, and `tests/trait_arg_conformance_bench.rs`
                // for the timing bench (run with `-- --ignored --nocapture`).
                for ((_, arg_expr), (arg_name, compiled_arg)) in
                    sub.args.iter().zip(compiled_args.iter())
                {
                    pending_bound_checks.push(PendingBoundCheck::TraitArgConformance {
                        target_name: sub.structure_name.clone(),
                        arg_name: arg_name.clone(),
                        compiled_arg: compiled_arg.clone(), // O(tree-size) — see cost note above
                        span: arg_expr.span,
                    });
                }

                // Compile the sub's where_clause into a GuardState (used by termination check).
                // Uses Severity::Error-only filter (not any-diagnostic) so that a guard
                // that compiles successfully but emits only warnings is still stored as
                // Compiled(_) — matching the pattern at conformance/checker.rs:548-550.
                let guard_state = match sub.where_clause.as_ref() {
                    None => GuardState::None,
                    Some(wc) => {
                        let diag_count_before = diagnostics.len();
                        let compiled =
                            compile_expr(&wc.condition, &scope, enum_defs, functions, diagnostics);
                        let had_error = diagnostics[diag_count_before..]
                            .iter()
                            .any(|d| d.severity == Severity::Error);
                        if had_error {
                            // Guard compilation emitted an error — the guard is unusable for
                            // termination analysis. Record the failure so the termination check
                            // can distinguish "user wrote no guard" from "user's guard was broken".
                            GuardState::Broken
                        } else {
                            GuardState::Compiled(compiled)
                        }
                    }
                };

                sub_components.push(SubComponentDecl {
                    name: sub.name.clone(),
                    structure_name: sub.structure_name.clone(),
                    visibility: Visibility::Public,
                    args: compiled_args,
                    type_args: resolved_type_args,
                    is_collection: sub.is_collection,
                    count_cell: None,
                    guard_state,
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
                    &type_param_names,
                    alias_registry,
                    structure_names,
                    trait_names,
                    &known_geometry_lets,
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
                                .unwrap_or_else(|| emit_ice_unresolved(UnresolvedKind::Name, &composite_name, param.span, diagnostics));

                            let auto_free = param.default.as_ref().and_then(extract_auto_free);

                            let decl = if let Some(free) = auto_free {
                                ValueCellDecl {
                                    id,
                                    kind: ValueCellKind::Auto { free },
                                    visibility: Visibility::Public,
                                    cell_type,
                                    default_expr: None,
                                    solver_hints: Vec::new(),
                                    span: param.span,
                                }
                            } else {
                                let default_expr = param.default.as_ref().map(|expr| {
                                    let mut compiled = compile_expr(
                                        expr,
                                        &scope,
                                        enum_defs,
                                        functions,
                                        diagnostics,
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
                                    solver_hints: Vec::new(),
                                    span: param.span,
                                }
                            };
                            port_members.push(decl);
                        }
                        reify_syntax::MemberDecl::Let(let_decl) => {
                            let composite_name = format!("{}.{}", port_decl.name, let_decl.name);
                            let mut compiled_expr = compile_expr(
                                &let_decl.value,
                                &scope,
                                enum_defs,
                                functions,
                                diagnostics,
                            );
                            fixup_option_none_for_let(
                                &mut compiled_expr,
                                let_decl.type_expr.as_ref(),
                                &type_param_names,
                                alias_registry,
                                structure_names,
                                trait_names,
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
                                solver_hints: Vec::new(),
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
                                optimized_target: None,
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
                    trait_registry,
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
                    trait_registry,
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

                // Allocate this instantiation's inst_idx before the per-predicate
                // loop so all predicates from one `constraint MinWall(...)` share
                // the same inst_idx — predicates differ only by pred_idx.
                // Uses a get_mut/insert split to avoid cloning `ci.name` on the
                // common case where the entry already exists.
                let inst_idx = if let Some(entry) = constraint_inst_counts.get_mut(&ci.name) {
                    let idx = *entry;
                    *entry += 1;
                    idx
                } else {
                    constraint_inst_counts.insert(ci.name.clone(), 1);
                    0
                };

                // For each predicate in the constraint def, substitute params with args
                // and compile the resulting expression in the calling entity's scope.
                // `annotations_optimized_target` was cached at def-compile time; clone it
                // directly per predicate rather than creating an extra intermediate clone.
                for (pred_idx, predicate) in def.predicates.iter().enumerate() {
                    let substituted = substitute_expr(predicate, &arg_map);
                    let compiled_expr =
                        compile_expr(&substituted, &scope, enum_defs, functions, diagnostics);

                    let id = ConstraintNodeId::new(entity_name, constraint_index);
                    let cc = CompiledConstraint {
                        id,
                        label: Some(format!("{}#{}[{}]", ci.name, inst_idx, pred_idx)),
                        expr: compiled_expr,
                        span: ci.span,
                        domain: None,
                        optimized_target: def.annotations_optimized_target.clone(),
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
            reify_syntax::MemberDecl::ForallConnect(f) => {
                // TODO(task 2364): per-element elaboration not yet implemented.
                diagnostics.push(
                    Diagnostic::error(
                        "statement-form forall (connect/chain) not yet elaborated",
                    )
                    .with_label(DiagnosticLabel::new(
                        f.span,
                        "not yet elaborated — see task 2364",
                    )),
                );
            }
            reify_syntax::MemberDecl::ForallConstraint(f) => {
                // TODO(task 2364): per-element elaboration not yet implemented.
                diagnostics.push(
                    Diagnostic::error(
                        "statement-form forall (constraint) not yet elaborated",
                    )
                    .with_label(DiagnosticLabel::new(
                        f.span,
                        "not yet elaborated — see task 2364",
                    )),
                );
            }
        }
    }

    // Third pass: compile geometry let bindings into realizations.
    // Build a lookup table mapping geometry let/param names to their initializer AST
    // expressions. This allows compile_geometry_call to resolve Ident references
    // (let-bound geometry variables) used as arguments to boolean ops.
    // `collect_geometry_exprs` recurses fully into nested GuardedGroupDecl members
    // so geometry params at any nesting depth are captured.
    let geometry_lets: HashMap<&str, &reify_syntax::Expr> = {
        let mut map = HashMap::new();
        collect_geometry_exprs(structure.members, &known_geometry_lets, functions, &mut map);
        map
    };

    let mut realizations = Vec::new();
    let mut realization_index: u32 = 0;

    for member in structure.members {
        match member {
            reify_syntax::MemberDecl::Let(let_decl)
                if is_geometry_let(&let_decl.value, functions, &known_geometry_lets) =>
            {
                if let Some(ops) = compile_geometry_call(
                    &let_decl.value,
                    &scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    0,
                    &geometry_lets,
                    &mut HashSet::new(),
                ) {
                    let feature_tags = derive_feature_tags(&ops, let_decl.span);
                    realizations.push(RealizationDecl {
                        id: RealizationNodeId::new(entity_name, realization_index),
                        name: Some(let_decl.name.clone()),
                        feature_tags,
                        operations: ops,
                        span: let_decl.span,
                    });
                    realization_index += 1;
                }
            }
            // Solid-typed params with a geometry-call default are lowered into
            // realizations at the same position in source order.
            reify_syntax::MemberDecl::Param(param)
                if known_geometry_lets.contains(param.name.as_str()) =>
            {
                if let Some(default_expr) = &param.default
                    && let Some(ops) = compile_geometry_call(
                        default_expr,
                        &scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        0,
                        &geometry_lets,
                        &mut HashSet::new(),
                    )
                {
                    let feature_tags = derive_feature_tags(&ops, param.span);
                    realizations.push(RealizationDecl {
                        id: RealizationNodeId::new(entity_name, realization_index),
                        name: Some(param.name.clone()),
                        feature_tags,
                        operations: ops,
                        span: param.span,
                    });
                    realization_index += 1;
                }
            }
            // Recurse into guarded groups to emit realizations for guarded
            // Solid-typed params at any nesting depth (registered in
            // known_geometry_lets by register_guarded_names). Guarded geometry
            // lets do NOT emit realizations here — that is a separate,
            // unimplemented feature.
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                let deps = GeometryRealizationDeps {
                    entity_name,
                    scope: &scope,
                    enum_defs,
                    functions,
                    known_geometry_lets: &known_geometry_lets,
                    geometry_lets: &geometry_lets,
                };
                let mut sink = GeometryRealizationSink {
                    realizations: &mut realizations,
                    realization_index: &mut realization_index,
                    diagnostics,
                };
                emit_guarded_geometry_realizations(&g.members, &deps, &mut sink);
                emit_guarded_geometry_realizations(&g.else_members, &deps, &mut sink);
            }
            _ => {}
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

        // Meta entry hashes: sort by key for deterministic ordering (HashMap is unordered).
        // Hash both key and value so that key renames and value changes are both detected.
        let mut sorted_meta_keys: Vec<&str> =
            scope.meta_entries.keys().map(String::as_str).collect();
        sorted_meta_keys.sort_unstable();
        let meta_hashes = sorted_meta_keys.into_iter().flat_map(|k| {
            // `k` was collected from this map's keys() above and the map is not
            // mutated between collection and lookup, so get() always succeeds.
            let v = scope
                .meta_entries
                .get(k)
                .expect("key collected from this map")
                .as_str();
            [ContentHash::of_str(k), ContentHash::of_str(v)]
        });

        // Block-level pragma hashes (in declaration order; span excluded as positional).
        // Appended last so pragma-free templates retain identical hashes to pre-pragma-hashing
        // compilations — mirrors the module-level convention in compile_builder/hash.rs:69-81.
        let pragma_hashes = structure.pragmas.iter().map(hash_pragma);

        let all_hashes = std::iter::once(name_hash)
            .chain(vc_hashes)
            .chain(constraint_hashes)
            .chain(sub_hashes)
            .chain(guard_hashes)
            .chain(port_hashes)
            .chain(connection_hashes)
            .chain(meta_hashes)
            .chain(pragma_hashes);

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

    let context = entity_kind.as_label();
    let annotations = lower_annotations(structure.annotations, diagnostics);
    validate_annotations(&annotations, context, diagnostics);
    validate_pragmas(structure.pragmas, context, diagnostics);

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
        meta: std::mem::take(&mut scope.meta_entries),
        content_hash,
        is_recursive: false,
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
    /// Deferred call-site conformance check for a trait-typed param slot.
    /// Enqueued at the sub-compile site; dispatched in the post-compilation
    /// pass where both the template registry and trait registry are available.
    ///
    /// Carries the full `CompiledExpr` so the conformance walker can recurse
    /// into nested `OptionSome` / `ListLiteral` / `SetLiteral` / `MapLiteral`
    /// nodes and derive `arg_call_name` from any nested `FunctionCall` for the
    /// existing `Real|Int → StructureRef` promotion.
    ///
    /// **Why owned, not `Rc`/borrowed (task 2280):** storing an `Rc<CompiledExpr>`
    /// here instead of an owned value yields no benefit in practice: the
    /// `compiled_args` local that produces this field is subsequently moved into
    /// `SubComponentDecl.args: Vec<(String, CompiledExpr)>` (see the clone site at
    /// `entity.rs` in the `MemberDecl::Sub` arm, ~30 lines above the
    /// `PendingBoundCheck` push).  If the pending check holds an `Rc`, converting
    /// back to the owned vec requires `Rc::try_unwrap`, which fails (refcount > 1)
    /// and falls back to `(*rc).clone()` — still one full deep clone per arg.
    /// A real win needs a broader refactor: either switch `SubComponentDecl.args`
    /// globally to `Vec<(String, Rc<CompiledExpr>)>` (~15 touch-sites across four
    /// crates) or introduce a `CompilationCtx`-owned arena (see
    /// `compile_builder/ctx.rs`).  Both are out of scope for this observational
    /// task.  Timing bench:
    ///   `crates/reify-compiler/tests/trait_arg_conformance_bench.rs`
    ///   `cargo test -p reify-compiler --test trait_arg_conformance_bench -- --ignored --nocapture`
    TraitArgConformance {
        target_name: String,
        arg_name: String,
        compiled_arg: CompiledExpr,
        span: SourceSpan,
    },
}

/// Recursively collect geometry-let and geometry-param initializer expressions
/// from a slice of `MemberDecl`s into `out`.
///
/// Mirrors `register_guarded_names` in guards.rs in its descend-into-GuardedGroup
/// recursion. The `known` set is the `known_geometry_lets` built by the pre-pass
/// and `register_guarded_names`; a Param is included iff its name is already in
/// `known` (meaning the pre-pass already classified it as a geometry param).
///
/// Used by `compile_entity`'s third pass to build the `geometry_lets` lookup
/// table that `compile_geometry_call` uses to resolve Ident references.
fn collect_geometry_exprs<'a>(
    members: &'a [reify_syntax::MemberDecl],
    known: &HashSet<&str>,
    functions: &[CompiledFunction],
    out: &mut HashMap<&'a str, &'a reify_syntax::Expr>,
) {
    for m in members {
        match m {
            reify_syntax::MemberDecl::Let(let_decl)
                if is_geometry_let(&let_decl.value, functions, known) =>
            {
                out.insert(let_decl.name.as_str(), &let_decl.value);
            }
            reify_syntax::MemberDecl::Param(param) if known.contains(param.name.as_str()) => {
                if let Some(e) = &param.default {
                    out.insert(param.name.as_str(), e);
                }
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                collect_geometry_exprs(&g.members, known, functions, out);
                collect_geometry_exprs(&g.else_members, known, functions, out);
            }
            _ => {}
        }
    }
}

/// Read-only dependencies for [`emit_guarded_geometry_realizations`].
///
/// Separating immutable inputs from mutable outputs (`GeometryRealizationSink`)
/// keeps the lifetime on each half independent, so a future field that borrows
/// from `realizations` won't fight the `'a` shared by the whole context.
struct GeometryRealizationDeps<'a> {
    entity_name: &'a str,
    scope: &'a CompilationScope<'a>,
    enum_defs: &'a [reify_types::EnumDef],
    functions: &'a [CompiledFunction],
    known_geometry_lets: &'a HashSet<&'a str>,
    geometry_lets: &'a HashMap<&'a str, &'a reify_syntax::Expr>,
}

/// Mutable output sinks for [`emit_guarded_geometry_realizations`].
struct GeometryRealizationSink<'a> {
    realizations: &'a mut Vec<RealizationDecl>,
    realization_index: &'a mut u32,
    diagnostics: &'a mut Vec<Diagnostic>,
}

/// Recursively emit `RealizationDecl`s for Solid-typed geometry params inside
/// guarded groups at any nesting depth.
///
/// This is the recursive counterpart to the `GuardedGroup` arm of the third-pass
/// main loop in `compile_entity`. It handles Params (whose names are in
/// `deps.known_geometry_lets`) and descends into nested GuardedGroup members.
///
/// Intentionally does NOT handle Lets — guarded geometry lets do not emit
/// realizations (that is a separate, unimplemented feature; see the existing
/// comment in the GuardedGroup arm of the third-pass loop).
fn emit_guarded_geometry_realizations(
    members: &[reify_syntax::MemberDecl],
    deps: &GeometryRealizationDeps<'_>,
    sink: &mut GeometryRealizationSink<'_>,
) {
    for m in members {
        match m {
            reify_syntax::MemberDecl::Param(param)
                if deps.known_geometry_lets.contains(param.name.as_str()) =>
            {
                if let Some(default_expr) = &param.default
                    && let Some(ops) = compile_geometry_call(
                        default_expr,
                        deps.scope,
                        deps.enum_defs,
                        deps.functions,
                        sink.diagnostics,
                        0,
                        deps.geometry_lets,
                        &mut HashSet::new(),
                    )
                {
                    let feature_tags = derive_feature_tags(&ops, param.span);
                    sink.realizations.push(RealizationDecl {
                        id: RealizationNodeId::new(deps.entity_name, *sink.realization_index),
                        name: Some(param.name.clone()),
                        feature_tags,
                        operations: ops,
                        span: param.span,
                    });
                    *sink.realization_index += 1;
                }
            }
            reify_syntax::MemberDecl::GuardedGroup(g) => {
                emit_guarded_geometry_realizations(&g.members, deps, sink);
                emit_guarded_geometry_realizations(&g.else_members, deps, sink);
            }
            _ => {}
        }
    }
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

// ---------------------------------------------------------------------------
// OptionNone fixup helpers (shared with guards.rs via `pub(crate) use entity::*`)
// ---------------------------------------------------------------------------

/// Fix up a compiled default expression for a param member.
///
/// When the expression is `none` and the declared param type is `Option<T>`,
/// the parser produces a fallback `Option<Real>` type. This helper overrides
/// that type with the correct `Option<T>` declared by the annotation.
///
/// Used in three places: top-level entity params (entity.rs), port member
/// params (entity.rs), and guarded member params (guards.rs).
pub(crate) fn fixup_option_none_for_param(compiled: &mut CompiledExpr, cell_type: &Type) {
    if matches!(&compiled.kind, CompiledExprKind::OptionNone)
        && matches!(cell_type, Type::Option(_))
    {
        *compiled = CompiledExpr::option_none(cell_type.clone());
    }
}

/// Fix up a compiled value expression for a let member.
///
/// When the expression is `none` and the let has a typed annotation like
/// `Option<T>`, the parser produces a fallback `Option<Real>` type. This
/// helper resolves the annotation and overrides the type with the correct
/// `Option<T>`.
///
/// Used in three places: top-level entity lets (entity.rs), port member
/// lets (entity.rs), and guarded member lets (guards.rs).
pub(crate) fn fixup_option_none_for_let(
    compiled_expr: &mut CompiledExpr,
    type_expr: Option<&reify_syntax::TypeExpr>,
    type_param_names: &HashSet<String>,
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if matches!(&compiled_expr.kind, CompiledExprKind::OptionNone)
        && let Some(te) = type_expr
        && let Some(resolved) = resolve_type_expr_with_aliases(
            te,
            type_param_names,
            alias_registry,
            diagnostics,
            structure_names,
            trait_names,
        )
        && matches!(&resolved, Type::Option(_))
    {
        *compiled_expr = CompiledExpr::option_none(resolved);
    }
}
