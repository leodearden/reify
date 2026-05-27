//! End-to-end integration tests for `fn solve_elastic_static` @optimized →
//! ComputeNode → trampoline pipeline (PRD §8 task η,
//! docs/prds/v0_3/compute-node-contract.md).
//!
//! Steps:
//!   step-3/4  — API surface pin + module skeleton
//!   step-5/6  — ComputeNode-insertion assertion + smoke .ri
//!   step-7/8  — cantilever stress magnitude assertion + real FEA impl
//!   step-9/10 — cache-hit assertion + doc comments

use std::sync::atomic::{AtomicU32, Ordering};

use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::{OpaqueState, Value};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Load and compile the cantilever smoke fixture.
///
/// Uses `include_str!` so the test binary carries the source at compile time
/// and is always in sync with the user-facing example file. This is the
/// "single source of truth" design decision documented in the plan.
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/fea_cantilever_smoke.ri")
}

/// Extract `result.max_von_mises` from an ElasticResult value.
///
/// Handles both `Value::StructureInstance(data)` (preferred path after step-8)
/// and `Value::Map(m)` (temporary fallback documented in plan step-8).
/// Returns `None` if the value doesn't match either shape.
fn extract_max_von_mises(result: &Value) -> Option<Value> {
    match result {
        // PersistentMap::get takes &K (= &String), not &str — use owned key.
        Value::StructureInstance(data) => {
            data.fields.get(&"max_von_mises".to_string()).cloned()
        }
        Value::Map(m) => m.get(&Value::String("max_von_mises".to_string())).cloned(),
        _ => None,
    }
}

/// Extract a named field from an ElasticResult value.
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        // PersistentMap::get takes &K (= &String), not &str — use owned key.
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

// ── step-3: RED — API surface pin ────────────────────────────────────────────
//
// Compile-time test: coerce
//   `reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline`
// to `ComputeFn` to pin the cross-crate signature. No runtime assertion —
// compile success is the signal. Expected to fail until step-4 creates the
// `compute_targets` module.

#[allow(dead_code)]
fn _seam_pin() {
    let _f: ComputeFn =
        reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline;
}

/// Step-3: `register_compute_fns` installs the trampoline under the correct key.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, asserts
/// `engine.compute_dispatch("solver::elastic_static").is_some()`.
///
/// Expected to fail until step-4 creates the `compute_targets` module.
#[test]
fn register_compute_fns_installs_solver_elastic_static() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("solver::elastic_static").is_some(),
        "register_compute_fns must install a trampoline under 'solver::elastic_static'"
    );
}

// ── step-5: RED — ComputeNode-insertion assertion ─────────────────────────────
//
// Mirrors the recipe at crates/reify-eval/tests/compute_dispatch_registry.rs:175-223.
// Three observable signals:
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "solver::elastic_static" exists in the graph
//   (c) the result cell has a non-Undef value (StructureInstance or Map)
//
// Expected to fail (compile error) because examples/fea_cantilever_smoke.ri
// does not yet exist — step-6 creates it.

/// End-to-end smoke: cantilever .ri lowers to a ComputeNode (not body-inlined)
/// and the result cell is a non-Undef StructureInstance or Map.
#[test]
fn e2e_cantilever_smoke_lowers_to_compute_node() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // (b) A ComputeNode with target == "solver::elastic_static" must be in the graph
    //     (confirming @optimized lowering fired, not body-inlined).
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "solver::elastic_static");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"solver::elastic_static\" in the graph; \
         found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // (c) The result cell must hold a non-Undef value (StructureInstance or Map).
    //     Step-6 upgrades the skeleton trampoline to return a placeholder ElasticResult.
    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found in eval result"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "expected result to be StructureInstance or Map (NOT Undef), got: {:?}",
        result_val
    );
}

// ── step-7: RED — cantilever stress magnitude assertion ───────────────────────
//
// Analytical reference (Euler–Bernoulli, rectangular cross-section):
//   σ_max = 6 · P · L / (b · h²)
//         = 6 × 1000 × 1.0 / (0.1 × 0.01)
//         = 6 000 000 Pa  (6 MPa)
//
// Tolerance: ±50% — documented method-error budget for a coarse P1-tet mesh.
// P1 tets are stiffer than reality, so the FEA underestimates by 20–50%.
// Design decision 2 in the plan documents this threshold as the achievability
// basis, not a guessed tolerance.
//
// Expected to fail (assertion error) until step-8 implements the real FEA solve,
// because the placeholder trampoline returns max_von_mises = 0 Pa.

/// Cantilever max von Mises within ±50% of the analytical 6 MPa reference.
#[test]
fn e2e_cantilever_max_von_mises_within_tolerance() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // No Error diagnostics — clean solve required before asserting on values.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics before stress assertion, got: {:?}",
        errors
    );

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell FeaCantileverSmoke.result not found in eval result"));

    // ── (a) max_von_mises ────────────────────────────────────────────────────
    //
    // Extract max_von_mises via the helper that handles both StructureInstance
    // and Map (the two ElasticResult shapes documented in plan step-8).
    let mvm = extract_max_von_mises(result_val).unwrap_or_else(|| {
        panic!(
            "could not extract max_von_mises from result: {:?}",
            result_val
        )
    });

    // The value must be Scalar with dimension == PRESSURE.
    let (si_value, dimension) = match &mvm {
        Value::Scalar { si_value, dimension } => (*si_value, dimension.clone()),
        other => panic!(
            "expected max_von_mises to be Value::Scalar {{ ... }}, got: {:?}",
            other
        ),
    };
    assert_eq!(
        dimension,
        DimensionVector::PRESSURE,
        "expected max_von_mises dimension == DimensionVector::PRESSURE, got: {:?}",
        dimension
    );

    // Analytical reference σ_max = 6PL/(bh²) = 6×1000×1.0/(0.1×0.01) = 6e6 Pa.
    // Tolerance: ±50% of analytical (3 MPa ≤ σ ≤ 9 MPa).
    let analytical_sigma: f64 = 6.0 * 1000.0 * 1.0 / (0.1 * 0.1 * 0.1);  // 6e6 Pa
    let lo = analytical_sigma * 0.5;   // 3e6 Pa  (P1 stiffness underestimate floor)
    let hi = analytical_sigma * 1.5;   // 9e6 Pa  (stress concentration head-room)
    assert!(
        si_value.is_finite(),
        "max_von_mises must be finite, got: {}",
        si_value
    );
    assert!(
        si_value > 0.0,
        "max_von_mises must be positive, got: {}",
        si_value
    );
    assert!(
        si_value >= lo && si_value <= hi,
        "max_von_mises = {:.3e} Pa is outside ±50% of analytical {:.3e} Pa \
         (expected [{:.3e}, {:.3e}])",
        si_value,
        analytical_sigma,
        lo,
        hi
    );

    // ── (b) converged ────────────────────────────────────────────────────────
    let converged = extract_field(result_val, "converged").unwrap_or_else(|| {
        panic!("could not extract 'converged' field from result: {:?}", result_val)
    });
    assert_eq!(
        converged,
        Value::Bool(true),
        "expected result.converged == Bool(true), got: {:?}",
        converged
    );

    // ── (c) iterations ───────────────────────────────────────────────────────
    let iterations = extract_field(result_val, "iterations").unwrap_or_else(|| {
        panic!("could not extract 'iterations' field from result: {:?}", result_val)
    });
    match &iterations {
        Value::Int(n) => {
            assert!(
                *n >= 0,
                "expected iterations >= 0, got: {}",
                n
            );
        }
        other => panic!("expected iterations to be Value::Int, got: {:?}", other),
    }
}

// ── step-9: RED — cache-hit assertion ────────────────────────────────────────
//
// Verifies that the second eval() of the same compiled module does NOT
// re-dispatch the trampoline. The significance_filter opt-in
// (significance_filter.rs:76) plus the cache machinery should prevent
// re-dispatch when inputs haven't changed.
//
// `counting_wrapper` is a module-level fn (required by the ComputeFn type alias,
// which is a plain fn-pointer and cannot be a boxed closure). DISPATCH_COUNT is
// a module-level AtomicU32 shared across all invocations.
//
// Expected: DISPATCH_COUNT == 1 after two eval() calls.
// If the second eval() re-dispatches (DISPATCH_COUNT == 2), the test fails —
// that would expose either a missing significance-filter opt-in OR a
// cache-key non-determinism in the trampoline output (see step-10 for the fix).

/// Dispatch counter incremented by `counting_wrapper` on every trampoline call.
/// Module-level static so it is callable as a plain `ComputeFn` fn-pointer.
static DISPATCH_COUNT: AtomicU32 = AtomicU32::new(0);

/// Counting wrapper: increments `DISPATCH_COUNT` then calls through to
/// the production trampoline.  Installed via `engine.register_compute_fn`
/// (bypasses `register_compute_fns` to avoid the panic-on-double-registration).
fn counting_wrapper(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    DISPATCH_COUNT.fetch_add(1, Ordering::SeqCst);
    reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline(
        value_inputs,
        realization_inputs,
        options,
        prior_warm_state,
        cancellation,
    )
}

/// Cache-hit: second eval() of the same compiled module must NOT re-dispatch.
///
/// Step-9 RED: asserts `DISPATCH_COUNT == 1` after two sequential `engine.eval()`
/// calls on the same `CompiledModule`.  Fails if the second eval re-dispatches
/// the trampoline (DISPATCH_COUNT would be 2).
#[test]
fn e2e_cantilever_second_eval_hits_cache() {
    // Reset counter for test isolation (guards against the test being re-run in
    // the same process without reinitialising the static).
    DISPATCH_COUNT.store(0, Ordering::SeqCst);

    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    // Register the counting wrapper directly (bypasses register_compute_fns so
    // the engine holds exactly one registration for "solver::elastic_static").
    engine.register_compute_fn("solver::elastic_static", counting_wrapper as ComputeFn);

    // ── First eval: trampoline must be dispatched once (cold start) ───────────
    let eval1 = engine.eval(&compiled);
    let errors1: Vec<_> = eval1
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors1.is_empty(),
        "first eval must have no Error diagnostics, got: {:?}",
        errors1
    );
    assert_eq!(
        DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "first eval must dispatch the trampoline exactly once"
    );

    let result_cell = ValueCellId::new("FeaCantileverSmoke", "result");
    let result1 = eval1
        .values
        .get(&result_cell)
        .cloned()
        .unwrap_or_else(|| panic!("first eval: cell FeaCantileverSmoke.result not found"));

    // ── Second eval: cache hit — must NOT re-dispatch ─────────────────────────
    let eval2 = engine.eval(&compiled);
    let errors2: Vec<_> = eval2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors2.is_empty(),
        "second eval must have no Error diagnostics, got: {:?}",
        errors2
    );

    assert_eq!(
        DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "second eval must hit the cache and NOT re-dispatch the trampoline \
         (DISPATCH_COUNT must stay at 1); if this fails, investigate \
         significance-filter opt-in or cache-key determinism — see step-10"
    );

    // ── Both evals must produce the same max_von_mises ────────────────────────
    let result2 = eval2
        .values
        .get(&result_cell)
        .cloned()
        .unwrap_or_else(|| panic!("second eval: cell FeaCantileverSmoke.result not found"));
    let mvm1 = extract_max_von_mises(&result1)
        .unwrap_or_else(|| panic!("first eval: could not extract max_von_mises"));
    let mvm2 = extract_max_von_mises(&result2)
        .unwrap_or_else(|| panic!("second eval: could not extract max_von_mises"));
    assert_eq!(
        mvm1, mvm2,
        "both evals must produce bit-identical max_von_mises \
         (deterministic trampoline contract)"
    );
}
