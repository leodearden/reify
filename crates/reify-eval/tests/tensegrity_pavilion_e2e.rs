//! Tensegrity-membrane θ — pavilion end-to-end integration gate (#4419).
//!
//! PRD: `docs/prds/v0_6/tensegrity-membrane.md` §8 / §9 (task θ).
//!
//! This is the M3 integration gate: the first `.ri` artifact that BOTH
//! form-finds (combined `solver::form_find_free`, task δ/#4415) AND carries
//! load (combined `solver::membrane_load`, task η/#4418). The example
//! `examples/tensegrity_pavilion.ri` is the user-observable θ signal.
//!
//! Test layers (step plan):
//!
//! **Form-find layer (steps 3→4)**
//!   (a) e2e — no Error diagnostics; exactly one `solver::form_find_free`
//!       ComputeNode (not body-inlined); `FormFindResult.member_forces` +
//!       `surface_stresses` non-empty; `converged == true`.
//!   (b) viewport signal — `tensegrity_surfaces(net)` emits facets all tagged
//!       `kind: "membrane"`.
//!   (c) cache-hit — counting wrapper around `solve_form_find_free_trampoline`;
//!       second eval of the same module hits the Final-gate (count stays 1);
//!       a perturbed-sigma variant re-dispatches (count increments to 2).
//!   (d) cancellation — cooperative-cancel wrapper around
//!       `solve_form_find_free_trampoline`; mid-trampoline cancellation leaves
//!       the form_find_free VC `Freshness::Pending` (NOT `Failed`) within
//!       `5 × CANCEL_POLL_MS` of the cancel signal; prior cached value intact.
//!
//! **Load layer (steps 5→6)** — added in the load step; these asserts will
//! appear below once step-6 adds the `membrane_load` call to the pavilion.
//!
//! RED signal (step-3): `include_str!` of the missing
//! `examples/tensegrity_pavilion.ri` is a **compile-time error** — the test
//! binary does not build, so all tests in this file fail RED until step-4
//! creates the example file.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use reify_core::{ComputeNodeId, DimensionVector, Severity, ValueCellId, VersionId};
use reify_eval::cache::{CachedResult, NodeCache, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_ir::{DeterminacyState, Freshness, OpaqueState, PersistentMap, StructureInstanceData,
               StructureTypeId, Value};
use reify_test_support::{collect_errors, compile_source_with_stdlib, make_simple_engine};

// ── pavilion source ───────────────────────────────────────────────────────────

/// The committed pavilion example.
///
/// `include_str!` makes a *missing* file a **compile-time error** — this is
/// the step-3 RED signal: the test binary refuses to compile until step-4
/// creates `examples/tensegrity_pavilion.ri`.
fn pavilion_source() -> &'static str {
    include_str!("../../../examples/tensegrity_pavilion.ri")
}

// ── value-construction helpers ────────────────────────────────────────────────

fn length(m: f64) -> Value {
    Value::Scalar { si_value: m, dimension: DimensionVector::LENGTH }
}

fn real(r: f64) -> Value {
    Value::Real(r)
}

fn node(x: f64, y: f64, z: f64) -> Value {
    Value::Point(vec![length(x), length(y), length(z)])
}

fn idx(i: i64) -> Value {
    Value::Int(i)
}

fn pair(a: i64, b: i64) -> Value {
    Value::List(vec![idx(a), idx(b)])
}

fn triple(a: i64, b: i64, c: i64) -> Value {
    Value::List(vec![idx(a), idx(b), idx(c)])
}

/// Build a minimal 6-node T-prism + 2-triangle membrane `Tensegrity` Value
/// (δ-compatible combined geometry, matching
/// `tensegrity_delta_combined_form_find_e2e.rs`).
///
/// This is used for the cancellation test where we need crafted value_inputs
/// to call `solve_form_find_free_trampoline` directly without going through
/// the full eval pipeline.
fn prism_with_membrane_tensegrity() -> Value {
    let nodes = Value::List(vec![
        node(1.0, 0.0, 1.0),          // 0: top A
        node(-0.5, 0.866, 1.0),       // 1: top B
        node(-0.5, -0.866, 1.0),      // 2: top C
        node(0.866, 0.5, -1.0),       // 3: bot A'
        node(-0.866, 0.5, -1.0),      // 4: bot B'
        node(0.0, -1.0, -1.0),        // 5: bot C'
    ]);
    let struts = Value::List(vec![pair(0, 4), pair(1, 5), pair(2, 3)]);
    let cables = Value::List(vec![
        pair(0, 1), pair(1, 2), pair(2, 0), // top triangle
        pair(3, 4), pair(4, 5), pair(5, 3), // bot triangle
        pair(0, 3), pair(1, 4), pair(2, 5), // verticals
    ]);
    let surfaces = Value::List(vec![triple(0, 1, 2), triple(3, 4, 5)]);
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), nodes),
        ("struts".to_string(), struts),
        ("cables".to_string(), cables),
        ("surfaces".to_string(), surfaces),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }))
}

/// Craft a 5-input `value_inputs` slice for `solver::form_find_free` on
/// the prism+membrane above. Mirrors the δ combined trampoline-unit payload.
///
/// group_ids: 3 struts→0, 9 cables→split by (horiz=1, vert=2)
/// seeds:     [-1.0, 1.0, 1.0]
/// reference_group: 1
/// surface_stresses: [σ, σ] (one per triangle, σ = 0.2)
fn prism_form_find_inputs(sigma: f64) -> Vec<Value> {
    let net = prism_with_membrane_tensegrity();
    // struts-then-cables: [0,0,0, 1,1,1, 1,1,1, 2,2,2]
    let group_ids = Value::List(vec![
        idx(0), idx(0), idx(0),  // 3 struts
        idx(1), idx(1), idx(1),  // top+bot horizontals (6)
        idx(1), idx(1), idx(1),
        idx(2), idx(2), idx(2),  // 3 verticals
    ]);
    let seeds = Value::List(vec![real(-1.0), real(1.0), real(1.0)]);
    let ref_group = Value::Int(1);
    let sigmas = Value::List(vec![real(sigma), real(sigma)]);
    vec![net, group_ids, seeds, ref_group, sigmas]
}

// ── (a) e2e: pavilion compiles, evals, combined form-find converges ───────────

/// (a) The pavilion compiles without Error-severity diagnostics, evals through
/// `register_compute_fns`, produces exactly one `solver::form_find_free`
/// ComputeNode (proof the @optimized call lowered, not body-inlined), and the
/// `FormFindResult` cell has `member_forces` + `surface_stresses` both
/// non-empty (combined D over lines + surfaces) and `converged == true`.
#[test]
fn pavilion_form_find_e2e_combined_dispatch_and_convergence() {
    let compiled = compile_source_with_stdlib(pavilion_source());

    // No Error diagnostics from the compile pipeline.
    let compile_errors = collect_errors(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "pavilion source must compile without Error diagnostics; got: {compile_errors:#?}"
    );

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // No Error diagnostics from eval either.
    let eval_errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "pavilion eval must produce no Error diagnostics; got: {eval_errors:#?}"
    );

    // Exactly one solver::form_find_free ComputeNode in the graph — proof of
    // @optimized lowering (not body-inlining).
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let form_find_free_nodes: Vec<_> = snapshot
        .graph
        .compute_nodes
        .iter()
        .filter(|(_, d)| d.target == "solver::form_find_free")
        .collect();
    assert_eq!(
        form_find_free_nodes.len(),
        1,
        "expected exactly one solver::form_find_free ComputeNode; found {form_find_free_nodes:?}"
    );

    // The FormFindResult cell must have member_forces (lines) and
    // surface_stresses (surfaces) both non-empty, and converged == true.
    let form_cell_id = eval_result
        .values
        .iter()
        .find(|(id, _)| id.member == "form")
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| {
            panic!(
                "no 'form' cell found in eval result; cells: {:?}",
                eval_result.values.iter().map(|(id, _)| id).collect::<Vec<_>>()
            )
        });
    let form = eval_result
        .values
        .get(&form_cell_id)
        .expect("form cell must be present");

    let data = match form {
        Value::StructureInstance(d) => d,
        other => panic!("'form' cell must be a FormFindResult StructureInstance; got {other:?}"),
    };
    assert_eq!(
        data.type_name, "FormFindResult",
        "form cell should be FormFindResult; got {:?}",
        data.type_name
    );

    // converged == true
    assert!(
        matches!(data.fields.get("converged"), Some(Value::Bool(true))),
        "form.converged must be true for a well-posed combined pavilion; \
         got {:?}",
        data.fields.get("converged")
    );

    // member_forces non-empty — proves lines contributed to the combined D
    let member_forces = data.fields.get("member_forces").unwrap_or_else(|| {
        panic!("FormFindResult.member_forces field missing")
    });
    let mf_len = match member_forces {
        Value::List(v) => v.len(),
        other => panic!("member_forces must be a List; got {other:?}"),
    };
    assert!(
        mf_len > 0,
        "FormFindResult.member_forces must be non-empty (lines in combined D)"
    );

    // surface_stresses non-empty — proves surfaces contributed to the combined D
    let surface_stresses = data.fields.get("surface_stresses").unwrap_or_else(|| {
        panic!("FormFindResult.surface_stresses field missing")
    });
    let ss_len = match surface_stresses {
        Value::List(v) => v.len(),
        other => panic!("surface_stresses must be a List; got {other:?}"),
    };
    assert!(
        ss_len > 0,
        "FormFindResult.surface_stresses must be non-empty (surfaces in combined D)"
    );
}

// ── (b) viewport signal: tensegrity_surfaces emits kind:"membrane" facets ─────

/// (b) The pavilion's `tensegrity_surfaces(net)` cell must emit a non-empty
/// `List<TensegritySurface>` where every facet has `kind == "membrane"`.
/// This is the exact producer-side signal that β's `surfaceManager.ts`
/// consumes for viewport rendering (β/#4413 owns the render channel; θ only
/// asserts the producer emits the correct kind tag).
#[test]
fn pavilion_surfaces_emit_membrane_kind_facets() {
    let compiled = compile_source_with_stdlib(pavilion_source());
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // Find the 'facets' cell (tensegrity_surfaces output).
    let facets_cell_id = eval_result
        .values
        .iter()
        .find(|(id, _)| id.member == "facets")
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| {
            panic!(
                "no 'facets' cell in eval result; cells: {:?}",
                eval_result.values.iter().map(|(id, _)| id).collect::<Vec<_>>()
            )
        });
    let facets = eval_result.values.get(&facets_cell_id).expect("facets cell present");

    let list = match facets {
        Value::List(v) => v,
        other => panic!("facets must be a List<TensegritySurface>; got {other:?}"),
    };
    assert!(
        !list.is_empty(),
        "tensegrity_surfaces(net) must emit at least one facet for a pavilion with surfaces"
    );

    for (i, facet) in list.iter().enumerate() {
        let data = match facet {
            Value::StructureInstance(d) => d,
            other => panic!("facets[{i}] must be a TensegritySurface StructureInstance; got {other:?}"),
        };
        assert_eq!(
            data.type_name, "TensegritySurface",
            "facets[{i}].type_name must be TensegritySurface; got {:?}",
            data.type_name
        );
        let kind = data.fields.get("kind").unwrap_or_else(|| {
            panic!("facets[{i}].kind field missing from TensegritySurface")
        });
        assert_eq!(
            kind,
            &Value::String("membrane".to_string()),
            "facets[{i}].kind must be \"membrane\" (the β surfaceManager viewport signal); got {kind:?}"
        );
    }
}

// ── (c) cache-hit: counting wrapper around form_find_free trampoline ──────────

/// Dispatch counter for the cache-hit test. Named PAVILION to avoid colliding
/// with the cable-net / membrane counters in tensegrity_t1a_form_find.rs when
/// tests run concurrently in the same process.
static PAVILION_DISPATCH_COUNT: AtomicU32 = AtomicU32::new(0);

/// Counting wrapper around `solve_form_find_free_trampoline`.
fn pavilion_counting_wrapper(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    PAVILION_DISPATCH_COUNT.fetch_add(1, Ordering::SeqCst);
    reify_eval::compute_targets::form_find::solve_form_find_free_trampoline(
        value_inputs,
        realization_inputs,
        options,
        prior_warm_state,
        cancellation,
    )
}

/// (c) Cache-hit: a second `eval()` of the same compiled pavilion must NOT
/// re-dispatch the `solver::form_find_free` trampoline — the Final-gate
/// (engine_eval.rs) short-circuits when all inputs and the output VC are
/// already Final. Additionally, evaluating an independent source with a
/// perturbed `surface_stresses` σ value causes a re-dispatch (different
/// cache key → new ComputeNode → count increments).
#[test]
fn pavilion_form_find_second_eval_hits_cache_perturbed_sigma_redispatches() {
    PAVILION_DISPATCH_COUNT.store(0, Ordering::SeqCst);

    let compiled = compile_source_with_stdlib(pavilion_source());
    let mut engine = make_simple_engine();
    engine.register_compute_fn(
        "solver::form_find_free",
        pavilion_counting_wrapper as ComputeFn,
    );
    // Register the real membrane_load trampoline so the pavilion's membrane_load
    // call dispatches successfully (no "no registered compute trampoline" Error
    // diagnostic). Only form_find_free dispatch is counted; membrane_load runs
    // normally via the real trampoline.
    engine.register_compute_fn(
        "solver::membrane_load",
        reify_eval::compute_targets::membrane_load::solve_membrane_load_trampoline as ComputeFn,
    );

    // First eval: cold start — exactly one dispatch.
    let eval1 = engine.eval(&compiled);
    let errors1 = collect_errors(&eval1.diagnostics);
    assert!(
        errors1.is_empty(),
        "first pavilion eval must have no Error diagnostics; got: {errors1:#?}"
    );
    assert_eq!(
        PAVILION_DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "first eval must dispatch solver::form_find_free exactly once"
    );

    // Second eval on the same compiled module: Final-gate cache hit.
    let eval2 = engine.eval(&compiled);
    let errors2 = collect_errors(&eval2.diagnostics);
    assert!(
        errors2.is_empty(),
        "second pavilion eval must have no Error diagnostics; got: {errors2:#?}"
    );
    assert_eq!(
        PAVILION_DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "second eval must hit the Final-gate cache (count must stay at 1)"
    );

    // Perturbed-sigma variant: a different compiled module (different σ value)
    // produces a new ComputeNode whose inputs differ → cache miss → re-dispatch.
    // This uses a short inline source with the same prism topology but σ = 0.99
    // instead of the pavilion's default, compiled as a fresh module.
    const PERTURBED_SOURCE: &str = r#"
structure def PerturbedPavilion {
    let prism = Tensegrity(
        nodes: [
            point3(1m, 0m, 1m),
            point3(-0.5m, 0.866m, 1m),
            point3(-0.5m, -0.866m, 1m),
            point3(0.866m, 0.5m, -1m),
            point3(-0.866m, 0.5m, -1m),
            point3(0m, -1m, -1m)
        ],
        struts: [[0, 4], [1, 5], [2, 3]],
        cables: [
            [0, 1], [1, 2], [2, 0],
            [3, 4], [4, 5], [5, 3],
            [0, 3], [1, 4], [2, 5]
        ],
        surfaces: [[0, 1, 2], [3, 4, 5]]
    )
    let gids  = [0, 0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2]
    let seeds = [-1.0, 1.0, 1.0]
    let ref_g = 1
    let sigmas = [0.99, 0.99]
    let form  = form_find_free(prism, gids, seeds, ref_g, sigmas)
}
"#;
    let compiled_perturbed = compile_source_with_stdlib(PERTURBED_SOURCE);
    let errors_p = collect_errors(&compiled_perturbed.diagnostics);
    assert!(
        errors_p.is_empty(),
        "perturbed-sigma source must compile without Error diagnostics; got: {errors_p:#?}"
    );

    engine.eval(&compiled_perturbed);
    assert_eq!(
        PAVILION_DISPATCH_COUNT.load(Ordering::SeqCst),
        2,
        "eval with perturbed σ must re-dispatch (new ComputeNode, count must reach 2)"
    );
}

// ── (d) cancellation: cooperative-cancel wrapper leaves VC Pending ────────────

/// Poll budget for the slow cooperative wrapper (ms). The SLA is ≤5× this
/// value (gives scheduling-jitter headroom on loaded CI, mirroring
/// cancellation_compute_dispatch.rs).
const CANCEL_POLL_MS: u64 = 100;

/// Published handle from `slow_cancel_form_find_free`. `OnceLock` so
/// registration is idempotent across potential test-process reuse.
///
/// **Single-test ownership invariant**: written exclusively by the wrapper
/// registered under `"solver::form_find_free"` in
/// `pavilion_form_find_free_cancellation_leaves_vc_pending`. A second test
/// registering under the same target will silently race on this cell.
static PAVILION_CANCEL_HANDLE: OnceLock<Mutex<Option<CancellationHandle>>> = OnceLock::new();

/// Cooperative-cancel wrapper: publishes its `CancellationHandle` clone so a
/// canceller thread can fire it, then polls `is_cancelled()` every
/// `CANCEL_POLL_MS` (capped at 20 iterations as a hang guard).
fn slow_cancel_form_find_free(
    _value_inputs: &[Value],
    _realization_inputs: &[RealizationReadHandle],
    _options: &Value,
    _prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    let cell = PAVILION_CANCEL_HANDLE.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(cancellation.clone());
    for _ in 0..20 {
        if cancellation.is_cancelled() {
            return ComputeOutcome::Cancelled;
        }
        std::thread::sleep(Duration::from_millis(CANCEL_POLL_MS));
    }
    ComputeOutcome::Cancelled
}

/// (d) Cooperative cancellation of `solver::form_find_free`: a mid-trampoline
/// cancel must leave the output VC `Freshness::Pending` (NOT `Failed`) within
/// `5 × CANCEL_POLL_MS` of the cancel signal, and the prior cached value must
/// be intact.
///
/// Mirrors `cooperative_cancellation_sla_2x_budget` in
/// `cancellation_compute_dispatch.rs`, adapted to the `solver::form_find_free`
/// target and a seeded prior Final value.
#[test]
fn pavilion_form_find_free_cancellation_leaves_vc_pending() {
    // Belt-and-suspenders: clear the published handle from any prior run.
    if let Some(m) = PAVILION_CANCEL_HANDLE.get() {
        *m.lock().unwrap() = None;
    }

    // Pavilion source must compile (RED if file is missing).
    let compiled = compile_source_with_stdlib(pavilion_source());
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "pavilion must compile without Error diagnostics (cancellation test pre-check); \
         got: {errors:#?}"
    );

    // Run the cancellation mechanics directly via `run_compute_dispatch`,
    // mirroring cancellation_compute_dispatch.rs test B.  A synthetic prior
    // Final value is seeded so `begin_compute_dispatch` records a
    // `last_substantive` — the "prior cache intact" check.
    let mut engine = make_simple_engine();
    engine.register_compute_fn(
        "solver::form_find_free",
        slow_cancel_form_find_free as ComputeFn,
    );

    let cell = ValueCellId::new("Pavilion", "form_cancel");
    let c_id = ComputeNodeId::new("Pavilion", 0);

    // Seed a Final entry with a sentinel value so the prior-cache check is
    // meaningful (matches cancellation_compute_dispatch.rs §D pattern).
    engine.cache_store_mut().put(
        NodeId::Value(cell.clone()),
        NodeCache::new(
            CachedResult::Value(Value::Int(42), DeterminacyState::Determined),
            Freshness::Final,
            DependencyTrace::default(),
            VersionId(1),
        ),
    );

    let cancel_fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancel_fired2 = cancel_fired.clone();

    // Canceller thread: busy-waits for the published handle then fires it.
    let canceller = std::thread::spawn(move || {
        let handle = loop {
            let cell = PAVILION_CANCEL_HANDLE.get_or_init(|| Mutex::new(None));
            if let Some(h) = cell.lock().unwrap().clone() {
                break h;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        handle.cancel();
        cancel_fired2.store(true, Ordering::SeqCst);
    });

    let handle = CancellationHandle::new();
    let start = Instant::now();
    // Use crafted inputs matching a form_find_free call (5 values, same shape
    // as prism_form_find_inputs) so the trampoline signature is satisfied.
    let inputs = prism_form_find_inputs(0.2);
    engine.run_compute_dispatch(
        &c_id,
        std::slice::from_ref(&cell),
        "solver::form_find_free",
        &inputs,
        &[],
        &Value::Undef,
        &handle,
        VersionId(2),
    );
    let elapsed = start.elapsed();

    canceller.join().expect("canceller thread must not panic");
    assert!(
        cancel_fired.load(Ordering::SeqCst),
        "canceller thread must have fired before dispatch returned"
    );

    // SLA: dispatch must return within 5 × CANCEL_POLL_MS of the cancel signal.
    let sla = Duration::from_millis(5 * CANCEL_POLL_MS);
    assert!(
        elapsed <= sla,
        "cancellation SLA exceeded: dispatch took {elapsed:?} (SLA: {sla:?}); \
         cooperative poll budget is {CANCEL_POLL_MS}ms"
    );

    // Use NodeId::Value to check the output VC (mirrors cancellation_compute_dispatch.rs test D).
    let vc_node = NodeId::Value(cell.clone());

    // (A1) Output VC must be Freshness::Pending — NOT Failed.
    assert!(
        matches!(engine.freshness(&vc_node), Freshness::Pending { .. }),
        "cancelled form_find_free dispatch must leave VC Pending; got {:?}",
        engine.freshness(&vc_node)
    );

    // (A2) Prior cached value (Int(42)) must be intact — not overwritten
    // (begin_compute_dispatch only changes freshness/pending_cause).
    let entry = engine
        .cache_store()
        .get(&vc_node)
        .expect("cache entry must exist after begin_compute_dispatch");
    match &entry.result {
        CachedResult::Value(v, _) => {
            assert_eq!(
                *v,
                Value::Int(42),
                "prior cached value must be unchanged after cancellation"
            );
        }
        other => panic!("expected CachedResult::Value(Int(42)); got {other:?}"),
    }
}

// ── (e) load e2e: membrane_load ComputeNode + G6 field population ─────────────

/// (e) The pavilion must dispatch exactly one `solver::membrane_load` ComputeNode
/// and the `MembraneLoadResult` cell (member "load") must have ALL 8 fields
/// populated with real (non-Undef) values — the G6 field-population invariant
/// (esc-2962-33): `displacements`, `member_forces`, `member_force_deltas`,
/// `member_slack`, `surface_stress_deltas`, `surface_principal_stresses`,
/// `surface_slack`, `converged`.
///
/// RED until step-6 adds `membrane_load(...)` to `examples/tensegrity_pavilion.ri`.
#[test]
fn pavilion_membrane_load_e2e_all_fields_populated() {
    let compiled = compile_source_with_stdlib(pavilion_source());
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "pavilion must compile without Error diagnostics (load e2e pre-check); got: {errors:#?}"
    );

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    let eval_errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "pavilion eval must produce no Error diagnostics; got: {eval_errors:#?}"
    );

    // Exactly one solver::membrane_load ComputeNode — proves the @optimized call
    // lowered (not body-inlined) once the membrane_load call is in the pavilion.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let membrane_load_nodes: Vec<_> = snapshot
        .graph
        .compute_nodes
        .iter()
        .filter(|(_, d)| d.target == "solver::membrane_load")
        .collect();
    assert_eq!(
        membrane_load_nodes.len(),
        1,
        "expected exactly one solver::membrane_load ComputeNode; found {membrane_load_nodes:?}"
    );

    // Find the MembraneLoadResult cell by member name "load".
    let load_cell_id = eval_result
        .values
        .iter()
        .find(|(id, _)| id.member == "load")
        .map(|(id, _)| id.clone())
        .unwrap_or_else(|| {
            panic!(
                "no 'load' cell in eval result; all cells: {:?}",
                eval_result.values.iter().map(|(id, _)| id).collect::<Vec<_>>()
            )
        });
    let load = eval_result
        .values
        .get(&load_cell_id)
        .expect("load cell must be present in ValueMap");

    let data = match load {
        Value::StructureInstance(d) => d,
        other => panic!(
            "'load' cell must be a MembraneLoadResult StructureInstance; got {other:?}"
        ),
    };
    assert_eq!(
        data.type_name, "MembraneLoadResult",
        "load cell type_name must be MembraneLoadResult; got {:?}",
        data.type_name
    );

    // G6 invariant: `converged` must be a Bool.
    assert!(
        matches!(data.fields.get("converged"), Some(Value::Bool(_))),
        "MembraneLoadResult.converged must be a Bool (G6); got {:?}",
        data.fields.get("converged")
    );

    // G6 invariant: all 7 List fields must be non-empty Lists (not Undef).
    for field_name in &[
        "displacements",
        "member_forces",
        "member_force_deltas",
        "member_slack",
        "surface_stress_deltas",
        "surface_principal_stresses",
        "surface_slack",
    ] {
        let v = data.fields.get(*field_name).unwrap_or_else(|| {
            panic!(
                "MembraneLoadResult.{field_name} field missing (G6: must be populated, \
                 not Undef); all fields: {:?}",
                data.fields.keys().collect::<Vec<_>>()
            )
        });
        match v {
            Value::List(items) => assert!(
                !items.is_empty(),
                "MembraneLoadResult.{field_name} must be non-empty (G6); got []"
            ),
            other => panic!(
                "MembraneLoadResult.{field_name} must be a List (G6); got {other:?}"
            ),
        }
    }
}

// ── (f) CLI dual-result smoke ─────────────────────────────────────────────────

/// Resolve the prebuilt `reify` binary: profile-local first, then debug fallback.
/// Mirrors `resolve_reify_bin` in `tensegrity_t1a_form_find.rs`.
fn resolve_reify_bin_pavilion() -> std::path::PathBuf {
    let test_bin = std::env::current_exe().expect("current_exe");
    let profile_dir = test_bin
        .parent()
        .and_then(|p| p.parent())
        .expect("test binary lives in target/<profile>/deps");
    let profile_local = profile_dir.join("reify");
    if profile_local.exists() {
        profile_local
    } else {
        profile_dir
            .parent()
            .map(|target_dir| target_dir.join("debug").join("reify"))
            .filter(|p| p.exists())
            .unwrap_or(profile_local)
    }
}

/// (f) CLI dual-result smoke: `reify eval examples/tensegrity_pavilion.ri` exits
/// 0 and stdout contains BOTH `FormFindResult { converged: true,` (the δ signal)
/// AND `MembraneLoadResult {` (the η signal) — the user-observable θ proof that
/// the pavilion form-finds AND carries load.
///
/// RED until step-6 adds `membrane_load(...)` to the pavilion (which adds the
/// `MembraneLoadResult` to the CLI output).
#[test]
fn pavilion_cli_prints_both_form_find_and_load_results() {
    let manifest = env!("CARGO_MANIFEST_DIR"); // .../crates/reify-eval
    let workspace_root = std::path::Path::new(manifest)
        .ancestors()
        .nth(2)
        .expect("workspace root two levels above crates/reify-eval")
        .to_path_buf();
    let example = workspace_root.join("examples/tensegrity_pavilion.ri");
    let reify_bin = resolve_reify_bin_pavilion();

    let output = std::process::Command::new(&reify_bin)
        .current_dir(&workspace_root)
        .arg("eval")
        .arg(&example)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "failed to spawn pre-built reify binary at {}: {e}; \
                 build with `cargo build --bin reify` first.",
                reify_bin.display()
            )
        });

    assert!(
        output.status.success(),
        "`reify eval examples/tensegrity_pavilion.ri` exited non-zero.\n\
         stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8(output.stdout).expect("stdout must be valid UTF-8");

    // θ signal (δ half): the pavilion form-finds to convergence.
    assert!(
        stdout.contains("FormFindResult { converged: true,"),
        "expected `FormFindResult {{ converged: true, … }}` in `reify eval` stdout; \
         got:\n{stdout}"
    );

    // θ signal (η half): the pavilion carries load (MembraneLoadResult present).
    assert!(
        stdout.contains("MembraneLoadResult {"),
        "expected `MembraneLoadResult {{…}}` in `reify eval` stdout — the θ load signal; \
         got:\n{stdout}"
    );
}
