//! Integration tests for task ζ (3425): OpaqueState lifecycle through
//! `Engine::run_compute_dispatch` — cache as the canonical at-rest store.
//!
//! See `docs/prds/v0_3/compute-node-contract.md` §5, §8-ζ.
//!
//! Observable signals landed:
//! - (A) Direct dispatch: a counter trampoline returns 0 on first call and
//!   1 on the second call, with `prior` sourced from the cache between
//!   dispatches.
//! - (B) `cost_per_byte`: the trampoline's reported cost is observable on
//!   the cache via `cache.cost_per_byte_of(&NodeId::Compute(c_id))` after
//!   each dispatch.

use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_test_support::make_simple_engine;
use reify_types::{
    ComputeNodeId, DeterminacyState, Freshness, OpaqueState, Value, ValueCellId, VersionId,
};

/// Counter trampoline (ζ §8): on each call, returns `Int(prior_count)` and
/// donates `OpaqueState::new(prior_count + 1, 4)` so the next call sees
/// `prior = Some(prior_count)`. When `prior` is `None`, returns `Int(0)`
/// and donates `OpaqueState::new(0, 4)`.
///
/// `cost_per_byte` is reported as `Some(0.5)` on every call so tests can
/// observe the cost being threaded into the cache via
/// `CacheStore::cost_per_byte_of`.
fn counter_trampoline(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let prior_count = prior_warm_state.and_then(|s| s.downcast_ref::<i32>().copied());
    let (result_int, next_count) = match prior_count {
        None => (0i32, 0i32),
        Some(n) => (n + 1, n + 1),
    };
    ComputeOutcome::Completed {
        result: Value::Int(result_int as i64),
        new_warm_state: Some(OpaqueState::new(next_count, 4)),
        cost_per_byte: Some(0.5),
        diagnostics: vec![],
    }
}

/// (A)+(B) — Two sequential dispatches return Int(0) then Int(1), with the
/// trampoline observing the previously-donated counter pulled out of the
/// cache by `run_compute_dispatch`. After each dispatch the cache holds the
/// new counter under `NodeId::Compute(c_id)` and the reported
/// `cost_per_byte` is observable via `cache.cost_per_byte_of`.
///
/// RED until step-2 lands `donate_warm_state_with_cost` / `cost_per_byte_of`,
/// step-4 extends `complete_compute_dispatch_atomically` to thread warm
/// state + cost into a `NodeId::Compute(c_id)` entry, and step-6 drops the
/// dead `prior_warm_state` parameter from `run_compute_dispatch`.
#[test]
fn counter_trampoline_two_dispatches_returns_0_then_1() {
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::counter_zeta_a", counter_trampoline as ComputeFn);

    let cell = ValueCellId::new("T", "counter_out");
    let c_id = ComputeNodeId::new("T", 0);

    // Seed an output VC with a Final entry so begin_compute_dispatch has a
    // last_substantive to display (mirrors cancellation_compute_dispatch.rs).
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );

    // ── Dispatch 1: prior was None → returns Int(0), donates counter=0 ──────
    let handle = CancellationHandle::new();
    let (val1, diags1) = engine
        .run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::counter_zeta_a",
            &[],
            &[],
            &Value::Undef,
            &handle,
            VersionId(2),
        )
        .expect("dispatch 1 must return Ok");
    assert_eq!(
        val1,
        Value::Int(0),
        "trampoline with prior=None must return Int(0); got {val1:?}",
    );
    assert!(diags1.is_empty(), "counter trampoline emits no diagnostics");

    // After dispatch 1 the Compute entry must hold counter=0 and cost=0.5.
    let entry1 = engine
        .cache_store()
        .get(&NodeId::Compute(c_id.clone()))
        .expect(
            "complete_compute_dispatch_atomically must auto-seed a Compute \
             entry when new_warm_state is Some",
        );
    assert_eq!(
        entry1
            .warm_state
            .as_ref()
            .and_then(|s| s.downcast_ref::<i32>().copied()),
        Some(0i32),
        "cache must hold counter=0 after dispatch 1",
    );
    assert_eq!(
        engine
            .cache_store()
            .cost_per_byte_of(&NodeId::Compute(c_id.clone())),
        Some(0.5),
        "cache must reflect trampoline's cost_per_byte (0.5) after dispatch 1",
    );

    // ── Dispatch 2: prior was Some(0) (pulled from cache) → returns Int(1) ──
    let handle = CancellationHandle::new();
    let (val2, diags2) = engine
        .run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::counter_zeta_a",
            &[],
            &[],
            &Value::Undef,
            &handle,
            VersionId(3),
        )
        .expect("dispatch 2 must return Ok");
    assert_eq!(
        val2,
        Value::Int(1),
        "trampoline with prior=Some(0) must return Int(1); got {val2:?}",
    );
    assert!(diags2.is_empty(), "counter trampoline emits no diagnostics");

    // After dispatch 2 the Compute entry must hold counter=1 and cost=0.5.
    let entry2 = engine
        .cache_store()
        .get(&NodeId::Compute(c_id.clone()))
        .expect("Compute entry must persist across dispatches");
    assert_eq!(
        entry2
            .warm_state
            .as_ref()
            .and_then(|s| s.downcast_ref::<i32>().copied()),
        Some(1i32),
        "cache must hold counter=1 after dispatch 2",
    );
    assert_eq!(
        engine
            .cache_store()
            .cost_per_byte_of(&NodeId::Compute(c_id)),
        Some(0.5),
        "cache must reflect trampoline's cost_per_byte (0.5) after dispatch 2",
    );
}
