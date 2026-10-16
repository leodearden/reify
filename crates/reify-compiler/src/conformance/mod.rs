pub(super) mod checker;
use checker::*;

pub(super) mod sub_component_validation;
pub(crate) use sub_component_validation::check_sub_structure_existence;

use super::*;
use crate::ambient_defaults::AmbientDefaults;
use crate::geometry_traits_inference::{
    GeometryTrait, InferredTraits, LetBindingEnv, infer_traits_for_expr_in_env, infer_traits_for_op,
};
use reify_core::BASE_UNIT_SYMBOLS;
use std::cell::RefCell;

/// Severity of struct-constructor field-conformance diagnostics: **`Error`**.
///
/// Task 5302 (α) generalized the 4584 struct-ctor conformance chokepoint from
/// its original 4-family allowlist (`List<TraitObject>` / `StructureRef` /
/// `Vector` / `Selector`) to ALL concrete field types; task 5303 (ε) added the
/// two structural codes. Both landed at `Severity::Warning` behind this const —
/// a landing convenience under PRD decision D4, so a large diagnostic surface
/// could reach the corpus before it could reject. **Task 5306 (δ) closed that
/// warn window.** A non-conforming ctor argument is now an Error, so
/// `reify check` exits 1 on it (`crates/reify-cli/src/main.rs`,
/// `.any(|d| d.severity == Severity::Error)`).
///
/// The surface still reads its severity from HERE (threaded through
/// `WalkCtx.severity`) rather than hard-coding `Severity::Error` at each emit
/// site, because that is what made δ a one-const flip and what keeps every
/// knob-governed site provably uniform. **There is no config flag and no
/// environment variable behind this const, and none may be added**: a
/// per-invocation severity would make `reify check`'s exit code depend on
/// something other than the source, which is the failure mode the whole staged
/// promotion existed to avoid.
///
/// **Two emit sites are deliberately outside this knob** and did NOT move with
/// δ, because they were already Error: (1) the fn-call conformance entry
/// (`check_fn_arg_conformance`) hard-codes `Severity::Error` (out of scope; see
/// the `WalkCtx.severity` field doc); (2) the geometry-trait leaf (`Bounded` /
/// `Connected` / `Convex`, reached via [`emit_geometry_unbounded`] /
/// [`emit_geometry_trait_violation`]) hard-codes `Severity::Error` because those
/// codes (`GeometryUnbounded` and the geometry `TypeNotConformingToTrait`) belong
/// to the geometry-primitive-constructors PRD, not to this ctor-field knob (see
/// the `WalkCtx.severity` doc's carve-out note). Both carve-outs remain live: a
/// future re-scoping of this const must not silently absorb them.
///
/// **ε (task 5303) additionally reads this knob from outside this module.** The
/// two structural emit sites in the `StructureInstanceCtor` by-name binder
/// (`crates/reify-compiler/src/expr.rs`) — unknown named argument
/// ([`DiagnosticCode::CtorUnknownField`]) and over-arity positional argument
/// ([`DiagnosticCode::CtorArity`]) — build their diagnostics with
/// [`diag_at`]`(CTOR_FIELD_CONFORMANCE_SEVERITY, …)` rather than a literal
/// severity, which is why both this const and [`diag_at`] are `pub(crate)`. That
/// is what made δ a literal one-const flip; a duplicated `Severity::Warning`
/// literal in expr.rs would have silently survived it (the C2(iv)
/// severity-invariance failure mode). The same reasoning applies to any emit site
/// added here later: read the const, never a literal.
///
/// # Language contract and deferred variants
///
/// The user-facing statement of what this knob governs is
/// `docs/reify-language-spec.md` §4.9. Around it:
///
/// * **No eval-time check, by ruling.** The eval-time ctor inserts args
///   verbatim; this compile-time surface is the single enforcement point
///   (`docs/prds/struct-ctor-field-type-conformance.md` D7).
/// * **Empty-collection args are skipped, not rejected.** They compile with no
///   expected type; typed enforcement belongs to
///   `docs/prds/expected-type-pushdown-return-field.md` (stub).
/// * **Bare `TraitObject` fields are exempt** on the expression-position path.
///   The exemption, its scope and its revisit condition live at
///   `check_expr_struct_ctor_args` in `compile_builder/entities_phase.rs` (PRD D6).
/// * **Construction-site gaps this knob does not yet reach** are listed under
///   spec §4.9 "Known limitations", each with its owner.
pub(crate) const CTOR_FIELD_CONFORMANCE_SEVERITY: Severity = Severity::Error;

/// Build a `Diagnostic` at an explicit `severity`.
///
/// The `Diagnostic` builder exposes `error`/`warning`/`info` constructors but no
/// severity-parameterized one; the ctor-conformance walker needs the severity to
/// be read from `WalkCtx.severity` at every emit site so the single knob above
/// governs the whole surface. `Diagnostic.severity` is a public field, so this
/// sets it directly after building — keeping the change confined to this crate
/// (no reify-core change, per task 5302).
///
/// `pub(crate)` since task 5303 (ε): the two structural ctor emit sites in
/// `crates/reify-compiler/src/expr.rs` need the same severity-parameterized
/// builder so they read [`CTOR_FIELD_CONFORMANCE_SEVERITY`] instead of
/// hard-coding a severity.
pub(crate) fn diag_at(severity: Severity, message: impl Into<String>) -> Diagnostic {
    let mut d = Diagnostic::error(message);
    d.severity = severity;
    d
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn check_trait_conformance(
    structure: &EntityDefRef<'_>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    structure_names: &HashSet<String>,
    trait_names: &HashSet<String>,
    scope: &mut CompilationScope,
    value_cells: &mut Vec<ValueCellDecl>,
    constraints: &mut Vec<CompiledConstraint>,
    constraint_index: &mut u32,
    enum_defs: &[reify_ir::EnumDef],
    functions: &[CompiledFunction],
    alias_registry: &TypeAliasRegistry,
    // task 4497 (ambient-default-material B): the ambient-default table resolved
    // for this structure's scope. Top-level structures are fed the file-level
    // table and resolve at file scope (DD6 → `purpose = None`).
    ambient: &AmbientDefaults,
    // task 4639: enclosing purpose name for innermost-wins resolution. `None`
    // for top-level structures (file scope); `Some(name)` for structures
    // nested inside a purpose body.
    purpose: Option<&str>,
    diagnostics: &mut Vec<Diagnostic>,
    // task 3939 δ: out-param receiving the resolved assoc-fn table, populated by
    // `check_phase_resolve_assoc_fns` (step-8). entity.rs stores it on the
    // conformer's TopologyTemplate (step-12).
    assoc_fns_out: &mut Vec<CompiledAssocFn>,
    // task 3972: out-param receiving the resolved assoc-type table, populated by
    // `check_phase_resolve_assoc_types`. entity.rs stores it on the conformer's
    // TopologyTemplate.
    assoc_types_out: &mut Vec<CompiledAssocType>,
) {
    // task 3973 ιγ: build the assoc-type scope BEFORE resolving structure
    // members so that `param x : AssocTypeName` annotations resolve correctly
    // in check_phase_resolve_structure_members.  We need:
    //   (a) the structure's own `type X = T` bindings (authoritative diagnostics
    //       — UnresolvedType for a bad RHS, TraitAssocTypeNotBound for unbound
    //       required types — are emitted here once, not duplicated downstream);
    //   (b) trait defaults for names the structure didn't bind;
    //   (c) declared_assoc_names for the anti-cascade poison sentinel.
    // This also moves the collect_structure_assoc_type_bindings call early so it
    // is not called twice (the result is reused below for phase 5).
    let structure_assoc_type_bindings = collect_structure_assoc_type_bindings(
        structure,
        alias_registry,
        structure_names,
        trait_names,
        diagnostics,
    );
    // Build assoc_type_scope = own bindings + trait defaults (own wins).
    let mut assoc_type_scope: HashMap<String, Type> = structure_assoc_type_bindings.clone();
    let mut declared_assoc_names: HashSet<String> = HashSet::new();
    for bound in structure.trait_bounds {
        if let Some(ct) = trait_registry.get(&bound.name) {
            for req in &ct.required_members {
                if matches!(req.kind, RequirementKind::AssocType(_)) {
                    declared_assoc_names.insert(req.name.clone());
                }
            }
            for def in &ct.defaults {
                if let DefaultKind::AssocType(ty) = &def.kind
                    && let Some(n) = &def.name
                {
                    declared_assoc_names.insert(n.clone());
                    assoc_type_scope
                        .entry(n.clone())
                        .or_insert_with(|| ty.clone());
                }
            }
        }
    }

    let (structure_param_members, structure_let_members, structure_constraint_labels) =
        check_phase_resolve_structure_members(
            structure,
            structure_names,
            trait_names,
            enum_defs,
            alias_registry,
            &assoc_type_scope,
            &declared_assoc_names,
            diagnostics,
        );

    // Build a combined name→type view for phases that only need to know whether the
    // structure declares a member by name (conflict-suppression, injection-skip, scope
    // pre-registration). Phases that must distinguish param from let members use
    // `structure_param_members` / `structure_let_members` directly (phase 5).
    //
    // If a name appears in both maps (no upstream diagnostic prevents it today),
    // the param entry wins: let-first chain means param entries overwrite let entries.
    let structure_all_members: HashMap<String, Type> = structure_let_members
        .iter()
        .chain(structure_param_members.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Derive the structure's own assoc-fn signatures (task 3939 δ) so phase 5
    // can exact-match a provided `fn` override against its trait requirement.
    let structure_fn_sigs =
        collect_structure_assoc_fn_sigs(structure, alias_registry, structure_names, trait_names);

    // (structure_assoc_type_bindings was already collected above — before the
    // check_phase_resolve_structure_members call — so it is in scope for phase 5.)

    let mut ctx = check_phase_collect_trait_bounds(
        structure,
        trait_registry,
        &structure_all_members,
        diagnostics,
    );

    // task 4497: synthesize ambient-default Param entries into `ctx.defaults`
    // for unfilled `Param(StructureRef(T))` requirements, BEFORE the
    // pre-register / available-defaults / inject phases read `ctx.defaults` —
    // so an ambient default rides the trait-default rails exactly like a
    // trait-declared param default (DD2). Top-level structures resolve at file
    // scope (DD6 → `purpose = None`); purpose-nested structures pass
    // `Some(purpose_name)` for innermost-wins resolution (task 4639).
    check_phase_inject_ambient_defaults(&mut ctx, ambient, &structure_all_members, purpose);

    let pre = check_phase_pre_register_default_types(
        &ctx,
        &structure_all_members,
        structure.name,
        scope,
        enum_defs,
        functions,
        diagnostics,
    );

    let available_defaults = check_phase_build_available_defaults_map(
        &ctx,
        &pre.inferred_let_exprs,
        &pre.pass1_skipped,
        &pre.pass1_param_skipped,
        &pre.pass2_skipped,
        &pre.pass2_compile_errors,
    );

    check_phase_check_members_against_requirements(
        &ctx,
        structure,
        &structure_param_members,
        &structure_let_members,
        &available_defaults,
        &structure_fn_sigs,
        &structure_assoc_type_bindings,
        diagnostics,
    );

    // task 3941 ζ: conformer member-name set for the assoc-fn body
    // bare-member → `self.member` desugar (PRD §4.4). The structure's own
    // params/lets (`structure_all_members`) plus any param/let the conformer
    // gains purely via trait conformance — a trait requirement or trait default
    // it does not redeclare in its own AST — so a default-only trait param
    // referenced bare in an assoc-fn body still desugars to a `self` field.
    let mut conformer_member_names: HashSet<String> =
        structure_all_members.keys().cloned().collect();
    for req in &ctx.requirements {
        if matches!(
            req.kind,
            RequirementKind::Param(_) | RequirementKind::Let(_)
        ) {
            conformer_member_names.insert(req.name.clone());
        }
    }
    for def in &ctx.defaults {
        if matches!(
            def.kind,
            DefaultKind::Param { .. } | DefaultKind::Let { .. }
        ) && let Some(name) = &def.name
        {
            conformer_member_names.insert(name.clone());
        }
    }

    // task 3939 δ: resolve the override-or-injected-default assoc-fn table.
    // Runs after phase 5 (satisfaction checks) but is independent of default
    // injection — it compiles fn bodies into `assoc_fns_out`, it does not touch
    // value cells / constraints.
    check_phase_resolve_assoc_fns(
        &ctx,
        structure,
        &conformer_member_names,
        enum_defs,
        functions,
        alias_registry,
        structure_names,
        trait_names,
        &structure_fn_sigs,
        diagnostics,
        assoc_fns_out,
    );

    // task 3972: resolve the override-or-injected-default assoc-type table.
    // Runs after phase 5 (same ordering as assoc-fn resolve). Reuses the
    // structure_assoc_type_bindings map built by collect_structure_assoc_type_bindings
    // (which phase 5 already needed for its satisfaction check).
    check_phase_resolve_assoc_types(&ctx, &structure_assoc_type_bindings, assoc_types_out);

    check_phase_inject_defaults(
        &ctx,
        structure,
        &structure_all_members,
        &structure_constraint_labels,
        pre.inferred_let_exprs,
        &pre.pass1_skipped,
        &pre.pass1_param_skipped,
        &pre.pass2_skipped,
        &pre.pass2_compile_errors,
        scope,
        value_cells,
        constraints,
        constraint_index,
        enum_defs,
        functions,
        diagnostics,
    );
}

/// Verify that a compiled arg value's type conforms to the declared param type
/// in the target structure when the declared type is `Type::TraitObject(trait_name)`
/// or a wrapper thereof (`Type::Option(...)`, `Type::List(...)`, `Type::Set(...)`,
/// `Type::Map(...)`).
///
/// The compiled arg carries the full `CompiledExpr` so the recursive walker can
/// descend into `OptionSome` / `ListLiteral` / `ReflectiveCellList` / `SetLiteral` / `MapLiteral` nodes
/// and derive `arg_call_name` from any nested `FunctionCall` for the existing
/// `Real|Int → StructureRef` promotion.
///
/// Conformance strategy:
/// - `(Type::Option(p), OptionSome(a))` → recurse on inner
/// - `(Type::Option(_), OptionNone)` → OK (none is always valid for Option<T>)
/// - `(Type::List(p), ListLiteral(es) | ReflectiveCellList(es))` → recurse on each element
/// - `(Type::Set(p), SetLiteral(es))` → recurse on each element
/// - `(Type::Map(kp, vp), MapLiteral(entries))` → recurse on each (k, v)
/// - `(Type::TraitObject(req), _)` → leaf check (existing logic)
/// - Wrapped param + unmatched arg kind → fall through (no check)
///
/// Skips silently when:
/// - The target template is not found (external/unknown structure).
/// - The arg name is not found in the target's value cells (positional arg or error).
/// - The declared param type is not a trait object or wrapper thereof.
/// - The compiled_arg has `result_type == Type::Error` (anti-cascade).
///
/// Emits at most one diagnostic per leaf conformance failure.
pub(crate) fn check_trait_arg_conformance(
    target_name: &str,
    arg_name: &str,
    compiled_arg: &CompiledExpr,
    span: SourceSpan,
    registries: ConformanceRegistries<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Anti-cascade: if the arg itself had a compilation error, skip.
    if matches!(compiled_arg.result_type, Type::Error) {
        return;
    }

    // Look up the target template — skip if not found (external/forward-ref miss).
    let Some(target) = registries.templates.get(target_name) else {
        return;
    };

    // Find the declared param cell for this arg name.
    let Some(cell) = target
        .value_cells
        .iter()
        .find(|vc| vc.id.member == arg_name)
    else {
        return; // Arg name not found — skip (positional arg or existing error).
    };

    // Recursively walk the param type against the compiled arg.
    // The walker dispatches on (param_type, compiled_arg.kind) pairs for wrappers,
    // and falls through to the leaf helper for Type::TraitObject.
    let mut ctx = WalkCtx {
        arg_name,
        span,
        registries,
        diagnostics,
        // Ctor-conformance entry (value-cell + sub `=` paths): knob-governed
        // (Warning at α). task 5302.
        severity: CTOR_FIELD_CONFORMANCE_SEVERITY,
    };
    walk_param_against_arg(&cell.cell_type, compiled_arg, &mut ctx);
}

/// Verify that a compiled function-call argument conforms to a declared function
/// parameter type when that type is `Type::TraitObject(trait_name)` or a wrapper
/// thereof (`Type::Option(...)`, `Type::List(...)`, `Type::Set(...)`,
/// `Type::Map(...)`).
///
/// This is the function-call analogue of [`check_trait_arg_conformance`]:
/// whereas that entry point looks the callee up in the structure
/// `template_registry` and reads the param type from a `ValueCellDecl`, this
/// entry point receives the param type directly (from `CompiledFunction.params`)
/// and delegates to the same shared recursive walker `walk_param_against_arg` —
/// inheriting wrapper recursion, StructureRef bound-walking, TraitObject
/// refinement, geometry-trait handling, and Type::Error anti-cascade unchanged.
///
/// The `arg_name` is used as the diagnostic's param label, matching reify's
/// keyword-arg == param-name convention used by the structure path.
///
/// # Anti-cascade
///
/// If `compiled_arg.result_type == Type::Error` the call returns immediately
/// without emitting any diagnostic — the root-cause diagnostic was already
/// emitted by the expression that produced the error.
///
/// See task-4081 design decision §3 for rationale.
pub(crate) fn check_fn_arg_conformance(
    param_type: &Type,
    arg_name: &str,
    compiled_arg: &CompiledExpr,
    span: SourceSpan,
    registries: ConformanceRegistries<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Anti-cascade: if the arg itself had a compilation error, skip.
    if matches!(compiled_arg.result_type, Type::Error) {
        return;
    }

    // Recursively walk the param type against the compiled arg.
    let mut ctx = WalkCtx {
        arg_name,
        span,
        registries,
        diagnostics,
        // Fn-call trait-conformance is OUT OF SCOPE for the 5302 ctor knob and
        // must stay a hard error (preserves task-4081 fn-call semantics).
        severity: Severity::Error,
    };
    walk_param_against_arg(param_type, compiled_arg, &mut ctx);
}

/// The value cells subject to param-default conformance: `value_cells` ∪ the
/// members of every port body.
///
/// `TopologyTemplate.value_cells` is NOT the whole surface: port-body params are
/// compiled separately (`entity.rs` port arm) under the composite member name
/// `ValueCellId(entity, "<port>.<param>")` and stored on `CompiledPort.members`,
/// a DISJOINT list that is deliberately never merged into `value_cells`
/// (`reify_ast::decl`'s `collect_param_default_candidates` doc-comment records
/// why: `set_parameter`, the GUI property panel, and
/// `find_param_default_expr`/`find_param_default_span` cell_id resolution all
/// key off `value_cells` and must not see port-internal names).
///
/// Walking only `value_cells` therefore left every port-member param default
/// unchecked at EVERY arm below — a `Geometry`, `String` or `StructureRef`
/// default inside a `port { }` block compiled with zero diagnostics (task 7174).
/// Chaining here rather than adding a second call site keeps ONE loop body, so
/// the two lists cannot drift apart again.
///
/// `CompiledGuardedGroup.members` / `.else_members` is deliberately OUT of this
/// chain. A guarded param already carries its own separately-owned decision
/// about default checking — `guards.rs` omits the sibling `check_param_default_type`
/// at that site on purpose, pinned by
/// `guarded_param_dimension_mismatched_default_does_not_check_param_default_type`
/// — so extending conformance there is a change with its own acceptance
/// criteria, not a ride-along on this one. Mechanically it would be one more
/// `.chain(…)` here; structurally it still needs no second call site.
///
/// UNANNOTATED cells ARE judged here, against the `Type::dimensionless_scalar()`
/// INFERENCE FALLBACK the compiler assigns when a `param` names no type — so
/// `param c = Color.Red` reports `Enum(Color)` vs `Real`, naming a type the
/// source never wrote. That is deliberate, and it is not something the port
/// cells introduce: the walk is site-blind by construction (ONE loop body), and
/// the top-level half has reported the fallback this way since α. The asymmetry
/// worth knowing is against the sibling `check_param_default_type` (`entity.rs`),
/// which IS gated on `param.type_expr.is_some()` at both its call sites and so
/// stays silent on the same source (`untyped_port_member_param_with_enum_default_does_not_error`).
/// The two checks are complementary, not alike-gated; what holds both SITES to
/// one answer is `port_unannotated_param_default_takes_real_fallback_like_top_level`.
///
/// Whether an inference fallback should be judged AT ALL is a live question, and
/// δ (task #5306) made it a loud one: the flip turned this diagnostic into a
/// hard error on source that named no type. It is recorded here rather than
/// pre-empted because
/// gating it is a behaviour change at BOTH sites — this chain cannot skip the
/// fallback for port cells without also skipping it for top-level ones, which is
/// exactly the parity this task established. The bit such a gate would need
/// (`param.type_expr.is_some()`, already computed at both `check_param_default_type`
/// call sites) is not carried on `ValueCellDecl` today.
fn param_default_cells(template: &TopologyTemplate) -> impl Iterator<Item = &ValueCellDecl> {
    template
        .value_cells
        .iter()
        .chain(template.ports.iter().flat_map(|p| p.members.iter()))
}

/// Check that each `Param`-kind cell enumerated by [`param_default_cells`]
/// (template value cells ∪ port-body members) with a default expression has a
/// default whose type is compatible with the declared `cell_type`, for nominal
/// leaf types (task-4584):
///
/// - **`Type::StructureRef`** params: applies an inline skip-list (see the arm
///   comment below for rationale — concretely, a `StructureRef` default for a
///   `StructureRef` param is intentionally not rejected here because nominal-name
///   mismatch does not imply incompatibility in Reify at eval time) and calls
///   [`emit_structure_ref_mismatch`] directly for clearly incompatible primitive
///   types (String, Int, …).  Does **not** delegate to `walk_param_against_arg`.
///   Skips if `default_expr.result_type` is `Type::Error` (anti-cascade).
///
/// - **`Type::Geometry`** params: uses a geometry-function-aware predicate rather
///   than `type_compatible` — geometry constructors compile to a scalar placeholder
///   (GHR-γ), so plain `type_compatible(Geometry, result_type)` would falsely reject
///   them.  Handled inline by the `Type::Geometry` arm below (no separate helper).
///
/// All other cell types (Real, Int, List, …) are out of scope for this pass.
pub(crate) fn check_param_default_conformance(
    template: &TopologyTemplate,
    registries: ConformanceRegistries<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for vc in param_default_cells(template) {
        if vc.kind != ValueCellKind::Param {
            continue;
        }
        let Some(default) = &vc.default_expr else {
            continue;
        };
        match &vc.cell_type {
            Type::StructureRef(_) => {
                // Anti-cascade: skip when the default expression itself had a compile error.
                if matches!(default.result_type, Type::Error) {
                    continue;
                }
                // Get the effective type, accounting for FunctionCall→StructureRef promotion
                // (e.g. `Steel_AISI_1045()` may carry a numeric fallback result_type but
                // its callee IS a known structure template → promote to StructureRef).
                let promoted =
                    promote_function_call_to_structure_ref(default, registries.templates);
                let effective_ty = promoted.as_ref().unwrap_or(&default.result_type);
                // Conservatively skip when the effective type is plausibly structure-
                // compatible at the eval level:
                //   • StructureRef(_) — a concrete type used as default for an abstract-like
                //     param (e.g. `Steel_AISI_1045` default for `Material`) is valid in Reify;
                //     nominal-name mismatch here does NOT imply incompatibility.
                //   • TraitObject(_) — may resolve to a conforming struct at evaluation.
                //   • TypeParam(_) — unresolved generic; conformance decided at instantiation.
                //   • Geometry — carries no nominal identity to verify here.
                // Only emit for clearly incompatible primitive types (String, Int, Scalar, …).
                if matches!(
                    effective_ty,
                    Type::StructureRef(_)
                        | Type::TraitObject(_)
                        | Type::TypeParam(_)
                        | Type::Geometry
                ) {
                    continue;
                }
                let mut ctx = WalkCtx {
                    arg_name: vc.id.member.as_str(),
                    span: vc.span,
                    registries,
                    diagnostics,
                    // Param-default conformance is a ctor-conformance entry:
                    // knob-governed (Warning at α). task 5302.
                    severity: CTOR_FIELD_CONFORMANCE_SEVERITY,
                };
                emit_structure_ref_mismatch(&vc.cell_type, effective_ty, &mut ctx);
            }
            Type::Geometry => {
                // Anti-cascade: skip when the default expression itself had an error or
                // is an unresolved type param — both are unverifiable.
                if matches!(default.result_type, Type::Error | Type::TypeParam(_)) {
                    continue;
                }
                // Geometry constructors (box/cylinder/...) compile to a
                // `Type::dimensionless_scalar()` PLACEHOLDER (GHR-γ), so
                // `type_compatible(Geometry, default.result_type)` would FALSELY reject
                // every legitimate geometry default.  Instead we detect geometry by:
                //   1. result_type is already Type::Geometry (a let-bound geometry ref), or
                //   2. the callee is a known geometry function (box/cylinder/sphere/...).
                // This is the same signal `check_leaf_trait_conformance` uses for args.
                let is_geometry_default = matches!(default.result_type, Type::Geometry)
                    || extract_function_call_name(default)
                        .map(is_geometry_function)
                        .unwrap_or(false);
                if !is_geometry_default {
                    // task 5302 α (D8): emit at the ctor-conformance knob severity
                    // (Warning at α), not a hard error — so ALL param-default
                    // diagnostics are governed by the single knob and the δ flip
                    // promotes them uniformly. Previously `Diagnostic::error`.
                    diagnostics.push(
                        diag_at(
                            CTOR_FIELD_CONFORMANCE_SEVERITY,
                            format!(
                                "param '{}' has type 'Geometry' but its default expression has non-geometry type '{}'",
                                vc.id.member, default.result_type
                            ),
                        )
                        .with_code(DiagnosticCode::TypeNotConformingToStructureRef)
                        .with_label(DiagnosticLabel::new(
                            vc.span,
                            format!(
                                "expected a geometry expression, got '{}'",
                                default.result_type
                            ),
                        )),
                    );
                }
            }
            // General concrete-leaf param defaults (task 5302 α, D8): every param
            // type beyond `StructureRef`/`Geometry` — `String`, `Int`, `Real`,
            // `Bool`, `Selector`, `Vector`, `Option<…>`, `List<…>`, `TraitObject`,
            // … — routes its compiled default through the SAME shared walker used
            // by the call-site ctor entries, at the knob severity. This closes the
            // param-default half of the "all concrete field types" generalization:
            // a `param label : String = 42` default now diagnoses identically to a
            // call-site `Widget(label: 42)` (same `ArgTypeMismatch`, same message).
            //
            // The old `_ => continue` silently accepted every non-StructureRef/
            // non-Geometry default (the "future work" hole this task fills). The
            // walker single-sources both the anti-cascade skip list and the
            // param-family gating: its `reject_if_incompatible` skips
            // `Error`/`TypeParam`/`Geometry`/`TraitObject` ARG types, and its
            // general concrete-leaf arm judges a PARAM type only when
            // [`general_leaf_param_family_is_validated`] vets it. So routing here
            // is unconditional but diagnosing is not — a default at an
            // un-vetted family stays silent, and no gating logic is duplicated
            // at this call site. Only the top-level `Error`-default guard is
            // repeated, matching the `StructureRef`/`Geometry` arms above.
            _ => {
                // Anti-cascade: skip when the default expression itself had a
                // compile error (its root-cause diagnostic was already emitted).
                if matches!(default.result_type, Type::Error) {
                    continue;
                }
                let mut ctx = WalkCtx {
                    arg_name: vc.id.member.as_str(),
                    span: vc.span,
                    registries,
                    diagnostics,
                    // Param-default conformance is a ctor-conformance entry:
                    // knob-governed (Warning at α). task 5302.
                    severity: CTOR_FIELD_CONFORMANCE_SEVERITY,
                };
                walk_param_against_arg(&vc.cell_type, default, &mut ctx);
            }
        }
    }
}

/// Context bundle threaded through the four recursive walker helpers.
///
/// Collects the five fields that were previously repeated as trailing arguments
/// in every recursive call to `walk_param_against_arg`, `walk_param_against_arg_type`,
/// `emit_leaf_conformance_for_arg_type`, and `check_leaf_trait_conformance`.  Bundling
/// them reduces each helper from 7 arguments to 2-3, and means adding a new wrapper
/// kind (e.g. `Tuple<T,U>`) only requires a one-line match arm rather than a full
/// 7-arg call duplication.
///
/// Recursive calls simply pass `ctx` — Rust auto-reborrows `&mut` correctly.
/// The three immutable lookup tables every ctor/fn conformance entry point needs,
/// bundled into one borrow-struct.
///
/// `templates`, `traits` and `enum_defs` were threaded as three separate
/// trailing arguments through six functions in this module plus two phase
/// functions in `entities_phase.rs`, which is what forced the
/// `#[allow(clippy::too_many_arguments)]` suppressions on
/// [`check_trait_arg_conformance`], [`check_fn_arg_conformance`] and
/// `check_expr_fn_calls`. [`WalkCtx`] already aggregated exactly this trio, so
/// the tuple was being unbundled at the entry points only to be re-bundled one
/// frame later.
///
/// Adding a fourth table is now a one-line change here rather than a ninth
/// parameter and a fourth suppression — which matters because
/// [`general_leaf_param_family_is_validated`]'s "still unexamined" list names
/// sixteen more `Type` variants, several of which will need their own lookup.
///
/// `Copy` because it is three shared references; callers pass it by value and
/// [`WalkCtx`] stores it by value, so no reborrow ceremony is needed at the
/// recursive call sites.
#[derive(Clone, Copy)]
pub(crate) struct ConformanceRegistries<'a> {
    /// Structure templates visible at the call site, keyed by name.
    pub templates: &'a HashMap<String, &'a TopologyTemplate>,
    /// Compiled traits visible at the call site, keyed by name.
    pub traits: &'a HashMap<String, &'a CompiledTrait>,
    /// Declared enums visible at the call site — `prelude ++ module-local`, i.e.
    /// exactly `CompilationCtx.resolution_enums` (task 5465, family 4).
    ///
    /// Exists SOLELY so the general concrete-leaf arm can tolerate D1/F-Mono
    /// enum erasure: a constructed variant's `result_type` is always the bare
    /// `Type::Enum(name)`, never the applied form, so a declared
    /// `Type::Applied { name: "Result", args: [...] }` param would spuriously
    /// fail raw [`type_compatible`], which has no Applied-vs-Enum arm in EITHER
    /// direction. The general concrete-leaf arm maps BOTH sides through
    /// [`base_enum_name`], which needs this table to recognise an applied ENUM
    /// (vs. an applied generic STRUCTURE like `Coupling<T>`, which is spelled
    /// the same way) while keeping a genuine cross-enum mismatch rejected.
    ///
    /// [`base_enum_name`] is the same oracle [`enum_payload_compatible`] uses at
    /// the payload-field site in `variant_construct.rs`, so the two decisions
    /// cannot drift.
    pub enum_defs: &'a [reify_ir::EnumDef],
}

struct WalkCtx<'a> {
    arg_name: &'a str,
    span: SourceSpan,
    /// The immutable lookup tables (templates / traits / enums) this walk reads.
    registries: ConformanceRegistries<'a>,
    diagnostics: &'a mut Vec<Diagnostic>,
    /// Severity at which conformance diagnostics emitted through this walk are
    /// built (task 5302). The two ctor-conformance entries
    /// (`check_trait_arg_conformance`, `check_param_default_conformance`) set
    /// this to [`CTOR_FIELD_CONFORMANCE_SEVERITY`] (Warning at α, `Error` since
    /// δ / task #5306); the fn-call
    /// entry (`check_fn_arg_conformance`) sets it to `Severity::Error` so the
    /// out-of-scope fn-call trait-conformance semantics stay hard errors. Every
    /// *field-conformance* emit site (leaf-trait, StructureRef, Vector, selector,
    /// wrapper-shape, and the general concrete-leaf `ArgTypeMismatch`) builds its
    /// `Diagnostic` via [`diag_at`]`(ctx.severity, …)`, so severity is read from
    /// exactly one place per walk (C2(iv) severity-invariance), which is what let
    /// δ (task #5306) promote the whole family by flipping only the const.
    ///
    /// **Carve-out — geometry-trait conformance stays always-Error.** The
    /// `Bounded` / `Connected` / `Convex` geometry-trait leaf (reached inside
    /// [`check_leaf_trait_conformance`] via [`emit_geometry_unbounded`] /
    /// [`emit_geometry_trait_violation`]) intentionally does NOT read
    /// `ctx.severity` — those two helpers hard-code `Severity::Error`. Their codes
    /// (`GeometryUnbounded` and the geometry `TypeNotConformingToTrait`) belong to
    /// the geometry-primitive-constructors PRD, not to this knob's ctor-field
    /// surface, and are therefore excluded from α's Warning downgrade set
    /// (Vector / Selector / StructureRef / wrapped-trait / wrapper-shape /
    /// general-leaf). Consequence: the δ one-const flip will NOT promote the
    /// geometry-trait ctor diagnostics; a δ author who wants them under the knob
    /// must route those two emit sites through `ctx.severity` explicitly (and
    /// update the `emit_geometry_*_helper_produces_error_*` unit tests +
    /// `geometry_traits_inference_tests`, which currently pin Error).
    severity: Severity,
}

/// Extract the callee function name from a compiled expression if it is a
/// `FunctionCall`. Returns `None` for any other expression kind.
///
/// Shared by `promote_function_call_to_structure_ref` (promotion check) and
/// `check_leaf_trait_conformance` (suppression guard) so both decisions use
/// the same pattern — any future extension of `FunctionCall` matching applies
/// consistently to both sites.
fn extract_function_call_name(arg: &CompiledExpr) -> Option<&str> {
    match &arg.kind {
        CompiledExprKind::FunctionCall { function, .. } => Some(function.name.as_str()),
        _ => None,
    }
}

/// Promote a `Real`/`Int`-typed `FunctionCall` whose callee is a known structure
/// template into a `Type::StructureRef(callee_name)`.
///
/// Returns `None` when the arg is not a numeric-typed FunctionCall, or when the
/// callee name is not registered in the template map (external/forward-ref miss).
///
/// Used by both `walk_param_against_arg`'s fallback arm and
/// `check_leaf_trait_conformance` so future wrapper kinds (e.g. `Tuple<T,U>`)
/// stay in sync — both call sites must promote identically to avoid drift in
/// diagnostic wording or trait-bound walks.
fn promote_function_call_to_structure_ref(
    arg: &CompiledExpr,
    templates: &HashMap<String, &TopologyTemplate>,
) -> Option<Type> {
    if !matches!(arg.result_type, Type::Scalar { .. } | Type::Int) {
        return None;
    }
    let name = extract_function_call_name(arg)?;
    if !templates.contains_key(name) {
        return None;
    }
    Some(Type::StructureRef(name.to_owned()))
}

/// Recursive dispatcher: walk `param_type` lockstep against `compiled_arg`,
/// recursing into Option/List/Set/Map wrapper pairs and delegating `TraitObject`
/// to the leaf helper.
fn walk_param_against_arg(param_type: &Type, compiled_arg: &CompiledExpr, ctx: &mut WalkCtx<'_>) {
    match (param_type, &compiled_arg.kind) {
        // Option wrapper: recurse into the inner value.
        (Type::Option(inner_p), CompiledExprKind::OptionSome(inner_a)) => {
            walk_param_against_arg(inner_p, inner_a, ctx);
        }
        // none is always valid for any Option<T> param.
        (Type::Option(_), CompiledExprKind::OptionNone) => {}
        // List wrapper: recurse on each element. Covers both user-written
        // ListLiteral (compile-time) and ReflectiveCellList (post-activation,
        // task-2458) — RCL behaves identically to ListLiteral at this layer
        // and is included so future refactors that move activation earlier
        // do not silently fall through to the type-level fallback (task-2543).
        (
            Type::List(inner_p),
            CompiledExprKind::ListLiteral(elements)
            | CompiledExprKind::ReflectiveCellList(elements),
        ) => {
            for elem in elements {
                walk_param_against_arg(inner_p, elem, ctx);
            }
        }
        // Set wrapper: recurse on each element.
        (Type::Set(inner_p), CompiledExprKind::SetLiteral(elements)) => {
            for elem in elements {
                walk_param_against_arg(inner_p, elem, ctx);
            }
        }
        // Map wrapper: recurse on both key and value positions independently.
        (Type::Map(key_p, val_p), CompiledExprKind::MapLiteral(entries)) => {
            for (key_expr, val_expr) in entries {
                walk_param_against_arg(key_p, key_expr, ctx);
                walk_param_against_arg(val_p, val_expr, ctx);
            }
        }
        // Leaf: param type is a trait object — call the existing conformance logic.
        (Type::TraitObject(required_trait), _) => {
            check_leaf_trait_conformance(required_trait, compiled_arg, ctx);
        }
        // Fallback: for non-literal wrapped args (e.g. ValueRef), attempt a type-level
        // wrapper walk comparing param_type against compiled_arg.result_type. This handles
        // cases like passing `param p : Option<Physical>` to a slot of `Option<Material>`
        // where `Physical : Material`. Non-wrapper and non-trait params fall through
        // silently inside walk_param_against_arg_type.
        //
        // Apply StructureRef promotion when the compiled arg is a FunctionCall whose
        // result_type defaulted to Real/Int (the expression compiler's numeric fallback
        // for structure calls) and the callee is a known structure template. Without this,
        // wrapper-shape diagnostics would show the misleading numeric fallback type (e.g.
        // 'Real') instead of the structure name (e.g. 'Steel') for cases like
        // `Host(m: Steel())` where `m : Option<MaterialSpec>`.
        _ => {
            let promoted =
                promote_function_call_to_structure_ref(compiled_arg, ctx.registries.templates);
            let effective_type = promoted.as_ref().unwrap_or(&compiled_arg.result_type);
            walk_param_against_arg_type(param_type, effective_type, ctx);
        }
    }
}

/// Emit `DiagnosticCode::GeometryUnbounded` for a geometry-typed argument
/// at a `param g : Bounded`-shaped call site whose inferred trait set lacks
/// `bounded`.
///
/// Pushes exactly one `Diagnostic::error(...)` with code
/// [`DiagnosticCode::GeometryUnbounded`] and a single label at `span`. The
/// canonical message wording is documented on the variant declaration in
/// `crates/reify-types/src/diagnostics.rs` — keep the two in sync.
///
/// **Always Error — carve-out from the task 5302 ctor-field severity knob.**
/// This helper is reached on the ctor walk path but intentionally hard-codes
/// `Severity::Error` rather than reading `WalkCtx.severity`, so the δ one-const
/// flip does NOT govern it (see the `CTOR_FIELD_CONFORMANCE_SEVERITY` /
/// `WalkCtx.severity` carve-out docs). `emit_geometry_unbounded_helper_produces_error_*`
/// pins this.
///
/// Reserved for the **Bounded** case only. `Connected`/`Convex` violations
/// at the same call-site shape reuse [`DiagnosticCode::TypeNotConformingToTrait`]
/// per the task's design decision §2 (the PRD only allocates
/// `E_GEOMETRY_UNBOUNDED` for missing Bounded).
pub(crate) fn emit_geometry_unbounded(
    arg_name: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::error(format!(
            "geometry argument '{}' is not Bounded; required by trait parameter",
            arg_name
        ))
        .with_code(DiagnosticCode::GeometryUnbounded)
        .with_label(DiagnosticLabel::new(
            span,
            format!("geometry argument '{}' is not Bounded", arg_name),
        )),
    );
}

/// Emit `DiagnosticCode::GeometryProfileRequired` for a geometry-typed argument
/// at a profile-consuming op (`extrude`/`extrude_symmetric`/`revolve`/`loft`/
/// `loft_guided`/`sweep`/`sweep_guided`/`pipe`) whose statically-known operand
/// violates the op's dimensionality precondition.
///
/// Pushes exactly one `Diagnostic::error(...)` with code
/// [`DiagnosticCode::GeometryProfileRequired`] and a single label at `span`. The
/// `requirement` is parameterized so the one helper serves both the Surface
/// profile consumers (`requirement` e.g. `"a 2D Surface profile (Closed, Planar)"`)
/// and the Curve path consumers (`requirement` e.g. `"a 1D Curve path"`). The
/// canonical message wording is documented on the variant declaration in
/// `crates/reify-core/src/diagnostics.rs` — keep the two in sync.
///
/// `arg_name` is the **consumer's parameter-slot label** (e.g. `"profile"` or
/// `"path"`), shown to the user as the offending argument's name — callers pass
/// the slot role, NOT the operand's own producer function name, so the message
/// points at the role that is wrong (`geometry argument 'profile' must be …`).
/// Callers should pass `span` as the offending operand's own span so the label
/// pinpoints that argument.
///
/// Sibling of [`emit_geometry_unbounded`]: same shape (one Error diagnostic, one
/// label at `span`) and the same non-fatal contract — the caller still lowers
/// the op. See PRD `docs/prds/geometry-primitive-constructors.md` task α.
pub(crate) fn emit_geometry_profile_required(
    arg_name: &str,
    requirement: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::error(format!(
            "geometry argument '{}' must be {}",
            arg_name, requirement
        ))
        .with_code(DiagnosticCode::GeometryProfileRequired)
        .with_label(DiagnosticLabel::new(
            span,
            format!("'{}' is not {}", arg_name, requirement),
        )),
    );
}

/// Emit a "does not conform to trait" geometry diagnostic for the `Connected`/`Convex`
/// cases — the symmetric sibling of [`emit_geometry_unbounded`] for the `Bounded` case.
///
/// Pushes exactly one `Diagnostic::error(...)` with code
/// [`DiagnosticCode::TypeNotConformingToTrait`] and a single label at `span`. The PRD
/// only allocates `E_GEOMETRY_UNBOUNDED` for missing `Bounded`; `Connected`/`Convex`
/// reuse `TypeNotConformingToTrait`.
///
/// **Always Error — carve-out from the task 5302 ctor-field severity knob**, for the
/// same reason as its sibling [`emit_geometry_unbounded`]: geometry-trait conformance
/// is not part of α's Warning downgrade set, so this hard-codes `Severity::Error` and
/// is not flipped by the δ const. `emit_geometry_trait_violation_helper_produces_error_*`
/// pins this.
///
/// The message intentionally does **not** include a separate param-name slot. Under
/// reify's keyword-arg convention the arg name and param name are identical in practice,
/// so appending `required by param '{}'` a second time is redundant and was dropped.
pub(crate) fn emit_geometry_trait_violation(
    arg_name: &str,
    required_trait: &str,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.push(
        Diagnostic::error(format!(
            "geometry argument '{}' does not conform to trait '{}'",
            arg_name, required_trait
        ))
        .with_code(DiagnosticCode::TypeNotConformingToTrait)
        .with_label(DiagnosticLabel::new(
            span,
            format!("geometry argument '{}' is not {}", arg_name, required_trait),
        )),
    );
}

/// Shared leaf helper: emit a "does not conform to trait" diagnostic if `arg_type`
/// does not satisfy `required_trait`.
///
/// Handles `Type::StructureRef` (walks trait bounds transitively via
/// `satisfies_trait_bound`) and `Type::TraitObject` (equality-or-refinement via
/// `trait_satisfies`). Other types are skipped silently — callers add their own
/// fallback arm for non-struct/non-trait types.
///
/// This is the single source of the StructureRef/TraitObject diagnostic wording and
/// `DiagnosticCode::TypeNotConformingToTrait` code shared by both the literal walker
/// (`check_leaf_trait_conformance`) and the type-level fallback walker
/// (`walk_param_against_arg_type`).
fn emit_leaf_conformance_for_arg_type(
    arg_type: &Type,
    required_trait: &str,
    ctx: &mut WalkCtx<'_>,
) {
    match arg_type {
        Type::StructureRef(struct_name) => {
            let Some(arg_template) = ctx.registries.templates.get(struct_name.as_str()) else {
                return;
            };
            if !satisfies_trait_bound(
                &arg_template.trait_bounds,
                required_trait,
                ctx.registries.traits,
            ) {
                ctx.diagnostics.push(
                    diag_at(
                        ctx.severity,
                        format!(
                            "type '{}' does not conform to trait '{}' required by param '{}'",
                            struct_name, required_trait, ctx.arg_name
                        ),
                    )
                    .with_code(DiagnosticCode::TypeNotConformingToTrait)
                    .with_label(DiagnosticLabel::new(
                        ctx.span,
                        format!(
                            "type '{}' does not conform to trait '{}'",
                            struct_name, required_trait
                        ),
                    )),
                );
            }
        }
        Type::TraitObject(arg_trait_name) => {
            let mut visited = HashSet::new();
            if !trait_satisfies(
                arg_trait_name,
                required_trait,
                ctx.registries.traits,
                &mut visited,
            ) {
                ctx.diagnostics.push(
                    diag_at(
                        ctx.severity,
                        format!(
                            "type '{}' does not conform to trait '{}' required by param '{}'",
                            arg_trait_name, required_trait, ctx.arg_name
                        ),
                    )
                    .with_code(DiagnosticCode::TypeNotConformingToTrait)
                    .with_label(DiagnosticLabel::new(
                        ctx.span,
                        format!(
                            "trait '{}' does not refine trait '{}'",
                            arg_trait_name, required_trait
                        ),
                    )),
                );
            }
        }
        // Non-struct, non-trait arg type — callers handle this with their own fallback arm.
        _ => {}
    }
}

/// Emit a single `Diagnostic::error` with
/// [`DiagnosticCode::TypeNotConformingToStructureRef`] when an arg type does not
/// match a `Type::StructureRef` param (task-4584).
///
/// Modelled on [`emit_leaf_conformance_for_arg_type`]: one diagnostic, one label
/// at `ctx.span`, message names the required structure and the offending type.
fn emit_structure_ref_mismatch(param_type: &Type, arg_type: &Type, ctx: &mut WalkCtx<'_>) {
    ctx.diagnostics.push(
        diag_at(
            ctx.severity,
            format!(
                "argument '{}' has type '{}' but param '{}' requires structure type '{}'",
                ctx.arg_name, arg_type, ctx.arg_name, param_type,
            ),
        )
        .with_code(DiagnosticCode::TypeNotConformingToStructureRef)
        .with_label(DiagnosticLabel::new(
            ctx.span,
            format!("expected '{}', got '{}'", param_type, arg_type),
        )),
    );
}

/// Emit a single `Diagnostic::error` with
/// [`DiagnosticCode::TypeNotConformingToVector`] when an arg does not conform to a
/// `Type::Vector`-typed param (task-4622): either the arg is not vector-shaped, or it is
/// `Type::Vector` with a mismatched arity (n).
///
/// Conformance rules:
/// - A `Type::Vector { n, .. }` arg is accepted only when `n` matches the param's `n`.
///   Arity mismatch (e.g. vec2 passed to a vec3 param) is a genuine type error.
/// - A `Type::Tensor { rank: 1, .. }` arg is accepted regardless of element count
///   (Tensor arity is not yet pinned in param signatures, so accepted conservatively).
/// - The quantity slot is intentionally loose (ty.rs Point/Vector quantity-slot convention).
/// - Bare scalars, strings, bools, and other non-vector kinds are rejected.
///
/// Modelled on [`emit_structure_ref_mismatch`]: one diagnostic, one label at `ctx.span`,
/// message names the required vector type and the offending arg type.
fn emit_vector_mismatch(param_type: &Type, arg_type: &Type, ctx: &mut WalkCtx<'_>) {
    ctx.diagnostics.push(
        diag_at(
            ctx.severity,
            format!(
                "argument '{}' has type '{}' but param '{}' requires vector type '{}'",
                ctx.arg_name, arg_type, ctx.arg_name, param_type,
            ),
        )
        .with_code(DiagnosticCode::TypeNotConformingToVector)
        .with_label(DiagnosticLabel::new(
            ctx.span,
            format!("expected '{}', got '{}'", param_type, arg_type),
        )),
    );
}

/// Emit a single diagnostic when an arg type does not match a `Type::Selector` or
/// `Type::AnySelector` param (task-4598), branching the code on the *nature* of the
/// mismatch (task 5302 α, D2):
///
/// - **Selector-kind mismatch** — a concrete `Selector(j)` arg with `j != k`, or a
///   bare `AnySelector` arg, against a single-kind `Selector(k)` param — carries
///   [`DiagnosticCode::SelectorKindMismatch`] and names the expected/found selector
///   kinds (the `SelectorKind` Display strings, e.g. `FaceSelector`/`EdgeSelector`).
///   This mirrors the fn-call param-binding path's `SelectorKindMismatch` tagging
///   (task 4581 / 4371 BT1) so the same wrong-kind error is coded uniformly in both
///   the ctor and fn-call surfaces.
/// - **Pose-vs-set** — a coordinate-pose arg (`Frame`/`Transform`/`Point`) against
///   a selector-typed param — keeps [`DiagnosticCode::ArgTypeMismatch`] but appends
///   the fixed hint substring `a coordinate pose is not a region target; select a
///   face/edge/vertex instead` (PRD §4 D2; task 4833's fixtures assert on it). A
///   message variant, not a new failure class.
/// - **Every other selector-slot mismatch** — a non-selector arg (`String`, `Int`,
///   …), or any arg against a bare `AnySelector` param — keeps
///   [`DiagnosticCode::ArgTypeMismatch`] with the plain message (the disallow-string
///   over-tag guard per 4581).
///
/// Modelled on [`emit_structure_ref_mismatch`] / [`emit_vector_mismatch`]: one
/// diagnostic, one label at `ctx.span`. No new code minted (both already exist in
/// diagnostics.rs). Severity is `ctx.severity` in BOTH branches (severity-invariant:
/// the 5302 knob governs the whole selector surface uniformly — see design decision
/// Q3/Option-A). The same-kind case (`Selector(k)` arg vs `Selector(k)` param) never
/// reaches here — `reject_if_incompatible`'s `type_compatible` gate accepts it — so
/// the `j != k` guard is defensive, not load-bearing.
fn emit_selector_mismatch(param_type: &Type, arg_type: &Type, ctx: &mut WalkCtx<'_>) {
    let is_kind_mismatch = match (param_type, arg_type) {
        (Type::Selector(k), Type::Selector(j)) => j != k,
        (Type::Selector(_), Type::AnySelector) => true,
        _ => false,
    };
    if is_kind_mismatch {
        ctx.diagnostics.push(
            diag_at(
                ctx.severity,
                format!(
                    "argument '{}' has selector kind '{}' but param '{}' requires selector kind '{}'",
                    ctx.arg_name, arg_type, ctx.arg_name, param_type,
                ),
            )
            .with_code(DiagnosticCode::SelectorKindMismatch)
            .with_label(DiagnosticLabel::new(
                ctx.span,
                format!("expected '{}', got '{}'", param_type, arg_type),
            )),
        );
    } else {
        // Pose-vs-set (task 5302 α, PRD §4 D2): a coordinate pose — `Frame(n)`,
        // `Transform(n)`, or `Point { .. }` — passed to a selector-typed field is
        // a common authoring confusion (a pose locates a datum; a selector names a
        // region target). It stays `ArgTypeMismatch` (a message variant, NOT a new
        // failure class / code) but carries a fixed hint substring — the
        // deterministic string task 4833's pose-vs-set fixtures assert on
        // verbatim. Every non-pose non-selector arg (`String`, `Int`, …) keeps the
        // plain message (disallow-string over-tag guard, 4581).
        let is_pose = matches!(
            arg_type,
            Type::Frame(_) | Type::Transform(_) | Type::Point { .. }
        );
        let mut message = format!(
            "argument '{}' has type '{}' but param '{}' requires selector type '{}'",
            ctx.arg_name, arg_type, ctx.arg_name, param_type,
        );
        if is_pose {
            message.push_str(
                "; a coordinate pose is not a region target; select a face/edge/vertex instead",
            );
        }
        ctx.diagnostics.push(
            diag_at(ctx.severity, message)
                .with_code(DiagnosticCode::ArgTypeMismatch)
                .with_label(DiagnosticLabel::new(
                    ctx.span,
                    format!("expected '{}', got '{}'", param_type, arg_type),
                )),
        );
    }
}

/// Skip-guard + `type_compatible` gate shared by the `StructureRef` and `Selector/AnySelector`
/// leaf arms in [`walk_param_against_arg_type`] (task-4584, task-4598).
///
/// Returns without emitting when `arg_ty` is one of the conservatively-skipped kinds:
/// - `Error` — anti-cascade poison sentinel.
/// - `TypeParam` — unresolved generic; conformance decided at instantiation.
/// - `Geometry` — carries no nominal identity verifiable at the type level.
/// - `TraitObject` — may resolve to a conforming type; defer.
///
/// For all other concrete arg types, calls `type_compatible`; if that returns false,
/// delegates to `emit` to push the mismatch diagnostic. Encapsulating the three-line
/// body prevents the anti-cascade skip list from drifting between the two arms.
///
/// Emit a single `DiagnosticCode::ArgTypeMismatch` diagnostic (at `ctx.severity`)
/// for a general concrete-leaf param/arg type mismatch (task 5302 α — the fully
/// general arg-shape pass that replaces the old silent `_` fall-through in
/// [`walk_param_against_arg_type`]).
///
/// Message shape mirrors the 4598 selector wording (`emit_selector_mismatch`):
/// `"argument 'X' has type 'A' but param 'X' requires type 'B'"`. Reused for any
/// non-wrapper / non-trait / non-selector / non-vector / non-structure leaf whose
/// arg type is not `type_compatible` with the declared param type (String←Int,
/// Bool←String, dimensioned-scalar mismatch, …). No new diagnostic code is minted
/// — `ArgTypeMismatch` already exists.
///
/// A rejection at a DIMENSIONED `Scalar` param additionally carries the D4-6
/// migration hint (see [`dimensioned_scalar_migration_hint`]); every other
/// family's message is byte-identical to what it was before task 5627.
fn emit_arg_type_mismatch(param_type: &Type, arg_ty: &Type, ctx: &mut WalkCtx<'_>) {
    let mut message = format!(
        "argument '{}' has type '{}' but param '{}' requires type '{}'",
        ctx.arg_name, arg_ty, ctx.arg_name, param_type,
    );
    // Gated on the param being a DIMENSIONED scalar, not applied unconditionally:
    // this emitter is shared with the `Bool` / `Int` / `String` / dimensionless
    // `Real` families task 5465 promoted, where the clause would be nonsense
    // ("pass a dimensioned Bool literal") and would silently reword four
    // already-shipped diagnostics.
    let dimensioned = match param_type {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Some(dimension),
        _ => None,
    };
    if let Some(dimension) = dimensioned {
        message.push_str("; ");
        message.push_str(&dimensioned_scalar_migration_hint(dimension));
    }
    ctx.diagnostics.push(
        diag_at(ctx.severity, message)
            .with_code(DiagnosticCode::ArgTypeMismatch)
            .with_label(DiagnosticLabel::new(
                ctx.span,
                format!("expected '{}', got '{}'", param_type, arg_ty),
            )),
    );
}

/// Whether each [`BASE_UNIT_SYMBOLS`] slot is spellable as a unit LITERAL, in
/// the same slot order.
///
/// `BASE_UNIT_SYMBOLS` is the DISPLAY table: it names every slot so a
/// `DimensionVector` can be rendered. That is a weaker property than being
/// parseable — `stdlib/units.ri` declares `m`, `kg`, `s`, `K`, `rad` and `USD`
/// as units, but NOT `A`, `mol`, `cd` or `sr`, so a composed `…/A` literal fails
/// with `unknown unit: A`. Measured, not assumed: the round-trip guard in
/// `struct_ctor_field_conformance_tests.rs` caught exactly this on `Voltage`
/// (`1m^2*kg/s^3/A`).
///
/// `[bool; 10]`, same as the symbol table, so an 11th base dimension breaks at
/// compile time here too. Registering one of the missing symbols in the stdlib
/// is a one-flag change, and the round-trip guard proves it immediately.
const BASE_UNIT_SYMBOL_IS_SPELLABLE: [bool; 10] = [
    true,  // m
    true,  // kg
    true,  // s
    false, // A
    true,  // K
    false, // mol
    false, // cd
    true,  // rad
    false, // sr
    true,  // USD
];

/// An example unit literal for `dimension`, spelled in reify's compound-unit
/// grammar — `"1m"`, `"1m/s^2"`, `"1kg/m^3"`, `"1kg*m^2/s^2"`.
///
/// Composed from [`BASE_UNIT_SYMBOLS`] rather than looked up in a per-dimension
/// table, which is what makes the hint unable to rot: every present and future
/// `NAMED_DIMENSIONS` row is covered uniformly, with no second list to keep in
/// step. (`si_units.rs`'s `SI_DERIVED_UNITS` was the obvious table and is not
/// usable: it covers ~25 derived units and misses Length, Mass, Time, Area,
/// Volume, Density — and Velocity and Acceleration, the two dimensions in this
/// task's own signal.)
///
/// NOT built from [`DimensionVector`]'s `Display`, which joins with `·` and
/// emits `^-1`; neither is reify syntax. Positive exponents go to a `*`-joined
/// numerator and negative ones to `/`-prefixed denominator terms, the shape
/// `compound_unit_resolution_tests.rs` pins as accepted (`5kN*m`, `9.81m/s^2`,
/// `7850kg/m^3`, `0.001kg/m/s`).
///
/// `None` when no clean literal exists — three cases, all measured against the
/// round-trip guard rather than guessed:
/// * a FRACTIONAL exponent (`den() != 1`, e.g. `FractureToughness`'s Pa·m^0.5)
///   has no unit-literal spelling at all;
/// * an EMPTY numerator (e.g. `Frequency`, s⁻¹) would need a bare `1/s`, which
///   reify's grammar reads as a division rather than as a quantity literal;
/// * a slot whose symbol the stdlib does not register as a unit (see
///   [`BASE_UNIT_SYMBOL_IS_SPELLABLE`]), e.g. `Voltage`'s ampere term.
///
/// Declining is deliberate: an example that does not parse is worse than none,
/// because the reader would paste it. The round-trip guard in
/// `struct_ctor_field_conformance_tests.rs` walks every `NAMED_DIMENSIONS` row
/// and fails the build if any offered example stops resolving to its own
/// dimension.
fn example_unit_literal(dimension: &DimensionVector) -> Option<String> {
    let mut numerator: Vec<String> = Vec::new();
    let mut denominator: Vec<String> = Vec::new();

    for (slot, exponent) in dimension.0.iter().enumerate() {
        if exponent.num() == 0 {
            continue;
        }
        if exponent.den() != 1 {
            return None;
        }
        if !BASE_UNIT_SYMBOL_IS_SPELLABLE[slot] {
            return None;
        }
        let symbol = BASE_UNIT_SYMBOLS[slot];
        let power = exponent.num();
        let magnitude = power.abs();
        let term = if magnitude == 1 {
            symbol.to_owned()
        } else {
            format!("{symbol}^{magnitude}")
        };
        if power > 0 {
            numerator.push(term);
        } else {
            denominator.push(term);
        }
    }

    // MEASURED, not assumed: `structure def S { param x : Frequency = 1/s }`
    // fails to compile with `unresolved name: s` — with no numerator term to
    // anchor it, the leading `1` is parsed as a plain number and `/s` as a
    // division by an identifier, never as a quantity literal.
    if numerator.is_empty() {
        return None;
    }

    let mut literal = format!("1{}", numerator.join("*"));
    for term in &denominator {
        literal.push('/');
        literal.push_str(term);
    }
    Some(literal)
}

/// The D4-6 migration hint clause appended to a rejection at a DIMENSIONED
/// `Scalar` ctor slot.
///
/// Shape: ``"pass a dimensioned <Dimension> literal such as `<example>`"``,
/// COPIED from `ArgRejection::message` in `crates/reify-ir/src/arg_acceptance.rs`
/// so the compile-time and runtime diagnostics for the same authoring mistake
/// read the same way. The clause itself is computed here rather than imported:
/// the only hints `reify-ir` renders are the fixed LENGTH / Density / Angle
/// literals it reads from `reify_core::units`, nothing that works for an
/// arbitrary dimension — and the ctor-slot LENGTH wording deliberately differs
/// from the shared one (pinned by
/// `builtin_slot_and_ctor_conformance_length_hints_are_deliberately_different`).
///
/// Degrades rather than disappearing, so the hint is present unconditionally at
/// a dimensioned slot:
/// * a composite dimension not in `NAMED_DIMENSIONS` has no `canonical_name()`,
///   and falls back to the `Display` form as the NAME (where `·`/`^-1` are fine
///   — it is prose, not something the reader pastes);
/// * a dimension with no clean example literal keeps the clause and drops only
///   the `such as` tail.
fn dimensioned_scalar_migration_hint(dimension: &DimensionVector) -> String {
    let name = dimension
        .canonical_name()
        .map(str::to_owned)
        .unwrap_or_else(|| dimension.to_string());
    match example_unit_literal(dimension) {
        Some(example) => format!("pass a dimensioned {name} literal such as `{example}`"),
        None => format!("pass a dimensioned {name} literal"),
    }
}

/// The arg types that every leaf gate in this walker conservatively SKIPS rather
/// than judges: each is "unknown / unverifiable here", NOT "definitely
/// non-conforming", so emitting on one would be a false positive.
///
/// * `Error` — anti-cascade poison sentinel; the root-cause diagnostic already fired.
/// * `TypeParam` — an unresolved generic type variable; real conformance is
///   decided when the var is bound at instantiation.
/// * `Geometry` — carries no nominal identity verifiable at the type level
///   (geometry conformance is decided from the compiled op-array, reachable only
///   through the literal walker).
/// * `TraitObject` — may resolve to a conforming type at evaluation; defer.
///
/// # Why a named helper rather than a repeated `matches!`
///
/// [`reject_if_incompatible`] and the four SHAPE-BASED leaf arms (`Vector`,
/// `Point`, `Matrix`/`Tensor`, `Field`) all need exactly this set. Before task
/// 5465's amendment pass each spelled it out inline and the arm comments claimed
/// the copies were "character-for-character" identical so they "cannot drift" —
/// a claim enforced only by comment. Routing every caller through one function
/// makes it structurally true.
///
/// # `Type::ScalarParam` is deliberately NOT in this set
///
/// `Type::ScalarParam(Q)` is the unresolved *dimension*-parameter placeholder
/// (`type_resolution.rs:3134`), which superficially looks like a sibling of
/// `TypeParam`. It is not, for this purpose: with `TypeParam` the whole type is
/// unknown, whereas with `ScalarParam` the FAMILY is known (it is a scalar) and
/// only its dimension is open. Adding it here would silence genuine family-level
/// mismatches such as `String ← Scalar<Q>` at every arm at once. Instead, the
/// two arms that already tolerate a scalar-family arg (`Point`,
/// `Matrix`/`Tensor`) accept it through [`is_numeric_placeholder_leaf`], which
/// is where its scalar-ness — not its unknown-ness — is the load-bearing
/// property.
fn arg_type_is_unverifiable(arg_ty: &Type) -> bool {
    matches!(
        arg_ty,
        Type::Error | Type::TypeParam(_) | Type::Geometry | Type::TraitObject(_)
    )
}

/// The NARROW numeric-leaf predicate the `Point` and `Matrix`/`Tensor` shape
/// arms use to accept a scalar-family arg (task 5465).
///
/// Its membership is exactly what the `matches!` below says: `Type::Int`, ANY
/// `Type::Scalar { .. }` — dimensioned or not, the match is dimension-BLIND —
/// and `Type::ScalarParam(_)`, a scalar shape whose DIMENSION is unresolved
/// rather than absent (see [`arg_type_is_unverifiable`]'s closing note). Read
/// the membership off the predicate, never off an example.
///
/// This is a SHARED helper, and each call site states its own reason for wanting
/// it: the `Point` arm (below), the `Matrix`/`Tensor` arm, and
/// `list_bottoms_out_numeric`. Notably the `Matrix`/`Tensor` arm wants the
/// `Type::Scalar { .. }` leg for rank-0 scalar equivalence (Rules 2a/2b), where
/// the arg is a REAL dimensioned scalar and not a placeholder at all — so
/// narrowing this predicate to `Int | ScalarParam` to suit the `Point` arm
/// would break that accept. Any claim about which args "survive" belongs to a
/// call site's own branch, not here.
///
/// `point3(…)` / `point2(…)` are NOT among the `Point` arm's placeholder inputs:
/// they carry a real `Type::Point` and take that arm's OTHER branch. Ruled once,
/// in the *Point / Vector quantity-slot convention* section of
/// `crates/reify-core/src/ty.rs`.
///
/// Deliberately NOT `type_compat.rs::is_scalar_like_leaf`, which also admits
/// `Bool`, `String`, `Enum`, `StructureRef`, `TraitObject` and `Geometry` — that
/// would make `Anchor(origin: "origin")` silent and defeat the value floors.
fn is_numeric_placeholder_leaf(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Scalar { .. } | Type::ScalarParam(_))
}

/// Whether a `Type::ScalarParam(_)` ARG at a `Type::Scalar { .. }` PARAM must be
/// DEFERRED rather than judged (task 5627 γ, D4-5; PRD invariant I5 / §7.4 B4).
///
/// `Type::ScalarParam(Q)` is the unresolved-DIMENSION placeholder a dim-kinded
/// generic produces: `fn fwd<Q: Dimension>(x: Scalar<Q>)` resolves `x` to
/// `ScalarParam("Q")` (`type_resolution.rs`), displayed `Scalar<Q>`. Its FAMILY
/// is known — it is a scalar — and only its DIMENSION is open (see
/// [`arg_type_is_unverifiable`]'s closing note, which draws the same distinction
/// against `TypeParam`). At a `Scalar` slot there is therefore no dimension to
/// compare: `Q` is bound at instantiation, and whether that binding conforms is
/// decided there. Judging the UNINSTANTIATED body under the strict
/// `DimensionVector` equality γ routes into this arm can only ever REJECT, so
/// every such site would be a false positive — one γ's own promotion opens.
///
/// # Deliberately narrower than [`is_numeric_placeholder_leaf`]
///
/// That predicate also matches `Type::Int` and any CONCRETE `Type::Scalar { .. }`.
/// Reusing it as the arg-side accept HERE would silence `Scalar<Q> ← Int` (I3)
/// and `Scalar<Length> ← Scalar<Mass>` (I2) — precisely the two rejections γ
/// exists to produce — leaving the promotion green, the corpus clean and the
/// whole task functionally inert. The `Point` and `Matrix`/`Tensor` arms want its
/// full membership set, each for the reason stated at its own call site; this
/// arm wants only the dimension-placeholder half.
///
/// It is a PER-ARM guard for the same reason it is not an entry in
/// [`arg_type_is_unverifiable`]: adding it there would silence
/// `String ← Scalar<Q>` at every arm at once, which that predicate's own doc
/// comment already rules out.
///
/// # A2 — the INTENDED side effect on the dimensionless half
///
/// `Real` is `Type::Scalar { dimension: <dimensionless> }`, so this necessarily
/// also silences `Real ← Scalar<Q>` — a warning belonging to task 5465's
/// already-shipped dimensionless family, not to the family γ promotes. That is
/// intended, not collateral: the argument for silence is identical in both
/// halves, since `Q` is unbound either way and there is nothing to compare.
/// Narrowing the guard to `!dimension.is_dimensionless()` to preserve that
/// warning would assert `Scalar<Q>` is definitely-not-`Real` while
/// simultaneously accepting it as maybe-`Scalar<Length>`.
///
/// All five behaviours above — the accept, the A2 post-state, and the three
/// narrowness fences — are pinned by name in the γ D4-5 section of
/// `crates/reify-compiler/tests/harness_structure_declarations/struct_ctor_field_conformance_tests.rs`.
fn scalar_param_arg_defers_at_scalar_slot(param_type: &Type, arg_ty: &Type) -> bool {
    matches!(param_type, Type::Scalar { .. }) && matches!(arg_ty, Type::ScalarParam(_))
}

/// ARG-side predicate: the dimension an ARG's quantity slot names — TOLERANT, so
/// a dimensionless `Scalar` (like `Type::Int` / `Type::ScalarParam`) names none.
///
/// Normative rule, and why this side is the tolerant one: the "Point / Vector
/// quantity-slot convention" section of `crates/reify-core/src/ty.rs` (tasks
/// 5766, 6159). This is its implementation, not a second source of truth.
fn arg_quantity_slot_dimension(quantity: &Type) -> Option<DimensionVector> {
    match quantity {
        Type::Scalar { dimension } if !dimension.is_dimensionless() => Some(*dimension),
        _ => None,
    }
}

/// PARAM-side predicate: STRICT sibling of [`arg_quantity_slot_dimension`] — a
/// dimensionless `Scalar` DOES name a dimension here; `Type::Int` /
/// `Type::ScalarParam` still name none.
///
/// Ruling and basis: `crates/reify-core/src/ty.rs` (task 6159).
fn param_quantity_slot_dimension(quantity: &Type) -> Option<DimensionVector> {
    match quantity {
        Type::Scalar { dimension } => Some(*dimension),
        _ => None,
    }
}

/// The quantity slot carried by a `Vector`/`Point`/`Matrix`/`Tensor` type, or
/// `None` for every other shape (`Type::List` — the nested-list-literal matrix
/// spelling — an [`is_numeric_placeholder_leaf`] scalar, …).
///
/// FAMILY-based, never side-based, so BOTH sides of the comparison are extracted
/// by this one function and cannot drift apart. Returning `Some` for a family a
/// given arm does not accept is harmless: every caller runs this only AFTER its
/// own family/arity check.
fn quantity_slot(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Vector { quantity, .. }
        | Type::Point { quantity, .. }
        | Type::Matrix { quantity, .. }
        | Type::Tensor { quantity, .. } => Some(quantity),
        _ => None,
    }
}

/// Apply the quantity-slot rule at one shape arm, emitting
/// [`DiagnosticCode::ArgTypeMismatch`] at `ctx.severity` when it fires.
///
/// Rule, rationale and residuals: the "Point / Vector quantity-slot convention"
/// section of `crates/reify-core/src/ty.rs`. This is its implementation, not a
/// second source of truth.
///
/// # Caller contract
///
/// Call this ONLY after the arm's own family/arity check has passed, and ONLY
/// from an arm whose `param_type` is a quantity-carrying shape — the
/// `debug_assert!` below is that second half, made loud in tests rather than
/// left to no-op silently. That ordering plus the [`quantity_slot`] gate is how
/// PRD 4's D4-5 unknown-ness fence survives at these arms: a placeholder scalar
/// carries no slot and so never reaches the comparison, and a `String` is
/// rejected before it. Routing all three arms here also keeps the diagnostic
/// `ArgTypeMismatch` at every one, leaving the `Vector` arm's bespoke
/// `TypeNotConformingToVector` owning ARITY/FAMILY failures only.
///
/// The rule is ASYMMETRIC, so each side is asked by its own
/// `*_quantity_slot_dimension` by NAME and the emission is INLINE, where both
/// types and both slots are already bound — leaving no parameter list of
/// adjacent `&Type`s for a call site to transpose. The comparison itself is the
/// derived `PartialEq` on [`DimensionVector`], the same primitive the bare-`Scalar`
/// leaf rule uses.
///
/// Two wording choices below are deliberate and are pinned by test, not by this
/// comment: the parenthetical claims ACCEPTANCE rather than shape EQUALITY (two
/// arms reach here with a genuinely different shape), and it names "the compared
/// shape" rather than "the arg type", which on the `Option`/`List`/`Set`/`Map`
/// wrapper recursion is the INNER type, not the one the user wrote.
/// `assert_quantity_slot_conflict` pins the discriminating fragments at every
/// probe and `dimensionless_quantity_param_rejects_dimensioned_vector_arg` pins
/// the whole rendering once.
fn emit_if_quantity_conflict(param_type: &Type, arg_ty: &Type, ctx: &mut WalkCtx<'_>) {
    debug_assert!(
        quantity_slot(param_type).is_some(),
        "emit_if_quantity_conflict must be called from a quantity-carrying param arm \
         (Vector/Point/Matrix/Tensor), got param type '{param_type}'. A shape arm added \
         by copy/paste whose family `quantity_slot` does not cover would otherwise \
         compile, run, and never emit, with no signal."
    );
    if let (Some(param_quantity), Some(arg_quantity)) =
        (quantity_slot(param_type), quantity_slot(arg_ty))
        && let (Some(param_dim), Some(arg_dim)) = (
            param_quantity_slot_dimension(param_quantity),
            arg_quantity_slot_dimension(arg_quantity),
        )
        && param_dim != arg_dim
    {
        ctx.diagnostics.push(
            diag_at(
                ctx.severity,
                format!(
                    "argument '{}' has quantity '{}' but param '{}' requires quantity '{}' \
                     (the compared shape '{}' is otherwise accepted at '{}'; only the quantity \
                     slot disagrees)",
                    ctx.arg_name, arg_quantity, ctx.arg_name, param_quantity, arg_ty, param_type,
                ),
            )
            .with_code(DiagnosticCode::ArgTypeMismatch)
            .with_label(DiagnosticLabel::new(
                ctx.span,
                format!("quantity mismatch: expected '{param_quantity}', got '{arg_quantity}'"),
            )),
        );
    }
}

/// Whether a `Type::List` arg at a `Matrix`/`Tensor` param is plausibly the
/// idiomatic nested-list-literal spelling of that matrix — i.e. it bottoms out
/// in a NUMERIC (or already tensor-shaped) element type.
///
/// The `Matrix`/`Tensor` arm accepts `Type::List` because
/// `[[0.0, …], [0.0, …], [0.0, …]]` is how every `examples/dynamics/*_idyn.ri`
/// spells a `MassProperties.inertia`, and that literal compiles to
/// `List<List<Real>>`. But "is a list" is not the same claim as "is a matrix
/// literal": an UNCONSTRAINED `Type::List(_)` accept also swallows
/// `Body(inertia: ["a", "b"])` (arg type `List<String>`), because a
/// `Matrix`-typed param never pairs with the literal walker's
/// `(Type::List(param), ListLiteral)` arm and so arrives here as a bare type.
/// That left the just-promoted family with a hole on exactly the shape the
/// accept was written for.
///
/// Peeling `List` recursively (rather than checking one level) keeps the rule
/// rank-agnostic: `List<Real>` for a rank-1 tensor, `List<List<Real>>` for a
/// 3×3 matrix, `List<List<List<Real>>>` for a rank-3 tensor. ARITY is still not
/// checked — see the arm comment for why that asymmetry with `Point`/`Vector` is
/// deliberate — only the element FAMILY is.
///
/// `Error` and `TypeParam` bottoms return true: those elements are unknown, not
/// definitely non-numeric, and rejecting them would violate the same
/// anti-cascade / defer-to-instantiation contract [`arg_type_is_unverifiable`]
/// encodes at the top level. `Geometry` / `TraitObject` are NOT included: a
/// `List<Geometry>` at a matrix slot is a genuine family mismatch, and neither
/// is a numeric-literal shape.
fn list_bottoms_out_numeric(ty: &Type) -> bool {
    match ty {
        Type::List(inner) => list_bottoms_out_numeric(inner),
        Type::Vector { .. } | Type::Matrix { .. } | Type::Tensor { .. } => true,
        Type::Error | Type::TypeParam(_) => true,
        other => is_numeric_placeholder_leaf(other),
    }
}

/// Vector's bespoke arity logic (`emit_vector_mismatch`) is intentionally separate.
fn reject_if_incompatible<F>(param_type: &Type, arg_ty: &Type, ctx: &mut WalkCtx<'_>, emit: F)
where
    F: FnOnce(&Type, &Type, &mut WalkCtx<'_>),
{
    if !arg_type_is_unverifiable(arg_ty) && !type_compatible(param_type, arg_ty) {
        emit(param_type, arg_ty, ctx);
    }
}

/// Type-level fallback walker: compare `param_type` against `arg_type` wrapper-by-wrapper.
///
/// Used when the compiled arg is not a literal (e.g. a `ValueRef`), so its inner
/// expressions are not available for inspection. Walks the wrapper structure of
/// `arg_type` in lockstep with `param_type`. At a `Type::TraitObject` leaf,
/// `StructureRef`/`TraitObject` args defer to `emit_leaf_conformance_for_arg_type`;
/// `Geometry`/`Error`/`TypeParam` args are skipped (unverifiable at the type level /
/// anti-cascade / unresolved generic); any other leaf type (scalar, enum, point, …)
/// cannot carry trait bounds and emits a
/// `TypeNotConformingToTrait` directly — mirroring the literal walker's fallback so a
/// wrapper-nested trait param reached with a non-literal scalar arg is not silently
/// accepted (task-4081 soundness, esc-4081-174).
///
/// Wrapper-shape mismatches (e.g. `Option<T>` param vs `List<T>` arg, or bare leaf arg
/// vs wrapper param) emit a `TypeNotConformingToTrait` diagnostic when `param_type` is
/// `Option/List/Set/Map`. A top-level `Type::Error` in either `param_type` or `arg_type`
/// short-circuits the whole walk via an early return, so no diagnostic — wrapper-shape,
/// leaf-conformance, or future-arm — is emitted on top of an already-reported upstream
/// error. The guard runs before the match, making the anti-cascade contract uniform
/// across all arms (current and future). Note: this guard is top-level only — an `Error`
/// nested inside a wrapper (e.g. `Option<Error>`) is not detected and may produce a
/// secondary wrapper-shape diagnostic on top of the root-cause error. Since task 5302,
/// non-wrapper, non-trait param types reach the general concrete-leaf arm, which checks
/// the families [`general_leaf_param_family_is_validated`] admits; the dedicated
/// shape arms (Vector, Point, Matrix/Tensor, Field, Selector) and the enum branch
/// check theirs. Every remaining family falls through silently (owner #7958).
fn walk_param_against_arg_type(param_type: &Type, arg_type: &Type, ctx: &mut WalkCtx<'_>) {
    // Anti-cascade: skip when either type carries the poison sentinel so no
    // wrapper-shape, leaf-conformance, or future-arm diagnostic piles on top
    // of an already-reported upstream error. Hoisted above the match so the
    // contract is explicit and uniform across all current and future arms.
    if matches!(param_type, Type::Error) || matches!(arg_type, Type::Error) {
        return;
    }
    match (param_type, arg_type) {
        // Wrapper pairs: recurse into inner types lockstep.
        (Type::Option(inner_p), Type::Option(inner_a)) => {
            walk_param_against_arg_type(inner_p, inner_a, ctx);
        }
        (Type::List(inner_p), Type::List(inner_a)) => {
            walk_param_against_arg_type(inner_p, inner_a, ctx);
        }
        (Type::Set(inner_p), Type::Set(inner_a)) => {
            walk_param_against_arg_type(inner_p, inner_a, ctx);
        }
        (Type::Map(key_p, val_p), Type::Map(key_a, val_a)) => {
            walk_param_against_arg_type(key_p, key_a, ctx);
            walk_param_against_arg_type(val_p, val_a, ctx);
        }
        // Leaf: param type is a trait object.
        // No FunctionCall promotion needed here — we work with the resolved result_type.
        (Type::TraitObject(required_trait), arg_ty) => match arg_ty {
            // StructureRef / TraitObject args: walk trait bounds via the shared helper.
            Type::StructureRef(_) | Type::TraitObject(_) => {
                emit_leaf_conformance_for_arg_type(arg_ty, required_trait, ctx);
            }
            // Geometry conformance (Bounded/Connected/Convex) is decided from the
            // compiled op-array, which is only reachable through the literal walker
            // (`check_leaf_trait_conformance`). A type-level `Type::Geometry` leaf
            // (e.g. an `Option<Geometry>` ValueRef) carries no op-array here, so we
            // cannot verify it — skip rather than risk a false positive. `Type::Error`
            // is anti-cascade (a nested poison sentinel reaching this leaf).
            // `Type::TypeParam` is an unresolved generic type variable (e.g. `T` in a
            // generic structure/function that forwards a type-param-typed value to a
            // trait param). It is "unknown/unresolved", NOT "definitely
            // non-conforming" — its real conformance is decided once `T` is bound to a
            // concrete type at instantiation — so it is unverifiable at this leaf and
            // skipped. Emitting here would be a false positive (esc-4081 amend,
            // reviewer_comprehensive robustness_false_positive).
            Type::Geometry | Type::Error | Type::TypeParam(_) => {}
            // Any other leaf type (scalar, enum, point, vector, …) can never carry
            // trait bounds, so it cannot conform to a trait param. Emit the same
            // diagnostic the literal walker emits for non-struct/non-trait leaves.
            // Without this arm, a wrapper-nested trait param reached with a
            // non-literal arg — e.g. `param j : Option<DrivingJoint>` given an
            // `Option<Real>` ValueRef — would silently lose the previously-existing
            // "no matching overload" hard error (task-4081 soundness regression,
            // esc-4081-174). The bare (unwrapped) trait-param case is already
            // covered by the literal walker's own fallback arm.
            _ => {
                ctx.diagnostics.push(
                    diag_at(
                        ctx.severity,
                        format!(
                            "type '{}' does not conform to trait '{}' required by param '{}'",
                            arg_ty, required_trait, ctx.arg_name
                        ),
                    )
                    .with_code(DiagnosticCode::TypeNotConformingToTrait)
                    .with_label(DiagnosticLabel::new(
                        ctx.span,
                        format!("expected a type conforming to trait '{}'", required_trait),
                    )),
                );
            }
        },
        // Leaf: param type is a StructureRef (nominal structure type, task-4584).
        // Skip/gate logic is in `reject_if_incompatible` (Error anti-cascade,
        // TypeParam unresolved, Geometry unverifiable, TraitObject may resolve).
        (Type::StructureRef(_), arg_ty) => {
            reject_if_incompatible(param_type, arg_ty, ctx, emit_structure_ref_mismatch);
        }
        // Leaf: param type is a Vector (task-4622). SHAPE-BASED with an arity check
        // and, since task 5766, a QUANTITY-SLOT check ([`emit_if_quantity_conflict`];
        // normative statement in `crates/reify-core/src/ty.rs`).
        // For `Type::Vector` args, additionally
        // require matching arity (n): `vec2` is NOT a valid substitute for a `vec3` param.
        // `Type::Tensor { rank: 1, .. }` args are accepted regardless of element count
        // (Tensor arity is not yet pinned in param signatures, so accepted conservatively).
        // Reject bare scalars, strings, bools, and any other non-vector kind.
        //
        // Unverifiable arg kinds are skipped through the shared
        // [`arg_type_is_unverifiable`] gate (Error anti-cascade, TypeParam
        // unresolved-generic, Geometry, TraitObject) — the SAME function the other
        // three shape arms and `reject_if_incompatible` call, so the set cannot
        // drift between them. For all other concrete arg types, reject unless the
        // arg is vector-shaped with matching arity.
        //
        // NOT type_compatible: implicitly_converts_to has no Vector<->Vector
        // quantity-coercion arm, so type_compatible(Vector3<Length>, Vector3<Real>)
        // is FALSE — a naive type_compatible gate would falsely reject `vec3(0,0,1)`
        // (dimensionless) for a Length-quantity param (see task-4622 design decision D1).
        // Task 5766's quantity check does NOT reintroduce that hazard:
        // [`arg_quantity_slot_dimension`] returns `None` for exactly the dimensionless
        // ARG such a gate would have false-rejected, so the rule cannot fire on it.
        // Task 6159's param-side tightening does not reach this case either — it
        // fires only when the PARAM slot is the dimensionless one.
        (Type::Vector { .. }, arg_ty) if !arg_type_is_unverifiable(arg_ty) => {
            // Accept vector-shaped args; for Type::Vector args, also require matching
            // arity (n). A Tensor{rank:1} is accepted regardless of its element count.
            let is_conforming = match arg_ty {
                Type::Vector { n: arg_n, .. } => match param_type {
                    Type::Vector { n: param_n, .. } => param_n == arg_n,
                    _ => true, // unreachable: outer arm guards param_type as Type::Vector
                },
                Type::Tensor { rank: 1, .. } => true,
                _ => false,
            };
            if !is_conforming {
                emit_vector_mismatch(param_type, arg_ty, ctx);
            } else {
                // QUANTITY SLOT (task 5766, param side ruled task 6159) — rule:
                // `crates/reify-core/src/ty.rs`; applied after the family/arity check.
                emit_if_quantity_conflict(param_type, arg_ty, ctx);
            }
        }
        // Leaf: param type is a Point (task 5465, family 1). SHAPE-BASED with an
        // arity check, modelled directly on the `Type::Vector` arm above and
        // sharing its anti-cascade/unverifiable skip guard via the same
        // [`arg_type_is_unverifiable`] function.
        //
        // Accepts:
        //   • `Type::Point { n: arg_n, .. }` when `arg_n` matches the param's
        //     `n` AND, since task 5766, the two QUANTITY slots do not conflict
        //     ([`emit_if_quantity_conflict`]; normative statement in the
        //     "Point / Vector quantity-slot convention" section of
        //     `crates/reify-core/src/ty.rs`, deliberately not restated here).
        //   • Scalar-like numeric args, as the expression compiler's
        //     numeric-fallback placeholder. NOTE this does NOT cover
        //     `point3(…)` / `point2(…)`, which carry a real `Type::Point` and
        //     take the branch ABOVE (ruled in the ty.rs section named just
        //     above). What arrives here is everything [`is_numeric_placeholder_leaf`]
        //     matches: `Type::Int`, ANY `Type::Scalar { .. }` (dimensioned or
        //     not — the match is dimension-blind, so a scalar-returning call
        //     such as `abs(-5kg)` lands here too), and `Type::ScalarParam(_)`.
        //
        // The placeholder predicate is deliberately NARROW — see
        // [`is_numeric_placeholder_leaf`] (`Int | Scalar | ScalarParam`).
        // `type_compat.rs::is_scalar_like_leaf` is NOT reused: it also admits
        // `Bool`, `String`, `Enum`, `StructureRef`, `TraitObject` and
        // `Geometry`, which would make `Anchor(origin: "origin")` silent.
        //
        // WHY THE TOLERANCE LIVES INSIDE THIS ARM rather than as an arg-side
        // `CompiledExpr` skip in the shared fallback. A broad "skip any
        // FunctionCall arg whose result_type is Scalar/Int" would punch a hole
        // through the ALREADY-allowlisted families — `Widget(label:
        // some_scalar_fn())` at a `String` param would go silent. Keyed on
        // `Type::Point`, this arm structurally cannot be reached by a `String` /
        // `Bool` / `Int` / `Real` param, so that hazard is dissolved by
        // construction. It also covers the `let p = point3(…); Anchor(origin: p)`
        // shape, which an arg-side skip would MISS: only the type-level walker
        // sees the resulting `ValueRef`. That shape reaches this SAME arm by a
        // second route — a persisted `Type::Point` on the value cell rather than
        // a `FunctionCall`'s inferred `result_type` — and the rule below fires
        // through it exactly as it does for a direct call. Pinned by
        // `point3_cross_dimension_via_let_at_dimensioned_point_param_errors_arg_type_mismatch`
        // (`struct_ctor_field_conformance_tests.rs`); this claim is not carried
        // by prose alone.
        //
        // THE BOUNDED, DELIBERATE COST, at its true size: ANY scalar-family
        // arg at a Point slot stays silent. That is a bare numeric literal
        // (`Anchor(origin: 5)`), but ALSO a scalar carrying the WRONG dimension
        // (`Anchor(origin: 5kg)` at `Point3<Length>`) and a scalar-returning
        // call (`Anchor(origin: abs(-5kg))`) — all measured silent, and pinned
        // respectively by `bare_numeric_literal_at_point_param_stays_clean`,
        // `dimensioned_scalar_at_point_param_stays_clean` and
        // `scalar_returning_call_at_point_param_stays_clean`. Note `5kg` at a
        // `Scalar<Length>` slot IS rejected; the asymmetry is the cost.
        //
        // The placeholder EXCLUSION is a standing ruling, identical in kind to
        // the pre-existing `Type::Geometry` one (geometry constructors compile
        // to a dimensionless-scalar placeholder, GHR-γ).
        //
        // OPEN ITEM, still live: tightening the dimensioned legs — e.g. a
        // `FunctionCall`-shaped check, or consulting the quantity slot for a
        // concrete `Type::Scalar { .. }`. Task 5344 removed `point3(…)` from
        // this branch but did NOT empty it of `FunctionCall`-shaped or
        // dimensioned inputs, so that narrowing is as available and as
        // motivated as it was before. It must be argued from the membership set
        // above, and it cannot be done by narrowing the shared predicate, which
        // the `Matrix`/`Tensor` rank-0 accept also depends on.
        //
        // That bounded cost is UNCHANGED by task 5766's quantity rule — see the
        // `else` branch below for why.
        (Type::Point { .. }, arg_ty) if !arg_type_is_unverifiable(arg_ty) => {
            let is_conforming = match arg_ty {
                Type::Point { n: arg_n, .. } => match param_type {
                    Type::Point { n: param_n, .. } => param_n == arg_n,
                    _ => true, // unreachable: outer arm guards param_type as Type::Point
                },
                // Numeric-fallback placeholder: `Int`, ANY `Scalar { .. }`
                // (dimension-blind) or `ScalarParam(_)`. NOT `point3(…)` /
                // `point2(…)`, which match the arm above. Pinned by
                // `bare_numeric_literal_at_point_param_stays_clean` and
                // `dimensioned_scalar_at_point_param_stays_clean`.
                other => is_numeric_placeholder_leaf(other),
            };
            if !is_conforming {
                emit_arg_type_mismatch(param_type, arg_ty, ctx);
            } else {
                // QUANTITY SLOT (task 5766, param side ruled task 6159) — rule:
                // `crates/reify-core/src/ty.rs`; applied after the arity check.
                //
                // A real corpus `point3(…)` arg reaches this check via the
                // `Type::Point { .. }` branch above, carrying a genuine quantity
                // slot, and IS compared here — measured in both directions by
                // `point3_cross_dimension_at_dimensioned_point_param_warns_arg_type_mismatch`
                // (dimensions disagree ⇒ one `ArgTypeMismatch`) and
                // `point3_dimensionless_at_dimensioned_point_param_stays_clean`
                // (arg names no dimension ⇒ silent, the arg-side tolerance), both
                // in `struct_ctor_field_conformance_tests.rs`. The placeholder
                // branch above does carry no slot and stay dimension-blind, but
                // it is not where those args go.
                emit_if_quantity_conflict(param_type, arg_ty, ctx);
            }
        }
        // Leaf: param type is a Matrix or Tensor (task 5465, family 2).
        // SHAPE-BASED, sharing the other shape arms' anti-cascade/unverifiable
        // skip guard through [`arg_type_is_unverifiable`]. Handled as ONE arm
        // because `type_compat.rs` already treats the two families as
        // interconvertible.
        //
        // Accepts, each for a stated reason:
        //   • `Type::Matrix` / `Type::Tensor` — the nominal families themselves.
        //     Rule 3 (`type_compat.rs:163-179`) already makes
        //     `Tensor<2,N,Q> → Matrix<N,N,Q>` legal. Since task 5766 the FAMILY
        //     accept is followed by the shared quantity-slot check
        //     ([`emit_if_quantity_conflict`], below).
        //   • `Type::Vector` — Rules 1a/1b (`type_compat.rs:83-108`) make
        //     `Vector<N,Q>` and `Tensor<1,N,Q>` interconvertible.
        //   • `Type::List` — the idiomatic nested-list-literal spelling of a
        //     matrix, which compiles to `List<List<Real>>` — but ONLY when it
        //     bottoms out in a numeric/tensor element type. See
        //     [`list_bottoms_out_numeric`]: an unconstrained `Type::List(_)`
        //     accept also swallowed `Body(inertia: ["a", "b"])`, leaving the
        //     promoted family a hole on the very shape the accept exists for.
        //   • `Type::Int | Type::Scalar | Type::ScalarParam` — everything
        //     [`is_numeric_placeholder_leaf`] matches. ANY `Type::Scalar { .. }`
        //     arrives (dimension- AND rank-blind: a scalar-returning call such
        //     as `trace(…)` lands here too). Only a matching scalar at a rank-0
        //     `Tensor` param has a rule behind it (Rules 2a/2b,
        //     `type_compat.rs`); at a `Matrix` or rank≥1 `Tensor` param it is a
        //     bounded cost, though spec §4.9 lists "a scalar" as legal. NOT the
        //     math builtins that build a matrix, tensor or vector (`matrix`,
        //     `diag`, `identity`, `vec`, `transpose`, `inverse`, `outer`): they
        //     carry a real `Type::Matrix` / `Type::Tensor` / `Type::Vector` from
        //     `math_fn_result_type`, take the nominal branch above and are
        //     quantity-checked, pinned by
        //     `matrix_builtin_cross_dimension_at_inertia_param_errors_arg_type_mismatch`.
        //     One pin per leg here:
        //     `bare_numeric_literal_at_matrix_param_stays_clean`,
        //     `dimensioned_scalar_at_matrix_param_stays_clean`,
        //     `matching_scalar_at_rank0_tensor_param_stays_clean` and
        //     `scalar_param_at_matrix_param_stays_clean`.
        //
        // Everything else (`String`, `Bool`, `Selector`/`AnySelector`, `Enum`,
        // `Applied`, `StructureRef`, `Field`, `Point`, `Frame`, `Transform`,
        // `Set`, `Map`, a non-numeric `List`, …) is rejected via the shared
        // `emit_arg_type_mismatch`.
        //
        // NO ARITY CHECK, in deliberate contrast to the `Point` and `Vector`
        // arms. `List<List<Real>>` — the idiomatic spelling — carries no element
        // counts in its type, so an m/n (or rank) check would either false-reject
        // every list-literal matrix or be silently unenforceable for exactly the
        // shape that matters. Constraining the ELEMENT FAMILY (above) is the part
        // that is decidable from the type alone; arity/PSD validation for
        // `MassProperties.inertia` is the ctor-time inertia hook's job (see
        // `examples/dynamics/pendulum_idyn.ri:24-26`), not this walker's.
        //
        // That asymmetry is UNCHANGED by task 5766 and ORTHOGONAL to it: ARITY is
        // undecidable here because the idiomatic `Type::List` spelling drops the
        // element counts, whereas the QUANTITY slot is compared whenever the arg
        // happens to arrive in one of the three nominal families instead. The
        // tolerance rationale and this leg's own residuals are stated normatively
        // in `crates/reify-core/src/ty.rs`, not restated here.
        (Type::Matrix { .. } | Type::Tensor { .. }, arg_ty)
            if !arg_type_is_unverifiable(arg_ty) =>
        {
            let is_conforming = match arg_ty {
                Type::Matrix { .. } | Type::Tensor { .. } | Type::Vector { .. } => true,
                Type::List(_) => list_bottoms_out_numeric(arg_ty),
                // Scalar-family leg; inputs, reason and pins: arm comment above.
                other => is_numeric_placeholder_leaf(other),
            };
            if !is_conforming {
                emit_arg_type_mismatch(param_type, arg_ty, ctx);
            } else {
                // QUANTITY SLOT (task 5766, param side ruled task 6159) — rule:
                // `crates/reify-core/src/ty.rs`; applied after the family check. The
                // `Type::List` and `is_numeric_placeholder_leaf` branches carry no
                // slot and are left EXACTLY as they were.
                emit_if_quantity_conflict(param_type, arg_ty, ctx);
            }
        }
        // Leaf: param type is a Selector or AnySelector (task-4598).
        // Skip/gate logic is in `reject_if_incompatible`; `type_compat.rs` AnySelector
        // arms encode: AnySelector accepts any Selector(k), rejects Real/String/Int/…;
        // Selector(k) accepts only Selector(k), rejects AnySelector/others.
        (Type::Selector(_) | Type::AnySelector, arg_ty) => {
            reject_if_incompatible(param_type, arg_ty, ctx, emit_selector_mismatch);
        }
        // Leaf: param type is a Field (task 5465, family 3). SHAPE-BASED, and
        // deliberately WITHOUT a domain/codomain check.
        //
        // Both `Field` slots erase to the expression compiler's numeric
        // fallback: a legitimate arg's `result_type` is `Field<Real, Real>`
        // however the field was declared (an analytical `field def` is the
        // common case — `examples/fea_shell_channels.ri` alone produced six such
        // false warnings before this arm existed: top / mid / bottom /
        // displacement / stress / frame). Comparing the declared slots against
        // that placeholder would compare a declaration against a hole, so this
        // arm is NOT routed through `type_compatible` — the same reason the
        // `Type::Vector` arm above is shape-based (task-4622 D1).
        //
        // The skip guard is the shared [`arg_type_is_unverifiable`] function, so
        // the anti-cascade / unverifiable set cannot drift between the arms:
        // Error (anti-cascade), TypeParam (unresolved generic, decided at
        // instantiation), Geometry (unverifiable at the type level), TraitObject
        // (may resolve to a field-producing type).
        (Type::Field { .. }, arg_ty) if !arg_type_is_unverifiable(arg_ty) => {
            // Accept any field-shaped arg, plus a lambda: `Type::Function` is a
            // legitimate spelling of a field-shaped arg at a `Field` slot.
            // Everything else (String, Bool, Int, Scalar, Point, List, …) is
            // rejected through the shared `emit_arg_type_mismatch`, which already
            // emits `ArgTypeMismatch` at `ctx.severity` — no new DiagnosticCode.
            if !matches!(arg_ty, Type::Field { .. } | Type::Function { .. }) {
                emit_arg_type_mismatch(param_type, arg_ty, ctx);
            }
        }
        // Option-unwrap (implicit-Some, C1 rule 6 — task 5302 α): an `Option<T>`
        // param supplied a bare non-Option arg of type `T` is a valid `Some(arg)`.
        // Recurse on the inner param type so the arg is checked against `T`. This
        // MUST precede the wrapper-shape catch-all below: without it, a valid
        // implicit-Some arg (e.g. `Option<FaceSelector>` ← `Selector(Face)`) would
        // fall through to the wrapper-shape arm and spuriously warn — the live hole
        // this task closes. The `Option`/`Option` pair is handled by the wrapper-
        // pair arm at the top of this match, so the guard here only ever fires for a
        // non-Option arg.
        (Type::Option(inner_p), arg_ty) if !matches!(arg_ty, Type::Option(_)) => {
            walk_param_against_arg_type(inner_p, arg_ty, ctx);
        }
        // Wrapper-shape mismatch OR general concrete-leaf mismatch.
        _ => {
            if matches!(
                param_type,
                Type::List(_) | Type::Set(_) | Type::Map(_, _)
            ) {
                // Wrapper-shape mismatch: a `List/Set/Map<T>` param supplied an arg
                // whose shape doesn't match (e.g. a bare leaf, or a differently-
                // shaped wrapper). (`Option<T>` params never reach here — the
                // Option-unwrap arm above handles every non-Option arg, and the
                // wrapper-pair arm handles `Option`/`Option`.) Emitted at ctx.severity.
                ctx.diagnostics.push(
                    diag_at(
                        ctx.severity,
                        format!(
                            "type '{}' does not match wrapper shape required by param '{}' (expected '{}')",
                            arg_type, ctx.arg_name, param_type
                        ),
                    )
                    .with_code(DiagnosticCode::TypeNotConformingToTrait)
                    .with_label(DiagnosticLabel::new(
                        ctx.span,
                        format!("expected '{}', got '{}'", param_type, arg_type),
                    )),
                );
            } else if let Some(param_enum) = base_enum_name(param_type, ctx.registries.enum_defs) {
                // ENUM FAMILY (task 5465, family 4). Handled as its own branch,
                // BEFORE the allowlist, so the D1/F-Mono erasure gap is absorbed
                // without a `type_compatible` call that cannot express it.
                //
                // Under erasure (PRD §7.1: "resolved args live only in the
                // per-site substitution map, never on the persisted `Type` or
                // `Value`"), a constructed variant's `result_type` is the bare
                // `Type::Enum(name)`, while a declared annotation may be the
                // applied `Type::Applied { name, args }`. `type_compatible` has
                // no Applied-vs-Enum rule in EITHER direction, so both spellings
                // must be reconciled here.
                //
                // SYMMETRIC by construction. The first cut short-circuited on
                // `enum_payload_compatible`, which only matches a bare
                // `Type::Enum` on the SUPPLIED side (type_compat.rs:334-337).
                // Combined with `Type::Enum(_)` joining the allowlist, the
                // reverse pairing — a param declared bare `Enum("Result")`
                // supplied an arg whose `result_type` is the applied
                // `Applied { name: "Result", .. }` (a `ValueRef` to a cell
                // declared `Result<Length, String>`, or a call to a fn whose
                // return annotation is the applied form, cf.
                // `examples/m6_result_recovery.ri:30`) — fell through to
                // `type_compatible` and produced a SPURIOUS `ArgTypeMismatch`
                // that was silent before the promotion. On the
                // `check_fn_arg_conformance` path that is a hard Error, so it
                // would have failed the compile rather than merely added noise
                // (reviewer_comprehensive, robustness-false-positive).
                //
                // Mapping BOTH sides through [`base_enum_name`] — the same
                // oracle `enum_payload_compatible` itself now uses, so the
                // payload-field guard in `variant_construct.rs` and this arm
                // cannot drift — makes the tolerance direction-agnostic. It also
                // removes the double `enum_defs` scan the first cut performed
                // per `Applied` leaf (once inside `enum_payload_compatible`,
                // again inside `general_leaf_param_family_is_validated`);
                // membership is now resolved exactly once per side, and not at
                // all for the non-enum params that dominate the corpus
                // (reviewer_comprehensive, efficiency).
                //
                // STILL A TARGETED TOLERANCE, NOT A FAMILY BYPASS: only the
                // same-base-name pairing is accepted. A genuine cross-enum
                // mismatch (`Hue` ← `Outline`) or a non-enum arg
                // (`Result<…>` ← `String`) falls straight through to the shared
                // `reject_if_incompatible` gate below and is rejected exactly as
                // an allowlisted family would be. Type-ARG agreement is the
                // inference/pin passes' job, not this walker's — see
                // `enum_payload_compatible`'s doc.
                if base_enum_name(arg_type, ctx.registries.enum_defs) != Some(param_enum) {
                    reject_if_incompatible(param_type, arg_type, ctx, emit_arg_type_mismatch);
                }
            } else if general_leaf_param_family_is_validated(param_type) {
                // GENERAL CONCRETE-LEAF arm (task 5302 α): this replaces the former
                // silent `_` fall-through for the param families that
                // [`general_leaf_param_family_is_validated`] vets, routing them
                // through the shared `reject_if_incompatible` → `type_compatible`
                // gate and emitting `ArgTypeMismatch` at ctx.severity for a genuine
                // mismatch. Every other family keeps falling through silently,
                // exactly as before α — see that predicate's doc comment for the
                // per-family rationale and the sound-by-construction posture.
                //
                // D4-5 (task 5627 γ): one arg-side accept sits in front of that
                // gate — an unresolved-DIMENSION placeholder at a `Scalar` slot
                // is deferred to instantiation rather than judged. See
                // [`scalar_param_arg_defers_at_scalar_slot`] for why it is
                // narrower than [`is_numeric_placeholder_leaf`] and why it is not
                // an [`arg_type_is_unverifiable`] entry.
                if !scalar_param_arg_defers_at_scalar_slot(param_type, arg_type) {
                    reject_if_incompatible(param_type, arg_type, ctx, emit_arg_type_mismatch);
                }
            }
        }
    }
}

/// Whether `param_type` belongs to a param family whose args are safe to judge
/// with raw [`type_compatible`] at the general concrete-leaf arm of
/// [`walk_param_against_arg_type`].
///
/// # Why this is an allowlist and not a skip list
///
/// The α cut of the general-leaf arm used a NEGATIVE guard
/// (`!matches!(param_type, Type::Geometry | Type::TypeParam(_))`), which made it
/// unsound BY DEFAULT: every present and future [`Type`] variant was opted in to
/// `type_compatible` unless somebody remembered to carve it out. Five families
/// were false-rejected across 15+ shipped examples before the corpus gate in
/// `examples_smoke.rs` caught it.
///
/// The inversion here makes the arm sound BY CONSTRUCTION — a newly-added `Type`
/// variant defaults to silence, not to a false positive. Broadening the list is
/// a deliberate act that must come with corpus evidence.
///
/// # Why these families
///
/// `type_compatible` is a *call-site coercion* predicate: it presumes both sides
/// carry genuinely inferred types. `Bool`, `Int`, `String` and the WHOLE
/// `Scalar` family — dimensionless (spelled `Real` in source, and by `Display`)
/// and dimensioned alike — are the families for which that presumption holds
/// unconditionally at a struct-ctor arg position: the expression compiler infers
/// them precisely, and `type_compat.rs` has real arms for each.
///
/// The dimensioned half is admitted by task 5627 (γ). It was HELD through tasks
/// 5302/5465 pending a language-semantics ruling on whether a bare dimensionless
/// arg is legal at a dimensioned slot; the ruling landed (fix the call sites, do
/// not relax the slot) and its contract table lives in
/// `docs/prds/v0_6/dimensioned-construction-strictness.md` §0.1 / §4.1 — read it
/// there rather than here, so the two cannot drift out of lockstep.
///
/// Nothing about the dimensioned half is special-cased in this walker: it rides
/// the same `reject_if_incompatible` → `type_compatible` gate as the other
/// three. Strict `DimensionVector` equality is what `type_compatible` already
/// does (`implicitly_converts_to` has no `Scalar`-vs-`Scalar` arm past the
/// `from == to` short-circuit), and the ONE scalar relaxation it carries
/// (`type_compat.rs:232-237`, `Int` → `Scalar`) is gated on the PARAM side being
/// dimensionless — which is simultaneously why `Real ← Int` survives the
/// promotion and why `Scalar<Q> ← Int` now rejects.
///
/// # Handled by dedicated shape-based arms instead (task 5465)
///
/// `Point`, `Field`, `Matrix` and `Tensor` are NOT excluded — they are handled
/// by dedicated SHAPE-BASED arms in [`walk_param_against_arg_type`], exactly as
/// `Type::Vector` is, so none ever reaches this predicate. The
/// placeholder/erasure/missing-coercion rationale below is still why those arms
/// are shape-based rather than `type_compatible`-based:
///
/// * an analytical `field def` erases both slots to `Field<Real, Real>` whatever
///   its declaration says;
/// * a nested list literal is the idiomatic `Matrix3x3` spelling but compiles to
///   `List<List<Real>>`, for which no `List`→`Matrix` coercion arm exists.
///
/// No erasure route runs through `point3(…)` / `point2(…)`; the `Point` arm stays
/// shape-based for the reasons the arm itself states. What the arg-side tolerance
/// rests on now is ruled once, in the *Point / Vector quantity-slot convention*
/// section of `crates/reify-core/src/ty.rs`.
///
/// But "unverifiable SLOTS" is not the same as "unverifiable FAMILY", and the
/// families themselves are now checked. For the remaining placeholder families
/// this is the same class the `Type::Geometry` exclusion below and
/// [`promote_function_call_to_structure_ref`] already exist for.
///
/// # Deliberately excluded, with evidence
///
/// Contrary to what this comment claimed before task 5465, promoting those four
/// families needed NO new `type_compat.rs` coercion RULE — the shape-based arms
/// and the [`base_enum_name`]-keyed enum branch sit entirely inside this walker
/// (`base_enum_name` itself is a factoring of a predicate `type_compat.rs`
/// already had, not a new coercion). What remains excluded is excluded for its
/// own stated reason, and each
/// entry names an owner (INV-SF-5 `placeholders-owned-and-loud`,
/// `docs/legibility/design-invariants.md:126-128`, forbids a blanket escape that
/// names none):
///
/// * **`Geometry`** — geometry constructors compile to a dimensionless-scalar
///   placeholder (GHR-γ) and `type_compatible` has no `Geometry` arm at all;
///   geometry conformance is decided only through the literal walker's op-array
///   inference. Owner: #8101.
/// * **`TypeParam`** — an unresolved generic param, whose real conformance is
///   decided when the type var is bound at instantiation (mirrors the arg-side
///   `TypeParam` skip inside [`reject_if_incompatible`]).
///
/// Both were the α negative guard's explicit carve-outs; they are subsumed by
/// simple absence from the allowlist, so the redundant `matches!` is gone rather
/// than left as dead belt-and-braces.
///
/// # Still unexamined
///
/// Every other [`Type`] variant defaults to silence and has simply not been
/// looked at yet. Enumerated against the real `Type` enum
/// (`crates/reify-core/src/ty.rs:120-345`) so the list cannot quietly rot:
/// `Complex`, `Orientation`, `Frame`, `Transform`, `AffineMap`, `Range`,
/// `Plane`, `Axis`, `Direction`, `BoundingBox`, `Feature`, `Function`, `Keyed`,
/// `Union`, `Projection`, `ScalarParam`. Promoting any of them is the same
/// deliberate act the four families above went through: a per-family probe pair
/// (clean fixture + value floor) plus a green corpus gate, with no new
/// `SKIP_SET` entry. Owner: #7958 (INV-SF-5 `placeholders-owned-and-loud`).
///
/// # Enum families (task 5465, family 4) — handled by the caller, not here
///
/// `Type::Enum(_)` and enum-named `Type::Applied` are promoted, but they never
/// reach this predicate: the general concrete-leaf arm dispatches them into its
/// own [`base_enum_name`]-keyed branch first, which absorbs the D1/F-Mono
/// erasure gap SYMMETRICALLY (either side may be the bare or the applied
/// spelling) and then falls into the same shared `reject_if_incompatible` gate
/// for a residual genuine mismatch (`Hue` ← `Outline`, `Result<…>` ← `String`).
///
/// The first cut of family 4 instead listed both variants here and relied on a
/// preceding `enum_payload_compatible` short-circuit. That was one-directional
/// (it matches only a bare `Type::Enum` on the SUPPLIED side) and it scanned
/// `enum_defs` twice per applied leaf. Keeping the whole enum decision in one
/// branch fixes both — see the arm comment.
///
/// Note that `Type::Applied` was never a candidate for a BLANKET entry here
/// regardless: it also represents generic STRUCTURE refs (e.g. `Coupling<T>`,
/// see `type_arg_applied_resolution_tests.rs`), and `implicitly_converts_to` has
/// no Applied-vs-StructureRef arm, so an unconditional entry would reintroduce
/// exactly the class of false positive this allowlist exists to prevent. An
/// applied generic structure still falls through this predicate silently.
fn general_leaf_param_family_is_validated(param_type: &Type) -> bool {
    matches!(
        param_type,
        Type::Bool | Type::Int | Type::String | Type::Scalar { .. }
    )
}

/// RAII guard that pops the top entry from an in-flight Vec when dropped.
///
/// Ensures the `push`–`pop` invariant around [`infer_traits_for_expr_in_env`]
/// in [`RealizationLetEnv::lookup`] is upheld even if the inner call panics.
/// Used only within `RealizationLetEnv::lookup`.
struct InFlightGuard<'a> {
    stack: &'a RefCell<Vec<ValueCellId>>,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        self.stack.borrow_mut().pop();
    }
}

/// A `LetBindingEnv` that resolves geometry-typed `let` bindings by looking up
/// their compiled op-arrays in the parent template's `realizations` list.
///
/// For non-geometry lets (`ValueCellKind::Let` with a `default_expr`), falls
/// back to `infer_traits_for_expr_in_env` on the resolved `CompiledExpr`,
/// threading `self` through so chained `ValueRef` lets are resolved recursively.
/// For any id that doesn't match a known let or realization, returns `None`
/// (safe-default: the caller falls back to `InferredTraits::all()`).
///
/// # Geometry-let lookup
///
/// `compile_entity` Pass 2 explicitly `continue`s past geometry-typed
/// `MemberDecl::Let`, so geometry lets never enter `value_cells`. They are
/// lowered in Pass 3 to `Vec<CompiledGeometryOp>` stored on
/// `RealizationDecl.operations` with `RealizationDecl.name = Some(let_name)`.
/// `lookup` therefore scans `realizations` first (geometry path) and falls
/// back to `value_cells` (non-geometry path).
///
/// # Chained let
///
/// `compile_geometry_call` already handles geometry-let chaining at the AST
/// level: for `let g = h; let h = union(box, box)`, the realization for `g`
/// recursively compiles `h`'s init expression and emits the SAME op array
/// under `realization.name = Some("g")`. No chained-ValueRef walk is needed
/// here.
///
/// # Cycle guard
///
/// The value_cells fallback calls `infer_traits_for_expr_in_env(expr, self)`,
/// which recurses back into `lookup` for any nested `ValueRef`. Malformed or
/// forward-referencing IR can produce cycles (`let g = ValueRef(g)`).  Rather
/// than stack-overflowing, the `in_flight` visited set detects re-entry and
/// returns `None`; the caller's `unwrap_or(InferredTraits::all())` mechanism
/// safe-defaults the result.  The realization arm does NOT recurse through
/// the env (it calls `infer_traits_for_op` instead), so `in_flight` is only
/// checked on the value_cells path.
struct RealizationLetEnv<'a> {
    templates: &'a HashMap<String, &'a TopologyTemplate>,
    /// In-flight visited set used to terminate cycles in the value_cells fallback.
    ///
    /// `RefCell` is required because `LetBindingEnv::lookup` takes `&self`;
    /// changing the trait to `&mut self` would cascade to all other impls.
    /// `Vec` (not `HashSet`) because typical recursion depth is 1–4; a linear
    /// scan is faster than hash overhead at this size, and avoids a new dep.
    in_flight: RefCell<Vec<ValueCellId>>,
}

impl<'a> LetBindingEnv for RealizationLetEnv<'a> {
    fn lookup(&self, id: &ValueCellId) -> Option<InferredTraits> {
        let template = self.templates.get(id.entity.as_str())?;

        // Geometry-let path: realization with matching name.
        if let Some(real) = template
            .realizations
            .iter()
            .find(|r| r.name.as_deref() == Some(id.member.as_str()))
        {
            return Some(infer_traits_for_op(&real.operations));
        }

        // Non-geometry-let fallback: value_cells Let with a resolved default_expr.
        // Match on both entity and member for symmetry with the realization arm
        // above (entity is already narrowed by `templates.get(id.entity.as_str())`,
        // but the explicit check makes the intent self-documenting and adds a
        // cheap guard against any future refactor that reuses cells across templates).

        // Cycle guard: if `id` is already being resolved on the current call stack,
        // return None.  The caller's `unwrap_or(InferredTraits::all())` in
        // `infer_traits_for_expr_in_env` converts None into the safe default, so
        // cycles in malformed or forward-referencing IR yield `all()` rather than
        // a stack overflow.
        if self.in_flight.borrow().iter().any(|p| p == id) {
            return None;
        }

        if let Some(cell) = template.value_cells.iter().find(|vc| {
            vc.id.entity == id.entity
                && vc.id.member == id.member
                && matches!(vc.kind, ValueCellKind::Let)
                && vc.default_expr.is_some()
        }) {
            // Thread the env through the resolved CompiledExpr so that any
            // ValueRef nested inside a non-geometry let (e.g. a chained let
            // whose default_expr references a geometry binding) is resolved via
            // the env rather than silently safe-defaulting to all(). The
            // in-flight guard above terminates any cycle in well-formed or
            // malformed IR before a stack overflow can occur.
            let expr = cell.default_expr.as_ref()?;
            self.in_flight.borrow_mut().push(id.clone());
            let _guard = InFlightGuard {
                stack: &self.in_flight,
            };
            let result = infer_traits_for_expr_in_env(expr, self);
            return Some(result);
        }

        None
    }
}

/// Leaf conformance check: verify that `compiled_arg` conforms to `required_trait`.
///
/// Derives `arg_call_name` from `compiled_arg.kind` so that any nested
/// `FunctionCall` discovered by the walk re-uses the existing `Real|Int →
/// StructureRef` promotion.
///
/// Conformance strategy:
/// - `Type::StructureRef` args: uses `satisfies_trait_bound` to walk the structure's declared
///   trait bounds, following refinement chains transitively (e.g. `Rigid : Physical : Material`
///   satisfies a `Material` param).
/// - `Type::TraitObject` args: uses `trait_satisfies` to check equality-or-refinement between
///   the arg trait and the required trait.
///
/// Emits at most one diagnostic per call.
fn check_leaf_trait_conformance(
    required_trait: &str,
    compiled_arg: &CompiledExpr,
    ctx: &mut WalkCtx<'_>,
) {
    let arg_type = &compiled_arg.result_type;

    // Derive arg_call_name using the shared helper so both the promotion check
    // (promote_function_call_to_structure_ref) and this suppression guard use the
    // same source of truth for "is this arg a function call?".
    let arg_call_name = extract_function_call_name(compiled_arg);

    // When the compiled arg_type defaulted to a numeric fallback (Real or Int)
    // from a FunctionCall expression and the callee is a known structure
    // template, promote to StructureRef so the conformance check can walk the
    // structure's trait bounds. Int appears when the callee's first arg is a
    // whole-number literal (e.g. `Steel(density: 1000.0)` — the literal 1000.0
    // is canonicalized to Int by the expression compiler).
    let promoted = promote_function_call_to_structure_ref(compiled_arg, ctx.registries.templates);
    let effective_arg_type = promoted.as_ref().unwrap_or(arg_type);

    // Geometry args at compile-inferred trait slots (`Bounded`/`Connected`/`Convex`):
    // route to the per-op inference table instead of the StructureRef/TraitObject
    // walker. The set of compile-inferred geometry traits is closed by the trait
    // markers in `crates/reify-compiler/stdlib/geometry_traits.ri` (task 2297) and
    // mirrored by the [`GeometryTrait`] enum in
    // `crates/reify-compiler/src/geometry_traits_inference.rs`.
    //
    // Per design decision §2 (see plan): missing `Bounded` emits the dedicated
    // [`DiagnosticCode::GeometryUnbounded`] code via [`emit_geometry_unbounded`];
    // missing `Connected`/`Convex` reuse the existing
    // [`DiagnosticCode::TypeNotConformingToTrait`] code (the PRD only allocates
    // `E_GEOMETRY_UNBOUNDED` for the Bounded case).
    //
    // The arg is treated as geometry-typed when **either**:
    //   1. `result_type == Type::Geometry` (a value-ref to a `let g = box(...)`
    //      previously registered as `Type::Geometry`), or
    //   2. it is a `FunctionCall` whose callee is in
    //      [`is_geometry_function`] — `box(...)` etc. compile to a
    //      `Type::dimensionless_scalar()` *placeholder* in the expression
    //      compiler (see `expr.rs:782`); without this fallback the placeholder
    //      Scalar would route to the generic-cascade arm.
    let is_geometry_arg = matches!(effective_arg_type, Type::Geometry)
        || extract_function_call_name(compiled_arg)
            .map(is_geometry_function)
            .unwrap_or(false);
    if is_geometry_arg {
        let geom_trait = match required_trait {
            "Bounded" => Some(GeometryTrait::Bounded),
            "Connected" => Some(GeometryTrait::Connected),
            "Convex" => Some(GeometryTrait::Convex),
            _ => None,
        };
        if let Some(trait_kind) = geom_trait {
            let env = RealizationLetEnv {
                templates: ctx.registries.templates,
                in_flight: RefCell::new(Vec::new()),
            };
            let inferred = infer_traits_for_expr_in_env(compiled_arg, &env);
            if !inferred.has(trait_kind) {
                // CARVE-OUT (task 5302 α): these two geometry-trait emit sites
                // deliberately stay `Severity::Error` — they do NOT read
                // `ctx.severity`. `GeometryUnbounded` and the geometry
                // `TypeNotConformingToTrait` belong to the geometry-primitive-
                // constructors PRD, not to this task's ctor-field knob, so they
                // are excluded from α's Warning downgrade set and the δ one-const
                // flip will NOT promote them. A future author who wants
                // geometry-trait ctor diagnostics governed by the knob must thread
                // `ctx.severity` through these helpers (see the
                // `CTOR_FIELD_CONFORMANCE_SEVERITY` / `WalkCtx.severity` docs).
                if matches!(trait_kind, GeometryTrait::Bounded) {
                    emit_geometry_unbounded(ctx.arg_name, ctx.span, ctx.diagnostics);
                } else {
                    emit_geometry_trait_violation(
                        ctx.arg_name,
                        required_trait,
                        ctx.span,
                        ctx.diagnostics,
                    );
                }
            }
            return;
        }
        // Other required traits against a geometry arg fall through to the
        // existing anti-cascade arm below.
    }

    // Check conformance based on effective_arg_type.
    // StructureRef and TraitObject are handled by the shared helper; non-struct/non-trait
    // types fall to the anti-cascade arm below.
    match effective_arg_type {
        Type::StructureRef(_) | Type::TraitObject(_) => {
            emit_leaf_conformance_for_arg_type(effective_arg_type, required_trait, ctx);
        }
        // Unresolved generic type variable — unverifiable at this leaf, mirroring the
        // type-level walker's `TypeParam` skip. A `T`-typed arg forwarded to a trait
        // param (e.g. `structure W<T> { let z = couple(inner) }` with `inner : T`) is
        // "unknown", not "definitely non-conforming"; its conformance is decided when
        // `T` is bound at instantiation. Skipping avoids a false positive (esc-4081
        // amend, reviewer_comprehensive robustness_false_positive).
        Type::TypeParam(_) => {}
        _ => {
            // Anti-cascade: when arg_type is a dimensionless-scalar/Int fallback
            // and arg_call_name refers to an UNKNOWN callee (not in the template
            // registry, so promotion returned None, and not a known geometry
            // function), an "undefined function" diagnostic already fired.
            // Emitting "type does not conform to trait 'X'" here would be
            // misleading — the fallback type is not the author's intended type.
            // Geometry functions (box, sphere, …) return dimensionless_scalar()
            // as a compile-time placeholder and are NOT unknown; they must NOT
            // be suppressed so a non-geometry trait receives the correct error.
            if matches!(arg_type, Type::Scalar { .. } | Type::Int)
                && arg_call_name.is_some_and(|n| !is_geometry_function(n))
            {
                return;
            }
            // Neither StructureRef nor TraitObject — cannot conform to a trait.
            // The original arg_type is used in the message (not the effective type,
            // which equals arg_type here since promotion didn't apply).
            ctx.diagnostics.push(
                diag_at(
                    ctx.severity,
                    format!(
                        "type '{}' does not conform to trait '{}' required by param '{}'",
                        arg_type, required_trait, ctx.arg_name
                    ),
                )
                .with_code(DiagnosticCode::TypeNotConformingToTrait)
                .with_label(DiagnosticLabel::new(
                    ctx.span,
                    format!("expected a type conforming to trait '{}'", required_trait),
                )),
            );
        }
    }
}

// ── Mechanism DrivingJoint bound check (task 4310 — mechanism γ) ─────────────

/// Resolve the nominal joint-structure type of a compiled argument expression.
///
/// **Path A** — `arg.result_type == Type::StructureRef(name)`: returns the name
/// directly. Covers structure-constructor `Coupling()` (once β lands), typed
/// `let` bindings annotated with a joint type, and any future site where the
/// compiler assigns `StructureRef` to a joint expression.
///
/// **Path B** — `arg` is a `CompiledExprKind::FunctionCall` whose callee is a
/// known joint-constructor builtin: maps the lowercase builtin name to the
/// PascalCase nominal type string.
///   couple / gear / screw / rack_and_pinion → "Coupling"
///   fixed                                   → "Fixed"
///   prismatic / revolute / cylindrical /
///   planar / spherical                      → their PascalCase kind names
///
/// Returns `None` for under-typed args (Real, Int, ValueRef without StructureRef
/// result type) and for unknown function names.  Callers skip the check in this
/// case; the α runtime guard backstops these sites (PRD D2).
fn resolve_joint_nominal_type(arg: &CompiledExpr) -> Option<String> {
    // Path A: result_type carries a nominal structure tag.
    //
    // Two variants (task #4605 ε, PRD §4.1):
    //   StructureRef(name)         — e.g. from prismatic(), revolute(), or a
    //                                 literal Coupling joint in a relate block.
    //   Applied { name, .. }       — e.g. from couple(j, ratio) after the
    //                                 couple→Applied change; phantom args are
    //                                 name-ignored (name-only extraction, PRD §4.1).
    //
    // A let-bound coupling (`let c = couple(j, ratio)`) has result_type
    // Applied{"Coupling",[StructureRef("Prismatic")]}; Path B (FunctionCall
    // name-map) does NOT match a ValueRef, so without this Applied arm the L2
    // E_MECHANISM_NONDRIVING_JOINT for `bind(c, v)` would be silently dropped,
    // regressing mechanism_nondriving_joint_diag_e2e.rs.
    match &arg.result_type {
        Type::StructureRef(name) => return Some(name.clone()),
        Type::Applied { name, .. } => return Some(name.clone()),
        _ => {}
    }
    // Path B: arg is a FunctionCall to a known joint-constructor builtin.
    //
    // DRIFT NOTE: this name→PascalCase mapping must stay in sync with the
    // joint-constructor builtins and JOINT_KINDS in reify-stdlib/src/joints.rs
    // (pub(crate), not directly importable here).  The unit test
    // `l2_joint_constructor_name_mapping_exhaustive` (conformance/mod.rs mod tests)
    // pins every arm; adding a new joint kind requires updating BOTH this match
    // AND that test.
    if let CompiledExprKind::FunctionCall { function, .. } = &arg.kind {
        let type_name = match function.name.as_str() {
            "couple" | "gear" | "screw" | "rack_and_pinion" => "Coupling",
            "fixed" => "Fixed",
            "prismatic" => "Prismatic",
            "revolute" => "Revolute",
            "cylindrical" => "Cylindrical",
            "planar" => "Planar",
            "spherical" => "Spherical",
            _ => return None,
        };
        return Some(type_name.to_owned());
    }
    None
}

/// Walk a `CompiledExpr` tree and emit
/// [`DiagnosticCode::MechanismNonDrivingJoint`] for each eval-builtin
/// mechanism call whose joint argument resolves to a known non-driving joint type.
///
/// ## Covered builtins (task γ step-8)
///
/// | Builtin | Arity condition | Joint arg index |
/// |---------|-----------------|-----------------|
/// | `bind`  | any             | 0               |
/// | `dim`   | any             | 0               |
/// | `sweep` | arity == 4      | 1               |
///
/// The arity-4 guard on `sweep` disambiguates the kinematic
/// `sweep(mechanism, joint, range, steps)` from the geometry
/// `sweep(profile, path)` (arity 2).  The geometry form is not a
/// mechanism builtin and must not be checked.
///
/// ## Joint-type resolution
///
/// Uses [`resolve_joint_nominal_type`]: Path A (`result_type == StructureRef`)
/// first, then Path B (FunctionCall to a known joint-constructor builtin).
/// Under-typed args (Real, Int, etc. with unknown callee) are silently skipped
/// — the α runtime guard backstops those sites (PRD D2).
///
/// ## Trait-bound check
///
/// Looks up the resolved structure name in `template_registry`.  If the
/// structure's `trait_bounds` do NOT satisfy `DrivingJoint` (checked via
/// [`satisfies_trait_bound`], which walks the refinement chain transitively),
/// one `MechanismNonDrivingJoint` diagnostic is emitted naming the type.
///
/// Coupling (trait_bounds = ["Joint"], NOT "DrivingJoint") → rejected.
/// Prismatic/Revolute/etc. (trait_bounds = ["DrivingJoint"]) → accepted.
///
/// Crucially, the check is **gated on Joint conformance**: if the resolved
/// structure does NOT satisfy `Joint` (i.e. it is not a joint type at all,
/// e.g. someone writes `bind(someBox, v)`), the function silently skips it
/// rather than emitting a misleading "joint type 'Box' does not satisfy
/// DrivingJoint" message.  A future generic type-mismatch pass will handle
/// that case.
///
/// ## L1/L2 disjoint invariant (PRD D2, "one code two sites")
///
/// [`DiagnosticCode::MechanismNonDrivingJoint`] has two emission sites:
/// - **L1 (eval):** `detect_nondriving_joint_errors` in `engine_eval.rs`
/// - **L2 (compile):** this function (task γ)
///
/// Double-emission is not a problem in the `reify check` CLI: `cmd_check`
/// (reify-cli/src/main.rs) returns early on any compile-time
/// `Severity::Error` diagnostic **before** invoking `engine.check()` (eval).
/// So: L2 fires → CLI exits → L1 never runs for the same expression.
/// The two sites are complementary (L2 for direct-constructor / nominal-typed
/// args; L1 for dynamically-determined joint values), not cumulative.
///
/// ## Diagnostic span
///
/// Uses the `representative_span` threaded by [`phase_fn_arg_conformance`]
/// from the enclosing field / constraint / realization.
pub(crate) fn check_expr_mechanism_joint_bound(
    expr: &CompiledExpr,
    template_registry: &HashMap<String, &TopologyTemplate>,
    trait_registry: &HashMap<String, &CompiledTrait>,
    span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    expr.walk(&mut |node: &CompiledExpr| {
        let CompiledExprKind::FunctionCall { function, args } = &node.kind else {
            return;
        };
        // Determine which argument position carries the joint.
        // bind/dim: arg0.  sweep@arity4 (kinematic): arg1.
        // Arity-2 sweep = geometry (CSG) — skip.
        let joint_arg_idx: usize = match function.name.as_str() {
            "bind" | "dim" => 0,
            "sweep" if args.len() == 4 => 1,
            _ => return,
        };
        let Some(joint_arg) = args.get(joint_arg_idx) else {
            return; // Arity too low — skip (malformed / already-diagnosed).
        };
        // Resolve the nominal joint type.
        let Some(type_name) = resolve_joint_nominal_type(joint_arg) else {
            return; // Under-typed / unknown — skip (D2: α runtime backstops).
        };
        // Look up the structure in the template registry.
        let Some(tmpl) = template_registry.get(type_name.as_str()) else {
            return; // Not a known structure in this scope — skip.
        };
        // Gate on Joint conformance: only emit MechanismNonDrivingJoint when the
        // structure actually conforms to Joint.  A non-joint StructureRef (e.g.
        // `bind(someBox, v)`) resolved via Path A would otherwise produce a
        // misleading "joint type 'Box' does not satisfy DrivingJoint" message when
        // Box is not a joint at all.  Skipping here keeps the diagnostic precise:
        // "you passed a Joint that is not a DrivingJoint."  The real type-mismatch
        // error (non-joint arg to bind/dim/sweep) is a separate concern for a
        // future generic type-check pass.
        if !satisfies_trait_bound(&tmpl.trait_bounds, "Joint", trait_registry) {
            return; // Not a Joint at all — skip; type-mismatch diagnostics handle it.
        }
        // Check DrivingJoint conformance (transitive through refinements).
        if !satisfies_trait_bound(&tmpl.trait_bounds, "DrivingJoint", trait_registry) {
            diagnostics.push(
                Diagnostic::error(format!(
                    "joint type '{}' does not satisfy DrivingJoint: \
                     bind/dim/sweep require an independent motion variable; \
                     Coupling (derived motion) and Fixed (0-DOF) cannot be driven",
                    type_name
                ))
                .with_code(DiagnosticCode::MechanismNonDrivingJoint)
                .with_label(DiagnosticLabel::new(
                    span,
                    format!("'{}' is not a DrivingJoint", type_name),
                )),
            );
        }
    });
}

#[cfg(test)]
/// Trait-conformance cases go through `check_trait_conformance` (inline .ri via `conform_source`); rationale: docs/prds/v0_6/compiler-declaration-passes.md §2.4 / B10.
mod tests {
    use super::*;

    /// Everything one `check_trait_conformance` call produced for its conformer.
    ///
    /// `injected_cells` and `injected_constraints` are the call's value-cell and
    /// constraint out-params. They start empty, so they hold ONLY what conformance
    /// injected from trait defaults — never the conformer's own members.
    struct ConformanceRun {
        diagnostics: Vec<Diagnostic>,
        injected_cells: Vec<ValueCellDecl>,
        injected_constraints: Vec<CompiledConstraint>,
        assoc_fns: Vec<CompiledAssocFn>,
    }

    /// Run `check_trait_conformance` for `structure_def` against `traits`.
    ///
    /// The ONE in-test call site of `check_trait_conformance`: [`run_conformance`] and
    /// [`conform_source`] both funnel through here, so the scaffolding (scope,
    /// out-params, registries, the 16-argument call itself) is written once and a
    /// future signature change is edited once.
    fn conform(
        traits: &[CompiledTrait],
        structure_def: &reify_ast::StructureDef,
        enum_defs: &[reify_ir::EnumDef],
        structure_names: &HashSet<String>,
        trait_names: &HashSet<String>,
    ) -> ConformanceRun {
        let entity_ref = EntityDefRef::from(structure_def);
        let trait_registry: HashMap<String, &CompiledTrait> =
            traits.iter().map(|t| (t.name.clone(), t)).collect();
        let mut scope = CompilationScope::new(&structure_def.name);
        let mut value_cells: Vec<ValueCellDecl> = vec![];
        let mut constraints: Vec<CompiledConstraint> = vec![];
        let mut constraint_index = 0u32;
        let functions: &[CompiledFunction] = &[];
        let alias_registry = TypeAliasRegistry::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        let mut assoc_fns: Vec<CompiledAssocFn> = vec![];
        let mut assoc_types: Vec<CompiledAssocType> = vec![];

        check_trait_conformance(
            &entity_ref,
            &trait_registry,
            structure_names,
            trait_names,
            &mut scope,
            &mut value_cells,
            &mut constraints,
            &mut constraint_index,
            enum_defs,
            functions,
            &alias_registry,
            // task 4497: empty table — these conformance unit tests exercise
            // trait-default behavior, not ambient injection.
            &AmbientDefaults::default(),
            // task 4639: file scope (no enclosing purpose) for unit tests.
            None,
            &mut diagnostics,
            &mut assoc_fns,
            &mut assoc_types,
        );

        ConformanceRun {
            diagnostics,
            injected_cells: value_cells,
            injected_constraints: constraints,
            assoc_fns,
        }
    }

    /// Run `check_trait_conformance` for a hand-built `structure_def` against `traits`,
    /// with no structure names in scope and `traits` as the only trait names.
    ///
    /// For fixtures that need a hand-built AST or hand-compiled trait; reach for
    /// [`conform_source`] when the fixture can be written as `.ri` source.
    fn run_conformance(
        traits: &[CompiledTrait],
        structure_def: &reify_ast::StructureDef,
        enum_defs: &[reify_ir::EnumDef],
    ) -> ConformanceRun {
        let trait_names: HashSet<String> = traits.iter().map(|t| t.name.clone()).collect();
        conform(
            traits,
            structure_def,
            enum_defs,
            &HashSet::new(),
            &trait_names,
        )
    }

    /// Parse `source`, compile its traits with the same `compile_trait` call
    /// `traits_phase` makes (minus its refinement-deprecation pass), and run
    /// `check_trait_conformance` for its one conformer.
    ///
    /// Declarations are scanned by `collect_decl_refs` and the trait and structure name
    /// sets come from `build_resolution_names`: the two steps a full compile runs before
    /// `phase_traits`. So the fixture's enums arrive with their declared variants and type
    /// parameters, and its names resolve as they do in a real compile. Variant payload
    /// fields stay unresolved (`enums_phase` resolves them later in a full compile), so
    /// keep fixtures to unit variants. The world is otherwise one module with no prelude:
    /// only builtin types such as `Real` and `Bool` and the fixture's own declarations
    /// resolve.
    ///
    /// The conformer is the single `structure def` that declares trait bounds;
    /// `source` must contain exactly one. Diagnostics from compiling the traits come
    /// first, then those from conformance — the same two emitters a full compile
    /// runs, without the duplicates `entity.rs` re-emits, so exact diagnostic counts
    /// stay meaningful.
    ///
    /// Fixture conventions:
    /// - Trait bounds are `+`-separated (`structure def S : A + B {}`); the comma
    ///   form is a parse error.
    /// - A trait sub requirement carries parens (`sub mount = Hole()`); without them
    ///   the member is silently dropped from the trait body.
    fn conform_source(source: &str) -> ConformanceRun {
        conform_source_editing_ast(source, |_| {})
    }

    /// [`conform_source`] for a fixture whose every `constraint`, in the traits and in
    /// the conformer alike, carries `label`.
    ///
    /// The grammar has no constraint-label syntax yet, so the fixture is written
    /// unlabeled and the label is set on the parsed AST before the traits compile.
    fn conform_source_with_labeled_constraints(source: &str, label: &str) -> ConformanceRun {
        conform_source_editing_ast(source, |parsed| {
            for declaration in &mut parsed.declarations {
                let members = match declaration {
                    reify_ast::Declaration::Trait(trait_decl) => &mut trait_decl.members,
                    reify_ast::Declaration::Structure(structure) => &mut structure.members,
                    _ => continue,
                };
                for member in members {
                    if let reify_ast::MemberDecl::Constraint(constraint) = member {
                        constraint.label = Some(label.to_string());
                    }
                }
            }
        })
    }

    /// [`conform_source`] with `edit_ast` applied to the parsed module before its
    /// declarations are scanned: for fixtures the grammar cannot spell.
    fn conform_source_editing_ast(
        source: &str,
        edit_ast: impl FnOnce(&mut reify_ast::ParsedModule),
    ) -> ConformanceRun {
        let mut parsed =
            reify_syntax::parse(source, reify_core::ModulePath::single("conformance_test"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors in conformance fixture: {:?}\n{source}",
            parsed.errors
        );
        edit_ast(&mut parsed);

        let mut scan_ctx = crate::compile_builder::ctx::CompilationCtx::new();
        let decl_refs = crate::compile_builder::pre_pass::collect_decl_refs(&mut scan_ctx, &parsed);
        assert!(
            scan_ctx.diagnostics.is_empty(),
            "declaration errors in conformance fixture: {:?}\n{source}",
            scan_ctx.diagnostics
        );
        crate::compile_builder::names_phase::build_resolution_names(
            &mut scan_ctx,
            &[],
            &decl_refs.trait_refs,
            &decl_refs.fn_refs,
        );
        let enum_defs = scan_ctx.enum_defs;
        let trait_names = scan_ctx.resolution_trait_names;
        let structure_names = scan_ctx.resolution_structure_names;
        let alias_registry = TypeAliasRegistry::new();

        let mut trait_diagnostics: Vec<Diagnostic> = vec![];
        let traits: Vec<CompiledTrait> = decl_refs
            .trait_refs
            .iter()
            .map(|t| {
                crate::traits::compile_trait(
                    t,
                    &enum_defs,
                    &alias_registry,
                    &structure_names,
                    &trait_names,
                    &mut trait_diagnostics,
                )
            })
            .collect();

        let mut conformers = decl_refs
            .structure_refs
            .iter()
            .filter(|s| !s.trait_bounds.is_empty());
        let conformer = conformers
            .next()
            .expect("conformance fixture must declare a structure with trait bounds");
        assert!(
            conformers.next().is_none(),
            "conformance fixture must declare exactly one structure with trait bounds:\n{source}"
        );

        let mut run = conform(
            &traits,
            conformer,
            &enum_defs,
            &structure_names,
            &trait_names,
        );
        run.diagnostics.splice(0..0, trait_diagnostics);
        run
    }

    /// The cells conformance injected for `member`, in injection order.
    fn injected_named<'a>(run: &'a ConformanceRun, member: &str) -> Vec<&'a ValueCellDecl> {
        run.injected_cells
            .iter()
            .filter(|cell| cell.id.member == member)
            .collect()
    }

    /// The one diagnostic `run` emitted; fails if it emitted any other number.
    fn sole_diagnostic(run: &ConformanceRun) -> &Diagnostic {
        assert_eq!(
            run.diagnostics.len(),
            1,
            "expected exactly one diagnostic; got: {:?}",
            run.diagnostics
        );
        &run.diagnostics[0]
    }

    /// Assert that `run` emitted exactly one diagnostic, carrying `code`, whose message
    /// contains every one of `fragments`.
    fn assert_sole_diagnostic(run: &ConformanceRun, code: DiagnosticCode, fragments: &[&str]) {
        let diagnostic = sole_diagnostic(run);
        assert_eq!(
            diagnostic.code,
            Some(code),
            "wrong diagnostic code for: {}",
            diagnostic.message
        );
        for fragment in fragments {
            assert!(
                diagnostic.message.contains(fragment),
                "message should contain {fragment:?}; got: {}",
                diagnostic.message
            );
        }
    }

    // --- B10: how same-name trait defaults collide (PRD
    // docs/prds/v0_6/compiler-declaration-passes.md §6) ---
    //
    // Characterization pins. Each fixes the two sets a restructure of the
    // conformance phases must reproduce: the ADVERTISED set (which defaults can
    // satisfy a `param` requirement) and the INJECTED set (the cells conformance
    // adds to the conformer).
    //
    // Only Param-keyed advertisements are pinned. A Let-keyed one is read solely by a
    // `let` requirement (a trait `let` with no value), which the parser cannot express,
    // so it is unobservable from source and a restructure need not reproduce it.

    /// Pass 1, annotated-`let` loser: `A`'s `param x` claims the scope slot first, so
    /// `B`'s annotated `let x` is not injected, and the Param default is what satisfies
    /// `C`'s `param x` requirement.
    #[test]
    fn annotated_trait_let_losing_to_same_name_param_is_not_injected() {
        let run = conform_source(
            r"
            trait A { param x : Real = 1.0 }
            trait B { let x : Real = 2.0 }
            trait C { param x : Real }
            structure def S : A + B + C {}
            ",
        );

        assert!(
            run.diagnostics.is_empty(),
            "C's `param x` must be satisfied by A's Param default; got: {:?}",
            run.diagnostics
        );
        let injected = injected_named(&run, "x");
        assert_eq!(
            injected.len(),
            1,
            "exactly one cell named `x` must be injected; got: {:?}",
            run.injected_cells
        );
        assert_eq!(injected[0].kind, ValueCellKind::Param);
        assert_eq!(injected[0].cell_type, Type::dimensionless_scalar());
        assert!(
            run.injected_constraints.is_empty(),
            "no trait declares a constraint; got: {:?}",
            run.injected_constraints
        );
    }

    /// Pass 1, `param` loser: `B`'s annotated `let x` is bound before `A`'s `param x`,
    /// so the Let wins the slot. The Param is then neither injected nor advertised,
    /// which leaves `C`'s `param x` requirement unsatisfied.
    #[test]
    fn param_losing_to_same_name_annotated_trait_let_is_neither_injected_nor_advertised() {
        let run = conform_source(
            r"
            trait A { param x : Real = 1.0 }
            trait B { let x : Real = 2.0 }
            trait C { param x : Real }
            structure def S : B + A + C {}
            ",
        );

        assert_sole_diagnostic(&run, DiagnosticCode::MissingRequiredMember, &["'x'"]);
        let injected = injected_named(&run, "x");
        assert_eq!(
            injected.len(),
            1,
            "exactly one cell named `x` must be injected; got: {:?}",
            run.injected_cells
        );
        assert_eq!(injected[0].kind, ValueCellKind::Let);
        assert!(
            run.injected_constraints.is_empty(),
            "no trait declares a constraint; got: {:?}",
            run.injected_constraints
        );

        let control = conform_source(
            r"
            trait A { param x : Real = 1.0 }
            trait C { param x : Real }
            structure def S : A + C {}
            ",
        );
        assert!(
            control.diagnostics.is_empty(),
            "without the Let trait, A's default alone must satisfy C; got: {:?}",
            control.diagnostics
        );
    }

    /// Pass 2, unannotated-`let` loser whose slot was claimed in Pass 1: `A`'s
    /// `param x` owns the scope slot, so `B`'s unannotated `let x` is dropped without
    /// being injected — and without the "not cached" internal error that would flag the
    /// Pass-2 and injection guards drifting apart. Bound order must not matter, because
    /// Pass 1 runs over every default before Pass 2 does. The Bool `let` makes a
    /// wrongly injected Let visibly distinct from the Real `param`.
    #[test]
    fn unannotated_trait_let_losing_to_same_name_param_is_not_injected() {
        let traits = r"
            trait A { param x : Real = 1.0 }
            trait B { let x = true }
        ";
        for bounds in ["A + B", "B + A"] {
            let run = conform_source(&format!("{traits} structure def S : {bounds} {{}}"));

            assert!(
                run.diagnostics.is_empty(),
                "`S : {bounds}`: expected no diagnostics; got: {:?}",
                run.diagnostics
            );
            let injected = injected_named(&run, "x");
            assert_eq!(
                injected.len(),
                1,
                "`S : {bounds}`: exactly one cell named `x` must be injected; got: {:?}",
                run.injected_cells
            );
            assert_eq!(injected[0].kind, ValueCellKind::Param, "`S : {bounds}`");
        }
    }

    /// Pass 2, unannotated-`let` loser whose name the conformer itself declares: `S`'s
    /// own `param x` overrides `B`'s unannotated `let x`, so conformance injects nothing
    /// for it. The `param` uses Bool, the let's inferred type, so the conformer-collision
    /// rule has nothing to object to.
    #[test]
    fn structure_member_overriding_unannotated_trait_let_injects_nothing() {
        let run = conform_source(
            r"
            trait B { let x = true }
            structure def S : B { param x : Bool = true }
            ",
        );

        assert!(
            run.diagnostics.is_empty(),
            "expected no diagnostics; got: {:?}",
            run.diagnostics
        );
        assert!(
            injected_named(&run, "x").is_empty(),
            "the conformer's own `x` must win; got injected: {:?}",
            run.injected_cells
        );
    }

    /// Pass 2, compile error: `let a = b` fails on the unresolved `b`. The root cause is
    /// reported exactly once and `a` is not injected. `a` is also poisoned in scope, so
    /// the dependent `let c = a` compiles silently against the poison instead of
    /// cascading into a second "unresolved name" — which is how `c` still gets injected.
    #[test]
    fn trait_let_with_compile_error_reports_root_cause_once_and_is_not_injected() {
        let run = conform_source(
            r"
            trait T {
                let a = b
                let c = a
            }
            structure def S : T {}
            ",
        );

        let errors: Vec<_> = run
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert_eq!(
            errors.len(),
            1,
            "expected only the root-cause error, with no cascade for `c`; got: {:?}",
            run.diagnostics
        );
        assert_eq!(errors[0].code, Some(DiagnosticCode::UnresolvedName));
        assert!(
            errors[0].message.contains("unresolved name: b"),
            "the root cause is the unresolved `b`; got: {}",
            errors[0].message
        );
        assert!(
            injected_named(&run, "a").is_empty(),
            "`a` failed to compile and must not be injected; got: {:?}",
            run.injected_cells
        );
        let injected_c = injected_named(&run, "c");
        assert_eq!(
            injected_c.len(),
            1,
            "`c` compiles against the poisoned `a` and is injected once; got: {:?}",
            run.injected_cells
        );
        assert_eq!(injected_c[0].kind, ValueCellKind::Let);
    }

    // --- Requirement checks and member annotations: what a conformer must provide,
    // and how its own annotations resolve (inline `.ri` through `conform_source`;
    // each asserts the EXACT diagnostic count, which the narrow harness keeps
    // meaningful) ---

    /// A `param` requirement that neither the structure nor any trait default provides
    /// is reported as a missing member.
    #[test]
    fn param_requirement_without_member_or_default_reports_missing_member() {
        let run = conform_source(
            r"
            trait T { param w : Real }
            structure def S : T {}
            ",
        );

        assert_sole_diagnostic(&run, DiagnosticCode::MissingRequiredMember, &["'w'"]);
    }

    /// A `let` member is a computed slot, not a settable one, so it cannot satisfy a
    /// `param` requirement. The diagnostic names the `param` slot the structure must
    /// provide; the properly provided `width` produces nothing.
    #[test]
    fn let_member_does_not_satisfy_param_requirement() {
        let run = conform_source(
            r"
            trait T {
                param width : Real
                param length : Real
            }
            structure def S : T {
                param width : Real
                let length : Real = 2.0
            }
            ",
        );

        assert_sole_diagnostic(
            &run,
            DiagnosticCode::MissingRequiredMember,
            &["'length'", "`param` slot"],
        );
    }

    /// A structure member whose type does not convert to the required type is a type
    /// mismatch, however the trait chain's defaults look.
    #[test]
    fn param_member_of_wrong_type_reports_type_mismatch() {
        let run = conform_source(
            r"
            trait T { param w : Real }
            structure def S : T { param w : Bool }
            ",
        );

        assert_sole_diagnostic(&run, DiagnosticCode::TypeMismatchForTraitMember, &["'w'"]);
    }

    /// With no member of its own, a `param` requirement can still be met by another
    /// trait's default — but only when that default's type fits. A Bool default does
    /// not satisfy a Real requirement.
    #[test]
    fn param_requirement_met_only_by_wrong_typed_default_reports_mismatch() {
        let run = conform_source(
            r"
            trait A { param w : Real }
            trait B { param w : Bool = true }
            structure def S : A + B {}
            ",
        );

        assert_sole_diagnostic(
            &run,
            DiagnosticCode::TypeMismatchForTraitMember,
            &["available default", "'w'"],
        );
    }

    /// A `sub` requirement the structure does not declare is reported by name.
    #[test]
    fn missing_required_sub_component_is_reported() {
        let run = conform_source(
            r"
            structure def Hole {}
            trait T { sub mount = Hole() }
            structure def S : T {}
            ",
        );

        assert_sole_diagnostic(
            &run,
            DiagnosticCode::MissingRequiredSubComponent,
            &["'mount'"],
        );
    }

    /// An enum-typed `param` requirement is satisfied by a structure `param` of the same
    /// enum type: the member's annotation resolves through the enum table.
    #[test]
    fn enum_typed_param_requirement_is_satisfied_by_enum_typed_member() {
        let run = conform_source(
            r"
            enum Polarity { In, Out }
            trait T { param dir : Polarity }
            structure def S : T { param dir : Polarity }
            ",
        );

        assert!(
            run.diagnostics.is_empty(),
            "expected no diagnostics; got: {:?}",
            run.diagnostics
        );
    }

    /// A conformer `param` that gives a non-generic enum type arguments
    /// (`Polarity<Real>`) is a user-facing error, reported once. It is a diagnostic
    /// rather than a `debug_assert!`, so release builds surface it too.
    #[test]
    fn enum_with_type_args_emits_error_diagnostic() {
        let run = conform_source(
            r"
            enum Polarity { In, Out }
            trait T {}
            structure def S : T { param dir : Polarity<Real> }
            ",
        );

        // No diagnostic code identifies this error, so its message does.
        let diagnostic = sole_diagnostic(&run);
        assert!(
            diagnostic
                .message
                .contains("enum `Polarity` does not accept type arguments"),
            "wrong diagnostic: {}",
            diagnostic.message
        );
    }

    /// An unknown type name given type arguments (`NotAnEnum<Real>`) is one unresolved
    /// type, even with an enum declared. It neither resolves silently nor draws the
    /// enum-only "does not accept type arguments" error, which needs the name to be a
    /// declared enum.
    #[test]
    fn unknown_named_type_with_type_args_produces_unresolved_diagnostic() {
        let run = conform_source(
            r"
            enum Polarity { In, Out }
            trait T {}
            structure def S : T { param p : NotAnEnum<Real> }
            ",
        );

        assert_sole_diagnostic(&run, DiagnosticCode::UnresolvedType, &["NotAnEnum"]);
    }

    // --- Default injection: what conformance adds to the conformer ---

    /// A trait's `param` default and its unlabeled constraint, which the conformer does
    /// not override, are both injected: one Param cell carrying the default expression,
    /// and one constraint compiled against that cell.
    #[test]
    fn non_overridden_param_default_and_constraint_are_injected() {
        let run = conform_source(
            r"
            trait T {
                param x : Real = 1.0
                constraint x > 0.0
            }
            structure def S : T {}
            ",
        );

        assert!(
            run.diagnostics.is_empty(),
            "expected no diagnostics; got: {:?}",
            run.diagnostics
        );
        assert_eq!(
            run.injected_cells.len(),
            1,
            "expected exactly the `x` cell; got: {:?}",
            run.injected_cells
        );
        let x = &run.injected_cells[0];
        assert_eq!(x.id.member, "x");
        assert_eq!(x.kind, ValueCellKind::Param);
        assert_eq!(x.cell_type, Type::dimensionless_scalar());
        assert!(
            x.default_expr.is_some(),
            "the trait's default expression must travel with the injected cell"
        );
        assert_eq!(
            run.injected_constraints.len(),
            1,
            "the unlabeled constraint default must be injected; got: {:?}",
            run.injected_constraints
        );
    }

    /// A conformer constraint with the same label as a trait's constraint default
    /// replaces it: the trait's is not injected. The control is
    /// [`labeled_trait_constraint_default_is_injected_when_the_conformer_lacks_that_label`].
    #[test]
    fn conformer_constraint_with_the_trait_constraints_label_suppresses_the_default() {
        let run = conform_source_with_labeled_constraints(
            r"
            trait T {
                param x : Real = 1.0
                constraint x > 0.0
            }
            structure def S : T { constraint x > 1.0 }
            ",
            "pos",
        );

        assert!(
            run.diagnostics.is_empty(),
            "expected no diagnostics; got: {:?}",
            run.diagnostics
        );
        assert!(
            run.injected_constraints.is_empty(),
            "the conformer's own `pos` constraint must stand in for the trait's; got: {:?}",
            run.injected_constraints
        );
    }

    /// A trait's labeled constraint default is injected, label intact, when the
    /// conformer declares no constraint with that label.
    #[test]
    fn labeled_trait_constraint_default_is_injected_when_the_conformer_lacks_that_label() {
        let run = conform_source_with_labeled_constraints(
            r"
            trait T {
                param x : Real = 1.0
                constraint x > 0.0
            }
            structure def S : T {}
            ",
            "pos",
        );

        assert!(
            run.diagnostics.is_empty(),
            "expected no diagnostics; got: {:?}",
            run.diagnostics
        );
        assert_eq!(
            run.injected_constraints.len(),
            1,
            "the labeled constraint default must be injected; got: {:?}",
            run.injected_constraints
        );
        assert_eq!(run.injected_constraints[0].label.as_deref(), Some("pos"));
    }

    /// A trait `param` whose default names an enum variant is injected as an enum-typed
    /// Param cell: the fixture's declared variants reach the compile of the default.
    #[test]
    fn enum_variant_param_default_is_injected_as_enum_typed_param() {
        let run = conform_source(
            r"
            enum Polarity { In, Out }
            trait T { param dir : Polarity = Polarity.In }
            structure def S : T {}
            ",
        );

        assert!(
            run.diagnostics.is_empty(),
            "expected no diagnostics; got: {:?}",
            run.diagnostics
        );
        let dir = injected_named(&run, "dir");
        assert_eq!(dir.len(), 1, "got: {:?}", run.injected_cells);
        assert_eq!(dir[0].kind, ValueCellKind::Param);
        assert_eq!(dir[0].cell_type, Type::Enum("Polarity".to_string()));
        assert!(
            dir[0].default_expr.is_some(),
            "the variant default must travel with the injected cell"
        );
    }

    /// An unannotated trait `let` is compiled against every annotated default, wherever
    /// it is declared: `positive` reads `w` before `w` is declared, and still resolves.
    /// Its injected cell carries the type inferred from the expression (Bool), not the
    /// dimensionless-scalar fallback.
    #[test]
    fn unannotated_trait_let_sees_annotated_defaults_and_is_injected_with_inferred_type() {
        let run = conform_source(
            r"
            trait T {
                let positive = w > 0.0
                param w : Real = 1.0
            }
            structure def S : T {}
            ",
        );

        assert!(
            run.diagnostics.is_empty(),
            "expected no diagnostics; got: {:?}",
            run.diagnostics
        );
        let w = injected_named(&run, "w");
        assert_eq!(w.len(), 1, "got: {:?}", run.injected_cells);
        assert_eq!(w[0].kind, ValueCellKind::Param);
        assert_eq!(w[0].cell_type, Type::dimensionless_scalar());
        let positive = injected_named(&run, "positive");
        assert_eq!(positive.len(), 1, "got: {:?}", run.injected_cells);
        assert_eq!(positive[0].kind, ValueCellKind::Let);
        assert_eq!(
            positive[0].cell_type,
            Type::Bool,
            "the injected let's type is inferred from `w > 0.0`"
        );
    }

    /// An unannotated trait `let` whose expression compiles with only a warning is
    /// still a success: the warning is reported, the cell is injected, and its type is
    /// the one the warning says it defaulted to. An empty list literal warns "cannot
    /// infer element type ... defaulting to Real".
    #[test]
    fn unannotated_trait_let_with_warning_only_compile_is_injected() {
        let run = conform_source(
            r"
            trait T { let xs = [] }
            structure def S : T {}
            ",
        );

        assert!(
            run.diagnostics
                .iter()
                .all(|d| d.severity != Severity::Error),
            "a warning-only compile must not be treated as a failure; got: {:?}",
            run.diagnostics
        );
        assert!(
            run.diagnostics
                .iter()
                .any(|d| d.severity == Severity::Warning),
            "the empty list literal must still warn; got: {:?}",
            run.diagnostics
        );
        let xs = injected_named(&run, "xs");
        assert_eq!(xs.len(), 1, "got: {:?}", run.injected_cells);
        assert_eq!(xs[0].kind, ValueCellKind::Let);
        assert_eq!(
            xs[0].cell_type,
            Type::List(Box::new(Type::dimensionless_scalar()))
        );
    }

    /// A required associated function that the conforming structure does not
    /// provide must surface an `E_TRAIT_FN_NOT_SATISFIED`
    /// (`DiagnosticCode::TraitFnNotSatisfied`) diagnostic naming the declaring
    /// trait and the missing fn.
    #[test]
    fn required_assoc_fn_not_provided_emits_trait_fn_not_satisfied() {
        // Trait `Shape` requires a bodyless `fn req(self) -> Real`.
        let shape = CompiledTrait {
            name: "Shape".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: "req".to_string(),
                kind: RequirementKind::Fn(CompiledAssocFnSig {
                    name: "req".to_string(),
                    has_self: true,
                    params: vec![],
                    return_type: Type::dimensionless_scalar(),
                }),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };

        // Structure `S : Shape { }` provides NO `fn req` member.
        let structure_def = reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_ast::TraitBoundRef {
                name: "Shape".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members: vec![],
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        };

        let diagnostics = run_conformance(&[shape], &structure_def, &[]).diagnostics;

        let fn_not_satisfied: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied))
            .collect();
        assert_eq!(
            fn_not_satisfied.len(),
            1,
            "expected exactly one TraitFnNotSatisfied diagnostic for the missing \
             required assoc fn 'req'; got: {:?}",
            diagnostics
        );
        let msg = &fn_not_satisfied[0].message;
        assert!(
            msg.contains("Shape"),
            "diagnostic should name the declaring trait 'Shape'; got: {}",
            msg
        );
        assert!(
            msg.contains("req"),
            "diagnostic should name the missing fn 'req'; got: {}",
            msg
        );
    }

    // --- assoc-fn signature exact-match fixtures (task 3939 δ) ---

    /// The implicit `self` receiver: `is_self == true` with the sentinel `self`
    /// named type (per decl.rs:818-823). Excluded from the derived signature's
    /// `params` and recorded as `has_self`.
    fn assoc_self_param() -> reify_ast::FnParam {
        reify_ast::FnParam {
            name: "self".to_string(),
            is_self: true,
            type_expr: reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: "self".to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::empty(0),
            },
            default: None,
            span: SourceSpan::empty(0),
        }
    }

    /// A non-self param `name : type_name` (bare named type).
    fn assoc_named_param(name: &str, type_name: &str) -> reify_ast::FnParam {
        reify_ast::FnParam {
            name: name.to_string(),
            is_self: false,
            type_expr: reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: type_name.to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::empty(0),
            },
            default: None,
            span: SourceSpan::empty(0),
        }
    }

    /// A structure-body `fn <name>(<params>) -> <return_type_name> { 0.0 }` member.
    /// The trivial body is irrelevant to signature matching — δ compares only the
    /// receiver / param / return signature.
    fn assoc_fn_member(
        name: &str,
        params: Vec<reify_ast::FnParam>,
        return_type_name: &str,
    ) -> reify_ast::MemberDecl {
        reify_ast::MemberDecl::Fn(reify_ast::FnDef {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            params,
            return_type: Some(reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: return_type_name.to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::empty(0),
            }),
            body: Some(reify_ast::FnBody {
                let_bindings: vec![],
                result_expr: reify_ast::Expr {
                    kind: reify_ast::ExprKind::NumberLiteral {
                        value: 0.0,
                        is_real: true,
                    },
                    span: SourceSpan::empty(0),
                },
            }),
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            annotations: vec![],
        })
    }

    /// Build trait `Shape` requiring a single bodyless `fn <fn_name>(self) -> Real`.
    fn shape_requiring_fn(fn_name: &str) -> CompiledTrait {
        CompiledTrait {
            name: "Shape".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![TraitRequirement {
                name: fn_name.to_string(),
                kind: RequirementKind::Fn(CompiledAssocFnSig {
                    name: fn_name.to_string(),
                    has_self: true,
                    params: vec![],
                    return_type: Type::dimensionless_scalar(),
                }),
                span: SourceSpan::empty(0),
            }],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        }
    }

    /// Build structure `S : Shape { <members> }`.
    fn structure_s_conforming_shape(
        members: Vec<reify_ast::MemberDecl>,
    ) -> reify_ast::StructureDef {
        reify_ast::StructureDef {
            name: "S".to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            trait_bounds: vec![reify_ast::TraitBoundRef {
                name: "Shape".to_string(),
                type_args: vec![],
                span: SourceSpan::empty(0),
            }],
            members,
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            pragmas: vec![],
            annotations: vec![],
        }
    }

    /// A provided assoc fn whose RETURN TYPE differs from the trait requirement
    /// must surface `TraitFnSignatureMismatch` (§8.8 same-name-different-type)
    /// and must NOT also surface `TraitFnNotSatisfied` — the fn IS present, it
    /// is just mis-typed.
    #[test]
    fn provided_assoc_fn_with_wrong_return_type_emits_signature_mismatch() {
        let shape = shape_requiring_fn("area");
        // Structure provides `fn area(self) -> Length { 0.0 }` (Length != Real).
        let structure_def = structure_s_conforming_shape(vec![assoc_fn_member(
            "area",
            vec![assoc_self_param()],
            "Length",
        )]);

        let diagnostics = run_conformance(&[shape], &structure_def, &[]).diagnostics;

        let mismatch: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "expected exactly one TraitFnSignatureMismatch for the wrong-return-type \
             provided fn 'area'; got: {:?}",
            diagnostics
        );
        assert!(
            mismatch[0].message.contains("area"),
            "signature-mismatch diagnostic should name the fn 'area'; got: {}",
            mismatch[0].message
        );

        let not_satisfied: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied))
            .collect();
        assert!(
            not_satisfied.is_empty(),
            "the fn 'area' is present (just mis-typed) — TraitFnNotSatisfied must NOT \
             fire; got: {:?}",
            diagnostics
        );
    }

    /// A provided assoc fn with the wrong ARITY (an extra non-self param) is also
    /// a signature mismatch. Same assertions as the return-type case.
    #[test]
    fn provided_assoc_fn_with_wrong_arity_emits_signature_mismatch() {
        let shape = shape_requiring_fn("area");
        // Structure provides `fn area(self, extra: Real) -> Real { 0.0 }`
        // (params [Real] != the required []).
        let structure_def = structure_s_conforming_shape(vec![assoc_fn_member(
            "area",
            vec![assoc_self_param(), assoc_named_param("extra", "Real")],
            "Real",
        )]);

        let diagnostics = run_conformance(&[shape], &structure_def, &[]).diagnostics;

        let mismatch: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "expected exactly one TraitFnSignatureMismatch for the wrong-arity \
             provided fn 'area'; got: {:?}",
            diagnostics
        );
        assert!(
            mismatch[0].message.contains("area"),
            "signature-mismatch diagnostic should name the fn 'area'; got: {}",
            mismatch[0].message
        );

        let not_satisfied: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied))
            .collect();
        assert!(
            not_satisfied.is_empty(),
            "the fn 'area' is present (just wrong arity) — TraitFnNotSatisfied must \
             NOT fire; got: {:?}",
            diagnostics
        );
    }

    // --- assoc-fn table population fixtures (task 3939 δ) ---

    /// Build a body-carrying `fn <name>(<params>) -> <return_type_name> { <value> }`
    /// `FnDef`. The distinct `value` lets the override/default tests assert that
    /// override and default bodies compile to different content hashes.
    fn assoc_fn_def(
        name: &str,
        params: Vec<reify_ast::FnParam>,
        return_type_name: &str,
        value: f64,
    ) -> reify_ast::FnDef {
        reify_ast::FnDef {
            name: name.to_string(),
            doc: None,
            is_pub: false,
            type_params: vec![],
            params,
            return_type: Some(reify_ast::TypeExpr {
                kind: reify_ast::TypeExprKind::Named {
                    name: return_type_name.to_string(),
                    type_args: vec![],
                },
                span: SourceSpan::empty(0),
            }),
            body: Some(reify_ast::FnBody {
                let_bindings: vec![],
                result_expr: reify_ast::Expr {
                    kind: reify_ast::ExprKind::NumberLiteral {
                        value,
                        is_real: true,
                    },
                    span: SourceSpan::empty(0),
                },
            }),
            span: SourceSpan::empty(0),
            content_hash: ContentHash(0),
            annotations: vec![],
        }
    }

    /// Trait `Shape` with a single default-providing assoc fn
    /// `fn <fn_name>(self) -> Real { <value> }` and NO required members.
    fn shape_with_default_fn(fn_name: &str, value: f64) -> CompiledTrait {
        CompiledTrait {
            name: "Shape".to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![TraitDefault {
                name: Some(fn_name.to_string()),
                kind: DefaultKind::Fn(assoc_fn_def(
                    fn_name,
                    vec![assoc_self_param()],
                    "Real",
                    value,
                )),
                span: SourceSpan::empty(0),
            }],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        }
    }

    /// A trait's default-providing assoc fn is injected into the conformer's
    /// assoc-fn table when the structure does NOT override it —
    /// `is_override == false`, keyed by `(trait, fn)`.
    #[test]
    fn default_assoc_fn_injected_into_table_when_not_overridden() {
        let shape = shape_with_default_fn("area", 1.0);
        let structure_def = structure_s_conforming_shape(vec![]); // no override

        let ConformanceRun {
            diagnostics,
            assoc_fns,
            ..
        } = run_conformance(&[shape], &structure_def, &[]);

        // A default-providing fn imposes no requirement, so conformance is clean.
        assert!(
            !diagnostics.iter().any(|d| {
                d.code == Some(DiagnosticCode::TraitFnNotSatisfied)
                    || d.code == Some(DiagnosticCode::TraitFnSignatureMismatch)
            }),
            "a default-providing assoc fn should conform cleanly; got: {:?}",
            diagnostics
        );
        assert_eq!(
            assoc_fns.len(),
            1,
            "expected exactly one injected assoc-fn table entry; got: {:?}",
            assoc_fns
        );
        let entry = &assoc_fns[0];
        assert_eq!(
            entry.trait_name, "Shape",
            "entry should be keyed by the declaring trait"
        );
        assert_eq!(
            entry.fn_name, "area",
            "entry should be keyed by the fn name"
        );
        assert!(
            !entry.is_override,
            "a non-overridden default must have is_override == false; got: {:?}",
            entry
        );
        assert_eq!(entry.function.name, "area");
        assert_eq!(
            entry.function.return_type,
            Type::dimensionless_scalar(),
            "the injected default's compiled return type should be Real"
        );
    }

    /// When the structure overrides a trait's default-providing assoc fn, the
    /// table entry is the OVERRIDE (`is_override == true`) and its compiled
    /// `function.content_hash` differs from the injected-default's (distinct
    /// body ⇒ distinct hash).
    #[test]
    fn override_assoc_fn_beats_default_in_table() {
        // Reference run: no override → the injected default body (1.0).
        let s_default = structure_s_conforming_shape(vec![]);
        let assoc_default =
            run_conformance(&[shape_with_default_fn("area", 1.0)], &s_default, &[]).assoc_fns;
        assert_eq!(
            assoc_default.len(),
            1,
            "default run should populate one entry"
        );
        assert!(!assoc_default[0].is_override);

        // Override run: structure provides `fn area(self) -> Real { 2.0 }`.
        let s_override = structure_s_conforming_shape(vec![reify_ast::MemberDecl::Fn(
            assoc_fn_def("area", vec![assoc_self_param()], "Real", 2.0),
        )]);
        let assoc_override =
            run_conformance(&[shape_with_default_fn("area", 1.0)], &s_override, &[]).assoc_fns;
        assert_eq!(
            assoc_override.len(),
            1,
            "override run should populate one entry"
        );
        let entry = &assoc_override[0];
        assert_eq!(entry.trait_name, "Shape");
        assert_eq!(entry.fn_name, "area");
        assert!(
            entry.is_override,
            "a structure-provided body must set is_override == true; got: {:?}",
            entry
        );

        // override-beats-default: distinct body (2.0 vs 1.0) ⇒ distinct hash.
        assert_ne!(
            entry.function.content_hash, assoc_default[0].function.content_hash,
            "the override body (2.0) must compile to a different content hash than \
             the injected default body (1.0)"
        );
    }

    /// Regression (task 3939 δ, reviewer amendment): overriding a trait's
    /// default-providing assoc fn with a MISMATCHED signature must surface
    /// `TraitFnSignatureMismatch` and keep the wrongly-typed override OUT of the
    /// dispatch table. A default-only fn produces no `RequirementKind::Fn`, so
    /// phase 5 never checks it — resolving the assoc-fn table is the sole
    /// validation site (PRD §5.4 exact-match-for-overrides). Before the
    /// amendment the bad override landed in the table with no diagnostic.
    #[test]
    fn override_of_default_assoc_fn_with_wrong_signature_emits_mismatch_and_skips_table() {
        // Trait default `fn area(self) -> Real { 1.0 }`.
        let shape = shape_with_default_fn("area", 1.0);
        // Structure overrides with `fn area(self) -> Length { 0.0 }` (Length != Real).
        let s_override = structure_s_conforming_shape(vec![assoc_fn_member(
            "area",
            vec![assoc_self_param()],
            "Length",
        )]);

        let ConformanceRun {
            diagnostics,
            assoc_fns,
            ..
        } = run_conformance(&[shape], &s_override, &[]);

        let mismatch: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch))
            .collect();
        assert_eq!(
            mismatch.len(),
            1,
            "a default override with a wrong return type must emit exactly one \
             TraitFnSignatureMismatch; got: {:?}",
            diagnostics
        );
        assert!(
            mismatch[0].message.contains("area"),
            "signature-mismatch diagnostic should name the fn 'area'; got: {}",
            mismatch[0].message
        );
        // The wrongly-typed override must NOT enter the dispatch table — task ζ
        // would otherwise key dispatch on an entry inconsistent with the error.
        assert!(
            assoc_fns.is_empty(),
            "a signature-mismatched default override must be kept out of the \
             assoc-fn table; got: {:?}",
            assoc_fns
        );
    }

    /// Regression (task 3939 δ, reviewer amendment): a required-fn override whose
    /// return annotation is UNRESOLVABLE must yield exactly one diagnostic — the
    /// `UnresolvedType` from `compile_assoc_function` — with NO accompanying
    /// `TraitFnSignatureMismatch`. The structure-derived sig carries `Type::Error`,
    /// which `assoc_fn_sig_has_error` recognises so the otherwise-spurious mismatch
    /// is suppressed (the sig "differs" only because resolution failed). Locks in
    /// the documented anti-cascade contract (one root cause → one diagnostic) and
    /// confirms the errored override is also kept out of the dispatch table.
    #[test]
    fn required_assoc_fn_override_with_unresolved_annotation_yields_single_diagnostic() {
        // Trait requires bodyless `fn area(self) -> Real`.
        let shape = shape_requiring_fn("area");
        // Structure provides `fn area(self) -> Nonexistent { 0.0 }` — the return
        // annotation does not resolve to any built-in / alias / structure / trait.
        let s_override = structure_s_conforming_shape(vec![assoc_fn_member(
            "area",
            vec![assoc_self_param()],
            "Nonexistent",
        )]);

        let ConformanceRun {
            diagnostics,
            assoc_fns,
            ..
        } = run_conformance(&[shape], &s_override, &[]);

        let unresolved: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.code == Some(DiagnosticCode::UnresolvedType))
            .collect();
        assert_eq!(
            unresolved.len(),
            1,
            "the unresolved return annotation should produce exactly one \
             UnresolvedType diagnostic; got: {:?}",
            diagnostics
        );
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::TraitFnSignatureMismatch)),
            "the Type::Error sig must suppress a spurious TraitFnSignatureMismatch \
             (anti-cascade: one root cause, one diagnostic); got: {:?}",
            diagnostics
        );
        // The fn IS present (just mis-annotated), so absence is not the failure.
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.code == Some(DiagnosticCode::TraitFnNotSatisfied)),
            "the fn 'area' is present — TraitFnNotSatisfied must NOT fire; got: {:?}",
            diagnostics
        );
        // The errored override must not enter the dispatch table.
        assert!(
            assoc_fns.is_empty(),
            "an errored / sig-mismatched required override must be kept out of the \
             assoc-fn table; got: {:?}",
            assoc_fns
        );
    }

    // ── unit tests for the resolve_let_advertised_type helper (task 1914) ──

    /// Pins the annotation-wins branch of `resolve_let_advertised_type`.
    ///
    /// When a cell_type annotation is present, the helper must return it unchanged
    /// regardless of what the inferred expression type is. This matches the behavior
    /// at the advertisement site (site 1) where annotated Let defaults bypass the
    /// inferred cache entirely.
    #[test]
    fn resolve_let_advertised_type_prefers_annotation_over_inferred() {
        let inferred = CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar());
        let result = resolve_let_advertised_type(&Some(Type::length()), Some(&inferred));
        assert_eq!(
            result,
            Type::length(),
            "Expected annotation type (Length) to win over inferred type (Real)"
        );
    }

    /// Pins the inferred-fallback branch of `resolve_let_advertised_type`.
    ///
    /// When there is no cell_type annotation but an inferred CompiledExpr is available,
    /// the helper must return the inferred expression's result_type. This is the common
    /// path for unannotated let defaults that passed Pass 2 compilation successfully.
    /// After task 1914 suggestion #1, this is the only non-defensive path for
    /// unannotated lets: names in `pass2_compile_errors` never reach this helper.
    #[test]
    fn resolve_let_advertised_type_uses_inferred_when_no_annotation() {
        let inferred = CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar());
        let result = resolve_let_advertised_type(&None, Some(&inferred));
        assert_eq!(
            result,
            Type::dimensionless_scalar(),
            "Expected inferred type (Real) when no annotation is present"
        );
    }

    /// Pins the anti-cascade-poison fallback branch of `resolve_let_advertised_type`.
    ///
    /// When neither a cell_type annotation nor an inferred expression is available,
    /// the helper returns `Type::Error` (anti-cascade poison). This arm should not be
    /// reached in practice — the `debug_assert!` at the advertisement call site
    /// (site 1) catches drift;
    /// the `Type::Error` fallback is defense in depth for release builds where the
    /// `debug_assert!` is a no-op (task 3749, tightening of the 3639 G-allow carve-out).
    #[test]
    fn resolve_let_advertised_type_falls_back_to_error_when_neither_annotation_nor_inferred() {
        let result = resolve_let_advertised_type(&None, None);
        assert_eq!(
            result,
            Type::Error,
            "Expected Type::Error anti-cascade poison when no annotation and no inferred expression"
        );
    }

    /// `emit_geometry_unbounded` pushes exactly one `Diagnostic` with severity
    /// `Error`, code `Some(DiagnosticCode::GeometryUnbounded)`, a message
    /// mentioning the arg name and the `Bounded` trait, and a `DiagnosticLabel`
    /// at the supplied span. This pins the diagnostic-shape contract
    /// independent of the conformance-walker integration (which is exercised
    /// end-to-end by the inference test file's positive case).
    #[test]
    fn emit_geometry_unbounded_helper_produces_error_with_code_and_label() {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let span = SourceSpan::new(7, 19);
        emit_geometry_unbounded("g", span, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            1,
            "emit_geometry_unbounded should push exactly one diagnostic"
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::GeometryUnbounded));
        assert!(
            d.message.contains("Bounded"),
            "message should mention the Bounded trait, got: {}",
            d.message
        );
        assert!(
            d.message.contains("'g'"),
            "message should mention the arg name 'g', got: {}",
            d.message
        );
        assert_eq!(
            d.labels.len(),
            1,
            "expected exactly one label attached at the supplied span"
        );
        assert_eq!(d.labels[0].span, span);
    }

    /// `emit_geometry_profile_required` pushes exactly one `Diagnostic` with
    /// severity `Error`, code `Some(DiagnosticCode::GeometryProfileRequired)`, a
    /// message naming both the arg and the `requirement`, and a single
    /// `DiagnosticLabel` at the supplied span. Pins the diagnostic-shape contract
    /// independent of the geometry.rs consumer wiring (which is exercised
    /// end-to-end in `geometry_profile_precondition_tests.rs`). Mirrors
    /// `emit_geometry_unbounded_helper_produces_error_with_code_and_label`; the
    /// `requirement` is parameterized so the one helper serves both the Surface
    /// profile and Curve path consumers.
    #[test]
    fn emit_geometry_profile_required_helper_produces_error_with_code_and_label() {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let span = SourceSpan::new(7, 19);
        emit_geometry_profile_required(
            "g",
            "a 2D Surface profile (Closed, Planar)",
            span,
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "emit_geometry_profile_required should push exactly one diagnostic"
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::GeometryProfileRequired));
        assert!(
            d.message.contains("Surface"),
            "message should mention the requirement text, got: {}",
            d.message
        );
        assert!(
            d.message.contains("'g'"),
            "message should mention the arg name 'g', got: {}",
            d.message
        );
        assert_eq!(
            d.labels.len(),
            1,
            "expected exactly one label attached at the supplied span"
        );
        assert_eq!(d.labels[0].span, span);
    }

    /// `emit_geometry_trait_violation` pushes exactly one `Diagnostic` with severity
    /// `Error`, code `Some(DiagnosticCode::TypeNotConformingToTrait)`, the exact message
    /// `"geometry argument 'g' does not conform to trait 'Connected'"`, and a
    /// `DiagnosticLabel` at the supplied span. This is the symmetric sibling of
    /// `emit_geometry_unbounded` for the `Connected`/`Convex` cases.
    ///
    /// The full `assert_eq!` on `d.message` is the wording contract: it simultaneously
    /// confirms the arg name and trait name are present and that there is no redundant
    /// `"required by param"` suffix (the old inline branch repeated the arg name there).
    #[test]
    fn emit_geometry_trait_violation_helper_produces_error_with_code_and_label() {
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let span = SourceSpan::new(7, 19);
        emit_geometry_trait_violation("g", "Connected", span, &mut diagnostics);

        assert_eq!(
            diagnostics.len(),
            1,
            "emit_geometry_trait_violation should push exactly one diagnostic"
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, Some(DiagnosticCode::TypeNotConformingToTrait));
        assert_eq!(
            d.message, "geometry argument 'g' does not conform to trait 'Connected'",
            "message wording contract: arg name + trait name, no 'required by param' suffix"
        );
        assert_eq!(
            d.labels.len(),
            1,
            "expected exactly one label attached at the supplied span"
        );
        assert_eq!(d.labels[0].span, span);
    }

    /// Build a minimal `TopologyTemplate` for cycle-guard tests.
    ///
    /// The template has no realizations so `RealizationLetEnv::lookup` always
    /// falls through to the value_cells arm — the arm containing the recursive
    /// call to `infer_traits_for_expr_in_env(expr, self)`.  All other fields are
    /// zeroed/empty; they are irrelevant to the cycle-guard path.
    fn minimal_template(name: &str, cells: Vec<ValueCellDecl>) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells: cells,
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            relations: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    /// Build a `Let`-kind `ValueCellDecl` whose `default_expr` is a `ValueRef`
    /// pointing at `ref_id`.  Used to construct cycle fixtures in the tests below.
    fn let_cell_with_ref(id: ValueCellId, ref_id: ValueCellId) -> ValueCellDecl {
        ValueCellDecl {
            id,
            kind: ValueCellKind::Let,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::dimensionless_scalar(),
            default_expr: Some(CompiledExpr::value_ref(
                ref_id,
                Type::dimensionless_scalar(),
            )),
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        }
    }

    /// Cycle guard: a self-referential `let` binding (`let g : Real = ValueRef(g)`)
    /// must not overflow the stack; the in-flight visited set terminates the cycle
    /// and `lookup` returns the safe-default `Some(InferredTraits::all())`.
    ///
    /// # Why the value_cells path is exercised
    ///
    /// Entity "E" has no geometry realization for "g" (no realization in the
    /// template), so the realization arm is skipped and `lookup` falls through to
    /// the value_cells arm — the arm that contains the recursive call to
    /// `infer_traits_for_expr_in_env(expr, self)`.
    ///
    /// # Why `Type::dimensionless_scalar()`
    ///
    /// A non-geometry type confirms the fixture bypasses the realization arm
    /// and only exercises the value_cells fallback where the recursion lives.
    ///
    /// # Assertion rationale
    ///
    /// `Some(InferredTraits::all())` is the safe-default propagation chain:
    /// inner `lookup` hits the in-flight guard → returns `None` →
    /// `infer_traits_for_expr_in_env(...).unwrap_or(all())` returns `all()` →
    /// outer `lookup` wraps in `Some`.
    #[test]
    fn lookup_self_referential_let_returns_safe_default_without_overflow() {
        use std::cell::RefCell;

        // let g : Real = ValueRef(g)  — direct self-reference
        let g_id = ValueCellId::new("E", "g");
        let template = minimal_template("E", vec![let_cell_with_ref(g_id.clone(), g_id.clone())]);

        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv {
            templates: &templates,
            in_flight: RefCell::new(Vec::new()),
        };

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::all()),
            "self-referential let must safe-default to all() rather than stack-overflowing"
        );
    }

    /// Cycle guard: a chained-cycle let (`let g = ValueRef(h); let h = ValueRef(g)`)
    /// must not overflow the stack; the in-flight visited set terminates the cycle
    /// at re-entry and `lookup` returns the safe-default `Some(InferredTraits::all())`.
    ///
    /// Pins that the guard handles cycles longer than direct self-reference.
    /// The assertion shape is identical to the self-referential test; both produce
    /// `Some(InferredTraits::all())`.
    #[test]
    fn lookup_chained_cycle_let_returns_safe_default_without_overflow() {
        use std::cell::RefCell;

        // let g : Real = ValueRef(h)
        // let h : Real = ValueRef(g)
        let g_id = ValueCellId::new("E", "g");
        let h_id = ValueCellId::new("E", "h");
        let template = minimal_template(
            "E",
            vec![
                let_cell_with_ref(g_id.clone(), h_id.clone()),
                let_cell_with_ref(h_id.clone(), g_id.clone()),
            ],
        );

        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv {
            templates: &templates,
            in_flight: RefCell::new(Vec::new()),
        };

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::all()),
            "chained-cycle let must safe-default to all() rather than stack-overflowing"
        );
    }

    /// Non-cyclic chain: `let g = ValueRef(h); let h = difference(box(), box())`.
    ///
    /// Asserts that the in-flight guard does **not** short-circuit a non-cyclic
    /// chain.  `lookup(h)` must resolve to `bounded_only()` — the inferred traits
    /// of `difference` applied to two bounded primitives — and `lookup(g)` must
    /// propagate that result rather than safe-defaulting to `all()`.
    ///
    /// This test distinguishes a correct guard (fires only when `id` is already
    /// in the in-flight set) from a guard that fires too eagerly — for example,
    /// one that returns `None` whenever `in_flight` is non-empty regardless of
    /// which id is being resolved.  Without this test, both cycle tests pass even
    /// with such a premature-firing guard because they assert `Some(all())`.
    ///
    /// # Fixture rationale
    ///
    /// `h`'s `default_expr` is `FunctionCall("difference", [box(), box()])` with
    /// `result_type: Type::Geometry`.  `infer_traits_for_expr_in_env` dispatches
    /// on `expr.kind` (not `cell_type`), so the `FunctionCall` arm fires and
    /// returns `combine_difference(all(), all()) = bounded_only()`.
    #[test]
    fn lookup_chained_noncyclic_let_propagates_terminal_traits() {
        use std::cell::RefCell;

        // box_expr: FunctionCall("box", [], Geometry) → InferredTraits::all()
        let box_expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "box".to_string(),
                    qualified_name: "std::box".to_string(),
                },
                args: vec![],
            },
            result_type: Type::Geometry,
            content_hash: ContentHash(0),
        };

        // diff_expr: FunctionCall("difference", [box, box]) → bounded_only()
        let diff_expr = CompiledExpr {
            kind: CompiledExprKind::FunctionCall {
                function: ResolvedFunction {
                    name: "difference".to_string(),
                    qualified_name: "std::difference".to_string(),
                },
                args: vec![box_expr.clone(), box_expr],
            },
            result_type: Type::Geometry,
            content_hash: ContentHash(0),
        };

        let g_id = ValueCellId::new("E", "g");
        let h_id = ValueCellId::new("E", "h");

        // g: Let Real = ValueRef(h)  — bridges g into the value_cells fallback
        let g_cell = let_cell_with_ref(g_id.clone(), h_id.clone());
        // h: Let Real = difference(box(), box())  — terminal, resolves to bounded_only()
        let h_cell = ValueCellDecl {
            id: h_id,
            kind: ValueCellKind::Let,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::dimensionless_scalar(),
            default_expr: Some(diff_expr),
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        };

        let template = minimal_template("E", vec![g_cell, h_cell]);
        let mut templates = HashMap::new();
        templates.insert("E".to_string(), &template);

        let env = RealizationLetEnv {
            templates: &templates,
            in_flight: RefCell::new(Vec::new()),
        };

        assert_eq!(
            env.lookup(&g_id),
            Some(InferredTraits::bounded_only()),
            "non-cyclic chain g→h→difference(box,box) must propagate bounded_only(), \
             not short-circuit to all() via premature cycle-guard firing"
        );
    }

    /// Regression for task-2543 / task-2458: `check_trait_arg_conformance` must
    /// recurse into `ReflectiveCellList` elements the same way it recurses into
    /// `ListLiteral` elements.
    ///
    /// # Why this test lives here (and cannot move to `tests/*.rs`)
    ///
    /// `ReflectiveCellList` is produced exclusively at activation time by
    /// `expand_purpose_reflective_placeholders` (reify-eval), *after* compile-time
    /// conformance has already run — there is no `compile_source(...)` string that
    /// routes an RCL node into `check_trait_arg_conformance`. The fixture must be
    /// hand-built, which requires `pub(crate)` access to
    /// `check_trait_arg_conformance`. See the module-level comment above for the
    /// full rationale shared by all unit tests in this block.
    ///
    /// # RED before the fix (task-2543)
    ///
    /// The `(Type::List, ListLiteral)` arm in `walk_param_against_arg` does not
    /// match the `ReflectiveCellList` variant. The walker falls through to the
    /// type-level fallback; `walk_param_against_arg_type(List<MS>, List<MS>)` sees
    /// identical wrapper shapes and emits no diagnostic. The element's actual
    /// `StructureRef("NotAMaterial")` is never inspected → 0 diagnostics.
    ///
    /// # GREEN after the fix (task-2543)
    ///
    /// The merged `ListLiteral | ReflectiveCellList` arm iterates the elements.
    /// The leaf check sees `StructureRef("NotAMaterial")`, looks up "NotAMaterial"
    /// (no `MaterialSpec` in `trait_bounds`), and emits
    /// `TypeNotConformingToTrait`.
    ///
    /// # Fixture shape (design decision #3)
    ///
    /// The RCL `result_type` is set to `List<TraitObject("MaterialSpec")>` —
    /// identical to the param type — so the type-level fallback finds no
    /// wrapper-level mismatch. This is what makes the test RED before the fix:
    /// only the dedicated literal arm can see the element's actual type.
    #[test]
    fn walk_param_against_arg_recurses_into_reflective_cell_list_elements() {
        // ── Trait registry ────────────────────────────────────────────────────
        let material_spec = CompiledTrait {
            name: "MaterialSpec".to_string(),
            is_pub: true,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        };
        let mut trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        trait_registry.insert("MaterialSpec".to_string(), &material_spec);

        // ── Template registry ─────────────────────────────────────────────────
        // "Host" template with one param: `ms : List<MaterialSpec>`.
        let ms_cell = ValueCellDecl {
            id: ValueCellId::new("Host", "ms"),
            kind: ValueCellKind::Param,
            visibility: Visibility::Public,
            is_aux: false,
            cell_type: Type::List(Box::new(Type::TraitObject("MaterialSpec".to_string()))),
            default_expr: None,
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        };
        let host_template = minimal_template("Host", vec![ms_cell]);

        // "NotAMaterial" template — no trait_bounds, so does NOT refine MaterialSpec.
        let not_a_material = minimal_template("NotAMaterial", vec![]);

        let mut template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        template_registry.insert("Host".to_string(), &host_template);
        template_registry.insert("NotAMaterial".to_string(), &not_a_material);

        // ── Hand-built RCL compiled arg ───────────────────────────────────────
        // RCL result_type intentionally matches the param type (List<MaterialSpec>)
        // so the type-level fallback finds no wrapper mismatch — the only way to
        // catch the element violation is via the dedicated literal arm (task-2543
        // design decision #3).
        let elem = CompiledExpr::value_ref(
            ValueCellId::new("E", "x"),
            Type::StructureRef("NotAMaterial".to_string()),
        );
        let rcl = CompiledExpr::reflective_cell_list(
            vec![elem],
            Type::List(Box::new(Type::TraitObject("MaterialSpec".to_string()))),
        );

        // ── Invoke the walker ─────────────────────────────────────────────────
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_trait_arg_conformance(
            "Host",
            "ms",
            &rcl,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        // ── Assertions ────────────────────────────────────────────────────────
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 diagnostic (RCL element 'NotAMaterial' does not conform \
             to 'MaterialSpec'), got {}: {diagnostics:?}",
            diagnostics.len(),
        );
        let d = &diagnostics[0];
        // check_trait_arg_conformance is a ctor-conformance entry, so its diagnostics
        // are emitted at CTOR_FIELD_CONFORMANCE_SEVERITY. Task 5302 α downgraded that
        // knob to Warning (Option-A uniform downgrade); task 5306 δ flipped it back to
        // Error. Code/count/message were unchanged by both moves. Written as a literal
        // rather than a read of the const so a future re-flip cannot pass vacuously.
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToTrait),
            "expected TypeNotConformingToTrait, got {:?}",
            d.code,
        );
        assert!(
            d.message.contains("NotAMaterial"),
            "diagnostic message should mention 'NotAMaterial', got: {:?}",
            d.message,
        );
        assert!(
            d.message.contains("MaterialSpec"),
            "diagnostic message should mention 'MaterialSpec', got: {:?}",
            d.message,
        );
    }

    // ── task-4081 step-3: check_fn_arg_conformance unit tests ────────────────

    /// Build a marker trait with no requirements.
    fn marker_trait(name: &str) -> CompiledTrait {
        CompiledTrait {
            name: name.to_string(),
            is_pub: false,
            doc: None,
            type_params: vec![],
            refinements: vec![],
            required_members: vec![],
            defaults: vec![],
            content_hash: ContentHash(0),
            annotations: vec![],
            pragmas: vec![],
        }
    }

    /// Build a `TopologyTemplate` with the given `trait_bounds` and no value cells.
    fn template_with_bounds(name: &str, bounds: Vec<&str>) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: vec![],
            trait_bounds: bounds.into_iter().map(|s| s.to_string()).collect(),
            value_cells: vec![],
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            relations: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    /// (a) NonConforming arg against DrivingJoint param → one TypeNotConformingToTrait.
    /// RED until step-4: `check_fn_arg_conformance` does not exist yet.
    #[test]
    fn fn_arg_conformance_non_conforming_emits_diagnostic() {
        let conforming = template_with_bounds("Conforming", vec!["DrivingJoint"]);
        let non_conforming = template_with_bounds("NonConforming", vec![]);
        let template_registry: HashMap<String, &TopologyTemplate> = [
            ("Conforming", &conforming),
            ("NonConforming", &non_conforming),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

        let dj = marker_trait("DrivingJoint");
        let trait_registry: HashMap<String, &CompiledTrait> = [("DrivingJoint", &dj)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::StructureRef("NonConforming".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::TraitObject("DrivingJoint".to_string()),
            "joint",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToTrait diagnostic, got: {:?}",
            diagnostics
        );
        let d = &diagnostics[0];
        assert_eq!(d.code, Some(DiagnosticCode::TypeNotConformingToTrait));
        assert!(
            d.message.contains("NonConforming"),
            "diagnostic should mention 'NonConforming'; got: {}",
            d.message
        );
        assert!(
            d.message.contains("DrivingJoint"),
            "diagnostic should mention 'DrivingJoint'; got: {}",
            d.message
        );
    }

    /// (b) Conforming arg against DrivingJoint param → zero diagnostics.
    #[test]
    fn fn_arg_conformance_conforming_emits_no_diagnostic() {
        let conforming = template_with_bounds("Conforming", vec!["DrivingJoint"]);
        let template_registry: HashMap<String, &TopologyTemplate> = [("Conforming", &conforming)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        let dj = marker_trait("DrivingJoint");
        let trait_registry: HashMap<String, &CompiledTrait> = [("DrivingJoint", &dj)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::StructureRef("Conforming".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::TraitObject("DrivingJoint".to_string()),
            "joint",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "expected no diagnostics for conforming arg, got: {:?}",
            diagnostics
        );
    }

    /// (c) Type::Error arg → zero diagnostics (anti-cascade).
    #[test]
    fn fn_arg_conformance_error_arg_no_cascade() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();

        let error_arg = CompiledExpr::literal(Value::Real(0.0), Type::Error);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::TraitObject("DrivingJoint".to_string()),
            "joint",
            &error_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "Type::Error arg must not produce diagnostics (anti-cascade), got: {:?}",
            diagnostics
        );
    }

    /// (d) Bare trait param + `Type::TypeParam` (non-literal ValueRef) arg → zero
    /// diagnostics. An unresolved generic type variable forwarded to a trait param
    /// routes through the literal walker (`check_leaf_trait_conformance`); it is
    /// unverifiable here, not non-conforming, so it must be skipped rather than emit a
    /// false-positive `TypeNotConformingToTrait` (esc-4081 amend,
    /// reviewer_comprehensive robustness_false_positive).
    #[test]
    fn fn_arg_conformance_type_param_arg_skipped() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let dj = marker_trait("DrivingJoint");
        let trait_registry: HashMap<String, &CompiledTrait> = [("DrivingJoint", &dj)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        // A non-literal arg (ValueRef) whose result_type is an unresolved type param.
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "inner"),
            Type::TypeParam("T".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::TraitObject("DrivingJoint".to_string()),
            "joint",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "an unresolved TypeParam arg must not produce a conformance diagnostic \
             (unverifiable, not non-conforming), got: {:?}",
            diagnostics
        );
    }

    /// (e) Wrapper trait param `Option<DrivingJoint>` + `Option<TypeParam>`
    /// (non-literal ValueRef) arg → zero diagnostics. The wrapper case routes through
    /// the type-level walker (`walk_param_against_arg_type`); its `TypeParam` leaf must
    /// be skipped for the same reason as the bare case above.
    #[test]
    fn fn_arg_conformance_wrapped_type_param_arg_skipped() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let dj = marker_trait("DrivingJoint");
        let trait_registry: HashMap<String, &CompiledTrait> = [("DrivingJoint", &dj)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        // Non-literal `Option<T>` ValueRef against an `Option<DrivingJoint>` param.
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "inner"),
            Type::Option(Box::new(Type::TypeParam("T".to_string()))),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::Option(Box::new(Type::TraitObject("DrivingJoint".to_string()))),
            "joint",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );

        assert!(
            diagnostics.is_empty(),
            "an unresolved Option<TypeParam> arg must not produce a conformance \
             diagnostic (unverifiable, not non-conforming), got: {:?}",
            diagnostics
        );
    }

    // ── Drift-guard: L2 joint-constructor name → PascalCase type mapping ──────

    /// Pin every arm of [`resolve_joint_nominal_type`]'s Path-B match so that
    /// adding a new joint kind to `reify-stdlib` forces an update here (and in
    /// the match itself).
    ///
    /// **Why this lives here (not in `tests/*.rs`):**
    /// `resolve_joint_nominal_type` is `fn` (private) — only accessible from
    /// `use super::*` inside this `mod tests` block.
    ///
    /// **DRIFT NOTE:** the canonical list of joint constructor builtins and their
    /// runtime `kind` strings lives in `reify-stdlib/src/joints.rs`
    /// (`JOINT_KINDS`, `pub(crate)`) — not directly importable from
    /// `reify-compiler`.  This behavioral test substitutes for a structural
    /// assertion against `JOINT_KINDS`.  A future kind addition requires updates
    /// in BOTH the Path-B match in `resolve_joint_nominal_type` AND this test.
    #[test]
    fn l2_joint_constructor_name_mapping_exhaustive() {
        // Build a minimal FunctionCall CompiledExpr for a given constructor name.
        // result_type = Type::dimensionless_scalar() (arbitrary — Path B ignores it and keeys on
        // the function.name string).
        let make_call = |name: &str| -> CompiledExpr {
            CompiledExpr {
                kind: CompiledExprKind::FunctionCall {
                    function: ResolvedFunction {
                        name: name.to_owned(),
                        qualified_name: format!("std::kinematic::{name}"),
                    },
                    args: vec![],
                },
                result_type: Type::dimensionless_scalar(),
                content_hash: ContentHash(0),
            }
        };

        // ── Non-driving coupling constructors → "Coupling" ────────────────────
        for ctor in ["couple", "gear", "screw", "rack_and_pinion"] {
            assert_eq!(
                resolve_joint_nominal_type(&make_call(ctor)).as_deref(),
                Some("Coupling"),
                "constructor '{ctor}' must resolve to 'Coupling'"
            );
        }

        // ── Non-driving fixed constructor → "Fixed" ───────────────────────────
        assert_eq!(
            resolve_joint_nominal_type(&make_call("fixed")).as_deref(),
            Some("Fixed"),
            "constructor 'fixed' must resolve to 'Fixed'"
        );

        // ── Driving joint constructors → their PascalCase kind names ──────────
        let driving = [
            ("prismatic", "Prismatic"),
            ("revolute", "Revolute"),
            ("cylindrical", "Cylindrical"),
            ("planar", "Planar"),
            ("spherical", "Spherical"),
        ];
        for (ctor, expected) in driving {
            assert_eq!(
                resolve_joint_nominal_type(&make_call(ctor)).as_deref(),
                Some(expected),
                "constructor '{ctor}' must resolve to '{expected}'"
            );
        }

        // ── Unknown constructor → None (skip, no diagnostic) ─────────────────
        assert_eq!(
            resolve_joint_nominal_type(&make_call("unknown_joint")).as_deref(),
            None,
            "unknown constructor must return None (not a recognized joint builtin)"
        );
    }

    // ── task-4584 step-1: walk_param_against_arg_type StructureRef leaf arm ──

    /// RED until step-2 (impl): `walk_param_against_arg_type` currently falls
    /// through the `_` arm silently for `Type::StructureRef` params, so no
    /// diagnostic is emitted even for clear mismatches like String-arg vs Part-param.
    ///
    /// (a) String-typed arg against StructureRef("Part") param → exactly one
    ///     `TypeNotConformingToStructureRef`.
    #[test]
    fn structureref_param_rejects_string_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(ValueCellId::new("Test", "x"), Type::String);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToStructureRef diagnostic for String arg \
             against Part param, got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToStructureRef),
            "expected TypeNotConformingToStructureRef, got {:?}",
            d.code,
        );
    }

    /// (b) Matching `StructureRef("Part")` arg against `StructureRef("Part")`
    ///     param → ZERO diagnostics (identity passes).
    #[test]
    fn structureref_param_accepts_same_structureref_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::StructureRef("Part".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "expected ZERO diagnostics for Part arg vs Part param (identity), \
             got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// (c) Different `StructureRef("Other")` arg against `StructureRef("Part")`
    ///     param → exactly one `TypeNotConformingToStructureRef` (nominal mismatch).
    #[test]
    fn structureref_param_rejects_different_structureref_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::StructureRef("Other".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToStructureRef for Other vs Part, \
             got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToStructureRef),
            "expected TypeNotConformingToStructureRef, got {:?}",
            d.code,
        );
    }

    /// (d) `Type::TypeParam` and `Type::Error` args against a `StructureRef`
    ///     param → ZERO diagnostics (anti-cascade / unverifiable skip).
    #[test]
    fn structureref_param_skips_typeparam_and_error_args() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();

        // Type::TypeParam — unresolved generic, skip.
        let typeparam_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "x"),
            Type::TypeParam("T".to_string()),
        );
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &typeparam_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "TypeParam arg must emit ZERO diagnostics (unverifiable), \
             got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );

        // Type::Error — anti-cascade, check_fn_arg_conformance returns early.
        let error_arg = CompiledExpr::value_ref(ValueCellId::new("Test", "y"), Type::Error);
        let mut diagnostics2: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &Type::StructureRef("Part".to_string()),
            "part",
            &error_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics2,
        );
        assert_eq!(
            diagnostics2.len(),
            0,
            "Type::Error arg must emit ZERO diagnostics (anti-cascade early return), \
             got {}: {:?}",
            diagnostics2.len(),
            diagnostics2,
        );
    }

    /// Pin the intentional leniency in `check_param_default_conformance` for the
    /// `Type::StructureRef` arm: a `StructureRef("Other")` effective type in the
    /// default expression is conservatively **skipped** (zero diagnostics), even
    /// though the param's declared cell_type is `StructureRef("Part")`.
    ///
    /// ## Why this is intentional
    ///
    /// In Reify, a concrete structure type used as a default for a param that is
    /// declared with a *different* structure name is not necessarily incompatible —
    /// structural compatibility is assessed at evaluation time, not by a nominal
    /// name comparison alone (e.g. `param material : Material = Steel_AISI_1045()`
    /// is a common valid pattern).  The `check_param_default_conformance` arm
    /// therefore skips StructureRef effective types and only rejects clearly
    /// incompatible *primitive* types (String, Int, …).
    ///
    /// This contrasts with the constructor-arg path (`walk_param_against_arg_type`
    /// StructureRef arm) which uses `type_compatible` and does reject a
    /// `StructureRef("Other")` arg for a `StructureRef("Part")` param.  The two
    /// paths are deliberately asymmetric on this case; this test pins that gap so
    /// any future tightening is made consciously.
    #[test]
    fn structureref_param_default_with_different_structureref_silently_accepted() {
        // Build a Param cell: `param part : Part = <expr of type Other>`.
        let param_cell = ValueCellDecl {
            id: ValueCellId::new("Test", "part"),
            kind: ValueCellKind::Param,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::StructureRef("Part".to_string()),
            default_expr: Some(CompiledExpr::value_ref(
                ValueCellId::new("OtherStruct", "instance"),
                Type::StructureRef("Other".to_string()),
            )),
            solver_hints: vec![],
            span: SourceSpan::empty(0),
        };
        let template = minimal_template("Test", vec![param_cell]);
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_param_default_conformance(
            &template,
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "StructureRef default for StructureRef param with different name must emit \
             ZERO diagnostics (intentional leniency — not a primitive mismatch), \
             got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// Task 7174: a port-body param default (`CompiledPort.members`) must reach
    /// the SAME `check_param_default_conformance` walk as a top-level
    /// `value_cells` param default.
    ///
    /// `template.value_cells` and `template.ports[].members` are disjoint lists
    /// (deliberately — see `param_default_cells`'s doc comment). Before the fix,
    /// `check_param_default_conformance` walked only `value_cells`, so pushing a
    /// `Geometry` param cell onto `template.ports` instead of `template.value_cells`
    /// made it invisible to the walk: RED (zero diagnostics) until
    /// `param_default_cells` chains `template.ports[].members` in.
    ///
    /// Its integration twin `port_member_geometry_param_default_errors`
    /// (`harness_structure_declarations`) proves the same diagnostic end-to-end from
    /// real source, so this probe is not here for the diagnostic — it is here for
    /// the ROUTE. Constructing the cell on `ports[].members` and nowhere else is
    /// the only way to distinguish "the chain reached the port list" from "the
    /// producer merged port members into `value_cells` after all", and that second
    /// shape is a regression of the disjointness the GUI / `set_parameter` /
    /// `find_param_default_expr` consumers depend on, which the integration probe
    /// would happily stay green through.
    #[test]
    fn port_member_param_default_reaches_conformance_walk() {
        let region_cell = ValueCellDecl {
            id: ValueCellId::new("Test", "mount.region"),
            kind: ValueCellKind::Param,
            visibility: Visibility::Private,
            is_aux: false,
            cell_type: Type::Geometry,
            default_expr: Some(CompiledExpr::literal(
                reify_ir::Value::Real(5.0),
                Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                },
            )),
            solver_hints: vec![],
            span: SourceSpan::new(10, 20),
        };
        let mut template = minimal_template("Test", vec![]);
        template.ports.push(CompiledPort {
            name: "mount".to_string(),
            direction: reify_core::PortDirection::Bidi,
            type_name: "P".to_string(),
            members: vec![region_cell],
            constraints: vec![],
            frame_expr: None,
            is_priv: false,
        });

        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_param_default_conformance(
            &template,
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "port-member param default must reach the conformance walk and emit exactly \
             one diagnostic, got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        assert!(
            diagnostics[0].message.contains("mount.region"),
            "message must name the composite port-member param 'mount.region', got: {:?}",
            diagnostics[0].message
        );
    }

    // ── task-4622: walk_param_against_arg_type Vector leaf arm ───────────────

    /// (a) Bare scalar arg against `Vector3<Length>` param →
    /// exactly one `TypeNotConformingToVector` Error.
    ///
    /// RED until S4 adds the `Type::Vector` arm to `walk_param_against_arg_type`.
    #[test]
    fn vector_param_rejects_scalar_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        // Bare Real scalar: `1.0`
        let compiled_arg =
            CompiledExpr::literal(reify_ir::Value::Real(1.0), Type::dimensionless_scalar());
        let param_type = Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "axis",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToVector diagnostic for scalar arg \
             against Vector3<Length> param, got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToVector),
            "expected TypeNotConformingToVector, got {:?}",
            d.code,
        );
    }

    /// (b) A dimensionless `Vector{n:3}` arg against `Vector3<Length>` param →
    /// ZERO diagnostics (locks the loose-quantity positive leg: a dimensionless
    /// vec3 is accepted for a Length-quantity param).
    ///
    /// RED until S4 adds the arm (before S4 the `_` arm silently produces 0;
    /// after S4 the arm explicitly accepts vector-shaped args → still 0, but now
    /// for the right structural reason). The positive-leg stays green before AND
    /// after S4; what changes is the REJECTION leg in test (a).
    #[test]
    fn vector_param_accepts_dimensionless_vector_arg() {
        assert_quantity_slot_clean(
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            // A dimensionless Vec3 arg: `vec3(0.0, 0.0, 1.0)` compiles to Vector{n:3, Real}.
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
            "axis",
            "a dimensionless Vector3 arg must be accepted for a Vector3<Length> param — the \
             ARG side of the rule is TOLERANT, so a dimensionless slot names no dimension \
             (loose-quantity convention).",
        );
    }

    /// (c) A `Vector{n:2}` (vec2) arg against `Vector3<Length>` param →
    /// exactly one `TypeNotConformingToVector` Error (arity mismatch).
    ///
    /// Locks the arity-check added in the amendment pass: shape-based conformance now
    /// also requires matching `n` for `Type::Vector` args. `vec2` is a real mismatch
    /// for a `vec3`-typed param and must be rejected at compile time.
    ///
    /// This probe builds its `Type::Vector { n: 2, .. }` by hand, so it reaches the
    /// walker without depending on `math_fn_result_type`'s name-suffix `n` inference.
    /// The `.ri` twin that DOES depend on that inference, and so is the one that would
    /// notice it ceasing to produce a `Type::Vector { n: 2, .. }` at all, is
    /// `vec2_arg_at_vector3_param_errors_arity_type_not_conforming`
    /// (`struct_ctor_field_conformance_tests.rs`). Sibling of
    /// `point_param_rejects_wrong_arity_point_arg`.
    #[test]
    fn vector_param_rejects_wrong_arity_vector_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        // A vec2 arg: Vector{n:2, dimensionless} — correct shape, wrong arity.
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "v"),
            Type::Vector {
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::vec3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "axis",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 TypeNotConformingToVector for Vector2 arg vs \
             Vector3<Length> param (arity mismatch), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        let d = &diagnostics[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.code,
            Some(DiagnosticCode::TypeNotConformingToVector),
            "expected TypeNotConformingToVector, got {:?}",
            d.code,
        );
    }

    /// Loose-quantity positive leg of the `Type::Point` arm (task 5465,
    /// family 1): a DIMENSIONLESS `Point{n:3}` arg is accepted for a
    /// `Point3<Length>` param.
    ///
    /// Pins the ty.rs "Point / Vector quantity-slot convention" — the resolver
    /// always wraps `Q` in `Type::Scalar` while `Value::Point::infer_type` may
    /// yield a dimensionless or `Int` quantity, and the looseness is
    /// intentional. Sibling of `vector_param_accepts_dimensionless_vector_arg`.
    #[test]
    fn point_param_accepts_dimensionless_point_arg() {
        assert_quantity_slot_clean(
            Type::point3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            Type::Point {
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
            "origin",
            "a dimensionless Point3 arg must be accepted for a Point3<Length> param — the \
             ARG side of the rule is TOLERANT, so a dimensionless slot names no dimension \
             (loose-quantity convention).",
        );
    }

    /// CROSS-DIMENSION leg of the `Type::Point` arm (task 5766): a `Point{n:3}`
    /// arg whose quantity slot names a CONCRETE dimension different from the
    /// param's is exactly one `ArgTypeMismatch`.
    ///
    /// Constructed as a direct `Type` so the probe reaches the walker without
    /// depending on `math_fn_result_type`'s agreeing-components quantity
    /// inference — NOT because a `.ri` source cannot produce a dimensioned
    /// `Type::Point` arg. That older premise expired when task 5344
    /// (`3c4ee5e9ac`) claimed `point3` / `point2` into the math construction
    /// family; it must not be re-asserted.
    ///
    /// The `.ri` twin of this exact cell is
    /// `point3_cross_dimension_at_dimensioned_point_param_warns_arg_type_mismatch`
    /// (`struct_ctor_field_conformance_tests.rs`, ctor path, `Severity::Error`
    /// since δ / task #5306).
    /// Pinning BOTH seams matters because they reach this arm by different
    /// routes — a hand-built `Type` here, versus a `FunctionCall`'s inferred
    /// `result_type` there — so this probe holds the walker's rule whatever the
    /// inference chain does, while only the `.ri` fixture would notice that
    /// chain ceasing to produce a dimensioned `Type::Point` at all. This is the
    /// same two-seams shape `vector_param_rejects_cross_dimension_vector_arg`
    /// states one arm over.
    ///
    /// The value-cell route (`let p = point3(…); Anchor(origin: p)`) is a THIRD
    /// entry point and NOTHING here stands in for it — this probe builds no
    /// `CompiledExpr` at all. It is pinned by its own fixture,
    /// `point3_cross_dimension_via_let_at_dimensioned_point_param_errors_arg_type_mismatch`.
    ///
    /// The complement of `point_param_accepts_dimensionless_point_arg` directly
    /// above: that one pins the TOLERANT half (either side declines to name a
    /// concrete dimension ⇒ silent), this one pins the STRICT half (both name one
    /// and they differ ⇒ reject).
    #[test]
    fn point_param_rejects_cross_dimension_point_arg() {
        assert_quantity_slot_conflict(
            Type::point3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            Type::Point {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::MASS,
                }),
            },
            "origin",
            "Scalar[m]",
            "Scalar[kg]",
            "a Point3<Mass> arg at a Point3<Length> param must be REJECTED — both sides name a \
             concrete dimension and they disagree (task 5766).",
        );
    }

    /// CROSS-DIMENSION leg of the `Type::Vector` arm at the direct-`Type` seam
    /// (task 5766), sibling of `point_param_rejects_cross_dimension_point_arg`.
    ///
    /// The `.ri` seam for the same rule is
    /// `vec3_cross_dimension_at_dimensioned_vector_param_errors_arg_type_mismatch`
    /// in `struct_ctor_field_conformance_tests.rs`. Pinning BOTH seams matters
    /// because they reach the arm by different routes — a hand-built `Type`
    /// here, versus a `FunctionCall`'s inferred `result_type` there — so this
    /// probe holds the walker's rule whatever the inference chain does.
    ///
    /// The code assertion is the load-bearing half: a quantity conflict must
    /// route to `ArgTypeMismatch`, NOT to this arm's bespoke
    /// `TypeNotConformingToVector`, which keeps owning ARITY/FAMILY failures only
    /// (`vector_param_rejects_wrong_arity_vector_arg` pins that side).
    #[test]
    fn vector_param_rejects_cross_dimension_vector_arg() {
        assert_quantity_slot_conflict(
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::MASS,
                }),
            },
            "axis",
            "Scalar[m]",
            "Scalar[kg]",
            "a Vector3<Mass> arg at a Vector3<Length> param must be REJECTED — both sides name a \
             concrete dimension and they disagree (task 5766) — and the QUANTITY conflict must \
             route to ArgTypeMismatch, leaving TypeNotConformingToVector owning ARITY/FAMILY only.",
        );
    }

    /// Run `arg_ty` against `param_type` on the empty-registry
    /// `check_fn_arg_conformance` entry point and return whatever it emitted.
    ///
    /// Every probe of the QUANTITY-SLOT rule (tasks 5766 / 6159) reaches the
    /// walker through here, via [`assert_quantity_slot_clean`] or
    /// [`assert_quantity_slot_conflict`] — accepting and rejecting legs alike —
    /// so a future change to how they reach it is made in one place. The
    /// hand-rolled `check_fn_arg_conformance` call sites remaining in this module
    /// pin a DIFFERENT rule (the task 5465 family/arity checks, whose
    /// expectations are `TypeNotConformingToVector` rather than a quantity
    /// conflict) and are deliberately not routed here.
    fn quantity_slot_diags(param_type: Type, arg_ty: Type, arg_name: &str) -> Vec<Diagnostic> {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(ValueCellId::new("Test", "a"), arg_ty);
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            arg_name,
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        diagnostics
    }

    /// Assert that `arg_ty` at `param_type` produces NO diagnostic — the
    /// accepting half of the task 5766 boundary fences.
    fn assert_quantity_slot_clean(param_type: Type, arg_ty: Type, arg_name: &str, why: &str) {
        let diagnostics = quantity_slot_diags(param_type, arg_ty, arg_name);
        assert_eq!(
            diagnostics.len(),
            0,
            "{why}\ngot {} diagnostic(s): {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// Rejecting sibling of [`assert_quantity_slot_clean`]: assert that `arg_ty`
    /// at `param_type` produces exactly one `ArgTypeMismatch`, at
    /// `Severity::Error`, naming `expected_param_quantity` after
    /// `requires quantity` and `expected_arg_quantity` after `has quantity`.
    /// Returns the diagnostics so a probe can pin more without compiling twice.
    ///
    /// EVERY pair in this signature runs PARAM-then-ARG — the two `Type`s, then
    /// the two expected quantity renderings — so a call site reads in one
    /// direction top to bottom and the strings cannot silently run counter to the
    /// types directly above them.
    ///
    /// The two renderings are the DISCRIMINATING part of the message and are
    /// written by hand at each probe rather than derived from the `Type`s under
    /// test: an expectation computed by the same extraction the emitter uses
    /// could not detect a transposition. They also pin the QUANTITY phrasing that
    /// separates this emitter from `emit_arg_type_mismatch`. The label
    /// expectation IS derived from those same two hand-written quantities — a
    /// fixed two-slot rendering, so a transposition still swaps both
    /// substitutions — and the WHOLE message is pinned exactly once, by
    /// `dimensionless_quantity_param_rejects_dimensioned_vector_arg`, whose two
    /// quantities differ asymmetrically (`Scalar[m]` vs `Real`) so a
    /// transposition there cannot render as a no-op.
    ///
    /// For a probe whose types are WRAPPERS (the `List<Vector3<…>>` recursion
    /// probe), the quantities named are the INNER ones the rule fires on.
    ///
    /// This pins the fn-call entry point this scaffold drives, which sets
    /// `Severity::Error`. The ctor entry sets [`CTOR_FIELD_CONFORMANCE_SEVERITY`]
    /// instead (Warning at α, `Error` since δ / task #5306, so the two entries no
    /// longer split on severity) and is pinned by the `.ri` fixtures in
    /// `struct_ctor_field_conformance_tests.rs`.
    fn assert_quantity_slot_conflict(
        param_type: Type,
        arg_ty: Type,
        arg_name: &str,
        expected_param_quantity: &str,
        expected_arg_quantity: &str,
        why: &str,
    ) -> Vec<Diagnostic> {
        let diagnostics = quantity_slot_diags(param_type, arg_ty, arg_name);
        assert_eq!(
            diagnostics.len(),
            1,
            "{why}\ngot {} diagnostic(s): {:?}",
            diagnostics.len(),
            diagnostics,
        );
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "{why}\na quantity conflict must route to ArgTypeMismatch, got {:?}",
            diagnostics[0].code,
        );
        assert_eq!(
            diagnostics[0].severity,
            Severity::Error,
            "{why}\nthe fn-call entry point sets Severity::Error (the ctor entry sets the \
             CTOR_FIELD_CONFORMANCE_SEVERITY knob, Error since δ); the quantity rule inherits \
             whichever the walk was entered with. Got {:?}",
            diagnostics[0].severity,
        );
        // The message must name the QUANTITY slots, not just the whole types: at
        // the Matrix/Tensor arm the family/arity difference the whole-type
        // rendering leads with (`Tensor2x3` vs `Matrix3x3`) is exactly what this
        // arm ACCEPTS, so it points the reader away from the actual cause.
        //
        // The two anchored fragments below are what DISCRIMINATES a correct
        // emission from a transposed one; the surrounding prose is pinned whole
        // exactly once, at the flagship probe named in this fn's doc.
        let has_quantity = format!("has quantity '{expected_arg_quantity}'");
        let requires_quantity = format!("requires quantity '{expected_param_quantity}'");
        assert!(
            diagnostics[0].message.contains(&has_quantity)
                && diagnostics[0].message.contains(&requires_quantity),
            "{why}\nthe message must name the ARG's quantity slot after `has quantity` \
             ({has_quantity:?}) and the PARAM's after `requires quantity` \
             ({requires_quantity:?}) — transposed, it reads as the exact opposite of the rule \
             that fired. Got: {:?}",
            diagnostics[0].message,
        );
        assert_eq!(
            diagnostics[0].labels.len(),
            1,
            "{why}\nthe quantity diagnostic carries exactly one span label, got {:?}",
            diagnostics[0].labels,
        );
        assert_eq!(
            diagnostics[0].labels[0].message,
            format!(
                "quantity mismatch: expected '{expected_param_quantity}', \
                 got '{expected_arg_quantity}'"
            ),
            "{why}\nthe label must name the PARAM slot as `expected` and the ARG slot as `got` — \
             transposed, it reads as the exact opposite of the rule that fired",
        );
        diagnostics
    }

    /// FENCE (a-i), task 5766 unknown-ness boundary: a dimension-GENERIC param
    /// slot accepts a concretely-dimensioned arg.
    ///
    /// `Vec3<Q: Dimension>` (`stdlib/trajectory.ri:103`) resolves its quantity to
    /// `Type::ScalarParam("Q")`. A generic slot names no dimension, so there is
    /// nothing to conflict with and the rule must stay silent — otherwise every
    /// instantiation of a dimension-generic vector wrapper would warn.
    #[test]
    fn generic_quantity_param_accepts_dimensioned_vector_arg() {
        assert_quantity_slot_clean(
            Type::vec3(Type::ScalarParam("Q".to_string())),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            },
            "axis",
            "a Vector3<Length> arg at a dimension-GENERIC Vector3<Scalar<Q>> param must stay \
             CLEAN — a ScalarParam slot names no concrete dimension, so \
             param_quantity_slot_dimension yields None and there is nothing to conflict with \
             (task 5766; task 6159's param-side tightening covers a dimensionless Scalar only, \
             not a ScalarParam).",
        );
    }

    /// FENCE (a-ii), the SAME boundary in the opposite direction: a concretely
    /// dimensioned param slot accepts a dimension-GENERIC arg.
    ///
    /// Pinned separately from (a-i) because a rule that compared only one side
    /// against `None` would pass (a-i) and fail here. Together they pin that
    /// [`emit_if_quantity_conflict`] requires BOTH sides to name a dimension —
    /// which is also how the `is_numeric_placeholder_leaf` unknown-ness fence
    /// (D4-5) survives at these arms.
    #[test]
    fn dimensioned_quantity_param_accepts_generic_vector_arg() {
        assert_quantity_slot_clean(
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::ScalarParam("Q".to_string())),
            },
            "axis",
            "a dimension-GENERIC Vector3<Scalar<Q>> arg at a Vector3<Length> param must stay \
             CLEAN — the rule requires BOTH sides to name a concrete dimension before it can \
             fire (task 5766); widening it here would re-open the D4-5 unknown-ness hole.",
        );
    }

    /// THE PARAM-SIDE RULING (task 6159): a DIMENSIONLESS param slot REJECTS a
    /// concretely-dimensioned arg.
    ///
    /// This is where the ruling's asymmetry is pinned from the strict side.
    /// (a-i)/(a-ii) hold the rule tolerant when either side declines to name a
    /// dimension via `ScalarParam`, and
    /// `vector_param_accepts_dimensionless_vector_arg` holds the ARG side
    /// tolerant when it is a dimensionless `Scalar`. The PARAM side is not the
    /// same kind of thing: `.ri` REQUIRES the quantity type-arg
    /// (`type_resolution.rs`'s `Vector3`/`Matrix`/`Tensor` arities), so a param's
    /// slot is always a WRITTEN DECLARATION and never `infer_type()` output —
    /// there is no absent spelling that could silently default to dimensionless.
    /// Post task 5848 (merged), `Vector3<Dimensionless>` on a direction field is
    /// a deliberate assertion of unit-lessness rather than a grammar workaround,
    /// so a `Vector3<Length>` arg there is a real error.
    ///
    /// Ruling and its basis: `crates/reify-core/src/ty.rs`. The ctor-path `.ri`
    /// twin of this same cell is
    /// `vec3_dimensioned_at_dimensionless_vector_param_warns_arg_type_mismatch`
    /// (`struct_ctor_field_conformance_tests.rs`), at `Severity::Error` since
    /// δ / task #5306 — so the two paths no longer split on severity.
    #[test]
    fn dimensionless_quantity_param_rejects_dimensioned_vector_arg() {
        let diagnostics = assert_quantity_slot_conflict(
            Type::vec3(Type::dimensionless_scalar()),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            },
            "direction",
            "Real",
            "Scalar[m]",
            "a Vector3<Length> arg at a DIMENSIONLESS Vector3<Dimensionless> param must be \
             REJECTED (task 6159) — a param's quantity slot is a written declaration, never an \
             erasure, so `Dimensionless` there asserts unit-lessness. The ARG-side tolerance \
             leg is deliberately untouched (`vector_param_accepts_dimensionless_vector_arg`).",
        );
        // The ONE whole-message pin for `emit_if_quantity_conflict`'s rendering
        // — see `assert_quantity_slot_conflict`'s doc for why it is here and only
        // here. This probe is chosen because its two quantities differ
        // ASYMMETRICALLY (`Scalar[m]` arg, `Real` param), so a transposition
        // anywhere in the rendering cannot come out as a no-op. Every other probe
        // asserts the two discriminating fragments only, so a rewording of the
        // surrounding prose is a ONE-site edit.
        //
        // Note "the compared shape … is otherwise accepted at …", not "the arg
        // type … at param type …": on the wrapper recursion the pair named is the
        // ELEMENT pair, which is not what the user wrote
        // (`list_wrapped_vector_param_rejects_cross_dimension_element`).
        assert_eq!(
            diagnostics[0].message,
            "argument 'direction' has quantity 'Scalar[m]' but param 'direction' requires \
             quantity 'Real' (the compared shape 'Vector3<Scalar[m]>' is otherwise accepted at \
             'Vector3<Real>'; only the quantity slot disagrees)",
            "the quantity diagnostic's WHOLE rendering is pinned here (and only here): it must \
             name the arg's quantity after `has quantity`, the param's after `requires \
             quantity`, and claim ACCEPTANCE of the compared shape rather than shape EQUALITY \
             — two of the three arms reach it with a genuinely different shape.",
        );
    }

    /// THE PARAM-SIDE RULING (task 6159) at the `Matrix`/`Tensor` arm.
    ///
    /// [`emit_if_quantity_conflict`] is shared by all three shape arms, but the
    /// ruling's new cell was pinned only at the `Vector` one, and
    /// `crates/reify-core/src/ty.rs` asserts a consequence specific to THIS arm:
    /// a dimensioned `matrix(…)` at a `Matrix<M, N, Dimensionless>` param is
    /// rejected, where before only a dimensioned param slot could trip it. That
    /// claim is only true if this arm actually reaches the STRICT param-side
    /// predicate — the arm's one other quantity fixture
    /// (`matrix_builtin_cross_dimension_at_inertia_param_errors_…` in
    /// `struct_ctor_field_conformance_tests.rs`) is concrete×concrete and was
    /// already green under task 5766's symmetric rule, so it cannot tell the two
    /// predicates apart. Without this probe, routing the arm through the
    /// TOLERANT predicate on both sides would leave every other test green.
    #[test]
    fn dimensionless_quantity_matrix_param_rejects_dimensioned_tensor_arg() {
        assert_quantity_slot_conflict(
            Type::Matrix {
                m: 3,
                n: 3,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
            Type::tensor(
                2,
                3,
                Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                },
            ),
            "jacobian",
            "Real",
            "Scalar[m]",
            "a Tensor2x3<Length> arg at a DIMENSIONLESS Matrix3x3<Dimensionless> param must be \
             REJECTED (task 6159): this arm applies no arity check, so the quantity slot is the \
             only separator — and the ty.rs ruling's Matrix/Tensor consequence is stated in terms \
             of exactly this cell.",
        );
    }

    /// THE PARAM-SIDE RULING (task 6159) at the `Point` arm — sibling of
    /// `dimensionless_quantity_matrix_param_rejects_dimensioned_tensor_arg`,
    /// closing the third of the three arms [`emit_if_quantity_conflict`] serves.
    ///
    /// Constructed as a direct `Type` so the probe reaches the walker without
    /// depending on `math_fn_result_type`'s agreeing-components quantity
    /// inference — NOT because a `.ri` source cannot produce a dimensioned
    /// `Type::Point` arg. That older premise expired when task 5344
    /// (`3c4ee5e9ac`) claimed `point3` / `point2` into the math construction
    /// family; it must not be re-asserted.
    /// Rule and the measured `.ri`-level cells: the "Point / Vector
    /// quantity-slot convention" section of `crates/reify-core/src/ty.rs`. The
    /// stale sites that section used to point at were corrected by task 6436;
    /// there are none outstanding.
    ///
    /// The `.ri` twin of this exact cell is
    /// `point3_dimensioned_at_dimensionless_point_param_errors_arg_type_mismatch`
    /// (`struct_ctor_field_conformance_tests.rs`, ctor path, `Severity::Error`
    /// since δ / task #5306), which pins the inference chain this direct-`Type`
    /// probe deliberately
    /// bypasses.
    #[test]
    fn dimensionless_quantity_point_param_rejects_dimensioned_point_arg() {
        assert_quantity_slot_conflict(
            Type::point3(Type::dimensionless_scalar()),
            Type::Point {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            },
            "origin",
            "Real",
            "Scalar[m]",
            "a Point3<Length> arg at a DIMENSIONLESS Point3<Dimensionless> param must be \
             REJECTED (task 6159): the arity check passes, so the STRICT param-side predicate \
             decides — the same cell the Vector arm's probe pins, reached through a different \
             arm.",
        );
    }

    /// FENCE (b), task 5766: an `Int` quantity slot stays CLEAN.
    ///
    /// Pins the ty.rs convention's statement that `Value::Vector::infer_type()`
    /// may yield a `Type::Int` quantity for a unit-less component vector — the
    /// shape `vec3(0, 1, 0)` takes. `Type::Int` names no dimension, so the rule
    /// is silent for the same structural reason as the dimensionless leg.
    #[test]
    fn int_quantity_vector_arg_at_dimensioned_param_stays_clean() {
        assert_quantity_slot_clean(
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Int),
            },
            "axis",
            "a Vector3<Int> arg at a Vector3<Length> param must stay CLEAN — the ty.rs \
             quantity-slot convention records that Value::Vector::infer_type() may yield an \
             Int quantity, and Int names no dimension (task 5766).",
        );
    }

    /// FENCE (b-param), task 6159: an `Int` PARAM quantity slot stays CLEAN.
    ///
    /// The PARAM-side half of fence (b) directly above, which puts `Type::Int` in
    /// the ARG slot and so constrains [`arg_quantity_slot_dimension`] only.
    /// `crates/reify-core/src/ty.rs` rules that on the param side "`Type::Int`
    /// and `Type::ScalarParam` still name nothing", and only the `ScalarParam`
    /// half of that was pinned param-side (fence (a-i),
    /// `dimensioned_quantity_param_accepts_generic_vector_arg`). Without this
    /// probe, adding `Type::Int => Some(DimensionVector::DIMENSIONLESS)` to
    /// [`param_quantity_slot_dimension`] leaves every other test in both files
    /// green.
    ///
    /// This is a fence on the PREDICATE's shape, not on a reachable source cell:
    /// `Vector3<Int>` is NOT writable in `.ri`. `resolve_parameterized_builtin_type`'s
    /// `"Vector3" if type_args.len() == 1` arm routes a concrete type-arg through
    /// `resolve_type_alias_expr_to_dimension` → `resolve_dimension_type`, which
    /// accepts `NAMED_DIMENSIONS` plus `"Dimensionless"` / `"Real"` only — `Int`
    /// is in neither, so `param v : Vector3<Int>` is a resolution error and never
    /// yields a `Type::Int` param quantity. (The `resolve_type_name` `"Int"` →
    /// `Type::Int` mapping applies to a BARE type position, `param n : Int`, not
    /// to a dimension slot.) `Type::Int` quantity slots arise on the ARG side
    /// only, via `math_fn_result_type`'s `vec3`/`point3` arm — which is what
    /// fence (b) directly above covers. That survives task 5889's narrowing of
    /// that arm because its heterogeneity check compares DIMENSIONS, not
    /// `Type`s: `Int` and `Real` are both dimensionless, so they agree and the
    /// `Type::Int` slot is kept rather than degraded away. Pinned by
    /// `vec_result_type_int_and_real_elements_agree_and_keep_int_quantity`
    /// (`math_signatures.rs`).
    ///
    /// Note what this does NOT say: the param-side strictness ruling is about a
    /// dimensionless `Type::Scalar` ONLY. `Int` is a different type, not a
    /// dimensionless spelling of `Scalar`, so it stays outside the tightening on
    /// BOTH sides.
    #[test]
    fn int_quantity_vector_param_accepts_dimensioned_vector_arg() {
        assert_quantity_slot_clean(
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Int),
            },
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            "axis",
            "a Vector3<Length> arg at a Vector3<Int> PARAM must stay CLEAN — a Type::Int param \
             quantity slot names no dimension (task 6159), because the STRICT param-side \
             predicate is strict about a dimensionless Scalar ONLY. Widening it to Int would \
             make this cell reject.",
        );
    }

    // FENCE (c) — the `Type::Field` HOLD — is pinned by
    // `field_param_given_erased_analytical_field_stays_clean`
    // (`struct_ctor_field_conformance_tests.rs`) and ruled in
    // `crates/reify-core/src/ty.rs`; not re-fixtured here.

    /// FENCE (d), task 5766: the `Vector` arm's rank-1 `Type::Tensor` arg leg
    /// REJECTS a conflicting dimension.
    ///
    /// `Tensor<1,3,…>` is the one non-`Type::Vector` shape the `Vector` arm
    /// accepts (Rules 1a/1b make the two interconvertible), so it reaches the
    /// quantity comparison by a different route than
    /// `vector_param_rejects_cross_dimension_vector_arg` — and nothing else
    /// pinned it.
    #[test]
    fn vector_param_rejects_cross_dimension_rank1_tensor_arg() {
        assert_quantity_slot_conflict(
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            Type::tensor(
                1,
                3,
                Type::Scalar {
                    dimension: DimensionVector::MASS,
                },
            ),
            "axis",
            "Scalar[m]",
            "Scalar[kg]",
            "a Tensor1x3<Mass> arg at a Vector3<Length> param must be REJECTED: the Vector arm \
             accepts a rank-1 Tensor as vector-shaped, so the quantity slots are then compared \
             and both name a concrete — and different — dimension (task 5766).",
        );
    }

    /// FENCE (e), task 5766: the `Matrix`/`Tensor` arm's `Type::Vector` arg leg
    /// REJECTS a conflicting dimension.
    ///
    /// `tensor_param_given_vector_stays_clean`
    /// (`struct_ctor_field_conformance_tests.rs`) covers only the MATCHING case
    /// for this leg, so without this probe the `Vector`-arg reject route through
    /// the `Matrix`/`Tensor` arm has no coverage.
    #[test]
    fn matrix_param_rejects_cross_dimension_vector_arg() {
        assert_quantity_slot_conflict(
            Type::Matrix {
                m: 3,
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::MOMENT_OF_INERTIA,
                }),
            },
            Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
            "inertia",
            "Scalar[m^2·kg]",
            "Scalar[m]",
            "a Vector3<Length> arg at a Matrix<3,3,MomentOfInertia> param must be REJECTED: \
             Rules 1a/1b make Vector conforming at this arm, so the quantity slots are then \
             compared and both name a concrete — and different — dimension (task 5766).",
        );
    }

    /// FENCE (f), task 5766: the `Matrix`/`Tensor` arm's `Type::Matrix` arg leg
    /// REJECTS a conflicting dimension.
    ///
    /// `matrix_param_accepts_matrix_arg_without_arity_check` below builds a
    /// DIMENSIONLESS `Matrix` arg, so it exercises only
    /// [`arg_quantity_slot_dimension`]'s `None` return; the family's one `.ri`-level
    /// tightening fixture routes through a `Type::Tensor` arg. This is the
    /// `Type::Matrix`-arg reject leg.
    ///
    /// Note the deliberate arity AGREEMENT here (`Matrix<3,3>` at `Matrix<3,3>`):
    /// the arm applies no arity check, so the quantity slot is the only thing
    /// that can separate these two types.
    #[test]
    fn matrix_param_rejects_cross_dimension_matrix_arg() {
        assert_quantity_slot_conflict(
            Type::Matrix {
                m: 3,
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::MOMENT_OF_INERTIA,
                }),
            },
            Type::Matrix {
                m: 3,
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            },
            "inertia",
            "Scalar[m^2·kg]",
            "Scalar[m]",
            "a Matrix<3,3,Length> arg at a Matrix<3,3,MomentOfInertia> param must be REJECTED: \
             the families agree and the arm applies no arity check, so the quantity slot is the \
             only separator and both sides name a concrete — and different — dimension \
             (task 5766).",
        );
    }

    /// FENCE (g), task 5766: the rule fires through the WRAPPER recursion, exactly
    /// once.
    ///
    /// The quantity check lives inside `walk_param_against_arg_type`, which the
    /// `Option`/`List`/`Set`/`Map` arms re-enter on their inner types, so a
    /// `List<Vector3<Length>>` param reaches the `Vector` arm one level down. Every
    /// other quantity-slot probe here enters that arm directly, so nothing pinned
    /// the recursive route — the shape most likely to expose a DOUBLE emission
    /// (wrapper-shape diagnostic plus quantity diagnostic) or a MISSED one if the
    /// recursion or the hoisted anti-cascade guard is changed.
    /// `assert_quantity_slot_conflict` asserts `len() == 1`, so both failure modes
    /// are caught here.
    #[test]
    fn list_wrapped_vector_param_rejects_cross_dimension_element() {
        assert_quantity_slot_conflict(
            Type::List(Box::new(Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }))),
            Type::List(Box::new(Type::Vector {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::MASS,
                }),
            })),
            "axes",
            // The INNER element quantities, PARAM then ARG as everywhere else:
            // the rule fires one level down,
            // after the `List`/`List` wrapper arm has recursed. That is also why
            // the message says "the compared shape" rather than "the arg type" —
            // the shapes it names here are `Vector3<…>`, which appear nowhere in
            // a source that wrote `List<Vector3<…>>`.
            "Scalar[m]",
            "Scalar[kg]",
            "a List<Vector3<Mass>> arg at a List<Vector3<Length>> param must be REJECTED with \
             EXACTLY ONE diagnostic: the List/List wrapper arm recurses lockstep into the \
             element types, so the quantity rule fires at the Vector arm one level down and \
             the wrapper walk must not also emit a shape diagnostic on top of it (task 5766).",
        );
    }

    /// Arity leg of the `Type::Point` arm (task 5465, family 1): a `Point{n:2}`
    /// arg against a `Point3<Length>` param is exactly one `ArgTypeMismatch`.
    ///
    /// **The param-side asymmetry, and what does NOT follow from it.** The
    /// surface language has no `Point2` PARAM spelling:
    /// `resolve_parameterized_builtin_type` recognises `Point3` only — its arms
    /// are `"Point3" if type_args.len() == 1` (`type_resolution.rs`, two sites),
    /// with no `"Point2"` arm anywhere. That is why this probe's param type is
    /// `Point3<Length>` and why the `.ri` twin's param must be spelled the same
    /// way. It does NOT follow that the arity rule is unreachable from
    /// inline-source fixtures — a param spelling constrains PARAMS, not ARGS,
    /// and that inference must not be re-asserted over the arg side. The `.ri`
    /// twin named below carries the detail.
    ///
    /// The arity leg IS now pinned from `.ri` source, by
    /// `point2_arg_at_point3_param_errors_arity_arg_type_mismatch`
    /// (`struct_ctor_field_conformance_tests.rs`). This probe stays as the
    /// direct-`Type` seam of the same pair the cross-dimension probe above
    /// describes: constructed directly so it reaches the walker without
    /// depending on `math_fn_result_type`'s name-suffix `n` inference, while the
    /// `.ri` fixture is the one that would notice that inference ceasing to
    /// produce a `Type::Point { n: 2, .. }` at all. (Task 5889 narrowed that
    /// arm's QUANTITY slot and deliberately left `n` alone — it still comes from
    /// the name suffix.)
    ///
    /// Sibling of `vector_param_rejects_wrong_arity_vector_arg`, which is also a
    /// direct-`Type` arity probe — note its own doc claims no erasure premise,
    /// so nothing there needs the correction this block carries. The `Vector`
    /// arm has the same asymmetry (no `Vector2` param spelling; `vec2` claimed
    /// into the same collapsed arm by 5344) and now has the matching `.ri` twin,
    /// `vec2_arg_at_vector3_param_errors_arity_type_not_conforming`. The two
    /// arms' arity legs differ in EMITTER, not in reachability: `Point` routes
    /// arity through `emit_arg_type_mismatch`, `Vector` keeps its bespoke
    /// `TypeNotConformingToVector`, and each `.ri` twin asserts its own code so
    /// the split cannot drift.
    #[test]
    fn point_param_rejects_wrong_arity_point_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "p"),
            Type::Point {
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        });
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "origin",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 ArgTypeMismatch for Point2 arg vs Point3<Length> param \
             (arity mismatch), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "expected ArgTypeMismatch, got {:?}",
            diagnostics[0].code,
        );
    }

    /// Self-accept leg of the `Matrix`/`Tensor` arm (task 5465, family 2): a
    /// genuinely `Type::Matrix`-typed value cell at a `Matrix` param.
    ///
    /// The family's integration probes all exercise the LOOSE accepts — the
    /// nested-list-literal spelling and the `Vector` → `Tensor<1,…>` conversion —
    /// so nothing pinned that the nominal family accepts ITSELF. Note the
    /// deliberate m/n difference (`Matrix<2,2>` arg at a `Matrix<3,3>` param):
    /// the arm applies NO arity check, because `List<List<Real>>` — the
    /// idiomatic spelling — carries no element counts, so an arity rule would be
    /// unenforceable for exactly the shape that matters. This test is what makes
    /// that asymmetry with the `Point`/`Vector` arms a pinned fact rather than
    /// an unstated consequence.
    #[test]
    fn matrix_param_accepts_matrix_arg_without_arity_check() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "m"),
            Type::Matrix {
                m: 2,
                n: 2,
                quantity: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::Matrix {
            m: 3,
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        };
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "inertia",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "a Matrix-typed arg must be accepted at a Matrix param (nominal family self-accept, \
             loose quantity, NO arity check), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// Lambda leg of the `Type::Field` arm (task 5465, family 3): a
    /// `Type::Function` arg is a legitimate spelling of a field-shaped arg at a
    /// `Field` slot.
    ///
    /// That accept is an explicit, deliberate branch in the arm and had zero
    /// coverage — nothing would have noticed it being dropped, or the arm being
    /// rewritten to reject lambdas. The integration probes for this family cover
    /// only the erased-`field def` accept and the `String` value floor.
    #[test]
    fn field_param_accepts_function_arg() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "f"),
            Type::Function {
                params: vec![Type::dimensionless_scalar()],
                return_type: Box::new(Type::dimensionless_scalar()),
            },
        );
        let param_type = Type::Field {
            domain: Box::new(Type::point3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
            codomain: Box::new(Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        };
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "mode_shape",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &[],
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "a lambda (Type::Function) arg must be accepted at a Field param, got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// A minimal declared-enum table for the two erasure-symmetry probes below.
    ///
    /// Only `EnumDef::name` is load-bearing here — [`base_enum_name`]'s
    /// membership test is what distinguishes an applied ENUM from an applied
    /// generic STRUCTURE (`Coupling<T>`), and it reads nothing else.
    fn enum_table(names: &[&str]) -> Vec<reify_ir::EnumDef> {
        names
            .iter()
            .map(|n| reify_ir::EnumDef {
                name: (*n).to_string(),
                variants: vec![],
                doc: None,
                type_params: vec![],
            })
            .collect()
    }

    /// REVERSE leg of the enum-erasure tolerance (task 5465 amendment): a param
    /// declared as the BARE `Type::Enum("Result")` supplied an arg whose
    /// `result_type` is the APPLIED `Type::Applied { name: "Result", .. }` must
    /// be accepted.
    ///
    /// **Why this is an in-module unit test.** The integration probes in
    /// `struct_ctor_field_conformance_tests.rs` cover the forward leg (applied
    /// param ← bare-enum arg), which is what enum erasure produces from a
    /// constructed variant. This pairing is the mirror image, reachable from a
    /// `ValueRef` to a cell declared with the applied annotation or from a fn
    /// whose return annotation is the applied form (cf.
    /// `examples/m6_result_recovery.ri:30`); constructing the two `Type`s
    /// directly pins it without depending on which surface spellings happen to
    /// survive inference today.
    ///
    /// It is a REGRESSION fence, not a new capability: the pairing was silent
    /// before `Type::Enum(_)` was promoted, and the first cut of family 4 made
    /// it a spurious `ArgTypeMismatch` because `enum_payload_compatible` matches
    /// only a bare `Type::Enum` on the supplied side. Routing both sides through
    /// [`base_enum_name`] restored the acceptance symmetrically. `Severity` here
    /// is `Error` (this entry point is the fn-call path), which is exactly why
    /// the false positive mattered: it failed the compile rather than warning.
    #[test]
    fn enum_param_accepts_applied_enum_arg_of_same_base() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let enum_defs = enum_table(&["Result"]);
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "r"),
            Type::Applied {
                name: "Result".to_string(),
                args: vec![
                    Type::Scalar {
                        dimension: DimensionVector::LENGTH,
                    },
                    Type::String,
                ],
            },
        );
        let param_type = Type::Enum("Result".to_string());
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "r",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &enum_defs,
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "a bare Enum(\"Result\") param given an Applied{{\"Result\"}} arg must be accepted — \
             the erasure tolerance is symmetric, and type_compatible has no Applied-vs-Enum arm \
             in EITHER direction. Got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }

    /// Fence on the reverse leg: symmetry must not become a blanket enum bypass.
    ///
    /// A bare `Enum("Hue")` param supplied an applied `Result<…>` arg resolves
    /// to two DIFFERENT base names, so it must still be exactly one
    /// `ArgTypeMismatch` — the same verdict the forward-direction cross-enum
    /// probe (`enum_param_given_wrong_enum_errors_arg_type_mismatch`) pins.
    #[test]
    fn enum_param_rejects_applied_enum_arg_of_different_base() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let enum_defs = enum_table(&["Hue", "Result"]);
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "r"),
            Type::Applied {
                name: "Result".to_string(),
                args: vec![Type::String, Type::String],
            },
        );
        let param_type = Type::Enum("Hue".to_string());
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "c",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &enum_defs,
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly 1 ArgTypeMismatch for Applied{{\"Result\"}} arg vs Enum(\"Hue\") \
             param (different base enums), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
        assert_eq!(
            diagnostics[0].code,
            Some(DiagnosticCode::ArgTypeMismatch),
            "expected ArgTypeMismatch, got {:?}",
            diagnostics[0].code,
        );
    }

    /// Fence on [`base_enum_name`]'s `enum_defs` membership test at THIS arm: an
    /// applied generic STRUCTURE (`Coupling<T>`) is spelled `Type::Applied` too,
    /// and must NOT be treated as an enum.
    ///
    /// With "Coupling" absent from the enum table both sides resolve to `None`,
    /// so the leaf falls through to `general_leaf_param_family_is_validated`,
    /// which does not vet `Type::Applied` — the pre-5465 silence for applied
    /// generic structures is preserved. Without the membership test this would
    /// instead take the enum branch.
    #[test]
    fn applied_generic_structure_param_is_not_treated_as_enum() {
        let template_registry: HashMap<String, &TopologyTemplate> = HashMap::new();
        let trait_registry: HashMap<String, &CompiledTrait> = HashMap::new();
        let enum_defs = enum_table(&["Result"]);
        let compiled_arg = CompiledExpr::value_ref(
            ValueCellId::new("Test", "c"),
            Type::Enum("Coupling".to_string()),
        );
        let param_type = Type::Applied {
            name: "Coupling".to_string(),
            args: vec![Type::String],
        };
        let mut diagnostics: Vec<Diagnostic> = vec![];
        check_fn_arg_conformance(
            &param_type,
            "c",
            &compiled_arg,
            SourceSpan::empty(0),
            ConformanceRegistries {
                templates: &template_registry,
                traits: &trait_registry,
                enum_defs: &enum_defs,
            },
            &mut diagnostics,
        );
        assert_eq!(
            diagnostics.len(),
            0,
            "an applied generic STRUCTURE param must stay outside the enum branch and fall \
             through the allowlist silently (pre-5465 behaviour), got {}: {:?}",
            diagnostics.len(),
            diagnostics,
        );
    }
}
