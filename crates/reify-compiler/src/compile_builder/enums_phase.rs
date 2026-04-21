//! Phase-6 resolution enums: build the merged enum table used for type
//! resolution during the rest of compilation.
//!
//! Only module-local `enum_defs` enter the output `CompiledModule`;
//! `resolution_enums` is the strictly larger set (prelude ++ local) used
//! for resolving enum type references from inside the user's decls.

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;

/// Run phase-6. Populates `ctx.resolution_enums` by chaining every prelude
/// module's `enum_defs` with the module-local `ctx.enum_defs`.
///
/// Must run after `collect_decl_refs` (which pushes module-local enums into
/// `ctx.enum_defs`) and before any phase that resolves enum type references
/// (functions, traits, fields, entities, purposes).
pub(crate) fn build_resolution_enums(ctx: &mut CompilationCtx, prelude: &[&CompiledModule]) {
    // resolution_enums is used for type resolution during compilation;
    // only enum_defs (module-local) goes into the output CompiledModule.
    ctx.resolution_enums = prelude
        .iter()
        .flat_map(|m| m.enum_defs.iter().cloned())
        .collect();
    ctx.resolution_enums.extend(ctx.enum_defs.iter().cloned());
}
