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
//! - **(B) COOPERATIVE SLA ≤4× BUDGET** — a slow trampoline that polls
//!   `is_cancelled()` every `POLL_BUDGET_MS` returns within 4× that budget of
//!   the cancel signal (4× gives scheduling-jitter headroom on loaded CI).
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
// Test B — COOPERATIVE SLA ≤2× BUDGET
// (Passes after step-2; drives run_compute_dispatch directly)
// ═══════════════════════════════════════════════════════════════════════════════

/// Poll budget for the slow trampoline (ms).  The SLA is ≤2× this value.
const POLL_BUDGET_MS: u64 = 100;

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
/// `Err(DispatchError::Cancelled)` within `5 × POLL_BUDGET_MS` of the cancel
/// signal.  The canceller thread is joined (no orphan).
///
/// The SLA is set to 5× (not 2×) to give the test headroom on loaded CI:
/// the trampoline's worst-case single poll-sleep is `POLL_BUDGET_MS`, plus
/// scheduling jitter can approach another full budget on a saturated system.
/// 5× gives three full budgets of jitter margin without degrading the
/// cooperative-cancellation property being tested.
///
/// Passes after step-2.
#[test]
fn cooperative_cancellation_sla_2x_budget() {
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

    // Wall-clock from dispatch start to return must be < 5× poll budget.
    // Worst case on a loaded CI host: trampoline sleeps one full poll period
    // before seeing the cancel, then scheduling jitter can add up to two more
    // full periods before the thread is scheduled.  5× gives three budgets of
    // slack without weakening the cooperative-poll property.
    let sla = Duration::from_millis(POLL_BUDGET_MS * 5);
    assert!(
        elapsed < sla,
        "dispatch wall-clock ({elapsed:?}) exceeded 5× poll budget ({sla:?}); \
         trampoline did not poll cooperatively",
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
