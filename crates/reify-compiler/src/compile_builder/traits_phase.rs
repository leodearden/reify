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

use reify_syntax::TraitDecl;

use crate::CompiledModule;
use crate::annotations::{deprecation_message, emit_deprecation_warning};
use crate::compile_builder::ctx::CompilationCtx;
use crate::traits::compile_trait;
use crate::types::CompiledTrait;

/// Build a combined trait registry (prelude first, then local override on
/// name collision) used by downstream phases for deprecation checks and
/// type-parameter bound resolution.
///
/// Borrows from both `local` (module-local trait defs in ctx) and every
/// prelude module's `trait_defs`. The returned map is phase-local because
/// its `&'a CompiledTrait` values borrow fields of `CompilationCtx` that
/// can't be stored alongside the owners (see task 2035 design decision #2).
pub(crate) fn build_trait_registry<'a>(
    local: &'a [CompiledTrait],
    prelude: &[&'a CompiledModule],
) -> HashMap<String, &'a CompiledTrait> {
    let mut registry: HashMap<String, &'a CompiledTrait> = HashMap::new();
    for m in prelude {
        for t in &m.trait_defs {
            registry.insert(t.name.clone(), t);
        }
    }
    for t in local {
        registry.insert(t.name.clone(), t);
    }
    registry
}

/// Run phase-8 (traits). Returns the compile-time `trait_names` set used by
/// downstream phases for `Type::TraitObject` resolution.
///
/// Steps, in order:
/// 1. Build `trait_names` from local trait decls + every prelude trait. This
///    happens BEFORE compiling traits so trait members whose types reference
///    other traits can resolve their siblings.
/// 2. Compile each local trait via [`compile_trait`] and push into
///    `ctx.trait_defs`.
/// 3. Build a phase-local `trait_registry` (prelude first, then local
///    override on name collision) and walk each local trait's `refinements`
///    emitting a deprecation warning when the parent carries `@deprecated`.
pub(crate) fn phase_traits(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    trait_refs: &[&TraitDecl],
) -> HashSet<String> {
    // 1. Build the set of trait names known at compile time so the type resolver
    //    can resolve `param m : Material` (trait name) to Type::TraitObject(...).
    //
    //    Collected from local trait declarations (syntax) and prelude trait defs
    //    (already compiled) BEFORE `compile_trait` runs, so trait members whose
    //    types reference other traits can resolve their siblings. Trait-name
    //    resolution is last in precedence (builtins → type params → alias → trait)
    //    so existing name-reuse stays backward compatible.
    let trait_names: HashSet<String> = trait_refs
        .iter()
        .map(|t| t.name.clone())
        .chain(
            prelude
                .iter()
                .flat_map(|m| m.trait_defs.iter().map(|t| t.name.clone())),
        )
        .collect();

    // 2. Compile each trait (depends on resolution_enums for enum type resolution in params).
    for trait_decl in trait_refs {
        let compiled_trait = compile_trait(
            trait_decl,
            &ctx.resolution_enums,
            &ctx.alias_registry,
            &trait_names,
            &mut ctx.diagnostics,
        );
        ctx.trait_defs.push(compiled_trait);
    }

    // 3. Build trait registry for deprecation checking (prelude first,
    //    local overrides on name collision).
    let trait_registry = build_trait_registry(&ctx.trait_defs, prelude);

    // Deprecation check: warn when a trait refinement references a @deprecated parent trait.
    // TraitDecl.refinements is Vec<String> without individual spans; use the child trait's span.
    for trait_decl in trait_refs {
        for refinement_name in &trait_decl.refinements {
            if let Some(parent_trait) = trait_registry.get(refinement_name.as_str())
                && let Some(msg) = deprecation_message(&parent_trait.annotations)
            {
                emit_deprecation_warning(
                    "trait",
                    refinement_name,
                    &msg,
                    trait_decl.span,
                    &mut ctx.diagnostics,
                );
            }
        }
    }

    trait_names
}
