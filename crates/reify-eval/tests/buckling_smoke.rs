//! End-to-end integration tests for `fn solve_buckling` @optimized →
//! ComputeNode → trampoline pipeline (PRD §13 task ε,
//! docs/prds/v0_5/buckling-eigensolver.md).
//!
//! Steps:
//!   step-1/2  — trampoline registration + seam pin
//!   step-3/4  — ComputeNode-insertion assertion + smoke .ri
//!   step-5/6  — critical-load accuracy + helper consistency + real FEA impl
//!   step-7/8  — cache-hit assertion + determinism guarantee

use std::sync::atomic::{AtomicU32, Ordering};

use reify_eval::{CancellationHandle, ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::{OpaqueState, Value};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Load and compile the buckling column smoke fixture.
fn buckling_source() -> &'static str {
    include_str!("../../../examples/buckling_column_smoke.ri")
}

/// Extract a named field from a BucklingResult value.
fn extract_field(result: &Value, field: &str) -> Option<Value> {
    match result {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String(field.to_string())).cloned(),
        _ => None,
    }
}

// ── step-1: RED — trampoline registration + seam pin ─────────────────────────
//
// Compile-time test: coerce
//   `reify_eval::compute_targets::buckling::solve_buckling_trampoline`
// to `ComputeFn` to pin the cross-crate signature. No runtime assertion —
// compile success is the signal. Expected to fail until step-2 creates the
// `compute_targets::buckling` module.

#[allow(dead_code)]
fn _seam_pin() {
    let _f: ComputeFn =
        reify_eval::compute_targets::buckling::solve_buckling_trampoline;
}

/// Step-1: `register_compute_fns` installs the buckling trampoline under the correct key.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, asserts
/// `engine.compute_dispatch("solver::buckling").is_some()`.
///
/// Expected to fail until step-2 creates the `compute_targets::buckling` module.
#[test]
fn register_compute_fns_installs_solver_buckling() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("solver::buckling").is_some(),
        "register_compute_fns must install a trampoline under 'solver::buckling'"
    );
}

// ── step-3: RED — end-to-end ComputeNode insertion ───────────────────────────
//
// Three observable signals:
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "solver::buckling" exists in the graph
//   (c) the result cell is a non-Undef StructureInstance or Map
//
// Gated: the full buckling solve takes ~25 s release / ~1000 s debug.
// The registration pin above runs always; this e2e gate runs release-only.

/// End-to-end smoke: buckling .ri lowers to a ComputeNode and result cell is non-Undef.
#[cfg_attr(debug_assertions, ignore = "heavy buckling solve; release-only")]
#[test]
fn e2e_buckling_smoke_lowers_to_compute_node() {
    let source = buckling_source();
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

    // (b) A ComputeNode with target == "solver::buckling" must be in the graph.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "solver::buckling");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"solver::buckling\" in the graph; \
         found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // (c) The result cell must hold a non-Undef value (StructureInstance or Map).
    let result_cell = ValueCellId::new("BucklingColumnSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell BucklingColumnSmoke.result not found in eval result"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "expected result to be StructureInstance or Map (NOT Undef), got: {:?}",
        result_val
    );
}

// ── step-5: RED — critical-load accuracy + helper consistency ─────────────────
//
// Analytical pin-pin Euler critical load for 20×20×800 mm Steel AISI 1045:
//   P_cr = π²·E·I / L²
//   E = 205e9, I = 0.02·0.02³/12 = 1.3333e-8 m⁴, L = 0.8 m
//   P_cr ≈ 42.15 kN
//
// Achievability: euler_column_pin_pin.rs observes 9.2% error on this exact
// geometry (nx=ny=8, nz=160). The trampoline mirrors that mesh → within 10%.
//
// Expected to fail (assertion error) until step-6 implements the real buckling solve,
// because the step-2 skeleton returns eigenvalue 0.0.

/// Critical load within 10% of the analytical Euler value.
#[cfg_attr(debug_assertions, ignore = "heavy buckling solve; release-only")]
#[test]
fn e2e_buckling_critical_load_within_ten_percent() {
    use std::f64::consts::PI;

    let source = buckling_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error diagnostics.
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

    // (b) `crit` cell: Scalar with dimension FORCE, within 10% of P_cr.
    let crit_cell = ValueCellId::new("BucklingColumnSmoke", "crit");
    let crit_val = eval_result
        .values
        .get(&crit_cell)
        .unwrap_or_else(|| panic!("cell BucklingColumnSmoke.crit not found in eval result"));

    let (crit_si, crit_dim) = match crit_val {
        Value::Scalar { si_value, dimension } => (*si_value, *dimension),
        other => panic!("expected crit to be Value::Scalar, got: {:?}", other),
    };
    assert_eq!(
        crit_dim,
        DimensionVector::FORCE,
        "expected crit dimension == FORCE, got: {:?}",
        crit_dim
    );
    assert!(crit_si.is_finite() && crit_si > 0.0, "crit must be finite and positive, got: {}", crit_si);

    // Analytical P_cr = π²·E·I / L²  (pin-pin, k=1)
    // E = 205e9 Pa, I = lx·ly³/12 = 0.02·0.02³/12, L = 0.8 m
    let e: f64 = 205.0e9;
    let i_min: f64 = 0.02 * 0.02_f64.powi(3) / 12.0;
    let l: f64 = 0.8;
    let p_cr = PI.powi(2) * e * i_min / (l * l);
    let rel_err = (crit_si - p_cr).abs() / p_cr;
    assert!(
        rel_err < 0.10,
        "critical_load = {:.3e} N, P_cr = {:.3e} N, rel_err = {:.2}% > 10%",
        crit_si, p_cr, rel_err * 100.0
    );

    // (c) Consistency: crit ≈ modes[0].eigenvalue × 1000 N.
    // Retrieve result.modes[0].eigenvalue from the BucklingResult.
    let result_cell = ValueCellId::new("BucklingColumnSmoke", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .expect("BucklingColumnSmoke.result not found");
    let modes = extract_field(result_val, "modes")
        .expect("result.modes not found");
    let lambda0 = match &modes {
        Value::List(items) if !items.is_empty() => {
            match &items[0] {
                Value::StructureInstance(data) => match data.fields.get(&"eigenvalue".to_string()) {
                    Some(Value::Real(r)) => *r,
                    other => panic!("modes[0].eigenvalue not Real, got: {:?}", other),
                },
                other => panic!("modes[0] not StructureInstance, got: {:?}", other),
            }
        }
        other => panic!("result.modes not List or empty, got: {:?}", other),
    };
    // crit = lambda0 * 1000N (SI: 1000 N)
    let crit_from_eigenvalue = lambda0 * 1000.0;
    let consistency_err = (crit_si - crit_from_eigenvalue).abs() / crit_from_eigenvalue.abs().max(1.0);
    assert!(
        consistency_err < 1e-9,
        "crit = {:.6e} N but lambda0 × 1000 N = {:.6e} N (consistency check failed)",
        crit_si, crit_from_eigenvalue
    );

    // (d) `sf` cell: dimensionless Real == modes[0].eigenvalue (> 0).
    let sf_cell = ValueCellId::new("BucklingColumnSmoke", "sf");
    let sf_val = eval_result
        .values
        .get(&sf_cell)
        .unwrap_or_else(|| panic!("cell BucklingColumnSmoke.sf not found"));
    match sf_val {
        Value::Real(sf) => {
            assert!(*sf > 0.0, "safety_factor_buckling must be positive, got: {}", sf);
            let sf_err = (*sf - lambda0).abs() / lambda0.abs().max(1e-30);
            assert!(
                sf_err < 1e-9,
                "sf = {} but modes[0].eigenvalue = {} (sf must equal eigenvalue)",
                sf, lambda0
            );
        }
        other => panic!("expected sf to be Value::Real, got: {:?}", other),
    }

    // (e) `ms` cell resolves without error (Undef is acceptable for task ε).
    let ms_cell = ValueCellId::new("BucklingColumnSmoke", "ms");
    assert!(
        eval_result.values.get(&ms_cell).is_some(),
        "cell BucklingColumnSmoke.ms not found in eval result"
    );
}

// ── step-7: RED — ComputeNode cache-hit on identical re-run ──────────────────
//
// Verifies that the second eval() does NOT re-dispatch the trampoline.
// The generic Final-gate (engine_eval.rs) short-circuits re-dispatch when
// all inputs are Final and the output VC is already Final from a prior eval().
//
// Expected: DISPATCH_COUNT == 1 after two sequential eval() calls.

/// Dispatch counter for the buckling counting wrapper.
static DISPATCH_COUNT: AtomicU32 = AtomicU32::new(0);

/// Counting wrapper: increments DISPATCH_COUNT then calls the production trampoline.
fn counting_wrapper(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    DISPATCH_COUNT.fetch_add(1, Ordering::SeqCst);
    reify_eval::compute_targets::buckling::solve_buckling_trampoline(
        value_inputs,
        realization_inputs,
        options,
        prior_warm_state,
        cancellation,
    )
}

/// Cache-hit: second eval() of the same compiled module must NOT re-dispatch.
#[cfg_attr(debug_assertions, ignore = "heavy buckling solve; release-only")]
#[test]
fn e2e_buckling_second_eval_hits_cache() {
    DISPATCH_COUNT.store(0, Ordering::SeqCst);

    let source = buckling_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    engine.register_compute_fn("solver::buckling", counting_wrapper as ComputeFn);

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

    let result_cell = ValueCellId::new("BucklingColumnSmoke", "result");
    let result1 = eval1
        .values
        .get(&result_cell)
        .cloned()
        .unwrap_or_else(|| panic!("first eval: cell BucklingColumnSmoke.result not found"));

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
         (DISPATCH_COUNT must stay at 1)"
    );

    // ── Both evals must produce the same modes[0].eigenvalue (deterministic) ──
    let result2 = eval2
        .values
        .get(&result_cell)
        .cloned()
        .unwrap_or_else(|| panic!("second eval: cell BucklingColumnSmoke.result not found"));

    let lambda1 = match &result1 {
        Value::StructureInstance(data) => match data.fields.get(&"modes".to_string()) {
            Some(Value::List(items)) if !items.is_empty() => {
                match &items[0] {
                    Value::StructureInstance(d) => match d.fields.get(&"eigenvalue".to_string()) {
                        Some(Value::Real(r)) => *r,
                        _ => panic!("first eval: modes[0].eigenvalue not Real"),
                    },
                    _ => panic!("first eval: modes[0] not StructureInstance"),
                }
            }
            _ => panic!("first eval: result.modes not List or empty"),
        },
        _ => panic!("first eval: result not StructureInstance"),
    };
    let lambda2 = match &result2 {
        Value::StructureInstance(data) => match data.fields.get(&"modes".to_string()) {
            Some(Value::List(items)) if !items.is_empty() => {
                match &items[0] {
                    Value::StructureInstance(d) => match d.fields.get(&"eigenvalue".to_string()) {
                        Some(Value::Real(r)) => *r,
                        _ => panic!("second eval: modes[0].eigenvalue not Real"),
                    },
                    _ => panic!("second eval: modes[0] not StructureInstance"),
                }
            }
            _ => panic!("second eval: result.modes not List or empty"),
        },
        _ => panic!("second eval: result not StructureInstance"),
    };
    assert_eq!(
        lambda1.to_bits(),
        lambda2.to_bits(),
        "both evals must produce bit-identical modes[0].eigenvalue \
         (deterministic trampoline contract)"
    );
}
