use std::collections::HashMap;
use std::time::Instant;

use reify_eval::cache::NodeId;
use reify_types::OpaqueState;

/// Entry in the warm-state pool, wrapping an `OpaqueState` with metadata.
struct PoolEntry {
    state: OpaqueState,
    last_accessed: Instant,
    size_bytes: usize,
}

/// Memory-budgeted pool for warm-start state across evaluation nodes.
///
/// Stores `OpaqueState` keyed by `NodeId` with LRU eviction when the
/// total estimated memory usage exceeds the configured budget.
pub struct WarmStatePool {
    pool: HashMap<NodeId, PoolEntry>,
    budget_bytes: usize,
    used_bytes: usize,
}

impl WarmStatePool {
    /// Create a new pool with the given memory budget in bytes.
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            pool: HashMap::new(),
            budget_bytes,
            used_bytes: 0,
        }
    }

    /// Store warm-start state for a node.
    ///
    /// If the pool exceeds its memory budget after insertion, LRU eviction
    /// is triggered to bring usage back within budget.
    /// Store warm-start state for a node.
    ///
    /// If the pool exceeds its memory budget after insertion, LRU eviction
    /// is triggered to bring usage back within budget. A single item that
    /// exceeds the entire budget is still stored (over-budget by one item
    /// is acceptable).
    pub fn donate(&mut self, node_id: NodeId, state: OpaqueState) {
        let size = state.estimated_size_bytes();

        // If this node already has an entry, remove the old one first
        if let Some(old) = self.pool.remove(&node_id) {
            self.used_bytes = self.used_bytes.saturating_sub(old.size_bytes);
        }

        // Evict LRU entries until the new item fits within budget
        while self.used_bytes + size > self.budget_bytes && !self.pool.is_empty() {
            self.evict_lru();
        }

        let entry = PoolEntry {
            state,
            last_accessed: Instant::now(),
            size_bytes: size,
        };
        self.pool.insert(node_id, entry);
        self.used_bytes += size;
    }

    /// Evict the least-recently-accessed entry from the pool.
    fn evict_lru(&mut self) {
        let lru_key = self
            .pool
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(key, _)| key.clone());

        if let Some(key) = lru_key
            && let Some(entry) = self.pool.remove(&key)
        {
            self.used_bytes = self.used_bytes.saturating_sub(entry.size_bytes);
        }
    }

    /// Retrieve and remove warm-start state for a node (take semantics).
    ///
    /// Returns the `OpaqueState` if present, removing it from the pool.
    /// A second call for the same node returns `None`.
    pub fn retrieve(&mut self, node_id: &NodeId) -> Option<OpaqueState> {
        let entry = self.pool.remove(node_id)?;
        self.used_bytes = self.used_bytes.saturating_sub(entry.size_bytes);
        Some(entry.state)
    }

    /// Current estimated memory usage in bytes.
    pub fn used_bytes(&self) -> usize {
        self.used_bytes
    }

    /// Configured memory budget in bytes.
    pub fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }

    /// Remove all entries from the pool and reset used_bytes to 0.
    pub fn clear(&mut self) {
        self.pool.clear();
        self.used_bytes = 0;
    }

    /// Number of entries in the pool.
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    /// Whether the pool has no entries.
    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::ValueCellId;

    #[test]
    fn donate_and_retrieve_roundtrip() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        let state = OpaqueState::new(42i32, 4);

        pool.donate(node.clone(), state);
        let retrieved = pool.retrieve(&node);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().downcast::<i32>(), Some(42));
    }

    #[test]
    fn retrieve_removes_entry() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node.clone(), OpaqueState::new(42i32, 4));

        let first = pool.retrieve(&node);
        assert!(first.is_some());

        let second = pool.retrieve(&node);
        assert!(second.is_none());
    }

    #[test]
    fn used_bytes_tracks_correctly() {
        let mut pool = WarmStatePool::new(1024);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));

        assert_eq!(pool.used_bytes(), 0);

        pool.donate(node_a.clone(), OpaqueState::new(1i32, 100));
        assert_eq!(pool.used_bytes(), 100);

        pool.donate(node_b.clone(), OpaqueState::new(2i32, 200));
        assert_eq!(pool.used_bytes(), 300);

        pool.retrieve(&node_a);
        assert_eq!(pool.used_bytes(), 200);

        pool.retrieve(&node_b);
        assert_eq!(pool.used_bytes(), 0);
    }

    #[test]
    fn donate_exceeding_budget_triggers_lru_eviction() {
        // Budget of 300. Donate 3 items of 100 each (fits exactly).
        // Then donate a 4th item of 100 — should evict the LRU entry.
        let mut pool = WarmStatePool::new(300);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));
        let node_c = NodeId::Value(ValueCellId::new("T", "c"));
        let node_d = NodeId::Value(ValueCellId::new("T", "d"));

        pool.donate(node_a.clone(), OpaqueState::new(1i32, 100));
        pool.donate(node_b.clone(), OpaqueState::new(2i32, 100));
        pool.donate(node_c.clone(), OpaqueState::new(3i32, 100));
        assert_eq!(pool.used_bytes(), 300);

        // Donate 4th item — exceeds budget, should evict node_a (oldest)
        pool.donate(node_d.clone(), OpaqueState::new(4i32, 100));
        assert!(pool.used_bytes() <= 300);

        // node_a should have been evicted
        assert!(pool.retrieve(&node_a).is_none());
        // node_d should be present
        assert!(pool.retrieve(&node_d).is_some());
    }

    #[test]
    fn eviction_respects_access_order() {
        // Donate A, B, C (budget=250). Retrieve B (updates access time).
        // Donate a large item D (200) that requires eviction.
        // A should be evicted (oldest untouched), not B (recently accessed).
        let mut pool = WarmStatePool::new(250);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));
        let node_c = NodeId::Value(ValueCellId::new("T", "c"));
        let node_d = NodeId::Value(ValueCellId::new("T", "d"));

        pool.donate(node_a.clone(), OpaqueState::new(1i32, 50));
        pool.donate(node_b.clone(), OpaqueState::new(2i32, 50));
        pool.donate(node_c.clone(), OpaqueState::new(3i32, 50));
        // used = 150, budget = 250

        // Note: retrieve removes the entry, so we re-donate to simulate "access"
        // For this test, we use retrieve + re-donate to update access time.
        let b_state = pool.retrieve(&node_b).unwrap();
        pool.donate(node_b.clone(), b_state);
        // used = 150 still (retrieve - 50, donate + 50)

        // Donate large item that pushes over budget
        pool.donate(node_d.clone(), OpaqueState::new(4i32, 200));
        // used would be 350 > 250, need to evict. A and C are oldest.
        // Evict A (50), used = 300, still > 250
        // Evict C (50), used = 250, within budget

        // A should be evicted (oldest)
        assert!(pool.retrieve(&node_a).is_none());
        // C should also be evicted
        assert!(pool.retrieve(&node_c).is_none());
        // B (recently accessed) and D (just added) should remain
        assert!(pool.retrieve(&node_b).is_some());
        assert!(pool.retrieve(&node_d).is_some());
    }

    #[test]
    fn single_oversized_item_still_stored() {
        // Budget of 10, donate item of size 100 — should still store it
        let mut pool = WarmStatePool::new(10);
        let node = NodeId::Value(ValueCellId::new("T", "big"));
        pool.donate(node.clone(), OpaqueState::new(42i32, 100));

        let retrieved = pool.retrieve(&node);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().downcast::<i32>(), Some(42));
    }

    #[test]
    fn donate_same_node_id_replaces_and_adjusts_used_bytes() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));

        pool.donate(node.clone(), OpaqueState::new(1i32, 100));
        assert_eq!(pool.used_bytes(), 100);

        // Replace with a larger item
        pool.donate(node.clone(), OpaqueState::new(2i32, 300));
        assert_eq!(pool.used_bytes(), 300);

        // Should get the new value
        let retrieved = pool.retrieve(&node).unwrap();
        assert_eq!(retrieved.downcast::<i32>(), Some(2));
    }

    #[test]
    fn zero_budget_still_accepts_first_item() {
        let mut pool = WarmStatePool::new(0);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node.clone(), OpaqueState::new(42i32, 100));

        let retrieved = pool.retrieve(&node);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().downcast::<i32>(), Some(42));
    }

    #[test]
    fn clear_resets_pool() {
        let mut pool = WarmStatePool::new(1024);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));

        pool.donate(node_a.clone(), OpaqueState::new(1i32, 100));
        pool.donate(node_b.clone(), OpaqueState::new(2i32, 200));
        assert_eq!(pool.len(), 2);
        assert_eq!(pool.used_bytes(), 300);

        pool.clear();
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.used_bytes(), 0);
        assert!(pool.is_empty());
        assert!(pool.retrieve(&node_a).is_none());
    }

    #[test]
    fn len_and_is_empty() {
        let mut pool = WarmStatePool::new(1024);
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());

        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node.clone(), OpaqueState::new(1i32, 4));
        assert_eq!(pool.len(), 1);
        assert!(!pool.is_empty());

        pool.retrieve(&node);
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());
    }
}
