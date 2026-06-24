use std::collections::HashMap;

use reify_compiler::TopologyTemplate;

/// The outcome of `nearest_container_objective` — which container (if any) provides
/// the objective inherited by a scope that lacks its own.
///
/// Derives only `Debug` because the `Inherited` variant carries `ObjectiveSet`,
/// which itself carries `CompiledExpr` (not `PartialEq`).
#[derive(Debug)]
pub(crate) enum ContainerObjective {
    /// Exactly one nearest objective-bearing ancestor: inherit its objective.
    Inherited {
        objective: reify_ir::ObjectiveSet,
        /// The name of the single container that provides the objective.
        container: String,
    },
    /// Two or more distinct objective-bearing nearest containers — ambiguous.
    ///
    /// Because `CompiledExpr` lacks `PartialEq`, objectives from distinct containers
    /// are treated as distinct (they reference globally-scoped per-scope `ValueCellId`s,
    /// so two distinct containers always have distinct objectives in every realizable
    /// model). This conservatively surfaces the loud `W_OBJECTIVE_INHERIT_AMBIGUOUS`
    /// diagnostic (δ), never silently mis-inherits — honoring PRD INV-6.
    Ambiguous {
        /// Deterministically ordered names of the conflicting containers.
        containers: Vec<String>,
    },
    /// No objective-bearing ancestor found.
    None,
}

/// Build the reverse containment index: for each template name (child), the list of
/// template names that directly contain it via `sub_components`.
///
/// Mirrors the forward-adjacency construction in `scc.rs::detect_recursive_structures`,
/// but inverts the direction: child → containers (instead of parent → children).
/// Duplicate edges (two subs in one parent referencing the same child) are deduped.
/// Sub names that do not resolve to a known template are skipped.
fn build_containment_index(templates: &[TopologyTemplate]) -> HashMap<String, Vec<String>> {
    // Stub: returns empty map. Implemented in step-2.
    let _ = templates;
    HashMap::new()
}

/// Return the `ContainerObjective` for `template` given the full `templates` slice.
///
/// Walks the reverse containment index (built from `sub_components`) upward from
/// `template`, collecting the FIRST (nearest) objective-bearing ancestor on each path
/// (narrowest-ancestor-wins). Deduplicates collected containers by name:
/// - 0 containers → `None`
/// - 1 container  → `Inherited { objective, container }`
/// - ≥2 containers → `Ambiguous { containers }`
///
/// Termination is guaranteed by a `visited` set that guards untagged cycles.
/// A container tagged `is_recursive` acts as a terminating leaf — it is evaluated
/// (its own objective counts) but its own containers are NOT enqueued (OQ2, PRD §13).
pub(crate) fn nearest_container_objective(
    template: &TopologyTemplate,
    templates: &[TopologyTemplate],
) -> ContainerObjective {
    // Stub: always returns None. Implemented in step-4/step-6/step-8.
    let _ = (template, templates);
    ContainerObjective::None
}
