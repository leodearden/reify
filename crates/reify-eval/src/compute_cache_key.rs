//! Cache key composition for `ComputeNode` inputs.
//!
//! Exposes [`compute_cache_key`], which composes a canonical deterministic
//! cache key over a [`ComputeNodeData`]'s inputs per
//! `docs/prds/v0_3/compute-node-infrastructure.md` §"Cache key".

use crate::graph::{ComputeNodeData, EvaluationGraph};
use reify_types::ContentHash;

/// Compose a deterministic cache key over a `ComputeNode`'s inputs.
///
/// See `docs/prds/v0_3/compute-node-infrastructure.md` §"Cache key" for the
/// full specification.  Composition is finalised in P3.2 task steps 2–16.
pub fn compute_cache_key(node: &ComputeNodeData, ctx: &EvaluationGraph) -> ContentHash {
    // Collect value-input cell content_hashes (Vec order — sort applied in step-10).
    let value_bucket_hash: ContentHash = {
        let hashes: Vec<ContentHash> = node
            .value_inputs
            .iter()
            .map(|id| {
                ctx.value_cells
                    .get(id)
                    .unwrap_or_else(|| {
                        panic!(
                            "compute_cache_key [task-3381]: value_input {:?} not present in graph",
                            id
                        )
                    })
                    .content_hash
            })
            .collect();
        ContentHash::combine_all(hashes)
    };

    ContentHash::combine_all([
        ContentHash::of_str(&node.target),
        value_bucket_hash,
        node.options_hash,
    ])
}

#[cfg(test)]
mod tests {
    use super::compute_cache_key;

    use reify_compiler::ValueCellKind;
    use reify_types::{ComputeNodeId, ContentHash, Type, ValueCellId};

    use crate::graph::{ComputeNodeData, EvaluationGraph, ValueCellNode};

    fn make_empty_node() -> ComputeNodeData {
        ComputeNodeData {
            computation_id: ComputeNodeId::new("Test", 0),
            target: "solver::test".to_string(),
            value_inputs: vec![],
            realization_inputs: vec![],
            options_hash: ContentHash(0),
            cache_key: ContentHash(0),
            cached_result: None,
            result_content_hash: None,
            opaque_state: None,
            running: None,
            output_value_cells: vec![],
        }
    }

    #[test]
    fn compute_cache_key_is_deterministic_for_empty_inputs() {
        let node = make_empty_node();
        let graph = EvaluationGraph::default();
        let key1 = compute_cache_key(&node, &graph);
        let key2 = compute_cache_key(&node, &graph);
        assert_eq!(key1, key2, "compute_cache_key must be deterministic");
    }

    /// Insert a bare ValueCellNode (no default_expr) with the given content_hash.
    fn insert_value_cell(
        graph: &mut EvaluationGraph,
        id: ValueCellId,
        content_hash: ContentHash,
    ) {
        graph.value_cells.insert(
            id.clone(),
            ValueCellNode {
                id,
                kind: ValueCellKind::Let,
                cell_type: Type::Real,
                default_expr: None,
                content_hash,
            },
        );
    }

    #[test]
    fn compute_cache_key_changes_when_value_input_cell_hash_changes() {
        let load_id = ValueCellId::new("Bracket", "load");
        let mut graph = EvaluationGraph::default();
        insert_value_cell(&mut graph, load_id.clone(), ContentHash::of_str("load_v1"));

        let mut node = make_empty_node();
        node.value_inputs = vec![load_id.clone()];

        let key_v1 = compute_cache_key(&node, &graph);

        // Mutate the cell's content_hash in-place (P3.1 landed get_mut for this).
        graph
            .value_cells
            .get_mut(&load_id)
            .unwrap()
            .content_hash = ContentHash::of_str("load_v2");

        let key_v2 = compute_cache_key(&node, &graph);
        assert_ne!(
            key_v1, key_v2,
            "mutating a value-cell's content_hash must change the cache key"
        );
    }

    #[test]
    fn compute_cache_key_is_invariant_under_value_input_reordering() {
        let id_a = ValueCellId::new("Bracket", "a");
        let id_b = ValueCellId::new("Bracket", "b");
        let id_c = ValueCellId::new("Bracket", "c");

        let mut graph = EvaluationGraph::default();
        insert_value_cell(&mut graph, id_a.clone(), ContentHash::of_str("hash_a"));
        insert_value_cell(&mut graph, id_b.clone(), ContentHash::of_str("hash_b"));
        insert_value_cell(&mut graph, id_c.clone(), ContentHash::of_str("hash_c"));

        let mut node_abc = make_empty_node();
        node_abc.value_inputs = vec![id_a.clone(), id_b.clone(), id_c.clone()];

        let mut node_cab = make_empty_node();
        node_cab.value_inputs = vec![id_c.clone(), id_a.clone(), id_b.clone()];

        let key_abc = compute_cache_key(&node_abc, &graph);
        let key_cab = compute_cache_key(&node_cab, &graph);
        assert_eq!(
            key_abc, key_cab,
            "cache key must be invariant under value_input ordering"
        );
    }

    #[test]
    fn compute_cache_key_changes_when_options_hash_changes() {
        let mut node_a = make_empty_node();
        node_a.options_hash = ContentHash::of_str("opts_a");

        let mut node_b = make_empty_node();
        node_b.options_hash = ContentHash::of_str("opts_b");

        let graph = EvaluationGraph::default();
        let key_a = compute_cache_key(&node_a, &graph);
        let key_b = compute_cache_key(&node_b, &graph);
        assert_ne!(
            key_a, key_b,
            "distinct options_hash values must produce distinct cache keys"
        );
    }

    #[test]
    fn compute_cache_key_changes_when_target_changes() {
        let mut node_a = make_empty_node();
        node_a.target = "solver::elastic_static".to_string();

        let mut node_b = make_empty_node();
        node_b.target = "solver::modal".to_string();

        let graph = EvaluationGraph::default();
        let key_a = compute_cache_key(&node_a, &graph);
        let key_b = compute_cache_key(&node_b, &graph);
        assert_ne!(
            key_a, key_b,
            "distinct target strings must produce distinct cache keys"
        );
    }
}
