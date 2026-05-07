//! End-to-end smoke test for the v0.3.x multi-load-case FEA stdlib structs
//! (task #3004): `LoadCase`, `MultiCaseResult`, `case_names`, `result_for`.
//!
//! Drives the new stdlib types and accessor free-functions through the full
//! `parse → compile_with_stdlib → eval` pipeline. Asserts:
//!   1. `case_names(mcr)` returns the cases Map keys in lexicographic order
//!      (`["operating", "overload"]`).
//!   2. `result_for(mcr, "operating")` returns the exact per-case
//!      `ElasticResult` Map value.
//!   3. `result_for(mcr, "missing")` returns `Value::Undef` (silent-Undef
//!      per PRD task #10 deferral).
//!
//! Mirrors the binding-level eval pattern from `kinematic_stdlib_smoke.rs`.

#![allow(clippy::mutable_key_type)]

use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Value, ValueCellId, ValueMap};

/// Reify source: a `SmokeFixture` structure that exercises `MultiCaseResult`,
/// `case_names`, and `result_for`.
///
/// Bindings:
///   `er_op`       = `ElasticResult(...)` with `iterations: 42`
///   `er_ov`       = `ElasticResult(...)` with `iterations: 99`
///   `mcr`         = `MultiCaseResult(cases: map{"operating" => er_op, "overload" => er_ov})`
///   `names`       = `case_names(mcr)` → `["operating", "overload"]` (lexicographic)
///   `op_result`   = `result_for(mcr, "operating")` → the `er_op` ElasticResult
///   `miss_result` = `result_for(mcr, "missing")` → `Undef`
///
/// `ElasticResult` field values are minimal fixtures that satisfy the
/// `iterations >= 0` and `max_von_mises >= 0` constraints:
///   `displacement: 0.0`, `stress: 0.0`, `max_von_mises: 0Pa`,
///   `converged: true`, `iterations: 42 / 99`.
const SMOKE_SOURCE: &str = r#"
structure def SmokeFixture {
    let er_op = ElasticResult(
        displacement: 0.0,
        stress: 0.0,
        max_von_mises: 0Pa,
        converged: true,
        iterations: 42
    )
    let er_ov = ElasticResult(
        displacement: 0.0,
        stress: 0.0,
        max_von_mises: 0Pa,
        converged: true,
        iterations: 99
    )
    let mcr = MultiCaseResult(cases: map{"operating" => er_op, "overload" => er_ov})
    let names      = case_names(mcr)
    let op_result  = result_for(mcr, "operating")
    let miss_result = result_for(mcr, "missing")
}
"#;

/// Look up a `SmokeFixture` binding from an eval result map by member name.
fn get_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("SmokeFixture", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("SmokeFixture.{name} not found in eval result"))
}

/// Smoke test: compile and eval the fixture source; assert all three accessor
/// bindings have their expected values.
#[test]
fn multi_load_case_stdlib_smoke_e2e() {
    // Compile. Any Error-severity compile diagnostics panic inside
    // `parse_and_compile_with_stdlib`.
    let compiled = parse_and_compile_with_stdlib(SMOKE_SOURCE);

    // Eval and capture results.
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // No Error-severity diagnostics from eval.
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;

    // ── case_names ────────────────────────────────────────────────────────────
    // `case_names(mcr)` must return the cases Map keys in BTreeMap
    // lexicographic order: "operating" < "overload".
    let names = get_value(v, "names");
    assert_eq!(
        names,
        &Value::List(vec![
            Value::String("operating".to_string()),
            Value::String("overload".to_string()),
        ]),
        "case_names(mcr) should return [\"operating\", \"overload\"] in lexicographic order, \
         got: {names:?}"
    );

    // ── result_for existing key ───────────────────────────────────────────────
    // `result_for(mcr, "operating")` must return the same `ElasticResult` Map
    // that the `er_op` binding holds. Compare by deep equality against the
    // already-evaluated `er_op` value — no need to re-construct the pressure
    // scalar in Rust.
    let er_op = get_value(v, "er_op");
    let op_result = get_value(v, "op_result");
    assert_eq!(
        op_result, er_op,
        "result_for(mcr, \"operating\") should equal the er_op ElasticResult value, \
         got: {op_result:?}"
    );

    // ── result_for missing key → Undef ────────────────────────────────────────
    // `result_for(mcr, "missing")` must return `Value::Undef` (silent-Undef
    // per PRD task #10 deferral, matching the `envelope_*` convention).
    let miss_result = get_value(v, "miss_result");
    assert!(
        miss_result.is_undef(),
        "result_for(mcr, \"missing\") should return Undef for a missing key, \
         got: {miss_result:?}"
    );
}
