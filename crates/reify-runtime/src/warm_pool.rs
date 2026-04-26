use std::collections::HashMap;
use std::time::Instant;

use reify_eval::cache::NodeId;
use reify_types::OpaqueState;

/// Environment variable that overrides the warm-state pool memory budget.
///
/// Accepted values:
/// - Absent / unset → uses [`DEFAULT_BUDGET_BYTES`].
/// - `"unlimited"` (case-insensitive) → disables the budget; eviction is skipped.
/// - A decimal integer string → interpreted as bytes.
/// - Any other value → a `tracing::warn!` is emitted and [`DEFAULT_BUDGET_BYTES`] is used.
pub const BUDGET_ENV_VAR: &str = "REIFY_WARM_STATE_BUDGET_BYTES";

/// Default warm-state pool memory budget: 2 GiB.
///
/// Used when [`BUDGET_ENV_VAR`] is absent or set to an unparseable value.
pub const DEFAULT_BUDGET_BYTES: usize = 2 * 1024 * 1024 * 1024;

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
/// When the budget is `None` (unlimited), eviction is skipped entirely.
pub struct WarmStatePool {
    pool: HashMap<NodeId, PoolEntry>,
    budget_bytes: Option<usize>,
    used_bytes: usize,
}

impl WarmStatePool {
    /// Create a new pool with the given memory budget in bytes.
    ///
    /// This is a back-compat wrapper around [`with_budget`](Self::with_budget).
    pub fn new(budget_bytes: usize) -> Self {
        Self::with_budget(Some(budget_bytes))
    }

    /// Create a new pool with an explicit budget (or `None` for unlimited).
    pub fn with_budget(budget_bytes: Option<usize>) -> Self {
        Self {
            pool: HashMap::new(),
            budget_bytes,
            used_bytes: 0,
        }
    }

    /// Create a new pool with no memory budget (unlimited; eviction is disabled).
    pub fn unlimited() -> Self {
        Self::with_budget(None)
    }

    /// Create a pool by reading [`BUDGET_ENV_VAR`] from the environment.
    ///
    /// Falls back to [`DEFAULT_BUDGET_BYTES`] when the variable is unset.
    /// See [`from_env_value`](Self::from_env_value) for parse semantics.
    pub fn from_env_or_default() -> Self {
        Self::from_env_value(std::env::var(BUDGET_ENV_VAR).ok().as_deref())
    }

    /// Create a pool from an optional string value (the test seam for env-var parsing).
    ///
    /// | `value`              | Result                                                         |
    /// |----------------------|----------------------------------------------------------------|
    /// | `None`               | `Some(DEFAULT_BUDGET_BYTES)`                                   |
    /// | `"unlimited"` (any case) | `None` (unlimited)                                         |
    /// | parseable `usize`    | `Some(parsed)`                                                 |
    /// | anything else        | `tracing::warn!` emitted; `Some(DEFAULT_BUDGET_BYTES)` used    |
    pub fn from_env_value(value: Option<&str>) -> Self {
        let budget = match value {
            None => Some(DEFAULT_BUDGET_BYTES),
            Some(s) if s.eq_ignore_ascii_case("unlimited") => None,
            Some(s) => match s.parse::<usize>() {
                Ok(n) => Some(n),
                Err(_) => {
                    tracing::warn!(
                        env_var = BUDGET_ENV_VAR,
                        value = s,
                        default_bytes = DEFAULT_BUDGET_BYTES,
                        "Invalid value for {}; falling back to default ({} bytes)",
                        BUDGET_ENV_VAR,
                        DEFAULT_BUDGET_BYTES,
                    );
                    Some(DEFAULT_BUDGET_BYTES)
                }
            },
        };
        Self::with_budget(budget)
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

        // Evict LRU entries until the new item fits within budget (unlimited pools skip this)
        if let Some(budget) = self.budget_bytes {
            while self.used_bytes + size > budget && !self.pool.is_empty() {
                self.evict_lru();
            }
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

    /// Configured memory budget in bytes, or `None` if the pool is unlimited.
    pub fn budget_bytes(&self) -> Option<usize> {
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

    // --- Step 1: unlimited-budget path tests ---

    #[test]
    fn with_budget_none_reports_unlimited() {
        let pool_a = WarmStatePool::with_budget(None);
        assert_eq!(pool_a.budget_bytes(), None);

        let pool_b = WarmStatePool::unlimited();
        assert_eq!(pool_b.budget_bytes(), None);
    }

    #[test]
    fn unlimited_pool_does_not_evict() {
        // 5 items of 1 GiB each — would exceed the 2 GiB default but should
        // not be evicted because the pool has no budget limit.
        let mut pool = WarmStatePool::unlimited();
        let gib: usize = 1 << 30;
        let nodes: Vec<NodeId> = (0..5)
            .map(|i| NodeId::Value(ValueCellId::new("T", &format!("n{i}"))))
            .collect();

        for node in &nodes {
            pool.donate(node.clone(), OpaqueState::new(0u8, gib));
        }

        assert_eq!(pool.len(), 5);
        assert_eq!(pool.used_bytes(), 5 * gib);

        for node in &nodes {
            assert!(
                pool.retrieve(node).is_some(),
                "node {:?} should not have been evicted",
                node
            );
        }
    }

    #[test]
    fn legacy_new_still_returns_some_budget() {
        let pool = WarmStatePool::new(1024);
        assert_eq!(pool.budget_bytes(), Some(1024));
    }

    // --- Step 3: env-var parsing tests (compile-fails until step-4) ---

    #[test]
    fn default_budget_is_two_gib() {
        assert_eq!(DEFAULT_BUDGET_BYTES, 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn budget_env_var_name() {
        assert_eq!(BUDGET_ENV_VAR, "REIFY_WARM_STATE_BUDGET_BYTES");
    }

    #[test]
    fn from_env_value_none_uses_default() {
        let pool = WarmStatePool::from_env_value(None);
        assert_eq!(pool.budget_bytes(), Some(DEFAULT_BUDGET_BYTES));
    }

    #[test]
    fn from_env_value_numeric_parses() {
        let pool = WarmStatePool::from_env_value(Some("1024"));
        assert_eq!(pool.budget_bytes(), Some(1024));
    }

    #[test]
    fn from_env_value_unlimited_disables_budget() {
        // All three case variants must work
        assert_eq!(WarmStatePool::from_env_value(Some("unlimited")).budget_bytes(), None);
        assert_eq!(WarmStatePool::from_env_value(Some("UNLIMITED")).budget_bytes(), None);
        assert_eq!(WarmStatePool::from_env_value(Some("Unlimited")).budget_bytes(), None);
    }

    #[test]
    fn from_env_value_invalid_falls_back_to_default() {
        let pool = WarmStatePool::from_env_value(Some("not-a-number"));
        assert_eq!(pool.budget_bytes(), Some(DEFAULT_BUDGET_BYTES));
    }

    // --- Step 5: cost_per_byte metadata tests (compile-fails until step-6) ---

    #[test]
    fn donate_with_cost_records_cost_per_byte() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate_with_cost(node.clone(), OpaqueState::new(0u8, 100), 0.5);
        assert_eq!(pool.cost_per_byte_of(&node), Some(0.5));
    }

    #[test]
    fn legacy_donate_defaults_cost_to_zero() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node.clone(), OpaqueState::new(0u8, 100));
        assert_eq!(pool.cost_per_byte_of(&node), Some(0.0));
    }

    #[test]
    fn cost_per_byte_of_missing_node_is_none() {
        let pool = WarmStatePool::new(1024);
        let unknown = NodeId::Value(ValueCellId::new("T", "unknown"));
        assert_eq!(pool.cost_per_byte_of(&unknown), None);
    }

    #[test]
    fn donate_with_cost_replaces_cost_on_same_node() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate_with_cost(node.clone(), OpaqueState::new(0u8, 100), 0.5);
        pool.donate_with_cost(node.clone(), OpaqueState::new(0u8, 100), 1.5);
        assert_eq!(pool.cost_per_byte_of(&node), Some(1.5));
    }

    #[test]
    fn cost_per_byte_does_not_alter_lru_eviction_order() {
        // Eviction is still pure LRU; cost_per_byte is stored but not consulted.
        // Setup: budget=250, donate A(50,cost=10.0), B(50,cost=0.1), C(50,cost=5.0).
        // Touch B (retrieve+re-donate), then donate large D(200).
        // Expect A and C evicted (oldest), B and D retained — same as pure LRU.
        let mut pool = WarmStatePool::new(250);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));
        let node_c = NodeId::Value(ValueCellId::new("T", "c"));
        let node_d = NodeId::Value(ValueCellId::new("T", "d"));

        // High cost on the would-be LRU victim — eviction must still be LRU
        pool.donate_with_cost(node_a.clone(), OpaqueState::new(1i32, 50), 10.0);
        pool.donate_with_cost(node_b.clone(), OpaqueState::new(2i32, 50), 0.1);
        pool.donate_with_cost(node_c.clone(), OpaqueState::new(3i32, 50), 5.0);

        // Touch B to make it newer than A and C
        let b_state = pool.retrieve(&node_b).unwrap();
        pool.donate_with_cost(node_b.clone(), b_state, 0.1);

        // Large donation forces eviction
        pool.donate_with_cost(node_d.clone(), OpaqueState::new(4i32, 200), 2.0);

        // Pure LRU order: A and C (oldest) must be evicted; B and D retained
        assert!(pool.retrieve(&node_a).is_none(), "A should be LRU-evicted");
        assert!(pool.retrieve(&node_c).is_none(), "C should be LRU-evicted");
        assert!(pool.retrieve(&node_b).is_some(), "B should be retained (recently accessed)");
        assert!(pool.retrieve(&node_d).is_some(), "D should be retained (just added)");
    }
}
