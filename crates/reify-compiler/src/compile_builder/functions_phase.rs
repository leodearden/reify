//! Function compilation, split into two sub-passes (task 5494 / PRD
//! `stdlib-namespace.md` D-8).
//!
//! * [`phase_fn_signatures`] runs EARLY — before any structure template exists —
//!   and resolves every `fn` declaration's signature.  It seeds
//!   `ctx.resolution_functions` with signature-only stubs so the phases between
//!   it and the unified pass (`phase_traits`, `phase_fields`,
//!   `phase_constraint_defs`) can resolve calls to same-module fns.
//! * [`compile_function_decl`] compiles one fn's param defaults and body.  It is
//!   driven by the unified source-order pass in `entities_phase::phase_entities`,
//!   which interleaves fn bodies with entity templates in declaration order, so
//!   a fn body constructs a same-module structure against that structure's
//!   AUTHORITATIVE [`TopologyTemplate`].
//!
//! A same-module structure declared BELOW the fn has no template yet; using it
//! draws `DiagnosticCode::StructureDeclaredAfterFunction`.

use std::collections::{HashMap, HashSet};

use reify_ast::FnDef;
use reify_core::Diagnostic;
use reify_ir::{CompiledFunction, EnumDef};

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::functions::{FnSignature, compile_function_with_signature, resolve_fn_signature};
use crate::name_env::{self, Origin};
use crate::types::TopologyTemplate;

/// Resolve every `fn` declaration's SIGNATURE, in source order.
///
/// The first half of the two-sub-pass split (task 5494 / PRD
/// `stdlib-namespace.md` D-8).  Signature resolution compiles no expression, so
/// it needs no template registry — which is what lets it run before any
/// structure template exists.  Param defaults and bodies are compiled later, by
/// the unified source-order pass in `phase_entities`, against the authoritative
/// registry.
///
/// Seeds `ctx.resolution_functions` with signature-only stubs composed with the
/// prelude by [`build_resolution_functions`], so the phases that run between
/// this one and the unified pass (`phase_traits`, `phase_fields`,
/// `phase_constraint_defs`) can resolve calls to same-module fns.
pub(crate) fn phase_fn_signatures(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    fn_refs: &[&FnDef],
) {
    for fn_def in fn_refs {
        let sig = resolve_fn_signature(
            fn_def,
            &ctx.resolution_enums,
            &ctx.alias_registry,
            &ctx.resolution_structure_names,
            &ctx.resolution_trait_names,
            &mut ctx.diagnostics,
        );
        ctx.fn_signatures.push(sig);
    }

    ctx.resolution_functions = build_resolution_functions(&ctx.fn_signature_stubs(), prelude);
}

/// Compile ONE `fn` declaration's param defaults and body — the second half of
/// the two-sub-pass split.
///
/// Called from the `Declaration::Function` arm of
/// `entities_phase::phase_entities`, in source order, so `user_fns_so_far` and
/// `templates_so_far` hold only what is declared ABOVE this fn:
///
/// * constructors resolve against [`name_env::ctor_template_registry`] over
///   `templates_so_far`, so a fn body may construct a same-module structure
///   declared above it and get that structure's authoritative template — the
///   same no-forward-reference rule `compile_entity` obeys for entity → entity
///   references;
/// * calls resolve against [`build_resolution_functions`] over
///   `user_fns_so_far` plus every prelude fn they do not shadow, so forward
///   references to sibling fns do not resolve.
///
/// A module-local `structure def` (`local_structure_def_names`)
/// with no template in `templates_so_far` is declared BELOW this fn; using it
/// draws `StructureDeclaredAfterFunction` rather than a silent fallback.
///
/// `declared_callables` is the module's order-independent callable vocabulary
/// (task #5371), built once per pass by the caller.
///
/// Signature-level diagnostics are NOT emitted here — [`phase_fn_signatures`]
/// already emitted them exactly once.
#[allow(clippy::too_many_arguments)]
pub(crate) fn compile_function_decl(
    fn_def: &FnDef,
    sig: &FnSignature,
    enum_defs: &[EnumDef],
    user_fns_so_far: &[CompiledFunction],
    templates_so_far: &[TopologyTemplate],
    prelude: &[&CompiledModule],
    local_structure_def_names: &HashSet<String>,
    declared_callables: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledFunction> {
    let compiled_so_far: HashSet<&str> = templates_so_far.iter().map(|t| t.name.as_str()).collect();
    let later_declared_structure_names: HashSet<String> = local_structure_def_names
        .iter()
        .filter(|name| !compiled_so_far.contains(name.as_str()))
        .cloned()
        .collect();
    let functions = build_resolution_functions(user_fns_so_far, prelude);
    let template_registry: HashMap<String, &TopologyTemplate> =
        name_env::ctor_template_registry(templates_so_far, prelude);
    compile_function_with_signature(
        fn_def,
        sig,
        enum_defs,
        &functions,
        Some(&later_declared_structure_names),
        declared_callables,
        Some(&template_registry),
        diagnostics,
    )
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
