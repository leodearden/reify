//! Phase-7 functions: compile every `fn` declaration, then build the merged
//! user+prelude resolution table for later phases.

use std::collections::HashMap;

use reify_ast::{FnDef, StructureDef};

use reify_ir::CompiledFunction;

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::entity::build_structure_def_skeleton;
use crate::functions::compile_function;
use crate::merge_prelude_functions;
use crate::types::{EntityKind, TopologyTemplate};

/// Run phase-7 (functions).
///
/// 1. Build a merged template registry from (a) prelude structure_def templates
///    and (b) skeleton templates built from the module's own `structure_def`s
///    (task 3895 — same-module construction support).  Prelude entries are
///    inserted first so same-module skeletons shadow them, mirroring the
///    prelude-first / local-shadows composition in `compile_entity`.
/// 2. Iterate `fn_refs` in source order, calling [`compile_function`] with
///    `ctx.resolution_enums` + the accumulated `ctx.functions` (so a function
///    can call an earlier sibling) + `ctx.alias_registry`; successful results
///    are pushed into `ctx.functions`.
/// 3. Build `ctx.resolution_functions` by merging user functions with prelude
///    functions via [`merge_prelude_functions`] — the canonical shadow
///    predicate for the Reify function table. User functions appear first so
///    first-match-wins dispatch shadows duplicate prelude entries.
pub(crate) fn phase_functions(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    fn_refs: &[&FnDef],
    structure_refs: &[&StructureDef],
) {
    // Build skeleton TopologyTemplates from this module's own structure_defs.
    //
    // These are transient — they are never stored in CompiledModule.  Their
    // sole purpose is to make `Foo()` ctor expressions in same-module fn bodies
    // lower to StructureInstanceCtor rather than a generic UserFunctionCall.
    //
    // Skeletons are declared BEFORE `merged_registry` so they outlive the
    // &-borrows taken by the registry (drop order is reverse-declaration).
    let skeletons: Vec<TopologyTemplate> = structure_refs
        .iter()
        .map(|s| {
            build_structure_def_skeleton(
                s,
                &ctx.resolution_enums,
                &ctx.functions,
                &ctx.alias_registry,
                &ctx.resolution_structure_names,
                &ctx.resolution_trait_names,
                &ctx.unit_registry,
            )
        })
        .collect();

    // Build the merged registry: prelude entries first (so same-module skeletons
    // shadow prelude templates of the same name — mirrors compile_entity's
    // entity_template_registry composition at entity.rs:396-405).
    let mut merged_registry: HashMap<String, &TopologyTemplate> = prelude
        .iter()
        .flat_map(|m| m.templates.iter())
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| (t.name.clone(), t))
        .collect();
    for sk in skeletons.iter() {
        merged_registry.insert(sk.name.clone(), sk);
    }

    // 2. Compile user functions in source order.
    //
    // Passing `&ctx.functions` as the "prior compiled functions" parameter lets
    // a function resolve a call to an earlier sibling declaration (forward
    // references are not supported at the function layer).
    for fn_def in fn_refs {
        if let Some(compiled_fn) = compile_function(
            fn_def,
            &ctx.resolution_enums,
            &ctx.functions,
            &ctx.alias_registry,
            &ctx.resolution_structure_names,
            &ctx.resolution_trait_names,
            Some(&merged_registry),
            &mut ctx.diagnostics,
        ) {
            ctx.functions.push(compiled_fn);
        }
    }

    // 2. Build the resolution function list for compile-time overload dispatch.
    //
    // User functions appear first (shadowing priority); prelude functions with
    // distinct `(name, arity, param_types)` triples are appended. See
    // `merge_prelude_functions()` for the canonical shadow predicate.
    // `ctx.functions` (user-only) remains the output stored in CompiledModule.
    let prelude_fns: Vec<CompiledFunction> = prelude
        .iter()
        .flat_map(|m| m.functions.iter().cloned())
        .collect();
    ctx.resolution_functions = merge_prelude_functions(&ctx.functions, &prelude_fns);
}
