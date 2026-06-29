//! Phase-6 resolution enums: build the merged enum table used for type
//! resolution during the rest of compilation.
//!
//! Only module-local `enum_defs` enter the output `CompiledModule`;
//! `resolution_enums` is the strictly larger set (prelude ++ local) used
//! for resolving enum type references from inside the user's decls.

use std::collections::HashSet;

use reify_ast::{Declaration, ParsedModule};
use reify_core::ty::Type;
use reify_ir::{EnumDef, EnumVariantDef, VariantPayload};

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;
use crate::type_resolution::resolve_type_expr_with_aliases;

/// Resolve each enum variant's named-field payload `TypeExpr`s into the IR's
/// `VariantPayload::Named(Vec<(String, Type)>)` (task δ #3942).
///
/// `pre_pass::collect_decl_refs` seeds `ctx.enum_defs` with every variant mapped
/// to `EnumVariantDef::unit` — the AST named-field payload is dropped — because
/// the alias registry and the structure/trait name sets do not yet exist when
/// it runs (payload field types such as `Length` are stdlib type aliases that
/// need [`resolve_type_expr_with_aliases`]).
///
/// This phase, called after `aliases_phase::phase_aliases` +
/// `names_phase::build_resolution_names` (so `ctx.alias_registry`,
/// `ctx.resolution_structure_names`, and `ctx.resolution_trait_names` are
/// populated) and **before** [`build_resolution_enums_from_cache`] clones
/// `ctx.enum_defs` into `ctx.resolution_enums` (the set threaded into
/// `compile_expr`), rebuilds each variant's payload from the AST:
///   - `VariantPayload::Unit` (bare) stays `Unit`.
///   - `VariantPayload::Named` resolves every field `TypeExpr`; an unresolvable
///     type becomes `Type::Error` (the resolver already pushed a diagnostic —
///     anti-cascade), keeping the field NAME so the downstream construction
///     field-set check still sees it.
///
/// The AST enum declarations are iterated in the same order
/// `collect_decl_refs` pushed them, so they align 1:1 with `ctx.enum_defs`.
pub(crate) fn resolve_enum_variant_payloads(ctx: &mut CompilationCtx, parsed: &ParsedModule) {
    // Move enum_defs out so the per-field resolution can borrow the other
    // (disjoint) ctx fields — alias_registry, diagnostics, resolution name sets
    // — without aliasing the enum_defs we are rebuilding.
    let mut enum_defs = std::mem::take(&mut ctx.enum_defs);

    let enum_decls = parsed.declarations.iter().filter_map(|d| match d {
        Declaration::Enum(e) => Some(e),
        _ => None,
    });

    for (enum_decl, enum_def) in enum_decls.zip(enum_defs.iter_mut()) {
        debug_assert_eq!(
            enum_decl.name, enum_def.name,
            "ctx.enum_defs must align 1:1 with parsed enum declarations"
        );
        // Type parameters declared on the enum head (`enum Maybe<T>`) are in
        // scope for the variant field types (`Some { value: T }`); empty for the
        // common non-generic enum.
        let type_param_names: HashSet<String> = enum_decl
            .type_params
            .iter()
            .map(|tp| tp.name.clone())
            .collect();

        let mut new_variants = Vec::with_capacity(enum_decl.variants.len());
        for v in &enum_decl.variants {
            let payload = match &v.payload {
                reify_ast::VariantPayload::Unit => VariantPayload::Unit,
                reify_ast::VariantPayload::Named(fields) => {
                    let mut resolved = Vec::with_capacity(fields.len());
                    for (field_name, type_expr) in fields {
                        // Direct field accesses on `ctx` borrow disjoint fields,
                        // so `&ctx.alias_registry` and `&mut ctx.diagnostics`
                        // coexist (split borrows).
                        let ty = resolve_type_expr_with_aliases(
                            type_expr,
                            &type_param_names,
                            &ctx.alias_registry,
                            &mut ctx.diagnostics,
                            &ctx.resolution_structure_names,
                            &ctx.resolution_trait_names,
                        )
                        .unwrap_or(Type::Error);
                        resolved.push((field_name.clone(), ty));
                    }
                    VariantPayload::Named(resolved)
                }
            };
            new_variants.push(EnumVariantDef {
                name: v.name.clone(),
                payload,
            });
        }
        enum_def.variants = new_variants;
    }

    ctx.enum_defs = enum_defs;
}

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
