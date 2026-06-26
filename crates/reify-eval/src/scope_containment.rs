use std::collections::{HashMap, HashSet, VecDeque};

use reify_compiler::containment_graph::sub_component_forward_adjacency;
use reify_compiler::TopologyTemplate;

/// The outcome of `nearest_container_objective` — which container (if any) provides
/// the objective inherited by a scope that lacks its own.
///
/// Derives only `Debug` because the `Inherited` variant carries `ObjectiveSet`,
/// which itself carries `CompiledExpr` (not `PartialEq`).
#[derive(Debug)]
pub enum ContainerObjective {
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

/// Pre-built reverse containment index for repeated [`nearest_container_objective`]
/// queries over the same template slice.
///
/// Build once with [`ContainmentIndex::new`] and call
/// [`ContainmentIndex::nearest_container_objective`] for each template that needs
/// an answer. This avoids the O(N+E) index rebuild and O(N) name-map rebuild that
/// would otherwise occur on every per-template query.
///
/// The free function [`nearest_container_objective`] builds the index internally
/// as a one-shot convenience wrapper.
pub struct ContainmentIndex<'a> {
    /// Reverse containment: child template index → sorted Vec of container template indices.
    ///
    /// `reverse[i]` contains the slice indices of every template that lists
    /// `templates[i]` as a sub-component. Indices are sorted for deterministic
    /// Ambiguous ordering without a secondary sort pass. Every template has a row
    /// (even leaves whose row is empty).
    reverse: Vec<Vec<usize>>,
    /// The templates slice for O(1) index → template lookups (replaces `name_to_template`).
    templates: &'a [TopologyTemplate],
    /// Forward: template name → slice index (for name-keyed query entry + Ambiguous ordering).
    name_to_idx: HashMap<&'a str, usize>,
}

impl<'a> ContainmentIndex<'a> {
    /// Build the containment index from `templates`.
    ///
    /// Constructs the reverse `sub_components` adjacency (child → containers) and
    /// two forward maps for fast per-query lookups.
    ///
    /// **Duplicate-name note:** on duplicate template names (an upstream compile
    /// error) the forward maps keep the **last** entry (HashMap::collect is
    /// last-wins). This differs from `scc.rs::detect_recursive_structures`, which
    /// uses `entry().or_insert()` (first-wins). The divergence is benign because
    /// duplicate template names are rejected as a compile error upstream.
    ///
    /// Duplicate sub edges (two subs in one parent referencing the same
    /// `structure_name`) are deduped per container. Sub names that do not resolve
    /// to a known template are skipped.
    pub fn new(templates: &'a [TopologyTemplate]) -> Self {
        // Forward name→index map (last-wins on duplicates; see doc-comment above).
        let name_to_idx: HashMap<&'a str, usize> = templates
            .iter()
            .enumerate()
            .map(|(i, t)| (t.name.as_str(), i))
            .collect();

        // Build forward adjacency using the shared helper (same byte-identical
        // per-row logic as scc.rs::detect_recursive_structures).
        let forward = sub_component_forward_adjacency(templates, &name_to_idx);

        // Transpose forward adjacency into reverse:
        // for each container idx c, push c onto reverse[child_idx] for every child.
        let mut reverse: Vec<Vec<usize>> = vec![vec![]; templates.len()];
        for (container_idx, children) in forward.iter().enumerate() {
            for &child_idx in children {
                reverse[child_idx].push(container_idx);
            }
        }
        // Sort each per-child container list for deterministic Ambiguous ordering.
        for containers in &mut reverse {
            containers.sort_unstable();
        }

        Self { reverse, templates, name_to_idx }
    }

    /// Return the `ContainerObjective` for `template`.
    ///
    /// Walks the reverse containment index upward from `template`, collecting the
    /// FIRST (nearest) objective-bearing ancestor on each path (narrowest-ancestor-wins).
    /// Deduplicates collected containers by name:
    /// - 0 containers → `None`
    /// - 1 container  → `Inherited { objective, container }`
    /// - ≥2 containers → `Ambiguous { containers }`
    ///
    /// Termination is guaranteed by a `visited` set that guards untagged cycles.
    /// A container tagged `is_recursive` acts as a terminating leaf — it is evaluated
    /// (its own objective counts) but its own containers are NOT enqueued (OQ2, PRD §13).
    pub fn nearest_container_objective(&self, template: &TopologyTemplate) -> ContainerObjective {
        // Resolve the query template to its slice index.  If the name is not in the
        // index (e.g. the template was not part of the original slice), return None.
        let Some(&start_idx) = self.name_to_idx.get(template.name.as_str()) else {
            return ContainerObjective::None;
        };

        // BFS upward from start_idx.  `visited` is a dense bitset (Vec<bool>) over
        // template indices — no per-step allocation.  Seeded true at start_idx so we
        // never re-enter the query template itself, and guards against untagged cycles.
        let n = self.templates.len();
        let mut visited = vec![false; n];
        visited[start_idx] = true;
        let mut queue: VecDeque<usize> = VecDeque::new();

        // Enqueue direct containers of the query template.
        for &ci in &self.reverse[start_idx] {
            if !visited[ci] {
                visited[ci] = true;
                queue.push_back(ci);
            }
        }

        // Collect indices of objective-bearing nearest ancestors.
        //
        // Invariant: `found` contains at most one entry per unique container index.
        // `visited[ci] = true` is set before any ci is enqueued, so the same index
        // can never be enqueued — and therefore never popped into `found` — more
        // than once.  The debug_assert below documents and verifies this invariant.
        let mut found: Vec<usize> = Vec::new();

        while let Some(ci) = queue.pop_front() {
            let container = &self.templates[ci];

            if container.objective.is_some() {
                // Objective-bearing container: record it as the nearest for paths
                // reaching it; do NOT enqueue its own containers (stop ascending).
                // NOTE: is_recursive containers that bear an objective are handled
                // here — they count as the nearest ancestor and terminate ascent.
                found.push(ci);
            } else if container.is_recursive {
                // Recursive, objective-less terminating leaf (PRD §13 OQ2).
                //
                // `is_recursive` is set by `scc.rs::detect_recursive_structures`
                // (Tarjan SCC) for any template involved in a containment cycle.
                // We treat it as a hard stop: the container's own (absent) objective
                // is evaluated (nothing to record), but its OWN containers are NEVER
                // enqueued — preventing infinite ascent through cyclic topologies.
                //
                // The `visited` set is a second safety net for any untagged cycle
                // that the SCC pass might have missed, but `is_recursive` is the
                // primary semantic guard here (the PRD OQ2 resolution).
            } else {
                // Objective-less, non-recursive: continue ascending toward
                // grandparent containers (narrowest-ancestor-wins still applies
                // because we stop the first time we find an objective-bearing node).
                for &pi in &self.reverse[ci] {
                    if !visited[pi] {
                        visited[pi] = true;
                        queue.push_back(pi);
                    }
                }
            }
        }

        // The `visited` array guarantees that `found` contains no duplicate indices:
        // an index reaches `found` only via a queue pop, and the queue is populated
        // only when `visited[ci]` transitions false → true — so the same container
        // can never be enqueued (and therefore never popped into `found`) more than once.
        debug_assert!(
            {
                let mut seen = HashSet::new();
                found.iter().all(|&i| seen.insert(i))
            },
            "found contains duplicate container indices — visited set invariant broken"
        );

        match found.len() {
            0 => ContainerObjective::None,
            1 => {
                let ci = found[0];
                let container = &self.templates[ci];
                ContainerObjective::Inherited {
                    objective: container.objective.clone().expect(
                        "pushed to found only when objective.is_some() — invariant violated",
                    ),
                    container: container.name.clone(),
                }
            }
            // ≥2 distinct objective-bearing nearest containers.
            //
            // We treat distinct containers as having distinct objectives because
            // `CompiledExpr` lacks `PartialEq` (reify-ir/src/expr.rs). Each
            // container's objective references its own globally-scoped per-scope
            // ValueCellIds; two distinct containers therefore always produce distinct
            // objectives in every realizable model. Conservative: always surface the
            // loud `W_OBJECTIVE_INHERIT_AMBIGUOUS` (δ), never silently mis-inherit —
            // per PRD INV-6.
            _ => {
                // Order by template-slice index for deterministic diagnostic output.
                // Sorting `found` directly (which contains indices) is equivalent to
                // the old name_to_idx-lookup sort, and avoids an extra allocation.
                found.sort_unstable();
                ContainerObjective::Ambiguous {
                    containers: found.iter().map(|&i| self.templates[i].name.clone()).collect(),
                }
            }
        }
    }
}

/// Return the `ContainerObjective` for `template` given the full `templates` slice.
///
/// Builds a [`ContainmentIndex`] from `templates` on each call. When resolving
/// inheritance for multiple templates over the same slice, prefer building a
/// [`ContainmentIndex`] once and calling its
/// [`ContainmentIndex::nearest_container_objective`] method directly to avoid the
/// O(N+E) index rebuild on each call.
pub fn nearest_container_objective(
    template: &TopologyTemplate,
    templates: &[TopologyTemplate],
) -> ContainerObjective {
    ContainmentIndex::new(templates).nearest_container_objective(template)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::Type;
    use reify_ir::{CompiledExpr, ObjectiveSense, ObjectiveSet, Value};
    use reify_test_support::TopologyTemplateBuilder;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Collect the reverse containment index for `templates` and return the sorted
    /// container-name list for `child_name` (empty vec if absent or not in slice).
    fn containers_of(child_name: &str, templates: &[TopologyTemplate]) -> Vec<String> {
        let idx = ContainmentIndex::new(templates);
        let child_idx = match idx.name_to_idx.get(child_name) {
            Some(&i) => i,
            None => return vec![],
        };
        let mut v: Vec<String> =
            idx.reverse[child_idx].iter().map(|&ci| idx.templates[ci].name.clone()).collect();
        v.sort();
        v
    }

    // ── step-1 tests: ContainmentIndex::new / .reverse ───────────────────────

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
        // "Leaf" is at index 0; every idx has a row, so reverse[0] exists but is empty.
        let idx = ContainmentIndex::new(&templates);
        assert!(idx.reverse[0].is_empty());
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
    /// NOT Ambiguous.
    ///
    /// The `visited` set prevents A from being enqueued a second time when B2's
    /// parents are processed (A was already visited via B1's path), so `found`
    /// contains A exactly once.  This exercises the visited-set dedup invariant
    /// documented by the `debug_assert` in the walk.
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

    /// Deterministic Ambiguous ordering: 'Z' (at slice index 0) and 'A' (at slice
    /// index 1) are both direct objective-bearing parents of C. The sort-by-slice-index
    /// ordering must place Z before A in the returned `containers` vec.
    ///
    /// The existing `two_objective_parents_returns_ambiguous` test uses A (index 0) and
    /// B (index 1), where alphabetical order coincides with slice-index order, so a
    /// regression to HashMap-iteration or alphabetical ordering would still pass that
    /// test. This test uses Z-before-A (by index) vs A-before-Z (alphabetically) to
    /// lock in the index-based guarantee.
    ///
    /// The `containers` vec is asserted WITHOUT re-sorting to catch any regression.
    #[test]
    fn ambiguous_ordering_is_by_slice_index_not_name() {
        let z = TopologyTemplateBuilder::new("Z")
            .sub_component("c1", "C", vec![])
            .objective(minimize_obj())
            .build();
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("c2", "C", vec![])
            .objective(maximize_obj())
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        // Z at index 0, A at index 1 — slice-index order is Z→A, alphabetical is A→Z.
        let templates = vec![z, a, c];

        match nearest_container_objective(&templates[2], &templates) {
            ContainerObjective::Ambiguous { containers } => {
                // Must be ["Z", "A"] (index order), NOT ["A", "Z"] (alphabetical).
                assert_eq!(containers, vec!["Z", "A"]);
            }
            other => panic!("expected Ambiguous with Z before A (index order), got {:?}", other),
        }
    }

    /// Mixed-depth Ambiguous: C has two parents — D (direct, bears obj at depth 1)
    /// and B (no obj, at depth 1), where B is itself contained by A (bears obj at depth 2).
    ///
    /// Walk from C:
    ///   - D → has objective → collected as nearest on that path (stop ascending D)
    ///   - B → no objective → continue up → A → has objective → collected as nearest
    ///
    /// Two distinct objective-bearing containers (D and A) are found at different
    /// depths → Ambiguous. This verifies that the walk correctly accumulates nearest
    /// objectives across paths of different depths, not just the two-direct-parents case.
    #[test]
    fn mixed_depth_ambiguous_direct_and_indirect() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("b_inst", "B", vec![])
            .objective(minimize_obj())
            .build();
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("c_inst", "C", vec![])
            .build();
        let d = TopologyTemplateBuilder::new("D")
            .sub_component("c_inst2", "C", vec![])
            .objective(maximize_obj())
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        // A at index 0, B at index 1, D at index 2, C at index 3.
        let templates = vec![a, b, d, c];

        match nearest_container_objective(&templates[3], &templates) {
            ContainerObjective::Ambiguous { containers } => {
                // A (index 0) before D (index 2) by slice-index ordering.
                assert_eq!(containers, vec!["A", "D"]);
            }
            other => panic!("expected Ambiguous (mixed depth A+D), got {:?}", other),
        }
    }

    // ── step-7 tests: recursive-containment safety (PRD §13 OQ2) ─────────────

    /// (a) Self-referential A⊂A (tagged recursive) terminates without hanging,
    /// and since A has no objective, returns `None`.
    ///
    /// Termination here is guaranteed by the `visited` set (seeded with "A"),
    /// which prevents re-enqueueing A regardless of `is_recursive`.
    #[test]
    fn self_referential_recursive_terminates() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("a_inst", "A", vec![])
            .is_recursive(true)
            .build();
        let templates = vec![a];

        // No objective anywhere → None.
        match nearest_container_objective(&templates[0], &templates) {
            ContainerObjective::None => {}
            other => panic!("expected None for self-referential A, got {:?}", other),
        }
    }

    /// (a) Mutual recursion A⊂B, B⊂A (both tagged recursive) terminates.
    /// Both A and B are objective-less, so both queries return None.
    #[test]
    fn mutual_recursive_terminates() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("b_inst", "B", vec![])
            .is_recursive(true)
            .build();
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("a_inst", "A", vec![])
            .is_recursive(true)
            .build();
        let templates = vec![a, b];

        // A's walk: A → B (recursive, no obj → terminating leaf) → None.
        match nearest_container_objective(&templates[0], &templates) {
            ContainerObjective::None => {}
            other => panic!("expected None for A in mutual recursion, got {:?}", other),
        }
        // B's walk: B → A (recursive, no obj → terminating leaf) → None.
        match nearest_container_objective(&templates[1], &templates) {
            ContainerObjective::None => {}
            other => panic!("expected None for B in mutual recursion, got {:?}", other),
        }
    }

    /// (b) Terminating-leaf semantics: C ⊂ B (recursive, objective-less) ⊂ A (objective).
    ///
    /// B is `is_recursive` and has no objective of its own.  The walk must treat B as a
    /// terminating leaf — it counts its own (absent) objective but does NOT ascend past
    /// it to A.  Therefore `nearest_container_objective(C)` returns `None`, not
    /// `Inherited { container: "A" }`.
    ///
    /// This distinguishes the is_recursive rule from ordinary objective-less traversal
    /// (where the walk WOULD continue up to A).
    #[test]
    fn recursive_leaf_blocks_ascent_to_objective_bearing_grandparent() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("b_inst", "B", vec![])
            .objective(minimize_obj())
            .build();
        // B is recursive and carries NO objective — it should be a terminating leaf.
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("c_inst", "C", vec![])
            .is_recursive(true)
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        let templates = vec![a, b, c];

        // The walk: C → B (recursive, no obj → STOP, do not enqueue A) → None.
        // Without is_recursive handling the walk would continue: C → B → A → Inherited.
        match nearest_container_objective(&templates[2], &templates) {
            ContainerObjective::None => {}
            other => panic!(
                "expected None (recursive B terminates ascent), got {:?}",
                other
            ),
        }
    }

    /// (c) A recursive container that DOES bear an objective is still returned as
    /// `Inherited` for a child reaching it (it terminates ascent but counts itself).
    ///
    /// This guards against a naive fix that skips recursive containers entirely.
    #[test]
    fn recursive_container_with_objective_returns_inherited() {
        // B is recursive AND bears an objective.
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("c_inst", "C", vec![])
            .objective(minimize_obj())
            .is_recursive(true)
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        let templates = vec![b, c];

        // The walk: C → B (recursive, HAS obj → record Inherited, STOP) → Inherited{"B"}.
        match nearest_container_objective(&templates[1], &templates) {
            ContainerObjective::Inherited { container, objective } => {
                assert_eq!(container, "B");
                assert_eq!(objective.terms[0].sense, ObjectiveSense::Minimize);
            }
            other => panic!(
                "expected Inherited (recursive B has own objective), got {:?}",
                other
            ),
        }
    }

    // ── suggestion-#3 coverage: dangling sub-component reference silently skipped

    /// Parent P contains a sub that references "Ghost" (no such template in the
    /// slice) plus a real sub referencing "C".  The filter_map in
    /// `ContainmentIndex::new` must skip the unresolved Ghost edge without
    /// panicking, and the P→C edge must survive so C returns Inherited{"P"}.
    ///
    /// Characterization test: expected GREEN on arrival; locks the skip-branch
    /// behavior before the step-6 refactor.
    #[test]
    fn sub_referencing_undefined_structure_is_skipped() {
        let p = TopologyTemplateBuilder::new("P")
            .sub_component("ghost_inst", "Ghost", vec![])
            .sub_component("c_inst", "C", vec![])
            .objective(minimize_obj())
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        // P at index 0, C at index 1; no "Ghost" template present.
        let templates = vec![p, c];

        // Must not panic even though "Ghost" is absent from the slice.
        match nearest_container_objective(&templates[1], &templates) {
            ContainerObjective::Inherited { container, objective } => {
                assert_eq!(container, "P", "C should inherit from P despite Ghost dangling");
                assert_eq!(objective.terms[0].sense, ObjectiveSense::Minimize);
            }
            other => panic!("expected Inherited{{P}}, got {:?}", other),
        }
    }

    // ── suggestion-#2 coverage: ContainmentIndex reuse == free-function ──────

    /// Build ONE `ContainmentIndex` for C ⊂ B ⊂ A (A bears minimize_obj) and
    /// run two queries against it.  Each result must match the free-function path
    /// (which rebuilds the index per call) — proving the reuse path is equivalent.
    ///
    /// Characterization test: expected GREEN on arrival; locks the reuse-path ==
    /// free-function equivalence BEFORE the step-6 private-field refactor.
    #[test]
    fn containment_index_reused_across_multiple_queries_matches_free_function() {
        let a = TopologyTemplateBuilder::new("A")
            .sub_component("b_inst", "B", vec![])
            .objective(minimize_obj())
            .build();
        let b = TopologyTemplateBuilder::new("B")
            .sub_component("c_inst", "C", vec![])
            .build();
        let c = TopologyTemplateBuilder::new("C").build();
        // A at index 0, B at index 1, C at index 2.
        let templates = vec![a, b, c];

        // Build the index ONCE and reuse it across two queries.
        let idx = ContainmentIndex::new(&templates);

        // Query 1: C should inherit from A (via B).
        let c_idx = idx.nearest_container_objective(&templates[2]);
        let c_free = nearest_container_objective(&templates[2], &templates);
        match (&c_idx, &c_free) {
            (
                ContainerObjective::Inherited { container: c1, objective: obj1 },
                ContainerObjective::Inherited { container: c2, objective: obj2 },
            ) => {
                assert_eq!(c1, "A", "reuse path: C should inherit from A");
                assert_eq!(c2, "A", "free-fn path: C should inherit from A");
                assert_eq!(
                    obj1.terms[0].sense,
                    ObjectiveSense::Minimize,
                    "reuse path: objective sense mismatch for C"
                );
                assert_eq!(
                    obj2.terms[0].sense,
                    ObjectiveSense::Minimize,
                    "free-fn path: objective sense mismatch for C"
                );
            }
            _ => panic!(
                "expected Inherited{{A}} for C from both paths; idx={:?} free={:?}",
                c_idx, c_free
            ),
        }

        // Query 2 (reuse same index): B should also inherit from A.
        let b_idx = idx.nearest_container_objective(&templates[1]);
        let b_free = nearest_container_objective(&templates[1], &templates);
        match (&b_idx, &b_free) {
            (
                ContainerObjective::Inherited { container: c1, objective: obj1 },
                ContainerObjective::Inherited { container: c2, objective: obj2 },
            ) => {
                assert_eq!(c1, "A", "reuse path: B should inherit from A");
                assert_eq!(c2, "A", "free-fn path: B should inherit from A");
                assert_eq!(
                    obj1.terms[0].sense,
                    ObjectiveSense::Minimize,
                    "reuse path: objective sense mismatch for B"
                );
                assert_eq!(
                    obj2.terms[0].sense,
                    ObjectiveSense::Minimize,
                    "free-fn path: objective sense mismatch for B"
                );
            }
            _ => panic!(
                "expected Inherited{{A}} for B from both paths; idx={:?} free={:?}",
                b_idx, b_free
            ),
        }
    }
}
