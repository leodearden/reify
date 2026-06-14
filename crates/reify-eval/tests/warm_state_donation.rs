//! Integration tests for warm-state donation/checkout across topology edits.
//!
//! Per arch §4.3 lines 539-540 and §6.4 lines 654-660:
//! - **Donation:** when `edit_source` removes a node from the topology, the
//!   engine donates that node's `cache.warm_state` (if any) to its
//!   `WarmStatePool` keyed by the node's identity.
//! - **Checkout:** when a later `edit_source` re-adds the same `NodeId`, the
//!   engine checks out the donated state and seeds the new cache entry's
//!   `warm_state` slot. A `None` checkout (because the entry was LRU-evicted)
//!   means evaluation falls through the cold-compute path with no seeded
//!   warm state — observable equivalence to a from-scratch baseline.
//!
//! ## Variant coverage scope
//!
//! The donation/checkout machinery is variant-agnostic — `WarmStatePool` is
//! keyed by `NodeId`, which currently has variants `Value`, `Constraint`,
//! `Realization`, `Resolution`. Coverage as of this task:
//!
//! - **Value cells:** full round-trip (donate → pool → checkout → seed
//!   `cache.warm_state`) — covered by tests in this file.
//! - **Realization nodes:** donation hook coverage (variant-symmetry smoke
//!   test). Realization cache entries are not created by `edit_source` —
//!   `engine_build.rs` routes realization geometry ops directly to the
//!   kernel without the cache — so the post-add seed step is a no-op for
//!   realizations today. The donation hook still fires for completeness;
//!   when realizations gain cache entries, the seed step picks them up
//!   automatically with no further changes.
//! - **Resolution nodes:** not in any `diff_*` helper yet (no
//!   `diff_resolutions`); donation/checkout does not fire for them. The
//!   pool API itself is variant-agnostic (verified at unit-test level).
//! - **ComputeNode:** not yet a `NodeId` variant per arch §3.4; coverage
//!   attaches automatically when the variant is introduced (no code change
//!   needed in the donation/checkout path).

use reify_constraints::SimpleConstraintChecker;
use reify_core::{ComputeNodeId, RealizationNodeId, Severity, ValueCellId};
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_eval::warm_pool::WarmStatePool;
use reify_ir::{OpaqueState, Value};
use reify_test_support::{bracket_compiled_module, make_simple_engine, parse_and_compile,
    parse_and_compile_with_stdlib};

/// Build a fresh Engine (no prior eval) backed by the real constraint checker.
fn fresh_engine() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None)
}

/// Bracket source with a single configurable let so tests can drop just the
/// let between module_a and module_b without touching params or constraints.
fn bracket_with_volume_let() -> &'static str {
    r#"structure Bracket {
    param width: Length = 80mm
    param height: Length = 100mm
    param thickness: Length = 5mm

    let volume = width * height * thickness

    constraint thickness > 2mm
}"#
}

/// Bracket source with the `volume` let stripped out — used as module_b in
/// the removal scenarios. Same params and constraint as `bracket_with_volume_let`.
fn bracket_without_volume_let() -> &'static str {
    r#"structure Bracket {
    param width: Length = 80mm
    param height: Length = 100mm
    param thickness: Length = 5mm

    constraint thickness > 2mm
}"#
}

// ── Step-5: edit_source donates warm state for a removed value cell ────────

/// When `edit_source` removes a value cell whose cache entry has warm state,
/// the engine must donate that state to its `WarmStatePool` keyed by the
/// cell's `NodeId` BEFORE invalidating the cache entry. This test pins the
/// donation hook on the value-cell removal path (engine_edit.rs ~L1483).
///
/// Test driver: simulate a future WarmStartable producer by injecting warm
/// state into the cache via `cache_store_mut().donate_warm_state(...)` after
/// `eval()` populates the cell's cache entry. Then call `edit_source` with a
/// module that drops the cell. Assert: the pool now contains the state for
/// the removed `NodeId` and a `checkout` returns the original payload.
#[test]
fn edit_source_donates_warm_state_for_removed_value_cell() {
    let mut engine = fresh_engine();

    // (1) Eval the canonical bracket — this populates the cache for `volume`.
    let module_a = parse_and_compile(bracket_with_volume_let());
    engine.eval(&module_a);

    let volume_id = ValueCellId::new("Bracket", "volume");
    let volume_node = NodeId::Value(volume_id.clone());

    // (2) Inject warm state into the cache for `volume` (simulates the future
    //     producer's output). 16 bytes is well under the default 2 GiB budget.
    let donated = engine
        .cache_store_mut()
        .donate_warm_state(&volume_node, OpaqueState::new(0xDEADBEEFu32, 16));
    assert!(
        donated,
        "donate_warm_state must succeed — `volume` cache entry must exist after eval"
    );

    // (3) Sanity: pool is empty before the edit.
    assert_eq!(
        engine.warm_pool().used_bytes(),
        0,
        "warm_pool must start empty before any donation"
    );

    // (4) edit_source to a module without `volume` — drives `volume` into the
    //     `removed` set of `diff_value_cells`.
    let module_b = parse_and_compile(bracket_without_volume_let());
    engine
        .edit_source(&module_b)
        .expect("edit_source must succeed after eval");

    // (5) The pool must now hold the donated state for the removed cell.
    assert!(
        engine.warm_pool().used_bytes() >= 16,
        "warm_pool must hold ≥16 bytes after edit_source removes a cell with warm state; \
         got used_bytes = {}",
        engine.warm_pool().used_bytes()
    );

    // (6) checkout(volume) returns the originally-donated payload (downcast equality).
    let checked_out = engine.warm_pool_mut().checkout(&volume_node);
    let state = checked_out
        .expect("warm_pool.checkout must return Some for the donated cell after edit_source");
    assert_eq!(
        state.downcast::<u32>(),
        Some(0xDEADBEEFu32),
        "checked-out OpaqueState must downcast to the originally-injected u32 payload"
    );
}

// ── Step-7: donation reuse — remove-then-reappear seeds cache.warm_state ──

/// After `edit_source` removes a cell whose cache had warm state (donated to
/// the pool), a subsequent `edit_source` that re-adds the same `NodeId`
/// (path-based identity) must seed the new cache entry's `warm_state` slot
/// from the pool. Verifies the full donate → pool → checkout → seed
/// round-trip wired through the canonical edit_source path. Test fails until
/// step-8 wires the checkout-and-seed phase into edit_source's add path.
#[test]
fn donation_reuse_remove_then_reappear_seeds_cache_warm_state() {
    let mut engine = fresh_engine();

    // (1) Eval module_a (with `volume`).
    let module_a = parse_and_compile(bracket_with_volume_let());
    engine.eval(&module_a);

    let volume_id = ValueCellId::new("Bracket", "volume");
    let volume_node = NodeId::Value(volume_id.clone());

    // (2) Inject warm state into `volume`'s cache entry.
    let donated = engine
        .cache_store_mut()
        .donate_warm_state(&volume_node, OpaqueState::new(0xDEADBEEFu32, 16));
    assert!(donated, "donate_warm_state on cached volume must succeed");

    // (3) edit_source #1: drop `volume` (donation fires per step-6).
    let module_b = parse_and_compile(bracket_without_volume_let());
    engine
        .edit_source(&module_b)
        .expect("first edit_source must succeed");

    // Sanity: pool now holds the state.
    assert!(
        engine.warm_pool().used_bytes() >= 16,
        "post-removal pool must hold ≥16 bytes"
    );

    // (4) edit_source #2: re-add `volume` (same path-based NodeId).
    let module_c = parse_and_compile(bracket_with_volume_let());
    engine
        .edit_source(&module_c)
        .expect("second edit_source must succeed");

    // (5) The new cache entry's `warm_state` slot must contain the donated payload.
    let cache = engine.cache_store();
    let entry = cache.get(&volume_node).expect(
        "after edit_source re-adds `volume`, its cache entry must exist (eval populated it)",
    );
    let warm = entry
        .warm_state
        .as_ref()
        .expect("post-checkout-and-seed cache entry must carry the donated warm_state");
    assert_eq!(
        warm.downcast_ref::<u32>().copied(),
        Some(0xDEADBEEFu32),
        "seeded warm_state must downcast to the originally-injected u32 payload"
    );
}

// ── Step-9: eviction fallback — checkout None ⇒ no seed ────────────────────

/// When the `WarmStatePool` LRU-evicts a previously-donated entry under
/// memory pressure, a subsequent `edit_source` that re-adds the same
/// `NodeId` must produce a cache entry whose `warm_state` slot is `None`
/// (no seed — the checkout returned None, so nothing was seeded). This pins
/// the "checkout None ⇒ no seed" half of the eviction contract per arch §4.3
/// lines 539-540.
///
/// The cold-eval parity half (`post_evict_value == cold_value`) is
/// intentionally deferred: no producer in the codebase consumes
/// `cache.warm_state` to alter its output today, so an
/// `assert_eq!(post_evict_value, &cold_value)` would be trivially satisfied
/// regardless of whether the seed pipeline is broken. That assertion becomes
/// load-bearing only after a real warm-state consumer lands; tracked as
/// Task 2518 (add warm-state parity assertion once a real consumer exists).
///
/// Eviction mechanics: a 50-byte budget pool. Donating a 32-byte entry
/// for `volume` fills 32/50. A subsequent 100-byte unrelated donation
/// triggers the LRU loop in `donate_with_cost`, which evicts every
/// existing entry (including `volume`) before inserting the over-budget
/// new entry — a single item exceeding the entire budget is allowed by
/// design (see `WarmStatePool::donate_with_cost` doc).
#[test]
fn eviction_fallback_evicted_state_seeds_no_warm_state() {
    let volume_id = ValueCellId::new("Bracket", "volume");
    let volume_node = NodeId::Value(volume_id.clone());

    // (1) Eviction scenario: engine with a tiny-budget pool.
    let mut engine = fresh_engine();
    *engine.warm_pool_mut() = WarmStatePool::new(50);

    let module_a = parse_and_compile(bracket_with_volume_let());
    engine.eval(&module_a);

    // Inject 32-byte warm state for `volume` (fits the 50-byte budget).
    let donated = engine
        .cache_store_mut()
        .donate_warm_state(&volume_node, OpaqueState::new(0xCAFEu32, 32));
    assert!(donated, "donate_warm_state on cached `volume` must succeed");

    // edit_source #1: drop `volume`. Donation hook (step-6) fires: the
    // 32-byte state is moved from the cache into the pool.
    let module_b = parse_and_compile(bracket_without_volume_let());
    engine
        .edit_source(&module_b)
        .expect("first edit_source must succeed");
    assert!(
        engine.warm_pool().used_bytes() >= 32,
        "post-removal pool must hold the 32-byte donated state; got {} bytes",
        engine.warm_pool().used_bytes()
    );

    // Force LRU eviction: directly donate an unrelated 100-byte entry. The
    // pool's eviction loop (`donate_with_cost`) drains every existing entry
    // (just `volume` here) before inserting the over-budget new entry.
    let evictor_id = ValueCellId::new("Bracket", "evictor_filler_for_lru_test");
    let evictor_node = NodeId::Value(evictor_id);
    engine
        .warm_pool_mut()
        .donate(evictor_node.clone(), OpaqueState::new(0u8, 100));

    // Sanity: the original `volume` entry is now evicted; a probe checkout
    // for it would return None.
    assert!(
        engine.warm_pool().used_bytes() == 100,
        "post-eviction pool must hold only the 100-byte evictor entry; got {} bytes",
        engine.warm_pool().used_bytes()
    );

    // (2) edit_source #2: re-add `volume`. The checkout-and-seed path
    //     (step-8) calls `warm_pool.checkout(volume_node)` — returns None
    //     because the entry was evicted — so `pending_warm_seeds` gets no
    //     entry for `volume`, and the cache entry's `warm_state` slot
    //     remains `None`.
    let module_c = parse_and_compile(bracket_with_volume_let());
    engine
        .edit_source(&module_c)
        .expect("second edit_source must succeed");

    // (a) cache.warm_state for `volume` is None (pool returned None ⇒ no seed).
    let entry = engine.cache_store().get(&volume_node).expect(
        "after edit_source re-adds `volume`, its cache entry must exist (eval populated it)",
    );
    assert!(
        entry.warm_state.is_none(),
        "cache.warm_state for `volume` must be None when pool checkout returned None (LRU-evicted)"
    );
}

// ── Step-11: variant-symmetry — donation hook fires for Realization removal ─

/// Smoke test: when `edit_source` removes a Realization node from the
/// topology, the donation hook (step-6) fires symmetrically with the
/// Value/Constraint variants — the Realization's `cache.warm_state` is
/// donated to `WarmStatePool` keyed by `NodeId::Realization(rid)` BEFORE
/// the cache entry is invalidated. Pins arch §6.4's "removed nodes'
/// warm state donated" rule for the Realization variant.
///
/// Variant-coverage caveats (see top-of-file comment):
/// - Full round-trip (donate → pool → checkout → seed cache.warm_state)
///   is verified for Value cells only. For Realization, the
///   post-add seed step is a no-op today: `cache.donate_warm_state`
///   returns `false` because no cache entry exists for the
///   re-added realization at edit_source time — `engine_build.rs`
///   creates Realization cache entries on demand from `build()` /
///   `check()`, not from `edit_source`. When realizations gain edit-
///   time cache entries, the seed step picks them up automatically
///   (no code change needed in step-8's checkout-and-seed loop).
/// - Resolution variant has no `diff_resolutions` helper yet, so neither
///   donation nor checkout fires for it; the pool API itself is
///   variant-agnostic — verified at the unit-test level (step-1).
/// - ComputeNode is not yet a `NodeId` variant per arch §3.4; coverage
///   attaches automatically when introduced.
///
/// Test driver: simulate a Realization producer that has parked warm
/// state by manually inserting a cache entry for `Bracket@0` (with a
/// dummy `GeometryHandle` payload) and injecting an OpaqueState into
/// its `warm_state` slot. Then `edit_source` to a module that omits
/// the realization (drives `Bracket@0` into `removed_realizations`).
/// Assert the pool now holds the donated state and a `checkout`
/// returns the original 0xBEEF payload.
///
/// TODO(post-engine_build-realization-cache): this test currently invokes
/// `cache_store_mut().insert_synthetic_realization_entry(&rid)` because
/// `engine_build.rs` populates Realization cache entries on demand at
/// `build()`/`check()` time, not at `edit_source()` time. When that
/// population path moves earlier (or another engine path creates Realization
/// cache entries during `edit_source`), replace the synthetic-helper call
/// here with the real production setup so this test exercises the actual
/// cache-population path rather than a placeholder. See
/// `CacheStore::insert_synthetic_realization_entry` doc 'When to retire'
/// note at cache.rs:500-504.
#[test]
fn donation_hook_fires_for_realization_removal() {
    let mut engine = fresh_engine();

    // (1) Eval the bracket fixture which has Bracket@0 realization.
    let module_a = bracket_compiled_module();
    engine.eval(&module_a);

    let rid = RealizationNodeId::new("Bracket", 0);
    let realization_node = NodeId::Realization(rid.clone());

    // (2) Manually create a cache entry for the Realization. `engine_build.rs`
    //     creates these on demand at build()/check() time — not at eval() —
    //     so we synthesize one here for the donation hook to find. The helper
    //     `insert_synthetic_realization_entry` centralizes the schema-coupled
    //     construction (see its docstring in cache.rs for the contract: the
    //     entry exists under NodeId::Realization(rid) and accepts
    //     donate_warm_state; the specific CachedResult variant is incidental).
    engine
        .cache_store_mut()
        .insert_synthetic_realization_entry(&rid);

    // (3) Inject an 8-byte 0xBEEF warm state into the cache entry.
    let donated = engine
        .cache_store_mut()
        .donate_warm_state(&realization_node, OpaqueState::new(0xBEEFu32, 8));
    assert!(
        donated,
        "donate_warm_state must succeed for the manually-inserted Realization cache entry"
    );

    // (4) Sanity: pool starts empty.
    assert_eq!(
        engine.warm_pool().used_bytes(),
        0,
        "warm_pool must start empty before the realization-removing edit"
    );

    // (5) edit_source to a source-text bracket without a realization. The
    //     fixture's other elements (extra params, extra constraints) also
    //     end up in their respective removed sets, but only the
    //     Realization donation matters for this test.
    let module_b = parse_and_compile(bracket_with_volume_let());
    engine
        .edit_source(&module_b)
        .expect("edit_source must succeed when transitioning fixture → source-text bracket");

    // (6) The pool must now hold the 8-byte donated state for Bracket@0.
    assert!(
        engine.warm_pool().used_bytes() >= 8,
        "warm_pool must hold ≥8 bytes after edit_source removes the realization with warm state; \
         got used_bytes = {}",
        engine.warm_pool().used_bytes()
    );

    // (7) checkout(Bracket@0) returns the originally-donated payload.
    let checked_out = engine.warm_pool_mut().checkout(&realization_node);
    let state = checked_out.expect(
        "warm_pool.checkout must return Some for the donated realization after edit_source",
    );
    assert_eq!(
        state.downcast::<u32>(),
        Some(0xBEEFu32),
        "checked-out OpaqueState must downcast to the originally-injected u32 payload (0xBEEF)"
    );
}

// ── Amendment: state taken in (4c) must not be silently lost in (14b) ──────

/// Pins the (4c)→(14b) round-trip preservation contract: when the
/// `pending_warm_seeds` drain finds no cache entry for a re-added node
/// (true today for Constraint and Realization variants — neither is
/// populated by `edit_source`'s eval loop), the engine MUST re-donate
/// the state to its `WarmStatePool` rather than silently drop it. This
/// test pins that re-donation hook end-to-end.
///
/// Driver: pre-seed the pool with a Realization key that we know the
/// post-eval drain step (14b) will see — bypass the cache donation so the
/// pool entry is the ONLY copy of the state. Then run an `edit_source`
/// where the same Realization is in the `added_realizations` set: (4c)
/// checks it out, (14b) tries to seed the cache, fails because no
/// Realization cache entry exists at edit_source time, and re-donates.
/// After the edit, the pool must still hold the state — i.e. it survived
/// the round-trip even though the cache cannot consume it yet.
#[test]
fn pool_state_survives_round_trip_when_cache_cannot_consume() {
    let mut engine = fresh_engine();

    // (1) Eval the canonical bracket fixture (Bracket@0 realization present).
    let module_a = bracket_compiled_module();
    engine.eval(&module_a);

    let rid = RealizationNodeId::new("Bracket", 0);
    let realization_node = NodeId::Realization(rid.clone());

    // (2) edit_source #1: re-add the realization (no-op semantically since
    //     it's already in the graph; what matters is wiring (4c) — but the
    //     simpler driver is to drop, donate, re-add).
    //
    //     First strip the realization with an edit to a source-text bracket.
    let module_b = parse_and_compile(bracket_with_volume_let());
    engine
        .edit_source(&module_b)
        .expect("edit_source to source-text bracket must succeed");

    // (3) Inject warm state directly into the pool keyed by the
    //     Realization NodeId. This is the only copy.
    engine
        .warm_pool_mut()
        .donate(realization_node.clone(), OpaqueState::new(0xFEEDu32, 8));
    let pool_used_before = engine.warm_pool().used_bytes();
    assert!(
        pool_used_before >= 8,
        "pool must hold the seed state before the round-trip; got {} bytes",
        pool_used_before
    );

    // (4) edit_source #2: back to a module that has Bracket@0. The
    //     realization enters `added_realizations`, (4c) checks the pool
    //     out, (14b) probes the cache (no entry) and re-donates.
    engine
        .edit_source(&module_a)
        .expect("edit_source back to fixture-bracket must succeed");

    // (5) Pool MUST still hold the state — re-donation kicked in.
    let recovered = engine.warm_pool_mut().checkout(&realization_node).expect(
        "after round-trip with no cache consumer, pool must still hold the state \
         (else state was silently dropped by (4c)→(14b) — review suggestion #1)",
    );
    assert_eq!(
        recovered.downcast::<u32>(),
        Some(0xFEEDu32),
        "re-donated OpaqueState must downcast to the originally-injected payload (0xFEED)"
    );
}

// ── Task 2518: Compute-node warm-state output-invariance parity test ──────────

/// Load the cantilever modal fixture (examples/modal/cantilever_beam_modes.ri).
fn modal_cantilever_source() -> &'static str {
    include_str!("../../../examples/modal/cantilever_beam_modes.ri")
}

/// Read a frequency cell as bit-pattern (`u64`), tolerating the `Real`
/// placeholder or a dimensioned `Scalar` — mirrors `read_frequency` in
/// `modal_analysis_e2e.rs:35-41`.
fn frequency_bits(val: &Value) -> u64 {
    match val {
        Value::Real(r) => r.to_bits(),
        Value::Scalar { si_value, .. } => si_value.to_bits(),
        other => panic!("expected a frequency Real/Scalar, got: {:?}", other),
    }
}

/// Warm-state seed pipeline output-invariance regression test.
///
/// Asserts that running an `@optimized` modal solve on the `ComputeNode` path
/// with warm state pre-parked in the `WarmStatePool` produces a result that is
/// **bit-identical** to a cold-baseline run.
///
/// ## What this test proves (and what it does NOT)
///
/// **Proven:** (1) **Pipeline consumption** — the parked warm state is checked
/// out of the pool by `run_compute_dispatch` during the warm eval (the pool
/// entry is absent afterwards). (2) **Output-invariance** — the warm eval
/// produces bit-identical results to the cold baseline.
///
/// **NOT proven:** assembly reuse.  The `reused_assembly` flag inside the
/// modal trampoline (modal_ops.rs) is `pub(crate)` and unobservable from this
/// integration test.  If a future regression broke `key.matches` so the
/// trampoline always re-assembled (K,M) from scratch, both runs would still
/// produce bit-identical eigenvalues (deterministic assembly + deterministic
/// eigensolver), and the pool entry would still be consumed, so this test would
/// remain GREEN.  Assembly-reuse correctness is covered by the in-crate unit
/// tests in modal_ops.rs:2504-2641 that directly assert on `reused_assembly`.
///
/// ## Why GREEN on arrival
///
/// The warm-state lifecycle landed in tasks 3425/3496 and is output-invariant
/// by construction (deterministic modal assembly + deterministic eigensolver ⇒
/// identical bits whether the trampoline reuses or re-assembles (K,M)).
/// There is no production code to fix.  This test guards against **future**
/// regressions in the seed pipeline that would silently corrupt the result
/// (e.g. a pool-checkout that returns a malformed OpaqueState causing the
/// trampoline to produce wrong eigenvalues).
///
/// ## Why two fresh engines
///
/// A second `eval()` on the same engine hits the value cache and does NOT
/// re-dispatch (proven by `e2e_cantilever_second_eval_hits_cache`).  So
/// warm-vs-cold must be staged across two separate engines: the cold engine
/// provides the reference result AND the donated warm state; a fresh engine
/// receives that warm state via `warm_pool_mut().donate(...)` before its
/// first eval.
///
/// ## Release gate
///
/// The `debug_assertions` guard skips this test in debug builds to avoid the
/// cost of two full modal solves.  The test DOES run at merge time: the
/// orchestrator sets `DF_VERIFY_ROLE=merge`, which causes `scripts/verify.sh`
/// to default to `--profile both` (debug + release passes).  `reify-eval` is
/// a release-sensitive crate and is included in the release nextest pass, so
/// every merge gate exercises this test.  To run it locally:
/// `cargo test --release -p reify-eval --test warm_state_donation`.
#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]
#[test]
fn warm_state_seeded_modal_solve_matches_cold_baseline() {
    let compiled = parse_and_compile_with_stdlib(modal_cantilever_source());

    // ── (A) Cold baseline engine ───────────────────────────────────────────
    // Runs cold (no prior warm state anywhere).  run_compute_dispatch sees
    // cache-miss + pool-miss → prior=None → full cold compute.
    let mut baseline = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut baseline);
    let baseline_eval = baseline.eval(&compiled);

    // No Error-severity diagnostics on the cold run.
    let errors: Vec<_> = baseline_eval
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "cold baseline eval must produce no Error diagnostics; got: {:?}",
        errors
    );

    // Capture result and f1 from the baseline.
    let result_cell = ValueCellId::new("CantileverBeamModes", "result");
    let f1_cell = ValueCellId::new("CantileverBeamModes", "f1");

    let baseline_result = baseline_eval
        .values
        .get(&result_cell)
        .cloned()
        .expect("baseline eval must produce CantileverBeamModes.result");
    let baseline_f1 = frequency_bits(
        baseline_eval
            .values
            .get(&f1_cell)
            .expect("baseline eval must produce CantileverBeamModes.f1"),
    );

    // Locate the modal ComputeNodeId from the baseline snapshot.
    let snapshot = baseline
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let c_id: ComputeNodeId = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, d)| d.target == "modal::free_vibration")
        .map(|(id, _)| id.clone())
        .expect(
            "cold modal solve must produce a ComputeNode with target==\"modal::free_vibration\"",
        );

    // Extract the donated warm state from the baseline cache (take semantics).
    let warm = baseline
        .cache_store_mut()
        .get_warm_state(&NodeId::Compute(c_id.clone()))
        .expect("cold modal solve must donate warm state under NodeId::Compute after eval");

    // ── (B) Warm engine ────────────────────────────────────────────────────
    // Pre-park the baseline's warm state in the fresh engine's WarmStatePool.
    // The FIRST eval on this engine dispatches the solve; run_compute_dispatch's
    // cache-miss → pool-hit checkout (engine_compute.rs:285-288) delivers the warm
    // state to the modal trampoline (which may reuse the assembled (K,M) via
    // key.matches — correctness of that reuse is tested in modal_ops.rs unit tests).
    let mut warm_engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut warm_engine);

    warm_engine
        .warm_pool_mut()
        .donate(NodeId::Compute(c_id.clone()), warm);
    assert!(
        warm_engine.warm_pool().contains(&NodeId::Compute(c_id.clone())),
        "warm state must be present in pool before the warm eval"
    );

    let warm_eval = warm_engine.eval(&compiled);

    // No Error-severity diagnostics on the warm run.
    let errors: Vec<_> = warm_eval
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "warm eval must produce no Error diagnostics; got: {:?}",
        errors
    );

    // Pipeline-exercised guard: the pool-hit checkout in run_compute_dispatch
    // consumed the parked warm state, so the pool entry is gone after eval.
    assert!(
        !warm_engine.warm_pool().contains(&NodeId::Compute(c_id.clone())),
        "warm state must be consumed (checked out) by run_compute_dispatch during warm eval; \
         pool still holds it → seed pipeline did not run"
    );

    // ── (C) Output-invariance assertions ──────────────────────────────────
    // Bit-identity is robust by construction: modal assembly is deterministic
    // and the dense eigensolver is deterministic, so whether the trampoline
    // reuses the baseline's assembled (K,M) via key.matches (the normal warm
    // path) or re-assembles from scratch, identical input geometry ⇒ identical
    // (K,M) ⇒ identical eigenvalues ⇒ identical bits.  Assembly-reuse
    // correctness itself is verified by in-crate unit tests in
    // modal_ops.rs:2504-2641 that assert on the `reused_assembly` flag.

    // (C1) Primary: f1 bits are identical.
    let warm_f1 = frequency_bits(
        warm_eval
            .values
            .get(&f1_cell)
            .expect("warm eval must produce CantileverBeamModes.f1"),
    );
    assert_eq!(
        warm_f1,
        baseline_f1,
        "f1 bits must be identical: deterministic assembly + deterministic eigensolver \
         guarantee bit-identity regardless of whether the trampoline reused (K,M); \
         warm_f1 bits={:#018x}, baseline_f1 bits={:#018x}",
        warm_f1,
        baseline_f1
    );

    // (C2) Comprehensive: the whole result Value is equal.
    // If incidental non-frequency metadata ever makes this brittle, narrow to
    // a per-mode frequency-bit comparison over result.modes (modes_freq_participation
    // pattern in modal_analysis_e2e.rs) — but the f1-bits assertion (C1) stands.
    let warm_result = warm_eval
        .values
        .get(&result_cell)
        .cloned()
        .expect("warm eval must produce CantileverBeamModes.result");
    assert_eq!(
        warm_result,
        baseline_result,
        "result Value must be identical across cold and warm runs"
    );
}
