//! Compile-time validation that every `sub` declaration's `structure_name`
//! resolves to a known template in (local module ∪ prelude).
//!
//! Task 4528. Mirrors the (module ∪ prelude) lookup performed at eval time by
//! `find_template_with_prelude` (crates/reify-eval/src/engine_eval.rs:55) and
//! the unknown-structure diagnostic emitted there (engine_eval.rs:2283/3476).
//!
//! The lookup set is the UNION of all prelude templates and local templates,
//! with NO EntityKind filter — stdlib OCCURRENCE templates (e.g. STLOutput)
//! must resolve, matching eval's behaviour. Compare with the filtered registries
//! in `phase_pending_bound_checks` / `phase_sub_override_autos`, which restrict
//! prelude to `EntityKind::Structure` for trait-bound purposes only.

use super::*;
use std::collections::HashSet;

/// Validate that every `sub` declaration in every local template references a
/// `structure_name` that exists in the combined (prelude ∪ local) template set.
///
/// For each violation, pushes a `Severity::Error` diagnostic with the message:
/// `sub-component "{name}" references unknown structure "{structure_name}"`
/// and a span label at the sub's source span.
///
/// # Semantics
///
/// - Iterates **all** sub_components unconditionally (scalar `= Foo(...)`,
///   collection `List<T>`, and specialization-scope forms), mirroring eval's
///   unconditional loop. For collection subs, the element type name lands in
///   `structure_name` (`is_collection=true`).
/// - The lookup set is built from ALL templates across `prelude` (no
///   `EntityKind` filter) plus `templates`, then `structure_name` is checked
///   for membership.  This is semantically equivalent to eval's `or_else`
///   chain: module-first, then prelude fallback (precedence only matters when
///   SELECTING which template to use, not when checking EXISTENCE).
/// - The message wording is byte-identical to the eval backstop so that users
///   see the same text whether the error is caught at compile time or eval time.
///   The compile-time version adds a span label (the compiler-diagnostic idiom,
///   see termination.rs) to point at the offending sub in `reify check` output.
///
/// # Relationship to termination.rs
///
/// `check_recursive_termination` (termination.rs) checks SCC *membership* for
/// subs that target a template WITHIN a recursive SCC — it does not check that
/// the target EXISTS in the full (module ∪ prelude) set. This function fills
/// that orthogonal role. If this function fires for a given sub,
/// `check_recursive_termination` will silently skip it (the target is absent
/// from both local templates and the SCC, so `scc.contains(&sub.structure_name)`
/// is false).
pub(crate) fn check_sub_structure_existence(
    templates: &[TopologyTemplate],
    prelude: &[&CompiledModule],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Build the set of known template names: prelude first, then local.
    // No EntityKind filter — occurrence templates (STLOutput, STEPOutput, …)
    // must resolve, matching eval's find_template_with_prelude contract.
    let mut known: HashSet<&str> = HashSet::new();
    for pm in prelude {
        for t in &pm.templates {
            known.insert(t.name.as_str());
        }
    }
    for t in templates {
        known.insert(t.name.as_str());
    }

    // Check every sub in every local template.
    for template in templates {
        for sub in &template.sub_components {
            if !known.contains(sub.structure_name.as_str()) {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "sub-component \"{}\" references unknown structure \"{}\"",
                        sub.name, sub.structure_name
                    ))
                    .with_label(DiagnosticLabel::new(
                        sub.span,
                        "references unknown structure",
                    )),
                );
            }
        }
    }
}
