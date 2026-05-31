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
use reify_ir::CompiledFunction;

use crate::CompiledModule;
use crate::annotations::{deprecation_message, emit_deprecation_warning};
use crate::compile_builder::ctx::CompilationCtx;
use crate::expr::trait_static_fn_symbol;
use crate::functions::compile_function;
use crate::merge_prelude_functions;
use crate::traits::compile_trait;
use crate::types::{CompiledTrait, DefaultKind};

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

    // 4. Register namespaced trait-static functions into `ctx.functions`.
    //
    // For each locally-compiled trait's body-carrying static (no-`self`) assoc fn,
    // compile the body in a neutral scope (same as a free fn) and register the result
    // as a `CompiledFunction` named `"Trait::method"` in `ctx.functions`.
    //
    // This registration is what makes `TraitStaticCall { trait_name, method, args }`
    // dispatch possible: the `compile_expr_guarded` arm for `TraitStaticCall` looks up
    // the mangled name via `resolve_function_overload` against `ctx.resolution_functions`,
    // so the fn must land in the function table before entity bodies are compiled.
    //
    // Filtering rules:
    // - `DefaultKind::Fn` only (body-carrying assoc fns).
    // - No `is_self` param (static; instance methods are task ζ=3941).
    // - `body.is_some()` guard (bodyless required fns appear only in `required_members`,
    //   not `defaults`, but guard anyway for safety).
    //
    // The neutral compile scope means a body referencing a trait member (e.g. `diameter`)
    // produces `DiagnosticCode::UnresolvedName` naming the member — the PRD §4.4 negative
    // signal — with no extra code needed.
    //
    // Name-drift guard: both producer (here) and consumer (expr.rs TraitStaticCall arm)
    // call the shared `trait_static_fn_symbol` helper, so the mangled name is always
    // byte-for-byte identical.
    for compiled_trait in &ctx.trait_defs {
        for default in &compiled_trait.defaults {
            if let DefaultKind::Fn(fn_def) = &default.kind {
                // Skip instance methods (has a self receiver).
                if fn_def.params.iter().any(|p| p.is_self) {
                    continue;
                }
                // Skip bodyless fns (should only appear in required_members, not defaults,
                // but guard for safety).
                if fn_def.body.is_none() {
                    continue;
                }
                // Clone and rename to the namespaced symbol.
                let mut namespaced_fn = fn_def.clone();
                namespaced_fn.name =
                    trait_static_fn_symbol(&compiled_trait.name, &fn_def.name);
                // Compile the body. Borrows of ctx fields are disjoint here:
                // - resolution_enums, functions, alias_registry, resolution_structure_names
                //   are immutable borrows released when compile_function returns.
                // - diagnostics is the mutable borrow taken by compile_function.
                // - functions.push() runs after compile_function returns (NLL-safe).
                let compiled_fn = compile_function(
                    &namespaced_fn,
                    &ctx.resolution_enums,
                    &ctx.functions,
                    &ctx.alias_registry,
                    &ctx.resolution_structure_names,
                    &trait_names,
                    None, // v1: no prelude template registry for static fn bodies
                    &mut ctx.diagnostics,
                );
                if let Some(f) = compiled_fn {
                    ctx.functions.push(f);
                }
            }
        }
    }

    // Rebuild `ctx.resolution_functions` so entity bodies (compiled in phase_entities,
    // which runs after phase_traits) can resolve `Trait::method` calls.
    // Pattern mirrors functions_phase.rs: user functions first (shadowing priority),
    // then prelude functions for distinct (name, arity, param_types) triples.
    // Note: `ctx.functions` (user-only, includes newly registered static fns) flows
    // to the final `CompiledModule.functions` (ctx.rs:181) used by the eval engine.
    let prelude_fns: Vec<CompiledFunction> = prelude
        .iter()
        .flat_map(|m| m.functions.iter().cloned())
        .collect();
    ctx.resolution_functions = merge_prelude_functions(&ctx.functions, &prelude_fns);

    trait_names
}
