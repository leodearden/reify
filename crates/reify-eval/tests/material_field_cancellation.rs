//! Integration test ε/3781 row 4: material-field retick cancellation.
//!
//! Exercises the dispatch-layer cancellation contract (ε/3424) in the context
//! of a material-field retick scenario — the "rapid retick" that supersedes an
//! in-flight FEA solve (e.g. a field parameter change arriving before the prior
//! solve has finished).
//!
//! # Note on the real FEA trampoline
//!
//! `solve_elastic_static_trampoline` runs a synchronous, single-threaded CG
//! solve in `SolverMode::Deterministic` and ignores its `&CancellationHandle`.
//! It therefore never returns `ComputeOutcome::Cancelled` and spawns no solver
//! threads — making "no orphaned solver threads" structural, not testable via
//! the real trampoline. The generic dispatch-layer contract (cancelled → Pending,
//! prior-cache-intact, canceller joins, no warm-state donated) is owned by
//! `engine_compute.rs` and is exercised here via a **synthetic cooperative
//! trampoline**, exactly as ε/3424 tests B + D do.
//!
//! Expected GREEN on write against the shipped dispatch harness.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use reify_core::{ComputeNodeId, ContentHash, ValueCellId, VersionId};
use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{
    CancellationHandle, ComputeFn, ComputeOutcome, DispatchError, RealizationReadHandle,
};
use reify_ir::{DeterminacyState, Freshness, OpaqueState, Value};
use reify_test_support::make_simple_engine;

// ─────────────────────────────────────────────────────────────────────────────
// Synthetic cooperative trampoline — mirrors the slow_poll_fn pattern from
// cancellation_compute_dispatch.rs, specialised to the material-field-retick
// scenario.
// ─────────────────────────────────────────────────────────────────────────────

/// Poll granularity for the synthetic trampoline (ms).
/// SLA assertion uses ≤5× this value to give scheduling headroom on loaded CI.
const RETICK_POLL_MS: u64 = 100;

/// Published handle from `material_field_retick_fn`.
///
/// **Single-test ownership invariant**: written exclusively by
/// `material_field_retick_fn` (target `"test::material_field_retick_fea_solve"`),
/// registered only by `material_field_retick_cancel_keeps_prior_fea_cache_intact_without_orphaned_threads`.
static RETICK_HANDLE: OnceLock<Mutex<Option<CancellationHandle>>> = OnceLock::new();

/// Synthetic FEA-solve trampoline representing an in-flight solve that can be
/// cancelled by a material-field retick.
///
/// Publishes a clone of its received `CancellationHandle`, then polls
/// `is_cancelled()` every `RETICK_POLL_MS` ms (cap 20 iterations to guard
/// against infinite hang if the test is misconfigured).
fn material_field_retick_fn(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    // Publish a clone so the canceller thread can fire it.
    let cell = RETICK_HANDLE.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(cancellation.clone());

    // Cooperative poll loop (cap at 20 to prevent infinite hang).
    for _ in 0..20 {
        if cancellation.is_cancelled() {
            return ComputeOutcome::Cancelled;
        }
        std::thread::sleep(Duration::from_millis(RETICK_POLL_MS));
    }
    // Safety-cap fall-through: return a Completed sentinel, NOT Cancelled.
    //
    // A correctly-formed run always fires cancel() within the poll loop above.
    // If the cancel signal never propagates (misconfigured test), returning
    // Completed here causes dispatch to return Ok(_), which immediately fails
    // the `matches!(result, Err(DispatchError::Cancelled))` assertion —
    // independently of the SLA timing check.  The previous Cancelled return
    // would have silently masked the misconfiguration.
    ComputeOutcome::Completed {
        result: Value::Int(0),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test
// ─────────────────────────────────────────────────────────────────────────────

/// PRD ε/3781 row 4 primary signal.
///
/// # Scenario
///
/// A material-field retick fires `cancel()` on the dispatch handle while the
/// synthetic FEA-solve trampoline is in-flight.  The dispatch harness must:
///
/// **(a)** Return `Err(DispatchError::Cancelled)` within 5× poll budget
///        (cooperative, no hang).
///
/// **(b)** The canceller thread joins cleanly (no orphaned thread).
///
/// **(c) PRIOR-CACHE-INTACT**: output VC transitions to
///        `Freshness::Pending{last_substantive: prior}`; the seeded Final
///        prior FEA-result value is unchanged in the cache; no warm-state
///        donated on the cancel path.
///
/// The prior FEA-result is represented by a sentinel `Value::Int(99)`.
/// The dispatch harness treats the cached value opaquely, so a sentinel
/// exercises the same code path as a real `ElasticResult` StructureInstance.
///
/// Directly reuses the ε/3424 test B (cooperative SLA) + test D (prior-cache-
/// intact) pattern, specialised to the material-field-retick named scenario
/// so it is discoverable as regression coverage for ε/3781 row 4.
#[test]
fn material_field_retick_cancel_keeps_prior_fea_cache_intact_without_orphaned_threads() {
    // Belt-and-suspenders: reset the published handle on test entry (inner
    // Option cleared even though OnceLock cannot be reset).
    if let Some(m) = RETICK_HANDLE.get() {
        *m.lock().unwrap() = None;
    }

    let mut engine = make_simple_engine();
    engine.register_compute_fn(
        "test::material_field_retick_fea_solve",
        material_field_retick_fn as ComputeFn,
    );

    let cell = ValueCellId::new("MaterialFieldFea", "result");
    let c_id = ComputeNodeId::new("MaterialFieldFea", 0);

    // Sentinel prior FEA-result: represents a Final ElasticResult from a
    // previous solve that must survive the cancelled retick dispatch.
    let prior_value = Value::Int(99);

    // Seed a Final cache entry so begin_compute_dispatch records last_substantive.
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(prior_value.clone(), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );

    // Shared flag: confirms the canceller fired before dispatch returned.
    let cancel_fired = Arc::new(AtomicBool::new(false));
    let cancel_fired2 = cancel_fired.clone();

    // Canceller thread: represents the "rapid retick" superseding the in-flight
    // FEA solve.  Waits for the published handle then fires .cancel().
    // No engine access — CancellationHandle is Send+Sync (no lock on Engine).
    let canceller = std::thread::spawn(move || {
        let handle = loop {
            let cell = RETICK_HANDLE.get_or_init(|| Mutex::new(None));
            if let Some(h) = cell.lock().unwrap().clone() {
                break h;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        handle.cancel();
        cancel_fired2.store(true, Ordering::SeqCst);
    });

    // The handle passed into run_compute_dispatch is the one the trampoline
    // receives; the canceller fires it via the published clone (same Arc).
    let handle = CancellationHandle::new();
    let start = Instant::now();
    let result = engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "test::material_field_retick_fea_solve",
        // value_inputs: material-field-shaped sentinel (Int(1))
        &[Value::Int(1)],
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
        ContentHash(0), // inert: no cache dir in tests
    );
    let elapsed = start.elapsed();

    // ── (b) Canceller joins without orphaning ─────────────────────────────────
    canceller.join().expect("canceller thread must not panic");
    assert!(
        cancel_fired.load(Ordering::SeqCst),
        "canceller thread must have fired before dispatch returned",
    );

    // ── (a) Cooperative SLA ≤5× poll budget ──────────────────────────────────
    assert!(
        matches!(result, Err(DispatchError::Cancelled)),
        "material-field retick must return Err(DispatchError::Cancelled), got {result:?}",
    );
    let sla = Duration::from_millis(RETICK_POLL_MS * 5);
    assert!(
        elapsed < sla,
        "dispatch wall-clock ({elapsed:?}) exceeded 5× poll budget ({sla:?}); \
         trampoline did not poll cooperatively",
    );

    // ── (c) Prior-cache-intact ────────────────────────────────────────────────
    let node = NodeId::Value(cell.clone());

    // VC must be Pending (begin_compute_dispatch ran; complete_compute_dispatch
    // did NOT — cancel path skips the complete step).
    assert!(
        matches!(engine.freshness(&node), Freshness::Pending { .. }),
        "post-cancel VC must be Freshness::Pending; got {:?}",
        engine.freshness(&node),
    );

    // pending_cause must point at the ComputeNode.
    assert_eq!(
        engine.pending_cause(&node),
        Some(NodeId::Compute(c_id.clone())),
        "pending_cause must point at ComputeNode(c_id) after cancelled dispatch",
    );

    // The cached result must still hold the seeded prior FEA-result value.
    let entry = engine
        .cache_store()
        .get(&node)
        .expect("cache entry must exist after begin_compute_dispatch");

    match &entry.result {
        CachedResult::Value(v, d) => {
            assert_eq!(
                *v, prior_value,
                "prior FEA-result cache value must be unchanged after cancellation \
                 (prior={prior_value:?}, got {v:?})",
            );
            assert_eq!(
                *d,
                DeterminacyState::Determined,
                "DeterminacyState must be Determined (unchanged from seeded entry)",
            );
        }
        other => panic!("expected CachedResult::Value, got {other:?}"),
    }

    // No warm-state donation on the cancel path (complete_compute_dispatch_atomically
    // was never called, so no new warm state was installed).
    assert!(
        entry.warm_state.is_none(),
        "warm_state must be None after cancelled dispatch (no donation on cancel path)",
    );
}
