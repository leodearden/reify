//! Compile-time validation that every `sub` declaration's structure resolves
//! to a template the declaring module can see.
//!
//! Task 4528; visibility per task #5523. The lookup set is the declaring
//! module's definition-environment set for templates
//! ([`name_env::resolve_visible`]): its own templates, each user import's
//! `pub` templates, then every stdlib template — the set `reify_eval::DefEnv`
//! resolves a sub's structure in at eval time, so check and eval agree on
//! which names are unknown. There is NO EntityKind filter: stdlib OCCURRENCE
//! templates (e.g. STLOutput) must resolve, as at eval. Compare the filtered
//! registries in `phase_pending_bound_checks` / `phase_sub_override_autos`,
//! which restrict the prelude to `EntityKind::Structure` for trait-bound
//! purposes only.

use super::*;
use crate::name_env;

/// Validate that every `sub` declaration in every local template names a
/// structure its module can see, pushing
/// [`SubComponentDecl::unknown_structure_diagnostic`] for each that does not.
///
/// # Semantics
///
/// - Iterates **all** sub_components unconditionally (scalar `= Foo(...)`,
///   collection `List<T>`, keyed, and specialization-scope forms), mirroring
///   eval's unconditional loop, and checks each sub's
///   [`SubComponentDecl::element_structure_name`].
/// - The diagnostic is the one eval-time graph build reports too (same code,
///   same text), so users see the same error whether check or eval catches
///   it; at compile time its span label points at the offending sub in
///   `reify check` output.
///
/// # Relationship to termination.rs
///
/// `check_recursive_termination` (termination.rs) checks SCC *membership* for
/// subs that target a template WITHIN a recursive SCC — it does not check that
/// the target EXISTS. This function fills that orthogonal role. If this
/// function fires for a given sub, `check_recursive_termination` will silently
/// skip it (the target is absent from both local templates and the SCC, so
/// `scc.contains(&sub.structure_name)` is false).
pub(crate) fn check_sub_structure_existence(
    templates: &[TopologyTemplate],
    prelude: &[&CompiledModule],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for template in templates {
        for sub in &template.sub_components {
            let target = sub.element_structure_name();
            if name_env::resolve_visible::<TopologyTemplate, _>(templates, prelude, target)
                .is_none()
            {
                diagnostics.push(sub.unknown_structure_diagnostic());
            }
        }
    }
}
