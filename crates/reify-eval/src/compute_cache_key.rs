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
pub fn compute_cache_key(_node: &ComputeNodeData, _ctx: &EvaluationGraph) -> ContentHash {
    ContentHash(0)
}

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
