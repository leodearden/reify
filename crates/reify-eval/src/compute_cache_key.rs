//! Cache key composition for `ComputeNode` inputs.
//!
//! Exposes [`compute_cache_key`], which composes a canonical deterministic
//! cache key over a [`ComputeNodeData`]'s inputs per
//! `docs/prds/v0_3/compute-node-infrastructure.md` §"Cache key".

use crate::graph::{ComputeNodeData, EvaluationGraph};
use reify_types::ContentHash;

/// Compose a deterministic cache key over a `ComputeNode`'s inputs.
///
/// # Composition order
///
/// The returned `ContentHash` is `combine_all([target_hash, value_bucket_hash,
/// realization_bucket_hash, options_hash])`, where:
///
/// - **`target_hash`** — `ContentHash::of_str(&node.target)`, the solver/target
///   string that identifies the computation kind (e.g. `"solver::elastic_static"`).
///
/// - **`value_bucket_hash`** — `combine_all` over the `content_hash` of each
///   `ValueCellNode` referenced by `node.value_inputs`.  Fetched from
///   `ctx.value_cells`.  A `ValueCellNode::content_hash` encodes the cell's
///   identity and default-expr content, so parameter edits automatically
///   propagate through.
///
/// - **`realization_bucket_hash`** — `combine_all` over the `content_hash` of
///   each `RealizationNodeData` referenced by `node.realization_inputs`.
///   Fetched from `ctx.realizations`.
///
/// - **`options_hash`** — `node.options_hash` verbatim (opaque; see Exclusion
///   contract below).
///
/// Domain separation: the 4 outer positions prevent aliasing — a value-cell
/// hash equal-by-collision to a realization hash cannot produce the same key
/// because they enter the outer `combine_all` at different positions.
/// `ContentHash::combine` is order-dependent (hash.rs:32-37).
///
/// # Canonical sort keys
///
/// Both input buckets are sorted before combining to make the key invariant
/// under `Vec` insertion-order variation:
///
/// - **`value_inputs`** sorted by `ValueCellId`'s derived `Ord`
///   (lexicographic `(entity, member)`; identity.rs:119).
///
/// - **`realization_inputs`** sorted by `(entity.as_str(), index)` tuple.
///   `RealizationNodeId` intentionally does not derive `Ord` upstream, so the
///   sort is performed locally using the same field-order comparison that a
///   derived `Ord` would produce.
///
/// # Exclusion contract
///
/// Thread count, determinism mode, and any future "execution profile" flags
/// MUST be filtered out by the **upstream `options_hash` producer** before
/// they reach this function.  `compute_cache_key` treats `options_hash` as
/// a fully opaque `ContentHash` and passes it through unchanged.
///
/// The canonical producer for FEA nodes is `ElasticOptions::cacheable_hash`
/// (to be implemented in P3.4 / `docs/prds/v0_3/structural-analysis-fea.md`
/// task #4).  Any struct intended to feed a `ComputeNode.options_hash` must
/// implement an equivalent "cacheable hash" that omits non-cacheable fields.
///
/// The behavioural contract is verified indirectly by
/// `compute_cache_key_changes_when_options_hash_changes`: if two solver
/// invocations produce the same `options_hash` (e.g. because threads was
/// filtered upstream), the composer produces the same cache key — it cannot
/// smuggle in any non-opaque fields.  The filtering itself belongs in the
/// upstream producer's tests (e.g. `ElasticOptions::cacheable_hash`).
///
/// # Missing-input and duplicate-input policy
///
/// If a `ValueCellId` or `RealizationNodeId` in `node.value_inputs` /
/// `node.realization_inputs` is not present in `ctx`, this function **panics**
/// via `.unwrap_or_else(|| panic!(...))`.  Missing references are a producer
/// bug — `ComputeNodeData` is constructed by the upstream lowering pass (P3.4)
/// and is expected to reference live graph nodes.  Silently substituting a
/// sentinel hash would mask such bugs and could create collisions.
///
/// **Duplicate inputs** (the same id appearing more than once in either vec)
/// are likewise a producer bug.  A `debug_assert!` catches duplicates in debug
/// builds.  In release builds the cache key is still deterministic for a given
/// duplicated shape, but it differs from the deduplicated version — so
/// duplicates cause spurious cache misses without producing incorrect results.
///
/// # PRD references
///
/// - `docs/prds/v0_3/compute-node-infrastructure.md` §"Cache key" — primary spec.
/// - `docs/prds/v0_3/compute-node-infrastructure.md` §"Resolved design decisions"
///   — exclusion contract rationale.
/// - `docs/prds/v0_3/structural-analysis-fea.md` task #4 — upstream producer.
pub fn compute_cache_key(node: &ComputeNodeData, ctx: &EvaluationGraph) -> ContentHash {
    // Collect value-input cell content_hashes sorted by ValueCellId (derived Ord
    // on (entity, member) lexicographic order) so the bucket is invariant under
    // insertion-order variation in node.value_inputs.
    // Sort references rather than owned values to avoid allocating Strings per input.
    let value_bucket_hash: ContentHash = {
        let mut sorted_refs: Vec<&reify_types::ValueCellId> = node.value_inputs.iter().collect();
        sorted_refs.sort(); // ValueCellId derives Ord via (entity, member)
        debug_assert!(
            sorted_refs.windows(2).all(|w| w[0] != w[1]),
            "compute_cache_key: value_inputs contains duplicate ValueCellId — producer bug"
        );
        let hashes: Vec<ContentHash> = sorted_refs
            .into_iter()
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

    // Collect realization_input content_hashes sorted by (entity, index) tuple.
    // RealizationNodeId does not derive Ord upstream (intentionally, per design
    // decision in plan.json), so we sort locally by the same (entity, index)
    // lexicographic ordering that a derived Ord would produce.
    // Sort references rather than owned values to avoid allocating Strings per input.
    let realization_bucket_hash: ContentHash = {
        let mut sorted_refs: Vec<&reify_types::RealizationNodeId> =
            node.realization_inputs.iter().collect();
        sorted_refs.sort_by(|a, b| (a.entity.as_str(), a.index).cmp(&(b.entity.as_str(), b.index)));
        debug_assert!(
            sorted_refs.windows(2).all(|w| {
                (w[0].entity.as_str(), w[0].index) != (w[1].entity.as_str(), w[1].index)
            }),
            "compute_cache_key: realization_inputs contains duplicate RealizationNodeId — producer bug"
        );
        let hashes: Vec<ContentHash> = sorted_refs
            .into_iter()
            .map(|id| {
                ctx.realizations
                    .get(id)
                    .unwrap_or_else(|| {
                        panic!(
                            "compute_cache_key [task-3381]: realization_input {:?} not present in graph",
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
        realization_bucket_hash,
        node.options_hash,
    ])
}

#[cfg(test)]
mod tests {
    use super::compute_cache_key;

    use reify_compiler::ValueCellKind;
    use reify_types::{ComputeNodeId, ContentHash, RealizationNodeId, Type, ValueCellId};

    use crate::graph::{ComputeNodeData, EvaluationGraph, RealizationNodeData, ValueCellNode};

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

    /// Insert a bare RealizationNodeData with the given content_hash.
    fn insert_realization(
        graph: &mut EvaluationGraph,
        id: RealizationNodeId,
        content_hash: ContentHash,
    ) {
        graph.realizations.insert(
            id.clone(),
            RealizationNodeData {
                id,
                operations: vec![],
                content_hash,
            },
        );
    }

    /// Insert a bare ValueCellNode (no default_expr) with the given content_hash.
    fn insert_value_cell(graph: &mut EvaluationGraph, id: ValueCellId, content_hash: ContentHash) {
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
        graph.value_cells.get_mut(&load_id).unwrap().content_hash = ContentHash::of_str("load_v2");

        let key_v2 = compute_cache_key(&node, &graph);
        assert_ne!(
            key_v1, key_v2,
            "mutating a value-cell's content_hash must change the cache key"
        );
    }

    #[test]
    fn compute_cache_key_is_invariant_under_realization_input_reordering() {
        // Three realizations with distinct (entity, index) pairs.
        let real_0 = RealizationNodeId::new("Bracket", 0);
        let real_1 = RealizationNodeId::new("Bracket", 1);
        let real_stud = RealizationNodeId::new("Stud", 0);

        let mut graph = EvaluationGraph::default();
        insert_realization(&mut graph, real_0.clone(), ContentHash::of_str("r0"));
        insert_realization(&mut graph, real_1.clone(), ContentHash::of_str("r1"));
        insert_realization(&mut graph, real_stud.clone(), ContentHash::of_str("rs"));

        let mut node_a = make_empty_node();
        node_a.realization_inputs = vec![real_0.clone(), real_1.clone(), real_stud.clone()];

        let mut node_b = make_empty_node();
        node_b.realization_inputs = vec![real_stud.clone(), real_0.clone(), real_1.clone()];

        let key_a = compute_cache_key(&node_a, &graph);
        let key_b = compute_cache_key(&node_b, &graph);
        assert_eq!(
            key_a, key_b,
            "cache key must be invariant under realization_input ordering"
        );
    }

    #[test]
    fn compute_cache_key_changes_when_realization_input_content_hash_changes() {
        let real_id = RealizationNodeId::new("Bracket", 0);
        let mut graph = EvaluationGraph::default();
        insert_realization(&mut graph, real_id.clone(), ContentHash::of_str("mesh_v1"));

        let mut node = make_empty_node();
        node.realization_inputs = vec![real_id.clone()];

        let key_v1 = compute_cache_key(&node, &graph);

        graph.realizations.get_mut(&real_id).unwrap().content_hash = ContentHash::of_str("mesh_v2");

        let key_v2 = compute_cache_key(&node, &graph);
        assert_ne!(
            key_v1, key_v2,
            "mutating a realization's content_hash must change the cache key"
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
