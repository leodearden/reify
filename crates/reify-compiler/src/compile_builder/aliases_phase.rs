//! Phase-5 type aliases: detect duplicate alias declarations and resolve
//! every alias via DFS with cycle detection.
//!
//! Populates `ctx.alias_registry` from `alias_refs`; `alias_decl_map` and
//! the `resolving` cycle-detection set stay phase-local because they are
//! only consumed inside this phase.
//!
//! # Prelude seeding
//!
//! When `prelude_aliases` is non-empty, pub aliases from the prelude are
//! seeded into `ctx.alias_registry` BEFORE any user-module alias is
//! processed.  One invariant is maintained:
//!
//! 1. **User shadows prelude** — if the user module declares an alias with
//!    the same name as a prelude alias, the prelude entry is skipped at seed
//!    time (`user_alias_names` set). This ensures the user's own DFS
//!    resolution wins without producing a duplicate-alias diagnostic.
//!
//! Parametric prelude aliases (non-empty `type_params`) are seeded via the
//! SAME path as non-parametric ones — `from_compiled_for_prelude` carries
//! the raw `TypeExpr` body across the module boundary so use-site
//! instantiation via `resolve_parameterized_alias` can substitute concrete
//! type args at compile time.  This is PRELUDE-ONLY: general user-module →
//! user-module alias import is the deferred bookmark (#4687 out-of-scope).
//!
//! **Cross-phase note:** cross-prelude alias collisions (two prelude modules
//! declaring the same pub alias name) are resolved by `PreludeContext::new`
//! *before* this phase is called — the `prelude_aliases` slice passed to
//! `phase_aliases` is already deduplicated (first-wins).  The seed loop below
//! is therefore idempotent with respect to prelude-vs-prelude duplicates.
//! See `prelude_context` § "Cross-prelude collision policy" for the full
//! cross-phase comparison (units = last-wins/warns; aliases =
//! first-wins/warns; functions = first-wins/silent).

use std::collections::{HashMap, HashSet};

use reify_ast::TypeAliasDecl;
use reify_core::{Diagnostic, DiagnosticLabel};

use crate::compile_builder::ctx::CompilationCtx;
use crate::compile_builder::traits_phase::build_trait_registry;
use crate::type_resolution::{TypeAliasEntry, resolve_alias_dfs, validate_pub_parametric_alias_def_site};
use crate::types::{CompiledModule, CompiledTrait, CompiledTypeAlias, EntityKind, TopologyTemplate};

/// Run phase-5 (type aliases).
///
/// **Execution order:**
/// 1. Seed `ctx.alias_registry` with pub entries from `prelude_aliases`
///    (skipping any name already declared by the user module, so user aliases
///    always shadow prelude aliases without producing a "duplicate type alias"
///    diagnostic).  Parametric prelude aliases (non-empty `type_params`) are
///    seeded via the same path — their raw `TypeExpr` body (carried by
///    `CompiledTypeAlias.type_expr` since task 4792) enables use-site
///    instantiation via `resolve_parameterized_alias`.
/// 2. Build a name → decl lookup map over user `alias_refs`, emitting
///    duplicate-alias diagnostics (keyed on `span` so both declared-here
///    locations are pointed at).
/// 3. DFS-resolve each user alias into `ctx.alias_registry`.  Cycles are
///    caught via the phase-local `resolving` set.
///
/// Pass `&[]` for `prelude_aliases` when the `#no_prelude` pragma is active or
/// when the caller has no prelude (mirrors the `resolution_enums` gate in
/// `lib.rs::compile_with_prelude_context`).
pub(crate) fn phase_aliases(
    ctx: &mut CompilationCtx,
    prelude_aliases: &[CompiledTypeAlias],
    alias_refs: &[&TypeAliasDecl],
) {
    // Collect user-declared alias names so we can let them shadow prelude entries.
    let user_alias_names: HashSet<&str> = alias_refs.iter().map(|d| d.name.as_str()).collect();

    // Seed prelude aliases (user names take precedence; parametric and
    // non-parametric aliases both flow through the same path — the raw
    // TypeExpr body carried by CompiledTypeAlias.type_expr enables use-site
    // instantiation for parametric ones).
    for pa in prelude_aliases {
        // Skip if user declared their own alias with this name (user wins).
        if user_alias_names.contains(pa.name.as_str()) {
            continue;
        }
        let entry = TypeAliasEntry::from_compiled_for_prelude(pa);
        // Use register_as_prelude_seed so the entry is excluded from
        // into_compiled() and iter() (which feed the module's exported
        // type_aliases and content hash).  Silently ignore Err — duplicates
        // are already deduplicated by PreludeContext::new (first-wins).
        let _ = ctx.alias_registry.register_as_prelude_seed(entry);
    }

    // Build a lookup map of all user alias declarations, detecting duplicates.
    let mut alias_decl_map: HashMap<String, &TypeAliasDecl> = HashMap::new();
    for alias_decl in alias_refs {
        if let Some(first) = alias_decl_map.get(&alias_decl.name) {
            ctx.diagnostics.push(
                Diagnostic::error(format!(
                    "duplicate type alias declaration '{}'",
                    alias_decl.name
                ))
                .with_label(DiagnosticLabel::new(
                    alias_decl.span,
                    "duplicate declared here",
                ))
                .with_label(DiagnosticLabel::new(first.span, "first declared here")),
            );
        } else {
            alias_decl_map.insert(alias_decl.name.clone(), alias_decl);
        }
    }

    // DFS-resolve each user alias with cycle detection via resolving-set.
    let mut resolving = HashSet::new();
    for alias_decl in alias_refs {
        resolve_alias_dfs(
            &alias_decl.name,
            &alias_decl_map,
            &mut ctx.alias_registry,
            &mut resolving,
            &mut ctx.diagnostics,
        );
    }
}

/// Post-entity-phase validation hook: validate every user-declared `pub`
/// parametric type alias at its definition site.
///
/// Iterates `ctx.alias_registry.iter()` (user-declared aliases only, excluding
/// prelude-seeded entries), filters to `is_pub && !type_params.is_empty()`,
/// and calls `validate_pub_parametric_alias_def_site` for each.
///
/// **Call site:** immediately after `phase_pending_bound_checks` in `lib.rs`,
/// where `ctx.alias_registry`, `ctx.resolution_structure_names`,
/// `ctx.resolution_trait_names`, `ctx.templates`, and `ctx.trait_defs` are
/// all fully populated.
///
/// Builds the template registry (prelude structures + local templates) and
/// trait registry (same composition as `phase_pending_bound_checks`) so the
/// def-site param-bound check (case b) has access to required-bound metadata.
pub(crate) fn phase_validate_pub_parametric_alias_defs(
    ctx: &mut CompilationCtx,
    prelude_refs: &[&CompiledModule],
) {
    // Build template registry (prelude structures first, then local override).
    let template_registry: HashMap<String, &TopologyTemplate> = prelude_refs
        .iter()
        .flat_map(|m| m.templates.iter())
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t: &TopologyTemplate| (t.name.clone(), t))
        .chain(ctx.templates.iter().map(|t| (t.name.clone(), t)))
        .collect();

    // Build trait registry (same composition as phase_pending_bound_checks).
    let trait_registry: HashMap<String, &CompiledTrait> =
        build_trait_registry(&ctx.trait_defs, prelude_refs);

    // Collect the entries to validate before mutably borrowing `ctx.diagnostics`.
    let entries_to_validate: Vec<_> = ctx
        .alias_registry
        .iter()
        .filter(|e| e.is_pub && !e.type_params.is_empty())
        .cloned()
        .collect();

    for entry in &entries_to_validate {
        validate_pub_parametric_alias_def_site(
            entry,
            &ctx.alias_registry,
            &ctx.resolution_structure_names,
            &ctx.resolution_trait_names,
            &template_registry,
            &trait_registry,
            &mut ctx.diagnostics,
        );
    }
}
