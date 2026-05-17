//! Integration tests for task δ (3423): Freshness::Pending integration during
//! in-flight ComputeNode dispatch + atomic completion.
//!
//! See `docs/prds/v0_3/compute-node-contract.md` §3 (atomic completion) and
//! §8 task δ. This is the δ engine-flow test surface; it joins the γ surface
//! `compute_dispatch_registry.rs` and reuses the same `compute_identity.ri`
//! fixture for the end-to-end lowering test.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{
    ComputeNodeId, DeterminacyState, Freshness, OpaqueState, Value, ValueCellId, VersionId,
};

// ── step-8: RED — run_compute_dispatch begin→trampoline→complete pipeline ─────

/// Number of times `observing_fn` was invoked. Only `observing_fn` (target
/// "test::observer", used by exactly one test) touches this static, so the
/// count is not contaminated by other tests in this binary.
static INVOCATION_COUNT: AtomicUsize = AtomicUsize::new(0);

/// The `value_inputs[0]` the trampoline observed on its single invocation.
static OBSERVED_INPUTS: OnceLock<Mutex<Option<Value>>> = OnceLock::new();

/// Synthetic trampoline: records its invocation and observed input, then
/// returns `Int(n + 1)` so the test can distinguish the trampoline's output
/// from the seeded prior value.
fn observing_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    INVOCATION_COUNT.fetch_add(1, Ordering::SeqCst);
    let input = value_inputs[0].clone();
    *OBSERVED_INPUTS
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(input.clone());
    let result = match input {
        Value::Int(n) => Value::Int(n + 1),
        other => other,
    };
    ComputeOutcome::Completed {
        result,
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

/// The combined begin → invoke trampoline → atomic-complete pipeline is pinned
/// by this single test (PRD §3 / §8 task δ). `run_compute_dispatch` does not
/// yet exist — RED (compilation failure) until step-9.
#[test]
fn run_compute_dispatch_helper_invokes_begin_then_trampoline_then_atomic_complete() {
    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::observer", observing_fn as ComputeFn);

    let b = ValueCellId::new("T", "b");
    let c_id = ComputeNodeId::new("T", 0);

    // Seed a Final entry for the output VC: Value::Int(41) @ VersionId(1).
    engine.cache_store_mut().put(
        NodeId::Value(b.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(41), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );

    let outcome = engine.run_compute_dispatch(
        &c_id,
        &[b.clone()],
        "test::observer",
        &[Value::Int(41)],
        &[],
        &Value::Undef,
        None,
        VersionId(2),
    );

    // (a) Ok with the trampoline's result (41 + 1 = 42).
    let (result, diagnostics) =
        outcome.expect("run_compute_dispatch must return Ok for a Completed trampoline");
    assert_eq!(
        result,
        Value::Int(42),
        "helper must surface the trampoline's result (41 + 1)"
    );
    assert!(
        diagnostics.is_empty(),
        "observing trampoline emits no diagnostics, got: {diagnostics:?}"
    );

    // (b) The trampoline ran exactly once.
    assert_eq!(
        INVOCATION_COUNT.load(Ordering::SeqCst),
        1,
        "trampoline must run exactly once"
    );

    // (c) The trampoline saw the supplied input.
    assert_eq!(
        *OBSERVED_INPUTS.get().unwrap().lock().unwrap(),
        Some(Value::Int(41)),
        "trampoline must observe value_inputs[0] == Int(41)"
    );

    // (d) Post-dispatch cache state: (Final, new value, no cause), version stamped.
    let node = NodeId::Value(b.clone());
    assert_eq!(
        engine.freshness(&node),
        Freshness::Final,
        "post-dispatch freshness must be Final"
    );
    assert_eq!(
        engine.pending_cause(&node),
        None,
        "post-dispatch pending_cause must be cleared"
    );
    let entry = engine
        .cache_store()
        .get(&node)
        .expect("output VC cache entry must exist after dispatch");
    match &entry.result {
        CachedResult::Value(v, d) => {
            assert_eq!(*v, Value::Int(42), "cache must hold the trampoline result");
            assert_eq!(*d, DeterminacyState::Determined);
        }
        other => panic!("expected CachedResult::Value, got {other:?}"),
    }
    assert_eq!(
        entry.basis_version,
        VersionId(2),
        "complete must stamp the supplied VersionId(2)"
    );
}

// ── step-10: RED — e2e @optimized eval pins the Pending lifecycle ─────────────

/// Identity trampoline for the e2e fixture: returns `value_inputs[0]` verbatim.
fn identity_fn(
    value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    _cancellation: &CancellationHandle,
) -> ComputeOutcome {
    ComputeOutcome::Completed {
        result: value_inputs[0].clone(),
        new_warm_state: None,
        cost_per_byte: None,
        diagnostics: vec![],
    }
}

/// End-to-end: a full `@optimized` fn eval routes through the lowering site.
/// After eval, NO Pending lingers on the output VC — the strongest
/// user-observable δ signal (PRD §3 / §8 task δ). Exercises the @optimized
/// lowering site in engine_eval.rs; step-11 refactors it to use
/// `run_compute_dispatch`.
#[test]
fn e2e_optimized_fn_eval_pins_pending_lifecycle_via_run_compute_dispatch() {
    let source = include_str!("fixtures/compute_identity.ri");
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("test::identity", identity_fn as ComputeFn);

    let eval_result = engine.eval(&compiled);

    let result_cell = ValueCellId::new("IdentityFixture", "result");
    let node = NodeId::Value(result_cell.clone());

    // (c) Observable cell value == the trampoline's identity-returned input.
    assert_eq!(
        eval_result.values.get(&result_cell),
        Some(&Value::Int(42)),
        "IdentityFixture.result must equal the trampoline identity output"
    );

    // (a) After eval the output VC is Final — no in-flight Pending lingers.
    assert_eq!(
        engine.freshness(&node),
        Freshness::Final,
        "post-eval freshness must be Final (no in-flight Pending lingers)"
    );

    // (b) No diagnostic chain root remains.
    assert_eq!(
        engine.pending_cause(&node),
        None,
        "post-eval pending_cause must be cleared"
    );

    // (c') The cache entry holds Int(42).
    let entry = engine
        .cache_store()
        .get(&node)
        .expect("output VC cache entry must exist after eval");
    match &entry.result {
        CachedResult::Value(v, _) => assert_eq!(
            *v,
            Value::Int(42),
            "cache must hold the trampoline result Int(42)"
        ),
        other => panic!("expected CachedResult::Value, got {other:?}"),
    }

    // (d) The graph contains a ComputeNode with target "test::identity".
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    assert!(
        snapshot
            .graph
            .compute_nodes
            .iter()
            .any(|(_, d)| d.target == "test::identity"),
        "graph must contain a ComputeNode with target \"test::identity\""
    );
}
