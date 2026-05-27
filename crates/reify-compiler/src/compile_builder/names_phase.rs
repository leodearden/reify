//! Pre-step: pre-compute the trait-name and structure-name sets on
//! [`CompilationCtx`] so that both `phase_functions` and `phase_traits` can
//! consume them without re-deriving the same information twice.
//!
//! ## Why a separate phase?
//!
//! In the original design, `phase_traits` built `trait_names` and
//! `structure_names` as local `HashSet<String>` values for its own use.
//! Because `phase_functions` runs BEFORE `phase_traits`, `compile_function`
//! had no access to those sets and passed empty ones, causing any fn parameter
//! or return type that referenced a user-defined structure or trait name to
//! produce a spurious "unresolved type" Error diagnostic.
//!
//! Lifting the construction into this tiny pre-step (run after
//! `enums_phase::build_resolution_enums_from_cache`, before
//! `functions_phase::phase_functions`) stores the result on
//! `CompilationCtx.resolution_trait_names` / `CompilationCtx.resolution_structure_names`.
//! Both `phase_functions` and `phase_traits` then read the cached ctx fields
//! instead of rebuilding or doing without.
//!
//! All inputs required for construction are available before `phase_functions`:
//! - `ctx.seen_entity_names` — populated by `pre_pass::collect_decl_refs`
//! - `trait_refs` — populated by `pre_pass::collect_decl_refs`
//! - `prelude.trait_defs` / `prelude.templates` — from the caller's prelude

use reify_ast::TraitDecl;

use crate::CompiledModule;
use crate::compile_builder::ctx::CompilationCtx;

/// Pre-compute the trait-name and structure/occurrence-name sets and store
/// them on `ctx` for consumption by downstream phases.
///
/// **Must be called** after `enums_phase::build_resolution_enums_from_cache`
/// and **before** `functions_phase::phase_functions`.
///
/// Mirrors the construction logic previously local to `phase_traits` (lines
/// 71-97 of the original `traits_phase.rs`) verbatim, so the sets are
/// semantically identical — they just live on `ctx` instead of a local let.
pub(crate) fn build_resolution_names(
    ctx: &mut CompilationCtx,
    prelude: &[&CompiledModule],
    trait_refs: &[&TraitDecl],
) {
    // Trait names: local declarations (from syntax) + every prelude trait def.
    // Collected before compile_trait runs so trait members whose types reference
    // other traits can resolve their siblings. Trait-name resolution is last in
    // precedence (builtins → type params → alias → structure → trait) so
    // existing name-reuse stays backward compatible.
    ctx.resolution_trait_names = trait_refs
        .iter()
        .map(|t| t.name.clone())
        .chain(
            prelude
                .iter()
                .flat_map(|m| m.trait_defs.iter().map(|t| t.name.clone())),
        )
        .collect();

    // Structure/occurrence names: local structure and occurrence declarations
    // (already in ctx.seen_entity_names after pre_pass::collect_decl_refs) +
    // every prelude module's exported templates (topology templates == structure
    // instances whose names are also valid StructureRef targets).
    ctx.resolution_structure_names = ctx
        .seen_entity_names
        .iter()
        .filter(|(_, (_, kind))| *kind == "structure" || *kind == "occurrence")
        .map(|(name, _)| name.clone())
        .chain(
            prelude
                .iter()
                .flat_map(|m| m.templates.iter().map(|t| t.name.clone())),
        )
        .collect();
}
