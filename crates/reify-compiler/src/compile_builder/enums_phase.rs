//! Phase-6 resolution enums: build the merged enum table used for type
//! resolution during the rest of compilation.
//!
//! Only module-local `enum_defs` enter the output `CompiledModule`;
//! `resolution_enums` is the strictly larger set (prelude ++ local) used
//! for resolving enum type references from inside the user's decls.

use reify_ir::EnumDef;

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;

/// Flatten the `enum_defs` from every module in `prelude` into a single
/// `Vec<EnumDef>`, preserving source order (m[0] first, m[1] second, …).
///
/// Used by [`PreludeContext::new`] to build the pre-computed enum cache once;
/// the compiled cache is then reused by [`build_resolution_enums_from_cache`]
/// on every subsequent compile call.
pub(crate) fn flatten_prelude_enum_defs(prelude: &[&CompiledModule]) -> Vec<EnumDef> {
    prelude
        .iter()
        .flat_map(|m| m.enum_defs.iter().cloned())
        .collect()
}

/// Run phase-6 using a pre-built enum list from a [`PreludeContext`].
///
/// Clones the pre-computed `prelude_resolution_enums` into `ctx.resolution_enums`
/// and then appends the module-local enums, skipping the flat_map over prelude
/// modules that would otherwise be repeated on every call.
pub(crate) fn build_resolution_enums_from_cache(
    ctx: &mut CompilationCtx,
    prelude_resolution_enums: &[EnumDef],
) {
    ctx.resolution_enums = prelude_resolution_enums.to_vec();
    ctx.resolution_enums.extend(ctx.enum_defs.iter().cloned());
}
