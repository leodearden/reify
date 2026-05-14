# Warm-State Eviction Policy

## Goal

Implement a configurable, memory-budgeted, cost-per-byte LRU eviction policy for the `WarmStatePool` so that long sessions do not exhaust memory through unbounded warm-state retention. Evicted state is semantically transparent: nodes whose warm state is gone fall back to `compute_cold` per the `WarmStartable` protocol (arch §4.1, lines 495–506). Topology updates that remove a node donate its warm state to a path-keyed pool entry for potential reuse on reappearance (arch §4.3, line 540).

## Background

- **Arch §4.1 (lines 495–506)** defines the `WarmStartable` protocol: `compute_cold(inputs) -> (Result, State)` and `compute_warm(inputs, prev_state, diff) -> (Result, State)`. Warm state is **NOT** content-addressed — performance hint only, fall back to cold if absent.
- **Arch §4.2 (lines 508–518)** describes input-diff-driven warm starts. v0.1 uses operation replay (previous `TopoDS_Shape` seeds the next compute); feature-level incrementality is deferred.
- **Arch §4.3 (lines 520–542)** is the key section. Specifies:
  - `NodeCache` carries `warm_state: Option<OpaqueState>`.
  - Pools support checkout/return; size 1 mutex generalises to size N.
  - **Memory budget**: configurable project-level cap.
  - **Eviction order**: LRU weighted by `estimated_cold_compute_time / state_size_bytes` (cost-per-byte); among equal-cost-per-byte states, LRU recency tiebreaks.
  - **Eviction mechanism**: `pool.checkout()` returns `None` when evicted; node falls back to `compute_cold`. No new machinery needed.
  - **Donated state**: topology removal donates warm state keyed by node type and path-based identity. Donated state counts against budget and is subject to the same eviction policy.
- **Arch §4.4 (lines 544–552)** scopes v0.1 to **Tier 1 only**: same node, previous result. Tier 2 (closest parameter set, multi-state per node) and Tier 3 (type-level pool across all instances) are deferred but the protocol must not preclude them.
- **Existing infrastructure:** task #27 landed the `WarmStartable` trait and `WarmStatePool` (in `reify-types`/`reify-eval`/`reify-runtime`). It does not implement eviction. OCCT `TopoDS_Shape` for moderately complex parts is 100s of MB — eviction is not optional for sustained sessions on large designs.
- **Why now:** v0.1 is OCCT-only (architectural decision), and the example designs growing in `examples/` will hit this limit during sustained editing sessions.

## Scope

1. Add a configurable memory budget to `WarmStatePool` (project-level config, defaulting to a sensible cap like 2 GB or N% of system RAM).
2. Track per-state metadata: byte size, last-access timestamp, estimated cold-compute time. The compute-time estimate can be measured during `compute_cold` (wall-clock duration) and refreshed lazily.
3. Implement cost-per-byte LRU eviction: when total warm-state usage exceeds the budget, evict states in increasing order of `cost_per_byte`, with LRU as the tiebreaker among equal cost-per-byte values.
4. Wire eviction into `pool.checkout()` so it returns `None` when the requested state has been evicted. Confirm that all warm-startable node types fall back to `compute_cold` correctly (this is already the protocol contract; verify with tests).
5. Wire donated state on topology removal (arch §6.4) into the pool, keyed by node type + path-based identity. Donated state is subject to the same budget and eviction rules.
6. Telemetry: realization-event hooks for `evicted` and `donated` so the diagnostic journal can show eviction pressure (helps debug performance regressions).
7. Tests: unit tests for the cost-per-byte ordering; integration tests that force eviction by setting a small budget and observe correct fallback to cold compute; donation+reuse round-trip test.

## Out of scope

- Tier 2 / Tier 3 multi-state pools (arch §4.4) — protocol must not preclude them, but no implementation in v0.1.
- Feature-level incrementality (arch §4.2) — operation replay is the v0.1 strategy.
- Persistent on-disk warm-state caches across process restarts — in-memory only.
- Automatic budget tuning based on system memory pressure — accept a static configured value.
- Cross-process or remote-worker warm-state sharing.

## Acceptance criteria

- `WarmStatePool` has a configurable `memory_budget_bytes` setting, with a documented default and a config-file or env-var override.
- Each pooled state carries `size_bytes`, `last_accessed: Instant`, and `cost_per_byte: f64` (or equivalent).
- When a return-to-pool or insertion would push aggregate usage above the budget, the pool evicts states in cost-per-byte ascending order until aggregate usage is back under the budget. Among equal cost-per-byte, LRU recency tiebreaks.
- `pool.checkout(node_id)` returns `None` when the state has been evicted. The recursive evaluator (already calling `compute_warm` vs `compute_cold` based on the option) handles this transparently.
- Topology-removal donation (arch §6.4 cancellation path, §4.3 line 540): when a node is removed, its warm state is deposited keyed by node type + path-based identity. A subsequent reappearance with the same key reuses the donated state. Donated state counts against the budget.
- Cost-per-byte ordering test: build a synthetic pool with 3 states {500MB/1s, 10MB/30s, 100MB/5s}, set budget to 200MB, force eviction. Assert the 500MB/1s state is evicted first.
- Fallback test: force-evict a real warm-startable node's state, re-evaluate, assert `compute_cold` was called and the result is identical to a non-evicted run.
- Donate-and-reuse test: remove a node via topology update, reappear it with same path, assert warm state was reused (e.g. via a counter on `compute_cold` calls).
- Realization events `EventKind::evicted` and `EventKind::donated` fire on the appropriate transitions.
- `cargo test -p reify-eval -p reify-runtime` green; `examples/` smoke-test with a small budget exercises eviction without functional regression.

## Task breakdown (queueing aim: 4–6 tasks)

1. **Add memory-budget configuration and per-state metadata** to `WarmStatePool` (`size_bytes`, `last_accessed`, `cost_per_byte`). Plumb a configurable budget from project config / env var. Land as no-op if budget is `None` / unlimited; existing tests stay green.
2. **Implement cost-per-byte LRU eviction**: ordered eviction loop triggered on insertion / return when aggregate exceeds budget. Unit tests for the ordering with synthetic states.
3. **Wire eviction into `pool.checkout()`** so it returns `None` on miss; confirm the evaluator's existing `Option<State>` handling falls back to `compute_cold`. Add a force-eviction integration test that asserts identical results between warm-fallback and cold-only runs.
4. **Wire topology-removal donation** (arch §6.4 / §4.3): on node removal, donate warm state keyed by node type + path-based identity. Add a remove-then-reappear integration test asserting reuse.
5. **Realization-event telemetry** for `evicted` and `donated`. Surface counts in the diagnostic journal / GUI debug panel for observability.
6. **(Optional)** End-to-end smoke test: open a moderately complex example with a small budget (50MB), edit-loop a parameter, confirm eviction fires, results stay correct, and overall memory stays under budget. May fold into task 3 if scope is small.

## Dependencies

- Builds on #27 (existing `WarmStatePool`).
- The realization-event hook (task 5) is independent of the freshness/event-journal PRD machinery; it can use the existing event-emission surface.

## Relationship to other PRDs and tasks

- **Backend event channel seam-owned by `docs/prds/v0_3/gui-event-channel-inventory.md`** — the `warm-pool-event` channel (consumed by `WarmPoolDebugPanel` in `gui/src/debug/`) is inventoried in inventory §2.2 Phase 3 task ε, with this PRD's M-010 (`drain_events` wiring from `WarmStatePool` to the eval-boundary journal translator) as the upstream data source. Emitter wiring is decomposed in the inventory PRD. See also `docs/gui-event-channels.md`.
