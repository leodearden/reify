//! Phase-5 type aliases: detect duplicate alias declarations and resolve
//! every alias via DFS with cycle detection.
//!
//! Populates `ctx.alias_registry` from `alias_refs`; `alias_decl_map` and
//! the `resolving` cycle-detection set stay phase-local because they are
//! only consumed inside this phase.

use std::collections::{HashMap, HashSet};

use reify_syntax::TypeAliasDecl;
use reify_types::{Diagnostic, DiagnosticLabel};

use crate::compile_builder::ctx::CompilationCtx;
use crate::type_resolution::resolve_alias_dfs;

/// Run phase-5 (type aliases).
///
/// Builds a name → decl lookup map with duplicate-alias diagnostics (keyed on
/// `span` so both declared-here locations are pointed at), then DFS-resolves
/// each alias into `ctx.alias_registry`. Cycles are caught via the phase-local
/// `resolving` set.
pub(crate) fn phase_aliases(ctx: &mut CompilationCtx, alias_refs: &[&TypeAliasDecl]) {
    // Build a lookup map of all alias declarations, detecting duplicates.
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

    // DFS-resolve each alias with cycle detection via resolving-set.
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
