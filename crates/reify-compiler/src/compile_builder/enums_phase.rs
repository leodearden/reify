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
use crate::type_resolution::{resolve_enum_type_with_args, resolve_type_expr_with_aliases};

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
///   - `VariantPayload::Named` resolves every field `TypeExpr`. A field type
///     may name a builtin, alias, structure, trait, **or another enum**
///     (e.g. `NotConverged { reason: BudgetReason }`): enum names are not known
///     to `resolve_type_expr_with_aliases` (it returns `None` silently for an
///     unknown bare `Named`), so they are resolved by a chained `enum_names`
///     lookup that mirrors the struct-param (`entity.rs`) / trait-member
///     (`traits.rs`) enum fallback via `resolve_enum_type`. A type that resolves
///     to nothing becomes `Type::Error`, keeping the field NAME so the
///     downstream construction field-set check still sees it.
///
/// The AST enum declarations are iterated in the same order
/// `collect_decl_refs` pushed them, so they align 1:1 with `ctx.enum_defs`.
///
/// `prelude_enums` carries the prelude's `EnumDef`s (empty under `#no_prelude`)
/// so a payload field may reference a prelude enum, exactly as a `param` type
/// can. Together with the module-local enum names this is the same merged
/// prelude ++ local enum set that `build_resolution_enums_from_cache` produces.
pub(crate) fn resolve_enum_variant_payloads(
    ctx: &mut CompilationCtx,
    parsed: &ParsedModule,
    prelude_enums: &[EnumDef],
) {
    // Move enum_defs out so the per-field resolution can borrow the other
    // (disjoint) ctx fields — alias_registry, diagnostics, resolution name sets
    // — without aliasing the enum_defs we are rebuilding.
    let mut enum_defs = std::mem::take(&mut ctx.enum_defs);

    // Enum names in scope for variant-payload field types: every module-local
    // enum PLUS every prelude enum, so a payload field can reference a sibling
    // or prelude enum (e.g. `ConvergenceStatus.NotConverged { reason:
    // BudgetReason }`). Owned `String`s (not `&str`) so the set does not alias
    // `enum_defs` during the mutable pass below.
    let enum_names: HashSet<String> = enum_defs
        .iter()
        .map(|e| e.name.clone())
        .chain(prelude_enums.iter().map(|e| e.name.clone()))
        .collect();

    let enum_decls = parsed.declarations.iter().filter_map(|d| match d {
        Declaration::Enum(e) => Some(e),
        _ => None,
    });

    // Pass 1 (immutable over `enum_defs`): resolve every enum's variants.
    // Kept as a separate pass from the write-back below (rather than mutating
    // in place via `iter_mut`) because a self-/sibling-referential GENERIC
    // field — e.g. `left: Tree<T>` inside `enum Tree<T> { Node { left:
    // Tree<T>, .. } }` — resolves via [`resolve_enum_type_with_args`] (task γ
    // #4031 fix), which needs an IMMUTABLE view of the FULL `enum_defs` slice
    // (including the enum currently being processed, for its own
    // `type_params` arity) to build `Type::Applied`. That is incompatible
    // with also holding a mutable borrow of the same slice via `iter_mut`.
    let mut new_variants_by_enum: Vec<Vec<EnumVariantDef>> = Vec::with_capacity(enum_defs.len());
    for (enum_decl, enum_def) in enum_decls.zip(enum_defs.iter()) {
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
                        .or_else(|| {
                            // Enum-typed payload field (e.g.
                            // `NotConverged { reason: BudgetReason }`):
                            // `resolve_type_expr_with_aliases` resolves builtins,
                            // aliases, structures, and traits but NOT enum names
                            // (it returns `None` silently — no diagnostic — for an
                            // unknown bare `Named`). Chain enum resolution here,
                            // the same fallback struct-param (`entity.rs`) and
                            // trait-member (`traits.rs`) type resolution use.
                            match &type_expr.kind {
                                reify_ast::TypeExprKind::Named { name, type_args }
                                    if type_args.is_empty()
                                        && enum_names.contains(name.as_str()) =>
                                {
                                    Some(Type::Enum(name.clone()))
                                }
                                // Generic enum reference WITH type args — e.g. a
                                // self-/sibling-referential `left: Tree<T>` inside
                                // `enum Tree<T> { .. }` (task γ #4031 fix). The
                                // bare-name arm above only covers the ARGLESS case
                                // (a v0.4 legacy restriction predating generic
                                // enums, task β #4030). Reuse
                                // `resolve_enum_type_with_args` — the SAME resolver
                                // `entity.rs` param-type resolution falls back to —
                                // against the MODULE-LOCAL `enum_defs` (covers
                                // self-reference and any sibling module-local
                                // generic enum; it returns `None` when `name`
                                // isn't found there, so a genuinely-unknown name
                                // still falls through to `Type::Error`). A generic
                                // PRELUDE enum referenced with type args in a
                                // payload field is not covered — falls through to
                                // `Type::Error`, unchanged from before this fix —
                                // no fixture exercises that combination.
                                reify_ast::TypeExprKind::Named { name, type_args }
                                    if !type_args.is_empty() =>
                                {
                                    resolve_enum_type_with_args(
                                        name,
                                        type_args,
                                        &enum_defs,
                                        &type_param_names,
                                        &ctx.alias_registry,
                                        &mut ctx.diagnostics,
                                        &ctx.resolution_structure_names,
                                        &ctx.resolution_trait_names,
                                        type_expr.span,
                                    )
                                }
                                _ => None,
                            }
                        })
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
        new_variants_by_enum.push(new_variants);
    }

    // Pass 2 (mutable): write the resolved variants back.
    for (enum_def, new_variants) in enum_defs.iter_mut().zip(new_variants_by_enum) {
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
