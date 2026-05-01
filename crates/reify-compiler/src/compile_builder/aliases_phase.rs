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
//! processed.  Two invariants are maintained:
//!
//! 1. **User shadows prelude** — if the user module declares an alias with
//!    the same name as a prelude alias, the prelude entry is skipped at seed
//!    time (`user_alias_names` set). This ensures the user's own DFS
//!    resolution wins without producing a duplicate-alias diagnostic.
//!
//! 2. **Parametric skip** — prelude aliases with non-empty `type_params` are
//!    skipped (TODO: cross-module parametric propagation requires carrying
//!    `TypeExpr` across the module boundary, which is deferred).
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

use reify_syntax::TypeAliasDecl;
use reify_types::{Diagnostic, DiagnosticLabel};

use crate::compile_builder::ctx::CompilationCtx;
use crate::type_resolution::{TypeAliasEntry, resolve_alias_dfs};
use crate::types::CompiledTypeAlias;

/// Run phase-5 (type aliases).
///
/// **Execution order:**
/// 1. Seed `ctx.alias_registry` with non-parametric pub entries from
///    `prelude_aliases` (skipping any name already declared by the user module,
///    so user aliases always shadow prelude aliases without producing a
///    "duplicate type alias" diagnostic).
/// 2. Build a name → decl lookup map over user `alias_refs`, emitting
///    duplicate-alias diagnostics (keyed on `span` so both declared-here
///    locations are pointed at).
/// 3. DFS-resolve each user alias into `ctx.alias_registry`.  Cycles are
///    caught via the phase-local `resolving` set.
///
/// **Parametric-alias limitation:** prelude aliases with non-empty `type_params`
/// are silently skipped (see the module-level doc and the TODO in the seed loop).
/// `CompiledTypeAlias` deliberately omits `type_expr`, so parameterized prelude
/// aliases cannot be instantiated cross-module until the module boundary
/// decision is revisited.
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

    // Seed prelude aliases first (non-parametric only; user names take precedence).
    for pa in prelude_aliases {
        // Skip parametric aliases — CompiledTypeAlias omits type_expr so they
        // cannot be instantiated at use sites. TODO: revisit when the module
        // boundary allows carrying type_expr cross-module.
        // Mark the name in the registry so that resolve_type_expr_with_aliases
        // can emit a Severity::Info hint at use sites, pointing the user at the
        // cross-module propagation limitation.
        // Shadow guard: only mark when the user has NOT declared their own alias
        // for this name — if they have, their alias wins at lookup and emitting
        // Info about cross-module propagation would be misleading.
        // This guard is verified by the
        // `user_shadowed_parametric_prelude_alias_emits_no_info_diagnostic` test.
        if !pa.type_params.is_empty() {
            if !user_alias_names.contains(pa.name.as_str()) {
                ctx.alias_registry.mark_skipped_parametric_prelude(pa.name.clone());
            }
            continue;
        }
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
