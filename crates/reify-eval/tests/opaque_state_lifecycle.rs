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
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
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

// ── ζ / task 3425 step-11: remove-and-reinsert via edit_source ────────────────
//
// PRD §5 + §8 ζ "(D) Remove-reinsert via edit_source: counter survives the
// round-trip through the warm_pool".
//
// Flow:
//   eval(v1)        → @optimized lowering creates a ComputeNode; counter
//                     dispatches first time, returns 0; cache holds counter=0
//                     under Compute(c_id).
//   edit_source(v2) → v2 OMITS the @optimized call. engine_edit step-(9b)
//                     iterates the OLD snapshot's compute_nodes and donates
//                     each cache.warm_state → warm_pool (with cost) before
//                     invalidating the cache entry. After this edit the
//                     cache no longer has a Compute(c_id) entry and the pool
//                     holds counter=0 keyed by Compute(c_id).
//   eval(v3 = v1)   → fresh eval. @optimized lowering recreates the
//                     ComputeNode at the same path-based c_id and calls
//                     run_compute_dispatch, which hits the cache-miss →
//                     warm_pool fallback (step-6) and observes prior=Some(0).
//                     Trampoline returns Int(1) and donates counter=1 → cache.
//
// Note: the v3 leg uses `eval()` rather than `edit_source()` because
// engine_edit.rs's per-cell eval loop (lines ~2411-2476) calls
// `reify_expr::eval_expr` directly and does NOT do @optimized → ComputeNode
// lowering — that path lives only in engine_eval.rs (lines ~2797+). A v3
// edit_source would re-evaluate `result` to its inlined expression value
// rather than creating a ComputeNode and dispatching. Wiring @optimized
// lowering into edit_source is a separate concern outside ζ scope; the
// signal (D) round-trip is fully exercised by eval(v1) → edit_source(v2)
// → eval(v3), since `eval()` preserves cache + warm_pool state across calls
// (it reassigns `eval_state` but does not clear `self.cache` or
// `self.warm_pool`).
//
// RED before step-12: fails because the cache-miss path in run_compute_dispatch
// (step-6) finds NEITHER a cache entry (invalidated by v2's edit) NOR a pool
// entry (because nothing donated to the pool), so the trampoline observes
// prior=None on the v3 dispatch and returns Int(0) instead of Int(1). After
// step-12 wires the engine_edit step-(9b) cache→pool donation for old
// compute_nodes, the pool half of the round-trip lands and the test goes GREEN.

/// v1 / v3: structure body calls the @optimized counter — ComputeNode present.
fn counter_source_with_call() -> &'static str {
    r#"@optimized("test::counter_zeta_d")
fn counter_compute(x: Int) -> Int {
    x
}

structure CounterFixture {
    param input: Int = 0
    let result = counter_compute(input)
}
"#
}

/// v2: structure body uses `input` directly — no @optimized call, no ComputeNode.
/// `counter_compute` is still declared (kept stable so v1/v2/v3 only differ in
/// the let-binding RHS) but no longer called — the ComputeNode falls out of
/// the graph between v1 → v2.
fn counter_source_without_call() -> &'static str {
    r#"@optimized("test::counter_zeta_d")
fn counter_compute(x: Int) -> Int {
    x
}

structure CounterFixture {
    param input: Int = 0
    let result = input
}
"#
}

#[test]
fn remove_and_reinsert_via_edit_source_preserves_counter() {
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::counter_zeta_d", counter_trampoline as ComputeFn);

    let result_cell = ValueCellId::new("CounterFixture", "result");

    // ── eval(v1): first counter dispatch — observes prior=None, returns 0 ──
    let v1 = parse_and_compile_with_stdlib(counter_source_with_call());
    let eval1 = engine.eval(&v1);
    assert_eq!(
        eval1.values.get(&result_cell),
        Some(&Value::Int(0)),
        "first dispatch with prior=None must yield Int(0); got {:?}",
        eval1.values.get(&result_cell),
    );

    // Identify the ComputeNode the compiler emitted for the counter call.
    let c_id_v1 = engine
        .eval_state()
        .expect("eval_state must be Some after first eval()")
        .snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, d)| d.target == "test::counter_zeta_d")
        .map(|(id, _)| id.clone())
        .expect("v1 graph must contain a ComputeNode for test::counter_zeta_d");

    // Cache must hold counter=0 under NodeId::Compute(c_id) after eval(v1).
    // This is the "pre-remove" state that step-12 must donate to the pool
    // when v2 drops the @optimized call.
    assert_eq!(
        engine
            .cache_store()
            .get(&NodeId::Compute(c_id_v1.clone()))
            .and_then(|e| e.warm_state.as_ref().and_then(|s| s.downcast_ref::<i32>().copied())),
        Some(0i32),
        "cache must hold counter=0 under Compute(c_id) after eval(v1)",
    );

    // ── edit_source(v2): drops the @optimized call — ComputeNode is removed ──
    let v2 = parse_and_compile_with_stdlib(counter_source_without_call());
    engine
        .edit_source(&v2)
        .expect("edit_source(v2) must succeed");

    let cn_in_v2 = engine
        .eval_state()
        .expect("eval_state must be Some after edit_source(v2)")
        .snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, d)| d.target == "test::counter_zeta_d");
    assert!(
        !cn_in_v2,
        "v2 graph must NOT contain a ComputeNode for test::counter_zeta_d \
         (the @optimized call was dropped)",
    );

    // After v2 the cache entry for the old ComputeNode must be gone — step-12
    // invalidates it after donating warm state to the pool.
    assert!(
        engine
            .cache_store()
            .get(&NodeId::Compute(c_id_v1.clone()))
            .is_none(),
        "cache entry for the removed ComputeNode must be invalidated by step-12",
    );

    // step-12: the prior counter must now sit in the warm_pool keyed by the
    // ComputeNode's NodeId. `contains` is non-destructive — it leaves the
    // entry intact so the run_compute_dispatch cache-miss → pool fallback
    // (step-6) can find it on the v3 re-eval below.
    assert!(
        engine.warm_pool().contains(&NodeId::Compute(c_id_v1.clone())),
        "warm_pool must hold the counter for the removed ComputeNode after \
         edit_source(v2) — step-12 must donate cache.warm_state → pool",
    );

    // ── eval(v3 = v1): re-introduces the @optimized call via the eval path ──
    // We use `eval()` (not `edit_source`) here because engine_edit.rs lacks
    // the @optimized → ComputeNode lowering; only engine_eval.rs's per-cell
    // loop creates ComputeNodes from @optimized calls. `eval()` preserves
    // both `self.cache` and `self.warm_pool` across calls (it only reassigns
    // `eval_state.snapshot`), so the pool donation made by v2's edit_source
    // survives into v3's dispatch.
    let v3 = parse_and_compile_with_stdlib(counter_source_with_call());
    let eval3 = engine.eval(&v3);

    // ComputeNode must be re-inserted and have the SAME id as v1 (path/index
    // identity stable across remove-reinsert).
    let c_id_v3 = engine
        .eval_state()
        .expect("eval_state must be Some after eval(v3)")
        .snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, d)| d.target == "test::counter_zeta_d")
        .map(|(id, _)| id.clone())
        .expect("v3 graph must contain a ComputeNode for test::counter_zeta_d");
    assert_eq!(
        c_id_v3, c_id_v1,
        "ComputeNode id must be stable across remove-reinsert (path-based identity)",
    );

    // Final cell value must be Int(1) — the trampoline ran with prior=Some(0)
    // sourced from the warm_pool via step-6's cache-miss → pool fallback.
    // RED until step-12 makes the pool hold this state.
    assert_eq!(
        eval3.values.get(&result_cell),
        Some(&Value::Int(1)),
        "after remove-reinsert, the counter must observe the prior from pool \
         and return prior+1; expected Int(1), got {:?}",
        eval3.values.get(&result_cell),
    );

    // Cache must now hold counter=1 (the trampoline's newly-donated warm
    // state, atomic-completed via step-8).
    assert_eq!(
        engine
            .cache_store()
            .get(&NodeId::Compute(c_id_v1.clone()))
            .and_then(|e| e.warm_state.as_ref().and_then(|s| s.downcast_ref::<i32>().copied())),
        Some(1i32),
        "cache must hold counter=1 after the v3 dispatch",
    );

    // The pool entry was consumed by run_compute_dispatch's
    // `checkout_with_lru_stamp` on the v3 dispatch — take semantics.
    assert!(
        !engine.warm_pool().contains(&NodeId::Compute(c_id_v1)),
        "warm_pool entry must be consumed (taken) by v3's run_compute_dispatch \
         cache-miss → pool fallback",
    );
}

// ── ζ / task 3425 step-12: cost_per_byte transfers through cache→pool donation
//
// Pins the cost-carrying invariant of engine_edit step-(9b): when
// `edit_source` drops a ComputeNode from the topology, the cache entry's
// `cost_per_byte` must travel through to the `warm_pool` alongside the warm
// state via `donate_with_cost`. The reinsert leg (pool → cache on next
// dispatch) is covered separately by step-6's cache-miss fallback in
// `run_compute_dispatch`. PRD §5 step-(4) names the cache as the canonical
// at-rest store for warm state and cost-per-byte; when a node leaves the
// topology, both must be parked in the pool together so the pool's
// cost-weighted LRU eviction (warm_pool.rs `insert_entry`) stays meaningful.
//
// Companion to `remove_and_reinsert_via_edit_source_preserves_counter`
// above (which exercises the (D) round-trip via state identity); this test
// uses a marker cost (0.4) so it can assert the cost is carried, not just
// the warm state.
//
// The plan (step-12) directed this test at `engine_edit.rs`'s unit-test
// module, but the engine setup requires `make_simple_engine` +
// `parse_and_compile_with_stdlib` from `reify-test-support`, and the lib's
// dev-dep instance of `reify-eval` is not type-identical to `crate::*`
// inside the lib (the test-instrumentation feature creates a distinct
// compilation), so the test lives here in the integration-test crate where
// all types come from a single instance.

/// Trampoline emitting cost_per_byte = 0.4 + a sentinel warm state — the
/// cost is the specific value the test asserts is carried through the
/// cache → pool donation in step-(9b).
fn cost_marker_trampoline(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: Value::Int(0),
        new_warm_state: Some(OpaqueState::new(0xA5A5_A5A5u32, 4)),
        cost_per_byte: Some(0.4),
        diagnostics: vec![],
    }
}

fn cost_marker_source_with_call() -> &'static str {
    r#"@optimized("test::cost_marker_zeta_e")
fn cost_marker(x: Int) -> Int {
    x
}

structure CostMarkerFixture {
    param input: Int = 0
    let result = cost_marker(input)
}
"#
}

fn cost_marker_source_without_call() -> &'static str {
    r#"@optimized("test::cost_marker_zeta_e")
fn cost_marker(x: Int) -> Int {
    x
}

structure CostMarkerFixture {
    param input: Int = 0
    let result = input
}
"#
}

#[test]
fn edit_source_donates_old_compute_node_warm_state_to_warm_pool_with_cost() {
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::cost_marker_zeta_e", cost_marker_trampoline as ComputeFn);

    // eval(v1): @optimized lowering creates a ComputeNode; the trampoline
    // donates warm state + cost 0.4 to the cache.
    let v1 = parse_and_compile_with_stdlib(cost_marker_source_with_call());
    let _eval1 = engine.eval(&v1);

    let c_id = engine
        .eval_state()
        .expect("eval_state must be Some after first eval()")
        .snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, d)| d.target == "test::cost_marker_zeta_e")
        .map(|(id, _)| id.clone())
        .expect("v1 graph must contain a ComputeNode for test::cost_marker_zeta_e");

    // Pre-check: after eval(v1) the cache must hold cost 0.4 under the
    // Compute entry — this is the source side of the step-(9b) transfer.
    assert_eq!(
        engine
            .cache_store()
            .cost_per_byte_of(&NodeId::Compute(c_id.clone())),
        Some(0.4),
        "cache must hold cost_per_byte=0.4 under Compute(c_id) after eval(v1)",
    );

    // edit_source(v2): drops the @optimized call → ComputeNode is removed,
    // step-(9b) donates the cache's warm state + cost to the pool.
    let v2 = parse_and_compile_with_stdlib(cost_marker_source_without_call());
    engine
        .edit_source(&v2)
        .expect("edit_source(v2) must succeed");

    // The pool must now carry cost_per_byte = 0.4 (carried from cache by
    // step-(9b)'s `donate_with_cost` call).
    assert_eq!(
        engine
            .warm_pool()
            .cost_per_byte_of(&NodeId::Compute(c_id.clone())),
        Some(0.4),
        "warm_pool must carry cost_per_byte=0.4 from cache after edit_source \
         drops the ComputeNode (step-12 cache→pool donation)",
    );

    // The cache entry for the dropped ComputeNode must be invalidated by the
    // tail of step-(9b) — pool is now the sole holder of warm state + cost.
    assert!(
        engine
            .cache_store()
            .get(&NodeId::Compute(c_id))
            .is_none(),
        "cache entry for the removed ComputeNode must be invalidated by step-(9b) \
         after the cost+state were donated to the pool",
    );
}
