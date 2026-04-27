use std::collections::HashMap;
use std::time::Instant;

use crate::cache::NodeId;
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

/// Telemetry event emitted by `WarmStatePool` on the donate / evict transitions.
///
/// Buffered internally and consumed via [`WarmStatePool::drain_events`]; a future
/// engine integration translates these into `EventKind::Donated` / `EventKind::Evicted`
/// records on the diagnostic journal (see `reify_eval::journal`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WarmPoolEvent {
    /// A warm state was donated to the pool (insertion).
    Donated { node_id: NodeId, size_bytes: usize },
    /// A warm-state pool entry was evicted (LRU eviction kicked in to free budget).
    Evicted { node_id: NodeId, size_bytes: usize },
}

/// Entry in the warm-state pool, wrapping an `OpaqueState` with metadata.
struct PoolEntry {
    state: OpaqueState,
    last_accessed: Instant,
    size_bytes: usize,
    /// Estimated cost of recomputing this state in seconds per byte of output.
    ///
    /// Per arch §4.3 line 538: `estimated_cold_compute_time_secs / size_bytes`.
    /// Currently stored but not consulted by the eviction comparator (pure LRU).
    /// Reserved for the future cost-weighted-LRU eviction policy.
    cost_per_byte: f64,
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
    /// Buffered telemetry events (donations and evictions) — consumed via [`drain_events`](Self::drain_events).
    ///
    /// # Bounding note
    /// This buffer is unbounded until the engine wires `drain_events()` at evaluation
    /// boundaries (task 2345 follow-up).  In the interim every donate/evict appends
    /// here; callers that keep a long-lived pool without draining will see slow growth.
    /// TODO(task-2345): verify drain is called at every eval boundary and add an
    /// integration test asserting buffer stays empty between drains in steady state.
    events: Vec<WarmPoolEvent>,
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
            events: Vec::new(),
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
    ///
    /// # Wiring note
    /// This constructor is the intended entry point for runtime pool construction.
    /// TODO: wire this into the runtime's pool construction site so that
    /// `REIFY_WARM_STATE_BUDGET_BYTES` actually takes effect at runtime.
    ///
    /// # Unit-test coverage
    /// This thin wrapper delegates entirely to `from_env_value` and is intentionally
    /// not unit-tested with `std::env::set_var` (which is `unsafe` in Rust 2024 edition
    /// and race-prone across parallel tests). Integration tests cover the real env-read path.
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
            // An empty string is a common shell artifact (`VAR=` exports "" rather than unset).
            // Treat it the same as absent — use the default rather than emitting a spurious warn.
            Some("") => Some(DEFAULT_BUDGET_BYTES),
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

    /// Store warm-start state for a node with an explicit cost-per-byte estimate.
    ///
    /// `cost_per_byte` is `estimated_cold_compute_time_secs / size_bytes` per arch §4.3.
    /// It is stored on the pool entry as metadata for the future cost-weighted-LRU eviction
    /// policy but is **not** consulted by the current pure-LRU eviction comparator.
    ///
    /// If the pool exceeds its memory budget after insertion, LRU eviction is triggered
    /// to bring usage back within budget. A single item that exceeds the entire budget is
    /// still stored (over-budget by one item is acceptable). Unlimited pools (`budget_bytes`
    /// is `None`) skip eviction entirely.
    pub fn donate_with_cost(&mut self, node_id: NodeId, state: OpaqueState, cost_per_byte: f64) {
        // Sanitize cost_per_byte: clamp NaN, ±inf, and negative values to 0.0 so that
        // a future cost-weighted-LRU comparator can safely call `partial_cmp` without
        // panicking on non-finite values or mishandling negative costs.
        let cost_per_byte = if cost_per_byte.is_finite() && cost_per_byte >= 0.0 {
            cost_per_byte
        } else {
            0.0
        };
        let size = state.estimated_size_bytes();

        // Capture a clone for telemetry emission after the move into pool.insert.
        let node_id_for_event = node_id.clone();

        // If this node already has an entry, remove the old one first
        if let Some(old) = self.pool.remove(&node_id) {
            self.used_bytes = self.used_bytes.saturating_sub(old.size_bytes);
        }

        // Evict LRU entries until the new item fits within budget (unlimited pools skip this).
        // Each eviction pushes an Evicted event inside evict_lru(); evictions naturally
        // precede the Donated event below, giving the drain consumer a "pressure then arrival"
        // ordering useful for the diagnostic panel's narrative.
        if let Some(budget) = self.budget_bytes {
            while self.used_bytes + size > budget && !self.pool.is_empty() {
                self.evict_lru();
            }
        }

        let entry = PoolEntry {
            state,
            last_accessed: Instant::now(),
            size_bytes: size,
            cost_per_byte,
        };
        self.pool.insert(node_id, entry);
        self.used_bytes += size;

        // Emit Donated after all evictions so the drained buffer orders evictions
        // before the donation that forced them.
        self.events.push(WarmPoolEvent::Donated {
            node_id: node_id_for_event,
            size_bytes: size,
        });
    }

    /// Store warm-start state for a node.
    ///
    /// Back-compat wrapper; `cost_per_byte` defaults to `0.0` and is currently inert
    /// (eviction is pure LRU). Use [`donate_with_cost`](Self::donate_with_cost) to record
    /// the actual cost when known.
    pub fn donate(&mut self, node_id: NodeId, state: OpaqueState) {
        self.donate_with_cost(node_id, state, 0.0);
    }

    /// Return the stored `cost_per_byte` for a node, or `None` if the node is not in the pool.
    pub fn cost_per_byte_of(&self, node_id: &NodeId) -> Option<f64> {
        self.pool.get(node_id).map(|e| e.cost_per_byte)
    }

    /// Evict the least-recently-accessed entry from the pool.
    ///
    /// Pushes one `WarmPoolEvent::Evicted` per call (i.e. per victim) onto the
    /// internal buffer.  The caller (`donate_with_cost`) may call this in a loop,
    /// producing one event per evicted entry before the single `Donated` event.
    fn evict_lru(&mut self) {
        let lru_key = self
            .pool
            .iter()
            .min_by_key(|(_, entry)| entry.last_accessed)
            .map(|(key, _)| key.clone());

        if let Some(key) = lru_key
            && let Some(entry) = self.pool.remove(&key)
        {
            self.events.push(WarmPoolEvent::Evicted {
                node_id: key,
                size_bytes: entry.size_bytes,
            });
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
    ///
    /// Note: the return type changed from `usize` to `Option<usize>` in this task
    /// (task-2340). In-tree there are no other callers (verified via grep). Out-of-tree
    /// consumers that pattern-matched the old `usize` will need to handle the `Option`.
    #[must_use]
    pub fn budget_bytes(&self) -> Option<usize> {
        self.budget_bytes
    }

    /// Drain buffered telemetry events (donations and evictions) and clear the buffer.
    ///
    /// Intended to be called from the engine at evaluation boundaries; each drained
    /// `WarmPoolEvent` is then translated into an `EvalEvent` with the current
    /// `VersionId` and recorded on the diagnostic journal.
    pub fn drain_events(&mut self) -> Vec<WarmPoolEvent> {
        std::mem::take(&mut self.events)
    }

    /// Remove all entries from the pool, reset used_bytes to 0, and clear the event buffer.
    ///
    /// # Telemetry note
    /// Callers **must** call [`drain_events`](Self::drain_events) before `clear()` if they
    /// care about pending telemetry events.  Any un-drained `WarmPoolEvent`s are silently
    /// discarded by the buffer clear below.  A debug-mode assertion fires if this contract
    /// is violated, surfacing the misuse during tests.
    pub fn clear(&mut self) {
        debug_assert!(
            self.events.is_empty(),
            "WarmStatePool::clear() called with {} un-drained telemetry event(s); \
             call drain_events() first to avoid losing diagnostic data",
            self.events.len()
        );
        self.pool.clear();
        self.used_bytes = 0;
        self.events.clear();
    }

    /// Number of entries in the pool.
    pub fn len(&self) -> usize {
        self.pool.len()
    }

    /// Whether the pool has no entries.
    pub fn is_empty(&self) -> bool {
        self.pool.is_empty()
    }

    /// Whether the pool currently holds an entry for `node_id`.
    ///
    /// Non-destructive: unlike [`retrieve`](Self::retrieve), this leaves the
    /// pool untouched. Use it to probe membership without consuming the entry
    /// or perturbing the LRU ordering (`last_accessed` is not updated).
    pub fn contains(&self, node_id: &NodeId) -> bool {
        self.pool.contains_key(node_id)
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

        // Drain telemetry before clearing (required by the clear() contract).
        let _ = pool.drain_events();
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

    #[test]
    fn contains_reports_pool_membership_non_destructively() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));

        // Before any donate: not present.
        assert!(!pool.contains(&node));

        // After donate: present.
        pool.donate(node.clone(), OpaqueState::new(0u8, 100));
        assert!(pool.contains(&node));

        // Second call is idempotent — does not consume the entry.
        assert!(pool.contains(&node));
        assert_eq!(pool.len(), 1, "contains must not remove entries");
        assert_eq!(pool.used_bytes(), 100, "contains must not modify used_bytes");

        // After retrieve (destructive): no longer present.
        pool.retrieve(&node);
        assert!(!pool.contains(&node));

        // After clear: no longer present.
        pool.donate(node.clone(), OpaqueState::new(0u8, 100));
        assert!(pool.contains(&node));
        // Drain telemetry before clearing (required by the clear() contract).
        let _ = pool.drain_events();
        pool.clear();
        assert!(!pool.contains(&node));
    }

    #[test]
    fn with_budget_none_reports_unlimited() {
        let pool_a = WarmStatePool::with_budget(None);
        assert_eq!(pool_a.budget_bytes(), None);

        let pool_b = WarmStatePool::unlimited();
        assert_eq!(pool_b.budget_bytes(), None);
    }

    // 32-bit targets cannot represent 5 GiB in a usize; gate the test so CI
    // stays green if a 32-bit target is ever added.
    #[cfg(target_pointer_width = "64")]
    #[test]
    fn unlimited_pool_does_not_evict() {
        // 5 items of 1 GiB each — would exceed the 2 GiB default but should
        // not be evicted because the pool has no budget limit.
        let mut pool = WarmStatePool::unlimited();
        let gib: usize = 1 << 30;
        let nodes: Vec<NodeId> = (0..5)
            .map(|i| NodeId::Value(ValueCellId::new("T", format!("n{i}"))))
            .collect();

        for node in &nodes {
            pool.donate(node.clone(), OpaqueState::new(0u8, gib));
        }

        assert_eq!(pool.len(), 5);
        assert_eq!(pool.used_bytes(), 5 * gib);

        // Non-destructive: `retrieve` would consume entries (see `retrieve_removes_entry`).
        for node in &nodes {
            assert!(
                pool.contains(node),
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

    #[test]
    fn from_env_value_empty_string_uses_default() {
        // `VAR=` in shell exports an empty string rather than unsetting the var.
        // We must treat "" the same as absent rather than emitting a spurious warning.
        let pool = WarmStatePool::from_env_value(Some(""));
        assert_eq!(
            pool.budget_bytes(),
            Some(DEFAULT_BUDGET_BYTES),
            "empty string should fall back to default, not trigger a warn"
        );
    }

    #[test]
    fn donate_emits_donated_event() {
        let mut pool = WarmStatePool::new(1024);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        pool.donate(node_a.clone(), OpaqueState::new(0u8, 100));

        let events = pool.drain_events();
        assert_eq!(
            events,
            vec![WarmPoolEvent::Donated {
                node_id: node_a,
                size_bytes: 100
            }]
        );
    }

    #[test]
    fn donate_with_cost_also_emits_donated_event() {
        let mut pool = WarmStatePool::new(1024);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        pool.donate_with_cost(node_a.clone(), OpaqueState::new(0u8, 200), 1.5);

        let events = pool.drain_events();
        assert_eq!(
            events,
            vec![WarmPoolEvent::Donated {
                node_id: node_a,
                size_bytes: 200
            }]
        );
    }

    #[test]
    fn over_budget_donate_emits_evicted_then_donated() {
        // budget=200, donate A(100) and B(100) — fills exactly.
        // Drain intermediate events. Then donate C(100): forces eviction of A (LRU).
        // The drain after C should have Evicted(A) then Donated(C).
        let mut pool = WarmStatePool::new(200);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));
        let node_c = NodeId::Value(ValueCellId::new("T", "c"));

        pool.donate(node_a.clone(), OpaqueState::new(0u8, 100));
        pool.donate(node_b.clone(), OpaqueState::new(0u8, 100));
        // Clear intermediate events
        let setup_events = pool.drain_events();
        assert_eq!(setup_events.len(), 2);

        // Now donate C, which forces eviction of A (LRU)
        pool.donate(node_c.clone(), OpaqueState::new(0u8, 100));
        let events = pool.drain_events();

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0],
            WarmPoolEvent::Evicted {
                node_id: node_a,
                size_bytes: 100
            }
        );
        assert_eq!(
            events[1],
            WarmPoolEvent::Donated {
                node_id: node_c,
                size_bytes: 100
            }
        );
    }

    #[test]
    fn single_oversized_donation_emits_one_evicted_per_victim() {
        // budget=300, fill with three 100-byte items (used=300), then donate a 150-byte
        // item. The loop: 300+150=450>300 → evict A (used=200); 200+150=350>300 → evict B
        // (used=100); 100+150=250≤300 → stop. Expect 2 Evicted events + 1 Donated.
        let mut pool = WarmStatePool::new(300);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));
        let node_c = NodeId::Value(ValueCellId::new("T", "c"));
        let node_big = NodeId::Value(ValueCellId::new("T", "big"));

        pool.donate(node_a.clone(), OpaqueState::new(0u8, 100));
        pool.donate(node_b.clone(), OpaqueState::new(0u8, 100));
        pool.donate(node_c.clone(), OpaqueState::new(0u8, 100));
        pool.drain_events(); // clear setup events

        pool.donate(node_big.clone(), OpaqueState::new(0u8, 150));
        let events = pool.drain_events();

        // 2 Evicted + 1 Donated
        let evicted_count = events
            .iter()
            .filter(|e| matches!(e, WarmPoolEvent::Evicted { .. }))
            .count();
        let donated_count = events
            .iter()
            .filter(|e| matches!(e, WarmPoolEvent::Donated { .. }))
            .count();
        assert_eq!(evicted_count, 2, "expected 2 evictions for the oversized donation");
        assert_eq!(donated_count, 1, "expected 1 donated event");
        // The last event should be Donated (evictions precede the donation)
        assert!(
            matches!(events.last(), Some(WarmPoolEvent::Donated { .. })),
            "donation event must follow eviction events"
        );
    }

    #[test]
    fn drain_events_clears_buffer() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node, OpaqueState::new(0u8, 10));

        let first_drain = pool.drain_events();
        assert!(!first_drain.is_empty(), "first drain should have events");

        let second_drain = pool.drain_events();
        assert!(second_drain.is_empty(), "second drain should be empty after clearing");
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn unlimited_pool_still_emits_donated_but_never_evicted() {
        // unlimited() pool: donate 3 items larger than DEFAULT_BUDGET_BYTES combined.
        // Expect 3 Donated events and 0 Evicted events.
        let mut pool = WarmStatePool::unlimited();
        let gib: usize = 1 << 30; // 1 GiB > DEFAULT_BUDGET_BYTES / 3

        for i in 0..3usize {
            let node = NodeId::Value(ValueCellId::new("T", format!("n{i}")));
            pool.donate(node, OpaqueState::new(0u8, gib));
        }

        let events = pool.drain_events();
        let donated = events
            .iter()
            .filter(|e| matches!(e, WarmPoolEvent::Donated { .. }))
            .count();
        let evicted = events
            .iter()
            .filter(|e| matches!(e, WarmPoolEvent::Evicted { .. }))
            .count();
        assert_eq!(donated, 3, "unlimited pool should emit 3 Donated events");
        assert_eq!(evicted, 0, "unlimited pool must never emit Evicted events");
    }

    #[test]
    fn donate_with_cost_clamps_non_finite_cost() {
        // NaN, ±inf, and negative costs must be clamped to 0.0 at entry so that
        // the future cost-weighted-LRU comparator can safely call `partial_cmp`.
        let mut pool = WarmStatePool::new(1024);
        let node_nan = NodeId::Value(ValueCellId::new("T", "nan"));
        let node_inf = NodeId::Value(ValueCellId::new("T", "inf"));
        let node_neg_inf = NodeId::Value(ValueCellId::new("T", "neg_inf"));
        let node_neg = NodeId::Value(ValueCellId::new("T", "neg"));

        pool.donate_with_cost(node_nan.clone(), OpaqueState::new(0u8, 10), f64::NAN);
        pool.donate_with_cost(node_inf.clone(), OpaqueState::new(0u8, 10), f64::INFINITY);
        pool.donate_with_cost(
            node_neg_inf.clone(),
            OpaqueState::new(0u8, 10),
            f64::NEG_INFINITY,
        );
        pool.donate_with_cost(node_neg.clone(), OpaqueState::new(0u8, 10), -1.0);

        assert_eq!(pool.cost_per_byte_of(&node_nan), Some(0.0), "NaN clamped to 0.0");
        assert_eq!(pool.cost_per_byte_of(&node_inf), Some(0.0), "+inf clamped to 0.0");
        assert_eq!(pool.cost_per_byte_of(&node_neg_inf), Some(0.0), "-inf clamped to 0.0");
        assert_eq!(pool.cost_per_byte_of(&node_neg), Some(0.0), "negative clamped to 0.0");
    }

    // --- Task 2345 step-1: WarmStatePool::checkout alias tests ---
    //
    // These tests pin the `checkout` API named in arch §4.3 line 539: take-semantics
    // retrieval that returns `None` when the entry is absent OR has been LRU-evicted.
    // The method aliases the existing `retrieve` (same body, same contract); both
    // names remain valid for back-compat. Compile-fails until step-2 adds the alias.

    #[test]
    fn checkout_returns_none_when_absent() {
        let mut pool = WarmStatePool::new(1024);
        let unknown = NodeId::Value(ValueCellId::new("T", "absent"));
        assert!(
            pool.checkout(&unknown).is_none(),
            "checkout on a fresh pool with no donations must return None"
        );
    }

    #[test]
    fn checkout_returns_state_and_removes_entry() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node.clone(), OpaqueState::new(42i32, 4));

        let first = pool.checkout(&node);
        assert!(first.is_some(), "first checkout returns Some");
        assert_eq!(
            first.unwrap().downcast::<i32>(),
            Some(42),
            "checkout returns the donated state"
        );

        let second = pool.checkout(&node);
        assert!(
            second.is_none(),
            "checkout has take semantics — second call returns None"
        );
    }

    #[test]
    fn checkout_returns_none_after_eviction() {
        // Tiny budget pool: donate A (50 bytes), then donate B (50 bytes), then
        // donate C (200 bytes) which triggers LRU eviction of A and B.
        let mut pool = WarmStatePool::new(100);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));
        let node_c = NodeId::Value(ValueCellId::new("T", "c"));

        pool.donate(node_a.clone(), OpaqueState::new(1i32, 50));
        pool.donate(node_b.clone(), OpaqueState::new(2i32, 50));
        // used = 100 (at budget). Donating C (200 bytes) forces eviction of A then B.
        pool.donate(node_c.clone(), OpaqueState::new(3i32, 200));

        assert!(
            pool.checkout(&node_a).is_none(),
            "evicted entry checks out as None"
        );
        // C is the just-donated item and remains.
        assert!(
            pool.checkout(&node_c).is_some(),
            "freshly donated entry remains checkable"
        );
    }
}
