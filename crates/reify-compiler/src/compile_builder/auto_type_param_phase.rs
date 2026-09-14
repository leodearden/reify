//! Phase: resolve `auto:` / `auto(free):` type-arguments at sub-component
//! call-sites (task 3558, B1 — the compile-pipeline resolver call-site).
//!
//! Runs between [`super::entities_phase::phase_entities`] and
//! [`super::entities_phase::phase_pending_bound_checks`]. By the time this phase
//! runs, `ctx.templates` is fully populated, so target templates (and their
//! `type_params`) are reachable by name even when forward-referenced from the
//! use site — the same deferred-resolution rationale as the `PendingBoundCheck`
//! post-pass.
//!
//! For each [`AutoResolutionRequest`] queued during `phase_entities`, the phase:
//!   1. looks up the target template and builds one [`AutoTypeParam`] per
//!      `auto:` clause (indexing `target.type_params` by the clause position and
//!      pairing with the clause's `bound` / `free` / `span`),
//!   2. dispatches [`resolve_auto_type_params_with_backtracking`] (v0.2 DFS) with
//!      a [`CompileTimeIndeterminateChecker`] and the two `reify_config` caps,
//!   3. on success, rewrites the matching `SubComponentDecl.type_args[position]`
//!      placeholder (`Type::TypeParam("__auto_<bound>")`) to a concrete
//!      `Type::StructureRef(resolved_template_name)` so the downstream
//!      bound-check sees the resolved candidate — this per-position rewrite
//!      applies unconditionally, independent of step 3a below,
//!      3a. separately, monomorph synthesis (cloning `target`, clearing its
//!      `type_params`, and substituting `Type::TypeParam` → `Type::StructureRef`
//!      into the clone's cells/exprs) additionally requires FULL coverage —
//!      every entry of `target.type_params` must be bound in `sigma`. An
//!      explicitly-supplied type-arg never produces an `auto:` clause, so a
//!      mixed use-site like `Widget<SealA, auto: Gasket>()` would otherwise
//!      leave a declared type parameter out of `sigma` without ever entering
//!      the per-param resolver at all. Before coverage is measured, `sigma`
//!      is seeded from every non-`auto:` position's already-resolved
//!      `SubComponentDecl.type_args` entry — an explicitly-supplied type-arg
//!      is not unbound, only unbound *by this resolver* — so full coverage,
//!      and therefore synthesis, is now the norm rather than the exception
//!      for mixed use-sites. The residual skip-synthesis path is reached for
//!      a genuinely unbound `auto:` param (shapes A/B) or an explicit
//!      type-arg that is not a concrete `Type::StructureRef` and so cannot
//!      be seeded; either way the use-site is left pointing at the generic
//!      template. Shapes A/B stay silent here — they already carry the
//!      resolver's own `NoCandidate` / `Ambiguous` error — while the
//!      un-seedable residual is diagnosed with a dedicated compile error
//!      instead of failing silently (#6854),
//!   4. accumulates `(param_name, template_name)` substitution pairs across all
//!      requests, deduping first-wins, into `ctx.auto_type_substitution`.
//!
//! Resolution diagnostics (NoCandidate / Ambiguous / NonUnique / depth-bound /
//! cap-fallback) are pushed into `ctx.diagnostics` by the resolver itself.

use std::collections::{HashMap, HashSet};

use reify_core::{ContentHash, Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, Type};
use reify_ir::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction,
};

use crate::CompiledModule;
use crate::auto_type_param::{
    AutoTypeParam, CtorSynthesisResult, check_candidate_constructible,
    resolve_auto_type_params_with_backtracking,
};
use crate::compile_builder::ctx::CompilationCtx;
use crate::compile_builder::traits_phase::build_trait_registry;
use crate::type_resolution::{substitute_expr_result_types, substitute_type_params};
use crate::types::{
    AutoTypeSubstitution, EntityKind, TopologyTemplate, ValueCellKind, mangle_monomorph_name,
    monomorph_name_would_collide,
};

/// A compile-time [`ConstraintChecker`] that returns
/// [`Satisfaction::Indeterminate`] for every input constraint.
///
/// `reify-compiler` is upstream of `reify-constraints` in the workspace DAG, so
/// the real `SimpleConstraintChecker` is unavailable here. Per PRD scope cut 2,
/// Phase B at compile time has an empty `ValueMap` (cells are `Undef`), so the
/// natural semantic verdict is `Indeterminate` — which the resolver's
/// feasibility rule (arch §2.5: only `Violated` rejects) treats as feasible.
/// The resolver therefore picks the lex-first candidate that has not been
/// provably falsified, matching the documented v0.1/v0.2 behaviour. When the
/// v0.2 substitution pass (M-013) lands, this stub can be swapped for a real
/// evaluator without touching the call-site signature.
///
/// Promoted to `pub(crate)` so `lib.rs`'s default-stub wrappers can construct
/// `&CompileTimeIndeterminateChecker` without widening the public API.
pub(crate) struct CompileTimeIndeterminateChecker;

impl ConstraintChecker for CompileTimeIndeterminateChecker {
    fn check(&self, input: &ConstraintInput) -> Vec<ConstraintResult> {
        input
            .constraints
            .iter()
            .map(|(id, _)| ConstraintResult {
                id: id.clone(),
                satisfaction: Satisfaction::Indeterminate,
                diagnostics: ConstraintDiagnostics::default(),
            })
            .collect()
    }

    /// Returns `true` because this is the compile-time stub — the Gap-C honesty
    /// diagnostic (`W_AUTO_TYPE_PARAM_CONSTRAINT_UNEVALUATED`) must be suppressed
    /// on this path.
    ///
    /// See [`reify_ir::ConstraintChecker::is_compile_time_stub`] for the full
    /// design rationale (task 4616 §CHECKER PATH).
    fn is_compile_time_stub(&self) -> bool {
        true
    }
}

/// Apply `substitute_type_params` and `substitute_expr_result_types` to every
/// `ValueCellDecl` in a collection: rewrites `cell_type` and, when present,
/// the root expr and all descendant nodes' `result_type` in `default_expr`.
///
/// Factored out so the same logic can be applied uniformly to
/// `value_cells`, `guarded_groups.members/else_members`, and `ports.members`
/// without repetition.
fn substitute_value_cell_collection(
    cells: &mut Vec<crate::types::ValueCellDecl>,
    sigma: &std::collections::HashMap<String, reify_core::Type>,
) {
    for cell in cells {
        cell.cell_type = substitute_type_params(&cell.cell_type, sigma);
        if let Some(expr) = &mut cell.default_expr {
            substitute_expr_result_types(expr, sigma);
        }
    }
}

/// Apply `substitute_expr_result_types` to every `CompiledConstraint.expr` in
/// a collection.
///
/// Factored out so the same logic can be applied uniformly to
/// `constraints`, `guarded_groups.constraints/else_constraints`, and
/// `ports.constraints` without repetition.
fn substitute_constraint_collection(
    constraints: &mut Vec<crate::types::CompiledConstraint>,
    sigma: &std::collections::HashMap<String, reify_core::Type>,
) {
    for constraint in constraints {
        substitute_expr_result_types(&mut constraint.expr, sigma);
    }
}

/// Drain `ctx.pending_auto_resolutions`, resolve each `auto:` type-arg use-site,
/// rewrite placeholder slots to concrete `StructureRef`s, and aggregate the
/// module's `auto_type_substitution`.
///
/// Early-returns (leaving `ctx.auto_type_substitution` at its empty default)
/// when no `auto:` type-args were declared — the load-bearing empty-substitution
/// invariant that keeps `topology_fingerprint` stable for modules without `auto:`.
///
/// `checker` is threaded in from the caller: the public default entry points
/// pass `&CompileTimeIndeterminateChecker`; callers that inject the real
/// `SimpleConstraintChecker` use the `*_checked` entry point siblings in `lib.rs`.
pub(crate) fn phase_auto_type_param_resolution(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    checker: &dyn ConstraintChecker,
) {
    // Empty-queue early-out: the common case (no `auto:` type-args anywhere).
    // Leaving `auto_type_substitution` at its empty default is load-bearing for
    // cache stability — an empty Vec hashes deterministically and must not shift
    // for modules that declare no `auto:` slots.
    if ctx.pending_auto_resolutions.is_empty() {
        return;
    }
    let requests = std::mem::take(&mut ctx.pending_auto_resolutions);
    // Copy config scalars before the immutable-borrow block — both are `usize`
    // (Copy) so this avoids borrow-checker friction with `&mut ctx.diagnostics`
    // held simultaneously inside the block.
    let max_depth = ctx.auto_type_params.max_depth;
    let max_cross_product_size = ctx.auto_type_params.max_cross_product_size;

    // Pass 1 — resolve every request while holding immutable registry borrows.
    // Collect:
    //   * `rewrites`         — (owner, sub_index, position, resolved_name)
    //                          for the existing type_args[pos]→StructureRef slot rewrite
    //   * `subst_pairs`      — raw (param_name, template_name) aggregation
    //   * `monomorph_clones` — per-use-site (TopologyTemplate, owner, sub_index, mono_name)
    //                          built from target clones with TypeParam→StructureRef substituted
    //
    // `ctx.templates` mutation is deferred to pass 2 so the registry's
    // `&TopologyTemplate` borrows don't conflict with `&mut`. We key rewrites
    // by `sub_index` (not `sub_name`) because match-arm clusters reuse
    // `sub_name` across multiple `SubComponentDecl`s — a name-only `find` would
    // resolve every arm's rewrite to arm[0], silently dropping the rest. The
    // index is captured at the request push site in `entity.rs` (where it
    // equals the about-to-be-pushed-position in the local `sub_components` vec).
    let (rewrites, subst_pairs, monomorph_clones, structure_name_rewrites) = {
        // Template registry: prelude `structure def`s first, then local
        // overrides — identical composition to `phase_pending_bound_checks`.
        let template_registry: HashMap<String, &TopologyTemplate> = prelude
            .iter()
            .flat_map(|m| m.templates.iter())
            .filter(|t| t.entity_kind == EntityKind::Structure)
            .map(|t: &TopologyTemplate| (t.name.clone(), t))
            .chain(ctx.templates.iter().map(|t| (t.name.clone(), t)))
            .collect();
        let trait_registry = build_trait_registry(&ctx.trait_defs, prelude);
        let functions = ctx.resolution_functions.as_slice();
        let diagnostics = &mut ctx.diagnostics;

        // (owner_structure, sub_index, type_args_position, resolved_template_name)
        let mut rewrites: Vec<(String, usize, usize, String)> = Vec::new();
        let mut subst_pairs: Vec<(String, String)> = Vec::new();
        // New monomorph clones to push (deduplicated by mono_name).
        let mut new_mono_templates: Vec<TopologyTemplate> = Vec::new();
        // (owner_structure, sub_index, mono_name) for EVERY use-site, including
        // deduped ones — every use-site must point at its shared monomorph.
        let mut structure_name_rewrites: Vec<(String, usize, String)> = Vec::new();
        // Phase-local dedup set keyed on the full monomorph name
        // (= generic + "$" + ordered candidates).  Clone-once, share across
        // all use-sites that map to the same name.
        let mut created_monomorphs: HashSet<String> = HashSet::new();

        for req in &requests {
            // Look up the instantiated template; an unknown target is handled by
            // the bound-check pass (and leaves the placeholder in place here).
            let target = match template_registry.get(req.target_name.as_str()) {
                Some(t) => *t,
                None => continue,
            };

            // Build one AutoTypeParam per clause, indexing `target.type_params`
            // by clause position (clause N binds target.type_params[N]).
            let mut params: Vec<AutoTypeParam> = Vec::new();
            let mut name_to_position: HashMap<String, usize> = HashMap::new();
            for clause in &req.auto_clauses {
                let tp = match target.type_params.get(clause.position) {
                    Some(tp) => tp,
                    // Arity mismatch — out of scope here; the bound-check pass
                    // reports the missing/excess type-argument.
                    None => continue,
                };
                params.push(AutoTypeParam {
                    name: tp.name.clone(),
                    bounds: vec![clause.bound.clone()],
                    free: clause.free,
                    use_site_span: clause.span,
                });
                name_to_position.insert(tp.name.clone(), clause.position);
            }
            if params.is_empty() {
                continue;
            }

            let outcome = resolve_auto_type_params_with_backtracking(
                &params,
                &template_registry,
                &trait_registry,
                target,
                checker,
                functions,
                max_depth,
                max_cross_product_size,
                diagnostics,
            );

            // Build Σ = {param_name → StructureRef(resolved)} and collect
            // candidates in (position, resolved_name) order for the mangle.
            let mut sigma: HashMap<String, Type> = HashMap::new();
            let mut candidates_by_position: Vec<(usize, String)> = Vec::new();

            for (param_name, template_name) in &outcome.substitution {
                if let Some(&position) = name_to_position.get(param_name.as_str()) {
                    rewrites.push((
                        req.owner_structure.clone(),
                        req.sub_index,
                        position,
                        template_name.clone(),
                    ));
                    sigma.insert(
                        param_name.clone(),
                        Type::StructureRef(template_name.clone()),
                    );
                    candidates_by_position.push((position, template_name.clone()));
                }
                subst_pairs.push((param_name.clone(), template_name.clone()));
            }

            // Seed `sigma` and `candidates_by_position` from this use-site's
            // already-resolved EXPLICIT type-args before measuring coverage
            // below. `entity.rs` only pushes an `AutoClause` for an `Auto`
            // type-arg, so an explicitly-supplied arg (e.g. the `SealA` in
            // `Widget<SealA, auto: Gasket>()`) never enters
            // `outcome.substitution` above, and `sigma` would otherwise stay
            // short of full coverage even though the position is already
            // resolved — just not by this resolver (#6854).
            if let Some(owner) = template_registry.get(req.owner_structure.as_str())
                && let Some(sub) = owner.sub_components.get(req.sub_index)
            {
                for (position, tp) in target.type_params.iter().enumerate() {
                    // Positions this use-site supplied as an `auto:` clause
                    // are the resolver's to bind; a failure there is a
                    // genuine unbound param (shapes A/B), not something to
                    // seed here.
                    if req.auto_clauses.iter().any(|c| c.position == position) {
                        continue;
                    }
                    // An explicitly-supplied type-arg is ALREADY RESOLVED —
                    // just not by this resolver. Seed it so coverage below
                    // sees the truth. A non-`StructureRef` slot (e.g. an
                    // enclosing generic's own `TypeParam`) is left unseeded
                    // and falls through to the partial-coverage skip below.
                    //
                    // When the position carries NO type-arg at all, fall back
                    // to the type parameter's DECLARED DEFAULT (shape D,
                    // #6854 review round 3). This mirrors `effective_arg` in
                    // `check_type_param_bounds` (entity.rs), which treats an
                    // omitted arg with a default as supplied rather than
                    // missing — `TypeParam::default` is a supported language
                    // feature, so `Widget<U: Gasket, T: Seal = SealA>`
                    // instantiated as `Widget<auto: Gasket>()` is a VALID
                    // use-site with T already resolved to `SealA`. Without
                    // this fallback such a use-site reaches residual-partial
                    // coverage and is rejected by the un-seedable diagnostic
                    // below, whose message ("an explicitly-supplied
                    // type-argument that is not a concrete structure") does
                    // not even describe what happened — the arg was omitted.
                    // A default is already-resolved information exactly like
                    // an explicit arg, so seeding it is the correct remedy.
                    let effective_arg: Option<&Type> =
                        sub.type_args.get(position).or(tp.default.as_ref());
                    if let Some(Type::StructureRef(name)) = effective_arg {
                        sigma.insert(tp.name.clone(), Type::StructureRef(name.clone()));
                        candidates_by_position.push((position, name.clone()));
                    }
                }
            }

            // Synthesize a monomorph only when EVERY type parameter DECLARED
            // BY THE TARGET is covered by `sigma` — not merely every
            // `auto:`-clause param. Coverage must be measured against
            // `target.type_params` rather than against `params` (this
            // use-site's auto-clause list): an explicitly-supplied type-arg
            // (e.g. the `SealA` in `Widget<SealA, auto: Gasket>()`) never
            // produces an `AutoClause` at all, so `params.len()` can be
            // strictly less than `target.type_params.len()` even though the
            // omitted param is a real, declared type parameter of `target`
            // (#6854). `sigma`'s keys are always a subset of
            // `target.type_params` names (`name_to_position` above is itself
            // built from `target.type_params`), so this check is exactly
            // "every declared type parameter is bound".
            //
            // The seeding loop directly above fills `sigma` for explicitly-
            // supplied positions too, so a mixed use-site like
            // `Widget<SealA, auto: Gasket>()` now normally reaches full
            // coverage rather than being treated as partial. An explicitly-
            // supplied type-arg is not unbound — it is already resolved,
            // just not by this resolver — so seeding it is the correct
            // remedy, not merely a workaround: skipping synthesis is only a
            // safe degradation when there is a sound generic template to
            // fall back to, and for a mixed use-site there is not.
            // `assert_value_cell_types_representable` walks the HYDRATED
            // GRAPH, not `compiled.templates`, so a generic `Widget` is
            // inert only while nothing points at it — leaving the use-site
            // on the generic is precisely what drags its `Type::TypeParam`
            // cells into the graph and panics at hydration (#6854 review
            // round 2).
            //
            // On residual PARTIAL coverage — a genuinely unbound `auto:`
            // param (shapes A/B), or a non-`auto:` position whose effective
            // type-arg (explicit arg, else declared default) is not a
            // concrete `Type::StructureRef` and so could not be seeded
            // — synthesis must still be skipped: the block below clears
            // `mono.type_params` — advertising the clone as fully concrete —
            // while any cell whose type-param was NOT in `sigma` keeps its
            // raw `Type::TypeParam(name)`. That is the one shape no
            // downstream `type_params.is_empty()` filter can ever detect,
            // since the clone itself claims to have zero free type-params.
            // The un-seedable residual (below) is diagnosed rather than left
            // silent; shapes A/B are not — they already carry the resolver's
            // own `NoCandidate`/`Ambiguous` error.
            let sigma_covers_all_type_params = !target.type_params.is_empty()
                && target
                    .type_params
                    .iter()
                    .all(|tp| sigma.contains_key(tp.name.as_str()));

            // Diagnose the residual un-seedable case: the resolver bound
            // every `auto:`-clause param it was asked to (so this is NOT
            // shape A/B, which already carries its own `NoCandidate` /
            // `Ambiguous` error and must not be double-reported), yet full
            // `target.type_params` coverage is still not reached — meaning
            // a non-`auto:` position's EFFECTIVE type-arg (its explicit arg,
            // else its declared default) was something other than a concrete
            // `Type::StructureRef` (e.g. an enclosing generic's own
            // `TypeParam`, #6854) and the seeding loop above could not bind
            // it. Gating on "the resolver bound everything it was asked to
            // bind" rather than scanning `diagnostics` for newly-pushed
            // errors avoids depending on which `Severity` the resolver
            // assigns each halt reason.
            let all_auto_clause_params_bound =
                params.iter().all(|p| sigma.contains_key(p.name.as_str()));
            if !sigma_covers_all_type_params && all_auto_clause_params_bound {
                let unbound: Vec<&str> = target
                    .type_params
                    .iter()
                    .map(|tp| tp.name.as_str())
                    .filter(|n| !sigma.contains_key(*n))
                    .collect();
                let sub_name = template_registry
                    .get(req.owner_structure.as_str())
                    .and_then(|owner| owner.sub_components.get(req.sub_index))
                    .map(|sub| sub.name.as_str())
                    .unwrap_or("<unknown>");
                let owner_structure = req.owner_structure.as_str();
                let target_name = req.target_name.as_str();
                diagnostics.push(Diagnostic::error(format!(
                    "sub-component '{sub_name}' of '{owner_structure}' instantiates \
                     generic '{target_name}' with a non-`auto:` type-argument that \
                     does not resolve to a concrete structure (type parameter(s) \
                     {unbound:?} could not be bound), so no monomorph can be \
                     synthesized; the sub-component would retain unsubstituted type \
                     parameters at evaluation time"
                )));
            }

            if sigma_covers_all_type_params {
                // Sort by position to guarantee deterministic mangle order
                // regardless of outcome.substitution iteration order.
                candidates_by_position.sort_by_key(|(pos, _)| *pos);
                let ordered_candidates: Vec<String> =
                    candidates_by_position.into_iter().map(|(_, c)| c).collect();
                let mono_name = mangle_monomorph_name(&req.target_name, &ordered_candidates);

                // Defensive collision guard: a pre-existing template named
                // `mono_name` that was NOT created by α in this pass would be
                // silently overwritten in pass-2.  This is impossible from valid
                // `.ri` source (`$` is illegal in identifiers), but guard
                // converts any future compiler regression into a build error.
                //
                // Skip both the clone AND the structure_name rewrite for this
                // use-site — there is no safe target to point the sub at.
                if monomorph_name_would_collide(&ctx.templates, &created_monomorphs, &mono_name) {
                    diagnostics.push(Diagnostic::error(format!(
                        "internal: synthesized monomorph name `{mono_name}` collides with \
                         a pre-existing template (impossible from source; this is a compiler bug)"
                    )));
                    continue;
                }

                // Dedup: clone the template only once per distinct mono_name.
                // `HashSet::insert` returns true on first insertion.
                if created_monomorphs.insert(mono_name.clone()) {
                    // First use-site for this (generic, candidates) pair — build
                    // the monomorph template.
                    let mut mono = target.clone();
                    mono.name = mono_name.clone();
                    // A monomorph has no free type parameters — it is concrete.
                    mono.type_params.clear();
                    // Substitute TypeParam→StructureRef in top-level value_cells.
                    substitute_value_cell_collection(&mut mono.value_cells, &sigma);
                    // Substitute TypeParam in top-level constraint expressions.
                    substitute_constraint_collection(&mut mono.constraints, &sigma);
                    // Substitute TypeParam in guarded-group members, else_members,
                    // constraints, else_constraints, and guard_expr.
                    // This covers `if guard { param seal : T }` patterns (invariant 1).
                    for group in &mut mono.guarded_groups {
                        substitute_expr_result_types(&mut group.guard_expr, &sigma);
                        substitute_value_cell_collection(&mut group.members, &sigma);
                        substitute_constraint_collection(&mut group.constraints, &sigma);
                        substitute_value_cell_collection(&mut group.else_members, &sigma);
                        substitute_constraint_collection(&mut group.else_constraints, &sigma);
                    }
                    // Substitute TypeParam in port members and constraints.
                    for port in &mut mono.ports {
                        substitute_value_cell_collection(&mut port.members, &sigma);
                        substitute_constraint_collection(&mut port.constraints, &sigma);
                        if let Some(expr) = &mut port.frame_expr {
                            substitute_expr_result_types(expr, &sigma);
                        }
                    }
                    // Substitute TypeParam in sub-component type_args so that nested
                    // generic instantiations like `sub inner = Inner<T>()` become
                    // `Inner<StructureRef(c)>` in the monomorph.
                    for sub in &mut mono.sub_components {
                        for arg in &mut sub.type_args {
                            *arg = substitute_type_params(arg, &sigma);
                        }
                    }
                    // δ synthesis: for each top-level Param cell whose ORIGINAL type
                    // (pre-substitution, from `target.value_cells`) is
                    // `Type::TypeParam(name)` with `name ∈ sigma` and no
                    // `default_expr`, synthesize a zero-arg StructureInstanceCtor
                    // default_expr from the resolved candidate's own param defaults.
                    //
                    // Reading the original type from `target.value_cells`
                    // (PRE-substitution) is precise: only auto-resolved type-param
                    // cells are filled, never a concretely-typed `param x : GasketSeal`
                    // that legitimately has no default (design decision 1).
                    //
                    // The synthesized ctor flows through unfold.rs's existing
                    // default branch (line 338) → `eval_structure_instance_ctor` →
                    // `Value::StructureInstance`.
                    for (idx, orig_cell) in target.value_cells.iter().enumerate() {
                        if orig_cell.kind != ValueCellKind::Param
                            || orig_cell.default_expr.is_some()
                        {
                            continue;
                        }
                        // Only fill cells whose original type is a TypeParam that was
                        // resolved in sigma (i.e., an auto-resolved type parameter).
                        // Keep `tp_name` in scope so the NotConstructible arm can look
                        // up the use-site span from `params`.
                        let tp_name = if let Type::TypeParam(n) = &orig_cell.cell_type {
                            n
                        } else {
                            continue;
                        };
                        let candidate_name = match sigma.get(tp_name.as_str()) {
                            Some(Type::StructureRef(cname)) => cname.clone(),
                            _ => continue,
                        };
                        let candidate = match template_registry.get(candidate_name.as_str()) {
                            Some(c) => *c,
                            None => continue,
                        };
                        // Constructible → set synthesized default on the monomorph cell.
                        // NotConstructible → emit E_AUTO_TYPE_PARAM_CANDIDATE_NOT_CONSTRUCTIBLE
                        //   and leave default_expr = None (no partial-Undef instance).
                        match check_candidate_constructible(candidate) {
                            CtorSynthesisResult::Ctor(ctor) => {
                                // mono is a clone of target; value_cells[idx] is the same
                                // cell with the substituted type — set its default.
                                mono.value_cells[idx].default_expr = Some(ctor);
                            }
                            CtorSynthesisResult::NotConstructible(required_param) => {
                                // Use-site span: find the AutoTypeParam for this type
                                // parameter, falling back to the prelude sentinel if
                                // the parameter is not in the auto-clause list.
                                let span = params
                                    .iter()
                                    .find(|p| p.name.as_str() == tp_name.as_str())
                                    .map_or(SourceSpan::prelude(), |p| p.use_site_span);
                                diagnostics.push(
                                    Diagnostic::error(format!(
                                        "auto type parameter resolved candidate '{}' is not \
                                         constructible: required parameter '{}' has no default; \
                                         cannot synthesize a zero-arg instance for \
                                         'param {} : {}'",
                                        candidate_name,
                                        required_param,
                                        orig_cell.id.member,
                                        candidate_name,
                                    ))
                                    .with_code(
                                        DiagnosticCode::AutoTypeParamCandidateNotConstructible,
                                    )
                                    .with_label(
                                        DiagnosticLabel::new(
                                            span,
                                            format!(
                                                "resolved to '{}', which has required param '{}'",
                                                candidate_name, required_param
                                            ),
                                        ),
                                    ),
                                );
                                // Leave default_expr = None — no partial-Undef instance.
                            }
                        }
                    }
                    // α partial-coverage: the following collections are NOT
                    // substituted and are documented as known gaps for M-013 α.
                    // They are only relevant when a generic body carries TypeParam
                    // in these positions, which is uncommon in current practice.
                    // β/γ tasks (constraint-aware selection, value population)
                    // can extend coverage as needed:
                    //   - sub_components[*].args  (CompiledExpr call-site values)
                    //   - realizations, connections, objective (geometry/eval exprs)
                    //   - match_arm_groups, forall_templates, assoc_fns, assoc_types
                    //
                    // Defensive invariant pin (#6854): the guard above now
                    // guarantees every declared type parameter of `target` is
                    // in `sigma`, so no top-level `value_cells` entry should
                    // retain a direct, unsubstituted `Type::TypeParam` for one
                    // of `target`'s own params. Deliberately scoped to
                    // top-level `value_cells` and to the direct variant only —
                    // the α partial-coverage note directly above lists the
                    // collections (sub_components[*].args, realizations,
                    // connections, objective, match_arm_groups,
                    // forall_templates, assoc_fns, assoc_types) that are
                    // knowingly NOT substituted; asserting over those would
                    // fire on healthy input.
                    debug_assert!(
                        !mono.value_cells.iter().any(|cell| matches!(
                            &cell.cell_type,
                            Type::TypeParam(n) if target.type_params.iter().any(|tp| &tp.name == n)
                        )),
                        "monomorph `{mono_name}` retains an unsubstituted type parameter in its top-level \
                         value cells despite full sigma coverage (#6854)",
                    );
                    // Mix the mono name into the content_hash so two distinct
                    // monomorphs that clone the same source hash (e.g. Bearing$A
                    // vs Bearing$B) produce different cache keys.
                    mono.content_hash = mono.content_hash.combine(ContentHash::of_str(&mono_name));
                    new_mono_templates.push(mono);
                }
                // Record the structure_name rewrite for this use-site regardless
                // of whether a new clone was created — every sub that resolved to
                // this monomorph must point at the shared template.
                structure_name_rewrites.push((
                    req.owner_structure.clone(),
                    req.sub_index,
                    mono_name,
                ));
            }
        }

        (
            rewrites,
            subst_pairs,
            new_mono_templates,
            structure_name_rewrites,
        )
    };

    // Pass 2 — push monomorph clones, apply structure_name and type_args rewrites.
    //
    // Order:
    //   1. Extend ctx.templates with the new (deduplicated) monomorphs.
    //   2. Build a name→index map for O(1) owner lookup (avoids an O(rewrites ×
    //      templates) scan when many auto: use-sites or many templates are present).
    //   3. Rewrite each originating sub's `structure_name` to the mono name
    //      (applies to ALL use-sites, including deduped ones).
    //   4. Apply the existing `type_args[pos]→StructureRef` slot rewrites.
    //
    // `sub_index` keys are unique per (owner, sub_index) so this safely targets
    // each `SubComponentDecl` even when match-arm clusters reuse `sub_name`
    // across multiple arms within the same template.

    // 1. Push deduplicated monomorph templates.
    ctx.templates.extend(monomorph_clones);

    // 2. Build a name→index map over the (now-extended) ctx.templates for O(1)
    //    owner lookup in steps 3 and 4.  Monomorphs pushed in step 1 are
    //    included so self-referential rewrites (if ever needed) resolve correctly.
    let owner_to_idx: HashMap<String, usize> = ctx
        .templates
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.clone(), i))
        .collect();

    // 3. Rewrite structure_name for ALL use-sites (including deduped ones).
    for (owner, sub_index, mono_name) in structure_name_rewrites {
        if let Some(&idx) = owner_to_idx.get(&owner)
            && let Some(sub) = ctx.templates[idx].sub_components.get_mut(sub_index)
        {
            sub.structure_name = mono_name;
        }
    }

    // 4. Apply type_args[pos]→StructureRef slot rewrites (pre-existing behaviour).
    for (owner, sub_index, position, resolved_name) in rewrites {
        if let Some(&idx) = owner_to_idx.get(&owner)
            && let Some(sub) = ctx.templates[idx].sub_components.get_mut(sub_index)
            && let Some(slot) = sub.type_args.get_mut(position)
        {
            *slot = Type::StructureRef(resolved_name);
        }
    }

    // Dedup first-wins on param name: `AutoTypeSubstitution::new` panics on
    // duplicate param names, and a panic at compile time is a worse UX than a
    // silent first-wins (B1 scope; future name-qualification is out of scope).
    //
    // ⚠️ Known lossy aggregation: when two sub-components in the same module
    // each instantiate a *different* template whose corresponding type-param is
    // named identically (e.g. both have `T: Seal`), only the first resolution
    // appears in the aggregate `auto_type_substitution`. The slot rewrites in
    // pass 2 above are NOT affected — each rewrite tuple carries its own
    // `(owner, sub_index, position)` so every `SubComponentDecl.type_args`
    // entry is correctly updated. The lossy behaviour is confined to the
    // module-level aggregate field, which downstream consumers should treat as
    // a debug/audit view rather than an authoritative per-use-site map.
    //
    // Future name-qualification (e.g. `Owner.sub.T` keys, or a shape change to
    // `Vec<(owner, sub_name, param, template)>`) would lift this restriction
    // but requires touching `types.rs::AutoTypeSubstitution` — out of B1 scope.
    // The negative-case test `multi_subs_with_colliding_param_names_first_wins`
    // pins the current first-wins behaviour so a future shape change cannot
    // regress silently.
    let mut seen: HashSet<String> = HashSet::new();
    let deduped: Vec<(String, String)> = subst_pairs
        .into_iter()
        .filter(|(param, _)| seen.insert(param.clone()))
        .collect();
    ctx.auto_type_substitution = AutoTypeSubstitution::new(deduped);
}
