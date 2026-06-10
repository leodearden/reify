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
//!      bound-check sees the resolved candidate,
//!   4. accumulates `(param_name, template_name)` substitution pairs across all
//!      requests, deduping first-wins, into `ctx.auto_type_substitution`.
//!
//! Resolution diagnostics (NoCandidate / Ambiguous / NonUnique / depth-bound /
//! cap-fallback) are pushed into `ctx.diagnostics` by the resolver itself.

use std::collections::{HashMap, HashSet};

use reify_core::{ContentHash, Diagnostic, Type};
use reify_ir::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction,
};

use crate::CompiledModule;
use crate::auto_type_param::{AutoTypeParam, resolve_auto_type_params_with_backtracking};
use crate::compile_builder::ctx::CompilationCtx;
use crate::compile_builder::traits_phase::build_trait_registry;
use crate::type_resolution::{substitute_expr_result_types, substitute_type_params};
use crate::types::{
    AutoTypeSubstitution, EntityKind, TopologyTemplate, mangle_monomorph_name,
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
struct CompileTimeIndeterminateChecker;

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
pub(crate) fn phase_auto_type_param_resolution(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
) {
    // Empty-queue early-out: the common case (no `auto:` type-args anywhere).
    // Leaving `auto_type_substitution` at its empty default is load-bearing for
    // cache stability — an empty Vec hashes deterministically and must not shift
    // for modules that declare no `auto:` slots.
    if ctx.pending_auto_resolutions.is_empty() {
        return;
    }

    let checker = CompileTimeIndeterminateChecker;
    let requests = std::mem::take(&mut ctx.pending_auto_resolutions);

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
                &checker,
                functions,
                reify_config::DEFAULT_AUTO_TYPE_PARAM_MAX_DEPTH,
                reify_config::DEFAULT_AUTO_TYPE_PARAM_MAX_CROSS_PRODUCT_SIZE,
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
                    sigma.insert(param_name.clone(), Type::StructureRef(template_name.clone()));
                    candidates_by_position.push((position, template_name.clone()));
                }
                subst_pairs.push((param_name.clone(), template_name.clone()));
            }

            // Synthesize a monomorph if at least one type-param was resolved.
            if !sigma.is_empty() {
                // Sort by position to guarantee deterministic mangle order
                // regardless of outcome.substitution iteration order.
                candidates_by_position.sort_by_key(|(pos, _)| *pos);
                let ordered_candidates: Vec<String> =
                    candidates_by_position.into_iter().map(|(_, c)| c).collect();
                let mono_name =
                    mangle_monomorph_name(&req.target_name, &ordered_candidates);

                // Defensive collision guard: a pre-existing template named
                // `mono_name` that was NOT created by α in this pass would be
                // silently overwritten in pass-2.  This is impossible from valid
                // `.ri` source (`$` is illegal in identifiers), but guard
                // converts any future compiler regression into a build error.
                //
                // Skip both the clone AND the structure_name rewrite for this
                // use-site — there is no safe target to point the sub at.
                if monomorph_name_would_collide(
                    &ctx.templates,
                    &created_monomorphs,
                    &mono_name,
                ) {
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
                    // α partial-coverage: the following collections are NOT
                    // substituted and are documented as known gaps for M-013 α.
                    // They are only relevant when a generic body carries TypeParam
                    // in these positions, which is uncommon in current practice.
                    // β/γ tasks (constraint-aware selection, value population)
                    // can extend coverage as needed:
                    //   - sub_components[*].args  (CompiledExpr call-site values)
                    //   - realizations, connections, objective (geometry/eval exprs)
                    //   - match_arm_groups, forall_templates, assoc_fns, assoc_types
                    // Mix the mono name into the content_hash so two distinct
                    // monomorphs that clone the same source hash (e.g. Bearing$A
                    // vs Bearing$B) produce different cache keys.
                    mono.content_hash =
                        mono.content_hash.combine(ContentHash::of_str(&mono_name));
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

        (rewrites, subst_pairs, new_mono_templates, structure_name_rewrites)
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
