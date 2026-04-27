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
use reify_eval::cache::NodeId;
use reify_eval::Engine;
use reify_test_support::parse_and_compile;
use reify_types::{OpaqueState, ValueCellId};

/// Build a fresh Engine (no prior eval) backed by the real constraint checker.
fn fresh_engine() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None)
}

/// Bracket source with a single configurable let so tests can drop just the
/// let between module_a and module_b without touching params or constraints.
fn bracket_with_volume_let() -> &'static str {
    r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

    let volume = width * height * thickness

    constraint thickness > 2mm
}"#
}

/// Bracket source with the `volume` let stripped out — used as module_b in
/// the removal scenarios. Same params and constraint as `bracket_with_volume_let`.
fn bracket_without_volume_let() -> &'static str {
    r#"structure Bracket {
    param width: Scalar = 80mm
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm

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
    let state = checked_out.expect(
        "warm_pool.checkout must return Some for the donated cell after edit_source",
    );
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
    let warm = entry.warm_state.as_ref().expect(
        "post-checkout-and-seed cache entry must carry the donated warm_state",
    );
    assert_eq!(
        warm.downcast_ref::<u32>().copied(),
        Some(0xDEADBEEFu32),
        "seeded warm_state must downcast to the originally-injected u32 payload"
    );
}
