use super::*;
use crate::compile_builder::hash::hash_pragma;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;

/// Reserved prefix used to mint the synthetic `Type::TypeParam` placeholder
/// for an `auto:` / `auto(free):` type-argument slot (see the two
/// `MemberDecl::Sub` lowering arms below). The placeholder lives in the same
/// `Type::TypeParam(_)` namespace as user-declared type-params and is
/// transparently skipped by `check_type_param_bounds`, so a user-declared
/// type-param sharing this prefix could mask the bound-check at the wrong
/// site. `compile_entity` rejects this prefix at the declaration site (see
/// `AutoTypeParamReservedPrefix` diagnostic) so the namespaces stay disjoint.
pub(crate) const AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX: &str = "__auto_";

/// Shared reference to entity definition fields (used by both StructureDef and OccurrenceDef).
pub(crate) struct EntityDefRef<'a> {
    pub(crate) name: &'a str,
    /// Documentation comment from the source `///` lines preceding the declaration.
    /// Mirrors `StructureDef::doc` / `OccurrenceDef::doc` and is forwarded to
    /// `TopologyTemplate::doc` by `compile_entity`.
    pub(crate) doc: Option<String>,
    pub(crate) is_pub: bool,
    pub(crate) type_params: &'a [reify_ast::TypeParamDecl],
    pub(crate) trait_bounds: &'a [reify_ast::TraitBoundRef],
    pub(crate) members: &'a [reify_ast::MemberDecl],
    pub(crate) annotations: &'a [reify_ast::Annotation],
    pub(crate) pragmas: &'a [reify_ast::Pragma],
    pub(crate) span: SourceSpan,
}

impl<'a> From<&'a reify_ast::StructureDef> for EntityDefRef<'a> {
    fn from(s: &'a reify_ast::StructureDef) -> Self {
        EntityDefRef {
            name: &s.name,
            doc: s.doc.clone(),
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

impl<'a> From<&'a reify_ast::OccurrenceDef> for EntityDefRef<'a> {
    fn from(o: &'a reify_ast::OccurrenceDef) -> Self {
        EntityDefRef {
            name: &o.name,
            doc: o.doc.clone(),
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
    expr: &reify_ast::Expr,
    bindings: &HashMap<String, reify_ast::Expr>,
) -> reify_ast::Expr {
    use reify_ast::{Expr, ExprKind, MatchArm};
    let span = expr.span;
    let new_kind = match &expr.kind {
        // Leaf variants — no sub-expressions to recurse into.
        ExprKind::NumberLiteral { value, is_real } => ExprKind::NumberLiteral {
            value: *value,
            is_real: *is_real,
        },
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
        ExprKind::TraitMethodCall {
            object,
            trait_name,
            method,
            args,
        } => ExprKind::TraitMethodCall {
            object: Box::new(substitute_expr(object, bindings)),
            trait_name: trait_name.clone(),
            method: method.clone(),
            args: args.iter().map(|a| substitute_expr(a, bindings)).collect(),
        },
        ExprKind::TraitStaticCall {
            trait_name,
            method,
            args,
        } => ExprKind::TraitStaticCall {
            trait_name: trait_name.clone(),
            method: method.clone(),
            args: args.iter().map(|a| substitute_expr(a, bindings)).collect(),
        },
        ExprKind::VariantConstruct { name, fields } => ExprKind::VariantConstruct {
            name: name.clone(),
            fields: fields
                .iter()
                .map(|(f, v)| (f.clone(), substitute_expr(v, bindings)))
                .collect(),
        },
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
/// Emit the duplicate-match-arm-cluster diagnostic.
///
/// Shared by the pre-pass (when the first cluster with this name was suppressed by an
/// outside-match collision so `scope.match_arm_groups` never has the entry and pass 2
/// cannot detect the duplicate) and pass 2 (the normal case) to keep the message
/// wording and label structure identical across both emission sites.
fn emit_duplicate_match_arm_cluster(
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    span: SourceSpan,
) {
    diagnostics.push(
        Diagnostic::error(format!(
            "duplicate match-arm cluster name '{}' — two match blocks declare \
             the same logical name in this structure",
            name
        ))
        .with_label(DiagnosticLabel::new(span, "duplicate cluster")),
    );
}

/// Emit the outside-match-collision diagnostic and record the cluster name in
/// `collisions`.
///
/// Shared by the forward-direction check (MatchArmDeclGroup pre-pass arm) and the
/// three reverse-direction checks (Param/Let/Sub pre-pass arms) to keep the message
/// wording and two-label structure identical across all call sites.
fn emit_outside_match_collision(
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    cluster_span: SourceSpan,
    outside_span: SourceSpan,
    collisions: &mut HashSet<String>,
) {
    diagnostics.push(
        Diagnostic::error(format!(
            "match-arm cluster '{}' collides with a declaration of the same name \
             outside the match block",
            name
        ))
        .with_label(DiagnosticLabel::new(cluster_span, "cluster declared here"))
        .with_label(DiagnosticLabel::new(
            outside_span,
            "originally declared outside the match",
        )),
    );
    collisions.insert(name.to_string());
}

/// Detect the E_OBJECTIVE_CONFLICT case (PRD §3.3/§6.3, task 4010).
///
/// Returns `Some(Diagnostic)` iff all of the following hold:
/// - `obj.combination == WeightedSum`
/// - `obj.terms.len() > 1`
/// - every term has default `weight == 1.0` and `priority == 0`
/// - at least one pair of terms has **opposite sense** (`Minimize` vs `Maximize`)
///   over **distinct expressions** (compared by `CompiledExpr.content_hash`)
///
/// Correctly excluded cases:
/// - Two same-sense terms (`minimize + minimize`) — no opposite-sense pair.
/// - Single objective — `terms.len() == 1`.
/// - Mixed-sense over the **same** expression — `content_hash` equality.
/// - `Lexicographic` combination — not yet source-reachable per PRD §5.
///
/// The returned diagnostic embeds `"E_OBJECTIVE_CONFLICT"` as the message prefix
/// (for CLI surfacing via `"{severity}: {message}"`) and attaches
/// `DiagnosticCode::ObjectiveConflict` for structured LSP/MCP consumers.
fn check_objective_conflict(
    obj: &ObjectiveSet,
    spans: &[SourceSpan],
    entity_name: &str,
) -> Option<Diagnostic> {
    use reify_ir::{ObjectiveCombination, ObjectiveSense};

    // Guard: WeightedSum only (Lexicographic is out of scope per §5).
    if obj.combination != ObjectiveCombination::WeightedSum {
        return None;
    }
    // Guard: more than one term.
    if obj.terms.len() <= 1 {
        return None;
    }
    // Guard: all terms at default weight and priority.
    if !obj.terms.iter().all(|t| t.weight == 1.0 && t.priority == 0) {
        return None;
    }
    // Detect: a pair of terms with opposite sense and distinct content_hash.
    let has_minimize = obj.terms.iter().any(|t| t.sense == ObjectiveSense::Minimize);
    let has_maximize = obj.terms.iter().any(|t| t.sense == ObjectiveSense::Maximize);
    if !has_minimize || !has_maximize {
        return None;
    }
    // Check that some minimize and maximize pair are over distinct expressions.
    let distinct_pair = obj.terms.iter().any(|a| {
        obj.terms.iter().any(|b| {
            a.sense == ObjectiveSense::Minimize
                && b.sense == ObjectiveSense::Maximize
                && a.expr.content_hash != b.expr.content_hash
        })
    });
    if !distinct_pair {
        return None;
    }

    // Build the diagnostic.  Pick the first two spans (Minimize and Maximize)
    // for the labels; spans is parallel to terms, so index 0 is the first term.
    // The convention (diagnostic_coverage_checkpoint.rs) requires ≥1 label with
    // a non-empty span.
    let mut diag = Diagnostic::error(format!(
        "E_OBJECTIVE_CONFLICT: entity '{}' has conflicting unweighted objectives \
         (minimize and maximize over distinct expressions with default weight/priority). \
         To resolve, choose one of: \
         (1) assign non-default weight values to distinguish objective importance; \
         (2) assign non-default priority values to lexicographically order objectives; \
         (3) combine objectives into a single expression before minimize/maximize.",
        entity_name
    ))
    .with_code(DiagnosticCode::ObjectiveConflict);

    // Attach spans for all terms (parallel to `spans`), labelling each.
    for (i, &span) in spans.iter().enumerate() {
        if span.len() > 0 {
            let label_msg = if i < obj.terms.len() {
                match obj.terms[i].sense {
                    ObjectiveSense::Minimize => "minimize objective declared here",
                    ObjectiveSense::Maximize => "maximize objective declared here",
                }
            } else {
                "conflicting objective declared here"
            };
            diag = diag.with_label(DiagnosticLabel::new(span, label_msg));
        }
    }

    // Ensure at least one label even if all spans are empty (defensive; should
    // not happen for well-formed AST).
    if diag.labels.is_empty() {
        if let Some(&span) = spans.first() {
            diag = diag.with_label(DiagnosticLabel::new(span, "conflicting objective declared here"));
        }
    }

    Some(diag)
}

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
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    trait_registry: &HashMap<String, &CompiledTrait>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    field_registry: &HashMap<String, &CompiledField>,
    constraint_def_registry: &HashMap<String, &CompiledConstraintDef>,
    unit_registry: &UnitRegistry,
    alias_registry: &TypeAliasRegistry,
    pending_bound_checks: &mut Vec<PendingBoundCheck>,
    pending_auto_resolutions: &mut Vec<AutoResolutionRequest>,
    pending_sub_override_autos: &mut Vec<PendingSubOverrideAuto>,
    diagnostics: &mut Vec<Diagnostic>,
    compiled_templates: &[TopologyTemplate],
    prelude_template_registry: &HashMap<String, &TopologyTemplate>,
) -> TopologyTemplate {
    let entity_name = structure.name;
    // task 3540 (SIR-α): make `structure def` templates reachable at the
    // expression-lowering site so `Foo()` can lower to a
    // `StructureInstanceCtor` (esc-3540-177 RULING 1). Composition: prelude
    // structure-defs first, then local already-compiled structure-defs
    // (later entries shadow earlier — local definitions shadow prelude,
    // matching Reify scoping). Declared BEFORE `scope` so it outlives the
    // scope's borrow (drop order is reverse-declaration).
    let entity_template_registry: HashMap<String, &TopologyTemplate> = prelude_template_registry
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .chain(
            compiled_templates
                .iter()
                .filter(|t| t.entity_kind == EntityKind::Structure)
                .map(|t| (t.name.clone(), t)),
        )
        .collect();
    let mut scope = CompilationScope::new(entity_name);
    scope.set_unit_registry(unit_registry);
    scope.is_entity_scope = true;
    scope.set_template_registry(&entity_template_registry);

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
    let mut objective_terms: Vec<ObjectiveTerm> = Vec::new();
    let mut objective_spans: Vec<SourceSpan> = Vec::new();
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

    // Reject user-declared type-params whose name collides with the
    // `__auto_` synthetic-placeholder prefix. The placeholder lives in the
    // same `Type::TypeParam(_)` namespace as user-declared type-params and is
    // skipped by `check_type_param_bounds`, so a user-named `__auto_Seal`
    // could silently mask a bound check at the wrong site (the collision
    // realistically never occurs, but the prefix is otherwise unreserved
    // anywhere in the language). Reserving it here keeps the two namespaces
    // disjoint without requiring a new `Type` variant in `reify-core`.
    for tp in structure.type_params.iter() {
        if tp.name.starts_with(AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX) {
            diagnostics.push(
                Diagnostic::error(format!(
                    "type-parameter name '{}' is reserved: the '{}' prefix is \
                     used by the compiler for `auto:` type-argument \
                     placeholders and must not appear in user-declared \
                     type-parameter names",
                    tp.name, AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX
                ))
                .with_label(DiagnosticLabel::new(
                    tp.span,
                    format!(
                        "rename this type-parameter to avoid the reserved '{}' prefix",
                        AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX
                    ),
                )),
            );
        }
    }

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
    // Tracks cluster logical names already registered in this pre-pass so that a
    // second MatchArmDeclGroup with the same logical name is skipped wholesale.
    // Mirrors the dup-cluster check in compile_match_arm_decl_group (entity.rs:2038)
    // which keeps the first registration and rejects later same-name clusters.
    // Without this guard the second cluster's pre-pass would overwrite
    // scope.sub_component_types / scope.sub_structure_traits / scope.sub_member_types
    // with the rejected cluster's child-template members — causing spurious
    // "unknown member" diagnostics on qualified access to the first cluster's sub.
    // (task 2613)
    let mut seen_match_arm_cluster_names: HashSet<String> = HashSet::new();
    // Logical name → span for each match-arm cluster accepted in the pre-pass.
    // Populated in the MatchArmDeclGroup arm so the reverse-direction check
    // (Sub/Param/Let AFTER the match) can emit a two-label collision diagnostic.
    // (task 2375)
    let mut match_arm_cluster_logical_names: HashMap<String, SourceSpan> = HashMap::new();
    // Cluster logical names that collide with an outside-of-match declaration.
    // Populated in both directions (forward + reverse). Plumbed into
    // compile_match_arm_decl_group (pass 2) to suppress cluster registration
    // when the collision was already diagnosed in the pre-pass. (task 2375, step-10)
    let mut clusters_with_outside_collision: HashSet<String> = HashSet::new();
    // Span recorded for every top-level `MemberDecl::Param`, `MemberDecl::Let`,
    // and `MemberDecl::Sub` at pre-pass time, keyed by decl name. Used to supply
    // the second DiagnosticLabel when a collision is detected in either direction
    // (task 2375). First-decl-wins: `entry().or_insert()` is used at all three
    // write sites so a duplicate decl name never silently moves the anchor span.
    //
    // SCOPE NOTE (task 2877, option b): This map intentionally covers ONLY the
    // three top-level member kinds above. Names registered through
    // `register_guarded_names` (guards.rs:128) for `MemberDecl::GuardedGroup`
    // children are NOT tracked here, so a `where g { param head … } else { … }`
    // whose name matches a match-cluster's logical name will not produce a
    // collision diagnostic. This is intentional scoping — extending coverage to
    // guarded-group children requires plumbing additional state into
    // `register_guarded_names` and is left for a future task (option a).
    let mut outside_decl_spans: HashMap<String, SourceSpan> = HashMap::new();
    for member in structure.members {
        match member {
            reify_ast::MemberDecl::Param(param) => {
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
                            if let reify_ast::TypeExprKind::Named { name, type_args } =
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
                                        .with_code(DiagnosticCode::UnresolvedType)
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
                // (is_solid_geometry_param inlined here — retired in GHR-γ, task 3605)
                if ty == Type::Geometry
                    && param
                        .default
                        .as_ref()
                        .map(|e| is_geometry_let(e, functions, &known_geometry_lets))
                        .unwrap_or(false)
                {
                    scope.has_geometry = true;
                    known_geometry_lets.insert(param.name.as_str());
                }
                scope.register(&param.name, ty);
                outside_decl_spans
                    .entry(param.name.clone())
                    .or_insert(param.span);
                // Reverse-direction collision (task 2375): if this Param's name
                // was already registered as a match-arm cluster, emit the collision
                // diagnostic. The cluster is suppressed; the Param is kept.
                if let Some(&cluster_span) = match_arm_cluster_logical_names.get(&param.name) {
                    emit_outside_match_collision(
                        diagnostics,
                        &param.name,
                        cluster_span,
                        param.span,
                        &mut clusters_with_outside_collision,
                    );
                }
            }
            reify_ast::MemberDecl::Let(let_decl) => {
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
                outside_decl_spans
                    .entry(let_decl.name.clone())
                    .or_insert(let_decl.span);
                // Reverse-direction collision (task 2375): if this Let's name
                // was already registered as a match-arm cluster, emit the collision
                // diagnostic. The cluster is suppressed; the Let is kept.
                if let Some(&cluster_span) = match_arm_cluster_logical_names.get(&let_decl.name) {
                    emit_outside_match_collision(
                        diagnostics,
                        &let_decl.name,
                        cluster_span,
                        let_decl.span,
                        &mut clusters_with_outside_collision,
                    );
                }
            }
            reify_ast::MemberDecl::GuardedGroup(g) => {
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
            reify_ast::MemberDecl::MatchArmDeclGroup(m) => {
                // Pre-pass: register per-arm member names so that the main pass
                // and any forward references can resolve them.
                // Sub-component type entries (used for `self.sub.member` qualified
                // access) are registered per-arm. Clusters with duplicate logical
                // names are skipped wholesale — the duplicate-cluster diagnostic is
                // emitted by compile_match_arm_decl_group in pass 2; skipping the
                // pre-pass prevents scope.sub_component_types,
                // scope.sub_structure_traits, and scope.sub_member_types from being
                // overwritten by the rejected cluster's child-template members.
                // "First cluster wins" in the pre-pass is symmetric with "first
                // cluster wins" in pass 2. (task 2613)

                // Compute logical name once; shared by duplicate-check and
                // collision-check below. When None (unsupported arm kind), both
                // checks are bypassed — pass 2 diagnoses unsupported member kinds.
                let maybe_logical_name = m.arms.first().and_then(|a| arm_member_name(&a.member));

                if let Some(logical_name) = maybe_logical_name {
                    // Duplicate cluster — skip pre-pass; pass 2 normally emits the
                    // diagnostic via scope.match_arm_groups.contains_key(...).
                    // Precedence: duplicate-check fires before collision-check when
                    // both apply — the first cluster with this name is canonical.
                    //
                    // Exception: if the *first* cluster with this name was suppressed
                    // by an outside-match collision (clusters_with_outside_collision),
                    // scope.match_arm_groups will never have the entry, so pass 2
                    // won't be able to detect the duplicate. Emit the duplicate-cluster
                    // diagnostic here in the pre-pass instead. (task 2376, step-2)
                    if !seen_match_arm_cluster_names.insert(logical_name.to_string()) {
                        if clusters_with_outside_collision.contains(logical_name) {
                            // The first cluster with this name was suppressed by an
                            // outside-match collision, so scope.match_arm_groups will
                            // never have the entry and pass 2 cannot detect the
                            // duplicate. Emit here in the pre-pass instead.
                            emit_duplicate_match_arm_cluster(diagnostics, logical_name, m.span);
                        }
                        // If the first cluster was NOT collision-suppressed, pass 2
                        // detects the duplicate via scope.match_arm_groups.contains_key
                        // (see compile_match_arm_decl_group) and emits the diagnostic
                        // there — so no pre-pass emission is needed in that branch.
                        continue;
                    }

                    // Forward-direction outside-match collision detection (task 2375).
                    // If the cluster's logical name is already registered as a regular
                    // Sub/Param/Let (recorded in outside_decl_spans), emit a collision
                    // diagnostic and skip this cluster's pre-pass registration entirely.
                    // The outside decl wins; the cluster is suppressed.
                    if let Some(&outside_span) = outside_decl_spans.get(logical_name) {
                        emit_outside_match_collision(
                            diagnostics,
                            logical_name,
                            m.span,
                            outside_span,
                            &mut clusters_with_outside_collision,
                        );
                        continue;
                    }

                    // No collision — record this cluster for reverse-direction checks
                    // (Sub/Param/Let declared AFTER this match block in source order).
                    match_arm_cluster_logical_names.insert(logical_name.to_string(), m.span);
                }

                for arm in &m.arms {
                    match &*arm.member {
                        reify_ast::MemberDecl::Sub(sub) => {
                            scope
                                .sub_component_types
                                .insert(sub.name.clone(), sub.structure_name.clone());
                            // Populate sub_structure_traits and sub_member_types so that
                            // `self.<arm-sub>.<member>` qualified access resolves correctly —
                            // mirrors the regular Sub pre-pass at entity.rs:594-602.
                            // (Suggestion 3 from review: match regular Sub pre-pass.)
                            //
                            // Note: per-arm member-type tracking for match_arm_group_arm_member_types
                            // (task 2373) is now handled inside compile_match_arm_decl_group,
                            // atomically with register_match_arm_group — moved from here in task 2872
                            // to close the orphan-entry bug on any early-return rejection path.
                            if let Some(child_tmpl) =
                                find_template(compiled_templates, &sub.structure_name)
                            {
                                scope.sub_structure_traits.insert(
                                    sub.structure_name.clone(),
                                    child_tmpl.trait_bounds.clone(),
                                );
                                scope.sub_member_types.insert(
                                    sub.name.clone(),
                                    member_type_map_from_template(child_tmpl),
                                );
                                // Populate sub_realization_names for cross-sub geometry diagnostic.
                                scope.sub_realization_names.insert(
                                    sub.name.clone(),
                                    realization_name_set_from_template(child_tmpl),
                                );
                            }
                        }
                        other => {
                            // suggestion 6: only 'sub' arms are supported in task 2372.
                            // Param/Let arms are explicitly rejected here so they are never
                            // inserted into scope.names — preserving the cluster-isolation
                            // invariant that prevents task 2375's dup-name diagnostics from
                            // misfiring. Future tasks may lift this restriction.
                            let member_label = match arm_member_name(other) {
                                Some(n) => format!("'{}'", n),
                                None => "this member".to_string(),
                            };
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "only 'sub' declarations are supported inside match arms; \
                                     {} is not yet supported",
                                    member_label
                                ))
                                .with_label(DiagnosticLabel::new(
                                    arm.span,
                                    "unsupported arm member kind",
                                )),
                            );
                        }
                    }
                }
            }
            reify_ast::MemberDecl::Port(port_decl) => {
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
                        reify_ast::MemberDecl::Param(param) => {
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
                                        .with_code(DiagnosticCode::UnresolvedType)
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
                        reify_ast::MemberDecl::Let(let_decl) => {
                            let composite_name = format!("{}.{}", port_decl.name, let_decl.name);
                            let id = ValueCellId::new(entity_name, &composite_name);
                            scope.names.insert(composite_name, (id, Type::Real, None));
                        }
                        _ => {}
                    }
                }
            }
            reify_ast::MemberDecl::Sub(sub) => {
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
                    // After GHR-γ (task 3605): geometry-typed params now appear in
                    // member_type_map_from_template (they have ValueCellDecls), so
                    // non-collection sub access to a Solid param resolves via the normal
                    // ValueRef path.  See member_type_map_from_template for details.
                    scope
                        .sub_member_types
                        .insert(sub.name.clone(), member_type_map_from_template(child_tmpl));
                    // Populate sub_realization_names for cross-sub geometry diagnostic.
                    scope.sub_realization_names.insert(
                        sub.name.clone(),
                        realization_name_set_from_template(child_tmpl),
                    );
                    // External-scope match-arm cluster pre-pass (task 2373):
                    // copy each cluster from the child template along with
                    // per-arm member maps so that `<sub>.<cluster>.<inner>`
                    // can typecheck from outside without re-resolving
                    // compiled_templates.
                    if !child_tmpl.match_arm_groups.is_empty() {
                        let mut clusters: Vec<SubClusterEntry> = Vec::new();
                        for group in &child_tmpl.match_arm_groups {
                            let mut per_arm: Vec<ArmMemberMap> =
                                Vec::with_capacity(group.arms.len());
                            for arm in &group.arms {
                                if let Type::StructureRef(arm_struct) = &arm.arm_type {
                                    let arm_members: BTreeMap<String, Type> =
                                        find_template(compiled_templates, arm_struct)
                                            .map(|t| {
                                                t.value_cells
                                                    .iter()
                                                    .map(|vc| {
                                                        (vc.id.member.clone(), vc.cell_type.clone())
                                                    })
                                                    .collect()
                                            })
                                            .unwrap_or_default();
                                    per_arm.push((arm_struct.clone(), arm_members));
                                } else {
                                    // Non-StructureRef arm types are not produced in v0.1;
                                    // fall back to an empty entry preserving arm-index alignment.
                                    per_arm.push((String::new(), BTreeMap::new()));
                                }
                            }
                            clusters.push((group.clone(), per_arm));
                        }
                        scope
                            .sub_match_arm_groups
                            .insert(sub.name.clone(), clusters);
                    }
                }
                if sub.is_collection {
                    scope.collection_sub_names.insert(sub.name.clone());
                }
                outside_decl_spans
                    .entry(sub.name.clone())
                    .or_insert(sub.span);
                // Reverse-direction collision (task 2375): if this Sub's name
                // was already registered as a match-arm cluster, emit the collision
                // diagnostic. The cluster is suppressed; the Sub is kept.
                if let Some(&cluster_span) = match_arm_cluster_logical_names.get(&sub.name) {
                    emit_outside_match_collision(
                        diagnostics,
                        &sub.name,
                        cluster_span,
                        sub.span,
                        &mut clusters_with_outside_collision,
                    );
                }
            }
            reify_ast::MemberDecl::MetaBlock(meta) => {
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

    // task 3939 δ (step-12): accumulates the override-or-injected-default
    // assoc-fn table resolved by conformance, stored onto this conformer's
    // TopologyTemplate below. Declared before the trait-bounds guard so it is
    // in scope at the struct literal (and stays empty for bound-less entities).
    let mut structure_assoc_fns: Vec<CompiledAssocFn> = Vec::new();
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
            &mut structure_assoc_fns,
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
            if crate::geometry_traits::is_geometry_marker_trait(&trait_bound.name) {
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
                        if let reify_ast::TypeExprKind::Named { name, .. } = &ta.kind {
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
    // Defer statement-form forall elaboration until after the main second-pass
    // loop completes — `sub_components` and `value_cells` (count cells included)
    // are populated in source order, but a `constraint <sub>.count == n` member
    // can appear before or after the `sub` declaration it pairs with (see
    // `compile_count_constraint_before_sub_declaration` in collection_sub_tests).
    // Processing forall in source order would race with that population.
    // Task 2364: per-element elaboration moved to a deferred sub-pass below.
    let mut pending_forall_constraint: Vec<&reify_ast::ForallConstraintDecl> = Vec::new();
    let mut pending_forall_connect: Vec<&reify_ast::ForallConnectDecl> = Vec::new();
    for member in structure.members {
        match member {
            reify_ast::MemberDecl::Param(param) => {
                let id = ValueCellId::new(entity_name, &param.name);
                let cell_type = scope
                    .resolve(&param.name)
                    .map(|(_, ty)| ty.clone())
                    .unwrap_or_else(|| {
                        emit_ice_unresolved(
                            UnresolvedKind::Name,
                            &param.name,
                            param.span,
                            diagnostics,
                        )
                    });

                let auto_free = param.default.as_ref().and_then(extract_auto_free);

                // Lower and validate annotations on this param
                let lowered_annotations = lower_annotations(&param.annotations, diagnostics);
                validate_annotations(&lowered_annotations, "param", diagnostics);
                let solver_hints = extract_solver_hints(&lowered_annotations, diagnostics);
                validate_solver_hint_collections(&solver_hints, &scope, functions, diagnostics);

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
                        let mut compiled =
                            compile_expr(expr, &scope, enum_defs, functions, diagnostics);
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
            reify_ast::MemberDecl::Let(let_decl) => {
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
                validate_solver_hint_collections(&solver_hints, &scope, functions, diagnostics);

                let decl = ValueCellDecl {
                    id,
                    kind: ValueCellKind::Let,
                    visibility,
                    is_aux: let_decl.is_aux,
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
            reify_ast::MemberDecl::Constraint(constraint) => {
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
                        is_aux: false,
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
            reify_ast::MemberDecl::Sub(sub) => {
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

                // Resolve type arguments to Type values. `auto:` / `auto(free):`
                // slots are lowered to a synthetic `Type::TypeParam("__auto_<bound>")`
                // placeholder (skipped by `check_type_param_bounds`) and recorded
                // as an `AutoClause`; the deferred `phase_auto_type_param_resolution`
                // rewrites the slot to a concrete `Type::StructureRef` once the
                // target template is reachable (task 3558).
                let mut auto_clauses: Vec<AutoClause> = Vec::new();
                let resolved_type_args: Vec<Type> = sub
                    .type_args
                    .iter()
                    .enumerate()
                    .map(|(position, ta)| match &ta.kind {
                        reify_ast::TypeExprKind::Named { name, .. } => {
                            resolve_type_name(name).unwrap_or_else(|| {
                                if type_param_names.contains(name) {
                                    Type::TypeParam(name.clone())
                                } else {
                                    Type::StructureRef(name.clone())
                                }
                            })
                        }
                        reify_ast::TypeExprKind::Auto { free, bound } => {
                            auto_clauses.push(AutoClause {
                                position,
                                free: *free,
                                bound: bound.clone(),
                                span: ta.span,
                            });
                            Type::TypeParam(format!(
                                "{}{}",
                                AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX, bound
                            ))
                        }
                        _ => {
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

                // Defer `auto:` resolution to the post-pass: one request per Sub
                // carrying every auto-clause, located by owner + sub-index so
                // the resolved candidate can be written back into the
                // placeholder slot. `sub_index` is the position at which the
                // matching `SubComponentDecl` is about to be pushed (the
                // following `sub_components.push(...)` is the only path that
                // grows the vec before the post-pass runs), so we capture
                // `sub_components.len()` here as the predicted index.
                if !auto_clauses.is_empty() {
                    pending_auto_resolutions.push(AutoResolutionRequest {
                        target_name: sub.structure_name.clone(),
                        auto_clauses,
                        owner_structure: entity_name.to_string(),
                        sub_index: sub_components.len(),
                    });
                }

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
                            GuardState::Compiled(Box::new(compiled))
                        }
                    }
                };

                // Compile the optional `at <pose>` clause.  For a collection
                // sub, carrying a pose is semantically invalid (per-element
                // placement is out of scope in v1, PRD §10): emit an error and
                // discard the pose so the IR is not left with a bad expression.
                // For a single sub, the compiled expression is stored as-is;
                // evaluation / type-checking as Transform is T4's responsibility.
                let pose = if sub.is_collection {
                    if let Some(pose_expr) = &sub.pose_expr {
                        diagnostics.push(
                            Diagnostic::error(
                                "'at' placement is not supported on collection subs; \
                                 per-element placement is out of scope in v1",
                            )
                            .with_code(DiagnosticCode::AtOnCollectionSub)
                            .with_label(DiagnosticLabel::new(
                                pose_expr.span,
                                "'at' not allowed on collection sub",
                            )),
                        );
                    }
                    None
                } else {
                    sub.pose_expr
                        .as_ref()
                        .map(|e| compile_expr(e, &scope, enum_defs, functions, diagnostics))
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
                    pose,
                    is_aux: sub.is_aux,
                    span: sub.span,
                    content_hash: sub.content_hash,
                });

                // Sub-instance auto overrides (task 3806, γ-slice):
                // For each `(name, expr)` in param_overrides where the value is `auto` /
                // `auto(free)`, push a scoped `ValueCellDecl { kind: Auto { free }, … }` into
                // the PARENT template's `value_cells` under id
                // `ValueCellId("<entity>.<sub>", "<member>")`.  This places the Auto cell in
                // the same per-template resolution problem as the parent's constraints, so the
                // existing M3 solver resolves it identically to a param-default `auto` cell
                // (the §4.4 invariant).  Non-auto overrides are carried in `param_overrides`
                // for future slices; the no-op here preserves the previous silent-discard
                // behaviour so there is no regression.
                for (override_name, override_expr) in &sub.param_overrides {
                    let Some(free) = extract_auto_free(override_expr) else {
                        continue;
                    };
                    // Three-case lookup (task 3806, step 10):
                    //
                    // Case 1 — forward-declared child: `sub_component_types` contains
                    //   the sub name (registered unconditionally at line ~836) but
                    //   `sub_member_types` does NOT (populated only when the child
                    //   template is already in `compiled_templates`).  Defer: push a
                    //   `PendingSubOverrideAuto` so the post-pass can re-resolve once
                    //   all templates are compiled.  Emit NO error here.
                    //
                    // Case 2 — child present but member absent: `sub_member_types`
                    //   has the child's map but the member name is not in it.  This is
                    //   a genuine "no such param" error — emit it now.
                    //
                    // Case 3 — child present and member found: push the scoped Auto
                    //   `ValueCellDecl` inline (original behavior from step 4).
                    match scope.sub_member_types.get(&sub.name) {
                        None => {
                            // Case 1: forward-declared child — defer.
                            pending_sub_override_autos.push(PendingSubOverrideAuto {
                                parent_entity_name: entity_name.to_string(),
                                sub_name: sub.name.clone(),
                                sub_structure_name: sub.structure_name.clone(),
                                override_member: override_name.clone(),
                                free,
                                span: override_expr.span,
                            });
                        }
                        Some(member_map) => match member_map.get(override_name) {
                            None => {
                                // Case 2: child compiled but member genuinely absent.
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "sub `{}`: override for `{}` — no such param in `{}`",
                                        sub.name, override_name, sub.structure_name
                                    ))
                                    .with_label(DiagnosticLabel::new(
                                        override_expr.span,
                                        "this member does not exist in the child structure",
                                    )),
                                );
                            }
                            Some(ty) => {
                                // Case 3: child compiled, member found — push inline.
                                let scoped_entity = format!("{}.{}", entity_name, sub.name);
                                let scoped_id =
                                    ValueCellId::new(&scoped_entity, override_name.as_str());
                                value_cells.push(ValueCellDecl {
                                    id: scoped_id,
                                    kind: ValueCellKind::Auto { free },
                                    visibility: Visibility::Public,
                                    cell_type: ty.clone(),
                                    default_expr: None,
                                    solver_hints: vec![],
                                    span: sub.span,
                                    // Auto sub-override cells are never aux declarations.
                                    is_aux: false,
                                });
                            }
                        },
                    }
                }
            }
            reify_ast::MemberDecl::Minimize(min_decl) => {
                let compiled_expr =
                    compile_expr(&min_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective_spans.push(min_decl.span);
                objective_terms.push(ObjectiveTerm::new(ObjectiveSense::Minimize, compiled_expr));
            }
            reify_ast::MemberDecl::Maximize(max_decl) => {
                let compiled_expr =
                    compile_expr(&max_decl.expr, &scope, enum_defs, functions, diagnostics);
                objective_spans.push(max_decl.span);
                objective_terms.push(ObjectiveTerm::new(ObjectiveSense::Maximize, compiled_expr));
            }
            reify_ast::MemberDecl::GuardedGroup(g) => {
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
            reify_ast::MemberDecl::AssociatedType(_) => {
                // Associated type compilation deferred to a later milestone.
            }
            reify_ast::MemberDecl::Fn(_) => {
                // task 3939 δ: structure-body assoc fns are recognized as
                // trait-fn overrides by `check_trait_conformance` (which scans
                // `structure.members` directly) and compiled into the conformer's
                // `assoc_fns` table. They lower to no value cell here, so this
                // member-compilation arm intentionally remains a no-op.
                // Instance dispatch (`self.member` resolution) is task ζ (3941).
            }
            reify_ast::MemberDecl::Port(port_decl) => {
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
                    .unwrap_or(reify_core::PortDirection::Bidi);

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
                        reify_ast::MemberDecl::Param(param) => {
                            let composite_name = format!("{}.{}", port_decl.name, param.name);
                            let id = ValueCellId::new(entity_name, &composite_name);
                            let cell_type = scope
                                .resolve(&composite_name)
                                .map(|(_, ty)| ty.clone())
                                .unwrap_or_else(|| {
                                    emit_ice_unresolved(
                                        UnresolvedKind::Name,
                                        &composite_name,
                                        param.span,
                                        diagnostics,
                                    )
                                });

                            let auto_free = param.default.as_ref().and_then(extract_auto_free);

                            let decl = if let Some(free) = auto_free {
                                ValueCellDecl {
                                    id,
                                    kind: ValueCellKind::Auto { free },
                                    visibility: Visibility::Public,
                                    is_aux: false,
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
                                    is_aux: false,
                                    cell_type,
                                    default_expr,
                                    solver_hints: Vec::new(),
                                    span: param.span,
                                }
                            };
                            port_members.push(decl);
                        }
                        reify_ast::MemberDecl::Let(let_decl) => {
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
                                // Propagate `is_aux` from the AST consistently with the
                                // structure-level Let path (entity.rs ~1179) and the guarded-member
                                // path (guards.rs ~469).  If `aux` turns out to be semantically
                                // invalid inside ports, the validator/T4 can emit a diagnostic
                                // rather than silently dropping the flag here.
                                is_aux: let_decl.is_aux,
                                cell_type,
                                default_expr: Some(compiled_expr),
                                solver_hints: Vec::new(),
                                span: let_decl.span,
                            });
                        }
                        reify_ast::MemberDecl::Constraint(constraint) => {
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
            reify_ast::MemberDecl::Connect(connect_decl) => {
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
            reify_ast::MemberDecl::Chain(chain_decl) => {
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
                            operator: reify_ast::ConnectOp::Forward,
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
            reify_ast::MemberDecl::MetaBlock(_) => {
                // Meta blocks are collected in the first pass; skip in second pass.
            }
            reify_ast::MemberDecl::ConstraintInst(ci) => {
                // Delegate to the shared expansion helper. The helper owns
                // every step (def lookup, arg validation, inst_idx allocation,
                // per-predicate substitution + emission, where-clause routing).
                // `label_suffix = None` preserves the original
                // `<name>#<inst_idx>[<pred_idx>]` label format for plain
                // instantiations; the forall instantiation branch in
                // `forall_elaborate.rs` passes `Some("forall@<var>[<i>]")`.
                expand_constraint_inst(
                    ci,
                    entity_name,
                    constraint_def_registry,
                    &mut scope,
                    enum_defs,
                    functions,
                    &mut constraints,
                    &mut constraint_index,
                    &mut constraint_inst_counts,
                    &mut guarded_groups,
                    &mut structure_controlling,
                    &mut guard_index,
                    diagnostics,
                    None,
                );
            }
            reify_ast::MemberDecl::ForallConnect(f) => {
                // Defer to the post-loop forall elaboration sub-pass — see the
                // `pending_forall_*` declarations above and the dispatch loop
                // after the main second pass. Sub-components and count cells
                // need to be fully populated before we can resolve element
                // counts (task 2364).
                pending_forall_connect.push(f);
            }
            reify_ast::MemberDecl::ForallConstraint(f) => {
                // Defer to the post-loop forall elaboration sub-pass (task 2364).
                pending_forall_constraint.push(f);
            }
            reify_ast::MemberDecl::MatchArmDeclGroup(m) => {
                // Compile each arm's guard and register a GuardedDeclGroup cluster
                // in the scope (task 2372, spec §6.4).  Clusters that collided with
                // an outside-of-match declaration are suppressed via
                // clusters_with_outside_collision (task 2375).
                compile_match_arm_decl_group(
                    entity_name,
                    m,
                    &mut scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    &mut guarded_groups,
                    &mut structure_controlling,
                    &mut guard_index,
                    &mut sub_components,
                    pending_bound_checks,
                    pending_auto_resolutions,
                    &type_param_names,
                    &clusters_with_outside_collision,
                    compiled_templates,
                );
            }
        }
    }

    // Deferred forall elaboration sub-pass (task 2364, spec §5.4).
    // Now that `sub_components` and `value_cells` (count cells included) are
    // fully populated, expand each `forall` statement-form into per-element
    // CompiledConstraints / CompiledConnections. Convert `constraint_inst_counts`
    // to a u32-valued map for the helpers; the existing `usize` map is kept for
    // the original ConstraintInst arm and will share the same counter once
    // step-14 refactors that arm into a shared helper.
    //
    // task 2629: when a `forall` over a deferred-count collection sub is
    // encountered, capture a `CompiledForallTemplate` here so the runtime
    // (engine_edit) can re-elaborate per-element constraints / connections
    // when the count cell becomes known. The capture is in addition to the
    // existing zero-emission compile-time silent-skip behaviour.
    let mut forall_templates_out: Vec<CompiledForallTemplate> = Vec::new();
    for f in &pending_forall_constraint {
        elaborate_forall_constraint(
            f,
            entity_name,
            &mut scope,
            enum_defs,
            functions,
            constraint_def_registry,
            &value_cells,
            &sub_components,
            &mut constraints,
            &mut constraint_index,
            &mut constraint_inst_counts,
            &mut guarded_groups,
            &mut structure_controlling,
            &mut guard_index,
            &mut forall_templates_out,
            diagnostics,
        );
    }
    for f in &pending_forall_connect {
        elaborate_forall_connect(
            f,
            entity_name,
            &ports,
            &scope,
            enum_defs,
            functions,
            trait_registry,
            &value_cells,
            &mut constraints,
            &mut constraint_index,
            &mut connections,
            &mut sub_components,
            &mut connector_index,
            &mut forall_templates_out,
            diagnostics,
        );
    }

    // Third pass: compile geometry let bindings into realizations.
    // Build a lookup table mapping geometry let/param names to their initializer AST
    // expressions. This allows compile_geometry_call to resolve Ident references
    // (let-bound geometry variables) used as arguments to boolean ops.
    // `collect_geometry_exprs` recurses fully into nested GuardedGroupDecl members
    // so geometry params at any nesting depth are captured.
    let geometry_lets: HashMap<&str, &reify_ast::Expr> = {
        let mut map = HashMap::new();
        collect_geometry_exprs(structure.members, &known_geometry_lets, functions, &mut map);
        map
    };

    let mut realizations = Vec::new();
    let mut realization_index: u32 = 0;

    for member in structure.members {
        match member {
            reify_ast::MemberDecl::Let(let_decl)
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
            reify_ast::MemberDecl::Param(param)
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
            reify_ast::MemberDecl::GuardedGroup(g) => {
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
            .any(|p| p.direction == reify_core::PortDirection::In);
        let has_out = ports
            .iter()
            .any(|p| p.direction == reify_core::PortDirection::Out);
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

    // Invariant: the key sets of match_arm_groups and match_arm_group_arm_member_types
    // must always be identical — every cluster registration must be paired with exactly
    // one per-arm-maps insertion, and vice versa. Any early-return in
    // compile_match_arm_decl_group (logical-name mismatch, unsupported discriminant,
    // non-exhaustive, outside-collision, etc.) that skips register_match_arm_group must
    // also skip inserting into match_arm_group_arm_member_types. (task 2872)
    assert!(
        {
            let groups = &scope.match_arm_groups;
            let per_arm = &scope.match_arm_group_arm_member_types;
            // Lengths-equal + every key in `groups` present in `per_arm` ⇒ same key set
            // (pigeonhole; HashMap forbids duplicate keys). Avoids the dual HashSet<&str>
            // allocation the symmetric-difference check would have performed every call.
            groups.len() == per_arm.len() && groups.keys().all(|k| per_arm.contains_key(k))
        },
        "match_arm_groups vs match_arm_group_arm_member_types key-set mismatch in entity '{}': \
         groups={:?} per_arm={:?} (orphan per-arm entries indicate a producer-side bug — task 2872)",
        entity_name,
        scope.match_arm_groups.keys().collect::<Vec<_>>(),
        scope
            .match_arm_group_arm_member_types
            .keys()
            .collect::<Vec<_>>(),
    );

    let objective = if objective_terms.is_empty() {
        None
    } else {
        let obj_set = ObjectiveSet { terms: objective_terms, combination: ObjectiveCombination::WeightedSum };
        if let Some(diag) = check_objective_conflict(&obj_set, &objective_spans, entity_name) {
            diagnostics.push(diag);
        }
        Some(obj_set)
    };

    TopologyTemplate {
        name: entity_name.to_string(),
        doc: structure.doc.clone(),
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
        // Expose the match-arm cluster map (task 2372 step-10).
        // Always present; production consumers wired in task 2373.
        // `match_arm_groups` is a `BTreeMap`, so `.values()` yields entries in
        // lexicographic key order — guaranteeing deterministic iteration of
        // `TopologyTemplate::match_arm_groups` across compiles.
        match_arm_groups: scope.match_arm_groups.values().cloned().collect(),
        // task 2629: per-element body templates captured by
        // `elaborate_forall_constraint` / `elaborate_forall_connect` for
        // statement-form `forall` over deferred-count collection subs.
        // Empty when no such forall exists.
        forall_templates: forall_templates_out,
        // task 3939 δ (step-12): the override-or-injected-default assoc-fn table
        // resolved by `check_trait_conformance` above. Deliberately excluded from
        // `content_hash` (see plan design decision); ζ (3941) looks this up by
        // (trait_name, fn_name) for `TraitMethodCall` dispatch.
        assoc_fns: structure_assoc_fns,
    }
}

/// Build the `member_name → Type` map for a child `TopologyTemplate`.
///
/// Called from both the pre-pass (to populate `scope.sub_member_types` for
/// `self.<arm-sub>.<member>` qualified access) and from `compile_match_arm_decl_group`
/// (to build `per_arm_member_maps` keyed under the cluster's logical name).
/// Extracting the logic here avoids duplicating the `value_cells` iteration at both
/// sites and ensures future changes to the mapping (e.g. filtering hidden members)
/// only need to be applied once. (task 2872)
fn member_type_map_from_template(tmpl: &TopologyTemplate) -> BTreeMap<String, Type> {
    // GHR-γ (task 3605): after bypass retirement, geometry-typed params
    // (`param x : Solid = <geom>`) now produce a ValueCellDecl AND are included
    // here.  For non-collection subs, this means `self.<sub>.<geom-param>`
    // resolves through the normal `ValueRef` path (Type::Geometry cell) rather
    // than the `CrossSubGeometryRef` bypass — the bypass only fires for geometry
    // LETS which remain realization-only (no ValueCellDecl) and therefore still
    // miss in `sub_member_types`.
    //
    // Note: for collection subs, including geometry cells means the "recommend
    // indexed access" diagnostic fires instead of the geometry-specific cross-sub
    // diagnostic for `self.<collection_sub>.<geom_param>` access.  The
    // geometry-specific collection-sub diagnostic tests are therefore `#[ignore]`
    // until GHR-δ+ provides a better routing strategy.  This is an accepted v0.1
    // limitation; the "recommend indexed access" message is still informative.
    tmpl.value_cells
        .iter()
        .map(|vc| (vc.id.member.clone(), vc.cell_type.clone()))
        .collect()
}

/// Collect the names of all named `RealizationDecl`s from a child `TopologyTemplate`.
///
/// Geometry-typed params (`param x : Solid = <geom>`) are lowered as BOTH a
/// `ValueCellDecl` (GHR-γ) AND a `RealizationDecl`.  Geometry lets
/// (`let x = box(...)`) are lowered as `RealizationDecl`s only (no value cell).
///
/// Geometry params appear in BOTH `member_type_map_from_template` output AND here.
/// Geometry lets appear here ONLY.  This lets `expr.rs` distinguish the two cases:
/// - Solid param: member IS in `sub_member_types` → `ValueRef` path (step-2+).
/// - Geometry let: member NOT in `sub_member_types` but IS in `sub_realization_names`
///   → `CrossSubGeometryRef` path → bypass warning (until step-4 retires the bypass).
/// - Genuinely missing: not in either map → "unknown member" error.
///
/// Called side-by-side with `member_type_map_from_template` in the two Sub
/// pre-pass sites (regular Sub at entity.rs ~line 766; match-arm Sub at ~line
/// 671), following the single-source-of-truth pattern of that helper.
fn realization_name_set_from_template(tmpl: &TopologyTemplate) -> BTreeSet<String> {
    tmpl.realizations
        .iter()
        .filter_map(|r| r.name.clone())
        .collect()
}

/// Compile a `MatchArmDeclGroupDecl` into a `GuardedDeclGroup` cluster (task 2372).
///
/// For each arm:
///   1. Synthesises a per-arm guard expression `discriminant == EnumType.Variant`
///      (or an OR of equality checks for `|`-pipe multi-pattern arms).
///   2. Allocates a synthetic `__guard_N` `ValueCellId` and registers it in
///      `structure_controlling` — identical bookkeeping to `compile_block_guard`.
///   3. Records the arm's declared type (`StructureRef` for `Sub` arms).
///   4. Compiles `Sub` arms directly into `sub_components` with a `GuardState::Compiled`
///      pointing to the per-arm guard, matching the `where` desugaring in spec §6.4.
///
/// After all arms are processed, builds a `GuardedDeclGroup { name, arms }` and
/// calls `scope.register_match_arm_group`.  Cluster names **never** route through
/// `scope.register()`, so same-name diagnostics from outside-match collisions
/// (emitted at pre-pass time) cannot misfire here — the cluster is suppressed via
/// `clusters_with_outside_collision` before any scope mutation.
#[allow(clippy::too_many_arguments)]
fn compile_match_arm_decl_group(
    entity_name: &str,
    m: &reify_ast::MatchArmDeclGroupDecl,
    scope: &mut CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
    sub_components: &mut Vec<SubComponentDecl>,
    pending_bound_checks: &mut Vec<PendingBoundCheck>,
    pending_auto_resolutions: &mut Vec<AutoResolutionRequest>,
    type_param_names: &HashSet<String>,
    clusters_with_outside_collision: &HashSet<String>,
    compiled_templates: &[TopologyTemplate],
) {
    // Resolve the discriminant's enum type.  Only simple `Ident` discriminants
    // are supported in this task; complex expressions are deferred to task 2373.
    let (discriminant_cell_id, enum_type_name) = match &m.discriminant.kind {
        reify_ast::ExprKind::Ident(name) => match scope.resolve(name) {
            Some((cell_id, Type::Enum(enum_name))) => (cell_id.clone(), enum_name.clone()),
            Some((_, other_ty)) => {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "match-arm discriminant '{}' has type {}, expected an enum",
                        name, other_ty
                    ))
                    .with_label(DiagnosticLabel::new(
                        m.discriminant.span,
                        "discriminant must be an enum-typed param or let",
                    )),
                );
                return;
            }
            None => {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "match-arm discriminant '{}' not found in scope",
                        name
                    ))
                    .with_label(DiagnosticLabel::new(
                        m.discriminant.span,
                        "unresolved identifier",
                    )),
                );
                return;
            }
        },
        _ => {
            diagnostics.push(
                Diagnostic::error(
                    "match-arm discriminant must be a simple identifier in this version",
                )
                .with_label(DiagnosticLabel::new(
                    m.discriminant.span,
                    "only identifier discriminants are supported (task 2373 extends this)",
                )),
            );
            return;
        }
    };

    // Extract the shared logical name from the first arm's member.
    let logical_name = match m.arms.first() {
        Some(arm) => match arm_member_name(&arm.member) {
            Some(n) => n.to_string(),
            None => {
                diagnostics.push(
                    Diagnostic::error(
                        "match-arm member must be a named declaration (param, let, or sub)",
                    )
                    .with_label(DiagnosticLabel::new(
                        m.span,
                        "unsupported member kind in arm",
                    )),
                );
                return;
            }
        },
        None => {
            // suggestion 5: explicit diagnostic for empty match block
            diagnostics.push(
                Diagnostic::error("match block must contain at least one arm")
                    .with_label(DiagnosticLabel::new(m.span, "empty match block")),
            );
            return;
        }
    };

    // Outside-match collision short-circuit (task 2375, step-10): if the pre-pass
    // already detected and diagnosed a collision between this cluster's logical name
    // and an outside-of-match Sub/Param/Let, suppress the cluster here — no need to
    // re-emit the diagnostic, and no partial cluster should be formed.
    if clusters_with_outside_collision.contains(&logical_name) {
        return;
    }

    // suggestion 4: validate that all subsequent arms share the same logical name
    for arm in &m.arms[1..] {
        match arm_member_name(&arm.member) {
            Some(name) if name == logical_name => {} // OK
            Some(name) => {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "all arms in a match block must declare the same logical name; \
                         expected '{}', found '{}'",
                        logical_name, name
                    ))
                    .with_label(DiagnosticLabel::new(arm.span, "mismatched arm name")),
                );
                return;
            }
            None => {
                diagnostics.push(
                    Diagnostic::error(
                        "match-arm member must be a named declaration (param, let, or sub)",
                    )
                    .with_label(DiagnosticLabel::new(
                        arm.span,
                        "unsupported member kind in arm",
                    )),
                );
                return;
            }
        }
    }

    // Guard against duplicate cluster names BEFORE the per-arm loop so that
    // sub_components and guarded_groups are never polluted with ghost entries from
    // the rejected cluster.
    if scope.match_arm_groups.contains_key(logical_name.as_str()) {
        emit_duplicate_match_arm_cluster(diagnostics, logical_name.as_str(), m.span);
        return;
    }

    let discriminant_ref =
        CompiledExpr::value_ref(discriminant_cell_id, Type::Enum(enum_type_name.clone()));

    // Validate every arm's pattern against the discriminant enum's variants
    // before compiling guards. A typo like `Hexx` would otherwise compile to a
    // guard that is unconditionally false, with no diagnostic emitted. We only
    // emit (we do not return), so that downstream compilation can continue and
    // surface follow-on issues in one pass.
    let known_enum_variants: Option<&[String]> = enum_defs
        .iter()
        .find(|e| e.name == enum_type_name)
        .map(|e| e.variants.as_slice());
    if let Some(variants) = known_enum_variants {
        for arm in &m.arms {
            for pat in &arm.patterns {
                if !variants.iter().any(|v| v == pat) {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "match-arm pattern '{}' is not a variant of enum '{}'",
                            pat, enum_type_name
                        ))
                        .with_label(DiagnosticLabel::new(arm.span, "unknown enum variant")),
                    );
                }
            }
        }

        // Exhaustiveness gate (task 2375): every variant of the discriminant enum
        // must be covered by at least one arm. Compute the covered set by
        // flattening ALL arms' patterns (so `Hex | Button => ...` contributes
        // both "Hex" and "Button"). No wildcard support needed here — decl-level
        // match arms only emit enum-ident patterns (unlike expr-level matches).
        let covered: std::collections::HashSet<&str> = m
            .arms
            .iter()
            .flat_map(|arm| arm.patterns.iter().map(|p| p.as_str()))
            .collect();
        let missing: Vec<&str> = variants
            .iter()
            .filter(|v| !covered.contains(v.as_str()))
            .map(|v| v.as_str())
            .collect();
        if !missing.is_empty() {
            diagnostics.push(
                Diagnostic::error(format!(
                    "non-exhaustive match on '{}': missing variant(s) {}",
                    enum_type_name,
                    missing
                        .iter()
                        .map(|v| format!("'{v}'"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
                .with_label(DiagnosticLabel::new(m.span, "missing variants")),
            );
            // Early-return: do NOT push sub_components/guarded_groups and do NOT
            // register the cluster. A partial cluster without all arms would have
            // subs registered without cluster-aware union typing — a footgun.
            return;
        }
    }

    let mut group_arms: Vec<GuardedDeclArm> = Vec::with_capacity(m.arms.len());
    // Per-arm member-type maps, collected in arm-order for Sub arms only.
    // Mirroring the group_arms cadence: both are populated for Sub arms exclusively.
    // Inserted into scope atomically with register_match_arm_group below (task 2872).
    let mut per_arm_member_maps: Vec<ArmMemberMap> = Vec::with_capacity(m.arms.len());

    for arm in &m.arms {
        // The pre-pass already emitted a diagnostic for any non-Sub arm.
        // Skip here to avoid a second misleading "could not resolve type" diagnostic
        // from arm_member_type attempting scope.resolve on an unregistered name.
        // (Suggestion 2 from review: suppress duplicate diagnostics for Param/Let arms.)
        if !matches!(&*arm.member, reify_ast::MemberDecl::Sub(_)) {
            continue;
        }

        // Synthesise the per-arm guard expression.
        // For a single pattern: `discriminant == EnumType.Variant`
        // For a pipe pattern:   `discriminant == V1 || discriminant == V2 || ...`
        let arm_guard_expr = build_arm_guard_expr(
            &discriminant_ref,
            &enum_type_name,
            &arm.patterns,
            arm.span,
            diagnostics,
        );

        // Allocate a synthetic guard ValueCell (mirrors compile_block_guard).
        let guard_cell_id = ValueCellId::new(entity_name, format!("__guard_{}", guard_index));
        *guard_index += 1;
        structure_controlling.insert(guard_cell_id.clone());

        // Record the arm type from the member declaration.
        let arm_type = arm_member_type(&arm.member, scope, diagnostics, arm.span);

        // Compile Sub members directly into sub_components with the per-arm guard.
        if let reify_ast::MemberDecl::Sub(sub) = &*arm.member {
            // suggestion 1: where_clause and body are not yet supported inside
            // match-arm subs — emit a diagnostic so users get explicit feedback
            // rather than silent data loss.
            if sub.where_clause.is_some() || sub.body.is_some() {
                diagnostics.push(
                    Diagnostic::error(
                        "where clauses and bodies are not yet supported \
                         in match-arm sub declarations",
                    )
                    .with_label(DiagnosticLabel::new(
                        arm.span,
                        "unsupported: where/body in match arm (future task)",
                    )),
                );
            }

            let compiled_args: Vec<(String, CompiledExpr)> = sub
                .args
                .iter()
                .map(|(name, expr)| {
                    (
                        name.clone(),
                        compile_expr(expr, scope, enum_defs, functions, diagnostics),
                    )
                })
                .collect();

            // suggestion 3: use .map() (not .filter_map()) so non-Named type-arg
            // entries emit a diagnostic and yield Type::Real, preserving positional
            // alignment for subsequent bound checks. `auto:` / `auto(free):` slots
            // mirror the non-arm Sub path (entity.rs): lowered to a synthetic
            // `Type::TypeParam("__auto_<bound>")` placeholder and recorded as an
            // `AutoClause` for the deferred `phase_auto_type_param_resolution`
            // (task 3558).
            let mut auto_clauses: Vec<AutoClause> = Vec::new();
            let resolved_type_args: Vec<Type> = sub
                .type_args
                .iter()
                .enumerate()
                .map(|(position, ta)| match &ta.kind {
                    reify_ast::TypeExprKind::Named { name, .. } => {
                        resolve_type_name(name).unwrap_or_else(|| {
                            if type_param_names.contains(name) {
                                Type::TypeParam(name.clone())
                            } else {
                                Type::StructureRef(name.clone())
                            }
                        })
                    }
                    reify_ast::TypeExprKind::Auto { free, bound } => {
                        auto_clauses.push(AutoClause {
                            position,
                            free: *free,
                            bound: bound.clone(),
                            span: ta.span,
                        });
                        Type::TypeParam(format!(
                            "{}{}",
                            AUTO_TYPE_PARAM_PLACEHOLDER_PREFIX, bound
                        ))
                    }
                    _ => {
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

            // Defer `auto:` resolution: one request per match-arm Sub, located
            // by owner + sub-index. Match-arm clusters reuse `sub_name`
            // across every arm (per the cluster's `logical_name` invariant),
            // so a name-only lookup in the post-pass would map every arm's
            // request to arm[0]'s `SubComponentDecl`, dropping every other
            // arm's resolved candidate. `sub_components.len()` here is the
            // index at which the matching `SubComponentDecl` is about to be
            // pushed (the following `sub_components.push(...)` is the only
            // path that grows the vec for this arm), so the index uniquely
            // identifies the right entry regardless of name collisions.
            if !auto_clauses.is_empty() {
                pending_auto_resolutions.push(AutoResolutionRequest {
                    target_name: sub.structure_name.clone(),
                    auto_clauses,
                    owner_structure: entity_name.to_string(),
                    sub_index: sub_components.len(),
                });
            }

            // suggestion 2: push one PendingBoundCheck::SubComponent + one
            // TraitArgConformance per arg, mirroring the non-arm Sub path
            // (entity.rs:984-1013) so trait-bound violations inside match arms
            // are caught in the post-compilation pass.
            pending_bound_checks.push(PendingBoundCheck::SubComponent {
                type_args: resolved_type_args.clone(),
                target_name: sub.structure_name.clone(),
                span: sub.span,
            });
            for ((_, arg_expr), (arg_name, compiled_arg)) in
                sub.args.iter().zip(compiled_args.iter())
            {
                pending_bound_checks.push(PendingBoundCheck::TraitArgConformance {
                    target_name: sub.structure_name.clone(),
                    arg_name: arg_name.clone(),
                    compiled_arg: compiled_arg.clone(),
                    span: arg_expr.span,
                });
            }

            // Mirror the collection+`at` rejection from the main sub-lowering path
            // (entity.rs ~1420) so the semantic rule is enforced uniformly.
            // Although the grammar currently hardcodes `is_collection: false` for
            // match-arm subs, the AST field may be true if the module was
            // hand-constructed; guarding on `sub.is_collection` is defensive.
            let pose = if sub.is_collection {
                if let Some(pose_expr) = &sub.pose_expr {
                    diagnostics.push(
                        Diagnostic::error(
                            "'at' placement is not supported on collection subs; \
                             per-element placement is out of scope in v1",
                        )
                        .with_code(DiagnosticCode::AtOnCollectionSub)
                        .with_label(DiagnosticLabel::new(
                            pose_expr.span,
                            "'at' not allowed on collection sub",
                        )),
                    );
                }
                None
            } else {
                sub.pose_expr
                    .as_ref()
                    .map(|e| compile_expr(e, scope, enum_defs, functions, diagnostics))
            };

            sub_components.push(SubComponentDecl {
                name: sub.name.clone(),
                structure_name: sub.structure_name.clone(),
                visibility: Visibility::Public,
                args: compiled_args,
                type_args: resolved_type_args,
                is_collection: false,
                count_cell: None,
                guard_state: GuardState::Compiled(Box::new(arm_guard_expr.clone())),
                pose,
                is_aux: sub.is_aux,
                span: sub.span,
                content_hash: sub.content_hash,
            });

            // Collect per-arm member types for match_arm_group_arm_member_types.
            // Always push an entry (empty BTreeMap when find_template fails) so the
            // per-arm Vec length always equals the cluster's Sub-arm count — preserving
            // the invariant that review suggestion 1 established in the pre-pass.
            // (task 2872: moved here from the pre-pass so insertion is atomic with
            // register_match_arm_group; see the if !group_arms.is_empty() block below.)
            let arm_member_types = find_template(compiled_templates, &sub.structure_name)
                .map(member_type_map_from_template)
                .unwrap_or_default();
            per_arm_member_maps.push((sub.structure_name.clone(), arm_member_types));
        }

        // Add an empty CompiledGuardedGroup so the guard cell participates in
        // the reference-safety sweep and guard_parent_map machinery.
        guarded_groups.push(CompiledGuardedGroup {
            guard_expr: arm_guard_expr.clone(),
            guard_value_cell: guard_cell_id.clone(),
            members: vec![],
            constraints: vec![],
            else_members: vec![],
            else_constraints: vec![],
            parent_guard: None,
        });

        group_arms.push(GuardedDeclArm {
            guard_expr: arm_guard_expr,
            guard_value_cell: guard_cell_id,
            arm_type,
        });
    }

    // Register the assembled cluster in the dedicated scope map.
    // If group_arms is empty, every arm was rejected (e.g. all Param/Let),
    // so no cluster should be exposed on TopologyTemplate::match_arm_groups —
    // an empty cluster is a latent footgun for downstream consumers (task 2373).
    if !group_arms.is_empty() {
        scope.register_match_arm_group(
            &logical_name,
            GuardedDeclGroup {
                name: logical_name.clone(),
                arms: group_arms,
            },
        );
        // Atomically insert the per-arm member maps under the cluster's logical name
        // (not per-arm sub.name) so the writer key matches the consumer's read key
        // in expr.rs. This insertion is the ONLY write site for this cluster's entry;
        // every early-return above (discriminant error, mismatch, non-exhaustive,
        // outside-collision, etc.) skips both register_match_arm_group and this
        // insert, keeping the key sets in sync. (task 2872)
        scope
            .match_arm_group_arm_member_types
            .insert(logical_name.clone(), per_arm_member_maps);
    }
}

/// Extract the shared logical name from an arm's `MemberDecl`.
fn arm_member_name(member: &reify_ast::MemberDecl) -> Option<&str> {
    match member {
        reify_ast::MemberDecl::Sub(s) => Some(&s.name),
        reify_ast::MemberDecl::Param(p) => Some(&p.name),
        reify_ast::MemberDecl::Let(l) => Some(&l.name),
        _ => None,
    }
}

/// Determine the `Type` of an arm's declared member for `GuardedDeclArm::arm_type`.
fn arm_member_type(
    member: &reify_ast::MemberDecl,
    scope: &CompilationScope,
    diagnostics: &mut Vec<Diagnostic>,
    span: SourceSpan,
) -> Type {
    match member {
        reify_ast::MemberDecl::Sub(s) => Type::StructureRef(s.structure_name.clone()),
        reify_ast::MemberDecl::Param(p) => {
            // Pre-pass registers this name; resolution failure here is a pass-1 invariant
            // violation. See `emit_ice_unresolved` for the full rationale.
            scope
                .resolve(&p.name)
                .map(|(_, ty)| ty.clone())
                .unwrap_or_else(|| {
                    emit_ice_unresolved(UnresolvedKind::Name, &p.name, span, diagnostics)
                })
        }
        reify_ast::MemberDecl::Let(l) => {
            // Same pass-1 registration invariant as the Param arm above; the ICE guards
            // against a future refactor regressing to silent Type::Real. See `emit_ice_unresolved`.
            scope
                .resolve(&l.name)
                .map(|(_, ty)| ty.clone())
                .unwrap_or_else(|| {
                    emit_ice_unresolved(UnresolvedKind::Name, &l.name, span, diagnostics)
                })
        }
        _ => {
            // Unhandled MemberDecl variant: emit a diagnostic so the caller gets explicit
            // feedback rather than a silently-wrong Type::Real.
            diagnostics.push(
                Diagnostic::error("unsupported member kind in match arm")
                    .with_label(DiagnosticLabel::new(span, "expected param, let, or sub")),
            );
            Type::Real
        }
    }
}

/// Build the guard expression for one match arm.
///
/// - Single pattern `Hex`: produces `discriminant == HeadType::Hex`
/// - Pipe patterns `Hex | Button`: produces `(discriminant == HeadType::Hex) || (discriminant == HeadType::Button)`
fn build_arm_guard_expr(
    discriminant_ref: &CompiledExpr,
    enum_type_name: &str,
    patterns: &[String],
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledExpr {
    if patterns.is_empty() {
        diagnostics.push(
            Diagnostic::error("match arm has no patterns")
                .with_label(DiagnosticLabel::new(span, "empty arm pattern list")),
        );
        // Return a sentinel Bool(false) so compilation can continue.
        return CompiledExpr::literal(Value::Bool(false), Type::Bool);
    }

    let mut expr: Option<CompiledExpr> = None;
    for variant in patterns {
        let variant_literal = CompiledExpr::literal(
            Value::Enum {
                type_name: enum_type_name.to_string(),
                variant: variant.clone(),
            },
            Type::Enum(enum_type_name.to_string()),
        );
        let eq = CompiledExpr::binop(
            BinOp::Eq,
            discriminant_ref.clone(),
            variant_literal,
            Type::Bool,
        );
        expr = Some(match expr {
            None => eq,
            Some(prev) => CompiledExpr::binop(BinOp::Or, prev, eq, Type::Bool),
        });
    }
    expr.expect("patterns was non-empty, so expr is Some")
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
        type_params: Vec<reify_ir::TypeParam>,
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

/// A deferred `auto:` / `auto(free):` type-argument resolution request, raised
/// at a sub-component instantiation site (`sub x = Foo<auto: Bound>()`) and
/// drained by `compile_builder::auto_type_param_phase::phase_auto_type_param_resolution`
/// after `phase_entities` has populated `ctx.templates`.
///
/// Mirrors the `PendingBoundCheck` deferred-resolution idiom: the request is
/// pushed during template build (when only the calling structure's context is
/// in scope) and resolved in a post-pass once the target template's
/// `type_params` and the full template/trait registries are reachable — even
/// when the target is forward-referenced from the use site.
///
/// `target_name` is the instantiated template (e.g. `"Bearing"`);
/// `owner_structure` + `sub_index` locate the `SubComponentDecl` whose
/// `type_args` placeholder slots are rewritten to concrete `Type::StructureRef`
/// on a successful resolution.
///
/// Why `sub_index` instead of just `sub_name`: match-arm clusters push one
/// `SubComponentDecl` per arm, all sharing the same `sub_name` (see
/// `compile_match_arm_decl_group`'s `logical_name` invariant). A name-only
/// lookup would resolve every arm's request to arm[0]'s `SubComponentDecl`,
/// silently losing the resolved candidate for every other arm. The index is
/// captured at push time as `sub_components.len()` — the position at which
/// the matching `SubComponentDecl` will be pushed immediately after — so the
/// post-pass can rewrite the correct entry even when multiple arms share a
/// name.
pub(crate) struct AutoResolutionRequest {
    /// The template being instantiated (e.g. `"Bearing"`).
    pub(crate) target_name: String,
    /// One clause per `auto:` slot in this sub's `type_args`, in source order.
    /// Each clause carries its own `span`; the resolver anchors per-param and
    /// collective diagnostics on those clause spans, so no request-level span is
    /// needed here.
    pub(crate) auto_clauses: Vec<AutoClause>,
    /// Name of the structure that owns this sub-component (e.g. `"Assembly"`).
    pub(crate) owner_structure: String,
    /// Index of the target `SubComponentDecl` in the owning template's
    /// `sub_components` vector. Captured at push time as the length of the
    /// local `sub_components` vec, which the immediately-following push fills
    /// — so the index unambiguously identifies the matching entry even when
    /// arm clusters reuse names across multiple `SubComponentDecl`s.
    pub(crate) sub_index: usize,
}

/// A single `auto:` / `auto(free):` type-argument clause within a
/// sub-component instantiation's `type_args` list.
///
/// `position` is the index into the target template's `type_params` (and the
/// matching `SubComponentDecl.type_args` placeholder slot). `bound` is the
/// trait name the candidate must satisfy (e.g. `"Seal"`); `free` distinguishes
/// strict `auto:` (`false`) from `auto(free):` (`true`).
pub(crate) struct AutoClause {
    /// Index into the target's `type_params` / the sub's `type_args` slots.
    pub(crate) position: usize,
    /// Strict (`false`) vs. free (`true`) resolution flag.
    pub(crate) free: bool,
    /// The required trait bound the resolved candidate must satisfy.
    pub(crate) bound: String,
    /// Span of the `auto:` clause, used for per-param diagnostic labels.
    pub(crate) span: SourceSpan,
}

/// A deferred sub-instance-override `auto` / `auto(free)` registration raised
/// when the **child structure is forward-declared** (parent compiled before
/// child).
///
/// During `compile_entity`, `scope.sub_member_types` is only populated when the
/// child template is already in `compiled_templates`.  When the child is
/// forward-declared, the member-type lookup returns `None` — but this is NOT a
/// "no such param" error: the child may well have that param once compiled.
/// Deferring via this struct lets the post-pass `phase_sub_override_autos`
/// re-run the lookup against the fully-populated template registry and push the
/// scoped `ValueCellDecl` (or emit a genuine "no such param" error if the
/// member is truly absent).
///
/// Mirrors `AutoResolutionRequest` / `PendingBoundCheck` in structure and lifecycle.
pub(crate) struct PendingSubOverrideAuto {
    /// Name of the parent structure that declares the sub (e.g. `"A"`).
    pub(crate) parent_entity_name: String,
    /// Local name of the sub instance inside the parent (e.g. `"b"`).
    pub(crate) sub_name: String,
    /// Name of the child structure being instantiated (e.g. `"Bearing"`).
    pub(crate) sub_structure_name: String,
    /// Name of the overridden member / param on the child (e.g. `"bore"`).
    pub(crate) override_member: String,
    /// `false` = strict `auto`; `true` = `auto(free)`.
    pub(crate) free: bool,
    /// Span of the `auto` / `auto(free)` expression in source; used for error labels.
    pub(crate) span: SourceSpan,
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
    members: &'a [reify_ast::MemberDecl],
    known: &HashSet<&str>,
    functions: &[CompiledFunction],
    out: &mut HashMap<&'a str, &'a reify_ast::Expr>,
) {
    for m in members {
        match m {
            reify_ast::MemberDecl::Let(let_decl)
                if is_geometry_let(&let_decl.value, functions, known) =>
            {
                out.insert(let_decl.name.as_str(), &let_decl.value);
            }
            reify_ast::MemberDecl::Param(param) if known.contains(param.name.as_str()) => {
                if let Some(e) = &param.default {
                    out.insert(param.name.as_str(), e);
                }
            }
            reify_ast::MemberDecl::GuardedGroup(g) => {
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
    enum_defs: &'a [reify_ir::EnumDef],
    functions: &'a [CompiledFunction],
    known_geometry_lets: &'a HashSet<&'a str>,
    geometry_lets: &'a HashMap<&'a str, &'a reify_ast::Expr>,
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
    members: &[reify_ast::MemberDecl],
    deps: &GeometryRealizationDeps<'_>,
    sink: &mut GeometryRealizationSink<'_>,
) {
    for m in members {
        match m {
            reify_ast::MemberDecl::Param(param)
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
            reify_ast::MemberDecl::GuardedGroup(g) => {
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
    type_params: &[reify_ir::TypeParam],
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
    type_expr: Option<&reify_ast::TypeExpr>,
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

// ---------------------------------------------------------------------------
// Constraint-instantiation expansion (shared by `MemberDecl::ConstraintInst`
// and the `forall` `ConstraintBody::Instantiation` branch in
// `forall_elaborate.rs`)
// ---------------------------------------------------------------------------

/// Expand a `constraint Foo(arg: x, ...)` instantiation into one or more
/// `CompiledConstraint`s — one per predicate in the matching constraint def.
///
/// Behaviour mirrors the original inline implementation that lived in
/// `compile_structure_inner`'s `MemberDecl::ConstraintInst` arm:
///   * Look up the constraint def in `constraint_def_registry`. Missing →
///     emit a single error diagnostic and return.
///   * Validate named-argument set against the def's params (unknown args,
///     missing required args). Validation errors short-circuit emission.
///   * Allocate one shared `inst_idx` for this instantiation by bumping the
///     `constraint_inst_counts` entry for `ci.name`, so all predicates from
///     this call share the same inst_idx and differ only by `pred_idx`.
///   * Per predicate: substitute params with the named args, compile the
///     resulting expression, build a `CompiledConstraint` whose label
///     follows the format `<name>#<inst_idx>[<pred_idx>]`, and either push
///     it onto `constraints` directly or route it through
///     `compile_per_decl_constraint_guard` if `ci.where_clause.is_some()`.
///
/// `label_suffix` (added for task 2364) optionally appends `:<suffix>` to
/// each emitted constraint's label. The forall instantiation branch passes
/// `Some("forall@<var>[<i>]")` so per-element diagnostics retain both the
/// inst-idx provenance and the forall element index. Non-forall callers
/// pass `None` to preserve the original `<name>#<inst_idx>[<pred_idx>]`
/// label format unchanged.
#[allow(clippy::too_many_arguments)]
pub(crate) fn expand_constraint_inst(
    ci: &reify_ast::ConstraintInstDecl,
    entity_name: &str,
    constraint_def_registry: &HashMap<String, &CompiledConstraintDef>,
    scope: &mut CompilationScope,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    constraint_inst_counts: &mut HashMap<String, usize>,
    guarded_groups: &mut Vec<CompiledGuardedGroup>,
    structure_controlling: &mut HashSet<ValueCellId>,
    guard_index: &mut u32,
    diagnostics: &mut Vec<Diagnostic>,
    label_suffix: Option<&str>,
) {
    // Look up the constraint definition.
    let def = match constraint_def_registry.get(&ci.name) {
        Some(d) => *d,
        None => {
            diagnostics.push(
                Diagnostic::error(format!("unknown constraint definition: {}", ci.name))
                    .with_label(DiagnosticLabel::new(
                        ci.span,
                        format!("no constraint def named '{}'", ci.name),
                    )),
            );
            return;
        }
    };

    // Build name → Expr bindings map from the named args.
    let arg_map: HashMap<String, reify_ast::Expr> = ci
        .args
        .iter()
        .map(|(name, expr)| (name.clone(), expr.clone()))
        .collect();

    // Validate: check for unknown argument names.
    let param_names: HashSet<&str> = def.params.iter().map(|p| p.name.as_str()).collect();
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
        return;
    }

    // Allocate this instantiation's inst_idx before the per-predicate loop
    // so all predicates from one `constraint MinWall(...)` share the same
    // inst_idx — predicates differ only by pred_idx. Uses a get_mut/insert
    // split to avoid cloning `ci.name` on the common case where the entry
    // already exists.
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
        let compiled_expr = compile_expr(&substituted, scope, enum_defs, functions, diagnostics);

        let id = ConstraintNodeId::new(entity_name, *constraint_index);
        let base_label = format!("{}#{}[{}]", ci.name, inst_idx, pred_idx);
        let label = match label_suffix {
            Some(suffix) => format!("{}:{}", base_label, suffix),
            None => base_label,
        };
        let cc = CompiledConstraint {
            id,
            label: Some(label),
            expr: compiled_expr,
            span: ci.span,
            domain: None,
            optimized_target: def.annotations_optimized_target.clone(),
        };
        *constraint_index += 1;

        if let Some(wc) = &ci.where_clause {
            compile_per_decl_constraint_guard(
                entity_name,
                wc,
                cc,
                scope,
                enum_defs,
                functions,
                diagnostics,
                guarded_groups,
                structure_controlling,
                guard_index,
            );
        } else {
            constraints.push(cc);
        }
    }
}

/// Build a skeleton [`TopologyTemplate`] for a `structure_def` that appears in
/// the same module as a compiled function.
///
/// The skeleton is transient — it is never stored in the [`CompiledModule`].
/// Its sole purpose is to make `Foo()` ctor expressions inside same-module
/// fn bodies lower to [`CompiledExprKind::StructureInstanceCtor`] via the
/// `prelude_template_registry` path in [`crate::functions::compile_function`].
///
/// Only three things are consumed by the
/// [`CompiledExprKind::StructureInstanceCtor`] lowering site
/// (`expr.rs:1070-1103`):
///
/// * `entity_kind == EntityKind::Structure`
/// * the `Param`-kind `value_cells` (member name + `default_expr`)
/// * `template.version()` (from annotations)
///
/// All other fields are empty / zero — `content_hash` is 0, no constraints,
/// sub-components, ports, etc.
///
/// Default expressions are compiled in a **neutral scope** (no sibling params
/// registered) with the unit registry set so quantity literals like `0Pa`
/// resolve.  [`fixup_option_none_for_param`] is applied so `Option<T> = none`
/// defaults match the authoritative template shape.  Diagnostics from skeleton
/// compilation are discarded into a throwaway buffer —
/// `entities_phase::phase_entities` later re-compiles the same `structure_def`s
/// and emits the authoritative diagnostics, avoiding double-emission and
/// spurious neutral-scope errors.
///
/// **Limitation — silent-divergence hazard** (accepted per task 3895 design
/// decision #3): a param default that references a sibling param or a
/// same-module function will not resolve in the neutral scope; `compile_expr`
/// returns a poison expression and its diagnostic is swallowed by the throwaway
/// buffer.  That poison default is then baked verbatim into the
/// `StructureInstanceCtor.defaults` map at the fn-body lowering site in
/// `expr.rs`.  `phase_entities` re-compiles the same `structure_def`
/// authoritatively (siblings resolve there), but it emits NO diagnostic for the
/// fn-body ctor.  Net result: a same-module fn body calling `Foo()` that relies
/// on such a default silently evaluates the poison value at runtime, while
/// direct entity instantiation of `Foo` gets the correct default — a
/// diagnostic-free divergence.  A follow-up task should either detect skeleton
/// poison defaults and emit a diagnostic at the ctor call site, or omit the
/// default from the skeleton so the existing "missing default" path reports it.
/// Acceptable for now because `std/flexures` and the acceptance test
/// (`same_module_structure_ctor_compile.rs`) use only literal defaults.
pub(crate) fn build_structure_def_skeleton(
    structure: &reify_ast::StructureDef,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    unit_registry: &UnitRegistry,
) -> TopologyTemplate {
    // Throwaway diagnostics — phase_entities re-compiles authoritatively.
    let mut throwaway_diags: Vec<Diagnostic> = Vec::new();

    // Clone the alias registry so skeleton type resolution does NOT consume
    // span-dedup slots in the original registry.  TypeAliasRegistry maintains
    // an interior-mutable `emitted_skipped_parametric_prelude_spans` dedup set
    // (RefCell<HashSet<SourceSpan>>); resolving a parametric-alias type expression
    // here records the span as "emitted" even though the diagnostic goes to
    // `throwaway_diags`.  If we shared the original registry, phase_entities'
    // authoritative re-compile of the same type expression would find the span
    // already recorded and silently skip its Info diagnostic (task 3895 bugfix).
    let local_alias_registry = alias_registry.clone();

    // Neutral scope: unit registry set so quantity literals resolve;
    // no sibling params registered (neutral-scope semantics).
    let mut scope = CompilationScope::new(&structure.name);
    scope.set_unit_registry(unit_registry);

    // Type params from the structure declaration (needed by resolve_type_expr_with_aliases).
    let type_param_names: HashSet<String> = structure
        .type_params
        .iter()
        .map(|tp| tp.name.clone())
        .collect();

    let visibility = if structure.is_pub {
        Visibility::Public
    } else {
        Visibility::Private
    };

    // Lower annotations so template.version() reflects @version(N) correctly.
    let annotations = lower_annotations(&structure.annotations, &mut throwaway_diags);

    // Build value_cells for Param members only; Lets and other member kinds
    // are irrelevant to the StructureInstanceCtor lowering path.
    let mut value_cells: Vec<ValueCellDecl> = Vec::new();
    for member in &structure.members {
        if let reify_ast::MemberDecl::Param(param) = member {
            // Resolve cell_type; fall back to Real on None / unresolvable.
            // cell_type is needed only by fixup_option_none_for_param to
            // detect Type::Option; the ctor lowering itself does not use it.
            let cell_type = param
                .type_expr
                .as_ref()
                .and_then(|te| {
                    resolve_type_expr_with_aliases(
                        te,
                        &type_param_names,
                        &local_alias_registry,
                        &mut throwaway_diags,
                        structure_names,
                        trait_names,
                    )
                })
                .unwrap_or(Type::Real);

            let default_expr = param.default.as_ref().map(|expr| {
                let mut compiled =
                    compile_expr(expr, &scope, enum_defs, functions, &mut throwaway_diags);
                fixup_option_none_for_param(&mut compiled, &cell_type);
                compiled
            });

            let id = ValueCellId::new(&structure.name, &param.name);
            value_cells.push(ValueCellDecl {
                id,
                kind: ValueCellKind::Param,
                visibility: Visibility::Public,
                is_aux: false,
                cell_type,
                default_expr,
                solver_hints: vec![],
                span: param.span,
            });
        }
    }

    TopologyTemplate {
        name: structure.name.to_string(),
        doc: structure.doc.clone(),
        entity_kind: EntityKind::Structure,
        visibility,
        // Skeleton does not carry type_params or trait_bounds —
        // the ctor lowering path does not read them.
        type_params: vec![],
        trait_bounds: vec![],
        value_cells,
        constraints: vec![],
        realizations: vec![],
        sub_components: vec![],
        ports: vec![],
        connections: vec![],
        guarded_groups: vec![],
        structure_controlling: HashSet::new(),
        objective: None,
        meta: HashMap::new(),
        // Skeleton is transient; content_hash=0 signals "not a real template".
        content_hash: ContentHash(0),
        is_recursive: false,
        annotations,
        pragmas: structure.pragmas.to_vec(),
        match_arm_groups: vec![],
        forall_templates: vec![],
        assoc_fns: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table-driven coverage: both Param and Let route through `emit_ice_unresolved`
    /// when the declared name is absent from scope.  We assert that:
    /// 1. `Type::Real` is returned (the ICE fallback value).
    /// 2. Exactly one diagnostic is pushed.
    /// 3. The diagnostic message contains `"internal compiler error"` — proving the
    ///    ICE pathway was taken, not the wildcard fallback ("unsupported member kind
    ///    in match arm").
    /// 4. The diagnostic message also contains `"unresolved name"` — pinning
    ///    `UnresolvedKind::Name` as the exact pathway (not `GuardedMember`).  The
    ///    exact ICE wording and label format are already pinned by the tests in
    ///    `ice.rs`.
    ///
    /// Regression guard for the `alias_registry.clone()` in
    /// `build_structure_def_skeleton` (task 3895 bugfix).
    ///
    /// `TypeAliasRegistry` maintains an interior-mutable
    /// `emitted_skipped_parametric_prelude_spans` dedup set.  If the skeleton
    /// builder were to share the caller's registry (instead of cloning it),
    /// type resolution of a parametric-prelude-alias param type would record
    /// the source span as "emitted" in the original registry, causing
    /// `phase_entities`' authoritative re-compile of the same type expression
    /// to skip its `Severity::Info` diagnostic silently.
    ///
    /// This test directly verifies the isolation: after calling
    /// `build_structure_def_skeleton` with a registry whose
    /// `emitted_skipped_parametric_prelude_spans` set is empty, the original
    /// registry must still report `should_emit_skipped_parametric_prelude_info`
    /// as `true` for the param type's source span — i.e. the skeleton's type
    /// resolution did NOT consume the span from the original registry.
    #[test]
    fn build_structure_def_skeleton_does_not_consume_alias_registry_dedup_slots() {
        let span = reify_core::SourceSpan::new(10, 30);

        // Register "MyAlias" as a skipped parametric prelude name.
        let mut alias_registry = crate::TypeAliasRegistry::new();
        alias_registry.mark_skipped_parametric_prelude("MyAlias".to_string());

        // A StructureDef with one param typed as `MyAlias<Real>`.  The skeleton
        // builder will call `resolve_type_expr_with_aliases` for this param,
        // which should hit the `should_emit_skipped_parametric_prelude_info`
        // check.  Because `build_structure_def_skeleton` clones the registry,
        // only the local clone's dedup set is populated — the original is clean.
        let structure = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: true,
            type_params: vec![],
            trait_bounds: vec![],
            members: vec![reify_ast::MemberDecl::Param(reify_ast::ParamDecl {
                name: "x".to_string(),
                doc: None,
                is_priv: false,
                type_expr: Some(reify_ast::TypeExpr {
                    kind: reify_ast::TypeExprKind::Named {
                        name: "MyAlias".to_string(),
                        type_args: vec![reify_ast::TypeExpr {
                            kind: reify_ast::TypeExprKind::Named {
                                name: "Real".to_string(),
                                type_args: vec![],
                            },
                            span,
                        }],
                    },
                    span,
                }),
                default: None,
                where_clause: None,
                annotations: vec![],
                span,
                content_hash: reify_core::ContentHash(0),
            })],
            span,
            content_hash: reify_core::ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let _ = build_structure_def_skeleton(
            &structure,
            &[],
            &[],
            &alias_registry,
            &Default::default(),
            &Default::default(),
            &UnitRegistry::new(),
        );

        // The original registry's dedup set must still be pristine — the
        // skeleton used a local clone, so span 10..30 was not consumed.
        // `should_emit_skipped_parametric_prelude_info` returns `true` exactly
        // once per span (recording it on first call); if it returns `false`
        // here the span was already consumed by the skeleton (bug regressed).
        assert!(
            alias_registry.should_emit_skipped_parametric_prelude_info("MyAlias", span),
            "build_structure_def_skeleton must not consume dedup slots in the \
             original alias_registry; the span must still be available for the \
             authoritative Info emission by phase_entities \
             (regression guard for task 3895 alias_registry.clone() bugfix)"
        );
    }

    #[test]
    fn arm_member_type_emits_ice_when_unresolved() {
        let span = SourceSpan::new(0, 0);

        let cases: &[(&str, reify_ast::MemberDecl)] = &[
            (
                "Param",
                reify_ast::MemberDecl::Param(reify_ast::ParamDecl {
                    name: "x".to_string(),
                    doc: None,
                    is_priv: false,
                    type_expr: None,
                    default: None,
                    where_clause: None,
                    annotations: vec![],
                    span,
                    content_hash: reify_core::ContentHash(0),
                }),
            ),
            (
                "Let",
                reify_ast::MemberDecl::Let(reify_ast::LetDecl {
                    name: "x".to_string(),
                    doc: None,
                    is_pub: false,
                    is_aux: false,
                    type_expr: None,
                    value: reify_ast::Expr {
                        kind: reify_ast::ExprKind::Ident("dummy".to_string()),
                        span,
                    },
                    where_clause: None,
                    annotations: vec![],
                    span,
                    content_hash: reify_core::ContentHash(0),
                }),
            ),
        ];

        for (label, member) in cases {
            // Empty scope — name "x" will not resolve.
            let scope = CompilationScope::new("TestEntity");
            let mut diagnostics: Vec<Diagnostic> = Vec::new();
            let ty = arm_member_type(member, &scope, &mut diagnostics, span);

            assert_eq!(
                ty,
                Type::Real,
                "[{label}] fallback type should be Type::Real"
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "[{label}] expected exactly one diagnostic, got: {diagnostics:?}",
            );
            assert!(
                diagnostics[0].message.contains("internal compiler error"),
                "[{label}] expected ICE diagnostic, got: {:?}",
                diagnostics[0].message,
            );
            assert!(
                diagnostics[0].message.contains("unresolved name"),
                "[{label}] expected UnresolvedKind::Name ICE, got: {:?}",
                diagnostics[0].message,
            );
        }
    }
}
