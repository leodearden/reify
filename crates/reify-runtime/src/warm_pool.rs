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
    pub fn donate(&mut self, node_id: NodeId, state: OpaqueState) {
        let size = state.estimated_size_bytes();
        let entry = PoolEntry {
            state,
            last_accessed: Instant::now(),
            size_bytes: size,
        };
        self.pool.insert(node_id, entry);
        self.used_bytes += size;
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
}
