//! End-to-end smoke test for the v0.3.x multi-load-case FEA stdlib structs
//! (task #3004): `LoadCase`, `MultiCaseResult`, `case_names`, `result_for`.
//!
//! Drives the new accessor free-functions through the full
//! `parse → compile_with_stdlib → eval` pipeline. Asserts:
//!   1. `case_names(mcr)` returns the cases Map keys in lexicographic order
//!      (`["operating", "overload"]`).
//!   2. `result_for(mcr, "operating")` returns the correct per-case value.
//!   3. `result_for(mcr, "missing")` returns `Value::Undef` (silent-Undef
//!      per PRD task #10 deferral).
//!
//! # Runtime shape
//!
//! `MultiCaseResult` struct instances are `Value::Map{"cases" -> Value::Map}`
//! at runtime — there is no `Value::StructureInstance` variant. Structure
//! constructors (e.g. `ElasticResult(...)`) are not builtins and evaluate to
//! `Value::Undef` (confirmed by `engine_eval.rs:115-125`). Therefore this
//! smoke test constructs the runtime-shape Maps **directly via map literals**,
//! bypassing the struct-constructor path that would Undef-out.
//!
//! The per-case inner values are simple integers (`42`, `99`) rather than
//! full `ElasticResult` Maps. `case_names` and `result_for` treat the inner
//! values as opaque — the accessors only inspect the outer shape (presence
//! of the `"cases"` key and its inner Map), so the simplification is safe
//! for smoke-test purposes.
//!
//! Mirrors the binding-level eval pattern from `kinematic_stdlib_smoke.rs`.

#![allow(clippy::mutable_key_type)]

use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Value, ValueCellId, ValueMap};

/// Reify source: a `SmokeFixture` structure that exercises `case_names` and
/// `result_for` through the full compile+eval pipeline.
///
/// `MultiCaseResult` is represented as the runtime `Value::Map{"cases" ->
/// Value::Map}` shape via nested map literals (no struct-constructor call).
///
/// Bindings:
///   `cases`      = `map{"operating" => 42, "overload" => 99}` (inner Map)
///   `mcr`        = `map{"cases" => cases}` (struct-shaped outer Map)
///   `names`      = `case_names(mcr)` → `["operating", "overload"]` (lexicographic)
///   `op_result`  = `result_for(mcr, "operating")` → `42`
///   `miss_result` = `result_for(mcr, "missing")` → `Undef`
const SMOKE_SOURCE: &str = r#"
structure def SmokeFixture {
    let cases = map{"operating" => 42, "overload" => 99}
    let mcr = map{"cases" => cases}
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
    // `result_for(mcr, "operating")` must return the value stored at that key
    // in the cases Map, which is `42` (Value::Int(42)).
    let op_result = get_value(v, "op_result");
    assert_eq!(
        op_result,
        &Value::Int(42),
        "result_for(mcr, \"operating\") should return Value::Int(42), got: {op_result:?}"
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
