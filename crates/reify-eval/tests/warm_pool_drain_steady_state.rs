//! Steady-state regression test for the warm-pool event drain at the engine
//! eval boundary.
//!
//! ## Why GREEN on arrival
//!
//! The drain→record→emit chain landed in task #3541:
//! `Engine::drain_and_record_warm_pool_events` (engine_admin.rs) drains the
//! `WarmStatePool` event buffer AND records each event to the diagnostic
//! journal via `translate_warm_pool_event_to_eval_event`.  Its live consumer
//! is `EngineSession::drain_and_emit_warm_pool_events` (gui/src-tauri/src/engine.rs),
//! called after every engine boundary (check / edit_check / build /
//! tessellate_snapshot).
//!
//! This test characterises that wiring as a reify-eval-level regression guard
//! and pins acceptance items #3 and #4 from task #3582:
//! - **#3 Steady state** — after each `drain_and_record_warm_pool_events()`
//!   call the buffer is immediately empty (per-iteration assertion; the
//!   `<= MAX / 4` framing would be vacuously true with only 2 events/iteration,
//!   so we assert `== 0` directly).
//! - **#4 debug_assert! safety** — the per-iteration drain keeps the buffer
//!   near-zero; `dropped_events` is checked as a sanity guard (the cap 65 536
//!   is never approached).  Overflow-boundary coverage lives in warm_pool.rs
//!   unit tests (`events_buffer_debug_assert_fires_on_overflow_in_debug_build`
//!   / `events_buffer_auto_trims_and_records_dropped_count`).
//!
//! The test runs in default (debug) profile so `debug_assertions` are active.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_eval::warm_pool::{WarmPoolEvent, WarmStatePool};
use reify_ir::OpaqueState;
use reify_test_support::parse_and_compile;

/// Build a fresh `Engine` with the real constraint checker (mirrors
/// `fresh_engine()` in warm_state_donation.rs).
fn fresh_engine() -> Engine {
    Engine::new(Box::new(SimpleConstraintChecker), None)
}

fn bracket_source() -> &'static str {
    r#"structure Bracket {
    param width: Length = 80mm
    param height: Length = 100mm
    param thickness: Length = 5mm
    constraint thickness > 2mm
}"#
}

fn bracket_source_b() -> &'static str {
    r#"structure Bracket {
    param width: Length = 80mm
    param height: Length = 100mm
    param thickness: Length = 6mm
    constraint thickness > 2mm
}"#
}

/// Warm-pool event drain is wired at the engine eval boundary and keeps the
/// buffer in steady state across a multi-eval/edit loop.
///
/// Acceptance assertions:
/// (a) Accumulated drained events contain >= 1 `WarmPoolEvent::Donated` AND
///     >= 1 `WarmPoolEvent::Evicted` — the boundary hook surfaced both kinds.
/// (b) `engine.journal().count_donated() >= 1` AND
///     `engine.journal().count_evicted() >= 1` — events were recorded in the
///     diagnostic journal (leaf observable for acceptance #1).
/// (c) After each `drain_and_record_warm_pool_events()` call the buffer is
///     **immediately empty** (acceptance #3 — steady state).  This assertion
///     is the real regression guard: if the hook is broken and does not clear
///     the buffer, the 2 events pushed each iteration remain and trip the
///     assertion.  A `<= MAX_BUFFERED_EVENTS / 4` post-loop check would be
///     vacuously true with only 6 events total, so we assert per-iteration
///     emptiness instead.
/// (d) `engine.warm_pool().dropped_events() == 0` — sanity check that no
///     events were silently dropped.  The cap (65 536) is never approached
///     with 2 events/iteration × 3 iterations, so this is not a full exercise
///     of the overflow path.  Overflow-boundary coverage for acceptance #4
///     lives in warm_pool.rs unit tests.
#[test]
fn drain_at_eval_boundary_keeps_buffer_in_steady_state() {
    let mut engine = fresh_engine();

    // Swap in a 1-byte budget pool so that every second donate triggers an
    // eviction — guarantees at least one Donated and one Evicted event per
    // iteration without relying on the real module topology.
    *engine.warm_pool_mut() = WarmStatePool::new(1);

    let module_a = parse_and_compile(bracket_source());
    let module_b = parse_and_compile(bracket_source_b());

    let node_a = NodeId::Value(ValueCellId::new("Bracket", "width"));
    let node_b = NodeId::Value(ValueCellId::new("Bracket", "height"));

    let mut all_drained: Vec<WarmPoolEvent> = Vec::new();

    // Run three iterations of: eval → donate×2 (guarantees Donated + Evicted)
    // → drain at the eval boundary.
    for i in 0..3 {
        // Alternate between two modules so edit_source advances the version.
        let module = if i % 2 == 0 { &module_a } else { &module_b };
        engine.eval(module);

        // Deterministic donate driver (mirrors lib.rs:2479 unit test):
        //   donate(a, size=1, budget=1) → Donated(a)
        //   donate(b, size=1, budget=1) → evicts a → Evicted(a), Donated(b)
        engine
            .warm_pool_mut()
            .donate(node_a.clone(), OpaqueState::new(1i32, 1));
        engine
            .warm_pool_mut()
            .donate(node_b.clone(), OpaqueState::new(2i32, 1));

        // Eval-boundary drain (the hook under test).
        let drained = engine.drain_and_record_warm_pool_events();

        // (c) Immediately assert that the drain hook cleared the buffer.
        // This is the meaningful regression guard for acceptance #3: if
        // drain_and_record_warm_pool_events were broken and did not call
        // drain_events(), the 2 events pushed above would still be here and
        // trip this assertion.  A post-loop MAX/4 check would be vacuously
        // true because 6 total events are far below 16 384.
        let post_drain_residual = engine.warm_pool_mut().drain_events();
        assert!(
            post_drain_residual.is_empty(),
            "drain_and_record_warm_pool_events must empty the event buffer \
             on iteration {} (acceptance #3 — steady state); \
             residual ({} events): {:?}",
            i,
            post_drain_residual.len(),
            post_drain_residual
        );

        all_drained.extend(drained);
    }

    // (a) At least one Donated and one Evicted surfaced via the boundary hook.
    let has_donated = all_drained
        .iter()
        .any(|e| matches!(e, WarmPoolEvent::Donated { .. }));
    let has_evicted = all_drained
        .iter()
        .any(|e| matches!(e, WarmPoolEvent::Evicted { .. }));
    assert!(
        has_donated,
        "accumulated drained events must contain at least one Donated; got {:?}",
        all_drained
    );
    assert!(
        has_evicted,
        "accumulated drained events must contain at least one Evicted; got {:?}",
        all_drained
    );

    // (b) Events were recorded to the diagnostic journal.
    assert!(
        engine.journal().count_donated() >= 1,
        "journal must record at least one Donated event; count={}",
        engine.journal().count_donated()
    );
    assert!(
        engine.journal().count_evicted() >= 1,
        "journal must record at least one Evicted event; count={}",
        engine.journal().count_evicted()
    );

    // (c) Post-loop: buffer is empty.  The per-iteration assertions above are
    // the meaningful regression guard; this final check is redundant (the
    // per-iteration drain_events() calls already consumed the buffer) but
    // documents the expected steady-state invariant explicitly.
    let residual = engine.warm_pool_mut().drain_events().len();
    assert_eq!(
        residual,
        0,
        "post-loop buffer must be empty (== 0); steady state is pinned \
         per-iteration above; got {}",
        residual
    );

    // (d) Sanity check: no events were dropped.
    // The cap (65 536) is never approached with 2 events/iteration × 3
    // iterations; this is not a full exercise of the overflow / debug_assert!
    // path.  Overflow-boundary coverage lives in warm_pool.rs unit tests
    // (events_buffer_debug_assert_fires_on_overflow_in_debug_build and
    // events_buffer_auto_trims_and_records_dropped_count).
    assert_eq!(
        engine.warm_pool().dropped_events(),
        0,
        "dropped_events must be 0 in steady state; got {}",
        engine.warm_pool().dropped_events()
    );
}
