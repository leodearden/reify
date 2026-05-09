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
//!   4. Stage 1 tracking: `LoadCase(...)` and `MultiCaseResult(...)` ctor
//!      calls currently evaluate to `Value::Undef` (tripwire assertion —
//!      flips RED when struct-constructor eval lands; see coverage note).
//!
//! # Runtime shape
//!
//! `MultiCaseResult` struct instances are `Value::Map{"cases" -> Value::Map}`
//! at runtime — there is no `Value::StructureInstance` variant. Structure
//! constructors (e.g. `ElasticResult(...)`) are not builtins and evaluate to
//! `Value::Undef` (confirmed by `reify_stdlib::eval_builtin` falling through
//! to Undef for unknown names). Therefore this
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
/// # Coverage note
///
/// This file does NOT exercise `LoadCase` or `MultiCaseResult` struct
/// *definitions* through the parse→compile→eval pipeline for their field
/// values. Struct constructors (e.g. `LoadCase(...)`, `MultiCaseResult(...)`)
/// evaluate to `Value::Undef` in the current engine, so attempting to assert
/// their fields would always observe Undef regardless of struct validity.
/// Struct-presence coverage (template existence, param shapes, defaults) is
/// delegated to the compiler-level test in
/// `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs`, which
/// inspects the compiled module directly via `load_stdlib_module()`.
///
/// Stage 1 tracking bindings (`lc_ctor`, `mcr_ctor`) ARE present below to
/// pin the current `Value::Undef` runtime behavior of struct ctor calls. When
/// these assertions flip RED, struct-constructor eval has landed and Stage 2
/// becomes unblocked: swap the hand-built map literals (`cases`/`mcr`) for
/// actual `LoadCase(...)` / `MultiCaseResult(...)` calls and assert against
/// the resulting `Value::Map` shape. The Undef fallthrough is in
/// `reify_stdlib::eval_builtin` (returns Undef for unrecognised names).
///
/// Bindings:
///   `cases`       = `map{"operating" => 42, "overload" => 99}` (inner Map)
///   `mcr`         = `map{"cases" => cases}` (struct-shaped outer Map)
///   `names`       = `case_names(mcr)` → `["operating", "overload"]` (lexicographic)
///   `op_result`   = `result_for(mcr, "operating")` → `42`
///   `miss_result` = `result_for(mcr, "missing")` → `Undef`
///   `lc_ctor`     = `LoadCase(name: "tracking", loads: [], supports: [])` → `Undef` (Stage 1 tripwire)
///   `mcr_ctor`    = `MultiCaseResult(cases: map{})` → `Undef` (Stage 1 tripwire)
const SMOKE_SOURCE: &str = r#"
structure def SmokeFixture {
    let cases = map{"operating" => 42, "overload" => 99}
    let mcr = map{"cases" => cases}
    let names      = case_names(mcr)
    let op_result  = result_for(mcr, "operating")
    let miss_result = result_for(mcr, "missing")
    let lc_ctor  = LoadCase(name: "tracking", loads: [], supports: [])
    let mcr_ctor = MultiCaseResult(cases: map{})
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
    // (Note: if struct-constructor eval lands and emits Error-severity
    // diagnostics for the Stage-1 tripwire bindings `lc_ctor`/`mcr_ctor`,
    // this assertion will fire before the tripwire assertions below — that
    // is also a Stage-2 signal; see the comment block near the tripwire
    // assertions for the full migration guide.)
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

    // ── Stage 1 tracking assertion: struct ctor calls → Undef ─────────────────
    // `LoadCase(...)` and `MultiCaseResult(...)` are user-defined struct
    // constructors. In the current engine they are not builtins:
    // `reify_stdlib::eval_builtin` returns `Value::Undef` for unknown names.
    // This assertion pins that contract as a tripwire.
    //
    // When struct-constructor eval lands and produces real `Value::Map`
    // instances, BOTH of these assertions will FAIL. That is the signal to
    // perform Stage 2: swap the hand-built map literals (`cases`/`mcr` above)
    // for actual `LoadCase(...)` / `MultiCaseResult(...)` calls and update the
    // assertions to inspect the resulting `Value::Map` shape.
    //
    // Note: a panic from `parse_and_compile_with_stdlib` above (i.e. an
    // Error-severity compile diagnostic arising from the new ctor calls — for
    // example a type-checker rejecting `loads: []` once empty-list inference
    // tightens, or a real ctor evaluator rejecting missing fields) is also a
    // Stage-2 signal. The tripwire may fire at the compile stage rather than
    // here; both outcomes mean struct-ctor eval has changed.
    let lc_ctor = get_value(v, "lc_ctor");
    assert!(
        lc_ctor.is_undef(),
        "Stage 1 tripwire: LoadCase(...) ctor should currently return Undef \
         (struct-constructor eval not yet implemented); got: {lc_ctor:?}. \
         If this assertion FAILS, struct-ctor eval has landed — perform the \
         Stage 2 migration (swap map literals for ctor calls)."
    );

    let mcr_ctor = get_value(v, "mcr_ctor");
    assert!(
        mcr_ctor.is_undef(),
        "Stage 1 tripwire: MultiCaseResult(...) ctor should currently return Undef \
         (struct-constructor eval not yet implemented); got: {mcr_ctor:?}. \
         If this assertion FAILS, struct-ctor eval has landed — perform the \
         Stage 2 migration (swap map literals for ctor calls)."
    );
}

/// Stage-2 readiness probe: verify that the `MultiCaseResult(...)` struct
/// constructor and the accessors flowing from it produce the correct
/// `Value::Map` shape once struct-constructor eval lands.
///
/// Currently `#[ignore]`d because struct-constructor eval is not yet
/// implemented: `reify_stdlib::eval_builtin` returns `Value::Undef` for
/// unrecognised names (including `MultiCaseResult`), so the `Value::Map`-shape
/// assertion panics with the current engine.
///
/// To run: `cargo test --test multi_load_case_stdlib_smoke -- --ignored`
///
/// Migration cue: when this test passes, swap the `cases`/`mcr` map literals
/// in `SMOKE_SOURCE` for an actual `MultiCaseResult(cases: map{...})` ctor
/// call, verify `multi_load_case_stdlib_smoke_e2e` still passes, then delete
/// this probe.
#[test]
#[ignore = "Stage 2: struct-ctor eval not yet implemented"]
fn struct_ctor_eval_stage_2_readiness() {
    const STAGE_2_SOURCE: &str = r#"
structure def SmokeFixture {
    let mcr = MultiCaseResult(cases: map{"operating" => 42, "overload" => 99})
    let names      = case_names(mcr)
    let op_result  = result_for(mcr, "operating")
}
"#;

    let compiled = parse_and_compile_with_stdlib(STAGE_2_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // 1. No Error-severity diagnostics from the ctor call.
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics from the ctor call, \
         got: {eval_errors:?}"
    );

    let v = &result.values;

    // 2. `mcr` is a `Value::Map` whose `"cases"` key maps to a `Value::Map`.
    let mcr = get_value(v, "mcr");
    match mcr {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(_)) => {}
            Some(other) => panic!("mcr[\"cases\"] should be Value::Map, got: {other:?}"),
            None => panic!("mcr should have a \"cases\" key, got: {mcr:?}"),
        },
        _ => panic!("mcr should be Value::Map, got: {mcr:?}"),
    }

    // 3. `case_names(mcr)` returns ["operating", "overload"] in lexicographic order.
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

    // 4. `result_for(mcr, "operating")` returns Value::Int(42).
    let op_result = get_value(v, "op_result");
    assert_eq!(
        op_result,
        &Value::Int(42),
        "result_for(mcr, \"operating\") should return Value::Int(42), got: {op_result:?}"
    );
}
