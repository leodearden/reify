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
    /// A warm-state pool entry was evicted — either because LRU eviction kicked in
    /// to free budget, OR because the same key was overwritten by a subsequent donate
    /// call (the prior entry is the victim whose state was discarded).
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
    /// # Bounding
    /// All mutations go through [`push_event`](Self::push_event), which enforces
    /// [`MAX_BUFFERED_EVENTS`](Self::MAX_BUFFERED_EVENTS):
    /// - **Debug builds**: a `debug_assert!` fires at the cap, surfacing the
    ///   "engine never drains" scenario loudly during `cargo test`.
    /// - **Release builds**: the oldest half of the buffer is auto-trimmed and
    ///   a single `tracing::warn!` is emitted per pool instance.
    ///
    /// TODO(task-2345): verify engine wires `drain_events()` at every eval
    /// boundary and add an integration test asserting the buffer stays near-empty
    /// in steady state.
    events: Vec<WarmPoolEvent>,
    /// Guards the once-per-pool-instance `tracing::warn!` emitted when the
    /// events buffer overflows and is auto-trimmed in release builds.
    ///
    /// Set to `true` on first trim; subsequent trim rounds on the same pool
    /// instance are silent to avoid log spam.  Per-instance scoping means each
    /// fresh pool emits its own first-overflow warn — useful when multiple pools
    /// co-exist in tests.
    auto_trim_warned: bool,
    /// Cumulative count of telemetry events silently dropped by the auto-trim
    /// safety net (release builds only).
    ///
    /// Incremented by `MAX_BUFFERED_EVENTS / 2` on every trim round.  Exposed via
    /// [`dropped_events`](Self::dropped_events) for diagnostic use (e.g. engine
    /// health checks or the diagnostic panel).  A non-zero value indicates that
    /// `drain_events()` is not being called at evaluation boundaries (task 2345
    /// follow-up).  Always `0` in normal steady-state operation.
    dropped_events: u64,
    /// Test-only override for the events buffer cap.
    ///
    /// When `Some(n)`, [`push_event`](Self::push_event) uses `n` instead of
    /// [`MAX_BUFFERED_EVENTS`](Self::MAX_BUFFERED_EVENTS).  This lets field-schema
    /// tests (e.g. `auto_trim_warn_omits_invariant_current_len_field`) fire the
    /// auto-trim warn with ~17 donations instead of 65 537, dramatically reducing
    /// unnecessary allocations in tests that only care about the warn's field set.
    ///
    /// Only present in test builds; has no effect on production behaviour.
    #[cfg(test)]
    test_events_cap: Option<usize>,
}

impl WarmStatePool {
    /// Maximum number of buffered telemetry events before the cap enforcement fires.
    ///
    /// # Rationale
    /// [`WarmStatePool::events`] is unbounded until the engine wires `drain_events()`
    /// at evaluation boundaries (task 2345 follow-up).  This constant defines the hard
    /// cap for two complementary safety nets:
    ///
    /// 1. **Debug-build tripwire** — a `debug_assert!` fires when the buffer reaches
    ///    this count, surfacing "engine never drains" loudly during `cargo test` (which
    ///    runs in debug mode by default).
    ///
    /// 2. **Release-build auto-trim** — when `events.len()` exceeds this cap the oldest
    ///    half of the buffer is dropped so memory stays bounded, and a single
    ///    `tracing::warn!` is emitted per pool instance (see `auto_trim_warned`).
    ///
    /// # Sizing
    /// 65 536 events × ≈64 bytes each ≈ **4 MiB** — three orders of magnitude below the
    /// 2 GiB warm-pool budget.  Normal test fixtures donate at most a handful of events,
    /// so this cap will never fire accidentally.
    ///
    /// See also: task 2345 (engine drain wiring follow-up).
    pub const MAX_BUFFERED_EVENTS: usize = 65_536;

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
            auto_trim_warned: false,
            dropped_events: 0,
            #[cfg(test)]
            test_events_cap: None,
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

    /// Create an unlimited pool with a test-only override for the events buffer cap.
    ///
    /// Intended for field-schema tests that need to trigger the auto-trim warn without
    /// pushing [`MAX_BUFFERED_EVENTS`](Self::MAX_BUFFERED_EVENTS) (65 536) events.  With
    /// `cap = 16`, for example, only 17 donations are needed to force one trim round.
    ///
    /// Note: the `debug_assert!` in [`push_event`](Self::push_event) fires at `cap` in
    /// debug builds, so callers must remain gated `#[cfg(not(debug_assertions))]` — the
    /// debug assertion still enforces the cap; only the *trim path* (release-only) is
    /// exercised by tests using this constructor.
    #[cfg(all(test, not(debug_assertions)))]
    pub(crate) fn with_test_events_cap(cap: usize) -> Self {
        let mut pool = Self::unlimited();
        pool.test_events_cap = Some(cap);
        pool
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
        self.insert_entry(node_id, state, Instant::now(), cost_per_byte);
    }

    /// Store warm-start state for a node.
    ///
    /// Back-compat wrapper; `cost_per_byte` defaults to `0.0` and is currently inert
    /// (eviction is pure LRU). Use [`donate_with_cost`](Self::donate_with_cost) to record
    /// the actual cost when known.
    pub fn donate(&mut self, node_id: NodeId, state: OpaqueState) {
        self.donate_with_cost(node_id, state, 0.0);
    }

    /// Re-donate a checked-out entry, preserving its original `last_accessed` timestamp.
    ///
    /// Use this on the (4c)→(14b) cache-miss path (see `engine_edit.rs` step (14b)) when
    /// an entry that was checked out via [`checkout_with_lru_stamp`](Self::checkout_with_lru_stamp)
    /// must be returned to the pool without refreshing its LRU clock.  Calling the ordinary
    /// [`donate`](Self::donate) instead would stamp the entry with `Instant::now()`, making it
    /// appear "recently accessed" and unfairly shielding it from eviction relative to entries
    /// that were never checked out.
    ///
    /// Semantics are otherwise identical to [`donate`](Self::donate): `cost_per_byte` defaults
    /// to `0.0` (matching the cost of entries that round-trip through the (14b) cache-miss arm,
    /// which had no recorded cost), and the eviction loop runs as normal.
    ///
    /// # Known limitation: `cost_per_byte` is silently reset to `0.0`
    ///
    /// This method does **not** accept nor preserve the entry's original `cost_per_byte`.
    /// For the current pure-LRU eviction policy this is benign — cost is not consulted during
    /// eviction.  Once cost-weighted LRU is activated (the `cost_per_byte` field exists for that
    /// purpose; see `insert_entry`'s cost-weighted comparator comment), entries that round-trip
    /// through the (4c)→(14b) cache-miss arm will systematically look cheaper than fresh
    /// donations, partially defeating the LRU-stamp-preservation fix this method provides.
    ///
    /// FIXME(cost-weighted-lru): extend the signature to accept and thread through the original
    /// `cost_per_byte` (or add a `checkout_with_lru_stamp_and_cost` variant).  The pinning test
    /// `donate_preserving_lru_resets_cost_to_zero_known_limitation` documents and will catch this
    /// regression when cost-weighted LRU lands.
    ///
    /// # Architecture reference
    /// arch §4.3 line 539 "(4c)→(14b) round-trip"; see also `engine_edit.rs` step (14b) doc
    /// comment block for the rationale behind LRU-stamp preservation on the cache-miss path.
    pub fn donate_preserving_lru(
        &mut self,
        node_id: NodeId,
        state: OpaqueState,
        last_accessed: Instant,
    ) {
        self.insert_entry(node_id, state, last_accessed, 0.0);
    }

    /// Shared core of all donate variants: sanitise cost, evict if over budget, insert entry,
    /// emit `Donated` event.
    ///
    /// `last_accessed` is provided by the caller so that [`donate_with_cost`] can use
    /// `Instant::now()` while [`donate_preserving_lru`] can pass a previously-captured stamp.
    fn insert_entry(
        &mut self,
        node_id: NodeId,
        state: OpaqueState,
        last_accessed: Instant,
        cost_per_byte: f64,
    ) {
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

        // If this node already has an entry, remove the old one first and emit an
        // Evicted event for the displaced entry so byte-accounting consumers can
        // maintain the invariant Σ Donated.size − Σ Evicted.size = used_bytes.
        // The emit must happen BEFORE the LRU loop so the drained sequence reads
        // "overwrite-victim evicted → optional LRU pressure evictions → donation".
        if let Some(old) = self.pool.remove(&node_id) {
            self.push_event(WarmPoolEvent::Evicted {
                node_id: node_id.clone(),
                size_bytes: old.size_bytes,
            });
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
            last_accessed,
            size_bytes: size,
            cost_per_byte,
        };
        self.pool.insert(node_id, entry);
        self.used_bytes += size;

        // Emit Donated after all evictions so the drained buffer orders evictions
        // before the donation that forced them.
        self.push_event(WarmPoolEvent::Donated {
            node_id: node_id_for_event,
            size_bytes: size,
        });
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
            self.push_event(WarmPoolEvent::Evicted {
                node_id: key,
                size_bytes: entry.size_bytes,
            });
            self.used_bytes = self.used_bytes.saturating_sub(entry.size_bytes);
        }
    }

    /// Returns the effective events buffer cap.
    ///
    /// In production builds this is always [`MAX_BUFFERED_EVENTS`](Self::MAX_BUFFERED_EVENTS).
    /// In test builds it returns the test-only override when one has been set via
    /// [`with_test_events_cap`](Self::with_test_events_cap), falling back to
    /// `MAX_BUFFERED_EVENTS` otherwise.  Using this helper in [`push_event`](Self::push_event)
    /// keeps the cap logic in one place and lets test fixtures fire the auto-trim path with
    /// a tiny cap (~17 events) rather than 65 537.
    #[inline]
    fn events_cap_effective(&self) -> usize {
        #[cfg(test)]
        if let Some(n) = self.test_events_cap {
            return n;
        }
        Self::MAX_BUFFERED_EVENTS
    }

    /// Append one telemetry event to the internal buffer, enforcing the cap.
    ///
    /// # Cap enforcement (layered safety nets)
    ///
    /// **Debug builds** — a `debug_assert!` fires when the buffer is already at
    /// [`MAX_BUFFERED_EVENTS`](Self::MAX_BUFFERED_EVENTS), causing a visible panic in
    /// `cargo test`.  This surfaces "engine never drains" loudly during development.
    ///
    /// **Release builds** — when `events.len() > MAX_BUFFERED_EVENTS` the oldest half
    /// is auto-trimmed, the [`dropped_events`](Self::dropped_events) counter is
    /// incremented by `MAX_BUFFERED_EVENTS / 2`, and a single `tracing::warn!` (with
    /// the running `total_dropped` field) is emitted per pool instance.
    ///
    /// All donate/evict event emissions must go through this helper so the cap logic
    /// lives in exactly one place.
    fn push_event(&mut self, ev: WarmPoolEvent) {
        let cap = self.events_cap_effective();
        debug_assert!(
            self.events.len() < cap,
            "WarmStatePool events buffer reached cap of {}; \
             engine drain_events() is not wired at evaluation boundaries \
             (task 2345 follow-up)",
            cap
        );
        self.events.push(ev);
        // Release-build seatbelt: if the buffer exceeded the cap (debug_assert!
        // is a no-op in release mode), drop the oldest half so memory stays bounded,
        // track the cumulative drop count, and emit a once-per-pool-instance warn.
        if self.events.len() > cap {
            self.dropped_events += (cap / 2) as u64;
            if !self.auto_trim_warned {
                tracing::warn!(
                    cap,
                    total_dropped = self.dropped_events,
                    task = "2345-followup",
                    "WarmStatePool events buffer exceeded cap; auto-trimming oldest half. \
                     Engine drain_events() not wired at evaluation boundaries."
                );
                self.auto_trim_warned = true;
            }
            self.events.drain(..cap / 2);
        }
    }

    /// Check out warm-start state for a node (take semantics).
    ///
    /// Architecturally-named per arch §4.3 line 539. Returns
    /// `Some(OpaqueState)` when the entry is present (and removes it from
    /// the pool); returns `None` when the entry is absent OR has been
    /// LRU-evicted. A second call for the same node returns `None`.
    ///
    /// Thin wrapper around [`checkout_with_lru_stamp`](Self::checkout_with_lru_stamp)
    /// that discards the `last_accessed` timestamp.  Use `checkout_with_lru_stamp`
    /// when the entry may need to be re-donated via
    /// [`donate_preserving_lru`](Self::donate_preserving_lru) on the (4c)→(14b)
    /// cache-miss path (arch §4.3).
    pub fn checkout(&mut self, node_id: &NodeId) -> Option<OpaqueState> {
        self.checkout_with_lru_stamp(node_id).map(|(s, _)| s)
    }

    /// Check out warm-start state together with the entry's original `last_accessed`
    /// timestamp (take semantics).
    ///
    /// Returns `Some((OpaqueState, Instant))` when the entry is present; the `Instant`
    /// is the value recorded at donation time (set by [`donate`](Self::donate) /
    /// [`donate_with_cost`](Self::donate_with_cost) via `Instant::now()`, or explicitly
    /// supplied by [`donate_preserving_lru`](Self::donate_preserving_lru)).
    ///
    /// The caller can pass the returned `Instant` directly to `donate_preserving_lru`
    /// to re-insert the entry without refreshing its LRU clock — the intended use on the
    /// `engine_edit.rs` (4c)→(14b) cache-miss path.  See `donate_preserving_lru` for the
    /// full rationale.
    ///
    /// Returns `None` when the entry is absent or has been LRU-evicted. A second call for
    /// the same node returns `None` (take semantics identical to [`checkout`](Self::checkout)).
    pub fn checkout_with_lru_stamp(
        &mut self,
        node_id: &NodeId,
    ) -> Option<(OpaqueState, Instant)> {
        let entry = self.pool.remove(node_id)?;
        self.used_bytes = self.used_bytes.saturating_sub(entry.size_bytes);
        Some((entry.state, entry.last_accessed))
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

    /// Total number of telemetry events silently dropped by the auto-trim safety net
    /// (release builds only; debug builds panic via `debug_assert!` instead).
    ///
    /// Each auto-trim round drops `MAX_BUFFERED_EVENTS / 2` events; this counter
    /// accumulates across all rounds since the pool was constructed.
    ///
    /// A non-zero value is a health signal: it means `drain_events()` is not being
    /// called at evaluation boundaries (task 2345 follow-up).  Returns `0` in the
    /// steady-state case where the engine drains regularly.
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events
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
    fn donate_and_checkout_roundtrip() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        let state = OpaqueState::new(42i32, 4);

        pool.donate(node.clone(), state);
        let retrieved = pool.checkout(&node);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().downcast::<i32>(), Some(42));
    }

    #[test]
    fn checkout_removes_entry() {
        let mut pool = WarmStatePool::new(1024);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node.clone(), OpaqueState::new(42i32, 4));

        let first = pool.checkout(&node);
        assert!(first.is_some());

        let second = pool.checkout(&node);
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

        pool.checkout(&node_a);
        assert_eq!(pool.used_bytes(), 200);

        pool.checkout(&node_b);
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
        assert!(pool.checkout(&node_a).is_none());
        // node_d should be present
        assert!(pool.checkout(&node_d).is_some());
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

        // Note: checkout removes the entry, so we re-donate to simulate "access"
        // For this test, we use checkout + re-donate to update access time.
        let b_state = pool.checkout(&node_b).unwrap();
        pool.donate(node_b.clone(), b_state);
        // used = 150 still (checkout - 50, donate + 50)

        // Donate large item that pushes over budget
        pool.donate(node_d.clone(), OpaqueState::new(4i32, 200));
        // used would be 350 > 250, need to evict. A and C are oldest.
        // Evict A (50), used = 300, still > 250
        // Evict C (50), used = 250, within budget

        // A should be evicted (oldest)
        assert!(pool.checkout(&node_a).is_none());
        // C should also be evicted
        assert!(pool.checkout(&node_c).is_none());
        // B (recently accessed) and D (just added) should remain
        assert!(pool.checkout(&node_b).is_some());
        assert!(pool.checkout(&node_d).is_some());
    }

    #[test]
    fn single_oversized_item_still_stored() {
        // Budget of 10, donate item of size 100 — should still store it
        let mut pool = WarmStatePool::new(10);
        let node = NodeId::Value(ValueCellId::new("T", "big"));
        pool.donate(node.clone(), OpaqueState::new(42i32, 100));

        let retrieved = pool.checkout(&node);
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
        let retrieved = pool.checkout(&node).unwrap();
        assert_eq!(retrieved.downcast::<i32>(), Some(2));
    }

    #[test]
    fn zero_budget_still_accepts_first_item() {
        let mut pool = WarmStatePool::new(0);
        let node = NodeId::Value(ValueCellId::new("T", "x"));
        pool.donate(node.clone(), OpaqueState::new(42i32, 100));

        let retrieved = pool.checkout(&node);
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
        assert!(pool.checkout(&node_a).is_none());
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

        pool.checkout(&node);
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

        // After checkout (destructive): no longer present.
        pool.checkout(&node);
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

        // Non-destructive: `checkout` would consume entries (see `checkout_removes_entry`).
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
        let b_state = pool.checkout(&node_b).unwrap();
        pool.donate_with_cost(node_b.clone(), b_state, 0.1);

        // Large donation forces eviction
        pool.donate_with_cost(node_d.clone(), OpaqueState::new(4i32, 200), 2.0);

        // Pure LRU order: A and C (oldest) must be evicted; B and D retained
        assert!(pool.checkout(&node_a).is_none(), "A should be LRU-evicted");
        assert!(pool.checkout(&node_c).is_none(), "C should be LRU-evicted");
        assert!(pool.checkout(&node_b).is_some(), "B should be retained (recently accessed)");
        assert!(pool.checkout(&node_d).is_some(), "D should be retained (just added)");
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

    // --- Task 2345 step-1: WarmStatePool::checkout tests ---
    //
    // These tests pin the `checkout` API named in arch §4.3 line 539: take-semantics
    // retrieval that returns `None` when the entry is absent OR has been LRU-evicted.
    // (Originally introduced as an alias for `retrieve`; the duplicate `retrieve`
    // method was removed in the amendment pass — `checkout` is the canonical name.)

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

    // --- Task 2457: events buffer bound / tripwire ---

    /// Assert the debug-build tripwire: pushing past `MAX_BUFFERED_EVENTS` panics
    /// via `debug_assert!` in debug builds.
    ///
    /// # Why `#[cfg(debug_assertions)]`
    /// In release builds, `debug_assert!` is a no-op — the panic never fires, so
    /// `catch_unwind` would return `Ok` and the `is_err()` assertion would fail.
    /// The complementary release-mode path is covered by
    /// `events_buffer_auto_trims_to_keep_recent_events_when_engine_never_drains`
    /// (step 5), which is gated `#[cfg(not(debug_assertions))]`.
    ///
    /// # Accepted noise
    /// `catch_unwind` on a `debug_assert!` panic may emit a panic stacktrace to
    /// stderr in default `cargo test` runs (the libtest panic hook fires before
    /// `catch_unwind` suppresses the unwind).  Correctness is unaffected; this
    /// matches the accepted behavior of `engine_purposes.rs:709` and its sibling.
    #[test]
    #[cfg(debug_assertions)]
    fn events_buffer_debug_assert_fires_on_overflow_in_debug_build() {
        use std::panic::AssertUnwindSafe;
        // NOTE: This test performs MAX_BUFFERED_EVENTS (65 536) donations to fill the
        // buffer to the exact cap, then one more to trigger the debug_assert! panic.
        // The count cannot be reduced without a test-only cap seam: the off-by-one
        // boundary (len == MAX passes, len == MAX+1 fires) requires filling to the
        // precise cap.  ~65 k HashMap inserts complete in ≲ 100 ms.  This test is
        // gated #[cfg(debug_assertions)] and does not run in the release test pass.

        // Build a pool with effectively unlimited memory budget so LRU eviction
        // never interferes.  We are testing the events-buffer tripwire, not eviction.
        let mut pool = WarmStatePool::unlimited();
        let template = NodeId::Value(reify_types::ValueCellId::new("T", "n"));

        // Fill the buffer to exactly MAX_BUFFERED_EVENTS by donating that many
        // distinct nodes (each donate() emits exactly one Donated event).
        // We drain_events() once at the start to empty the buffer, then donate
        // MAX_BUFFERED_EVENTS items without draining.
        let _ = pool.drain_events();
        for i in 0..WarmStatePool::MAX_BUFFERED_EVENTS {
            let node = NodeId::Value(reify_types::ValueCellId::new("T", format!("n{i}")));
            pool.donate(node, OpaqueState::new(0u8, 1));
        }
        // Buffer is now at capacity.  One more donation must fire the debug_assert!.
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            pool.donate(template, OpaqueState::new(0u8, 1));
        }));

        assert!(
            result.is_err(),
            "debug-build tripwire: expected debug_assert! panic when pushing past \
             MAX_BUFFERED_EVENTS ({}) without draining, but catch_unwind returned Ok",
            WarmStatePool::MAX_BUFFERED_EVENTS
        );
    }

    /// Assert the release-build auto-trim: after donating more than `MAX_BUFFERED_EVENTS`
    /// events without draining, the buffer stays bounded and the most-recent events
    /// are preserved (oldest are discarded).
    ///
    /// # Why `#[cfg(not(debug_assertions))]`
    /// In debug builds, `push_event`'s `debug_assert!` fires at the cap and panics
    /// before the trim path is reached.  This test covers the release-mode path
    /// exercised by the orchestrator's `cargo test -p reify-eval --release` pass.
    ///
    /// # What "keep newest" means
    /// The auto-trim drops the oldest half of the buffer, so after overflow the last
    /// event in `drain_events()` references the most-recently donated node.
    #[test]
    #[cfg(not(debug_assertions))]
    fn events_buffer_auto_trims_to_keep_recent_events_when_engine_never_drains() {
        let mut pool = WarmStatePool::unlimited();

        let total = WarmStatePool::MAX_BUFFERED_EVENTS + 100;
        let mut last_node = NodeId::Value(reify_types::ValueCellId::new("T", "n0"));
        for i in 0..total {
            let node = NodeId::Value(reify_types::ValueCellId::new("T", format!("n{i}")));
            last_node = node.clone();
            pool.donate(node, OpaqueState::new(0u8, 1));
        }

        let events = pool.drain_events();

        // (a) Buffer stayed bounded.
        assert!(
            events.len() <= WarmStatePool::MAX_BUFFERED_EVENTS,
            "auto-trim: expected at most {} events after {} donations, got {}",
            WarmStatePool::MAX_BUFFERED_EVENTS,
            total,
            events.len()
        );

        // (b) The last event references the most-recently donated node (newest kept).
        let last_event_node = match events.last() {
            Some(WarmPoolEvent::Donated { node_id, .. }) => node_id.clone(),
            other => panic!(
                "auto-trim: expected last event to be Donated, got {:?}",
                other
            ),
        };
        assert_eq!(
            last_event_node, last_node,
            "auto-trim must keep newest events and discard oldest; \
             last event should reference the final donated node"
        );

        // (c) dropped_events counter reflects the events silently dropped by the trim.
        // We donated MAX+100 events: that triggers exactly one trim (at the MAX+1-th
        // push), dropping MAX/2 events from the front.
        assert_eq!(
            pool.dropped_events(),
            (WarmStatePool::MAX_BUFFERED_EVENTS / 2) as u64,
            "after one trim round, dropped_events should equal MAX_BUFFERED_EVENTS / 2"
        );
    }

    /// Assert the once-per-session warn: a single `tracing::warn!` is emitted the
    /// first time the auto-trim fires on a given pool instance, and subsequent trim
    /// rounds on the same pool are silent.
    ///
    /// # Why `#[cfg(not(debug_assertions))]`
    /// In debug builds, `push_event`'s `debug_assert!` fires at the cap, so the
    /// auto-trim and warn paths are never reached.  This test covers the release-mode
    /// path exercised by the orchestrator's `cargo test -p reify-eval --release` pass.
    ///
    /// # "Per-pool-instance" scoping
    /// Each pool gets its own `auto_trim_warned: bool` field, so distinct pools each
    /// emit their first-overflow warn independently.
    #[test]
    #[cfg(not(debug_assertions))]
    fn events_buffer_emits_tracing_warn_once_per_session_on_overflow() {
        use std::sync::atomic::Ordering;
        use reify_test_support::CountingSubscriberBuilder;
        // NOTE: This test performs MAX_BUFFERED_EVENTS * 2 + 100 ≈ 131 k donations.
        // Two full overflow rounds are the minimum needed to verify the warn-once
        // invariant: a single overflow would not distinguish "warn exactly once" from
        // "warn on every overflow".  The loop completes in ≲ 200 ms in release mode.
        // This test is gated #[cfg(not(debug_assertions))] and runs only in the
        // orchestrator's release test pass (`cargo test -p reify-eval --release`).

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_eval::warm_pool")
            .build();
        let warn_count = counters[&tracing::Level::WARN].clone();

        tracing::subscriber::with_default(subscriber, || {
            let mut pool = WarmStatePool::unlimited();

            // Donate enough events to overflow at least twice.
            for i in 0..(WarmStatePool::MAX_BUFFERED_EVENTS * 2 + 100) {
                let node =
                    NodeId::Value(reify_types::ValueCellId::new("T", format!("n{i}")));
                pool.donate(node, OpaqueState::new(0u8, 1));
            }
        });

        assert_eq!(
            warn_count.load(Ordering::Acquire),
            1,
            "warn-once-per-session: expected exactly 1 WARN from reify_eval::warm_pool \
             when the events buffer overflows multiple times on the same pool instance; \
             subsequent overflows must be silent"
        );
    }

    /// `dropped_events()` returns zero on a fresh pool regardless of build mode.
    #[test]
    fn events_buffer_dropped_events_starts_at_zero() {
        assert_eq!(
            WarmStatePool::unlimited().dropped_events(),
            0,
            "fresh pool must report zero dropped events before any trim fires"
        );
        assert_eq!(
            WarmStatePool::new(1024).dropped_events(),
            0,
            "fresh budgeted pool must also start at zero"
        );
    }

    /// `dropped_events()` accumulates across multiple trim rounds in release builds.
    ///
    /// # Why `#[cfg(not(debug_assertions))]`
    /// Debug builds panic via `debug_assert!` before the trim path runs.
    /// NOTE: Uses MAX_BUFFERED_EVENTS + 1 + MAX/2 ≈ 98 k donations, release-only.
    #[test]
    #[cfg(not(debug_assertions))]
    fn events_buffer_dropped_events_accumulates_across_trim_rounds() {
        let mut pool = WarmStatePool::unlimited();

        // Trigger the first trim: donate MAX+1 events (the (MAX+1)-th push takes
        // events.len() to MAX+1 > MAX, firing the trim and dropping MAX/2 events).
        for i in 0..=WarmStatePool::MAX_BUFFERED_EVENTS {
            let node = NodeId::Value(reify_types::ValueCellId::new("T", format!("n{i}")));
            pool.donate(node, OpaqueState::new(0u8, 1));
        }
        assert_eq!(
            pool.dropped_events(),
            (WarmStatePool::MAX_BUFFERED_EVENTS / 2) as u64,
            "after first trim: dropped_events == MAX/2"
        );

        // After the first trim, events.len() == MAX/2 + 1.  Donate MAX/2 more events
        // to push len back to MAX+1 and trigger a second trim.
        for i in 0..WarmStatePool::MAX_BUFFERED_EVENTS / 2 {
            let node = NodeId::Value(reify_types::ValueCellId::new("T", format!("m{i}")));
            pool.donate(node, OpaqueState::new(0u8, 1));
        }
        assert_eq!(
            pool.dropped_events(),
            WarmStatePool::MAX_BUFFERED_EVENTS as u64,
            "after second trim: dropped_events == MAX (two × MAX/2 rounds accumulated)"
        );
    }

    /// The auto-trim `tracing::warn!` must NOT include the `current_len` field.
    ///
    /// `current_len` is captured after the overflow push but before the drain, so it
    /// is always `MAX_BUFFERED_EVENTS + 1` — a constant derivable from `cap`.  It
    /// provides zero diagnostic signal and is explicitly excluded from the warn schema.
    ///
    /// This test also positively pins that `cap` and `total_dropped` ARE present, so
    /// we're verifying the auto-trim warn and not some unrelated warn from the pool.
    ///
    /// Uses [`WarmStatePool::with_test_events_cap`] to set the cap to 16, so only 17
    /// donations are needed to trigger the trim rather than 65 537.  None of the
    /// existing auto-trim tests pin the warn's field schema, so this is the only test
    /// for the negative `current_len` assertion; there is nothing to fold it into.
    ///
    /// # Why `#[cfg(not(debug_assertions))]`
    /// In debug builds, `push_event`'s `debug_assert!` fires at the (effective) cap,
    /// halting execution before the auto-trim and warn paths are reached.  Running this
    /// test in debug mode would require bypassing that assert, which would undermine the
    /// existing debug-mode safety net for "engine never drains" (see the option-a
    /// discussion in the code-review comments for task 2520).  This test therefore covers
    /// the release-mode path, exercised by `cargo test -p reify-eval --release` — the CI
    /// lane mandated by the orchestrator's verify pipeline for all release-gated tests.
    #[test]
    #[cfg(not(debug_assertions))]
    fn auto_trim_warn_omits_invariant_current_len_field() {
        use reify_test_support::warn_capturing_subscriber;

        // Use a tiny cap so only 17 donations (instead of 65 537) are needed to
        // trigger one auto-trim round.  The warn field schema is identical regardless
        // of which cap value fires the trim.
        const TEST_CAP: usize = 16;

        let (subscriber, capture) = warn_capturing_subscriber();

        tracing::subscriber::with_default(subscriber, || {
            let mut pool = WarmStatePool::with_test_events_cap(TEST_CAP);

            // Donate TEST_CAP+1 events: the (TEST_CAP+1)-th push takes events.len()
            // to TEST_CAP+1 > TEST_CAP, firing exactly one auto-trim round and one warn.
            for i in 0..=TEST_CAP {
                let node =
                    NodeId::Value(reify_types::ValueCellId::new("T", format!("n{i}")));
                pool.donate(node, OpaqueState::new(0u8, 1));
            }
        });

        // Exactly one warn must have fired (the auto-trim warn).
        capture.assert_count(1);

        let all_fields = capture.fields_by_event();
        let event_fields = &all_fields[0];

        // Positive assertions: pin that this is the auto-trim warn (not a stray warn).
        assert!(
            event_fields.contains_key("cap"),
            "auto-trim warn must include `cap` field; got fields: {event_fields:?}"
        );
        assert!(
            event_fields.contains_key("total_dropped"),
            "auto-trim warn must include `total_dropped` field; got fields: {event_fields:?}"
        );

        // Negative assertion: the misleading invariant field must be absent.
        assert!(
            !event_fields.contains_key("current_len"),
            "`current_len` must NOT appear in the auto-trim warn (it is always \
             cap+1 before the drain — a constant derivable from `cap`, zero diagnostic \
             signal); got fields: {event_fields:?}"
        );
    }

    // --- Task 2516 step-1: donate_preserving_lru / checkout_with_lru_stamp ---
    //
    // These tests pin the LRU-preserving round-trip API added in response to
    // reviewer suggestion S1 (see task 2516 analysis).  The (4c)→(14b) cache-miss
    // path in engine_edit.rs previously called `pool.donate(nid, state)` which
    // refreshed `last_accessed` to `Instant::now()`, inadvertently making a
    // round-tripped entry look "recently accessed" and thus less likely to be
    // evicted than genuinely-old entries already in the pool.

    /// `donate_preserving_lru` must re-insert the entry using the *provided*
    /// `last_accessed` Instant, not a fresh `Instant::now()`.
    ///
    /// Setup: budget=250; donate A then B (with a sleep between to guarantee
    /// A's timestamp is strictly older than B's).  Round-trip A through
    /// `checkout_with_lru_stamp` + `donate_preserving_lru`.  Donating C (100
    /// bytes) forces exactly one eviction (200+100=300 > 250).  Because A's
    /// preserved stamp is older than B's, A must be the LRU victim — not B.
    ///
    /// If `donate_preserving_lru` incorrectly called `Instant::now()`, A's stamp
    /// would become newer than B's (set during the sleep window), causing B to be
    /// evicted instead, and the assertion `pool.checkout(&node_a).is_none()` would
    /// fail.
    #[test]
    fn donate_preserving_lru_does_not_refresh_access_time() {
        use std::time::Duration;

        let mut pool = WarmStatePool::new(250);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));
        let node_c = NodeId::Value(ValueCellId::new("T", "c"));

        pool.donate(node_a.clone(), OpaqueState::new(1i32, 100));
        // Sleep to ensure A's donation timestamp is strictly older than B's.
        // Without the sleep, two back-to-back Instant::now() calls may be equal
        // on coarse-grained clocks (Windows Instant granularity can be ~15 ms;
        // frequency-scaling CI runners can coalesce consecutive reads too),
        // making the eviction order non-deterministic.  15 ms is safely above
        // the worst-case platform resolution while keeping the test fast.
        std::thread::sleep(Duration::from_millis(15));
        pool.donate(node_b.clone(), OpaqueState::new(2i32, 100));
        // used = 200, budget = 250.

        // Round-trip A: checkout preserving the stamp, then re-donate preserving it.
        let (a_state, a_stamp) = pool
            .checkout_with_lru_stamp(&node_a)
            .expect("A must be in the pool before round-trip");
        // A is now removed from pool; used = 100.
        pool.donate_preserving_lru(node_a.clone(), a_state, a_stamp);
        // A is back in pool with its *original* (older) stamp; used = 200.

        // Donate C: 200+100=300 > 250 → exactly one eviction needed.
        // A has the oldest stamp (preserved from before the sleep), so A is the
        // LRU victim.
        pool.donate(node_c.clone(), OpaqueState::new(3i32, 100));

        assert!(
            pool.checkout(&node_a).is_none(),
            "A must be evicted: its preserved (original) stamp is older than B's; \
             if donate_preserving_lru called Instant::now() instead, B would be \
             evicted here and this assertion would succeed on the wrong entry"
        );
        assert!(
            pool.checkout(&node_b).is_some(),
            "B must remain: its stamp is newer than A's preserved stamp"
        );
        assert!(
            pool.checkout(&node_c).is_some(),
            "C (just donated) must remain in the pool"
        );
    }

    /// `checkout_with_lru_stamp` returns both the `OpaqueState` and an `Instant`
    /// that was captured at donation time (bounded between `before` and `after`
    /// the donate call).
    ///
    /// Pins the return-type contract for the new method: the caller receives
    /// `(OpaqueState, Instant)` so it can later pass the stamp to
    /// `donate_preserving_lru` without losing the original LRU ordering.
    #[test]
    fn checkout_with_lru_stamp_returns_state_and_original_instant() {
        let mut pool = WarmStatePool::new(1024);
        let node_x = NodeId::Value(ValueCellId::new("T", "x"));

        let before = std::time::Instant::now();
        pool.donate(node_x.clone(), OpaqueState::new(42i32, 8));
        let after = std::time::Instant::now();

        let (state, stamp) = pool
            .checkout_with_lru_stamp(&node_x)
            .expect("X must be in the pool after donate");

        assert_eq!(
            state.downcast::<i32>(),
            Some(42),
            "checkout_with_lru_stamp must return the donated state"
        );
        assert!(
            stamp >= before,
            "last_accessed stamp must be >= the Instant captured before donate; \
             stamp = {:?}, before = {:?}",
            stamp,
            before
        );
        assert!(
            stamp <= after,
            "last_accessed stamp must be <= the Instant captured after donate; \
             stamp = {:?}, after = {:?}",
            stamp,
            after
        );
        // Entry must have been consumed (take semantics).
        assert!(
            pool.checkout(&node_x).is_none(),
            "checkout_with_lru_stamp must have take-semantics: second call returns None"
        );
    }

    // --- Task 2456: emit Evicted on same-key overwrite ---

    /// All three donate variants emit overwrite-Evicted symmetrically via `insert_entry`.
    ///
    /// (a) `donate_with_cost`: donate X(100, cost=0.5), drain, donate X(200, cost=1.5) —
    ///     assert `[Evicted{X,100}, Donated{X,200}]`.
    ///
    /// (b) `donate_preserving_lru`: donate Y(50), drain, call
    ///     `donate_preserving_lru(Y, state2, some_stamp)` directly (without checkout)
    ///     to trigger the overwrite path — assert `[Evicted{Y,50}, Donated{Y,80}]`.
    ///
    /// Guards the design decision "localize the change to insert_entry" — all three
    /// donate variants funnel through the shared core so the overwrite-Evicted fires
    /// symmetrically regardless of which public API is used.
    #[test]
    fn donate_with_cost_and_donate_preserving_lru_also_emit_overwrite_evicted() {
        // (a) donate_with_cost overwrite
        {
            let mut pool = WarmStatePool::new(1024);
            let node_x = NodeId::Value(ValueCellId::new("T", "x"));

            pool.donate_with_cost(node_x.clone(), OpaqueState::new(0u8, 100), 0.5);
            pool.drain_events(); // clear setup

            pool.donate_with_cost(node_x.clone(), OpaqueState::new(0u8, 200), 1.5);
            let events = pool.drain_events();

            assert_eq!(
                events,
                vec![
                    WarmPoolEvent::Evicted {
                        node_id: node_x.clone(),
                        size_bytes: 100,
                    },
                    WarmPoolEvent::Donated {
                        node_id: node_x,
                        size_bytes: 200,
                    },
                ],
                "donate_with_cost same-key overwrite must emit Evicted then Donated"
            );
        }

        // (b) donate_preserving_lru overwrite (direct call without prior checkout)
        {
            let mut pool = WarmStatePool::new(1024);
            let node_y = NodeId::Value(ValueCellId::new("T", "y"));
            let stamp = std::time::Instant::now();

            pool.donate(node_y.clone(), OpaqueState::new(0u8, 50));
            pool.drain_events(); // clear setup

            // Call donate_preserving_lru directly on an already-present key.
            pool.donate_preserving_lru(node_y.clone(), OpaqueState::new(0u8, 80), stamp);
            let events = pool.drain_events();

            assert_eq!(
                events,
                vec![
                    WarmPoolEvent::Evicted {
                        node_id: node_y.clone(),
                        size_bytes: 50,
                    },
                    WarmPoolEvent::Donated {
                        node_id: node_y,
                        size_bytes: 80,
                    },
                ],
                "donate_preserving_lru same-key overwrite must emit Evicted then Donated"
            );
        }
    }

    /// Same-key overwrite combined with LRU pressure: ordering invariant and accounting.
    ///
    /// Setup: budget=200, donate A(100) + B(100) → used=200 (at budget). Drain.
    /// Donate A again with size=200: the overwrite reclaims A's old 100 bytes
    /// (used drops to 100), then 100+200=300>200 → LRU loop evicts B (used drops to 0),
    /// then A(200) is inserted (used=200).
    ///
    /// Expected drained sequence (exact order):
    ///   [Evicted{A, 100} (overwrite), Evicted{B, 100} (LRU), Donated{A, 200}]
    ///
    /// This pins the design decision "overwrite-Evicted goes BEFORE the LRU loop"
    /// and verifies accounting balance after both kinds of Evicted fire in one insert.
    #[test]
    fn same_key_overwrite_with_lru_pressure_emits_overwrite_evicted_first() {
        let mut pool = WarmStatePool::new(200);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));
        let node_b = NodeId::Value(ValueCellId::new("T", "b"));

        pool.donate(node_a.clone(), OpaqueState::new(0u8, 100));
        pool.donate(node_b.clone(), OpaqueState::new(0u8, 100));
        let setup = pool.drain_events();
        assert_eq!(setup.len(), 2, "setup: 2 Donated events");
        assert_eq!(pool.used_bytes(), 200, "setup: used=200 at budget");

        // Donate A again with size=200: triggers overwrite-Evicted(A,100) then LRU-Evicted(B,100)
        pool.donate(node_a.clone(), OpaqueState::new(0u8, 200));
        let events = pool.drain_events();

        assert_eq!(
            events,
            vec![
                WarmPoolEvent::Evicted {
                    node_id: node_a.clone(),
                    size_bytes: 100,
                },
                WarmPoolEvent::Evicted {
                    node_id: node_b,
                    size_bytes: 100,
                },
                WarmPoolEvent::Donated {
                    node_id: node_a,
                    size_bytes: 200,
                },
            ],
            "overwrite-Evicted must precede LRU-Evicted, which must precede Donated"
        );
        assert_eq!(
            pool.used_bytes(),
            200,
            "used_bytes must equal new entry size after both kinds of Evicted"
        );
    }

    /// Same-key overwrite must emit `Evicted{old_size}` then `Donated{new_size}`.
    ///
    /// The overwrite-evicted event is required for byte-accounting consumers
    /// to maintain the invariant `Σ Donated.size − Σ Evicted.size = used_bytes`.
    ///
    /// Fails on the unpatched code because no Evicted event is emitted today.
    #[test]
    fn donate_same_node_emits_evicted_then_donated_on_overwrite() {
        let mut pool = WarmStatePool::new(1024);
        let node_x = NodeId::Value(ValueCellId::new("T", "x"));

        // First donation: setup.
        pool.donate(node_x.clone(), OpaqueState::new(0u8, 100));
        pool.drain_events(); // Clear the setup Donated event.

        // Second donation of the *same* node: should emit Evicted(old) then Donated(new).
        pool.donate(node_x.clone(), OpaqueState::new(0u8, 300));

        let events = pool.drain_events();
        assert_eq!(
            events,
            vec![
                WarmPoolEvent::Evicted {
                    node_id: node_x.clone(),
                    size_bytes: 100,
                },
                WarmPoolEvent::Donated {
                    node_id: node_x,
                    size_bytes: 300,
                },
            ],
            "same-key overwrite must emit Evicted{{old_size}} THEN Donated{{new_size}}"
        );
        assert_eq!(
            pool.used_bytes(),
            300,
            "used_bytes must reflect only the new entry after overwrite"
        );
    }

    /// FIXME(cost-weighted-lru): `donate_preserving_lru` currently resets `cost_per_byte`
    /// to `0.0`, silently discarding any cost recorded at the original donation site.
    ///
    /// This is intentional for the **current** pure-LRU eviction policy, where `cost_per_byte`
    /// is stored but not consulted during eviction.  The assertion below pins the known
    /// (limited) behaviour so a future cost-weighted-LRU activator cannot silently break it:
    /// if `donate_preserving_lru` is updated to preserve cost, this test will fail and the
    /// FIXME can be resolved by updating the assertion or removing the test altogether.
    ///
    /// When cost-weighted LRU is enabled, address the matching `FIXME` note in the
    /// `donate_preserving_lru` doc comment and decide whether to:
    /// (a) thread the original cost through `checkout_with_lru_stamp` + `donate_preserving_lru`,
    /// (b) leave the reset and document it as an intentional trade-off.
    #[test]
    fn donate_preserving_lru_resets_cost_to_zero_known_limitation() {
        let mut pool = WarmStatePool::new(1024);
        let node_a = NodeId::Value(ValueCellId::new("T", "a"));

        // Donate with a non-zero cost.
        pool.donate_with_cost(node_a.clone(), OpaqueState::new(1i32, 8), 1.5);
        assert_eq!(
            pool.cost_per_byte_of(&node_a),
            Some(1.5),
            "sanity: cost_per_byte must be recorded at donation time"
        );

        // Round-trip via checkout_with_lru_stamp + donate_preserving_lru.
        let (state, stamp) = pool
            .checkout_with_lru_stamp(&node_a)
            .expect("A must be in pool after donate_with_cost");
        pool.donate_preserving_lru(node_a.clone(), state, stamp);

        // KNOWN LIMITATION: cost_per_byte is reset to 0.0 after the round-trip.
        // Update this assertion (and donate_preserving_lru's signature) when
        // cost-weighted LRU is activated — see FIXME above.
        assert_eq!(
            pool.cost_per_byte_of(&node_a),
            Some(0.0),
            "KNOWN LIMITATION: donate_preserving_lru resets cost_per_byte to 0.0; \
             this is benign while eviction is pure-LRU but becomes a fairness issue \
             once cost-weighted LRU is enabled — see FIXME in donate_preserving_lru doc"
        );
    }
}
