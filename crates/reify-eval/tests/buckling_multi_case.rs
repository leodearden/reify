#![allow(clippy::mutable_key_type)]
//! Integration tests for task 3456: `solve_buckling_load_cases`
//! @optimized → `"solver::buckling_multi_case"` ComputeNode lowering +
//! compute-result-reuse verification.
//!
//! Two observable signals:
//!
//! - **(a) TWO-CASE SOLVE → POPULATED MultiCaseBucklingResult** — after
//!   @optimized lowering fires, each case in the returned
//!   `Value::Map{"cases"→Map}` is a `Value::StructureInstance("BucklingResult")`
//!   with a non-empty `modes` list; a ComputeNode with target
//!   `"solver::buckling_multi_case"` appears in the graph; and per-case
//!   independence holds (2× tip load ⇒ ~½× eigenvalue, so
//!   overload.modes[0].eigenvalue < operating.modes[0].eigenvalue).
//!
//! - **(b) SECOND EVAL REUSES COMPUTE RESULT** — a counting wrapper
//!   registered for `"solver::buckling_multi_case"` is dispatched exactly
//!   once across two `engine.eval()` calls (§8-η Final-gate).
//!
//! Both tests are **RED** until step-2 adds:
//!   1. `@optimized("solver::buckling_multi_case")` to `solve_buckling_load_cases`
//!      in `solver_buckling_fns.ri`;
//!   2. `crates/reify-eval/src/compute_targets/buckling_multi_case.rs` with
//!      `pub fn solve_buckling_multi_case_trampoline(...)`;
//!   3. Registration in `compute_targets::register_compute_fns`.
//!
//! Mirrors the scaffold from `multi_case_compute_node.rs`.

use std::sync::atomic::{AtomicU32, Ordering};

use reify_core::{Severity, ValueCellId};
use reify_eval::graph::CancellationHandle;
use reify_eval::{ComputeFn, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── Fixture source ─────────────────────────────────────────────────────────────
//
// Two LoadCases differing only in tip compressive force (1 kN vs 2 kN).
// Same 20×20×800 mm column geometry as buckling_column_smoke.ri.
// P_cr = π²EI/L² is fixed by geometry; λ = P_cr/P_applied, so:
//   operating (1 kN):  λ_op  ≈ P_cr / 1000  (larger eigenvalue, less critical)
//   overload  (2 kN):  λ_ovl ≈ P_cr / 2000  (smaller eigenvalue, more critical)
// ⇒ λ_ovl < λ_op (physical sanity assertion in test_A below).

const TWO_CASE_SOURCE: &str = r#"
structure def BucklingMultiCaseFixture {
    let lc1 = LoadCase(
        name:     "operating",
        loads:    [PointLoad(point: "top", force: 1000.0)],
        supports: [FixedSupport(target: "base")],
    )
    let lc2 = LoadCase(
        name:     "overload",
        loads:    [PointLoad(point: "top", force: 2000.0)],
        supports: [FixedSupport(target: "base")],
    )
    let result = solve_buckling_load_cases(
        Steel_AISI_1045(), 800mm, 20mm, 20mm, [lc1, lc2]
    )
}
"#;

// ── Helpers ────────────────────────────────────────────────────────────────────

/// Extract a named field from a `Value::StructureInstance`.
fn extract_field(val: &Value, field: &str) -> Option<Value> {
    match val {
        Value::StructureInstance(data) => data.fields.get(&field.to_string()).cloned(),
        _ => None,
    }
}

/// Extract `modes[0].eigenvalue` as `f64` from a BucklingResult value.
///
/// Returns `None` on any shape mismatch (missing modes, empty modes, non-Real
/// eigenvalue). Used in both the integration test and the capstone test.
pub(crate) fn extract_first_mode_eigenvalue(case_val: &Value) -> Option<f64> {
    let modes = extract_field(case_val, "modes")?;
    let modes_list = match modes {
        Value::List(v) => v,
        _ => return None,
    };
    let first_mode = modes_list.first()?;
    let eigenvalue = extract_field(first_mode, "eigenvalue")?;
    match eigenvalue {
        Value::Real(v) => Some(v),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// (a) TWO-CASE SOLVE → POPULATED MultiCaseBucklingResult
// ─────────────────────────────────────────────────────────────────────────────

/// End-to-end: `solve_buckling_load_cases` with 2 cases lowers to a
/// `"solver::buckling_multi_case"` ComputeNode and returns a
/// `MultiCaseBucklingResult`-shaped `Value::Map` where each per-case is a
/// `Value::StructureInstance("BucklingResult")` with a non-empty `modes` list.
///
/// # RED until step-2
///
/// Before step-2:
///   - `reify_eval::compute_targets::buckling_multi_case` does not exist →
///     this file **fails to compile** until step-2 creates the module.
///   - `solve_buckling_load_cases` has no `@optimized` annotation → no
///     ComputeNode with `"solver::buckling_multi_case"` would be created.
#[test]
fn two_case_buckling_solve_returns_populated_result() {
    let compiled = parse_and_compile_with_stdlib(TWO_CASE_SOURCE);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (1) No Error-severity diagnostics ─────────────────────────────────────
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {errors:?}"
    );

    // ── (2) A ComputeNode with target "solver::buckling_multi_case" must exist ─
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_mc_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "solver::buckling_multi_case");
    assert!(
        has_mc_node,
        "expected a ComputeNode with target==\"solver::buckling_multi_case\" in the graph; \
         found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // ── (3) Result is Value::Map{"cases" -> Map} with exactly 2 entries ────────
    let result_cell = ValueCellId::new("BucklingMultiCaseFixture", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell BucklingMultiCaseFixture.result not found"));

    let cases_map = match result_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => panic!("result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!(
            "solve_buckling_load_cases result must be Value::Map (not {:?})",
            std::mem::discriminant(other)
        ),
    };
    assert_eq!(
        cases_map.len(),
        2,
        "cases map must have exactly 2 entries; got {} entries: {:?}",
        cases_map.len(),
        cases_map.keys().collect::<Vec<_>>()
    );

    // ── (4) Per-case shape: StructureInstance("BucklingResult") with modes ────
    for case_name in ["operating", "overload"] {
        let case_val = cases_map
            .get(&Value::String(case_name.to_string()))
            .unwrap_or_else(|| {
                panic!(
                    "cases map must contain \"{case_name}\" key; got: {:?}",
                    cases_map.keys().collect::<Vec<_>>()
                )
            });

        match case_val {
            Value::StructureInstance(data) => {
                assert_eq!(
                    data.type_name, "BucklingResult",
                    "case \"{case_name}\" must be StructureInstance(\"BucklingResult\"), \
                     got type_name = \"{}\"",
                    data.type_name
                );
            }
            other => panic!(
                "case \"{case_name}\" must be Value::StructureInstance(\"BucklingResult\"), \
                 got: {other:?}"
            ),
        }

        let modes = extract_field(case_val, "modes").unwrap_or_else(|| {
            panic!("case \"{case_name}\": modes field missing from BucklingResult")
        });
        let modes_list = match &modes {
            Value::List(v) => v.clone(),
            other => panic!("case \"{case_name}\": modes must be Value::List, got: {other:?}"),
        };
        assert!(
            !modes_list.is_empty(),
            "case \"{case_name}\": modes list must be non-empty"
        );
    }

    // ── (5) Physical sanity: overload eigenvalue < operating eigenvalue ────────
    //
    // P_cr = π²EI/L² is fixed by geometry; λ = P_cr/P_applied.
    // "overload" (2 kN) ⇒ λ_ovl = P_cr/2000 < P_cr/1000 = λ_op.
    let op_val = cases_map
        .get(&Value::String("operating".to_string()))
        .expect("cases map must contain \"operating\"");
    let ov_val = cases_map
        .get(&Value::String("overload".to_string()))
        .expect("cases map must contain \"overload\"");

    let lambda_op = extract_first_mode_eigenvalue(op_val)
        .expect("operating: could not extract modes[0].eigenvalue");
    let lambda_ovl = extract_first_mode_eigenvalue(ov_val)
        .expect("overload: could not extract modes[0].eigenvalue");

    assert!(
        lambda_op > 0.0,
        "operating.modes[0].eigenvalue must be positive (real FEA eigenvalue), got {lambda_op}"
    );
    assert!(
        lambda_ovl < lambda_op,
        "overload.modes[0].eigenvalue ({lambda_ovl:.4}) must be strictly less than \
         operating.modes[0].eigenvalue ({lambda_op:.4}) — \
         λ = P_cr/P_applied, so 2× applied load ⇒ ~½× eigenvalue"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// (b) SECOND EVAL REUSES COMPUTE RESULT (§8-η Final-gate)
// ─────────────────────────────────────────────────────────────────────────────

/// Dispatch counter incremented by `bmc_counting_wrapper` on every
/// buckling_multi_case trampoline call.
static BMC_DISPATCH_COUNT: AtomicU32 = AtomicU32::new(0);

/// Counting wrapper: increments `BMC_DISPATCH_COUNT` then delegates to the
/// production `solve_buckling_multi_case_trampoline`.
///
/// Registered for `"solver::buckling_multi_case"` on a fresh engine (bypasses
/// `register_compute_fns` to avoid the panic-on-double-registration).
fn bmc_counting_wrapper(
    value_inputs: &[Value],
    realization_inputs: &[RealizationReadHandle],
    options: &Value,
    prior_warm_state: Option<&OpaqueState>,
    cancellation: &CancellationHandle,
) -> ComputeOutcome {
    BMC_DISPATCH_COUNT.fetch_add(1, Ordering::SeqCst);
    // RED until step-2: this module does not exist yet → compile error.
    reify_eval::compute_targets::buckling_multi_case::solve_buckling_multi_case_trampoline(
        value_inputs,
        realization_inputs,
        options,
        prior_warm_state,
        cancellation,
    )
}

/// Cache-reuse: second `engine.eval()` on the same module must NOT
/// re-dispatch the `"solver::buckling_multi_case"` trampoline — the §8-η
/// Final-gate serves the cached result.
///
/// # RED until step-2
///
/// Before step-2: `reify_eval::compute_targets::buckling_multi_case` does not
/// exist → compile error.  Even if it existed, `solve_buckling_load_cases`
/// has no `@optimized` annotation → no dispatch ever fires →
/// `BMC_DISPATCH_COUNT` stays 0, failing the `== 1` assertion.
#[test]
fn buckling_multi_case_second_eval_reuses_compute_result() {
    // Reset for test isolation.
    BMC_DISPATCH_COUNT.store(0, Ordering::SeqCst);

    let compiled = parse_and_compile_with_stdlib(TWO_CASE_SOURCE);

    let mut engine = make_simple_engine();
    // Register only the counting wrapper for "solver::buckling_multi_case".
    // The per-case single-case buckling solver is called directly from the
    // trampoline (not via engine dispatch), so "solver::buckling" need not
    // be registered in the engine for this fixture.
    engine.register_compute_fn("solver::buckling_multi_case", bmc_counting_wrapper as ComputeFn);

    // ── First eval: trampoline dispatched once (cold start) ───────────────────
    let eval1 = engine.eval(&compiled);
    let errors1: Vec<_> = eval1
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors1.is_empty(),
        "first eval must have no Error diagnostics, got: {errors1:?}"
    );
    assert_eq!(
        BMC_DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "first eval must dispatch the buckling_multi_case trampoline exactly once"
    );

    // ── Second eval: Final-gate hit — must NOT re-dispatch ───────────────────
    let eval2 = engine.eval(&compiled);
    let errors2: Vec<_> = eval2
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors2.is_empty(),
        "second eval must have no Error diagnostics, got: {errors2:?}"
    );
    assert_eq!(
        BMC_DISPATCH_COUNT.load(Ordering::SeqCst),
        1,
        "second eval must reuse the cached ComputeNode result (§8-η Final-gate) \
         and NOT re-dispatch the buckling_multi_case trampoline"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Capstone integration test (step-7)
// ─────────────────────────────────────────────────────────────────────────────
//
// Loads the actual example fixture and exercises the full compiler → engine
// → eval_fea path including worst_buckling_case and envelope_critical_load.
//
// Expected GREEN after steps 2, 4, 6 land.

/// Capstone: load `examples/buckling_multi_case_smoke.ri`, eval the full pipeline,
/// and assert:
///   1. No Error diagnostics.
///   2. `worst` cell == "overload" (higher-load case has strictly smaller eigenvalue).
///   3. `envelope` cell == min-eigenvalue × 1kN == critical_load(overload_result, 1kN).
#[test]
fn buckling_multi_case_smoke_integration() {
    const SMOKE_SRC: &str =
        include_str!("../../../examples/buckling_multi_case_smoke.ri");

    let compiled = parse_and_compile_with_stdlib(SMOKE_SRC);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // ── (1) No Error diagnostics ───────────────────────────────────────────────
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {errors:?}"
    );

    // ── (2) worst cell == "overload" ───────────────────────────────────────────
    let worst_cell = ValueCellId::new("BucklingMultiCaseSmoke", "worst");
    let worst_val = eval_result
        .values
        .get(&worst_cell)
        .unwrap_or_else(|| panic!("cell BucklingMultiCaseSmoke.worst not found"));
    assert_eq!(
        *worst_val,
        Value::String("overload".to_string()),
        "worst_buckling_case must return \"overload\" (2× load ⇒ ~½× eigenvalue)"
    );

    // ── (3) envelope == critical_load(result_for(mcbr, "overload"), 1kN) ───────
    let mcbr_cell = ValueCellId::new("BucklingMultiCaseSmoke", "mcbr");
    let mcbr_val = eval_result
        .values
        .get(&mcbr_cell)
        .unwrap_or_else(|| panic!("cell BucklingMultiCaseSmoke.mcbr not found"));

    let cases_map = match mcbr_val {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(inner)) => inner.clone(),
            other => panic!("mcbr[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!(
            "mcbr must be Value::Map, got: {:?}",
            std::mem::discriminant(other)
        ),
    };

    let overload_result = cases_map
        .get(&Value::String("overload".to_string()))
        .expect("cases map must contain \"overload\"");

    let overload_eigenvalue = extract_first_mode_eigenvalue(overload_result)
        .expect("overload: could not extract modes[0].eigenvalue");
    assert!(
        overload_eigenvalue > 0.0,
        "overload eigenvalue must be positive, got {overload_eigenvalue}"
    );

    // expected: eigenvalue_overload × 1000 N
    let expected_si = overload_eigenvalue * 1000.0;

    let envelope_cell = ValueCellId::new("BucklingMultiCaseSmoke", "envelope");
    let envelope_val = eval_result
        .values
        .get(&envelope_cell)
        .unwrap_or_else(|| panic!("cell BucklingMultiCaseSmoke.envelope not found"));

    match envelope_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                reify_core::DimensionVector::FORCE,
                "envelope_critical_load must return a Force-dimensioned Scalar"
            );
            assert!(
                (si_value - expected_si).abs() < 1e-9,
                "envelope_critical_load mismatch: \
                 expected {expected_si} N (overload eigenvalue × 1kN), \
                 got {si_value} N"
            );
        }
        other => panic!(
            "envelope cell must be Value::Scalar{{Force}}, got: {other:?}"
        ),
    }
}
