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

use reify_core::Type;
use reify_ir::{
    ConstraintChecker, ConstraintDiagnostics, ConstraintInput, ConstraintResult, Satisfaction,
};

use crate::CompiledModule;
use crate::auto_type_param::{AutoTypeParam, resolve_auto_type_params_with_backtracking};
use crate::compile_builder::ctx::CompilationCtx;
use crate::compile_builder::traits_phase::build_trait_registry;
use crate::types::{AutoTypeSubstitution, EntityKind, TopologyTemplate};

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
    // Collect (owner, sub_name, position, resolved_name) rewrites and the raw
    // substitution pairs; defer the mutation of `ctx.templates` to pass 2 so the
    // registry's `&TopologyTemplate` borrows don't conflict with `&mut`.
    let (rewrites, subst_pairs) = {
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

        let mut rewrites: Vec<(String, String, usize, String)> = Vec::new();
        let mut subst_pairs: Vec<(String, String)> = Vec::new();

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

            for (param_name, template_name) in outcome.substitution {
                if let Some(&position) = name_to_position.get(&param_name) {
                    rewrites.push((
                        req.owner_structure.clone(),
                        req.sub_name.clone(),
                        position,
                        template_name.clone(),
                    ));
                }
                subst_pairs.push((param_name, template_name));
            }
        }

        (rewrites, subst_pairs)
    };

    // Pass 2 — apply placeholder rewrites and store the deduped substitution.
    for (owner, sub_name, position, resolved_name) in rewrites {
        if let Some(template) = ctx.templates.iter_mut().find(|t| t.name == owner) {
            if let Some(sub) = template
                .sub_components
                .iter_mut()
                .find(|s| s.name == sub_name)
            {
                if let Some(slot) = sub.type_args.get_mut(position) {
                    *slot = Type::StructureRef(resolved_name);
                }
            }
        }
    }

    // Dedup first-wins on param name: `AutoTypeSubstitution::new` panics on
    // duplicate param names, and a panic at compile time is a worse UX than a
    // silent first-wins (B1 scope; future name-qualification is out of scope).
    let mut seen: HashSet<String> = HashSet::new();
    let deduped: Vec<(String, String)> = subst_pairs
        .into_iter()
        .filter(|(param, _)| seen.insert(param.clone()))
        .collect();
    ctx.auto_type_substitution = AutoTypeSubstitution::new(deduped);
}
