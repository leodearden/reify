//! Cache key composition for `ComputeNode` inputs.
//!
//! Exposes [`compute_cache_key`], which composes a canonical deterministic
//! cache key over a [`ComputeNodeData`]'s inputs per
//! `docs/prds/v0_3/compute-node-infrastructure.md` §"Cache key".

use crate::graph::{ComputeNodeData, EvaluationGraph};
use reify_core::ContentHash;

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
        let mut sorted_refs: Vec<&reify_core::ValueCellId> = node.value_inputs.iter().collect();
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
        let mut sorted_refs: Vec<&reify_core::RealizationNodeId> =
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
    use reify_core::{ComputeNodeId, ContentHash, RealizationNodeId, Type, ValueCellId};
    use reify_ir::ReprKind;

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
            RealizationNodeData { geometry_cell: None,
                id,
                operations: vec![],
                content_hash,
                produced_repr: ReprKind::BRep,
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

    #[test]
    fn compute_cache_key_domain_separates_value_input_from_realization_input() {
        // Same ContentHash on both a value cell and a realization — only the
        // outer-position slot differs (value_bucket at pos 1, realization_bucket
        // at pos 2 of combine_all([target, val_bucket, real_bucket, opts])).
        // ContentHash::combine is order-dependent, so these must differ even when
        // the inner bucket hash is identical.  Pins the outer combine_all call at
        // the bottom of compute_cache_key against a future flat-XOR refactor that
        // would collapse domain separation.
        let shared_h = ContentHash::of_str("shared_H");
        let val_id = ValueCellId::new("Bracket", "x");
        let real_id = RealizationNodeId::new("Bracket", 0);

        let mut graph = EvaluationGraph::default();
        insert_value_cell(&mut graph, val_id.clone(), shared_h);
        insert_realization(&mut graph, real_id.clone(), shared_h);

        let mut node_value = make_empty_node();
        node_value.value_inputs = vec![val_id];

        let mut node_real = make_empty_node();
        node_real.realization_inputs = vec![real_id];

        let key_value = compute_cache_key(&node_value, &graph);
        let key_real = compute_cache_key(&node_real, &graph);
        assert_ne!(
            key_value, key_real,
            "domain separation: a value-input hash and a realization-input hash with the \
             same ContentHash must not produce the same cache key — they occupy different \
             positions in the outer combine_all in compute_cache_key"
        );
    }

    #[test]
    #[should_panic(expected = "value_input")]
    fn compute_cache_key_panics_on_missing_value_input() {
        // Empty graph — the ValueCellId "ghost" was never inserted.
        // The .unwrap_or_else(|| panic!(...)) in the value_bucket_hash block fires
        // with a message containing "value_input".  Pins the producer-bug panic
        // policy (documented in compute_cache_key's docstring) against a future
        // silent-fallback refactor (e.g. ContentHash(0)).
        let graph = EvaluationGraph::default();
        let mut node = make_empty_node();
        node.value_inputs = vec![ValueCellId::new("Bracket", "ghost")];
        compute_cache_key(&node, &graph);
    }

    #[test]
    #[should_panic(expected = "realization_input")]
    fn compute_cache_key_panics_on_missing_realization_input() {
        // Empty graph — RealizationNodeId("Bracket", 0) was never inserted.
        // The .unwrap_or_else(|| panic!(...)) in the realization_bucket_hash block
        // fires with a message containing "realization_input".  "realization_input"
        // is disjoint from "value_input", so this test pins exactly the
        // realization-side panic arm.
        let graph = EvaluationGraph::default();
        let mut node = make_empty_node();
        node.realization_inputs = vec![RealizationNodeId::new("Bracket", 0)];
        compute_cache_key(&node, &graph);
    }

    #[test]
    fn compute_cache_key_changes_when_value_input_cardinality_changes() {
        // Compare node_one ([a]) vs node_two ([a, b]).  The only varying factor is
        // bucket cardinality — both reference cell `a` and node_two also includes `b`.
        // Adding a value input must change the cache key produced by compute_cache_key.
        let a = ValueCellId::new("Bracket", "a");
        let b = ValueCellId::new("Bracket", "b");

        let mut graph = EvaluationGraph::default();
        insert_value_cell(&mut graph, a.clone(), ContentHash::of_str("hash_a"));
        insert_value_cell(&mut graph, b.clone(), ContentHash::of_str("hash_b"));

        let mut node_one = make_empty_node();
        node_one.value_inputs = vec![a.clone()];

        let mut node_two = make_empty_node();
        node_two.value_inputs = vec![a.clone(), b.clone()];

        let key_one = compute_cache_key(&node_one, &graph);
        let key_two = compute_cache_key(&node_two, &graph);
        assert_ne!(
            key_one, key_two,
            "adding a value input must change the cache key: [a] and [a, b] must produce \
             distinct keys"
        );
    }

    #[test]
    fn compute_cache_key_changes_when_realization_input_cardinality_changes() {
        // Mirror of the value-bucket cardinality test for the realization bucket.
        // Compare node_one ([real_0]) vs node_two ([real_0, real_1]).  Adding a
        // realization input must change the cache key produced by compute_cache_key.
        let real_0 = RealizationNodeId::new("Bracket", 0);
        let real_1 = RealizationNodeId::new("Bracket", 1);

        let mut graph = EvaluationGraph::default();
        insert_realization(&mut graph, real_0.clone(), ContentHash::of_str("mesh_a"));
        insert_realization(&mut graph, real_1.clone(), ContentHash::of_str("mesh_b"));

        let mut node_one = make_empty_node();
        node_one.realization_inputs = vec![real_0.clone()];

        let mut node_two = make_empty_node();
        node_two.realization_inputs = vec![real_0.clone(), real_1.clone()];

        let key_one = compute_cache_key(&node_one, &graph);
        let key_two = compute_cache_key(&node_two, &graph);
        assert_ne!(
            key_one, key_two,
            "adding a realization input must change the cache key: [real_0] and \
             [real_0, real_1] must produce distinct keys"
        );
    }
}
