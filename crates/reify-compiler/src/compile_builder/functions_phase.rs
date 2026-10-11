//! Phase-7 functions: compile every `fn` declaration, then build the merged
//! user+prelude resolution table for later phases.

use std::collections::HashMap;

use reify_ast::{FnDef, StructureDef};

use reify_ir::CompiledFunction;

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::entity::build_structure_def_skeleton;
use crate::functions::compile_function;
use crate::name_env::{self, Origin};
use crate::types::TopologyTemplate;

/// Run phase-7 (functions).
///
/// 1. If `fn_refs` is non-empty: build a merged template registry from
///    (a) prelude structure_def templates and (b) skeleton templates from the
///    module's own `structure_def`s (task 3895 — same-module construction
///    support); then iterate `fn_refs` in source order calling
///    [`compile_function`] with the registry, `ctx.resolution_enums`,
///    accumulated `ctx.functions`, and `ctx.alias_registry`.  When `fn_refs`
///    is empty this block is skipped — skeleton compilation would be wasted.
///    The skeletons are the local tier of [`name_env::structure_registry`], so
///    they shadow prelude templates of the same name.
/// 2. (always) Build `ctx.resolution_functions` via
///    [`build_resolution_functions`]. User functions appear first so
///    first-match-wins dispatch shadows duplicate prelude entries.
pub(crate) fn phase_functions(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    fn_refs: &[&FnDef],
    structure_refs: &[&StructureDef],
) {
    // Build skeleton TopologyTemplates and compile user functions only when
    // fn_refs is non-empty.  When the module has no functions the merged
    // registry is never consumed, so building skeletons (and compiling their
    // param defaults) is entirely wasted work.
    if !fn_refs.is_empty() {
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

        // Same-module skeletons are the local tier: they shadow prelude
        // templates of the same name.
        let merged_registry: HashMap<String, &TopologyTemplate> =
            name_env::structure_registry(&skeletons, prelude);

        // Compile user functions in source order.
        //
        // Passing `&ctx.functions` as the "prior compiled functions" parameter lets
        // a function resolve a call to an earlier sibling declaration (forward
        // references are not supported at the function layer).
        //
        // The order-INDEPENDENT companion to that order-dependent table
        // (task #5371), merged ONCE for the whole phase and borrowed by every
        // body scope below. It changes nothing about which overload resolves;
        // see `CompilationScope::declared_callable_names` for what it is for.
        let declared_callables = crate::functions::declared_callable_names(
            &ctx.declared_fn_names,
            &ctx.resolution_structure_names,
        );
        for fn_def in fn_refs {
            if let Some(compiled_fn) = compile_function(
                fn_def,
                &ctx.resolution_enums,
                &ctx.functions,
                &ctx.alias_registry,
                &ctx.resolution_structure_names,
                &ctx.resolution_trait_names,
                &declared_callables,
                Some(&merged_registry),
                &mut ctx.diagnostics,
            ) {
                ctx.functions.push(compiled_fn);
            }
        }
    }

    // 3. Build the resolution function list for compile-time overload dispatch.
    // `ctx.functions` (user-only) remains the output stored in CompiledModule.
    ctx.resolution_functions = build_resolution_functions(&ctx.functions, prelude);
}

/// The compile-time overload-dispatch table: every user function, then each
/// prelude function that wins its `(name, param types)` key under NS-P1
/// ([`name_env`]: imports before std modules, first wins). A prelude function
/// sharing a user function's key is shadowed.
pub(crate) fn build_resolution_functions(
    user: &[CompiledFunction],
    prelude: &[&CompiledModule],
) -> Vec<CompiledFunction> {
    let bindings = name_env::bind(user, prelude.iter().copied());
    let prelude_winners = bindings
        .winning_candidates()
        .iter()
        .filter(|candidate| candidate.origin != Origin::Local)
        .map(|candidate| candidate.definition.clone());
    user.iter().cloned().chain(prelude_winners).collect()
}
