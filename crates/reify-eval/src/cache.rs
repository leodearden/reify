use std::collections::HashMap;

use reify_types::{
    ConstraintNodeId, ContentHash, DeterminacyState, Freshness, GeometryHandleId,
    RealizationNodeId, Satisfaction, Value, ValueCellId, VersionId,
};

use crate::deps::DependencyTrace;

/// Unified identifier for any node in the evaluation graph.
/// Used as the key in the cache store.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeId {
    Value(ValueCellId),
    Constraint(ConstraintNodeId),
    Realization(RealizationNodeId),
}

impl From<ValueCellId> for NodeId {
    fn from(id: ValueCellId) -> Self {
        NodeId::Value(id)
    }
}

impl From<ConstraintNodeId> for NodeId {
    fn from(id: ConstraintNodeId) -> Self {
        NodeId::Constraint(id)
    }
}

impl From<RealizationNodeId> for NodeId {
    fn from(id: RealizationNodeId) -> Self {
        NodeId::Realization(id)
    }
}

/// Stores different kinds of evaluation results in the cache.
#[derive(Clone, Debug)]
pub enum CachedResult {
    /// A value cell result with its determinacy state.
    Value(Value, DeterminacyState),
    /// A constraint satisfaction result.
    Satisfaction(Satisfaction),
    /// A geometry handle result (proxy for the actual shape).
    GeometryHandle(GeometryHandleId),
}

impl CachedResult {
    /// Compute a content hash for early cutoff comparison.
    /// Domain-separated with tag bytes [20], [21], [22] per variant.
    pub fn content_hash(&self) -> ContentHash {
        match self {
            CachedResult::Value(val, det) => {
                let tag = ContentHash::of(&[20]);
                let val_hash = val.content_hash();
                let det_hash = ContentHash::of(&[*det as u8]);
                tag.combine(val_hash).combine(det_hash)
            }
            CachedResult::Satisfaction(sat) => {
                let tag = ContentHash::of(&[21]);
                tag.combine(sat.content_hash())
            }
            CachedResult::GeometryHandle(handle_id) => {
                let tag = ContentHash::of(&[22]);
                tag.combine(handle_id.content_hash())
            }
        }
    }
}

/// Signal indicating whether a node's result changed after re-evaluation.
/// Used to control dirty propagation (early cutoff).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalOutcome {
    /// The result changed compared to the previous cached value.
    Changed,
    /// The result is the same as the previous cached value (early cutoff).
    Unchanged,
}

/// Per-node cache entry storing the evaluation result and metadata.
#[derive(Clone, Debug)]
pub struct NodeCache {
    /// The cached evaluation result.
    pub result: CachedResult,
    /// Content hash of the result, for early cutoff comparison.
    pub result_hash: ContentHash,
    /// Freshness of the cached value.
    pub freshness: Freshness,
    /// Which value cells were read during evaluation of this node.
    pub dependency_trace: DependencyTrace,
    /// The version at which this cache entry was last validated.
    pub basis_version: VersionId,
}

impl NodeCache {
    /// Create a new cache entry, automatically computing the result hash.
    pub fn new(
        result: CachedResult,
        freshness: Freshness,
        dependency_trace: DependencyTrace,
        basis_version: VersionId,
    ) -> Self {
        let result_hash = result.content_hash();
        Self {
            result,
            result_hash,
            freshness,
            dependency_trace,
            basis_version,
        }
    }
}

/// Store managing per-node cache entries for incremental evaluation.
pub struct CacheStore {
    caches: HashMap<NodeId, NodeCache>,
}

impl CacheStore {
    /// Create an empty cache store.
    pub fn new() -> Self {
        Self {
            caches: HashMap::new(),
        }
    }

    /// Look up a cached entry by node id.
    pub fn get(&self, node: &NodeId) -> Option<&NodeCache> {
        self.caches.get(node)
    }

    /// Store or overwrite a cache entry.
    pub fn put(&mut self, node: NodeId, cache: NodeCache) {
        self.caches.insert(node, cache);
    }

    /// Remove a cached entry.
    pub fn invalidate(&mut self, node: &NodeId) {
        self.caches.remove(node);
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.caches.len()
    }

    /// Whether the store has no entries.
    pub fn is_empty(&self) -> bool {
        self.caches.is_empty()
    }

    /// Remove all cached entries.
    pub fn clear(&mut self) {
        self.caches.clear();
    }

    /// Record an evaluation result and determine if it changed (early cutoff).
    ///
    /// Compares the new result's content hash with the existing cache entry.
    /// - If same hash: updates basis_version only (early cutoff), returns Unchanged.
    /// - If different or no prior cache: updates full cache entry, returns Changed.
    pub fn record_evaluation(
        &mut self,
        node: NodeId,
        new_result: CachedResult,
        version: VersionId,
        trace: DependencyTrace,
    ) -> EvalOutcome {
        let new_hash = new_result.content_hash();

        if let Some(existing) = self.caches.get_mut(&node) {
            if existing.result_hash == new_hash {
                // Early cutoff: result unchanged, just update version
                existing.basis_version = version;
                existing.dependency_trace = trace;
                existing.freshness = Freshness::Final;
                return EvalOutcome::Unchanged;
            }
        }

        // Changed or cold start: store full entry
        self.caches.insert(
            node,
            NodeCache {
                result: new_result,
                result_hash: new_hash,
                freshness: Freshness::Final,
                dependency_trace: trace,
                basis_version: version,
            },
        );
        EvalOutcome::Changed
    }

    /// Version fast path: if the node is cached and its basis_version matches
    /// the current version, return a clone of the cached result without
    /// re-evaluation.
    pub fn try_fast_path(&self, node: &NodeId, current_version: VersionId) -> Option<CachedResult> {
        let entry = self.caches.get(node)?;
        if entry.basis_version == current_version {
            Some(entry.result.clone())
        } else {
            None
        }
    }
}

impl Default for CacheStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{ConstraintNodeId, RealizationNodeId, ValueCellId};

    #[test]
    fn node_id_from_value_cell_id() {
        let vcid = ValueCellId::new("Bracket", "width");
        let node: NodeId = NodeId::from(vcid.clone());
        assert_eq!(node, NodeId::Value(vcid));
    }

    #[test]
    fn node_id_from_constraint_node_id() {
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let node: NodeId = NodeId::from(cnid.clone());
        assert_eq!(node, NodeId::Constraint(cnid));
    }

    #[test]
    fn node_id_from_realization_node_id() {
        let rnid = RealizationNodeId::new("Bracket", 0);
        let node: NodeId = NodeId::from(rnid.clone());
        assert_eq!(node, NodeId::Realization(rnid));
    }

    #[test]
    fn node_id_variants_not_equal_even_with_overlapping_strings() {
        let vcid = ValueCellId::new("Bracket", "width");
        let cnid = ConstraintNodeId::new("Bracket", 0);
        let rnid = RealizationNodeId::new("Bracket", 0);

        let v = NodeId::Value(vcid);
        let c = NodeId::Constraint(cnid);
        let r = NodeId::Realization(rnid);

        assert_ne!(v, c);
        assert_ne!(v, r);
        assert_ne!(c, r);
    }

    #[test]
    fn node_id_clone_and_debug() {
        let vcid = ValueCellId::new("Bracket", "width");
        let node = NodeId::Value(vcid);
        let cloned = node.clone();
        assert_eq!(node, cloned);

        let debug = format!("{:?}", node);
        assert!(debug.contains("Value"));
    }

    #[test]
    fn node_id_hash_as_map_key() {
        use std::collections::HashMap;
        let vcid = ValueCellId::new("Bracket", "width");
        let cnid = ConstraintNodeId::new("Bracket", 0);

        let mut map = HashMap::new();
        map.insert(NodeId::Value(vcid.clone()), "value");
        map.insert(NodeId::Constraint(cnid.clone()), "constraint");

        assert_eq!(map.get(&NodeId::Value(vcid)), Some(&"value"));
        assert_eq!(map.get(&NodeId::Constraint(cnid)), Some(&"constraint"));
    }

    // --- invalidate_dependents tests ---

    #[test]
    fn invalidate_dependents_removes_dependent_nodes() {
        let mut store = CacheStore::new();
        let a = ValueCellId::new("Bracket", "a");
        let b = ValueCellId::new("Bracket", "b");
        let x_id = ValueCellId::new("Bracket", "x");
        let y_id = ValueCellId::new("Bracket", "y");

        // x depends on a
        let node_x = NodeId::Value(x_id.clone());
        let mut trace_x = DependencyTrace::default();
        trace_x.reads.push(a.clone());
        store.put(
            node_x.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
                Freshness::Final,
                trace_x,
                VersionId(1),
            ),
        );

        // y depends on b (not a)
        let node_y = NodeId::Value(y_id.clone());
        let mut trace_y = DependencyTrace::default();
        trace_y.reads.push(b.clone());
        store.put(
            node_y.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Int(2), DeterminacyState::Determined),
                Freshness::Final,
                trace_y,
                VersionId(1),
            ),
        );

        // Invalidate dependents of a
        store.invalidate_dependents(&[a]);

        // x should be invalidated (depends on a)
        assert!(store.get(&node_x).is_none());
        // y should be retained (depends on b, not a)
        assert!(store.get(&node_y).is_some());
    }

    // --- record_evaluation tests ---

    #[test]
    fn record_evaluation_early_cutoff_same_hash() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "x"));

        // First evaluation: cold start
        let result1 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let outcome1 = store.record_evaluation(
            node.clone(),
            result1,
            VersionId(1),
            DependencyTrace::default(),
        );
        assert_eq!(outcome1, EvalOutcome::Changed);

        // Second evaluation: same result, new version -> early cutoff
        let result2 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let outcome2 = store.record_evaluation(
            node.clone(),
            result2,
            VersionId(2),
            DependencyTrace::default(),
        );
        assert_eq!(outcome2, EvalOutcome::Unchanged);

        // basis_version should be updated even though result unchanged
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(2));
    }

    #[test]
    fn record_evaluation_changed_when_different_hash() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "x"));

        // First evaluation
        let result1 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        store.record_evaluation(node.clone(), result1, VersionId(1), DependencyTrace::default());

        // Second evaluation: different result
        let result2 = CachedResult::Value(Value::Int(99), DeterminacyState::Determined);
        let outcome = store.record_evaluation(
            node.clone(),
            result2,
            VersionId(2),
            DependencyTrace::default(),
        );
        assert_eq!(outcome, EvalOutcome::Changed);
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(2));
    }

    #[test]
    fn record_evaluation_cold_start_is_changed() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "x"));

        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let outcome = store.record_evaluation(
            node.clone(),
            result,
            VersionId(1),
            DependencyTrace::default(),
        );
        assert_eq!(outcome, EvalOutcome::Changed);
        assert!(store.get(&node).is_some());
    }

    // --- Version fast path tests ---

    #[test]
    fn try_fast_path_hit_when_same_version() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.put(node.clone(), make_test_node_cache(42, 1));

        let result = store.try_fast_path(&node, VersionId(1));
        assert!(result.is_some());
    }

    #[test]
    fn try_fast_path_miss_when_different_version() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.put(node.clone(), make_test_node_cache(42, 1));

        let result = store.try_fast_path(&node, VersionId(2));
        assert!(result.is_none());
    }

    #[test]
    fn try_fast_path_miss_when_not_cached() {
        let store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));

        let result = store.try_fast_path(&node, VersionId(1));
        assert!(result.is_none());
    }

    // --- CacheStore tests ---

    fn make_test_node_cache(val: i64, version: u64) -> NodeCache {
        NodeCache::new(
            CachedResult::Value(Value::Int(val), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(version),
        )
    }

    #[test]
    fn cache_store_new_is_empty() {
        let store = CacheStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn cache_store_put_and_get() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        let cache = make_test_node_cache(42, 1);
        store.put(node.clone(), cache);

        assert!(!store.is_empty());
        assert_eq!(store.len(), 1);
        assert!(store.get(&node).is_some());
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(1));
    }

    #[test]
    fn cache_store_get_missing() {
        let store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        assert!(store.get(&node).is_none());
    }

    #[test]
    fn cache_store_invalidate() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.put(node.clone(), make_test_node_cache(42, 1));
        assert!(store.get(&node).is_some());

        store.invalidate(&node);
        assert!(store.get(&node).is_none());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn cache_store_invalidate_missing_is_noop() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.invalidate(&node); // no panic
        assert!(store.is_empty());
    }

    #[test]
    fn cache_store_put_overwrites() {
        let mut store = CacheStore::new();
        let node = NodeId::Value(ValueCellId::new("Bracket", "width"));
        store.put(node.clone(), make_test_node_cache(42, 1));
        store.put(node.clone(), make_test_node_cache(99, 2));

        assert_eq!(store.len(), 1);
        assert_eq!(store.get(&node).unwrap().basis_version, VersionId(2));
    }

    #[test]
    fn cache_store_len_and_is_empty() {
        let mut store = CacheStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);

        let node1 = NodeId::Value(ValueCellId::new("Bracket", "width"));
        let node2 = NodeId::Value(ValueCellId::new("Bracket", "height"));
        store.put(node1, make_test_node_cache(42, 1));
        assert_eq!(store.len(), 1);
        assert!(!store.is_empty());

        store.put(node2, make_test_node_cache(99, 1));
        assert_eq!(store.len(), 2);
    }

    // --- EvalOutcome tests ---

    #[test]
    fn eval_outcome_changed_variant() {
        let outcome = EvalOutcome::Changed;
        assert_eq!(outcome, EvalOutcome::Changed);
        assert_ne!(outcome, EvalOutcome::Unchanged);
    }

    #[test]
    fn eval_outcome_unchanged_variant() {
        let outcome = EvalOutcome::Unchanged;
        assert_eq!(outcome, EvalOutcome::Unchanged);
        assert_ne!(outcome, EvalOutcome::Changed);
    }

    #[test]
    fn eval_outcome_debug_and_copy() {
        let outcome = EvalOutcome::Changed;
        let copied = outcome; // Copy
        assert_eq!(outcome, copied); // original still usable
        let debug = format!("{:?}", outcome);
        assert!(debug.contains("Changed"));
    }

    // --- NodeCache tests ---

    #[test]
    fn node_cache_construction() {
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};
        use crate::deps::DependencyTrace;

        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let expected_hash = result.content_hash();
        let cache = NodeCache::new(
            result.clone(),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );

        assert_eq!(cache.result_hash, expected_hash);
        assert_eq!(cache.freshness, Freshness::Final);
        assert_eq!(cache.basis_version, VersionId(1));
    }

    #[test]
    fn node_cache_clone_and_debug() {
        use reify_types::{DeterminacyState, Freshness, Value, VersionId};
        use crate::deps::DependencyTrace;

        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let cache = NodeCache::new(
            result,
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        );
        let cloned = cache.clone();
        assert_eq!(cache.result_hash, cloned.result_hash);
        assert_eq!(cache.basis_version, cloned.basis_version);

        let debug = format!("{:?}", cache);
        assert!(debug.contains("NodeCache"));
    }

    #[test]
    fn node_cache_result_hash_matches_content_hash() {
        use reify_types::{Freshness, Satisfaction, VersionId};
        use crate::deps::DependencyTrace;

        let result = CachedResult::Satisfaction(Satisfaction::Violated);
        let expected = result.content_hash();
        let cache = NodeCache::new(
            result,
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(5),
        );
        assert_eq!(cache.result_hash, expected);
    }

    // --- CachedResult tests ---

    #[test]
    fn cached_result_value_variant() {
        use reify_types::{DeterminacyState, Value};
        let result = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("Value"));
        assert!(debug.contains("42"));
    }

    #[test]
    fn cached_result_satisfaction_variant() {
        use reify_types::Satisfaction;
        let result = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("Satisfaction"));
    }

    #[test]
    fn cached_result_geometry_handle_variant() {
        use reify_types::GeometryHandleId;
        let result = CachedResult::GeometryHandle(GeometryHandleId(7));
        let cloned = result.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("GeometryHandle"));
    }

    #[test]
    fn cached_result_content_hash_value_variant() {
        use reify_types::{DeterminacyState, Value};
        let r1 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        let r2 = CachedResult::Value(Value::Int(42), DeterminacyState::Determined);
        assert_eq!(r1.content_hash(), r2.content_hash());

        // Different value -> different hash
        let r3 = CachedResult::Value(Value::Int(99), DeterminacyState::Determined);
        assert_ne!(r1.content_hash(), r3.content_hash());

        // Same value, different determinacy -> different hash
        let r4 = CachedResult::Value(Value::Int(42), DeterminacyState::Undetermined);
        assert_ne!(r1.content_hash(), r4.content_hash());
    }

    #[test]
    fn cached_result_content_hash_satisfaction_variant() {
        use reify_types::Satisfaction;
        let r1 = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let r2 = CachedResult::Satisfaction(Satisfaction::Satisfied);
        assert_eq!(r1.content_hash(), r2.content_hash());

        let r3 = CachedResult::Satisfaction(Satisfaction::Violated);
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    #[test]
    fn cached_result_content_hash_geometry_variant() {
        use reify_types::GeometryHandleId;
        let r1 = CachedResult::GeometryHandle(GeometryHandleId(7));
        let r2 = CachedResult::GeometryHandle(GeometryHandleId(7));
        assert_eq!(r1.content_hash(), r2.content_hash());

        let r3 = CachedResult::GeometryHandle(GeometryHandleId(8));
        assert_ne!(r1.content_hash(), r3.content_hash());
    }

    #[test]
    fn cached_result_content_hash_domain_separation() {
        // Ensure different variants produce different hashes even with
        // "similar" inner data
        use reify_types::{DeterminacyState, GeometryHandleId, Satisfaction, Value};
        let val = CachedResult::Value(Value::Int(0), DeterminacyState::Determined);
        let sat = CachedResult::Satisfaction(Satisfaction::Satisfied);
        let geo = CachedResult::GeometryHandle(GeometryHandleId(0));

        assert_ne!(val.content_hash(), sat.content_hash());
        assert_ne!(val.content_hash(), geo.content_hash());
        assert_ne!(sat.content_hash(), geo.content_hash());
    }
}
