//! Cache key composition for `ComputeNode` inputs.

#[cfg(test)]
mod tests {
    use super::compute_cache_key;

    use crate::graph::{ComputeNodeData, EvaluationGraph};
    use reify_types::{ComputeNodeId, ContentHash};

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
}
