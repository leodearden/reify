//! Phase-8 traits: compile every trait declaration, build the compile-time
//! `trait_names` set, and emit deprecation warnings for refinements of
//! `@deprecated` parent traits.
//!
//! Returns `trait_names` so downstream phases (fields, defs, entities) can
//! resolve `param m : TraitName` against it. The trait registry (a
//! phase-local `HashMap<String, &CompiledTrait>`) is rebuilt lazily inside
//! each phase that needs it via [`build_trait_registry`] — it borrows from
//! ctx's owned `trait_defs` and therefore can't live on ctx alongside the
//! owner (see task 2035 design decision #2).

use std::collections::{HashMap, HashSet};

use reify_ast::{FnDef, TraitDecl};

use crate::CompiledModule;
use crate::annotations::{deprecation_message, emit_deprecation_warning};
use crate::call_table::CallTable;
use crate::compile_builder::ctx::CompilationCtx;
use crate::compile_builder::functions_phase::ResolutionTable;
use crate::expr::trait_static_fn_symbol;
use crate::functions::compile_function;
use crate::name_env;
use crate::traits::compile_trait;
use crate::types::{CompiledTrait, DefaultKind};

/// Build the trait registry — every trait name bound by [`crate::name_env`]
/// (NS-P1: local, then imports, then std modules, first wins) — used by
/// downstream phases for deprecation checks and type-parameter bound
/// resolution.
///
/// Borrows from both `local` (module-local trait defs in ctx) and every
/// prelude module's `trait_defs`. The returned map is phase-local because
/// its `&'a CompiledTrait` values borrow fields of `CompilationCtx` that
/// can't be stored alongside the owners (see task 2035 design decision #2).
pub(crate) fn build_trait_registry<'a>(
    local: &'a [CompiledTrait],
    prelude: &[&'a CompiledModule],
) -> HashMap<String, &'a CompiledTrait> {
    name_env::bind(local, prelude.iter().copied())
        .winners()
        .map(|t| (t.name.clone(), t))
        .collect()
}

/// Run phase-8 (traits). Returns the compile-time `trait_names` set used by
/// downstream phases for `Type::TraitObject` resolution.
///
/// The `trait_names` and `structure_names` sets are pre-computed by
/// `names_phase::build_resolution_names` (which runs before
/// `phase_fn_signatures`) and stored on `ctx`. This phase reads them from
/// `ctx` rather than rebuilding, so both `phase_fn_signatures` and
/// `phase_traits` consume the same shared sets.
///
/// Since task 5494's reorder this phase also precedes user fn-BODY
/// compilation (bodies are compiled by the unified source-order pass in
/// `entities_phase::phase_entities`) — which is why it buffers trait static
/// fns into `ctx.trait_static_fns` rather than assuming any body has been
/// compiled yet.
///
/// Steps, in order:
/// 1. Read `trait_names` and `structure_names` from `ctx` (pre-computed by
///    `names_phase::build_resolution_names` before `phase_fn_signatures`).
/// 2. Compile each local trait via [`compile_trait`] and push into
///    `ctx.trait_defs`.
/// 3. Build a phase-local `trait_registry` (prelude first, then local
///    override on name collision) and walk each local trait's `refinements`
///    emitting a deprecation warning when the parent carries `@deprecated`.
/// 4. Register each local trait's body-carrying static fn via
///    [`register_trait_static_fns`].
pub(crate) fn phase_traits(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    trait_refs: &[&TraitDecl],
) -> HashSet<String> {
    // 1. The trait_names and structure_names sets were pre-computed by
    //    names_phase::build_resolution_names (which ran before phase_fn_signatures).
    //    Move trait_names out of ctx (no other phase reads resolution_trait_names
    //    after phase_traits, so cloning would be wasteful); borrow structure_names
    //    in place.
    let trait_names = std::mem::take(&mut ctx.resolution_trait_names);
    let structure_names = &ctx.resolution_structure_names;

    // 2. Compile each trait (depends on resolution_enums for enum type resolution in params).
    for trait_decl in trait_refs {
        let compiled_trait = compile_trait(
            trait_decl,
            &ctx.resolution_enums,
            &ctx.alias_registry,
            structure_names,
            &trait_names,
            &mut ctx.diagnostics,
        );
        ctx.trait_defs.push(compiled_trait);
    }

    // 3. Build trait registry for deprecation checking (prelude first,
    //    local overrides on name collision).
    let trait_registry = build_trait_registry(&ctx.trait_defs, prelude);

    // Deprecation check: warn when a trait refinement references a @deprecated parent trait.
    // Each refinement carries its own per-entry span, so the label highlights exactly
    // the offending identifier (e.g. `Base` in `trait Derived : Base`) rather than
    // the whole child-trait declaration.
    for trait_decl in trait_refs {
        for refinement in &trait_decl.refinements {
            if let Some(parent_trait) = trait_registry.get(refinement.name.as_str())
                && let Some(msg) = deprecation_message(&parent_trait.annotations)
            {
                emit_deprecation_warning(
                    "trait",
                    &refinement.name,
                    msg,
                    refinement.span,
                    &mut ctx.diagnostics,
                );
            }
        }
    }

    // 4. Register namespaced trait-static functions.
    register_trait_static_fns(ctx, prelude, &trait_names);

    trait_names
}

/// Compile each local trait's body-carrying static fn under its
/// [`trait_static_fn_symbol`] into `ctx.trait_static_fns`, which the unified
/// pass appends after the user fns.
///
/// Each body resolves against the signature stubs, the static fns compiled
/// before it, and the prelude. That table is built at the first such fn, so a
/// module with none clones no prelude fn. It is then written back to
/// `ctx.resolution_functions` for the unified pass, with the stubs still at
/// `0..fn_signatures.len()`, where that pass replaces stub `i` by declaration
/// index.
fn register_trait_static_fns(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    trait_names: &HashSet<String>,
) {
    let declared_callables = crate::functions::declared_callable_names(
        &ctx.declared_fn_names,
        &ctx.resolution_structure_names,
    );
    let mut static_fn_calls: Option<ResolutionTable> = None;
    for (trait_name, fn_def) in body_carrying_static_fns(&ctx.trait_defs) {
        let mut namespaced_fn = fn_def.clone();
        namespaced_fn.name = trait_static_fn_symbol(trait_name, &fn_def.name);
        let calls_above = static_fn_calls
            .get_or_insert_with(|| ResolutionTable::new(ctx.fn_signature_stubs(), prelude));
        if let Some(f) = compile_function(
            &namespaced_fn,
            &ctx.resolution_enums,
            CallTable::with_pending(calls_above.functions(), &ctx.fns_with_pending_defaults),
            &ctx.alias_registry,
            &ctx.resolution_structure_names,
            trait_names,
            &declared_callables,
            None, // v1: no prelude template registry for static fn bodies
            &mut ctx.diagnostics,
        ) {
            calls_above.push_user(f.clone());
            ctx.trait_static_fns.push(f);
        }
    }
    if let Some(calls) = static_fn_calls {
        ctx.resolution_functions = calls.into_functions();
    }
}

/// Each trait's static (no-`self`) fn default that carries a body, with its
/// trait's name, in trait order then default order.
fn body_carrying_static_fns(traits: &[CompiledTrait]) -> impl Iterator<Item = (&str, &FnDef)> {
    traits.iter().flat_map(|compiled_trait| {
        compiled_trait
            .defaults
            .iter()
            .filter_map(move |default| match &default.kind {
                DefaultKind::Fn(fn_def)
                    if fn_def.body.is_some() && !fn_def.params.iter().any(|p| p.is_self) =>
                {
                    Some((compiled_trait.name.as_str(), fn_def))
                }
                _ => None,
            })
    })
}
