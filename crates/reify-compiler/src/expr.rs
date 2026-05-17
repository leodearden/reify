//! Expression compilation and the `Type::Error` anti-cascade sentinel.
//!
//! # Poison policy (task-448 / task-1912 / task-1921 / task-1969)
//!
//! `Type::Error` is the poison-value sentinel for type-inference failure. Any
//! producer site that emits a `Severity::Error` diagnostic for a truly
//! unrecoverable type-inference failure must pair it with a `Type::Error`
//! result so consumer guards (`type_compat::implicitly_converts_to`,
//! `type_compat::type_compatible`, `type_compat::infer_binop_type`) can
//! short-circuit and suppress cascading diagnostics.
//!
//! ## Canonical producer helpers
//!
//! `make_poison_literal(diagnostics, diagnostic)` constructs the diagnostic,
//! pushes it into the queue, and returns
//! `CompiledExpr::literal(Value::Undef, Type::Error)`.  The "push paired with
//! poison" invariant is enforced **by construction**: the caller passes the
//! `Diagnostic` value directly so there is no separate push step to accidentally
//! omit.  A `debug_assert!` on the diagnostic's severity catches callers that
//! mistakenly pass a `Warning` or `Info` value.
//!
//! `make_poison_type(diagnostics, diagnostic)` is the parallel helper for
//! ICE-path producer sites that assign a `Type` to a local variable rather than
//! returning a `CompiledExpr`.  It carries the same by-construction invariant.
//! `grep "make_poison_"` finds every producer site uniformly.
//!
//! ## Consumer propagation helper
//!
//! `propagate_poison()` returns `CompiledExpr::literal(Value::Undef, Type::Error)`
//! without any `debug_assert!`.  It is for consumer sites that propagate an
//! already-existing `Type::Error` without emitting a new diagnostic, making
//! producer vs. consumer sites grep-distinct.
//!
//! ## Intentional non-Error fallbacks
//!
//! Some producers emit a diagnostic but return a non-`Type::Error` fallback
//! because the fallback type is semantically correct for downstream checks
//! (e.g. `Type::Bool` for determinacy predicates, `Type::String` for meta-block
//! access, `Type::Enum(name)` for unknown enum variants).  For the authoritative
//! enumeration and rationale see
//! `crates/reify-compiler/tests/expr_error_sentinel_tests.rs` (task-1921).
//!
//! All other `Value::Undef`-returning error branches route through
//! `make_poison_literal` per the audit in task-1921.

use super::*;

/// Return a `CompiledExpr` poison literal (`Value::Undef, Type::Error`) for
/// use at any producer site that emits a `Severity::Error` diagnostic.
///
/// # Anti-cascade contract (task-448 / task-1912 / task-1921)
///
/// `Type::Error` is the poison-value sentinel: once a sub-expression is typed
/// as `Type::Error`, consumer guards in `type_compat.rs`
/// (`implicitly_converts_to`, `type_compatible`, `infer_binop_type`) and in
/// `expr.rs` (aggregation, index-access, quantifier) short-circuit and avoid
/// emitting cascading type-mismatch diagnostics on top of the root-cause error.
///
/// # By-construction invariant (task-1969)
///
/// The caller passes a pre-constructed `Diagnostic` directly; this helper
/// pushes it into the queue and then returns the poison literal.  The
/// "push paired with poison" invariant is therefore enforced **by construction**
/// rather than by a post-hoc `debug_assert!` over queue indices.
///
/// A `debug_assert!` on the diagnostic's severity catches callers that
/// mistakenly pass a `Warning` or `Info` diagnostic.  `#[track_caller]`
/// ensures a failing assert points to the producer site, not this body.
///
/// All producer sites that return `Type::Error` **and** emit their own
/// diagnostic should route through this helper.  Consumer sites that propagate
/// an existing `Type::Error` without emitting a new diagnostic should use
/// [`propagate_poison`] instead.  ICE-path producer sites that assign a `Type`
/// to a local variable route through the parallel [`make_poison_type`] helper.
#[track_caller]
fn make_poison_literal(diagnostics: &mut Vec<Diagnostic>, diagnostic: Diagnostic) -> CompiledExpr {
    debug_assert!(
        diagnostic.severity == Severity::Error,
        "make_poison_literal requires a Severity::Error diagnostic; \
         got severity={:?} — did you pass a Warning or Info by mistake?",
        diagnostic.severity,
    );
    diagnostics.push(diagnostic);
    CompiledExpr::literal(Value::Undef, Type::Error)
}

/// Return a `Type::Error` poison sentinel for ICE-path producer sites that
/// assign a `Type` to a local variable rather than returning a `CompiledExpr`.
///
/// Mirrors [`make_poison_literal`] for the Type-level ICE-path fallbacks
/// (range-no-bounds, match-no-arms, unresolved-sub-member-type, non-collection
/// iteration, non-collection index) so that all producer sites route through a
/// helper and `grep "make_poison_"` finds every producer site uniformly.
///
/// Applies the same by-construction invariant as [`make_poison_literal`]: the
/// caller passes the `Diagnostic` directly; this helper pushes it and returns
/// `Type::Error`.  `debug_assert!` checks severity; `#[track_caller]` ensures
/// a failing assert blames the producer site.
#[track_caller]
fn make_poison_type(diagnostics: &mut Vec<Diagnostic>, diagnostic: Diagnostic) -> Type {
    debug_assert!(
        diagnostic.severity == Severity::Error,
        "make_poison_type requires a Severity::Error diagnostic; \
         got severity={:?} — did you pass a Warning or Info by mistake?",
        diagnostic.severity,
    );
    diagnostics.push(diagnostic);
    Type::Error
}

/// Return a `CompiledExpr` poison literal for **consumer-propagation** sites.
///
/// Unlike [`make_poison_literal`], this helper takes no diagnostic argument and
/// performs no `debug_assert!`.  It is for consumer sites that propagate an
/// existing `Type::Error` without emitting a new diagnostic — for example, the
/// already-poisoned short-circuit at the non-aggregation member-access arm.
///
/// Using this helper (rather than the raw `CompiledExpr::literal(Value::Undef,
/// Type::Error)`) makes producer vs. consumer sites grep-distinct.
fn propagate_poison() -> CompiledExpr {
    CompiledExpr::literal(Value::Undef, Type::Error)
}

/// Emit the cross-sub geometry-access diagnostic via `make_poison_literal` (task-3397).
///
/// Used at all three sub-member-access sites (non-collection sub, bare collection
/// sub, indexed collection sub) when the missing member is found in
/// `scope.sub_realization_names` — i.e. it exists as a `RealizationDecl` on the
/// child template, but cross-sub geometry access is not yet supported in v0.1.
///
/// Centralising the wording here prevents drift across the three call sites and
/// makes the keyword "geometry"+"not yet supported in v0.1" grep-findable in one
/// place.  The anti-cascade contract is preserved: `make_poison_literal` returns
/// `Type::Error`, so downstream type-checks short-circuit and no cascade
/// diagnostics fire.
#[track_caller]
fn make_cross_sub_geometry_error(
    diagnostics: &mut Vec<Diagnostic>,
    member: &str,
    sub_name: &str,
    child_struct: &str,
    span: reify_types::SourceSpan,
) -> CompiledExpr {
    make_poison_literal(
        diagnostics,
        Diagnostic::error(format!(
            "cross-sub access to geometry-typed member '{}' on sub '{}' \
             is not yet supported in v0.1; compose geometry inside '{}' \
             or pass scalar parameters to its primitives",
            member, sub_name, child_struct
        ))
        .with_label(DiagnosticLabel::new(span, "cross-sub geometry access")),
    )
}

/// Check whether `member` is a geometry realization on the sub named `sub_name`,
/// and if so emit the cross-sub geometry diagnostic via `make_cross_sub_geometry_error`
/// (task-3397).
///
/// Returns `Some(poisoned_expr)` when the member is found in
/// `scope.sub_realization_names[sub_name]`, so the caller can `return` early with
/// `Type::Error` and skip the generic "unknown member" path.  Returns `None` when
/// the member is not a realization, allowing the caller to fall through to its
/// existing generic-error branch.
///
/// Used at the two **collection-sub** member-access sites (bare collection sub,
/// indexed collection sub).  Collection-sub cross-sub geometry remains deferred
/// in v0.1 because per-instance handles would require per-element realisation,
/// which is out of scope.  The **non-collection** sub site uses the sibling
/// helper [`try_resolve_cross_sub_geometry_value_ref`] instead, which produces a
/// working value-ref (task 3441) rather than a diagnostic.
///
/// # Invariant (task-3420)
///
/// `sub_realization_names[sub_name].is_some()` implies
/// `sub_component_types.contains_key(sub_name)`.  Both maps are populated
/// together in `entity.rs` (regular Sub pre-pass and match-arm Sub pre-pass)
/// inside the same `if let Some(child_tmpl) = find_template(...)` guard, with
/// `sub_component_types` written unconditionally before the template lookup.
/// The `sub_component_types.get(sub_name)` lookup below enforces this invariant
/// in **all** build modes (debug and release; task-3431) and names the offending
/// sub instance in the panic message (task-3439), so a future code path that
/// populates `sub_realization_names` without `sub_component_types` panics
/// loudly rather than silently producing a diagnostic that names the sub
/// instance instead of its child structure.
pub(crate) fn try_emit_cross_sub_geometry(
    scope: &CompilationScope<'_>,
    sub_name: &str,
    member: &str,
    span: reify_types::SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledExpr> {
    if scope
        .sub_realization_names
        .get(sub_name)
        .is_some_and(|s| s.contains(member))
    {
        let child_struct = scope
            .sub_component_types
            .get(sub_name)
            .unwrap_or_else(|| panic!(
                "sub_realization_names ⊂ sub_component_types invariant (task-3420; release-enforced task-3431): \
                 sub '{}' has realization entries but no structure-name entry — check entity.rs Sub/match-arm pre-passes",
                sub_name
            ))
            .as_str();
        Some(make_cross_sub_geometry_error(
            diagnostics,
            member,
            sub_name,
            child_struct,
            span,
        ))
    } else {
        None
    }
}

/// **Non-collection** sub working path for cross-sub geometry access (task 3441).
///
/// When `<sub_name>` is a non-collection sub of the current entity AND `<member>`
/// is a geometry realisation on its child structure (per
/// `scope.sub_realization_names[sub_name].contains(member)`), this helper
/// produces a synthetic `CompiledExpr` of kind
/// `CompiledExprKind::CrossSubGeometryRef` (task 3508) whose entity stamp follows
/// the same `format!("{}.{}", entity_name, sub_name)` convention used at
/// expr.rs:1317 for scalar cross-sub member access, with `Type::Geometry`.
/// The typed variant lets the bare-let drop site in entity.rs recognise the
/// synthetic shape unambiguously via pattern match, rather than the fragile
/// `entity.contains('.')` heuristic.
///
/// Returns `None` when the member is not a realisation on the child template,
/// allowing the caller to fall through to its existing "unknown member" branch.
///
/// **No diagnostic emitted on success.**  The eval side (engine_build.rs) is
/// responsible for plumbing the realised geometry handle into
/// `named_steps["<sub>.<member>"]` so that the parallel `GeomRef::Sub("<sub>.<member>")`
/// in the realisation ops resolves to the child's handle.
///
/// The collection-sub call sites continue to use [`try_emit_cross_sub_geometry`]
/// to emit the v0.1 diagnostic until per-instance handles are implemented.
///
/// # Forward-declared sub (runtime fallback)
///
/// When the parent template is compiled before the child template (i.e., the sub's
/// `structure_name` was not yet in `compiled_templates` at the time the parent's
/// scope was built), `scope.sub_member_types` and `scope.sub_realization_names`
/// are both empty for that sub.  In that case this helper still emits the
/// working-path value-ref optimistically: the compile-side cannot distinguish a
/// forward-declared geometry member from a forward-declared scalar member, so we
/// trust the runtime to flag the missing handle via the
/// `unresolvable GeomRef::Sub('<sub>.<member>')` diagnostic produced by
/// `geometry_ops.rs::resolve_geom_ref`.  The optimism is centralized in
/// `CompilationScope::sub_member_is_cross_sub_geometry_or_forward_declared`
/// (task 3455), shared with `geometry.rs::try_resolve_cross_sub_geom_ref`.
fn try_resolve_cross_sub_geometry_value_ref(
    scope: &CompilationScope<'_>,
    sub_name: &str,
    member: &str,
) -> Option<CompiledExpr> {
    // Forward-declared optimism + realization check (task 3455): single
    // source of truth shared with `geometry.rs::try_resolve_cross_sub_geom_ref`
    // so the value-ref / GeomRef::Sub handshake cannot drift.  See the
    // helper's docstring for the predicate semantics.
    if scope.sub_member_is_cross_sub_geometry_or_forward_declared(sub_name, member) {
        let scoped_entity = format!("{}.{}", scope.entity_name, sub_name);
        let scoped_id = ValueCellId::new(&scoped_entity, member);
        // Emit the typed discriminator (task-3508) so the bare-let drop site in
        // entity.rs can recognise this synthetic shape via pattern match on the
        // variant, rather than the fragile `entity.contains('.')` heuristic.
        Some(CompiledExpr::cross_sub_geometry_ref(scoped_id, Type::Geometry))
    } else {
        None
    }
}

/// Resolve `<scope>.<cluster>.<inner>` against a per-arm member-type map for a
/// match-arm decl group (task 2373).
///
/// Shared by the `self.<cluster>.<inner>` and `<sub>.<cluster>.<inner>`
/// branches in the `MemberAccess` arm of `compile_expr_guarded`. The two
/// call sites differ only in:
///
/// * `scoped_entity` — the entity stamp for the synthetic `ValueCellId`
///   (`scope.entity_name` for the inner case, `<entity>.<sub>` for the
///   external case),
/// * `sub_qualifier` — diagnostic preamble fragment (`None` ⇒ "match-arm
///   types"; `Some("bolt")` ⇒ "match-arm types of sub 'bolt'"),
///
/// while the per-arm lookup, missing-arm filter, divergent-types branch,
/// and synthetic stamp construction are identical. Extracting them here
/// avoids the ~70 lines of duplication called out in the post-impl review.
///
/// Returns a poison literal (`Type::Error`) on missing-arm or divergent-type
/// diagnostics so downstream expressions don't cascade.
///
/// # Empty `per_arm` invariant (review-cycle-1, blocking-fix; task 2373 step-22)
///
/// Empty `per_arm` is a producer-side bug. This guard is the single source of
/// truth for the empty-per_arm "match-arm cluster has no resolvable arm
/// structures" diagnostic for **both** call sites:
///
/// * `self.<cluster>.<inner>` (inner call site, expr.rs ~1029) — task 2869
///   removed the former `Some(arms) if !arms.is_empty()` precheck there and
///   now passes `.map(Vec::as_slice).unwrap_or(&[])` directly, so `None` and
///   `Some(empty)` both reach this guard.
/// * `<sub>.<cluster>.<inner>` (external call site, expr.rs ~1188) — checked
///   that the cluster entry exists before calling, but per_arm can still be
///   empty if no arm structures resolved.
///
/// Centralizing here means any future call site is safe by construction and
/// emits a uniform diagnostic without a separate precheck.
fn resolve_cluster_inner_member(
    per_arm: &[(String, std::collections::BTreeMap<String, Type>)],
    inner: &str,
    scoped_entity: &str,
    group_name: &str,
    sub_qualifier: Option<&str>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledExpr {
    // Empty per_arm guard (task 2373 step-22): without this, the
    // `missing.is_empty()` branch below would index `lookups[0]` and panic
    // with index-out-of-bounds. Emit a uniform cluster-shape diagnostic and
    // return a poison literal so downstream expressions don't cascade.
    if per_arm.is_empty() {
        let qualifier_pre = match sub_qualifier {
            Some(s) => format!(" of sub '{}'", s),
            None => String::new(),
        };
        return make_poison_literal(
            diagnostics,
            Diagnostic::error(format!(
                "match-arm cluster '{}'{} has no resolvable arm structures; \
                 cannot resolve member '{}'",
                group_name, qualifier_pre, inner
            ))
            .with_label(DiagnosticLabel::new(
                span,
                "cluster has no resolved arm structures",
            )),
        );
    }
    let lookups: Vec<(String, Option<Type>)> = per_arm
        .iter()
        .map(|(sname, mts)| (sname.clone(), mts.get(inner).cloned()))
        .collect();
    let missing: Vec<&str> = lookups
        .iter()
        .filter_map(|(s, t)| if t.is_none() { Some(s.as_str()) } else { None })
        .collect();

    let qualifier: String = match sub_qualifier {
        Some(s) => format!(" of sub '{}'", s),
        None => String::new(),
    };

    if missing.is_empty() {
        // All arms have the field; check that all types agree.
        let first_type = lookups[0].1.clone().unwrap();
        let all_equal = lookups.iter().all(|(_, t)| t.as_ref() == Some(&first_type));
        if all_equal {
            let synthetic_member = format!("__match_arm_group_{}__{}", group_name, inner);
            let id = ValueCellId::new(scoped_entity, &synthetic_member);
            return CompiledExpr::value_ref(id, first_type);
        }
        // Divergent types across arms — emit precise diagnostic listing each
        // arm's structure → divergent type.
        let divergent: Vec<String> = lookups
            .iter()
            .map(|(s, t)| {
                format!(
                    "{}: {}",
                    s,
                    t.as_ref().map(|x| x.to_string()).unwrap_or_default()
                )
            })
            .collect();
        return make_poison_literal(
            diagnostics,
            Diagnostic::error(format!(
                "field '{}' has divergent types across match-arm types{}: {}",
                inner,
                qualifier,
                divergent.join(", ")
            ))
            .with_label(DiagnosticLabel::new(
                span,
                "divergent field types across cluster arms",
            )),
        );
    }
    // Some arms are missing the field — emit precise diagnostic naming the
    // offending arm types.
    make_poison_literal(
        diagnostics,
        Diagnostic::error(format!(
            "field '{}' is not present in match-arm types{}: {}",
            inner,
            qualifier,
            missing.join(", ")
        ))
        .with_label(DiagnosticLabel::new(
            span,
            "field missing from one or more cluster arms",
        )),
    )
}

/// Aggregation operations available on collection subs.
///
/// When accessed through `self.<sub>.<member>`, these emit a "drop self." recommendation
/// rather than the indexed-access recommendation used for regular struct members.
/// Also used by the general method-call path to infer result types for collection methods.
const COLLECTION_AGGREGATION_MEMBERS: &[&str] = &["count", "sum", "keys", "values"];

/// Reflective aggregation member names for purpose subjects.
///
/// When a purpose body accesses `subject.<name>` where `subject` has type
/// `StructureRef(_)` and `<name>` is in this list, the compiler emits an empty
/// `ListLiteral` with `result_type = Type::List(Box::new(Type::Real))`.
///
/// Semantics:
/// - Compile-time only: runtime expansion of the list elements against the bound
///   entity's actual params is deferred to a follow-up task.
/// - The empty list means `forall p in subject.params: ...` evaluates vacuously
///   true at eval time, which is safe and anti-cascade-consistent.
/// - `Type::Real` element type is future-proof; a later task can refine to
///   `List<ParamRef>` without changing call-site patterns.
///
/// Deferred names (documented in `crates/reify-mcp/src/tools/chunks/purposes.md`
/// but not yet exercised by `examples/m5_purpose.ri`): `sub_entities`, `ports`,
/// `constraints`. Add them here and to the activation-time expansion when ready.
const PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS: &[&str] =
    &["params", "geometric_params", "material_params"];

/// Entity-kind name that acts as the purpose-subject wildcard.
///
/// A purpose declared as `purpose check(subject : Structure)` binds to *any*
/// structure entity at activation time — there is no static template to validate
/// member accesses against.  The compiler uses this constant to detect that case
/// and skip member validation.
///
/// If a sibling wildcard kind is ever added (e.g., `"Occurrence"` gains first-class
/// wildcard status), add it here alongside this constant rather than embedding
/// another bare string literal at the call site.
const WILDCARD_STRUCTURE_KIND: &str = "Structure";

/// Extract the `free` flag from an `ExprKind::Auto` expression.
///
/// Returns `Some(free)` if the expression is `Auto { free }`, `None` for any other kind.
/// Used to detect auto-solved parameters and build `ValueCellKind::Auto` declarations.
pub(crate) fn extract_auto_free(expr: &reify_syntax::Expr) -> Option<bool> {
    if let reify_syntax::ExprKind::Auto { free } = &expr.kind {
        Some(*free)
    } else {
        None
    }
}

pub(crate) fn compile_expr(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
) -> CompiledExpr {
    let mut lambda_counter = 0u32;
    compile_expr_guarded(
        expr,
        scope,
        enum_defs,
        functions,
        diagnostics,
        None,
        &mut lambda_counter,
    )
}

/// Resolve a collection sub name to its `List<T>` value cell.
///
/// Shared by both the bare-ident arm (`bolts`) and the `self.member` arm (`self.bolts`)
/// of the Identifier/MemberAccess branches in `compile_expr_guarded`, ensuring that
/// `self.bolts` and `bolts` compile to identical `ValueRef`s.
///
/// Resolution strategy:
/// 1. Look up `sub_name` in `scope.sub_member_types` (populated from compiled structure templates).
/// 2. Pick the lexicographically-first key in the inner `BTreeMap` (deterministic order).
/// 3. Return `ValueCellId(entity, "__list_{sub}__{first_member}")` with `Type::List(member_ty)`.
///
/// Fallback (no entry or empty inner map): returns `__list_{sub}` with
/// `List(StructureRef(type_name))`.  The structure type name (e.g. `"Bolt"`) is
/// looked up from `scope.sub_component_types` (populated unconditionally for every
/// sub declaration in the `MemberDecl::Sub` arm of `compile_entity_members` in entity.rs).
/// If absent (e.g. manually constructed scopes in unit tests), the field name is used as
/// a safety fallback.
/// This path is legitimately reached when the sub's structure template has not yet
/// been compiled (e.g. ad-hoc structures or forward references), so it must not panic.
fn resolve_collection_sub_to_list(scope: &CompilationScope, sub_name: &str) -> CompiledExpr {
    if let Some(members) = scope.sub_member_types.get(sub_name) {
        // sub_member_types inner map is BTreeMap — iteration order is lexicographic.
        if let Some((first_member, member_ty)) = members.iter().next() {
            let list_id = ValueCellId::new(
                &scope.entity_name,
                format!("__list_{}__{}", sub_name, first_member),
            );
            let list_type = Type::List(Box::new(member_ty.clone()));
            return CompiledExpr::value_ref(list_id, list_type);
        }
    }
    // Fallback: sub_member_types has no entry for this sub (structure not yet compiled,
    // ad-hoc structure, or empty params).  Use the structure type name from
    // sub_component_types so the StructureRef carries the correct type name, not the
    // field name.  Fall back to field name only if the map has no entry (safety net for
    // manually-constructed CompilationScope in unit tests).
    let type_name = scope
        .sub_component_types
        .get(sub_name)
        .cloned()
        .unwrap_or_else(|| sub_name.to_owned());
    let list_id = ValueCellId::new(&scope.entity_name, format!("__list_{}", sub_name));
    let list_type = Type::List(Box::new(Type::StructureRef(type_name)));
    CompiledExpr::value_ref(list_id, list_type)
}

/// Build a `CompiledExpr` for a `UserFunctionCall` node.
///
/// Centralises the `TAG_USER_FUNCTION_CALL` ContentHash fold so both the
/// `OverloadResolution::Resolved` arm and the default-padding branch produce
/// identical construction logic.  Deprecation-warning emission stays at each
/// call site: the annotation source differs between the two branches and the
/// helper has no business taking a diagnostics sink.
fn build_user_function_call_expr(
    name: &str,
    args: Vec<CompiledExpr>,
    return_type: Type,
) -> CompiledExpr {
    let content_hash = {
        let mut h = ContentHash::of(&[TAG_USER_FUNCTION_CALL]).combine(ContentHash::of_str(name));
        for arg in &args {
            h = h.combine(arg.content_hash);
        }
        h
    };
    CompiledExpr {
        kind: CompiledExprKind::UserFunctionCall {
            function_name: name.to_string(),
            args,
        },
        result_type: return_type,
        content_hash,
    }
}

/// Compile an `Expr` from the AST into a `CompiledExpr`, with guard context.
///
/// When `current_guard` is Some, references to names guarded by a different
/// guard will produce a diagnostic error about unsafe unguarded references.
#[allow(clippy::only_used_in_recursion)]
pub(crate) fn compile_expr_guarded(
    expr: &reify_syntax::Expr,
    scope: &CompilationScope,
    enum_defs: &[reify_types::EnumDef],
    functions: &[CompiledFunction],
    diagnostics: &mut Vec<Diagnostic>,
    current_guard: Option<&ValueCellId>,
    lambda_counter: &mut u32,
) -> CompiledExpr {
    match &expr.kind {
        reify_syntax::ExprKind::NumberLiteral { value, is_real } => {
            // Int/Real classification (incl. integer-form overflow fallback) is
            // shared with `lower_annotations` via reify_syntax::classify_number_literal
            // so the boundary cannot drift between literal lowering and annotation
            // lowering.
            match reify_syntax::classify_number_literal(*value, *is_real) {
                reify_syntax::NumberClass::Int(i) => {
                    CompiledExpr::literal(Value::Int(i), Type::Int)
                }
                reify_syntax::NumberClass::Real(f) => {
                    CompiledExpr::literal(Value::Real(f), Type::Real)
                }
                // Mirror site: lower_annotations in annotations.rs handles LossyReal the same way.
                reify_syntax::NumberClass::LossyReal(f) => {
                    diagnostics.push(crate::diagnostics::lossy_real_warning(expr.span));
                    CompiledExpr::literal(Value::Real(f), Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::QuantityLiteral { value, unit } => {
            // Check the unit registry first (for user-declared units), then fall back to hardcoded.
            let resolved = scope
                .lookup_unit_in_registry(*value, unit)
                .or_else(|| unit_to_scalar(*value, unit));
            match resolved {
                Some((scalar_val, dimension)) => {
                    // Defense-in-depth: reject non-finite si_value from either
                    // lookup_unit_in_registry or unit_to_scalar (overflow, inf literal, etc.)
                    if let Value::Scalar { si_value, .. } = &scalar_val
                        && !si_value.is_finite()
                    {
                        diagnostics.push(
                            Diagnostic::error(
                                "overflow in quantity literal: result is not finite".to_string(),
                            )
                            .with_label(DiagnosticLabel::new(expr.span, "non-finite result")),
                        );
                        return CompiledExpr::literal(
                            Value::Undef,
                            Type::Scalar {
                                dimension: DimensionVector::DIMENSIONLESS,
                            },
                        );
                    }
                    let ty = Type::Scalar { dimension };
                    CompiledExpr::literal(scalar_val, ty)
                }
                None => {
                    diagnostics.push(
                        Diagnostic::error(format!("unknown unit: {}", unit))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized unit")),
                    );
                    // Return an undef literal with dimensionless scalar type as a fallback.
                    // Using Scalar (not Real) keeps the type system consistent for quantity expressions.
                    CompiledExpr::literal(
                        Value::Undef,
                        Type::Scalar {
                            dimension: DimensionVector::DIMENSIONLESS,
                        },
                    )
                }
            }
        }
        reify_syntax::ExprKind::BoolLiteral(b) => {
            CompiledExpr::literal(Value::Bool(*b), Type::Bool)
        }
        reify_syntax::ExprKind::StringLiteral(s) => {
            CompiledExpr::literal(Value::String(s.clone()), Type::String)
        }
        reify_syntax::ExprKind::Ident(name) => {
            // Intercept `self` in entity scope — bare `self` resolves to StructureRef(entity_name).
            // In function scopes (is_entity_scope == false), self falls through to "unresolved name".
            if name == "self" && scope.is_entity_scope {
                let self_id = ValueCellId::new(&scope.entity_name, "__self");
                return CompiledExpr::value_ref(
                    self_id,
                    Type::StructureRef(scope.entity_name.clone()),
                );
            }
            // Intercept `none` before scope lookup — it's a language-level keyword.
            // Default inner type is Real; contextual override happens at param/let sites.
            if name == "none" {
                return CompiledExpr::option_none(Type::Option(Box::new(Type::Real)));
            }
            match scope.resolve(name) {
                Some((id, ty)) => CompiledExpr::value_ref(id.clone(), ty.clone()),
                None => {
                    // Check if this is a collection sub name — delegate to shared helper
                    // that also handles `self.sub_name` in the MemberAccess arm.
                    // Collection sub-names originate from user-declared structures, so they take
                    // precedence over built-in constants (mirroring how scope.resolve already
                    // prioritises user definitions).
                    if scope.collection_sub_names.contains(name.as_str()) {
                        return resolve_collection_sub_to_list(scope, name.as_str());
                    }
                    // Check built-in constants (pi, tau, …) after scope and collection
                    // sub-name resolution so that user definitions always shadow builtins.
                    if let Some(ce) = crate::constants::resolve_builtin_constant(name) {
                        return ce;
                    }
                    let msg = if let Some(canonical) = crate::constants::builtin_constant_hint(name)
                    {
                        format!("unresolved name: {} (did you mean `{}`?)", name, canonical)
                    } else {
                        format!("unresolved name: {}", name)
                    };
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    make_poison_literal(
                        diagnostics,
                        Diagnostic::error(msg)
                            .with_code(DiagnosticCode::UnresolvedName)
                            .with_label(DiagnosticLabel::new(expr.span, "not found in scope")),
                    )
                }
            }
        }
        reify_syntax::ExprKind::BinOp { op, left, right } => {
            // Chained comparison desugaring: `a < b < c` → `And(Lt(a,b), Lt(b,c))`.
            // Detect when the outer op is a comparison and the left operand is also a comparison BinOp.
            if is_comparison_op(op)
                && let reify_syntax::ExprKind::BinOp { op: inner_op, .. } = &left.kind
                && is_comparison_op(inner_op)
            {
                let (operands, ops) = flatten_comparison_chain(op, left, right);
                // Compile each operand exactly once
                let compiled_operands: Vec<CompiledExpr> = operands
                    .iter()
                    .map(|e| {
                        compile_expr_guarded(
                            e,
                            scope,
                            enum_defs,
                            functions,
                            diagnostics,
                            current_guard,
                            lambda_counter,
                        )
                    })
                    .collect();
                // Build pairwise comparison nodes
                let mut pairs: Vec<CompiledExpr> = Vec::new();
                for (i, op_str) in ops.iter().enumerate() {
                    match resolve_binop(op_str) {
                        Some(bin_op) => {
                            let lhs = compiled_operands[i].clone();
                            let rhs = compiled_operands[i + 1].clone();
                            let result_type =
                                infer_binop_type(bin_op, &lhs.result_type, &rhs.result_type);
                            pairs.push(CompiledExpr::binop(bin_op, lhs, rhs, result_type));
                        }
                        None => {
                            // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                            return make_poison_literal(
                                diagnostics,
                                Diagnostic::error(format!("unknown operator: {}", op_str))
                                    .with_label(DiagnosticLabel::new(
                                        expr.span,
                                        "unrecognized operator",
                                    )),
                            );
                        }
                    }
                }
                // Left-fold pairs into And-chain
                let mut acc = pairs.remove(0);
                for pair in pairs {
                    acc = CompiledExpr::binop(BinOp::And, acc, pair, Type::Bool);
                }
                return acc;
            }

            let compiled_left = compile_expr_guarded(
                left,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_right = compile_expr_guarded(
                right,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            match resolve_binop(op) {
                Some(bin_op) => {
                    let result_type = infer_binop_type(
                        bin_op,
                        &compiled_left.result_type,
                        &compiled_right.result_type,
                    );

                    // Dimension compatibility check for Add/Sub
                    if matches!(bin_op, BinOp::Add | BinOp::Sub) {
                        let op_name = if bin_op == BinOp::Add {
                            "addition"
                        } else {
                            "subtraction"
                        };
                        match (&compiled_left.result_type, &compiled_right.result_type) {
                            // Scalar + Scalar with different dimensions
                            (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd })
                                if ld != rd =>
                            {
                                diagnostics.push(format_dimension_mismatch_diagnostic(
                                    op_name,
                                    &compiled_left.result_type,
                                    &compiled_right.result_type,
                                    expr.span,
                                ));
                            }
                            // Scalar + Int/Real or Int/Real + Scalar (dimensioned + dimensionless)
                            (Type::Scalar { .. }, Type::Int | Type::Real)
                            | (Type::Int | Type::Real, Type::Scalar { .. }) => {
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "incompatible types in {}: {} vs {}",
                                        op_name,
                                        compiled_left.result_type,
                                        compiled_right.result_type,
                                    ))
                                    .with_label(
                                        DiagnosticLabel::new(
                                            expr.span,
                                            "dimensioned + dimensionless",
                                        ),
                                    ),
                                );
                            }
                            _ => {}
                        }
                    }

                    CompiledExpr::binop(bin_op, compiled_left, compiled_right, result_type)
                }
                None => {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!("unknown operator: {}", op))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized operator")),
                    )
                }
            }
        }
        reify_syntax::ExprKind::UnOp { op, operand } => {
            let compiled_operand = compile_expr_guarded(
                operand,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            match resolve_unop(op) {
                Some(un_op) => {
                    let result_type = match un_op {
                        UnOp::Not => Type::Bool,
                        UnOp::Neg => compiled_operand.result_type.clone(),
                    };
                    CompiledExpr::unop(un_op, compiled_operand, result_type)
                }
                None => {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!("unknown unary operator: {}", op))
                            .with_label(DiagnosticLabel::new(expr.span, "unrecognized operator")),
                    )
                }
            }
        }
        reify_syntax::ExprKind::Range {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => {
            let compiled_lower = lower.as_ref().map(|e| {
                compile_expr_guarded(
                    e,
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_guard,
                    lambda_counter,
                )
            });
            let compiled_upper = upper.as_ref().map(|e| {
                compile_expr_guarded(
                    e,
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_guard,
                    lambda_counter,
                )
            });
            // Dimensional checking: both bounds must have the same dimension
            if let (Some(lo), Some(hi)) = (&compiled_lower, &compiled_upper) {
                match (&lo.result_type, &hi.result_type) {
                    (Type::Scalar { dimension: ld }, Type::Scalar { dimension: rd })
                        if ld != rd =>
                    {
                        diagnostics.push(format_dimension_mismatch_diagnostic(
                            "range",
                            &lo.result_type,
                            &hi.result_type,
                            expr.span,
                        ));
                    }
                    (Type::Scalar { .. }, Type::Int | Type::Real)
                    | (Type::Int | Type::Real, Type::Scalar { .. }) => {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "incompatible types in range: {} vs {}",
                                lo.result_type, hi.result_type,
                            ))
                            .with_label(DiagnosticLabel::new(
                                expr.span,
                                "dimensioned + dimensionless",
                            )),
                        );
                    }
                    _ => {}
                }
            }
            // Infer the element type from whichever bound is present.
            // NOTE: the parser (lower_range_expr) always provides both lower
            // and upper via `?`, so both being None is an ICE path that is
            // unreachable from user code.
            let element_type = compiled_lower
                .as_ref()
                .map(|e| &e.result_type)
                .or_else(|| compiled_upper.as_ref().map(|e| &e.result_type))
                .cloned()
                .unwrap_or_else(|| {
                    // Anti-cascade (task-1921): Type::Error fallback keeps the ICE diagnostic
                    // from cascading into downstream type-mismatch errors.
                    make_poison_type(
                        diagnostics,
                        Diagnostic::error(
                            "internal compiler error: range has no bounds; cannot infer element type",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "ICE: no lower or upper bound")),
                    )
                });
            let result_type = Type::range(element_type);
            CompiledExpr::range_constructor(
                compiled_lower,
                compiled_upper,
                *lower_inclusive,
                *upper_inclusive,
                result_type,
            )
        }
        reify_syntax::ExprKind::FunctionCall { name, args } => {
            // Intercept `some(expr)` before general function resolution.
            // some() is a language-level constructor, not a user-defined function.
            if name == "some" {
                if !check_arg_count_exact("some", args.len(), 1, expr.span, diagnostics) {
                    // Anti-cascade (task-448/task-1912/task-1921): helper pushes; propagate poison.
                    return propagate_poison();
                }
                let inner = compile_expr_guarded(
                    &args[0],
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_guard,
                    lambda_counter,
                );
                let result_type = Type::Option(Box::new(inner.result_type.clone()));
                return CompiledExpr::option_some(inner, result_type);
            }

            let compiled_args: Vec<CompiledExpr> = args
                .iter()
                .map(|arg| {
                    compile_expr_guarded(
                        arg,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    )
                })
                .collect();

            let arg_types: Vec<Type> = compiled_args
                .iter()
                .map(|a| a.result_type.clone())
                .collect();

            // ── task 3540 (SIR-α): structure-instance ctor lowering ─────────
            // When the callee name resolves to a `structure def`
            // `TopologyTemplate` in `scope.template_registry`, emit a
            // `StructureInstanceCtor` instead of a stdlib `FunctionCall`
            // (precedence over `eval_builtin` — design-decision-2). Identity
            // is `(type_name, version)`; `type_id` is a baked
            // `StructureTypeId(0)` placeholder re-stamped by name on any
            // registry-keyed path (esc-3540-177 RULING 2). `version` is read
            // from the `@version(N)` accessor `template.version()`
            // (esc-3540-176 / RULING 3). Positional args bind to the
            // template's `Param` cells in declaration order; uncovered params
            // contribute their `default_expr`.
            if let Some(registry) = scope.template_registry
                && let Some(template) = registry.get(name.as_str())
                && template.entity_kind == EntityKind::Structure
            {
                let params: Vec<(&str, Option<&CompiledExpr>)> = template
                    .value_cells
                    .iter()
                    .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
                    .map(|vc| (vc.id.member.as_str(), vc.default_expr.as_ref()))
                    .collect();
                let mut ordered_args: Vec<(String, CompiledExpr)> =
                    Vec::with_capacity(compiled_args.len());
                for (i, arg) in compiled_args.iter().enumerate() {
                    let pname = params
                        .get(i)
                        .map(|(n, _)| (*n).to_string())
                        .unwrap_or_else(|| format!("__arg{}", i));
                    ordered_args.push((pname, arg.clone()));
                }
                let covered = ordered_args.len();
                let defaults: Vec<(String, CompiledExpr)> = params
                    .iter()
                    .skip(covered)
                    .filter_map(|(n, d)| d.map(|e| ((*n).to_string(), e.clone())))
                    .collect();
                return CompiledExpr::structure_instance_ctor(
                    reify_types::StructureTypeId(0),
                    name.clone(),
                    template.version(),
                    ordered_args,
                    defaults,
                    Type::StructureRef(name.clone()),
                );
            }

            match resolve_function_overload(name, &arg_types, functions) {
                OverloadResolution::Resolved(matched_fn) => {
                    // Exactly one user fn matches — emit UserFunctionCall
                    // Deprecation check: warn if the called function is @deprecated.
                    if let Some(msg) = deprecation_message(&matched_fn.annotations) {
                        emit_deprecation_warning("function", name, msg, expr.span, diagnostics);
                    }
                    let result_type = matched_fn.return_type.clone();
                    build_user_function_call_expr(name, compiled_args, result_type)
                }
                OverloadResolution::Ambiguous(candidates) => {
                    // Multiple user fns match — ambiguous call
                    let candidate_sigs: Vec<String> =
                        candidates.iter().map(|f| format_fn_signature(f)).collect();
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!(
                            "ambiguous function call: {} candidates match {}({}): {}",
                            candidates.len(),
                            name,
                            arg_types
                                .iter()
                                .map(|t| format!("{}", t))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate_sigs.join(", ")
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "ambiguous call")),
                    )
                }
                OverloadResolution::NoMatch(named_candidates) => {
                    // Before emitting an error, attempt default-padding: find the unique
                    // same-name candidate whose trailing params all have compiled defaults
                    // and whose provided-arg prefix types match exactly.
                    if let Some((padded_fn, default_exprs)) =
                        try_default_padding(&named_candidates, &compiled_args, &arg_types)
                    {
                        let result_type = padded_fn.return_type.clone();
                        // Deprecation check: mirror the Resolved arm — warn if the
                        // padded function is @deprecated.
                        if let Some(msg) = deprecation_message(&padded_fn.annotations) {
                            emit_deprecation_warning(
                                "function",
                                name,
                                msg,
                                expr.span,
                                diagnostics,
                            );
                        }
                        let mut padded_args = compiled_args;
                        padded_args.extend(default_exprs);
                        return build_user_function_call_expr(name, padded_args, result_type);
                    }
                    // User functions with this name exist, but none match — error with candidates
                    let candidate_sigs: Vec<String> = named_candidates
                        .iter()
                        .map(|f| format_fn_signature(f))
                        .collect();
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!(
                            "no matching overload for {}({}), candidates: {}",
                            name,
                            arg_types
                                .iter()
                                .map(|t| format!("{}", t))
                                .collect::<Vec<_>>()
                                .join(", "),
                            candidate_sigs.join(", ")
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "no matching overload")),
                    )
                }
                OverloadResolution::NoUserFunctions => {
                    // Determinacy predicate intrinsics — compiler transforms these
                    // calls into DeterminacyPredicate nodes evaluated by the engine
                    // using the snapshot's DeterminacyState for each ValueCellId.
                    //
                    // User-facing semantic contract:
                    //   determined(x)           — true iff x is fully resolved
                    //                             (state == Determined)
                    //   undetermined(x)         — true iff x has no value
                    //                             (state == Undetermined),
                    //                             regardless of constraints
                    //   constrained(x)          — true iff x is a solver variable
                    //                             (state == Auto || Provisional);
                    //                             tests solver involvement, NOT
                    //                             constraint presence
                    //   partially_determined(x) — true iff x is in solver
                    //                             intermediate state
                    //                             (state == Provisional only);
                    //                             narrowed from original spec to
                    //                             distinguish from Auto (which is
                    //                             covered by constrained())
                    let determinacy_kind = match name.as_str() {
                        "determined" => Some(DeterminacyPredicateKind::Determined),
                        "undetermined" => Some(DeterminacyPredicateKind::Undetermined),
                        "constrained" => Some(DeterminacyPredicateKind::Constrained),
                        "partially_determined" => {
                            Some(DeterminacyPredicateKind::PartiallyDetermined)
                        }
                        _ => None,
                    };

                    if let Some(kind) = determinacy_kind {
                        if !check_arg_count_exact(
                            name,
                            compiled_args.len(),
                            1,
                            expr.span,
                            diagnostics,
                        ) {
                            // Intentional non-Error fallback (task-1921): determinacy predicates
                            // return Type::Bool per the documented poison policy in this module.
                            return CompiledExpr::literal(Value::Undef, Type::Bool);
                        }

                        let arg = &compiled_args[0];
                        if let CompiledExprKind::ValueRef(cell_id) = &arg.kind {
                            return CompiledExpr::determinacy_predicate(kind, cell_id.clone());
                        } else {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "{}() argument must be a direct cell reference, not a computed expression",
                                    name
                                ))
                                .with_label(DiagnosticLabel::new(expr.span, "expected cell reference")),
                            );
                            return CompiledExpr::literal(Value::Undef, Type::Bool);
                        }
                    }

                    // No user fn with this name — fall through to stdlib FunctionCall
                    let resolved = ResolvedFunction {
                        name: name.clone(),
                        qualified_name: format!("std::{}", name),
                    };

                    // Infer a result type — for geometry functions, use a placeholder
                    let result_type = if is_geometry_query_helper(name) {
                        // is_watertight / is_manifold / is_orientable: query helpers
                        // that return Bool. Eval-time dispatch is in
                        // `reify_eval::geometry_ops::try_eval_conformance_query`.
                        // Setting the cell type up-front avoids the first-arg
                        // (Type::Geometry) fallback that would trip
                        // `assert_value_cell_types_representable`.
                        Type::Bool
                    } else if is_geometry_kinematic_query(name) {
                        // interferes / interferes_with / min_clearance: kinematic
                        // query helpers dispatched at eval time by
                        // `reify_eval::geometry_ops::try_eval_kinematic_query`.
                        // Per-name result type (List of pair Maps, Bool, length-
                        // Scalar) is set up-front so the post-process patched
                        // `Value` matches the cell type via
                        // `value_type_kind_matches`. Falling through to the
                        // first-arg (Snapshot Map) default would mismatch.
                        kinematic_query_result_type(name)
                            .expect("is_geometry_kinematic_query implies result type")
                    } else if is_geometry_topology_selector(name) {
                        // closest_point / is_on / angle_between_surfaces: topology-
                        // selector helpers dispatched at eval time by
                        // `reify_eval::geometry_ops::try_eval_topology_selector`.
                        // Per-name result type (Point3<Length> / Bool / Angle)
                        // is set up-front so the post-process patched `Value`
                        // matches the cell type via `value_type_kind_matches`.
                        // Falling through to the first-arg default would
                        // mismatch — the first arg is a Point or a Surface, not
                        // the helper's actual return type.
                        topology_selector_result_type(name)
                            .expect("is_geometry_topology_selector implies result type")
                    } else if is_geometry_query(name) {
                        // volume / area / length / perimeter / centroid /
                        // bounding_box / distance / contains / intersects /
                        // geo_equiv / angle / curvature: the GHR-α / PRD §1
                        // Phase-1 geometry-query family (task 3603). The
                        // per-name result type comes from
                        // `geometry_query_result_type`, which is the frozen
                        // PRD §1 table. Eval-time dispatch arrives in Phase 6
                        // (GHR-ζ); Phase 1 produces `Value::Undef` cells with
                        // the correct compile-time type so downstream
                        // user-asserted-constraint typing and trait conformance
                        // (notably the spec-shape `Physical` trait's
                        // `let mass = volume(geometry) * material.density`
                        // and `let centroid = centroid(geometry)`) typecheck.
                        // Falling through to the first-arg default would
                        // mismatch — the first arg is a `Geometry` / `Solid` /
                        // `Surface` / `Curve` handle, not the helper's actual
                        // return type.
                        geometry_query_result_type(name)
                            .expect("is_geometry_query implies result type")
                    } else if is_geometry_function(name) {
                        Type::dimensionless_scalar()
                    } else if let Some(t) = infer_list_helper_return_type(name, &compiled_args) {
                        t
                    } else {
                        compiled_args
                            .first()
                            .map(|a| a.result_type.clone())
                            .unwrap_or_else(|| {
                                diagnostics.push(
                                    Diagnostic::warning(format!(
                                        "cannot infer return type of zero-arg function '{}', defaulting to Real",
                                        name
                                    ))
                                    .with_label(DiagnosticLabel::new(
                                        expr.span,
                                        "zero-arg function: return type inferred as Real",
                                    )),
                                );
                                Type::Real
                            })
                    };

                    let content_hash = {
                        let mut h = ContentHash::of(&[TAG_FUNCTION_CALL])
                            .combine(ContentHash::of_str(&resolved.qualified_name));
                        for arg in &compiled_args {
                            h = h.combine(arg.content_hash);
                        }
                        h
                    };

                    CompiledExpr {
                        kind: CompiledExprKind::FunctionCall {
                            function: resolved,
                            args: compiled_args,
                        },
                        result_type,
                        content_hash,
                    }
                }
            }
        }
        reify_syntax::ExprKind::MemberAccess { object, member } => {
            // Check if this is a `self.member` or `self.sub.member` access in entity scope.
            if scope.is_entity_scope {
                // Pattern: self.member
                if let reify_syntax::ExprKind::Ident(obj_name) = &object.kind
                    && obj_name == "self"
                {
                    // self.<match-arm cluster> — task 2373.
                    //
                    // When `member` is the logical name of a `match`-block decl
                    // cluster (PRD `match-block-decls.md` §6.4), the static type
                    // is `Type::Union(arm_types)`. The synthetic ValueRef stamp
                    // `__match_arm_group_<member>` is a compile-time sentinel —
                    // no real cell is allocated; `Type::Union` is rejected by
                    // `is_representable_cell_type` so any downstream eval-time
                    // demand on this cell is a clear bug, not a silent miss.
                    //
                    // Narrowing under arm guards is handled in step-15/16 via
                    // `narrow_arms_under_guard`; for now we always return the
                    // full union (correct when `current_guard == None`, which is
                    // the common case for v0.1 surface syntax).
                    if let Some(group) = scope.resolve_match_arm_group(member.as_str()) {
                        let arm_types: Vec<Type> =
                            group.arms.iter().map(|a| a.arm_type.clone()).collect();
                        let synthetic_entity = scope.entity_name.clone();
                        let synthetic_member = format!("__match_arm_group_{}", member);
                        let group_id = ValueCellId::new(&synthetic_entity, &synthetic_member);
                        return CompiledExpr::value_ref(group_id, Type::Union(arm_types));
                    }
                    // self.sub — for single-instance subs, return a StructureRef so outer
                    // chaining works. Collection subs are excluded here and handled below
                    // via resolve_collection_sub_to_list (self.bolts ≡ bare bolts).
                    if scope.sub_component_types.contains_key(member.as_str())
                        && !scope.collection_sub_names.contains(member.as_str())
                    {
                        let structure_name = scope.sub_component_types[member.as_str()].clone();
                        let scoped_entity = format!("{}.{}", scope.entity_name, member);
                        let sub_id = ValueCellId::new(&scoped_entity, "__self");
                        return CompiledExpr::value_ref(sub_id, Type::StructureRef(structure_name));
                    }
                    // Collection sub accessed through self: delegate to the same helper used
                    // by the bare-ident collection-sub resolution in the Identifier arm of
                    // compile_expr_guarded.  Guarantees `self.bolts` ≡ bare `bolts`.
                    if scope.collection_sub_names.contains(member.as_str()) {
                        return resolve_collection_sub_to_list(scope, member.as_str());
                    }
                    // Resolve member from the entity scope (same as bare identifier).
                    match scope.resolve(member) {
                        Some((id, ty)) => {
                            let id = id.clone();
                            let ty = ty.clone();
                            return CompiledExpr::value_ref(id, ty);
                        }
                        None => {
                            // Anti-cascade (task-448/task-1921/task-1969): by-construction
                            // invariant — make_poison_literal pushes the diagnostic and
                            // returns the poison literal in one call.
                            return make_poison_literal(
                                diagnostics,
                                Diagnostic::error(format!("unknown member '{}' on self", member))
                                    .with_label(DiagnosticLabel::new(expr.span, "unknown member")),
                            );
                        }
                    }
                }

                // Pattern: self.<cluster>.<inner> — task 2373 step-12.
                //
                // Cluster-aware nested MemberAccess: when `<cluster>` is a registered
                // match-arm decl group, we walk each arm's child template members and
                // either return the common-field type (when every arm exposes <inner>
                // with the same type) or fall through. This branch must precede the
                // generic `self.<sub>.<member>` branch below because the pre-pass
                // also registers each arm's sub.name in `sub_component_types` (last
                // arm wins) — without an early intercept the merged-map path would
                // mask per-arm differences. Step-14 extends this branch with
                // diagnostics for arms missing the field.
                if let reify_syntax::ExprKind::MemberAccess {
                    object: inner_obj,
                    member: group_name,
                } = &object.kind
                    && let reify_syntax::ExprKind::Ident(self_name) = &inner_obj.kind
                    && self_name == "self"
                    && scope.resolve_match_arm_group(group_name.as_str()).is_some()
                {
                    // Deduplicated call (task 2869): coerce Option<&Vec<ArmMemberMap>> to
                    // &[ArmMemberMap] so the helper's existing empty-slice guard (task
                    // 2373 step-22) fires for both the None and Some(empty) cases. The
                    // helper is the single source of truth for the empty-per_arm
                    // diagnostic at both inner and external call sites.
                    let per_arm: &[ArmMemberMap] = scope
                        .match_arm_group_arm_member_types
                        .get(group_name.as_str())
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    return resolve_cluster_inner_member(
                        per_arm,
                        member,
                        &scope.entity_name,
                        group_name,
                        None,
                        expr.span,
                        diagnostics,
                    );
                }

                // Pattern: self.sub.member (object is MemberAccess { Ident("self"), sub_name }).
                // Single match; branches internally on whether sub_name is a collection sub.
                // Invariant: collection_sub_names ⊆ sub_component_types.keys(), so the outer
                // sub_component_types guard is sufficient to cover both branches.
                if let reify_syntax::ExprKind::MemberAccess {
                    object: inner_obj,
                    member: sub_name,
                } = &object.kind
                    && let reify_syntax::ExprKind::Ident(self_name) = &inner_obj.kind
                    && self_name == "self"
                    && scope.sub_component_types.contains_key(sub_name.as_str())
                {
                    if scope.collection_sub_names.contains(sub_name.as_str()) {
                        // Error: collection sub member accessed directly through self.
                        // Aggregation members (count/sum/keys/values) should use bare sub
                        // access, not indexed access — emit a distinct recommendation.
                        // For members that don't exist on the sub type at all, emit a
                        // generic "unknown member" error rather than suggesting indexed
                        // access to a field that doesn't exist.
                        if COLLECTION_AGGREGATION_MEMBERS.contains(&member.as_str()) {
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "cannot access aggregation '{}' of collection sub '{}' through self; \
                                     use `{}.{}` directly",
                                    member, sub_name, sub_name, member
                                ))
                                .with_label(DiagnosticLabel::new(
                                    expr.span,
                                    "collection sub aggregation: drop `self.`",
                                )),
                            );
                        } else if scope
                            .sub_member_types
                            .get(sub_name.as_str())
                            .is_some_and(|m| m.contains_key(member.as_str()))
                        {
                            // Known struct member — recommend indexed access.
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "cannot access member '{}' of collection sub '{}' directly through self; \
                                     use `{}[i].{}` for a specific instance",
                                    member, sub_name, sub_name, member
                                ))
                                .with_label(DiagnosticLabel::new(
                                    expr.span,
                                    "collection sub member requires indexing",
                                )),
                            );
                        } else if let Some(e) = try_emit_cross_sub_geometry(
                            scope,
                            sub_name,
                            member,
                            expr.span,
                            diagnostics,
                        ) {
                            // Member is a geometry realization — specific cross-sub
                            // diagnostic emitted (task-3397). Type::Error prevents cascade.
                            return e;
                        } else {
                            // Member doesn't exist on the element type at all — don't suggest
                            // indexing a field that isn't there.
                            diagnostics.push(
                                Diagnostic::error(format!(
                                    "unknown member '{}' on collection sub '{}'",
                                    member, sub_name
                                ))
                                .with_label(DiagnosticLabel::new(expr.span, "unknown member")),
                            );
                        }
                        // Determine the fallback type to suppress cascading type-mismatch
                        // diagnostics in downstream expressions.
                        //
                        // Known aggregation members (count/sum/keys/values) are NOT in
                        // sub_member_types (they're methods, not struct fields); infer their
                        // concrete types the same way the general method-call path does.
                        //
                        // For any *unknown* member, the diagnostic above already captures the
                        // root cause.  We must NOT fall back to Type::Real here: doing so lets
                        // downstream BinOp consumers see `Real + Real = Real` and swallow the
                        // error, defeating the Type::Error anti-cascade policy described in
                        // the `make_poison_literal` doc-block in this module and the
                        // `infer_binop_type` Type::Error short-circuit in type_compat.rs.
                        // `unwrap_or(Type::Error)` ensures the literal carries the poison
                        // sentinel so `infer_binop_type` short-circuits correctly.
                        let fallback_type = match member.as_str() {
                            // G-allow: `count`/`sum`/`keys`/`values` are
                            // COLLECTION_AGGREGATION_MEMBERS — the diagnostic at lines
                            // 1272–1282 ("cannot access aggregation through self") already
                            // captures the root cause.  Returning the known concrete type
                            // (Int for count, Real for sum/keys/values) avoids spurious
                            // downstream type-mismatch diagnostics: the user knows the
                            // intended return type of the aggregation method they typed, so
                            // any downstream check against that concrete type is not cascade
                            // (plan design decision #2, task 3639 review).
                            "count" => Type::Int,
                            "sum" | "keys" | "values" => Type::Real,
                            _ => scope
                                .sub_member_types
                                .get(sub_name.as_str())
                                .and_then(|m| m.get(member.as_str()))
                                .cloned()
                                // G-allow: Type::Error is intentional here — unknown member on a
                                // collection sub is an unrecoverable error; poison the literal so
                                // downstream consumers short-circuit (task 3639 review).
                                .unwrap_or(Type::Error),
                        };
                        return CompiledExpr::literal(Value::Undef, fallback_type);
                    }
                    // Non-collection sub: resolve member type from sub_member_types.
                    let member_type = match scope
                        .sub_member_types
                        .get(sub_name.as_str())
                        .and_then(|m| m.get(member.as_str()))
                        .cloned()
                    {
                        Some(ty) => ty,
                        None => {
                            // Cross-sub geometry working path (task 3441):
                            // when `member` is a geometry realisation on the
                            // non-collection sub's child template, return a
                            // value-ref CompiledExpr stamped with the same
                            // `<entity>.<sub>` scope used for scalar cross-sub
                            // access (line 1317 below).  The eval side
                            // (engine_build.rs) populates the matching
                            // compound-key `named_steps["<sub>.<member>"]`
                            // entry, and the parallel `GeomRef::Sub` produced
                            // by `geometry.rs::try_resolve_cross_sub_geom_ref`
                            // resolves through it.
                            if let Some(e) =
                                try_resolve_cross_sub_geometry_value_ref(scope, sub_name, member)
                            {
                                return e;
                            }
                            // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                            return make_poison_literal(
                                diagnostics,
                                Diagnostic::error(format!(
                                    "unknown member '{}' on sub '{}'",
                                    member, sub_name
                                ))
                                .with_label(DiagnosticLabel::new(expr.span, "unknown member")),
                            );
                        }
                    };
                    let scoped_entity = format!("{}.{}", scope.entity_name, sub_name);
                    let scoped_id = ValueCellId::new(&scoped_entity, member);
                    return CompiledExpr::value_ref(scoped_id, member_type);
                }

                // Pattern: <sub>.<cluster>.<inner> — task 2373 step-18.
                //
                // External-scope cluster access: when `<sub>` is a sub of the
                // current entity AND `<cluster>` is a match-arm cluster on the
                // sub's child structure, look up `<inner>` in each arm's child
                // template per the per-arm member maps populated in the entity.rs
                // Sub pre-pass. Step-20 extends this branch with missing-arm
                // diagnostics.
                //
                // Suggestion 5 from review: explicitly skip collection subs
                // here. `bolts.head.X` on a `bolts: List<Bolt>` collection sub
                // must be written as `bolts[i].head.X`; the indexed-access
                // branch below handles the per-instance shape. Without this
                // guard, the cluster-aware path would synthesize a non-list
                // result type and bypass the collection semantics enforced
                // for non-cluster `<sub>.<member>` access.
                if let reify_syntax::ExprKind::MemberAccess {
                    object: inner_obj,
                    member: group_name,
                } = &object.kind
                    && let reify_syntax::ExprKind::Ident(sub_name) = &inner_obj.kind
                    && !scope.collection_sub_names.contains(sub_name.as_str())
                    && let Some(clusters) = scope.sub_match_arm_groups.get(sub_name.as_str())
                    && let Some((_group, per_arm)) =
                        clusters.iter().find(|(g, _)| &g.name == group_name)
                {
                    let scoped_entity = format!("{}.{}", scope.entity_name, sub_name);
                    return resolve_cluster_inner_member(
                        per_arm,
                        member,
                        &scoped_entity,
                        group_name,
                        Some(sub_name),
                        expr.span,
                        diagnostics,
                    );
                }
            }

            // Check if this is a port member access (port_name.member_name)
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && scope.port_names.contains(name.as_str())
            {
                let composite_key = format!("{}.{}", name, member);
                if let Some((id, ty)) = scope.resolve(&composite_key) {
                    let id = id.clone();
                    let ty = ty.clone();
                    return CompiledExpr::value_ref(id, ty);
                } else {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!("port '{}' has no member '{}'", name, member))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown port member")),
                    );
                }
            }

            // Pattern: <col_sub>[i].<cluster>.<inner> — task 2871.
            //
            // Cluster-aware lookup for collection subs accessed via literal index.
            // Detects the deep three-level pattern:
            //   OUTER: MemberAccess { object: M2, member: inner }
            //     M2: MemberAccess { object: IndexAccess(Ident(col_sub), idx), member: cluster }
            //
            // This branch MUST fire before the OUTER MemberAccess falls into
            // `compile_obj` at line ~1349. Once M2 is compiled through the regular
            // indexed-access branch the merged-map type is already baked in; per-arm
            // distinction is unrecoverable from there. Placing the branch here (before
            // the regular indexed-access branch) keeps it at the OUTER MemberAccess
            // level where the shape is still recognizable.
            //
            // For literal integer index `i`, builds
            //   scoped_entity = "<entity>.<col_sub>[<i>]"
            // and dispatches to `resolve_cluster_inner_member` (same helper used by
            // the self-cluster path and the external-sub-cluster path) to handle:
            //   - all arms have `inner` with same type → ValueRef of that type
            //   - all arms have `inner` with divergent types → poison + diagnostic
            //   - some arms missing `inner` → poison + diagnostic
            //
            // For non-literal index: falls through to the regular indexed-access
            // branch below (produces today's behavior; a follow-up task can extend).
            // Cross-reference: scope.sub_match_arm_groups is populated for collection
            // subs in entity.rs:778–810 (the insert fires before the is_collection
            // check at entity.rs:813, so no change needed there).
            if let reify_syntax::ExprKind::MemberAccess {
                object: inner_obj,
                member: group_name,
            } = &object.kind
                && let reify_syntax::ExprKind::IndexAccess {
                    object: idx_obj,
                    index,
                } = &inner_obj.kind
                && let reify_syntax::ExprKind::Ident(col_sub_name) = &idx_obj.kind
                && scope.collection_sub_names.contains(col_sub_name.as_str())
                && let Some(clusters) = scope.sub_match_arm_groups.get(col_sub_name.as_str())
                && let Some((_group, per_arm)) =
                    clusters.iter().find(|(g, _)| &g.name == group_name)
                && let reify_syntax::ExprKind::NumberLiteral { value: n, .. } = &index.kind
            {
                if !n.is_finite() || *n >= i64::MAX as f64 {
                    // Out-of-range or non-finite index in a cluster-routing pattern:
                    // emit the diagnostic directly rather than falling through.
                    // Fall-through would skip the next block (it guards on
                    // `object.kind == IndexAccess`, but here `object.kind` is
                    // `MemberAccess`) and reach `compile_expr_guarded`, which
                    // recurses on `col[i].<cluster>` and emits a misleading
                    // "unknown member '<cluster>' on collection sub" error.
                    // `>= i64::MAX as f64` (not `>`) is required: i64::MAX rounds
                    // UP to 2^63 in f64, so `n = 2^63` satisfies
                    // `n > i64::MAX as f64 == false` and would silently saturate.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error("collection index is out of range or non-finite")
                            .with_label(DiagnosticLabel::new(expr.span, "invalid index")),
                    );
                } else if n.fract() != 0.0 || *n < 0.0 {
                    // Fractional or negative: fall through for a consistent error.
                } else {
                    let i = *n as i64;
                    let scoped_entity = format!("{}.{}[{}]", scope.entity_name, col_sub_name, i);
                    return resolve_cluster_inner_member(
                        per_arm,
                        member,
                        &scoped_entity,
                        group_name,
                        Some(col_sub_name),
                        expr.span,
                        diagnostics,
                    );
                }
                // Non-literal index or fractional/negative: the guards above
                // short-circuit so control falls through to the indexed-access branch.
            }

            // Check if this is an indexed collection member access: collection[i].member
            if let reify_syntax::ExprKind::IndexAccess {
                object: idx_obj,
                index,
            } = &object.kind
                && let reify_syntax::ExprKind::Ident(name) = &idx_obj.kind
                && scope.collection_sub_names.contains(name.as_str())
            {
                // Resolve member type from pre-populated sub_member_types
                let member_type = match scope
                    .sub_member_types
                    .get(name.as_str())
                    .and_then(|m| m.get(member.as_str()))
                    .cloned()
                {
                    Some(ty) => ty,
                    None => {
                        // Check for geometry realization member (task-3397).
                        if let Some(e) =
                            try_emit_cross_sub_geometry(scope, name, member, expr.span, diagnostics)
                        {
                            return e;
                        }
                        // Anti-cascade (task-448/task-1921): return poison early rather than
                        // synthesising a dangling ValueRef to a non-existent cell.
                        return make_poison_literal(
                            diagnostics,
                            Diagnostic::error(format!(
                                "unknown member '{}' on collection sub '{}'",
                                member, name
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown member")),
                        );
                    }
                };

                // For literal integer index, resolve directly to a scoped ValueRef
                if let reify_syntax::ExprKind::NumberLiteral { value: n, .. } = &index.kind {
                    // Task 3045: guard non-finite / out-of-representable-range values first.
                    // `*n as i64` silently saturates to i64::MAX for inputs like 1e20 or
                    // any finite float ≥ 2^63, producing a bogus scoped ValueRef with no
                    // diagnostic.  `>= i64::MAX as f64` uses `>=` because i64::MAX rounds
                    // UP to the f64 value 2^63 (not representable exactly), so the
                    // boundary must be included.  `!n.is_finite()` catches NaN and ±∞
                    // before the `>=` comparison (NaN comparisons are always false).
                    if !n.is_finite() || *n >= i64::MAX as f64 {
                        return make_poison_literal(
                            diagnostics,
                            Diagnostic::error("collection index is out of range or non-finite")
                                .with_label(DiagnosticLabel::new(expr.span, "invalid index")),
                        );
                    }
                    if n.fract() != 0.0 || *n < 0.0 {
                        // Anti-cascade (task-1921): use make_poison_literal (Type::Error)
                        // rather than Value::Undef typed as member_type. A fractional or
                        // negative index means the index expression is fundamentally invalid;
                        // returning member_type would let downstream type-checks see a
                        // "well-typed" hole and potentially emit cascade diagnostics.
                        return make_poison_literal(
                            diagnostics,
                            Diagnostic::error(
                                "collection index must be a non-negative integer literal",
                            )
                            .with_label(DiagnosticLabel::new(expr.span, "invalid index")),
                        );
                    }
                    let i = *n as i64;
                    let scoped_entity = format!("{}.{}[{}]", scope.entity_name, name, i);
                    let scoped_id = ValueCellId::new(&scoped_entity, member);
                    return CompiledExpr::value_ref(scoped_id, member_type);
                }
                // For non-literal index, compile as IndexAccess into a per-member synthetic list.
                // The eval engine creates __list_{name}__{member} cells that gather each
                // instance's member value into a List, so indexing gives the right value.
                let list_member = format!("__list_{}__{}", name, member);
                let list_id = ValueCellId::new(&scope.entity_name, &list_member);
                let collection_ref =
                    CompiledExpr::value_ref(list_id, Type::List(Box::new(member_type.clone())));
                diagnostics.push(
                    Diagnostic::info(format!(
                        "dynamic collection index: {}[<expr>].{} — result depends on runtime list assembly",
                        name, member
                    ))
                );
                let compiled_idx = compile_expr_guarded(
                    index,
                    scope,
                    enum_defs,
                    functions,
                    diagnostics,
                    current_guard,
                    lambda_counter,
                );
                return CompiledExpr::index_access(collection_ref, compiled_idx, member_type);
            }

            // Check if this is a collection sub member access: collection.count
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && scope.collection_sub_names.contains(name.as_str())
                && member == "count"
            {
                // Resolve to the synthetic __count_ cell
                let count_member = format!("__count_{}", name);
                let count_id = ValueCellId::new(&scope.entity_name, &count_member);
                return CompiledExpr::value_ref(count_id, Type::Int);
            }

            // Check if this is a meta block access: meta.key
            if let reify_syntax::ExprKind::Ident(name) = &object.kind
                && name == "meta"
            {
                if !scope.has_meta_block {
                    diagnostics.push(
                        Diagnostic::error("entity has no meta block".to_string())
                            .with_label(DiagnosticLabel::new(expr.span, "no meta block")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::String);
                }
                if scope.meta_entries.contains_key(member.as_str()) {
                    return CompiledExpr::meta_access(scope.entity_name.clone(), member.clone());
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!("meta block has no key: {}", member))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown meta key")),
                    );
                    return CompiledExpr::literal(Value::Undef, Type::String);
                }
            }

            // For non-port member access, check if it's a known collection method
            let compiled_obj = compile_expr_guarded(
                object,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );

            // ── Purpose-subject member access (task-2181) ──────────────────────
            //
            // Trigger: compiled_obj is a ValueRef whose entity stamp equals the
            // current scope's entity name (= the purpose name) AND its type is
            // StructureRef(_) AND we are NOT in entity scope.
            //
            // The `!scope.is_entity_scope` guard prevents misfiring in entity
            // bodies: `param material : Material` in a structure registers
            // `material` as Type::StructureRef("Material") when Material is a
            // known structure name.  Without the guard, `material.density` in a
            // structure constraint would silently emit
            // `ValueRef(entity_name, "density")` — a cell that doesn't exist —
            // rather than the correct "member access not yet supported" error.
            // Purpose scopes have is_entity_scope=false (traits.rs:228 uses
            // CompilationScope::new); entity scopes set is_entity_scope=true
            // (entity.rs:247).
            //
            // Combining the outer type-check with the inner ValueRef pattern into
            // a single `if let` removes a statically infallible inner match and
            // makes the control flow unambiguous — no implicit fall-through.
            //
            // Anti-cascade: this branch is placed AFTER the compile_obj call so
            // the existing `is_error()` poison short-circuit below still fires
            // for already-poisoned subjects.
            //
            // Single-StructureRef-param invariant (task-2201): the
            // `ValueCellId::new(&id.entity, member)` emit at line ~1222 below
            // collapses ALL purpose-subject member refs onto the purpose-name
            // entity stamp (id.entity == scope.entity_name == purpose_name).
            // This is correct only when the purpose has exactly one StructureRef
            // param, because `activate_purpose` rewrites the stamp with a single
            // `remap_entity(purpose_name, entity_ref)` call
            // (reify-types/src/expr.rs:660) — there is no per-param dispatch.
            // `compile_purpose` (traits.rs) rejects multi-param purposes with a
            // clear diagnostic before this branch ever runs for them; see
            // esc-2181-18 S3 for the deferred Approach-2 design.
            if let CompiledExprKind::ValueRef(ref id) = compiled_obj.kind
                && matches!(&compiled_obj.result_type, Type::StructureRef(_))
                && id.entity == scope.entity_name
                && !scope.is_entity_scope
            {
                if PURPOSE_REFLECTIVE_AGGREGATION_MEMBERS.contains(&member.as_str()) {
                    // Reflective-aggregation placeholder (task-2289).
                    //
                    // Emits the marker variant `PurposeReflectiveAggregation`,
                    // which `Engine::activate_purpose` (in
                    // `crates/reify-eval/src/engine_purposes.rs`) walks and
                    // replaces with a populated `ListLiteral` of `ValueRef`s
                    // built from `CompiledPurpose.resolved_queries`. For the
                    // currently-resolved `params` query that yields the bound
                    // entity's param cells, flipping `forall p in
                    // subject.params: determined(p)` from a vacuous-true
                    // result to a real check. For `geometric_params`/
                    // `material_params` the activation walk currently emits an
                    // empty list (no resolved query — task-1904 follow-up
                    // territory), preserving today's vacuous-true behaviour.
                    //
                    // The compile-time placeholder element type stays
                    // `List<Real>`; activation refines each element's
                    // `result_type` from the looked-up `ValueCellNode.cell_type`.
                    //
                    // See `docs/notes/purpose-reflective-aggregation.md` for the
                    // full rationale and the §8 acceptance test in
                    // `crates/reify-eval/tests/purpose_activation.rs`.
                    return CompiledExpr::purpose_reflective_aggregation(
                        id.member.clone(),
                        member.clone(),
                        Type::List(Box::new(Type::Real)),
                    );
                } else {
                    // Regular member access (e.g., `subject.mass`):
                    //   - Emit a ValueRef whose entity stamp equals the purpose
                    //     name (= scope.entity_name).  At activation time,
                    //     `activate_purpose` calls `remap_entity(purpose_name,
                    //     entity_ref)` which rewrites this ref to
                    //     `ValueCellId(entity_ref, member)` — exactly the bound
                    //     entity's member cell.
                    //   - Concrete-subject validation (task-2200): when the subject
                    //     type is a named structure (not the generic "Structure"
                    //     wildcard) and template_registry is available, verify that
                    //     `member` is declared in the template (value_cells, ports,
                    //     or sub_components).  If not found in any, emit
                    //     "has no member" and return a Type::Error poison so
                    //     downstream checks (e.g., `subject.bogus > 0`) do not
                    //     cascade.  Port/sub members fall through to the existing
                    //     CompiledExpr::value_ref emit — their type resolution is a
                    //     separate follow-up task.
                    //   - Wildcard path: when entity_kind == "Structure" or registry
                    //     lookup fails (no template by that name), fall through
                    //     silently — the generic form binds at activation time and
                    //     has no static template to validate against.
                    //   - Belt-and-braces: `struct_name != WILDCARD_STRUCTURE_KIND` makes
                    //     the wildcard-skip intent explicit even though a registry miss
                    //     (no template named "Structure") would also fall through.
                    //     Both guards are intentional: the name guard protects
                    //     against a hypothetical future stdlib "Structure" template;
                    //     the registry-miss guard covers other unregistered wildcard
                    //     kinds (e.g., "Occurrence").
                    //   - Type::Real is a compile-time fallback; member-type
                    //     resolution (e.g., Length vs. Mass) is a separate
                    //     follow-up task and is NOT addressed here.
                    let struct_name = match &compiled_obj.result_type {
                        Type::StructureRef(name) => name.clone(),
                        _ => unreachable!("outer guard ensures StructureRef"),
                    };
                    if struct_name != WILDCARD_STRUCTURE_KIND
                        && let Some(registry) = scope.template_registry
                        && let Some(template) = registry.get(struct_name.as_str())
                    {
                        // Accept members from value_cells, ports, or sub_components.
                        // Port/sub members are valid member kinds even if their type
                        // resolution is not yet implemented — only truly undeclared
                        // names get a "has no member" diagnostic.
                        let member_known = template
                            .value_cells
                            .iter()
                            .any(|vc| vc.id.member == *member)
                            || template.ports.iter().any(|p| p.name == *member)
                            || template.sub_components.iter().any(|sc| sc.name == *member);
                        if !member_known {
                            return make_poison_literal(
                                diagnostics,
                                Diagnostic::error(format!(
                                    "structure '{}' has no member '{}'",
                                    struct_name, member
                                ))
                                .with_label(DiagnosticLabel::new(expr.span, "unknown member")),
                            );
                        }
                    }
                    let member_id = ValueCellId::new(&id.entity, member);
                    return CompiledExpr::value_ref(member_id, Type::Real);
                }
            }
            // ── End purpose-subject member access ──────────────────────────────

            // ── task 3540 (SIR-α): StructureInstance field projection ──────────
            //
            // Handler esc-3540-182 (A): when the object resolves to a
            // structure/trait-typed value, `.member` projects the field out of
            // the runtime `Value::StructureInstance`. This is the entity-scope
            // member-access path for chains like
            // `self.primary.material.youngs_modulus` — `self.primary.material`
            // already resolves (via the `self.sub.member` branch above) to a
            // value-ref whose runtime value is a `Value::StructureInstance`
            // (the structure-def param/let default lowered by the
            // StructureInstanceCtor path). Reuse `IndexAccess` with a
            // string-literal key (handler (A)(1) — no new CompiledExprKind);
            // the eval-side IndexAccess arm reads `fields[member]`.
            //
            // (A)(2) member-Type resolution: for a concrete `StructureRef`,
            // resolve the declared field type from the structure-def template
            // in `scope.template_registry` (esc-3540-177-threaded). For a
            // `TraitObject` the concrete runtime type is not statically known
            // (traits are not in `template_registry`); fall back to `Type::Real`
            // — a permissive, non-poison type so the chain neither cascades nor
            // is rejected. The runtime `Value` is whatever the field actually
            // holds (e.g. a `Value::Scalar`), independent of this static type.
            //
            // The poison short-circuit must run first so an already-errored
            // object propagates rather than being treated as a structure.
            if !compiled_obj.result_type.is_error()
                && let Type::StructureRef(struct_name) | Type::TraitObject(struct_name) =
                    &compiled_obj.result_type
            {
                let member_type = scope
                    .template_registry
                    .and_then(|r| r.get(struct_name.as_str()))
                    .and_then(|t| {
                        t.value_cells
                            .iter()
                            .find(|vc| vc.id.member == *member)
                            .map(|vc| vc.cell_type.clone())
                    })
                    .unwrap_or(Type::Real);
                let key = CompiledExpr::literal(Value::String(member.clone()), Type::String);
                return CompiledExpr::index_access(compiled_obj, key, member_type);
            }

            if COLLECTION_AGGREGATION_MEMBERS.contains(&member.as_str()) {
                // Anti-cascade consumer (task-448 / task-1921 S4): if the object
                // is already poisoned, propagate via propagate_poison() (a
                // Literal node) rather than emitting a dead MethodCall that
                // downstream passes could try to evaluate.  This is a consumer
                // propagating an existing poison — NOT a new producer — so
                // make_poison_literal does not apply (no new diagnostic is
                // pushed).  Cross-reference: module-header policy.
                if compiled_obj.result_type.is_error() {
                    return propagate_poison();
                }
                // Infer result type from method and object type
                let result_type = match member.as_str() {
                    "count" => Type::Int,
                    "sum" => match &compiled_obj.result_type {
                        Type::List(inner) => (**inner).clone(),
                        _ => Type::Real,
                    },
                    "keys" => match &compiled_obj.result_type {
                        Type::Map(k, _) => Type::List(k.clone()),
                        _ => Type::List(Box::new(Type::Real)),
                    },
                    "values" => match &compiled_obj.result_type {
                        Type::Map(_, v) => Type::List(v.clone()),
                        _ => Type::List(Box::new(Type::Real)),
                    },
                    // task-2066 amend: this arm is structurally unreachable today — the outer
                    // `if COLLECTION_AGGREGATION_MEMBERS.contains(...)` guard constrains `member`
                    // to one of count/sum/keys/values, each of which has an explicit arm above.
                    // `debug_assert!(false, ...)` panics in debug/test builds to detect drift
                    // between the const and this match early; in release builds we fall back to an
                    // error diagnostic + Type::Error (anti-cascade policy) rather than an ICE.
                    // If you extend COLLECTION_AGGREGATION_MEMBERS, add a matching arm here.
                    _ => {
                        debug_assert!(
                            false,
                            "COLLECTION_AGGREGATION_MEMBERS restricts member to \
                             count/sum/keys/values; extend the inner match when you extend the const"
                        );
                        make_poison_type(
                            diagnostics,
                            Diagnostic::error(format!(
                                "internal: unknown aggregation member '{}'; \
                                 expected one of count/sum/keys/values",
                                member
                            ))
                            .with_label(DiagnosticLabel::new(
                                expr.span,
                                "unknown aggregation member",
                            )),
                        )
                    }
                };
                CompiledExpr::method_call(compiled_obj, member.clone(), vec![], result_type)
            } else {
                // Already-poisoned short-circuit: root-cause error was reported
                // at the producer site, so we do not push a new diagnostic here.
                // Use propagate_poison() — the no-assert consumer helper — per
                // the policy described in the module header.
                if compiled_obj.result_type.is_error() {
                    return propagate_poison();
                }
                // Anti-cascade (task-448/task-1921/task-1969): by-construction
                // invariant — make_poison_literal pushes the diagnostic and
                // returns the poison literal in one call.
                make_poison_literal(
                    diagnostics,
                    Diagnostic::error(format!("member access not yet supported: .{}", member))
                        .with_label(DiagnosticLabel::new(expr.span, "unsupported")),
                )
            }
        }
        reify_syntax::ExprKind::ListLiteral(elements) => {
            let compiled_elems: Vec<CompiledExpr> = elements
                .iter()
                .map(|e| {
                    compile_expr_guarded(
                        e,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    )
                })
                .collect();
            // Infer element type from first element, warn and default to Real for empty lists
            let elem_type = compiled_elems
                .first()
                .map(|e| e.result_type.clone())
                .unwrap_or_else(|| {
                    diagnostics.push(
                        Diagnostic::warning(
                            "cannot infer element type of empty list literal, defaulting to Real",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "empty list")),
                    );
                    Type::Real
                });
            let result_type = Type::List(Box::new(elem_type));
            CompiledExpr::list_literal(compiled_elems, result_type)
        }
        reify_syntax::ExprKind::SetLiteral(elements) => {
            let compiled_elems: Vec<CompiledExpr> = elements
                .iter()
                .map(|e| {
                    compile_expr_guarded(
                        e,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    )
                })
                .collect();
            let elem_type = compiled_elems
                .first()
                .map(|e| e.result_type.clone())
                .unwrap_or_else(|| {
                    diagnostics.push(
                        Diagnostic::warning(
                            "cannot infer element type of empty set literal, defaulting to Real",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "empty set")),
                    );
                    Type::Real
                });
            let result_type = Type::Set(Box::new(elem_type));
            CompiledExpr::set_literal(compiled_elems, result_type)
        }
        reify_syntax::ExprKind::MapLiteral(entries) => {
            let compiled_entries: Vec<(CompiledExpr, CompiledExpr)> = entries
                .iter()
                .map(|(k, v)| {
                    let ck = compile_expr_guarded(
                        k,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    );
                    let cv = compile_expr_guarded(
                        v,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    );
                    (ck, cv)
                })
                .collect();
            let key_type = compiled_entries
                .first()
                .map(|(k, _)| k.result_type.clone())
                .unwrap_or_else(|| {
                    diagnostics.push(
                        Diagnostic::warning(
                            "cannot infer key type of empty map literal, defaulting to String",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "empty map")),
                    );
                    Type::String
                });
            let val_type = compiled_entries
                .first()
                .map(|(_, v)| v.result_type.clone())
                .unwrap_or_else(|| {
                    // Warning already emitted for empty map at key_type step above;
                    // no second warning needed for the value type.
                    Type::Real
                });
            let result_type = Type::Map(Box::new(key_type), Box::new(val_type));
            CompiledExpr::map_literal(compiled_entries, result_type)
        }
        reify_syntax::ExprKind::IndexAccess { object, index } => {
            let compiled_obj = compile_expr_guarded(
                object,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_idx = compile_expr_guarded(
                index,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            // Infer result type from collection's element type.
            // Anti-cascade guard (task-448): if the object is already
            // poisoned, propagate Type::Error rather than falling back to
            // Type::Real.
            let result_type = if compiled_obj.result_type.is_error() {
                Type::Error
            } else {
                match &compiled_obj.result_type {
                    Type::List(inner) => (**inner).clone(),
                    Type::Map(_, val) => (**val).clone(),
                    // task-2066: emit a diagnostic instead of silently defaulting to Type::Real.
                    // Anti-cascade policy: Type::Error propagates downstream via existing
                    // is_error() guards so no cascade of type-mismatch errors follows.
                    _ => {
                        make_poison_type(
                            diagnostics,
                            Diagnostic::error(format!(
                                "cannot index into non-collection type '{}': expected List<_> or Map<_,_>",
                                compiled_obj.result_type
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "not indexable")),
                        )
                    }
                }
            };
            CompiledExpr::index_access(compiled_obj, compiled_idx, result_type)
        }
        reify_syntax::ExprKind::EnumAccess { type_name, variant } => {
            // Look up the enum type in the registry
            if let Some(enum_def) = enum_defs.iter().find(|e| e.name == *type_name) {
                if enum_def.contains_variant(variant) {
                    CompiledExpr::literal(
                        Value::Enum {
                            type_name: type_name.clone(),
                            variant: variant.clone(),
                        },
                        Type::Enum(type_name.clone()),
                    )
                } else {
                    diagnostics.push(
                        Diagnostic::error(format!(
                            "unknown variant '{}' on enum '{}'",
                            variant, type_name
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "unknown variant")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Enum(type_name.clone()))
                }
            } else {
                // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                make_poison_literal(
                    diagnostics,
                    Diagnostic::error(format!("unknown enum type '{}'", type_name))
                        .with_label(DiagnosticLabel::new(expr.span, "unknown enum")),
                )
            }
        }
        reify_syntax::ExprKind::Match { discriminant, arms } => {
            let compiled_discriminant = compile_expr_guarded(
                discriminant,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_arms: Vec<reify_types::CompiledMatchArm> = arms
                .iter()
                .map(|arm| {
                    let body = compile_expr_guarded(
                        &arm.body,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    );
                    reify_types::CompiledMatchArm {
                        patterns: arm.patterns.clone(),
                        body,
                    }
                })
                .collect();

            // Result type from the first arm's body.
            // NOTE: the grammar requires at least one arm so an empty arms
            // list is an ICE path unreachable from user code.
            let result_type = compiled_arms
                .first()
                .map(|a| a.body.result_type.clone())
                .unwrap_or_else(|| {
                    // Anti-cascade (task-1921): Type::Error fallback keeps the ICE diagnostic
                    // from cascading into downstream type-mismatch errors.
                    make_poison_type(
                        diagnostics,
                        Diagnostic::error(
                            "internal compiler error: match expression has no arms; cannot infer result type",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "ICE: match with no arms")),
                    )
                });

            // Exhaustiveness check: if discriminant is a known enum type,
            // verify all variants are covered by arm patterns or a wildcard.
            if let Type::Enum(ref enum_name) = compiled_discriminant.result_type
                && let Some(enum_def) = enum_defs.iter().find(|e| e.name == *enum_name)
            {
                let has_wildcard = compiled_arms
                    .iter()
                    .any(|arm| arm.patterns.iter().any(|p| p == "_"));

                if !has_wildcard {
                    let covered: std::collections::HashSet<&str> = compiled_arms
                        .iter()
                        .flat_map(|arm| arm.patterns.iter().map(|p| p.as_str()))
                        .collect();

                    let missing: Vec<&str> = enum_def
                        .variants
                        .iter()
                        .filter(|v| !covered.contains(v.as_str()))
                        .map(|v| v.as_str())
                        .collect();

                    if !missing.is_empty() {
                        diagnostics.push(
                            Diagnostic::error(format!(
                                "non-exhaustive match on '{}': missing variant(s) {}",
                                enum_name,
                                missing.join(", ")
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "missing variants")),
                        );
                    }
                }
            }

            // Content hash: tag TAG_MATCH + discriminant + all arms
            let mut content_hash =
                ContentHash::of(&[TAG_MATCH]).combine(compiled_discriminant.content_hash);
            for arm in &compiled_arms {
                for pattern in &arm.patterns {
                    content_hash = content_hash.combine(ContentHash::of_str(pattern));
                }
                content_hash = content_hash.combine(arm.body.content_hash);
            }

            CompiledExpr {
                kind: CompiledExprKind::Match {
                    discriminant: Box::new(compiled_discriminant),
                    arms: compiled_arms,
                },
                result_type,
                content_hash,
            }
        }
        reify_syntax::ExprKind::Auto { .. } => {
            // Auto expressions should not appear inside compile_expr — they are
            // handled at the param compilation level. If we reach here, emit an
            // Undef literal as a safe fallback.
            CompiledExpr::literal(Value::Undef, Type::Real)
        }
        reify_syntax::ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            let compiled_cond = compile_expr_guarded(
                condition,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_then = compile_expr_guarded(
                then_branch,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let compiled_else = compile_expr_guarded(
                else_branch,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );
            let result_type = compiled_then.result_type.clone();

            let content_hash = ContentHash::of(&[TAG_CONDITIONAL])
                .combine(compiled_cond.content_hash)
                .combine(compiled_then.content_hash)
                .combine(compiled_else.content_hash);

            CompiledExpr {
                kind: CompiledExprKind::Conditional {
                    condition: Box::new(compiled_cond),
                    then_branch: Box::new(compiled_then),
                    else_branch: Box::new(compiled_else),
                },
                result_type,
                content_hash,
            }
        }
        reify_syntax::ExprKind::Lambda { params, body } => {
            let lambda_entity = format!("$lambda{}.{}", lambda_counter, scope.entity_name);
            *lambda_counter += 1;

            let mut lambda_scope = scope.clone();
            let mut compiled_params: Vec<(String, Option<Type>)> = Vec::new();
            let mut param_types: Vec<Type> = Vec::new();
            let mut param_ids: Vec<ValueCellId> = Vec::new();

            for param in params {
                let ty = if let Some(type_expr) = &param.type_expr {
                    // Extract name from Named; DimensionalOp can't appear as a lambda param type.
                    let name_opt = match &type_expr.kind {
                        reify_syntax::TypeExprKind::Named { name, .. } => Some(name.as_str()),
                        reify_syntax::TypeExprKind::DimensionalOp { .. } => None,
                        reify_syntax::TypeExprKind::IntegerLiteral(_) => None,
                        // Auto type-args cannot be used as lambda param types;
                        // resolution semantics are deferred to task 3477/3558.
                        reify_syntax::TypeExprKind::Auto { .. } => None,
                    };
                    if let Some(name) = name_opt {
                        match resolve_type_name(name) {
                            Some(t) => t,
                            None => {
                                // Anti-cascade (task-1921): Type::Error propagates through body
                                // via consumer guards in infer_binop_type / implicitly_converts_to.
                                make_poison_type(
                                    diagnostics,
                                    Diagnostic::error(format!(
                                        "unresolved type in lambda param '{}': {}",
                                        param.name, name
                                    ))
                                    .with_code(DiagnosticCode::UnresolvedType),
                                )
                            }
                        }
                    } else {
                        // Anti-cascade (task-1921): same rationale as Named arm above.
                        make_poison_type(
                            diagnostics,
                            Diagnostic::error(format!(
                                "unresolved type in lambda param '{}': {}",
                                param.name, type_expr
                            ))
                            .with_code(DiagnosticCode::UnresolvedType),
                        )
                    }
                } else {
                    Type::Real // default untyped params to Real
                };

                let param_id = ValueCellId::new(&lambda_entity, &param.name);
                lambda_scope
                    .names
                    .insert(param.name.clone(), (param_id.clone(), ty.clone(), None));

                param_ids.push(param_id);
                param_types.push(ty.clone());
                compiled_params.push((param.name.clone(), param.type_expr.as_ref().map(|_| ty)));
            }

            // Compile body in the nested scope
            let compiled_body = compile_expr_guarded(
                body,
                &lambda_scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );

            // Capture analysis: collect ValueRefs in body, filter out lambda params
            let lambda_param_set: HashSet<ValueCellId> = param_ids.iter().cloned().collect();
            let all_refs = collect_body_refs(&compiled_body);
            let mut seen = HashSet::new();
            let mut captures: Vec<ValueCellId> = Vec::new();
            for id in all_refs {
                if !lambda_param_set.contains(&id) && seen.insert(id.clone()) {
                    captures.push(id);
                }
            }

            let return_type = compiled_body.result_type.clone();
            let result_type = Type::Function {
                params: param_types,
                return_type: Box::new(return_type),
            };

            CompiledExpr::lambda(
                compiled_params,
                param_ids,
                compiled_body,
                captures,
                result_type,
            )
        }
        reify_syntax::ExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
        } => {
            let quant_entity = format!("$quant{}.{}", lambda_counter, scope.entity_name);
            *lambda_counter += 1;

            // Compile collection in the outer scope
            let compiled_collection = compile_expr_guarded(
                collection,
                scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );

            // Create a nested scope with the bound variable
            let mut quant_scope = scope.clone();
            let variable_id = ValueCellId::new(&quant_entity, variable);
            // Infer element type from the collection's result type.
            // Anti-cascade guard (task-448): if the collection is already
            // poisoned, propagate Type::Error into elem_type rather than
            // falling back to Type::Real.
            let elem_type = if compiled_collection.result_type.is_error() {
                Type::Error
            } else {
                match &compiled_collection.result_type {
                    Type::List(elem) | Type::Set(elem) => *elem.clone(),
                    // task-2066: emit a diagnostic instead of silently defaulting to Type::Real.
                    // Type::Error propagates into quant_scope so the bound variable also
                    // carries Type::Error; existing is_error() guards in the predicate suppress
                    // cascade (anti-cascade policy).
                    _ => {
                        make_poison_type(
                            diagnostics,
                            Diagnostic::error(format!(
                                "cannot iterate over non-collection type '{}' in forall/exists: expected List<_> or Set<_>",
                                compiled_collection.result_type
                            ))
                            .with_label(DiagnosticLabel::new(expr.span, "not iterable")),
                        )
                    }
                }
            };
            quant_scope
                .names
                .insert(variable.clone(), (variable_id.clone(), elem_type, None));

            // Compile predicate in the nested scope
            let compiled_predicate = compile_expr_guarded(
                predicate,
                &quant_scope,
                enum_defs,
                functions,
                diagnostics,
                current_guard,
                lambda_counter,
            );

            let compiled_kind = match kind {
                reify_syntax::QuantifierKind::ForAll => reify_types::QuantifierKind::ForAll,
                reify_syntax::QuantifierKind::Exists => reify_types::QuantifierKind::Exists,
            };

            CompiledExpr::quantifier(
                compiled_kind,
                variable.clone(),
                variable_id,
                compiled_collection,
                compiled_predicate,
            )
        }
        reify_syntax::ExprKind::AdHocSelector {
            base,
            selector,
            args,
        } => {
            // Resolve selector kind.
            // `n` is captured immediately before the push inside the `unknown` arm so it
            // cannot be falsely whitelisted by any future diagnostic added to the other arms.
            let selector_kind = match selector.as_str() {
                "face" => SelectorKind::Face,
                "point" => SelectorKind::Point,
                "edge" => SelectorKind::Edge,
                unknown => {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!(
                            "unknown selector kind '@{}'; expected face, point, or edge",
                            unknown
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "unknown selector")),
                    );
                }
            };

            // Validate argument count and types per selector kind
            match selector_kind {
                SelectorKind::Face | SelectorKind::Edge => {
                    if args.len() != 1 {
                        // Anti-cascade (task-448/task-1912/task-1921): helper pushes; propagate poison.
                        push_labeled_arg_count_error(
                            format!(
                                "@{} expects exactly 1 argument (a string name), got {}",
                                selector,
                                args.len()
                            ),
                            expr.span,
                            diagnostics,
                        );
                        return propagate_poison();
                    }
                    // Check that the argument is a string literal (type check)
                    if let reify_syntax::ExprKind::NumberLiteral { .. } = &args[0].kind {
                        // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                        return make_poison_literal(
                            diagnostics,
                            Diagnostic::error(format!(
                                "@{} expects a string argument for the face/edge name, got a numeric type",
                                selector
                            ))
                            .with_label(DiagnosticLabel::new(
                                args[0].span,
                                "expected string",
                            )),
                        );
                    }
                }
                SelectorKind::Point => {
                    if args.len() != 3 {
                        // Anti-cascade (task-448/task-1912/task-1921): helper pushes; propagate poison.
                        push_labeled_arg_count_error(
                            format!(
                                "@point expects exactly 3 coordinate arguments, got {}",
                                args.len()
                            ),
                            expr.span,
                            diagnostics,
                        );
                        return propagate_poison();
                    }
                }
            }

            // Geometry availability check: @face/@edge on a direct port in the current
            // scope requires the structure to have geometry declarations.
            if matches!(selector_kind, SelectorKind::Face | SelectorKind::Edge) {
                let is_direct_port = matches!(&base.kind, reify_syntax::ExprKind::Ident(name) if scope.port_names.contains(name.as_str()));
                if is_direct_port && !scope.has_geometry {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!(
                            "@{} requires the structure to have geometry, but no geometry declarations found",
                            selector
                        ))
                        .with_label(DiagnosticLabel::new(
                            expr.span,
                            "no geometry in this structure",
                        )),
                    );
                }
            }

            // Resolve the base expression as a port reference. Ports are not
            // regular value cells so we compile the base to a string literal
            // containing the port path. The evaluator (task 250) interprets
            // this to find the geometry context.
            let compiled_base = match &base.kind {
                reify_syntax::ExprKind::Ident(name) => {
                    // Validate: must be a known port or a scope variable (e.g. forall var)
                    if !scope.port_names.contains(name.as_str()) && scope.resolve(name).is_none() {
                        // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                        return make_poison_literal(
                            diagnostics,
                            Diagnostic::error(format!(
                                "unresolved port or variable '{}' in ad-hoc selector",
                                name
                            ))
                            .with_label(DiagnosticLabel::new(base.span, "unknown name")),
                        );
                    }
                    CompiledExpr::literal(Value::String(name.clone()), Type::String)
                }
                reify_syntax::ExprKind::MemberAccess { object, member } => {
                    // Sub-component or variable member: "sub.port" or "var.port"
                    if let reify_syntax::ExprKind::Ident(obj_name) = &object.kind {
                        CompiledExpr::literal(
                            Value::String(format!("{}.{}", obj_name, member)),
                            Type::String,
                        )
                    } else {
                        // Complex base expression — compile normally
                        compile_expr_guarded(
                            base,
                            scope,
                            enum_defs,
                            functions,
                            diagnostics,
                            current_guard,
                            lambda_counter,
                        )
                    }
                }
                _ => {
                    // Anything else — compile normally
                    compile_expr_guarded(
                        base,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    )
                }
            };

            let compiled_args: Vec<CompiledExpr> = args
                .iter()
                .map(|arg| {
                    compile_expr_guarded(
                        arg,
                        scope,
                        enum_defs,
                        functions,
                        diagnostics,
                        current_guard,
                        lambda_counter,
                    )
                })
                .collect();

            CompiledExpr::ad_hoc_selector(compiled_base, selector_kind, compiled_args)
        }
        reify_syntax::ExprKind::QualifiedAccess { qualifier, member } => {
            // Resolve `TraitName::member` to the member's ValueCellId in the current scope.
            // Only simple `Ident::member` form is supported.
            let trait_name = match &qualifier.kind {
                reify_syntax::ExprKind::Ident(name) => name.clone(),
                _ => {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error(
                            "unsupported qualified access: only 'TraitName::member' form is supported",
                        )
                        .with_label(DiagnosticLabel::new(expr.span, "unsupported form")),
                    );
                }
            };

            // Validate trait existence.
            let members = match scope.trait_members.get(&trait_name) {
                None => {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!("trait '{}' not found", trait_name))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown trait")),
                    );
                }
                Some(m) => m,
            };

            // Validate member existence in trait.
            if !members.contains(member.as_str()) {
                // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                return make_poison_literal(
                    diagnostics,
                    Diagnostic::error(format!(
                        "member '{}' not defined in trait '{}'",
                        member, trait_name
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "not in trait")),
                );
            }

            // Resolve the member in the current scope (the structure should have it
            // because it conforms to the trait).
            match scope.resolve(member) {
                Some((id, ty)) => CompiledExpr::value_ref(id.clone(), ty.clone()),
                None => {
                    // Member not found in scope.  Conformance checking will report the
                    // missing member as a separate error.  Emit an info diagnostic here
                    // so this path is visible if conformance checking is ever bypassed
                    // or reordered in the future.
                    diagnostics.push(
                        Diagnostic::info(format!(
                            "qualified access '{}::{}': member not found in scope; \
                             conformance checking should report the missing member separately",
                            trait_name, member,
                        ))
                        .with_label(DiagnosticLabel::new(expr.span, "member not found in scope")),
                    );
                    CompiledExpr::literal(Value::Undef, Type::Real)
                }
            }
        }
        reify_syntax::ExprKind::InstanceQualifiedAccess { object, qualified } => {
            // Resolve `sub.(TraitName::member)` to a ValueCellId for the sub's member.

            // Extract the sub-component name.
            let sub_name = match &object.kind {
                reify_syntax::ExprKind::Ident(name) => name.clone(),
                _ => {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error(
                            "unsupported instance qualified access: object must be an identifier",
                        )
                        .with_label(DiagnosticLabel::new(object.span, "unsupported")),
                    );
                }
            };

            // Extract trait_name and member from the qualified access part.
            let (trait_name, member) = match &qualified.kind {
                reify_syntax::ExprKind::QualifiedAccess { qualifier, member } => {
                    match &qualifier.kind {
                        reify_syntax::ExprKind::Ident(name) => (name.clone(), member.clone()),
                        _ => {
                            // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                            return make_poison_literal(
                                diagnostics,
                                Diagnostic::error(
                                    "unsupported qualified access in instance access",
                                )
                                .with_label(DiagnosticLabel::new(
                                    qualified.span,
                                    "unsupported form",
                                )),
                            );
                        }
                    }
                }
                _ => {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error(
                            "expected 'Trait::member' form in instance qualified access",
                        )
                        .with_label(DiagnosticLabel::new(
                            qualified.span,
                            "expected qualified access",
                        )),
                    );
                }
            };

            // Look up the sub-component's structure type.
            let structure_name = match scope.sub_component_types.get(&sub_name) {
                Some(s) => s.clone(),
                None => {
                    // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                    return make_poison_literal(
                        diagnostics,
                        Diagnostic::error(format!("unknown sub-component '{}'", sub_name))
                            .with_label(DiagnosticLabel::new(expr.span, "unknown sub-component")),
                    );
                }
            };

            // Check if the sub-component's structure implements the referenced trait.
            let trait_bounds = scope
                .sub_structure_traits
                .get(&structure_name)
                .cloned()
                .unwrap_or_default();
            if !trait_bounds.contains(&trait_name) {
                // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                return make_poison_literal(
                    diagnostics,
                    Diagnostic::error(format!(
                        "sub-component '{}' (type '{}') does not implement trait '{}'",
                        sub_name, structure_name, trait_name
                    ))
                    .with_code(DiagnosticCode::TraitNotImplemented)
                    .with_label(DiagnosticLabel::new(expr.span, "trait not implemented")),
                );
            }

            // Optionally validate the member exists in the trait.
            if let Some(members) = scope.trait_members.get(&trait_name)
                && !members.contains(member.as_str())
            {
                // Anti-cascade (task-448/task-1912/task-1921): poison to prevent follow-on cascade.
                return make_poison_literal(
                    diagnostics,
                    Diagnostic::error(format!(
                        "member '{}' not defined in trait '{}'",
                        member, trait_name
                    ))
                    .with_label(DiagnosticLabel::new(expr.span, "not in trait")),
                );
            }

            // Generate ValueCellId for the sub-component's member.
            // The eval engine scopes sub-components as "{parent}.{sub_name}".
            let scoped_entity = format!("{}.{}", scope.entity_name, sub_name);
            let id = ValueCellId::new(&scoped_entity, &member);
            // Infer member type from the sub's structure member types if available.
            // sub_member_types covers ALL subs (collection and non-collection), so it is
            // the authoritative source here.  If a sub exists but the member is missing,
            // the invariant is violated and the ICE branch below is the correct outcome.
            let ty = scope
                .sub_member_types
                .get(&sub_name)
                .and_then(|m| m.get(&member))
                .cloned()
                .unwrap_or_else(|| {
                    // Anti-cascade (task-1921): Type::Error fallback keeps the ICE diagnostic
                    // from cascading into downstream type-mismatch errors.
                    make_poison_type(
                        diagnostics,
                        Diagnostic::error(format!(
                            "internal compiler error: unresolved sub-member type for '{}.{}'",
                            sub_name, member
                        ))
                        .with_label(DiagnosticLabel::new(
                            expr.span,
                            "ICE: sub-member type not registered",
                        )),
                    )
                });
            CompiledExpr::value_ref(id, ty)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the `unwrap_or_else` safety fallback in `resolve_collection_sub_to_list`:
    /// when `sub_component_types` has no entry for the sub name (as in a manually-constructed
    /// CompilationScope used in unit tests), the field name is used as the StructureRef name.
    ///
    /// This path cannot be triggered by the full compilation pipeline (entity.rs always
    /// populates `sub_component_types` for every sub declaration), but it must not panic —
    /// and this test documents and guards that contract.
    #[test]
    fn collection_sub_fallback_missing_sub_component_types_uses_field_name() {
        let mut scope = CompilationScope::new("S");
        // Populate collection_sub_names so the name is recognised as a collection sub,
        // but leave sub_component_types and sub_member_types empty.
        scope.collection_sub_names.insert("parts".to_string());

        let result = resolve_collection_sub_to_list(&scope, "parts");

        // Cell ID should be S.__list_parts
        let expected_id = ValueCellId::new("S", "__list_parts");
        let refs = result.collect_value_refs();
        assert!(
            refs.contains(&expected_id),
            "safety-fallback cell ID should be S.__list_parts, got: {:?}",
            refs
        );

        // Type should be List(StructureRef("parts")) — the field name, not a structure type name
        match &result.result_type {
            Type::List(inner) => {
                assert_eq!(
                    inner.as_ref(),
                    &Type::StructureRef("parts".to_string()),
                    "safety-fallback inner type should be StructureRef(\"parts\") (field name), got: {:?}",
                    inner
                );
            }
            other => panic!("expected List type, got: {:?}", other),
        }
    }

    /// `make_poison_literal` pushes the supplied `Diagnostic` into the vec and
    /// returns `CompiledExpr::literal(Value::Undef, Type::Error)`.
    ///
    /// Verifies the new by-construction invariant: the helper is responsible for
    /// the push, so callers no longer need the `let n = diagnostics.len()` /
    /// `diagnostics.push(...)` / `make_poison_literal(diagnostics, n)` pattern.
    #[test]
    fn make_poison_literal_pushes_error_diagnostic_and_returns_poison_literal() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = make_poison_literal(
            &mut diagnostics,
            Diagnostic::error("root cause")
                .with_label(DiagnosticLabel::new(SourceSpan::prelude(), "here")),
        );
        // Diagnostic was pushed internally.
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].message, "root cause");
        // Returned expr is the poison literal.
        assert_eq!(result.result_type, Type::Error);
        assert!(
            matches!(result.kind, CompiledExprKind::Literal(_)),
            "expected Literal kind, got: {:?}",
            result.kind
        );
    }

    /// `make_poison_literal` fires the `debug_assert!` when given a diagnostic
    /// whose severity is not `Severity::Error`.
    ///
    /// The new API enforces the "push paired with poison" invariant by
    /// construction: the helper itself pushes, so the only check left is that
    /// callers don't accidentally pass a Warning or Info diagnostic.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "severity")]
    fn make_poison_literal_panics_with_non_error_severity_diagnostic() {
        make_poison_literal(&mut vec![], Diagnostic::warning("not an error"));
    }

    /// `make_poison_type` pushes the supplied `Diagnostic` into the vec and
    /// returns `Type::Error`.
    ///
    /// Mirrors `make_poison_literal_pushes_error_diagnostic_and_returns_poison_literal`
    /// for the parallel `make_poison_type` helper so both helpers have explicit
    /// positive-behavior coverage.
    #[test]
    fn make_poison_type_pushes_error_diagnostic_and_returns_type_error() {
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = make_poison_type(
            &mut diagnostics,
            Diagnostic::error("ICE: no bounds")
                .with_label(DiagnosticLabel::new(SourceSpan::prelude(), "here")),
        );
        // Diagnostic was pushed internally.
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert_eq!(diagnostics[0].message, "ICE: no bounds");
        // Returned type is the poison sentinel.
        assert_eq!(result, Type::Error);
    }

    /// `make_poison_type` fires the `debug_assert!` when given a diagnostic
    /// whose severity is not `Severity::Error`.
    ///
    /// Mirrors `make_poison_literal_panics_with_non_error_severity_diagnostic`
    /// for the Type helper so both helpers have explicit panic-contract coverage.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "severity")]
    fn make_poison_type_panics_with_non_error_severity_diagnostic() {
        let _ = make_poison_type(&mut vec![], Diagnostic::info("wrong severity"));
    }

    /// `try_emit_cross_sub_geometry` must name the child **structure type** in the
    /// diagnostic's "compose geometry inside '...'" phrase, not the sub instance name.
    ///
    /// Concretely: given `sub bolts : List<Bolt>`, the diagnostic for the still-
    /// unsupported collection-sub geometry access must say "compose geometry
    /// inside 'Bolt'" — not "inside 'bolts'".  The distinction is the
    /// `sub_realization_names ⊂ sub_component_types` invariant (task-3420):
    /// `sub_component_types` maps the instance name ("bolts") to the structure type
    /// name ("Bolt"), and the lookup in `try_emit_cross_sub_geometry` uses that
    /// mapping.  If the mapping were absent the fallback would silently produce
    /// "inside 'bolts'" (lower-case instance name), which is the bug this invariant
    /// exists to prevent.
    ///
    /// This is an end-to-end test using the full compile pipeline so it exercises
    /// the production code path through `entity.rs` (which populates both maps) and
    /// through `try_emit_cross_sub_geometry` (which consumes them).
    ///
    /// **Task 3441 note.**  The original test used `sub inner = Inner()` +
    /// `let copy = self.inner.body` to exercise the diagnostic path.  Task 3441
    /// flipped non-collection sub geometry access to a working-path lowering, so
    /// the diagnostic no longer fires for that shape.  The test now uses a
    /// **collection sub** (`bolts : List<Bolt>` with `bolts[0].body`) to keep
    /// exercising the same diagnostic-emitting branch — collection-sub cross-sub
    /// geometry remains deferred in v0.1 and continues to call
    /// `try_emit_cross_sub_geometry`.
    #[test]
    fn cross_sub_geometry_diagnostic_names_child_structure_type() {
        use reify_test_support::compile_source;
        use reify_types::Severity;
        // "Bolt" (capital-B structure type) vs "bolts" (lower-case instance name).
        // The diagnostic's "compose geometry inside '...'" phrase must use the former.
        let source = r#"pub structure Bolt {
    param body : Solid = cylinder(2mm, 10mm)
}
pub structure Rack {
    sub bolts : List<Bolt>
    let first = bolts[0].body
}"#;
        let compiled = compile_source(source);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();

        // (a) A geometry-specific diagnostic must fire.
        let geometry_diagnostic = errors
            .iter()
            .find(|d| d.message.contains("geometry") && d.message.contains("not yet"));
        assert!(
            geometry_diagnostic.is_some(),
            "expected a geometry-specific diagnostic for `bolts[0].body`; got: {:?}",
            errors.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        // (b) The "compose geometry inside '...'" phrase must name 'Bolt' (capital-B
        //     structure type name from sub_component_types), not 'bolts' (instance name).
        //     This directly pins the behavior the sub_realization_names ⊂ sub_component_types
        //     invariant exists to preserve.
        let msg = &geometry_diagnostic.unwrap().message;
        assert!(
            msg.contains("inside 'Bolt'"),
            "diagnostic must say \"inside 'Bolt'\" (the structure type name), \
             not \"inside 'bolts'\" (the instance name); got: {:?}",
            msg
        );
    }

    /// `try_emit_cross_sub_geometry` must panic (not silently fall back) when
    /// `sub_realization_names` contains an entry for a sub but `sub_component_types`
    /// does not — in **all** build modes (debug and release).
    ///
    /// ## What this test pins
    ///
    /// The `sub_component_types.get(sub_name)` lookup enforces the
    /// `sub_realization_names ⊂ sub_component_types` invariant in both debug and
    /// release builds (task-3431), unlike the previous `debug_assert!` +
    /// `.unwrap_or(sub_name)` combination which only caught violations in debug
    /// builds and silently mis-named the child structure in release.  The panic
    /// message dynamically names the offending sub instance (task-3439), so
    /// `expected = "'inner'"` proves both that the correct enforcement site fired
    /// **and** that the runtime sub name reached the panic message.
    ///
    /// ## Why this test is NOT `#[cfg(debug_assertions)]`-gated
    ///
    /// The explicit goal of task-3431 is to make the enforcement active in **release**
    /// builds too. Gating the test on `debug_assertions` would only verify debug-only
    /// behavior — exactly the gap this task closes. Omitting the gate ensures the
    /// `#[should_panic]` assertion is checked in both profiles.
    ///
    /// ## Why this test calls `try_emit_cross_sub_geometry` directly
    ///
    /// The invariant-violating state (a sub in `sub_realization_names` with no
    /// corresponding entry in `sub_component_types`) is unreachable via the public
    /// compile API — `entity.rs` always populates both maps together. The helper
    /// must be called directly to reach the panicking code path.
    ///
    /// ## Why `expected = "'inner'"` rather than a static tag
    ///
    /// `'inner'` can only appear in the panic message when the runtime `sub_name`
    /// value is interpolated — the format literal contains no instance name.  This
    /// pins both release-enforcement and dynamic naming (task-3439) in one assertion,
    /// without coupling to the surrounding prose of the message.
    #[test]
    #[should_panic(expected = "'inner'")]
    fn try_emit_cross_sub_geometry_panics_on_invariant_violation_in_all_builds() {
        use std::collections::BTreeSet;
        let mut scope = CompilationScope::new("Outer");
        // Populate sub_realization_names["inner"] = {"body"} but deliberately leave
        // sub_component_types empty — this violates the invariant and must panic.
        scope
            .sub_realization_names
            .insert("inner".to_string(), BTreeSet::from(["body".to_string()]));
        // sub_component_types intentionally not populated.
        try_emit_cross_sub_geometry(
            &scope,
            "inner",
            "body",
            reify_types::SourceSpan::prelude(),
            &mut Vec::new(),
        );
    }

    /// `resolve_cluster_inner_member` must NOT panic when called with an empty
    /// `per_arm` slice (review-cycle-1 robustness fix; task 2373 step-21/22).
    ///
    /// Before the fix, the helper computed `missing` = empty (vacuously true on
    /// empty input), entered the all-arms-have-the-field branch, then indexed
    /// `lookups[0]` → index-out-of-bounds panic.
    ///
    /// Contract: an empty `per_arm` is treated as "cluster has no resolvable arm
    /// structures" and emits the same diagnostic shape as the inner-call-site
    /// fallback at expr.rs:1049-1059, then returns a poison literal so downstream
    /// expressions don't cascade. Both `sub_qualifier = None` and
    /// `sub_qualifier = Some(...)` paths are covered: the latter pins the
    /// `"sub '<name>'"` qualifier substring expected by external-scope callers.
    #[test]
    fn resolve_cluster_inner_member_empty_per_arm_returns_poison_without_panic() {
        // Case 1: sub_qualifier = None (inner self.<cluster>.<inner> call site).
        let per_arm: Vec<(String, std::collections::BTreeMap<String, Type>)> = vec![];
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = resolve_cluster_inner_member(
            &per_arm,
            "anything",
            "Bolt",
            "head",
            None,
            SourceSpan::prelude(),
            &mut diagnostics,
        );
        // (b) returned CompiledExpr.result_type == Type::Error.
        assert_eq!(
            result.result_type,
            Type::Error,
            "empty per_arm must return a poison literal (Type::Error), got: {:?}",
            result.result_type
        );
        // (c) exactly one Severity::Error diagnostic mentioning the cluster shape.
        assert_eq!(
            diagnostics.len(),
            1,
            "empty per_arm must push exactly one diagnostic, got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert!(
            diagnostics[0].message.contains("match-arm cluster"),
            "diagnostic must mention 'match-arm cluster' for cluster shape; got: {:?}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("head"),
            "diagnostic must name the empty cluster ('head'); got: {:?}",
            diagnostics[0].message
        );

        // Case 2: sub_qualifier = Some("bolt") (external <sub>.<cluster>.<inner>
        // call site). Diagnostic must contain the qualifier fragment.
        let mut diagnostics2: Vec<Diagnostic> = vec![];
        let result2 = resolve_cluster_inner_member(
            &per_arm,
            "anything",
            "Driver.bolt",
            "head",
            Some("bolt"),
            SourceSpan::prelude(),
            &mut diagnostics2,
        );
        assert_eq!(
            result2.result_type,
            Type::Error,
            "empty per_arm with sub qualifier must also return Type::Error"
        );
        assert_eq!(diagnostics2.len(), 1);
        assert_eq!(diagnostics2[0].severity, Severity::Error);
        assert!(
            diagnostics2[0].message.contains("sub 'bolt'"),
            "external-call-site diagnostic must include qualifier `sub 'bolt'`; got: {:?}",
            diagnostics2[0].message
        );
    }

    /// Builds the shared per-arm fixture for the missing-arm unit tests:
    /// `HexHead` has `head_thickness : Real`; `SocketHead` is missing it.
    /// Reuses the canonical Hex/Socket arm-name pairing from the integration
    /// tests (`match_arm_decl_group_typing_tests.rs`) so both test layers share
    /// the same conceptual fixture.
    fn missing_arm_fixture() -> Vec<(String, std::collections::BTreeMap<String, Type>)> {
        vec![
            (
                "HexHead".to_string(),
                [("head_thickness".to_string(), Type::Real)]
                    .into_iter()
                    .collect(),
            ),
            // SocketHead is missing "head_thickness" → it is the missing arm.
            ("SocketHead".to_string(), std::collections::BTreeMap::new()),
        ]
    }

    /// Covers the missing-arm branch of `resolve_cluster_inner_member` with
    /// `sub_qualifier = None` (self.<cluster>.<inner> call site).  Pins the
    /// poison-literal return to lock in the anti-cascade contract at the helper
    /// boundary: when one or more match-arm types lack the requested field the
    /// helper must name those arms in the diagnostic and return `Type::Error` so
    /// downstream expressions do not attempt to dereference an absent cell.
    #[test]
    fn resolve_cluster_inner_member_missing_arm_unqualified_diagnostic() {
        let per_arm = missing_arm_fixture();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let result = resolve_cluster_inner_member(
            &per_arm,
            "head_thickness",
            "Bolt",
            "head",
            None,
            SourceSpan::prelude(),
            &mut diagnostics,
        );
        assert_eq!(
            result.result_type,
            Type::Error,
            "missing-arm path must return a poison literal (Type::Error); got: {:?}",
            result.result_type
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "missing-arm path must emit exactly one diagnostic; got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert!(
            diagnostics[0].message.contains("SocketHead"),
            "diagnostic must name the missing arm 'SocketHead'; got: {:?}",
            diagnostics[0].message
        );
        assert!(
            !diagnostics[0].message.contains("sub '"),
            "unqualified call must not include 'sub \\'' qualifier preamble; got: {:?}",
            diagnostics[0].message
        );
    }

    /// Covers the missing-arm branch of `resolve_cluster_inner_member` with
    /// `sub_qualifier = Some("bolt")` (external <sub>.<cluster>.<inner> call site).
    /// Companion to `resolve_cluster_inner_member_missing_arm_unqualified_diagnostic`
    /// (unqualified case); the two tests together lock in both qualifier branches of
    /// the format string at expr.rs:200-203 and 244-248 independently so a regression
    /// in the qualifier-preamble path shows up as a distinct failure.
    #[test]
    fn resolve_cluster_inner_member_missing_arm_qualified_diagnostic() {
        let per_arm = missing_arm_fixture();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        // scoped_entity uses the external-call shape "Driver.bolt" to mirror the
        // empty-per_arm sibling test's external-case fixture at expr.rs:2618.
        let result = resolve_cluster_inner_member(
            &per_arm,
            "head_thickness",
            "Driver.bolt",
            "head",
            Some("bolt"),
            SourceSpan::prelude(),
            &mut diagnostics,
        );
        assert_eq!(
            result.result_type,
            Type::Error,
            "missing-arm path with qualifier must return Type::Error; got: {:?}",
            result.result_type
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "missing-arm path must emit exactly one diagnostic; got {} diagnostics: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(diagnostics[0].severity, Severity::Error);
        assert!(
            diagnostics[0].message.contains("SocketHead"),
            "diagnostic must name the missing arm 'SocketHead'; got: {:?}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0].message.contains("sub 'bolt'"),
            "qualified diagnostic must contain \"sub 'bolt'\"; got: {:?}",
            diagnostics[0].message
        );
    }

    /// `compile_expr_guarded` on `self.<cluster>.<inner>` must use the helper's
    /// empty-per_arm diagnostic when the cluster is registered but
    /// `match_arm_group_arm_member_types` has no entry for it (producer-side bug).
    ///
    /// This path is unreachable through the full compilation pipeline (entity.rs's
    /// `if !group_arms.is_empty()` gate prevents `register_match_arm_group` from
    /// being called when no arms compile), so the test hand-constructs a
    /// `CompilationScope` to exercise the call-site branch directly.
    ///
    /// Paired coverage at both call-site and helper levels
    /// (`resolve_cluster_inner_member_empty_per_arm_returns_poison_without_panic`)
    /// prevents future drift from reintroducing the duplication removed in task 2869.
    #[test]
    fn compile_expr_inner_cluster_missing_per_arm_returns_helper_diagnostic() {
        use reify_types::Value;

        // Build the scope: entity "Bolt" with a registered "head" cluster but no
        // per-arm type map — this is the bug-condition the test pins.
        let mut scope = CompilationScope::new("Bolt");
        scope.is_entity_scope = true;
        let group = GuardedDeclGroup {
            name: "head".to_string(),
            arms: vec![GuardedDeclArm {
                guard_expr: CompiledExpr::literal(Value::Bool(true), Type::Bool),
                guard_value_cell: ValueCellId::new("Bolt", "__guard_0"),
                arm_type: Type::StructureRef("HexHead".to_string()),
            }],
        };
        scope.register_match_arm_group("head", group);
        // Deliberately leave `match_arm_group_arm_member_types` empty for "head".

        // Build AST: self.head.across_flats (two nested MemberAccess nodes).
        let self_expr = reify_syntax::Expr {
            kind: reify_syntax::ExprKind::Ident("self".to_string()),
            span: SourceSpan::prelude(),
        };
        let self_head = reify_syntax::Expr {
            kind: reify_syntax::ExprKind::MemberAccess {
                object: Box::new(self_expr),
                member: "head".to_string(),
            },
            span: SourceSpan::prelude(),
        };
        let expr = reify_syntax::Expr {
            kind: reify_syntax::ExprKind::MemberAccess {
                object: Box::new(self_head),
                member: "across_flats".to_string(),
            },
            span: SourceSpan::prelude(),
        };

        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut lambda_counter = 0u32;
        let result = compile_expr_guarded(
            &expr,
            &scope,
            &[],
            &[],
            &mut diagnostics,
            None,
            &mut lambda_counter,
        );

        // (a) poison literal returned.
        assert_eq!(
            result.result_type,
            Type::Error,
            "missing per_arm at inner call site must return Type::Error; got: {:?}",
            result.result_type
        );
        // (b) exactly one Severity::Error diagnostic.
        assert_eq!(
            diagnostics.len(),
            1,
            "missing per_arm must produce exactly one diagnostic; got {}: {:?}",
            diagnostics.len(),
            diagnostics
        );
        assert_eq!(diagnostics[0].severity, Severity::Error);
        // (c) message matches the helper's empty-per_arm shape.
        assert!(
            diagnostics[0]
                .message
                .contains("match-arm cluster 'head' has no resolvable arm structures"),
            "diagnostic must mention the cluster shape; got: {:?}",
            diagnostics[0].message
        );
        assert!(
            diagnostics[0]
                .message
                .contains("cannot resolve member 'across_flats'"),
            "diagnostic must name the unresolvable member; got: {:?}",
            diagnostics[0].message
        );
        // (d) label text mirrors the helper's empty-per_arm label.
        let label_msgs: Vec<&str> = diagnostics[0]
            .labels
            .iter()
            .map(|l| l.message.as_str())
            .collect();
        assert!(
            label_msgs.contains(&"cluster has no resolved arm structures"),
            "diagnostic must carry the cluster label; got labels: {:?}",
            label_msgs
        );
    }

    /// `try_resolve_cross_sub_geometry_value_ref` must emit
    /// `CompiledExprKind::CrossSubGeometryRef` (the typed discriminator added in
    /// task-3508), NOT `CompiledExprKind::ValueRef`.
    ///
    /// Before task-3508 the producer called `CompiledExpr::value_ref`, so the
    /// bare-let drop site in entity.rs had to use the fragile
    /// `entity.contains('.')` heuristic. After task-3508 the producer calls
    /// `CompiledExpr::cross_sub_geometry_ref`, making the consumer's
    /// pattern match structurally unambiguous.
    ///
    /// RED until step-4 flips the producer from `value_ref` to
    /// `cross_sub_geometry_ref`.
    #[test]
    fn try_resolve_cross_sub_geometry_value_ref_emits_typed_discriminator() {
        use std::collections::{BTreeMap, BTreeSet};
        use reify_types::{CompiledExprKind, Type};

        let mut scope = CompilationScope::new("Outer");
        scope
            .sub_component_types
            .insert("inner".to_string(), "Inner".to_string());
        // Empty inner member-type map so the realization-name branch governs.
        scope
            .sub_member_types
            .insert("inner".to_string(), BTreeMap::new());
        scope
            .sub_realization_names
            .insert("inner".to_string(), BTreeSet::from(["body".to_string()]));

        // (a) helper must return Some.
        let result = try_resolve_cross_sub_geometry_value_ref(&scope, "inner", "body");
        assert!(result.is_some(), "expected Some from the helper for a known realization");

        let result = result.unwrap();

        // (b) kind must be CrossSubGeometryRef (not ValueRef) — the typed discriminator.
        assert!(
            matches!(result.kind, CompiledExprKind::CrossSubGeometryRef(_)),
            "producer must emit CrossSubGeometryRef, not ValueRef, after task 3508 (got {:?})",
            result.kind
        );
        // (c) kind must NOT be ValueRef.
        assert!(
            !matches!(result.kind, CompiledExprKind::ValueRef(_)),
            "producer must not emit ValueRef after task 3508"
        );
        // (d) inner ValueCellId must carry the scoped entity stamp and member name.
        if let CompiledExprKind::CrossSubGeometryRef(vid) = &result.kind {
            assert_eq!(vid.entity, "Outer.inner");
            assert_eq!(vid.member, "body");
        }
        // (e) result_type must be Type::Geometry.
        assert_eq!(result.result_type, Type::Geometry);
    }

    // ───────────────────────────────────────────────────────────────────────
    // task 3540 step-15: structure-instance ctor lowering (RED)
    //
    // When a FunctionCall callee resolves to a `structure def` template in
    // `scope.template_registry`, the compiler must emit
    // `CompiledExprKind::StructureInstanceCtor` instead of a stdlib
    // `FunctionCall` (design-decision-2). type_id is a baked
    // `StructureTypeId(0)` placeholder; (type_name, version) are authoritative
    // (esc-3540-173 / RULING 2+3). version is read via `template.version()`
    // (the @version(N) accessor, esc-3540-176). Builtins (e.g. `cos`) are NOT
    // perturbed.
    //
    // NOTE (escalate_info, design_concern, non-blocking): plan step-15(b)
    // posits a `Beam { length: 2.0m }` "named-arg form". There is no
    // record/struct-literal `ExprKind` in the surface grammar
    // (reify-syntax/src/lib.rs ExprKind has FunctionCall only) — structure
    // construction is positional-call form exclusively. Scenario (b) is
    // therefore covered by the positional-binding test below rather than a
    // separate `{}` form.
    // ───────────────────────────────────────────────────────────────────────

    /// Build a minimal structure-def `TopologyTemplate` with the given
    /// `(param_name, default)` params (mirrors scc.rs::minimal_template).
    fn sct_template(
        name: &str,
        params: &[(&str, Option<CompiledExpr>)],
    ) -> crate::types::TopologyTemplate {
        let value_cells = params
            .iter()
            .map(|(pname, default)| crate::types::ValueCellDecl {
                id: ValueCellId::new(name, *pname),
                kind: crate::types::ValueCellKind::Param,
                visibility: crate::types::Visibility::Public,
                cell_type: Type::Real,
                default_expr: default.clone(),
                solver_hints: vec![],
                span: SourceSpan::prelude(),
            })
            .collect();
        crate::types::TopologyTemplate {
            name: name.to_string(),
            entity_kind: crate::types::EntityKind::Structure,
            visibility: crate::types::Visibility::Public,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells,
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: std::collections::HashSet::new(),
            objective: None,
            meta: std::collections::HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
        }
    }

    fn call_expr(name: &str, args: Vec<reify_syntax::Expr>) -> reify_syntax::Expr {
        reify_syntax::Expr {
            kind: reify_syntax::ExprKind::FunctionCall {
                name: name.to_string(),
                args,
            },
            span: SourceSpan::prelude(),
        }
    }

    fn num_expr(v: f64) -> reify_syntax::Expr {
        reify_syntax::Expr {
            kind: reify_syntax::ExprKind::NumberLiteral {
                value: v,
                is_real: true,
            },
            span: SourceSpan::prelude(),
        }
    }

    #[test]
    fn structure_def_zero_arg_call_lowers_to_ctor() {
        let tmpl = sct_template(
            "Steel_AISI_1045",
            &[(
                "youngs_modulus",
                Some(CompiledExpr::literal(Value::Int(200), Type::Int)),
            )],
        );
        let mut registry: std::collections::HashMap<String, &crate::types::TopologyTemplate> =
            std::collections::HashMap::new();
        registry.insert("Steel_AISI_1045".to_string(), &tmpl);

        let mut scope = CompilationScope::new("Host");
        scope.is_entity_scope = true;
        scope.set_template_registry(&registry);

        let mut diags: Vec<Diagnostic> = vec![];
        let result = compile_expr(
            &call_expr("Steel_AISI_1045", vec![]),
            &scope,
            &[],
            &[],
            &mut diags,
        );

        match &result.kind {
            CompiledExprKind::StructureInstanceCtor {
                type_name,
                version,
                ordered_args,
                defaults,
                ..
            } => {
                assert_eq!(type_name, "Steel_AISI_1045");
                assert_eq!(*version, 1, "absent @version defaults to 1 via version()");
                assert!(ordered_args.is_empty(), "zero-arg call → no ordered args");
                assert!(
                    defaults.iter().any(|(n, _)| n == "youngs_modulus"),
                    "omitted param's default must be captured, got {:?}",
                    defaults.iter().map(|(n, _)| n).collect::<Vec<_>>()
                );
            }
            other => panic!("expected StructureInstanceCtor, got {:?}", other),
        }
        assert_eq!(
            result.result_type,
            Type::StructureRef("Steel_AISI_1045".to_string())
        );
    }

    #[test]
    fn structure_def_positional_args_bind_in_declaration_order() {
        let tmpl = sct_template(
            "PointLoad",
            &[
                ("target", Some(CompiledExpr::literal(Value::Undef, Type::Real))),
                (
                    "magnitude",
                    Some(CompiledExpr::literal(Value::Int(0), Type::Int)),
                ),
            ],
        );
        let mut registry: std::collections::HashMap<String, &crate::types::TopologyTemplate> =
            std::collections::HashMap::new();
        registry.insert("PointLoad".to_string(), &tmpl);

        let mut scope = CompilationScope::new("Host");
        scope.is_entity_scope = true;
        scope.set_template_registry(&registry);

        let mut diags: Vec<Diagnostic> = vec![];
        let result = compile_expr(
            &call_expr("PointLoad", vec![num_expr(5.0)]),
            &scope,
            &[],
            &[],
            &mut diags,
        );

        match &result.kind {
            CompiledExprKind::StructureInstanceCtor {
                ordered_args,
                defaults,
                ..
            } => {
                assert_eq!(ordered_args.len(), 1, "one positional arg supplied");
                assert_eq!(
                    ordered_args[0].0, "target",
                    "first positional binds to first param in declaration order"
                );
                assert!(
                    defaults.iter().any(|(n, _)| n == "magnitude"),
                    "uncovered param keeps its default"
                );
                assert!(
                    !defaults.iter().any(|(n, _)| n == "target"),
                    "covered param must NOT appear in defaults"
                );
            }
            other => panic!("expected StructureInstanceCtor, got {:?}", other),
        }
    }

    #[test]
    fn builtin_call_not_perturbed_by_ctor_path() {
        // Empty template registry → `cos` is not a structure-def → must stay
        // a FunctionCall (the stdlib path), NOT a StructureInstanceCtor.
        let registry: std::collections::HashMap<String, &crate::types::TopologyTemplate> =
            std::collections::HashMap::new();
        let mut scope = CompilationScope::new("Host");
        scope.is_entity_scope = true;
        scope.set_template_registry(&registry);

        let mut diags: Vec<Diagnostic> = vec![];
        let result = compile_expr(
            &call_expr("cos", vec![num_expr(0.0)]),
            &scope,
            &[],
            &[],
            &mut diags,
        );

        assert!(
            !matches!(result.kind, CompiledExprKind::StructureInstanceCtor { .. }),
            "builtin `cos` must not lower to StructureInstanceCtor"
        );
        assert!(
            matches!(result.kind, CompiledExprKind::FunctionCall { .. }),
            "builtin `cos` must remain a FunctionCall, got {:?}",
            result.kind
        );
    }
}
