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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_test_support::TopologyTemplateBuilder;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Collect the containment index for `templates` and return the sorted
    /// container-name list for `child_name` (empty vec if absent).
    fn containers_of(child_name: &str, templates: &[TopologyTemplate]) -> Vec<String> {
        let idx = build_containment_index(templates);
        let mut v = idx.get(child_name).cloned().unwrap_or_default();
        v.sort();
        v
    }

    // ── step-1 tests: build_containment_index ────────────────────────────────

    /// (a) Single parent A with sub C: C's containers should be {A}.
    #[test]
    fn single_parent_maps_child() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("c_inst", "C", vec![])
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        let templates = vec![a, c];

        assert_eq!(containers_of("C", &templates), vec!["A"]);
        // A itself has no container.
        assert_eq!(containers_of("A", &templates), Vec::<String>::new());
    }

    /// (b) Two parents A and B both contain C: C's containers should be {A, B}.
    #[test]
    fn two_parents_map_shared_child() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("c1", "C", vec![])
            .build();
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("c2", "C", vec![])
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        let templates = vec![a, b, c];

        let got = containers_of("C", &templates);
        assert_eq!(got, vec!["A", "B"]);
    }

    /// (c) A top-level template (nobody's child) maps to ∅/absent.
    #[test]
    fn top_level_template_has_no_containers() {
        let top = TopologyTemplateBuilder::new("Top").build();
        let templates = vec![top];
        assert_eq!(containers_of("Top", &templates), Vec::<String>::new());
    }

    /// A template with no sub_components is not a container of anyone.
    #[test]
    fn leaf_template_maps_to_nothing() {
        let leaf = TopologyTemplateBuilder::new("Leaf").build();
        let templates = vec![leaf];
        // "Leaf" is not in the index at all (no parent added it).
        let idx = build_containment_index(&templates);
        assert!(idx.get("Leaf").is_none());
    }

    /// (d) Duplicate sub edges to the same child from one parent dedup to one.
    #[test]
    fn duplicate_sub_edges_deduped() {
        // Parent P contains child C twice (two differently-named subs, same structure_name).
        let p = TopologyTemplateBuilder::new("P")
            .sub_component("x", "C", vec![])
            .sub_component("y", "C", vec![])
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        let templates = vec![p, c];

        // P should appear only once as a container of C.
        let got = containers_of("C", &templates);
        assert_eq!(got, vec!["P"]);
    }
}
