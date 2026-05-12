# Audit: Warm-State Eviction Policy

**PRD path:** `docs/prds/warm-state-eviction.md`
**Auditor:** audit-warm-state-eviction
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 11

## Top concerns

- **The eviction comparator is pure-LRU; cost-per-byte (the PRD's primary ordering rule) is recorded but explicitly NOT consulted.** A test (`cost_per_byte_does_not_alter_lru_eviction_order`, warm_pool.rs:902) actively pins this drift. Task 2340 is marked `done` despite the PRD acceptance criterion "500 MB/1 s state is evicted first" being unsatisfiable in the current implementation. The PRD-required unit test does not exist.
- **Cold-compute-time measurement infrastructure is missing.** PRD §"Scope" item 2 says "compute-time estimate can be measured during compute_cold (wall-clock duration) and refreshed lazily" — there is no timing instrumentation, no production caller of `donate_with_cost`, and no path that converts wall-clock to `cost_per_byte`. All production donations go through `donate()` which hard-codes `cost_per_byte = 0.0`. Engine value/constraint/realization donation paths in `engine_edit.rs` never measure cost.
- **Telemetry events (`Evicted`/`Donated`) are buffered inside the pool but never drained in production.** `WarmStatePool.events` is plumbed and `EventKind::Evicted`/`Donated` exist on the journal, but `drain_events()` has zero non-test callers and there is no engine-side translator that emits `EvalEvent`s. The `WarmStatePool` doc comments themselves acknowledge this as "task 2345 follow-up" with a release-build auto-trim/warn-once safety net suggesting the wiring is expected but absent. No GUI debug-panel surface.
- **PRD-stated config surface is env-var only; no project-config-file plumbing.** PRD §"Acceptance criteria" allows "config-file or env-var override" — only `REIFY_WARM_STATE_BUDGET_BYTES` exists. `reify-config` crate has no `warm_state_budget` field; the env-var path is the only configurable surface.
- **PRD invokes a `WarmStartable` shape that doesn't match the code.** The arch §4.1 protocol (and PRD background) defines `compute_cold(inputs) -> (Result, State)` and `compute_warm(inputs, prev_state, diff) -> (Result, State)`. The actual trait (`reify_types::WarmStartable`) is `fn warm_state(&self) -> Option<OpaqueState>` + `fn with_warm_state(&mut self, state: OpaqueState)` — a save/restore shape, not a unified compute call. Whether this is DRIFT in the trait, or just informal-prose-vs-code, will affect Phase 3 framing of "what 'WarmStartable protocol' even means".

## Mechanisms

### M-001: `WarmStatePool` struct with `OpaqueState` entries

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/warm_pool.rs:54` (`WarmStatePool` struct); `crates/reify-types/src/warm.rs:9` (`OpaqueState`); task #27 done.
- **Blocks:** none
- **Note:** Pool struct exists with `pool: HashMap<NodeId, PoolEntry>`, `budget_bytes: Option<usize>`, `used_bytes: usize`; entries carry `OpaqueState`, `last_accessed`, `size_bytes`, `cost_per_byte`. Foundation per PRD §"Existing infrastructure".

### M-002: Configurable `memory_budget_bytes` at pool construction

- **State:** PARTIAL
- **Failure mode:** F3 (PRD-asserted surface narrower than code)
- **Evidence:** `crates/reify-eval/src/warm_pool.rs:54-205` — `with_budget(Option<usize>)`, `from_env_or_default()`, `from_env_value()`, `BUDGET_ENV_VAR = "REIFY_WARM_STATE_BUDGET_BYTES"`, `DEFAULT_BUDGET_BYTES = 2 GiB`; `crates/reify-eval/src/engine_admin.rs:160` wires `Engine::new` through `from_env_or_default`. **No** project-config-file (`<project>/.reify/config.toml`) field for warm-state budget; grep on `reify-config` crate returns zero matches for `warm` or `memory_budget`.
- **Blocks:** PRD §"Acceptance criteria" first bullet ("config-file or env-var override").
- **Note:** Env-var path satisfies one half of the "or"; project-config-file path is absent. Default = 2 GiB matches PRD's suggestion.

### M-003: Per-state metadata: `size_bytes`, `last_accessed: Instant`, `cost_per_byte: f64`

- **State:** WIRED (storage); PARTIAL (semantics — see M-004 for cost ordering and M-005 for cost provenance)
- **Failure mode:** N/A on storage
- **Evidence:** `crates/reify-eval/src/warm_pool.rs:37-47` (`PoolEntry` struct). All three fields present.
- **Blocks:** none on storage shape.
- **Note:** Field schema matches PRD §"Acceptance criteria" second bullet exactly. Cost ordering and cost provenance are separate mechanisms (M-004, M-005).

### M-004: Cost-per-byte LRU eviction comparator (`cost_per_byte` ascending, `last_accessed` tiebreak)

- **State:** DRIFT
- **Failure mode:** F2 (PRD/arch shape differs from landed code; intentional simplification pinned by test)
- **Evidence:** `crates/reify-eval/src/warm_pool.rs:364-380` (`evict_lru` uses `min_by_key(|(_, e)| e.last_accessed)` — pure LRU, ignores `cost_per_byte`); `crates/reify-eval/src/warm_pool.rs:44-46` doc-comment "Currently stored but not consulted by the eviction comparator (pure LRU). Reserved for the future cost-weighted-LRU eviction policy"; `crates/reify-eval/src/warm_pool.rs:902-936` test `cost_per_byte_does_not_alter_lru_eviction_order` actively pins the drift. Task 2340 marked `done`.
- **Blocks:** PRD §"Acceptance criteria" bullets 3 and 6 (the 500 MB/1 s ordering test). The canonical synthetic-pool ordering test from the PRD's task-1 breakdown does not exist anywhere in the test suite.
- **Note:** The most prominent gap. PRD's *primary* ordering rule is silently replaced by pure-LRU with an explicit FIXME(cost-weighted-lru) and a pinning test. Closing this requires both (a) flipping the comparator and (b) wiring cost provenance (M-005). Note also that `donate_preserving_lru` resets `cost_per_byte` to 0.0 (warm_pool.rs:280-287) — a known second-order limitation that will only matter once cost-weighted LRU activates.

### M-005: Wall-clock measurement during `compute_cold` and lazy refresh of cost estimate

- **State:** FICTION
- **Failure mode:** F1 (PRD asserts mechanism exists; code provides nothing)
- **Evidence:** No `Instant::now()` / `Duration` wrapping of any `compute_cold` invocation. `donate_with_cost` has zero production callers (warm_pool.rs:234 — only test callers at lines 873/896/914-923). Engine donation sites (`engine_edit.rs:670, 738, 822`) all call `donate()` / `donate_preserving_lru()` which hard-code `cost_per_byte = 0.0`. There is no compute-time-estimate registry, no per-WarmStartable cost trait method, and no lazy-refresh path.
- **Blocks:** M-004 (cost-weighted comparator); PRD §"Scope" item 2.
- **Note:** Even if M-004 flips to cost-weighted LRU, the input is always 0.0 in production until this is wired. Worth noting: there's no production `WarmStartable` *producer* feeding the pool today (M-013) — OCCT and FEA-elastic implement the trait but no engine path round-trips real state through the pool. So the measurement gap may be moot until producers wire up first.

### M-006: Eviction loop on insertion / return — drop entries by comparator until under budget

- **State:** PARTIAL
- **Failure mode:** F2 (loop exists but uses wrong comparator per M-004)
- **Evidence:** `crates/reify-eval/src/warm_pool.rs:331-335` — `while self.used_bytes + size > budget && !self.pool.is_empty() { self.evict_lru(); }` inside `insert_entry`. Triggered on `donate*` paths. Single-oversized-item path documented: kept even if over-budget (warm_pool.rs:670-679 test pins).
- **Blocks:** none for the structural loop; gated on M-004 for ordering correctness.
- **Note:** Loop runs on insertion. PRD §"Scope" item 3 also says "return-to-pool" should trigger eviction — `donate_preserving_lru` (which is the re-donate-on-return path, engine_edit.rs:670, 738) does invoke `insert_entry`, so this is satisfied. Eviction does NOT run on `checkout` (checkout reduces `used_bytes` so cannot put pool over budget).

### M-007: `pool.checkout()` returns `None` when state evicted; caller falls through to `compute_cold`

- **State:** PARTIAL
- **Failure mode:** F3 (mechanical None-return wired; the `compute_cold` fall-through is implicit and partially-untested)
- **Evidence:** `crates/reify-eval/src/warm_pool.rs:455-478` — `checkout`/`checkout_with_lru_stamp` return `None` on miss. Cache-side: `crates/reify-eval/tests/warm_state_donation.rs:222-286` `eviction_fallback_evicted_state_seeds_no_warm_state` pins that an evicted value-cell's cache `warm_state` is `None` after re-add. No production caller computes `compute_cold` — that name doesn't appear in the codebase (see M-014); so "fall through to cold compute" is best-read as "evaluator sees no warm seed and just evaluates the cell normally".
- **Blocks:** none structurally; PRD §"Acceptance criteria" 4th bullet has a documented test for value cells only. The same hook for Realization is a no-op per the test file's L300-310 caveats (engine_build creates Realization cache entries lazily). Resolution/Compute variants don't fire at all (no `diff_resolutions` helper; ComputeNode not yet a NodeId variant).
- **Note:** The contract works for value cells. PRD's "all warm-startable node types" coverage is currently Value-only on the round-trip; Realization is a smoke-test stub; Resolution/Compute are absent. PRD acceptance criterion "all warm-startable node types fall back correctly" is over-claimed relative to coverage.

### M-008: Topology-removal donation keyed by node type + path-based identity

- **State:** PARTIAL
- **Failure mode:** F3 (donates keyed by `NodeId`, which is path-based for Value but index-based for Realization/Constraint/Resolution)
- **Evidence:** `crates/reify-eval/src/engine_edit.rs:2275-2301` — donate hook fires for `removed`/`removed_constraints`/`removed_realizations` (Value/Constraint/Realization), invoking `donate_warm_state_and_invalidate` (engine_edit.rs:809-822). Key is the raw `NodeId`. `crates/reify-types/src/identity.rs:120-205`: `ValueCellId { entity, member }` is path-based (`Bracket.volume`); `RealizationNodeId/ConstraintNodeId/ResolutionNodeId { entity, index }` are *index-based*. Arch §4.3:540 + §6.4:654/660/664 specifically calls for "path-based identity" (where `assembly.bracket.thickness` round-trips through removal/re-add). Index-based identity for Realization is fragile under reorderings.
- **Blocks:** Reappearance reuse for non-Value variants (Realization/Constraint/Resolution) under any structural reordering. Resolution donation is absent entirely (no `diff_resolutions` per engine_edit.rs:2272-2274 comment).
- **Note:** This is a subtle DRIFT-leaning PARTIAL. Value variant matches arch intent. Realization variant works for stable indices but the PRD/arch's promise that "reappear with same path reuses state" doesn't hold under index reshuffles. ComputeNode (the v0.3 FEA shape) isn't yet a NodeId variant so its donation hook is FICTION (carries to M-013 PRD scope but flagged here for completeness).

### M-009: Donated state counts against budget and is subject to the same eviction policy

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/warm_pool.rs:294-352` `insert_entry` is the single donation core; the eviction loop runs on ALL donation paths regardless of origin (topology-removal donate, re-donate-on-return, fresh insert). `used_bytes` accumulates uniformly.
- **Blocks:** none.
- **Note:** Structurally clean: one insertion path, one eviction loop. Per PRD §"Scope" item 5.

### M-010: Realization-event hooks — `EventKind::evicted` and `EventKind::donated` fire on the appropriate transitions

- **State:** PARTIAL
- **Failure mode:** F4 (event vocabulary exists; emission/translation layer absent)
- **Evidence:** `crates/reify-eval/src/journal.rs:48-63` `EventKind::Evicted { size_bytes }` and `EventKind::Donated { size_bytes }` exist with translation-contract doc comments. `crates/reify-eval/src/warm_pool.rs:319-351` emits `WarmPoolEvent::Donated/Evicted` into a buffered `events: Vec<WarmPoolEvent>` (NOT the journal). `drain_events()` has ZERO non-test callers in the reify crates (verified via grep — only test calls). No engine-side translator exists; `WarmStatePool` doc comments themselves call this out ("task 2345 follow-up: verify engine wires `drain_events()` at every eval boundary"). Release-build buffer overflow auto-trim + once-per-pool warn-tracing acts as a tripwire for the missing wiring.
- **Blocks:** PRD §"Scope" item 6 (telemetry on diagnostic journal); PRD §"Acceptance criteria" bullet "Realization events `EventKind::evicted` and `EventKind::donated` fire on the appropriate transitions" (they fire to a buffer; they don't reach the journal).
- **Note:** Two halves are independently complete but the seam between them is missing. Once a translator lands, the existing test coverage of the buffered events should suffice. The release-mode warn-once + `dropped_events` counter (warm_pool.rs:80-88) is the only signal Leo would see today if the buffer fills.

### M-011: GUI debug-panel surface for eviction-pressure counts

- **State:** FICTION
- **Failure mode:** F1 (PRD asserts surface for observability; nothing exists)
- **Evidence:** Grep on `gui/src` for `warm.*pool`, `warm_pool`, `warm.*state`, `eviction`, `Evicted`, `Donated`: zero matches outside `node_modules`. The engine never drains events to the journal (M-010), and even if it did, no UI consumer exists.
- **Blocks:** PRD §"Scope" item 6 / Task-5 "Surface counts in the diagnostic journal / GUI debug panel for observability".
- **Note:** End-to-end visibility for eviction pressure does not exist. `EventJournal.count_donated()` / `count_evicted()` accessors do exist on the journal (journal.rs:161-168) so the consumer side is half-built.

### M-012: End-to-end smoke test against `examples/` with small budget (e.g. 50 MB), edit-loop eviction without functional regression

- **State:** TODO
- **Failure mode:** F5 (acceptance test absent; PRD-task 6 declared "(Optional)" so may have been deliberately skipped)
- **Evidence:** No file under `crates/reify-eval/tests/` matches `smoke.*budget` / `examples.*evict` / `small_budget` / `50MB`. Existing integration tests (`warm_state_donation.rs`) use synthetic NodeIds, not real `examples/` modules, and inject `OpaqueState` manually rather than driving a producer end-to-end.
- **Blocks:** PRD §"Acceptance criteria" final bullet "`examples/` smoke-test with a small budget exercises eviction without functional regression".
- **Note:** PRD task-6 is marked "(Optional)". Phase 3 should decide whether this counts as a gap or accept-and-document.

### M-013: Production-path WarmStartable producer that round-trips real state through the pool

- **State:** PARTIAL
- **Failure mode:** F3 (trait implementors exist; the engine→producer→pool→consumer→engine cycle is not wired for any real node kind)
- **Evidence:** `crates/reify-kernel-occt/src/handle.rs:1178-1199`, `crates/reify-kernel-occt/src/lib.rs:2515` implement `WarmStartable for OcctKernel/OcctKernelHandle`; `crates/reify-solver-elastic/src/warm_state.rs:99-113` defines `solve_cg_with_warm_state` producer. But: zero production call sites invoke `engine.warm_pool_mut().donate(...)` from inside any evaluation/realization/solve path. All non-test donations are topology-removal hand-offs (engine_edit.rs:822) — none arise from a producer completing a compute. So warm state can be donated when a node is *removed*, but no producer ever generates it. Tests inject `OpaqueState` manually via `cache_store_mut().donate_warm_state(...)` to drive the donation pathway.
- **Blocks:** real-world utility of the eviction policy (until a producer fills the pool, eviction has nothing to evict in practice).
- **Note:** This is the most architecturally interesting gap — the eviction policy is fully built on top of an empty pool. PRD §"Background" calls task #27 the foundation ("does not implement eviction") but the post-#27 producer-wires-into-pool step is itself missing. Cross-PRD breadcrumb: this is the producer-half of the same gap the compute-node-infrastructure PRD partially addresses for FEA.

### M-014: PRD-vs-code shape mismatch on the `WarmStartable` protocol surface

- **State:** DRIFT
- **Failure mode:** F2 (PRD/arch invokes a protocol shape that doesn't match the trait)
- **Evidence:** PRD §"Background" first bullet and arch §4.1 lines 500-504 specify `fn compute_cold(inputs) -> (Result, State)` and `fn compute_warm(inputs, prev_state, diff) -> (Result, State)`. Actual `crates/reify-types/src/warm.rs:58-66` trait is `fn warm_state(&self) -> Option<OpaqueState>` + `fn with_warm_state(&mut self, state: OpaqueState)`. Grep on `compute_warm` / `compute_cold` across the codebase: zero matches. The trait shape that landed is "save/restore around an externally-driven compute call", not "two compute-call variants with explicit input-diff propagation". This affects PRD `pool.checkout()` semantics: there is no `compute_cold` to "fall back to" — the consumer simply doesn't get a warm seed and the existing evaluator continues.
- **Blocks:** the conceptual framing of the entire PRD §"Background" + §"Scope" item 4. The wiring is functionally OK because both shapes accomplish "warm-state may be absent → cheap fallback"; the divergence is in vocabulary and in input-diff threading (which the arch's `compute_warm` signature carried, and the landed trait does not).
- **Note:** Could be deferred as "informal-prose-vs-code" rather than DRIFT. Phase 3 may prefer to fix the arch doc to match the trait, or keep both shapes (the arch's `compute_warm(inputs, prev_state, diff)` becomes the *outer* engine call that internally uses `with_warm_state` + the cell's own eval expression). Flagging for visibility.

## Cross-PRD breadcrumbs

- **compute-node-infrastructure PRD** — Owns ComputeNode addition to NodeId. M-008's `NodeId::Compute` donation hook + M-007's checkout-for-ComputeNode fall-back are gated on that PRD landing.
- **structural-analysis-fea PRD** — Task #14 produces `CgWarmState`; task #16 was supposed to wire it through to the pool via `@optimized`. M-005 (wall-clock measurement) and M-013 (producer→pool round-trip) are the structural pre-reqs for FEA to actually benefit from this PRD. The audit-brief notes 2924 (FEA #16) is the integration task.
- **multi-load-case-fea PRD / persistent-fea-cache PRD** — Both assume FEA warm state survives across edits. Until M-013 is wired, no warm state actually reaches the pool from a producer; until M-004 flips to cost-weighted, large-cheap states won't preferentially evict before small-expensive FEA states. Either PRD assuming "expensive FEA solves stay warm" may surprise.
- **persistent-naming-v2 PRD** — Owns path-based identity for non-Value variants. M-008's partial state (index-based Realization/Constraint/Resolution NodeIds) is upstream-blocked here.
