//! Phase-6 resolution enums: build the merged enum table used for type
//! resolution during the rest of compilation.
//!
//! Only module-local `enum_defs` enter the output `CompiledModule`;
//! `resolution_enums` is the strictly larger set (prelude ++ local) used
//! for resolving enum type references from inside the user's decls.

use reify_types::EnumDef;

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;

/// Flatten the `enum_defs` from every module in `prelude` into a single
/// `Vec<EnumDef>`, preserving source order (m[0] first, m[1] second, …).
///
/// This is the single-source body used by both [`build_resolution_enums`]
/// (context-less path) and [`PreludeContext::new`] (pre-built cache path) so
/// the two are byte-for-byte identical by construction.
pub(crate) fn flatten_prelude_enum_defs(prelude: &[&CompiledModule]) -> Vec<EnumDef> {
    prelude
        .iter()
        .flat_map(|m| m.enum_defs.iter().cloned())
        .collect()
}

/// Run phase-6. Populates `ctx.resolution_enums` by chaining every prelude
/// module's `enum_defs` with the module-local `ctx.enum_defs`.
///
/// Must run after `collect_decl_refs` (which pushes module-local enums into
/// `ctx.enum_defs`) and before any phase that resolves enum type references
/// (functions, traits, fields, entities, purposes).
pub(crate) fn build_resolution_enums(ctx: &mut CompilationCtx, prelude: &[&CompiledModule]) {
    // resolution_enums is used for type resolution during compilation;
    // only enum_defs (module-local) goes into the output CompiledModule.
    ctx.resolution_enums = flatten_prelude_enum_defs(prelude);
    ctx.resolution_enums.extend(ctx.enum_defs.iter().cloned());
}

/// Run phase-6 using a pre-built enum list from a [`PreludeContext`].
///
/// Clones the pre-computed `prelude_resolution_enums` into `ctx.resolution_enums`
/// and then appends the module-local enums, skipping the flat_map over prelude
/// modules that [`build_resolution_enums`] performs on every call.
pub(crate) fn build_resolution_enums_from_cache(
    ctx: &mut CompilationCtx,
    prelude_resolution_enums: &[EnumDef],
) {
    ctx.resolution_enums = prelude_resolution_enums.to_vec();
    ctx.resolution_enums.extend(ctx.enum_defs.iter().cloned());
}
