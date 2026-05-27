//! Phase-7 functions: compile every `fn` declaration, then build the merged
//! user+prelude resolution table for later phases.

use std::collections::HashMap;

use reify_ast::FnDef;
use reify_ir::CompiledFunction;

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::functions::compile_function;
use crate::merge_prelude_functions;
use crate::types::{EntityKind, TopologyTemplate};

/// Run phase-7 (functions).
///
/// 1. Iterate `fn_refs` in source order, calling [`compile_function`] with
///    `ctx.resolution_enums` + the accumulated `ctx.functions` (so a function
///    can call an earlier sibling) + `ctx.alias_registry`; successful results
///    are pushed into `ctx.functions`.
/// 2. Build `ctx.resolution_functions` by merging user functions with prelude
///    functions via [`merge_prelude_functions`] — the canonical shadow
///    predicate for the Reify function table. User functions appear first so
///    first-match-wins dispatch shadows duplicate prelude entries.
pub(crate) fn phase_functions(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    fn_refs: &[&FnDef],
) {
    // Build a prelude-only template registry of structure_def templates so
    // that a `pub fn` body referencing a prelude structure-def via
    // constructor syntax (e.g. `FlexureCompliance()`) lowers to
    // `CompiledExprKind::StructureInstanceCtor` rather than a generic
    // `FunctionCall` (esc-3851-32; mirrors the `prelude_template_registry`
    // built in `entities_phase::phase_entities`).
    //
    // NOTE — same-module structure_def constructors in fn bodies are NOT
    // supported here. `phase_functions` runs before `phase_entities`, so
    // `ctx.templates` is empty at this point. Modules that need a fn body
    // to construct a same-module structure_def must currently split the
    // structure_def into a separately-loaded prelude module (e.g.
    // `std/flexures.types` is loaded before `std/flexures`). A follow-up
    // task may add a pre-pass that collects skeleton TopologyTemplates from
    // parsed same-module structure_defs before phase_functions runs.
    let prelude_template_registry: HashMap<String, &TopologyTemplate> = prelude
        .iter()
        .flat_map(|m| m.templates.iter())
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| (t.name.clone(), t))
        .collect();

    // 1. Compile user functions in source order.
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
            Some(&prelude_template_registry),
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
