use std::collections::{HashMap, HashSet, VecDeque};

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
    // Forward name→index map (mirrors scc.rs::detect_recursive_structures).
    let name_to_idx: HashMap<&str, usize> = templates
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.as_str(), i))
        .collect();

    // Reverse index: child name → Vec of container names.
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for container in templates {
        // Collect children, resolve each sub's structure_name to a known template,
        // dedup duplicate edges (same child referenced by two differently-named subs).
        let mut child_indices: Vec<usize> = container
            .sub_components
            .iter()
            .filter_map(|sub| name_to_idx.get(sub.structure_name.as_str()).copied())
            .collect();
        child_indices.sort_unstable();
        child_indices.dedup();

        for child_idx in child_indices {
            index
                .entry(templates[child_idx].name.clone())
                .or_default()
                .push(container.name.clone());
        }
    }

    // Sort per-child container lists for deterministic output.
    for containers in index.values_mut() {
        containers.sort_unstable();
    }

    index
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
    // Build the reverse containment index and a name→template lookup.
    let index = build_containment_index(templates);
    let name_to_template: HashMap<&str, &TopologyTemplate> =
        templates.iter().map(|t| (t.name.as_str(), t)).collect();

    // BFS upward from `template`.  For each upward path, stop at the FIRST
    // (nearest) objective-bearing container — narrowest-ancestor-wins.
    // The `visited` set is seeded with the target's own name so we never
    // re-enter it, and guards against untagged cycles.
    let mut visited: HashSet<String> = HashSet::from_iter([template.name.clone()]);
    let mut queue: VecDeque<String> = VecDeque::new();

    if let Some(direct_parents) = index.get(&template.name) {
        for p in direct_parents {
            if visited.insert(p.clone()) {
                queue.push_back(p.clone());
            }
        }
    }

    // Collect (name, objective) for each objective-bearing nearest ancestor.
    let mut found: Vec<(String, reify_ir::ObjectiveSet)> = Vec::new();

    while let Some(container_name) = queue.pop_front() {
        let Some(container) = name_to_template.get(container_name.as_str()) else {
            continue;
        };

        if let Some(obj) = &container.objective {
            // Objective-bearing container: record it as the nearest for paths
            // reaching it; do NOT enqueue its own containers (stop ascending).
            found.push((container_name, obj.clone()));
        } else {
            // Objective-less: continue ascending toward grandparent containers.
            if let Some(parents) = index.get(&container_name) {
                for p in parents {
                    if visited.insert(p.clone()) {
                        queue.push_back(p.clone());
                    }
                }
            }
        }
    }

    // Dedup found containers by name: a diamond topology may reach the same
    // objective-bearing container via two distinct paths; sort then dedup.
    found.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    found.dedup_by(|a, b| a.0 == b.0);

    match found.len() {
        0 => ContainerObjective::None,
        1 => {
            let (name, obj) = found.remove(0);
            ContainerObjective::Inherited {
                objective: obj,
                container: name,
            }
        }
        // ≥2 distinct objective-bearing nearest containers — see note in
        // `Ambiguous` variant doc.  The Ambiguous return branch is wired in
        // step-6; return None as a temporary placeholder.
        _ => ContainerObjective::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::Type;
    use reify_ir::{CompiledExpr, ObjectiveSense, ObjectiveSet, Value};
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

    // ── step-3 tests: nearest_container_objective — Inherited + None ─────────

    fn minimize_expr() -> CompiledExpr {
        CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar())
    }

    fn minimize_obj() -> ObjectiveSet {
        ObjectiveSet::single(ObjectiveSense::Minimize, minimize_expr())
    }

    /// 3-LEVEL: C ⊂ B ⊂ A, only A bears an objective.
    /// Both C and B should inherit from A (ascending through objective-less B).
    #[test]
    fn three_level_chain_inherits_from_top() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("b_inst", "B", vec![])
            .objective(minimize_obj())
            .build();
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("c_inst", "C", vec![])
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        let templates = vec![a, b, c];

        // C should see A as its nearest objective-bearing ancestor.
        match nearest_container_objective(&templates[2], &templates) {
            ContainerObjective::Inherited { container, objective } => {
                assert_eq!(container, "A");
                assert_eq!(objective.terms[0].sense, ObjectiveSense::Minimize);
            }
            other => panic!("expected Inherited for C, got {:?}", other),
        }

        // B also sees A as its nearest objective-bearing ancestor.
        match nearest_container_objective(&templates[1], &templates) {
            ContainerObjective::Inherited { container, objective } => {
                assert_eq!(container, "A");
                assert_eq!(objective.terms[0].sense, ObjectiveSense::Minimize);
            }
            other => panic!("expected Inherited for B, got {:?}", other),
        }
    }

    /// Direct child under a single objective-bearing parent → Inherited.
    #[test]
    fn direct_child_inherits_parent_objective() {
        let parent = TopologyTemplateBuilder::new("Parent")
            .sub_component("child_inst", "Child", vec![])
            .objective(minimize_obj())
            .build();
        let child = TopologyTemplateBuilder::new("Child").build();
        let templates = vec![parent, child];

        match nearest_container_objective(&templates[1], &templates) {
            ContainerObjective::Inherited { container, .. } => {
                assert_eq!(container, "Parent");
            }
            other => panic!("expected Inherited, got {:?}", other),
        }
    }

    /// No objective anywhere in the chain → None.
    #[test]
    fn no_objective_anywhere_returns_none() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("b_inst", "B", vec![])
            .build();
        let b = TopologyTemplateBuilder::new("B").build();
        let templates = vec![a, b];

        match nearest_container_objective(&templates[1], &templates) {
            ContainerObjective::None => {}
            other => panic!("expected None, got {:?}", other),
        }
    }

    /// Top-level template (no container) → None.
    #[test]
    fn top_level_returns_none() {
        let top = TopologyTemplateBuilder::new("Top")
            .objective(minimize_obj())
            .build();
        let templates = vec![top];

        match nearest_container_objective(&templates[0], &templates) {
            ContainerObjective::None => {}
            other => panic!("expected None for top-level, got {:?}", other),
        }
    }

    // ── step-5 tests: Ambiguous + diamond dedup ───────────────────────────────

    fn maximize_obj() -> ObjectiveSet {
        ObjectiveSet::single(
            ObjectiveSense::Maximize,
            CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
        )
    }

    /// (a) C reused under TWO distinct objective-bearing parents A (Minimize) and
    /// B (Maximize) → Ambiguous with names {A, B}.
    #[test]
    fn two_objective_parents_returns_ambiguous() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("c1", "C", vec![])
            .objective(minimize_obj())
            .build();
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("c2", "C", vec![])
            .objective(maximize_obj())
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        let templates = vec![a, b, c];

        match nearest_container_objective(&templates[2], &templates) {
            ContainerObjective::Ambiguous { containers } => {
                let mut names = containers.clone();
                names.sort();
                assert_eq!(names, vec!["A", "B"]);
            }
            other => panic!("expected Ambiguous, got {:?}", other),
        }
    }

    /// (b) Diamond: C ⊂ B1, C ⊂ B2, B1 ⊂ A, B2 ⊂ A (both B1/B2 objective-less,
    /// A bears an objective). Two distinct paths both reach the SAME A → Inherited,
    /// NOT Ambiguous (dedup-by-name collapses the diamond).
    #[test]
    fn diamond_single_top_returns_inherited() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("b1_inst", "B1", vec![])
            .sub_component("b2_inst", "B2", vec![])
            .objective(minimize_obj())
            .build();
        let b1 = TopologyTemplateBuilder::new("B1")
            .sub_component("c_inst", "C", vec![])
            .build();
        let b2 = TopologyTemplateBuilder::new("B2")
            .sub_component("c_inst2", "C", vec![])
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        let templates = vec![a, b1, b2, c];

        match nearest_container_objective(&templates[3], &templates) {
            ContainerObjective::Inherited { container, .. } => {
                assert_eq!(container, "A");
            }
            other => panic!("expected Inherited (diamond dedup), got {:?}", other),
        }
    }
}
