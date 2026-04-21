//! Phase-13 module content hash.
//!
//! Combines every piece of owned compiled content from `CompilationCtx`
//! plus `compiled_purposes` and `parsed.path` into a single
//! [`ContentHash`] via `ContentHash::combine_all`. Type-alias hashes are
//! sorted by name so alias-declaration order doesn't perturb the result.

use reify_syntax::ParsedModule;
use reify_types::{CompiledFunction, ContentHash};

use crate::compile_builder::ctx::CompilationCtx;
use crate::types::CompiledPurpose;

/// Compute the module-level content hash by combining the path with every
/// piece of compiled content owned by `ctx` plus the external
/// `compiled_purposes` slice.
pub(crate) fn compute_module_hash(
    ctx: &CompilationCtx,
    parsed: &ParsedModule,
    compiled_purposes: &[CompiledPurpose],
) -> ContentHash {
    let path_hash = ContentHash::of_str(&parsed.path.to_string());

    // Template content hashes
    let template_hashes = ctx.templates.iter().map(|t| t.content_hash);

    // Import path hashes
    let import_hashes = ctx.imports.iter().map(|i| ContentHash::of_str(&i.path));

    // Enum def hashes
    let enum_hashes = ctx.enum_defs.iter().map(|e| {
        let mut h = ContentHash::of_str(&e.name);
        for v in &e.variants {
            h = h.combine(ContentHash::of_str(v));
        }
        h
    });

    // Function content hashes
    let function_hashes = ctx.functions.iter().map(|f: &CompiledFunction| f.content_hash);

    // Trait content hashes
    let trait_hashes = ctx.trait_defs.iter().map(|t| t.content_hash);

    // Field content hashes
    let field_hashes = ctx.fields.iter().map(|f| f.content_hash);

    // Purpose content hashes
    let purpose_hashes = compiled_purposes.iter().map(|p| p.content_hash);

    // Unit content hashes
    let unit_hashes = ctx.compiled_units.iter().map(|u| u.content_hash);

    // Type alias content hashes (sorted by name for deterministic ordering)
    let mut alias_hash_pairs: Vec<_> = ctx
        .alias_registry
        .iter()
        .map(|a| (a.name.clone(), a.content_hash))
        .collect();
    alias_hash_pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let alias_hashes = alias_hash_pairs.into_iter().map(|(_, h)| h);

    let all_hashes = std::iter::once(path_hash)
        .chain(template_hashes)
        .chain(import_hashes)
        .chain(enum_hashes)
        .chain(function_hashes)
        .chain(trait_hashes)
        .chain(field_hashes)
        .chain(purpose_hashes)
        .chain(unit_hashes)
        .chain(alias_hashes);

    ContentHash::combine_all(all_hashes)
}
