//! Phase-7 functions: compile every `fn` declaration, then build the merged
//! user+prelude resolution table for later phases.

use reify_syntax::FnDef;
use reify_types::CompiledFunction;

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::functions::compile_function;
use crate::merge_prelude_functions;

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
