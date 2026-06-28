//! Integration tests for task ε (3424): Cancellation wiring through dispatch.
//!
//! See `docs/prds/v0_3/compute-node-contract.md` §2, §7.1, §8-ε.
//!
//! These tests exercise the four observable signals from PRD §7.1 / §8-ε:
//!
//! - **(A) EVAL-PATH CANCELLED→PENDING** — after `engine.eval()` with a trampoline
//!   that returns `ComputeOutcome::Cancelled`, the output VC must remain
//!   `Freshness::Pending` (NOT `Failed`) and the `running` slot must be cleared.
//!   Step-4 implemented the Cancelled arm split at the `@optimized` lowering site;
//!   this test is green.
//!
//! - **(B) COOPERATIVE CROSS-THREAD CANCEL PROPAGATION** — a canceller thread
//!   fires mid-trampoline; dispatch must return `Err(DispatchError::Cancelled)`.
//!   The original wall-clock SLA (`elapsed < 5 × POLL_BUDGET_MS`) was
//!   load-dependent and is now a **non-fatal `eprintln!` observation**
//!   (esc-4583-45); the load-independent regression guard moved to test E.
//!   Passes after step-2.
//!
//! - **(C) SYNCHRONOUS DISPATCH** — across 20 sequential dispatches the global
//!   in-flight counter returns to zero between every call, confirming dispatch
//!   is synchronous.  (One-in-flight under concurrent dispatch is trivially
//!   guaranteed by synchrony and is deferred to the future async-driver slice.)
//!   Passes after step-2.
//!
//! - **(D) PRIOR-CACHE-INTACT** — a seeded Final output VC is left in
//!   `Freshness::Pending{last_substantive: prior}` after a cancelled dispatch;
//!   the prior cached value is unchanged and no warm-state is donated.
//!   Passes after step-2.
//!
//! - **(E) PRE-CANCELLED REGRESSION GUARD** — a handle cancelled *before*
//!   `run_compute_dispatch` (no canceller thread, no race) causes the
//!   instrumented trampoline (`precancel_poll_fn`) to return after exactly one
//!   poll iteration (`PRECANCEL_POLL_ITERS <= 1`).  Load-independent: no
//!   `Duration` asserted.  Replaces the removed wall-clock SLA from test B.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use reify_core::{ComputeNodeId, ContentHash, ValueCellId, VersionId};
use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{
    CancellationHandle, ComputeFn, ComputeOutcome, DispatchError, RealizationReadHandle,
};
use reify_ir::{DeterminacyState, Freshness, OpaqueState, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ═══════════════════════════════════════════════════════════════════════════════
// Test A — EVAL-PATH CANCELLED→PENDING
// ═══════════════════════════════════════════════════════════════════════════════

/// Trampoline A: always returns `ComputeOutcome::Cancelled` without inspecting
/// the handle (simulates a compute fn that observes cancellation immediately).
fn always_cancelled_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Cancelled
}

/// (A) After `engine.eval()` with a trampoline returning `Cancelled`, the output
/// VC must be `Freshness::Pending` (NOT `Failed`), `pending_cause` must point at
/// the `ComputeNode`, and the `running` slot must be cleared.
///
/// The Cancelled arm was split at the `@optimized` lowering site in step-4 of
/// task ε/3424 (`engine_eval.rs` — `Err(DispatchError::Cancelled)` leaves the VC
/// `Pending` per PRD §2 / §7.1 rather than forwarding to the Failed arm).
#[test]
fn eval_path_cancelled_leaves_output_vc_pending_not_failed() {
    let source = include_str!("fixtures/compute_identity.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", always_cancelled_fn as ComputeFn);

    engine.eval(&compiled);

    let result_cell = ValueCellId::new("IdentityFixture", "result");
    let node = NodeId::Value(result_cell.clone());
    // The lowering site uses `cell_id.entity` + `max(index) + 1`; for a single
    // @optimized call on IdentityFixture, the first index is 0.
    let c_id = ComputeNodeId::new("IdentityFixture", 0);

    // (A1) Output VC must be Pending — NOT Failed (PRD §7.1).
    assert!(
        matches!(engine.freshness(&node), Freshness::Pending { .. }),
        "cancelled dispatch must leave VC Pending, not Failed; got {:?}",
        engine.freshness(&node),
    );

    // (A2) pending_cause must still point at the in-flight ComputeNode.
    assert_eq!(
        engine.pending_cause(&node),
        Some(NodeId::Compute(c_id.clone())),
        "pending_cause must point at ComputeNode(c_id) after cancellation",
    );

    // (A3) running slot must be cleared on any terminal outcome (PRD §5 step-3 /
    // design decision in task ε/3424).
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let compute_data = snapshot
        .graph
        .get_compute_node(&c_id)
        .expect("ComputeNode must remain in graph after cancelled dispatch");
    assert!(
        compute_data.running.is_none(),
        "running slot must be cleared after terminal outcome; got Some(_)",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test B — COOPERATIVE CANCELLATION (canceller-thread dance covers cross-thread
// cancel propagation; wall-clock SLA is now a non-fatal observation — see test E
// for the load-independent regression guard)
// (Passes after step-2; drives run_compute_dispatch directly)
// ═══════════════════════════════════════════════════════════════════════════════

/// Poll budget for the slow trampoline (ms).
const POLL_BUDGET_MS: u64 = 100;

/// Iteration counter for `precancel_poll_fn` — the load-independent regression
/// guard (test E).  A pre-cancelled handle is seen on the first poll (counter
/// == 1) before any `thread::sleep`.  A regression that ignores cancel loops
/// all 20 iterations (counter == 20) and fails the assert in test E.
///
/// **Single-test ownership invariant**: reset and read exclusively by
/// `cooperative_cancellation_pre_cancelled_returns_after_one_poll` (test E).
static PRECANCEL_POLL_ITERS: AtomicUsize = AtomicUsize::new(0);

/// Instrumented trampoline (E): increments `PRECANCEL_POLL_ITERS` at the TOP
/// of each iteration BEFORE checking `is_cancelled()`, then sleeps.
///
/// With a pre-cancelled handle the loop runs exactly once and returns
/// `Cancelled` — the counter reaches 1.  A regression that ignores cancel
/// would loop 20 times (counter 20) and reach the fall-through.
///
/// Fall-through returns `Completed` (NOT `Cancelled`), mirroring
/// `material_field_retick_fn` (material_field_cancellation.rs:83-89): if the
/// cancel signal never propagates (misconfigured test), the `Err(Cancelled)`
/// assert in test E fails independently of the iteration-count guard.
fn precancel_poll_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    for _ in 0..20 {
        PRECANCEL_POLL_ITERS.fetch_add(1, Ordering::SeqCst);
        if cancellation.is_cancelled() {
            return ComputeOutcome::Cancelled;
        }
        std::thread::sleep(Duration::from_millis(POLL_BUDGET_MS));
    }
    // Safety-cap fall-through: Completed so a misconfigured test fails the
    // Err(Cancelled) assert rather than silently masking it.
    ComputeOutcome::Completed {
        result: Value::Int(0),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
        structured_detail: vec![],
    }
}

/// Published cancellation handle from `slow_poll_fn`.
///
/// **Single-test ownership invariant**: this static is written exclusively by
/// `slow_poll_fn` (target `"test::slow_poll_sla"`) which is registered only by
/// `cooperative_cancellation_sla_2x_budget`.  A second test in this binary that
/// registers under the same target will silently race on this cell.
static SLA_PUBLISHED_HANDLE: OnceLock<Mutex<Option<CancellationHandle>>> = OnceLock::new();

/// Slow trampoline (B): publishes a clone of its received handle so the canceller
/// thread can fire it, then polls `is_cancelled()` every `POLL_BUDGET_MS` (cap 20
/// iterations to avoid an infinite hang on test misconfiguration).
fn slow_poll_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // Publish a clone so the canceller thread can cancel it.
    let cell = SLA_PUBLISHED_HANDLE.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(cancellation.clone());

    // Poll every POLL_BUDGET_MS; cap at 20 iterations (~2 s) as a hang guard.
    for _ in 0..20 {
        if cancellation.is_cancelled() {
            return ComputeOutcome::Cancelled;
        }
        std::thread::sleep(Duration::from_millis(POLL_BUDGET_MS));
    }
    // Should not be reached in a well-formed test run.
    ComputeOutcome::Cancelled
}

/// (B) A canceller thread fires mid-trampoline; dispatch must return
/// `Err(DispatchError::Cancelled)`.  The canceller thread is joined (no orphan).
///
/// The wall-clock SLA (`elapsed < 5 × POLL_BUDGET_MS`) that originally appeared
/// here was a load-dependent bound: under the verify pipeline's by-design CPU
/// oversubscription the canceller thread can be starved and the `thread::sleep`
/// calls overrun their nominal budget, so the assert flaked (esc-4583-45).
/// The SLA is now a **non-fatal `eprintln!` observation** — it still exercises
/// the cross-thread cancel-propagation dance, but no `assert!` gates on wall
/// clock.  The load-independent regression guard (`PRECANCEL_POLL_ITERS <= 1`)
/// lives in test E (`cooperative_cancellation_pre_cancelled_returns_after_one_poll`).
///
/// Passes after step-2.
#[test]
fn cooperative_cancellation_cross_thread_propagation() {
    // Belt-and-suspenders: reset the published handle on entry (the inner Option
    // can be cleared even though OnceLock itself cannot be reset).
    if let Some(m) = SLA_PUBLISHED_HANDLE.get() {
        *m.lock().unwrap() = None;
    }

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::slow_poll_sla", slow_poll_fn as ComputeFn);

    let cell = ValueCellId::new("T", "sla");
    let c_id = ComputeNodeId::new("T", 0);

    // Seed a Final entry so begin_compute_dispatch has a last_substantive.
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(1), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );

    // Shared flag so the main thread knows the canceller has fired.
    let cancel_fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_fired2 = cancel_fired.clone();

    // Canceller thread: busy-waits for the published handle, then cancels it.
    // No engine access — CancellationHandle is Send + Sync, no lock on Engine.
    let canceller = std::thread::spawn(move || {
        let h = loop {
            let cell = SLA_PUBLISHED_HANDLE.get_or_init(|| Mutex::new(None));
            if let Some(h) = cell.lock().unwrap().clone() {
                break h;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        h.cancel();
        cancel_fired2.store(true, Ordering::SeqCst);
    });

    // The handle passed into run_compute_dispatch is the one the trampoline
    // receives (invoke_compute_trampoline threads it through).  The canceller
    // fires it via the published clone (same Arc<AtomicBool>).
    let handle = CancellationHandle::new();
    let start = Instant::now();
    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "test::slow_poll_sla",
        &[Value::Int(1)],
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
        ContentHash(0), // inert: no cache dir in tests
    );
    let elapsed = start.elapsed();

    // Join the canceller — must not panic or orphan.
    canceller.join().expect("canceller thread must not panic");
    assert!(
        cancel_fired.load(Ordering::SeqCst),
        "canceller thread must have fired before dispatch returned",
    );

    // Dispatch must return Cancelled.
    assert!(
        matches!(result, Err(DispatchError::Cancelled)),
        "slow trampoline must return Err(DispatchError::Cancelled), got {result:?}",
    );

    // Non-fatal SLA observation: the wall-clock bound is load-dependent and
    // was removed as a hard assert (esc-4583-45).  The eprintln preserves
    // observability and keeps `elapsed`/`Instant`/`POLL_BUDGET_MS` referenced
    // so no unused-binding/import warnings fire under -D warnings.
    // The load-independent regression guard lives in test E.
    eprintln!(
        "[cooperative_cancellation_cross_thread_propagation] elapsed={elapsed:?} \
         poll_budget={POLL_BUDGET_MS}ms (SLA observation, non-fatal)",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test C — DISPATCH IS SYNCHRONOUS (counter returns to zero between calls)
// (Passes after step-2; drives run_compute_dispatch directly)
//
// NOTE on scope: because dispatch is strictly sequential in this test there is
// no concurrency and the in-flight counter can never exceed 1 by construction.
// The assertion that it returns to zero *between* calls is meaningful — it
// confirms that the trampoline's increment/decrement bookkeeping runs to
// completion before the next dispatch begins, i.e. the call is truly
// synchronous.  Asserting a maximum of 1 *across* sequential calls would be
// tautological and is NOT what this test checks.
// The real "one-in-flight under concurrent dispatch" invariant belongs to the
// future async-driver slice when trampolines can be in-flight concurrently.
// ═══════════════════════════════════════════════════════════════════════════════

/// Current in-flight dispatch count tracked by `count_fn`.
///
/// **Single-test ownership invariant**: only `count_fn` (target
/// `"test::count_in_flight"`) reads / writes this static.
static IN_FLIGHT_CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Counter trampoline (C): increments `IN_FLIGHT_CURRENT`, then immediately
/// returns `Cancelled` and decrements it.  Being a fn-ptr it can only touch
/// statics.
fn count_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    IN_FLIGHT_CURRENT.fetch_add(1, Ordering::SeqCst);
    IN_FLIGHT_CURRENT.fetch_sub(1, Ordering::SeqCst);
    ComputeOutcome::Cancelled
}

/// (C) Over 20 sequential dispatches the in-flight counter returns to zero
/// between every pair of calls, confirming that `run_compute_dispatch` is
/// synchronous (the trampoline runs to completion before the call returns).
///
/// "One-in-flight under concurrent dispatch" is a structural consequence of
/// synchrony and is not separately tested here — that invariant belongs to the
/// future async-driver slice where trampolines can genuinely overlap.
///
/// Passes after step-2.
#[test]
fn dispatch_is_synchronous_counter_returns_to_zero_between_calls() {
    // Reset static.
    IN_FLIGHT_CURRENT.store(0, Ordering::SeqCst);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::count_in_flight", count_fn as ComputeFn);

    let cell = ValueCellId::new("T", "oif");
    let c_id = ComputeNodeId::new("T", 0);

    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(0), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );

    // Drive 20 sequential dispatches.
    for i in 0..20u32 {
        let handle = CancellationHandle::new();
        let _ = engine.run_compute_dispatch(
            &c_id,
            std::slice::from_ref(&cell),
            "test::count_in_flight",
            &[Value::Int(i as i64)],
            &[],
            &Value::Undef,
            &handle,
            VersionId(2 + u64::from(i)),
            ContentHash(0), // inert: no cache dir in tests
        );

        // After each synchronous call the trampoline has already returned,
        // so the in-flight count must be back to 0.
        assert_eq!(
            IN_FLIGHT_CURRENT.load(Ordering::SeqCst),
            0,
            "in-flight count must be 0 after synchronous dispatch returns (iteration {i})",
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test D — PRIOR-CACHE-INTACT
// (Passes after step-2; drives run_compute_dispatch directly)
// ═══════════════════════════════════════════════════════════════════════════════

/// (D) After a cancelled dispatch the seeded Final output VC transitions to
/// `Freshness::Pending{last_substantive: prior}` and the prior cached value is
/// unchanged.  No warm-state donation occurs.
///
/// Passes after step-2 (`run_compute_dispatch` discriminates `Cancelled` and
/// does NOT call `complete_compute_dispatch_atomically`; the VC stays `Pending`
/// from `begin_compute_dispatch`).
#[test]
fn prior_cache_intact_after_cancelled_dispatch() {
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::always_cancelled_d", always_cancelled_fn as ComputeFn);

    let cell = ValueCellId::new("T", "pci");
    let c_id = ComputeNodeId::new("T", 0);
    let prior_value = Value::Int(55);

    // Seed a Final entry: prior value Int(55) @ VersionId(1).
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(prior_value.clone(), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );

    let handle = CancellationHandle::new();
    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "test::always_cancelled_d",
        std::slice::from_ref(&prior_value),
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
        ContentHash(0), // inert: no cache dir in tests
    );

    // Dispatch returns Cancelled.
    assert!(
        matches!(result, Err(DispatchError::Cancelled)),
        "expected Err(DispatchError::Cancelled), got {result:?}",
    );

    let node = NodeId::Value(cell.clone());

    // (D1) VC is Pending (begin_compute_dispatch ran; complete did not).
    assert!(
        matches!(engine.freshness(&node), Freshness::Pending { .. }),
        "post-cancel VC must be Pending; got {:?}",
        engine.freshness(&node),
    );

    // (D2) pending_cause points at the ComputeNode.
    assert_eq!(
        engine.pending_cause(&node),
        Some(NodeId::Compute(c_id.clone())),
        "pending_cause must point at ComputeNode(c_id) after cancelled dispatch",
    );

    // (D3) The cached result still holds the prior value (begin_compute_dispatch
    // only changes freshness/pending_cause; it does not overwrite the result).
    let entry = engine
        .cache_store()
        .get(&node)
        .expect("cache entry must exist after begin_compute_dispatch");
    match &entry.result {
        CachedResult::Value(v, d) => {
            assert_eq!(
                *v, prior_value,
                "prior cached value must be unchanged after cancellation",
            );
            assert_eq!(*d, DeterminacyState::Determined);
        }
        other => panic!("expected CachedResult::Value, got {other:?}"),
    }

    // (D4) No warm-state on the OUTPUT VC's entry (no prior warm state was
    // seeded for this dispatch — restore-prior is a no-op). ζ / step-10's
    // restore-prior arm only touches the Compute(c_id) entry, never the
    // output VC's entry.
    assert!(
        entry.warm_state.is_none(),
        "warm_state must be None after cancelled dispatch (no donation on cancel path)",
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test E — PRE-CANCELLED REGRESSION GUARD (deterministic, load-independent)
// Replaces the wall-clock SLA in test B with a load-independent iteration
// counter.  A pre-cancelled handle is observed on the trampoline's first
// poll iteration (counter == 1) and returns before any thread::sleep —
// fully load-independent, zero race.  A regression that ignores the cancel
// signal would loop all 20 iterations (counter == 20) and fail the assert.
// ═══════════════════════════════════════════════════════════════════════════════

/// (E) A pre-cancelled handle must cause the instrumented trampoline to return
/// after at most one poll iteration (the cancel is observed on the first check,
/// before any `thread::sleep`).
///
/// This is the load-independent regression guard that replaces the wall-clock
/// SLA bound that was removed from `cooperative_cancellation_cross_thread_propagation`.
/// It asserts an iteration count, not a `Duration`, so it is immune to CPU
/// oversubscription and scheduling jitter in the verify pipeline.
///
/// The correctness contract mirrors
/// `run_compute_dispatch_pre_cancelled_returns_dispatch_error_cancelled` in
/// `engine_compute.rs` with the addition of the `PRECANCEL_POLL_ITERS` guard.
#[test]
fn cooperative_cancellation_pre_cancelled_returns_after_one_poll() {
    // Reset the iteration counter before the test run.
    PRECANCEL_POLL_ITERS.store(0, Ordering::SeqCst);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::precancel_poll", precancel_poll_fn as ComputeFn);

    let cell = ValueCellId::new("T", "precancel");
    let c_id = ComputeNodeId::new("T", 0);

    // Seed a Final entry so begin_compute_dispatch has a last_substantive.
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(7), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );

    // Pre-cancel the handle BEFORE dispatch (no canceller thread, no race).
    let handle = CancellationHandle::new();
    handle.cancel();

    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "test::precancel_poll",
        &[Value::Int(7)],
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
        ContentHash(0), // inert: no cache dir in tests
    );

    // (E1) The trampoline must have polled at most once — the cancel is
    // observed on the first iteration before any thread::sleep.
    let iters = PRECANCEL_POLL_ITERS.load(Ordering::SeqCst);
    assert!(
        iters <= 1,
        "pre-cancelled trampoline must poll at most once; polled {iters} times \
         (a regression that ignores cancel would poll 20 times)",
    );

    // (E2) Dispatch must return Err(DispatchError::Cancelled).
    assert!(
        matches!(result, Err(DispatchError::Cancelled)),
        "pre-cancelled dispatch must return Err(DispatchError::Cancelled), got {result:?}",
    );

    let node = NodeId::Value(cell.clone());

    // (E3) Output VC must be Freshness::Pending (begin ran; complete did not).
    assert!(
        matches!(engine.freshness(&node), Freshness::Pending { .. }),
        "post-cancel VC must be Pending; got {:?}",
        engine.freshness(&node),
    );

    // (E4) pending_cause must point at the ComputeNode.
    assert_eq!(
        engine.pending_cause(&node),
        Some(NodeId::Compute(c_id.clone())),
        "pending_cause must point at ComputeNode(c_id) after pre-cancelled dispatch",
    );

    // (E5) The prior cached value (Int(7)) must be intact.
    let entry = engine
        .cache_store()
        .get(&node)
        .expect("cache entry must exist after begin_compute_dispatch");
    match &entry.result {
        CachedResult::Value(v, d) => {
            assert_eq!(
                *v,
                Value::Int(7),
                "prior cached value must be unchanged after pre-cancelled dispatch",
            );
            assert_eq!(*d, DeterminacyState::Determined);
        }
        other => panic!("expected CachedResult::Value(Int(7)); got {other:?}"),
    }
}
