use std::collections::{HashMap, HashSet};

use crate::TopologyTemplate;

/// Tags all cycle participants with `is_recursive = true` and emits one warning diagnostic
/// per strongly connected component (SCC) that contains a cycle.
///
/// Uses Tarjan's SCC algorithm to find cycles in the sub-component reference graph.
/// Returns the list of cyclic SCCs (each as a set of template names) for use by the
/// termination check pass.
pub(crate) fn detect_recursive_structures(
    templates: &mut [TopologyTemplate],
    diagnostics: &mut Vec<reify_core::Diagnostic>,
) -> Vec<HashSet<String>> {
    // Build an index: name -> index in templates.
    // Use explicit insertion so that duplicates are detected and reported instead of
    // silently overwriting (which would corrupt the adjacency graph).
    let mut name_to_idx: HashMap<&str, usize> = HashMap::new();
    for (i, t) in templates.iter().enumerate() {
        if let Some(&prev_idx) = name_to_idx.get(t.name.as_str()) {
            diagnostics.push(reify_core::Diagnostic::error(format!(
                "duplicate template name '{}': indices {} and {}",
                t.name, prev_idx, i
            )));
            // Keep the first entry (prev wins) — don't overwrite.
        } else {
            name_to_idx.insert(t.name.as_str(), i);
        }
    }

    // Build adjacency list: for each template index, collect the indices of templates it
    // references via sub_components (only those that exist in the template set).
    // sort_unstable + dedup removes duplicate edges (e.g. two subs referencing the same
    // target), keeping the graph clean and the self-loop check at line 70 principled.
    let adjacency: Vec<Vec<usize>> = templates
        .iter()
        .map(|t| {
            let mut adj: Vec<usize> = t
                .sub_components
                .iter()
                .filter_map(|sub| name_to_idx.get(sub.structure_name.as_str()).copied())
                .collect();
            adj.sort_unstable();
            adj.dedup();
            adj
        })
        .collect();

    let n = templates.len();

    // Tarjan's SCC state
    let mut st = TarjanState {
        index: vec![None; n],
        lowlink: vec![0; n],
        on_stack: vec![false; n],
        scc_stack: Vec::new(),
        index_counter: 0,
        sccs: Vec::new(),
    };

    for start in 0..n {
        if st.index[start].is_none() {
            tarjan_scc_visit(start, &adjacency, &mut st);
        }
    }

    // Single pass: tag cycle participants, emit diagnostics, and collect cyclic SCCs.
    let mut in_cycle = vec![false; n];
    let mut cyclic_sccs: Vec<HashSet<String>> = Vec::new();
    for scc in &st.sccs {
        let is_cycle = if scc.len() > 1 {
            true
        } else {
            // Single-node SCC: cycle only if there is a self-edge
            let v = scc[0];
            adjacency[v].contains(&v)
        };

        if is_cycle {
            for &v in scc {
                in_cycle[v] = true;
            }
            let cycle_path = reconstruct_scc_cycle(scc, &adjacency, templates);
            let scc_set: HashSet<usize> = scc.iter().copied().collect();
            let mut diag = reify_core::Diagnostic::warning(format!(
                "recursive structure cycle detected: {}",
                cycle_path
            ));
            // Add a label for each sub-component declaration that creates a cycle edge
            // (i.e., references another member of the same SCC).
            for &v in scc {
                for sub in &templates[v].sub_components {
                    if let Some(&target) = name_to_idx.get(sub.structure_name.as_str())
                        && scc_set.contains(&target)
                    {
                        diag = diag.with_label(reify_core::DiagnosticLabel::new(
                            sub.span,
                            format!("references {}", sub.structure_name),
                        ));
                    }
                }
            }
            diagnostics.push(diag);
            let scc_names: HashSet<String> =
                scc.iter().map(|&v| templates[v].name.clone()).collect();
            cyclic_sccs.push(scc_names);
        }
    }

    for (i, template) in templates.iter_mut().enumerate() {
        if in_cycle[i] {
            template.is_recursive = true;
        }
    }

    cyclic_sccs
}

/// Mutable state threaded through Tarjan's SCC traversal.
struct TarjanState {
    index: Vec<Option<usize>>,
    lowlink: Vec<usize>,
    on_stack: Vec<bool>,
    scc_stack: Vec<usize>,
    index_counter: usize,
    sccs: Vec<Vec<usize>>,
}

/// Iterative visit for Tarjan's SCC algorithm.
///
/// Uses an explicit call stack to avoid OS stack overflow on deep/large structure graphs.
/// Each frame tracks (node, neighbor_index) so the DFS can be resumed without recursion.
fn tarjan_scc_visit(v: usize, adjacency: &[Vec<usize>], st: &mut TarjanState) {
    // Each frame: (node, index into adjacency[node] for the next neighbor to process)
    let mut call_stack: Vec<(usize, usize)> = Vec::new();

    // Initialize the starting node
    st.index[v] = Some(st.index_counter);
    st.lowlink[v] = st.index_counter;
    st.index_counter += 1;
    st.scc_stack.push(v);
    st.on_stack[v] = true;
    call_stack.push((v, 0));

    while let Some(&mut (node, ref mut neighbor_idx)) = call_stack.last_mut() {
        if *neighbor_idx < adjacency[node].len() {
            let w = adjacency[node][*neighbor_idx];
            *neighbor_idx += 1;

            if st.index[w].is_none() {
                // w has not been visited — "recurse" by pushing a new frame
                st.index[w] = Some(st.index_counter);
                st.lowlink[w] = st.index_counter;
                st.index_counter += 1;
                st.scc_stack.push(w);
                st.on_stack[w] = true;
                call_stack.push((w, 0));
            } else if st.on_stack[w] {
                // w is on the current SCC stack: back edge within the current SCC
                st.lowlink[node] = st.lowlink[node].min(st.index[w].unwrap());
            }
            // If w is off the stack (already in a completed SCC), ignore.
        } else {
            // All neighbors of `node` have been processed — equivalent to returning
            // from the recursive call. Pop this frame and propagate lowlink to parent.
            let (finished_node, _) = call_stack.pop().unwrap();

            if let Some(&(parent, _)) = call_stack.last() {
                st.lowlink[parent] = st.lowlink[parent].min(st.lowlink[finished_node]);
            }

            // If finished_node is a root (lowlink == index), pop the completed SCC
            if st.lowlink[finished_node] == st.index[finished_node].unwrap() {
                let mut scc = Vec::new();
                loop {
                    let w = st.scc_stack.pop().unwrap();
                    st.on_stack[w] = false;
                    scc.push(w);
                    if w == finished_node {
                        break;
                    }
                }
                st.sccs.push(scc);
            }
        }
    }
}

/// Reconstruct a representative cycle path string for a non-trivial SCC.
///
/// For single-node SCCs with a self-edge returns "X -> X".
/// For larger SCCs, performs a DFS within the SCC nodes to find a path from the first
/// member back to itself, then formats it as "A -> B -> ... -> A".
fn reconstruct_scc_cycle(
    scc: &[usize],
    adjacency: &[Vec<usize>],
    templates: &[TopologyTemplate],
) -> String {
    if scc.len() == 1 {
        let v = scc[0];
        return format!("{} -> {}", templates[v].name, templates[v].name);
    }

    // Build a set of SCC members for fast membership test
    let scc_set: HashSet<usize> = scc.iter().copied().collect();
    let start = scc[0];

    if let Some(cycle) = find_cycle_back_to(start, &scc_set, adjacency) {
        cycle
            .iter()
            .map(|&i| templates[i].name.as_str())
            .collect::<Vec<_>>()
            .join(" -> ")
    } else {
        // Invariant: find_cycle_back_to should always succeed for a valid SCC.
        // If it fails, the SCC adjacency is broken — noisy in debug builds,
        // graceful degradation in release builds (lossy but safe fallback).
        debug_assert!(
            false,
            "find_cycle_back_to returned None for valid SCC — Tarjan algorithm invariant violated"
        );
        let mut names: Vec<&str> = scc.iter().map(|&i| templates[i].name.as_str()).collect();
        names.push(templates[scc[0]].name.as_str());
        names.join(" -> ")
    }
}

/// Iterative DFS within SCC nodes to find a cycle from `start` back to itself.
/// Returns the full cycle path (including the closing `start` node) on success.
///
/// Uses an explicit stack to avoid OS stack overflow for large SCCs.
fn find_cycle_back_to(
    start: usize,
    scc_set: &HashSet<usize>,
    adjacency: &[Vec<usize>],
) -> Option<Vec<usize>> {
    let mut path = vec![start];
    let mut visited = HashSet::new();
    visited.insert(start);
    // Each frame: index into adjacency[path.last()] for the next neighbor to try
    let mut neighbor_idx_stack: Vec<usize> = vec![0];

    while let Some(ni) = neighbor_idx_stack.last_mut() {
        let current = *path.last().unwrap();
        if *ni >= adjacency[current].len() {
            // Backtrack: all neighbors of `current` exhausted
            path.pop();
            neighbor_idx_stack.pop();
            if let Some(&backtracked) = path.last() {
                // Only remove from visited when we're not the start node
                // (we keep start in visited to avoid revisiting it as non-target)
                let _ = backtracked; // backtracked node stays — current gets removed
                visited.remove(&current);
            }
            continue;
        }
        let next = adjacency[current][*ni];
        *ni += 1;

        if !scc_set.contains(&next) {
            continue; // Stay within the SCC
        }
        if next == start && path.len() > 1 {
            // Completed the cycle back to the start
            path.push(start);
            return Some(path);
        }
        if !visited.contains(&next) {
            visited.insert(next);
            path.push(next);
            neighbor_idx_stack.push(0);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EntityKind, GuardState, SubComponentDecl, Visibility};
    use reify_core::{ContentHash, SourceSpan};
    use std::collections::HashMap;

    /// Helper: build a minimal SubComponentDecl referencing `target`.
    fn sub_ref(name: &str, target: &str) -> SubComponentDecl {
        SubComponentDecl {
            name: name.to_string(),
            structure_name: target.to_string(),
            visibility: Visibility::Public,
            args: vec![],
            type_args: vec![],
            is_collection: false,
            count_cell: None,
            guard_state: GuardState::None,
            pose: None,
            is_aux: false,
            span: SourceSpan::new(0, 0),
            content_hash: ContentHash(0),
        }
    }

    /// Helper: build a minimal TopologyTemplate with just a name.
    fn minimal_template(name: &str) -> TopologyTemplate {
        TopologyTemplate {
            name: name.to_string(),
            doc: None,
            entity_kind: EntityKind::Structure,
            visibility: Visibility::Public,
            type_params: vec![],
            trait_bounds: vec![],
            value_cells: vec![],
            constraints: vec![],
            realizations: vec![],
            sub_components: vec![],
            ports: vec![],
            connections: vec![],
            guarded_groups: vec![],
            structure_controlling: HashSet::new(),
            objective: None,
            meta: HashMap::new(),
            content_hash: ContentHash(0),
            is_recursive: false,
            annotations: vec![],
            pragmas: vec![],
            match_arm_groups: vec![],
            forall_templates: vec![],
            assoc_fns: vec![],
            assoc_types: vec![],
        }
    }

    #[test]
    fn duplicate_sub_refs_deduped() {
        // Template S has two sub_components both referencing itself — a self-loop via
        // two distinct sub names. After dedup the adjacency should have one edge.
        // The result: is_recursive==true and exactly 1 cycle warning.
        let mut s = minimal_template("S");
        s.sub_components = vec![sub_ref("x", "S"), sub_ref("y", "S")];
        let mut templates = vec![s];
        let mut diagnostics = Vec::new();
        detect_recursive_structures(&mut templates, &mut diagnostics);

        assert!(
            templates[0].is_recursive,
            "S with two self-ref subs should be recursive"
        );

        let cycle_warnings: Vec<_> = diagnostics
            .iter()
            .filter(|d| {
                d.severity == reify_core::Severity::Warning
                    && d.message.contains("recursive structure cycle")
            })
            .collect();
        assert_eq!(
            cycle_warnings.len(),
            1,
            "expected exactly 1 cycle warning even with two self-referencing subs, got: {:?}",
            cycle_warnings
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn duplicate_template_names_emits_diagnostic() {
        // Two templates with the same name — detect_recursive_structures should emit
        // a diagnostic mentioning "duplicate" rather than silently overwriting the first.
        let mut templates = vec![minimal_template("A"), minimal_template("A")];
        let mut diagnostics = Vec::new();
        detect_recursive_structures(&mut templates, &mut diagnostics);
        let duplicate_diags: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.message.to_lowercase().contains("duplicate"))
            .collect();
        assert!(
            !duplicate_diags.is_empty(),
            "expected a diagnostic mentioning 'duplicate' when two templates share a name, got: {:?}",
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "Tarjan algorithm invariant violated")]
    fn reconstruct_scc_cycle_panics_on_invalid_scc() {
        let templates = vec![minimal_template("A"), minimal_template("B")];
        // Node 0 -> 1, but node 1 has no edges — find_cycle_back_to(0, {0,1}, adj)
        // cannot find a cycle back to 0, so it returns None.
        let adjacency = vec![vec![1], vec![]];
        // This should hit the debug_assert in the else branch
        reconstruct_scc_cycle(&[0, 1], &adjacency, &templates);
    }
}
