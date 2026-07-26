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
//! There is no mirror/skeleton template registry any more: the transient
//! `build_structure_def_skeleton` templates task 3895 fabricated for every
//! `structure_def` — and the two silent-divergence hazards they carried (poison
//! defaults compiled with discarded diagnostics; nested-ctor-in-let lowering to
//! a generic `FunctionCall` because no template registry was installed) — are
//! deleted.

use std::collections::HashMap;

use reify_ast::FnDef;

use reify_core::Diagnostic;
use reify_ir::{CompiledFunction, EnumDef};

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::functions::{FnSignature, compile_function_with_signature, resolve_fn_signature};
use crate::merge_prelude_functions;
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
/// Seeds `ctx.resolution_functions` with signature-only stubs merged with the
/// prelude via [`merge_prelude_functions`] — the same composition the old
/// `phase_functions` used — so the phases that run between this one and the
/// unified pass (`phase_traits`, `phase_fields`, `phase_constraint_defs`) can
/// resolve calls to same-module fns.
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

    let prelude_fns: Vec<CompiledFunction> = prelude
        .iter()
        .flat_map(|m| m.functions.iter().cloned())
        .collect();
    ctx.resolution_functions = merge_prelude_functions(&ctx.fn_signature_stubs(), &prelude_fns);
}

/// Compile ONE `fn` declaration's param defaults and body — the second half of
/// the two-sub-pass split.
///
/// Called from the `Declaration::Function` arm of
/// `entities_phase::phase_entities`, in source order, so `template_registry`
/// holds the prelude structure_defs plus every same-module structure template
/// compiled SO FAR.  A fn body may therefore construct a same-module structure
/// declared ABOVE it and get that structure's authoritative template — the same
/// no-forward-reference rule `compile_entity` already obeys for entity → entity
/// references (`compiled_templates` = templates so far).
///
/// `functions` is the prior-siblings table: a fn resolves calls to earlier
/// sibling declarations only (forward references are not supported at the
/// function layer).
///
/// Signature-level diagnostics are NOT emitted here — [`phase_fn_signatures`]
/// already emitted them exactly once.
pub(crate) fn compile_function_decl(
    fn_def: &FnDef,
    sig: &FnSignature,
    enum_defs: &[EnumDef],
    functions: &[CompiledFunction],
    template_registry: &HashMap<String, &TopologyTemplate>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CompiledFunction> {
    compile_function_with_signature(
        fn_def,
        sig,
        enum_defs,
        functions,
        Some(template_registry),
        diagnostics,
    )
}
