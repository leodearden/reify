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

use reify_ast::TraitDecl;

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
/// The `trait_names` and `structure_names` sets are pre-computed by
/// `names_phase::build_resolution_names` (which runs before `phase_functions`)
/// and stored on `ctx`. This phase reads them from `ctx` rather than
/// rebuilding, so both `phase_functions` and `phase_traits` consume the same
/// shared sets.
///
/// Steps, in order:
/// 1. Read `trait_names` and `structure_names` from `ctx` (pre-computed by
///    `names_phase::build_resolution_names` before `phase_functions`).
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
    // 1. The trait_names and structure_names sets were pre-computed by
    //    names_phase::build_resolution_names (which ran before phase_functions).
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

    trait_names
}
