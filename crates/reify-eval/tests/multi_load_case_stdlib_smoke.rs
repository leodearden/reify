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
/// Stage-2 readiness — when struct-ctor eval lands and the smoke fixture should
/// swap map literals for ctor calls — is probed by the `#[ignore]`d
/// `struct_ctor_eval_stage_2_readiness` test below; run via
/// `cargo test --test multi_load_case_stdlib_smoke -- --ignored`.
///
/// Bindings:
///   `cases`       = `map{"operating" => 42, "overload" => 99}` (inner Map)
///   `mcr`         = `map{"cases" => cases}` (struct-shaped outer Map)
///   `names`       = `case_names(mcr)` → `["operating", "overload"]` (lexicographic)
///   `op_result`   = `result_for(mcr, "operating")` → `42`
///   `miss_result` = `result_for(mcr, "missing")` → `Undef`
///   `mcr_ctor`    = `MultiCaseResult(cases: map{})` → `Undef` (struct-ctor tripwire; fires
///                  automatically when ctor eval lands — see `struct_ctor_eval_stage_2_readiness`)
const SMOKE_SOURCE: &str = r#"
structure def SmokeFixture {
    let cases = map{"operating" => 42, "overload" => 99}
    let mcr = map{"cases" => cases}
    let names      = case_names(mcr)
    let op_result  = result_for(mcr, "operating")
    let miss_result = result_for(mcr, "missing")
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

/// Regression guard: asserts that the `std/fea/multi_case` module is
/// registered by the stdlib loader with zero Error-severity compile
/// diagnostics.
///
/// The accessor smoke test (`multi_load_case_stdlib_smoke_e2e`) constructs the
/// `MultiCaseResult` runtime shape via raw map literals, bypassing the struct
/// constructor path. As a result it would still pass even if the loader
/// silently swallowed a compile error from `fea_multi_case.ri`. This test
/// closes that gap by inspecting the registered module directly — matching the
/// approach in `crates/reify-compiler/tests/multi_load_case_stdlib_tests.rs`.
#[test]
fn multi_load_case_stdlib_module_registers_without_errors() {
    let stdlib = reify_compiler::stdlib_loader::load_stdlib();
    let module = stdlib
        .iter()
        .find(|m| m.path.to_string() == "std/fea/multi_case")
        .expect(
            "std/fea/multi_case must be registered by the stdlib loader; \
             check that fea_multi_case.ri is included in the embedded-source \
             list and registered in stdlib_loader.rs",
        );

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "std/fea/multi_case should have no Error-severity compile diagnostics; \
         got: {errors:?}"
    );
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

    // ── struct-ctor tripwire ──────────────────────────────────────────────────
    // `MultiCaseResult(cases: map{})` is not a recognised builtin and currently
    // falls through `reify_stdlib::eval_builtin` to `Value::Undef`. This
    // assertion fires automatically on every `cargo test` run the moment ctor
    // eval starts returning a non-Undef value, giving zero-effort signalling
    // that Stage-2 has landed. When it fires, run the companion `#[ignore]`d
    // `struct_ctor_eval_stage_2_readiness` test for full contract verification:
    //   `cargo test --test multi_load_case_stdlib_smoke -- --ignored`
    let mcr_ctor = get_value(v, "mcr_ctor");
    assert!(
        mcr_ctor.is_undef(),
        "MultiCaseResult(cases: map{{}}) should still evaluate to Undef (struct-ctor eval not yet \
         implemented); got: {mcr_ctor:?} — struct-ctor eval has landed: run \
         `cargo test --test multi_load_case_stdlib_smoke -- --ignored` to verify the Stage-2 contract"
    );
}

/// Reify source: a `WorstCaseFixture` structure that exercises `worst_case`
/// (Lambda-aware accessor) through the full compile+eval pipeline.
///
/// Each per-case value is bound directly to a Sampled `Length -> Real` field
/// (per the field-def pattern in `field_eval_tests.rs`). The lambda body is
/// the identity (`|f| f`) — at runtime the dispatch arm passes each per-case
/// `Value::Field` to the lambda, which returns it unchanged; the dispatch arm
/// then collapses each Field via `field_reductions::compute_max` and returns
/// the case name with the largest scalar.
///
/// # Why identity-lambda (and not `|e| e["displacement"]`)?
///
/// The natural shape for a real worst_case call is
/// `worst_case(mcr, |e| e["displacement"])` where each case is an
/// `ElasticResult`-shaped Map. However, untyped lambda params default to
/// `Type::Real` (per `compile_expr_guarded`'s Lambda arm at expr.rs:2092),
/// and `IndexAccess` on `Real` is rejected by the type checker
/// ("cannot index into non-collection type 'Real'"). The Reify lambda
/// param-type syntax accepts only bare named types resolvable by
/// `resolve_type_name` (Bool / Int / Real / String / named dimensions),
/// so `|e: Map<String, Field<...>>|` cannot currently be expressed.
///
/// The identity-lambda variant pins the dispatch-arm contract end-to-end
/// (lambda application → `compute_max` per case → strict `>` running-best
/// → BTreeMap-lex iteration order) without requiring a richer lambda
/// param-type syntax. The full `e["displacement"]` form will become
/// expressible when richer lambda parameter types land (orthogonal work).
///
/// Engineered max values: operating→50, overload→200, transport→100.
/// Expected winner: `"overload"`.
///
/// Bindings:
///   `cases`  = `map{"operating" => disp_op, "overload" => disp_ov, "transport" => disp_tr}`
///   `mcr`    = `map{"cases" => cases}`
///   `worst`  = `worst_case(mcr, |f| f)` → `"overload"`
const WORST_CASE_SOURCE: &str = r#"
field def disp_op : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [10.0, 20.0, 50.0] } }
field def disp_ov : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [100.0, 50.0, 200.0] } }
field def disp_tr : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [30.0, 100.0, 60.0] } }

structure def WorstCaseFixture {
    let cases = map{"operating" => disp_op, "overload" => disp_ov, "transport" => disp_tr}
    let mcr = map{"cases" => cases}
    let worst = worst_case(mcr, |f| f)
}
"#;

/// Look up a `WorstCaseFixture` binding from an eval result map by member name.
fn get_worst_case_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("WorstCaseFixture", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("WorstCaseFixture.{name} not found in eval result"))
}

/// Smoke test: `worst_case(mcr, |e| e["displacement"])` returns the case name
/// with the largest per-case displacement-field max (engineered: operating=50,
/// overload=200, transport=100 → winner = "overload").
///
/// Pins the v0.3.x `worst_case` Lambda dispatch arm (in `reify-expr/src/lib.rs`,
/// modeled on `flat_map`) end-to-end through compile + eval. This test fails
/// until the dispatch arm is added — the call falls through `eval_builtin` →
/// `eval_fea` → the `worst_case` Undef stub.
#[test]
fn worst_case_three_case_returns_dominant_case_name() {
    let compiled = parse_and_compile_with_stdlib(WORST_CASE_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let worst = get_worst_case_value(&result.values, "worst");
    assert_eq!(
        worst,
        &Value::String("overload".to_string()),
        "worst_case should return \"overload\" (max=200, dominant over operating=50 and transport=100), \
         got: {worst:?}"
    );
}

/// Reify source: a `WorstCaseTieFixture` structure that exercises the
/// tie-break invariant of `worst_case` — when two or more cases share the
/// largest max, the lexicographically smallest case name must win.
///
/// Engineered: alpha and beta both have max 100; gamma has max 50.
/// Expected winner: `"alpha"` (lex-smaller of the two tied maxes).
///
/// Pins the strict-`>` running-best comparator combined with `BTreeMap`'s
/// lexicographic iteration over `Value::String` keys. A regression to `>=`
/// (or to a non-deterministic iteration order) would let `"beta"` win
/// instead. Mirrors the first-occurrence-wins discipline of
/// `argmax_argmin_index` (`field_reductions.rs:198`) and `envelope_reduce`
/// (`fea.rs`). See `eval_worst_case_dispatch` for the dispatch contract.
///
/// Uses the identity-lambda variant (`|f| f`) for the same reason as
/// `worst_case_three_case_returns_dominant_case_name` — see that test's
/// docstring.
///
/// Bindings:
///   `cases`  = `map{"alpha" => disp_alpha, "beta" => disp_beta, "gamma" => disp_gamma}`
///   `mcr`    = `map{"cases" => cases}`
///   `winner` = `worst_case(mcr, |f| f)` → `"alpha"`
const WORST_CASE_TIE_SOURCE: &str = r#"
field def disp_alpha : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [10.0, 20.0, 100.0] } }
field def disp_beta  : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [50.0, 100.0, 80.0] } }
field def disp_gamma : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [10.0, 30.0, 50.0] } }

structure def WorstCaseTieFixture {
    let cases = map{"alpha" => disp_alpha, "beta" => disp_beta, "gamma" => disp_gamma}
    let mcr = map{"cases" => cases}
    let winner = worst_case(mcr, |f| f)
}
"#;

/// Look up a `WorstCaseTieFixture` binding from an eval result map by member
/// name.
fn get_worst_case_tie_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("WorstCaseTieFixture", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("WorstCaseTieFixture.{name} not found in eval result"))
}

/// Smoke test: when two or more cases share the largest per-case max,
/// `worst_case` returns the lexicographically smallest case name (engineered:
/// alpha=100, beta=100, gamma=50 → winner = "alpha").
///
/// Pins the strict-`>` + BTreeMap-lex iteration tie-break invariant of
/// `eval_worst_case_dispatch` end-to-end through compile + eval. A regression
/// to `>=` (or to a non-deterministic iteration order) would let `"beta"` win
/// instead.
#[test]
fn worst_case_tied_max_returns_lex_smaller_case_name() {
    let compiled = parse_and_compile_with_stdlib(WORST_CASE_TIE_SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let winner = get_worst_case_tie_value(&result.values, "winner");
    assert_eq!(
        winner,
        &Value::String("alpha".to_string()),
        "worst_case should return \"alpha\" (lex-min of the two tied maxes; alpha and beta both have max=100, gamma=50), \
         got: {winner:?}"
    );
}

/// Stage-2 readiness probe: verify that the `MultiCaseResult(...)` and
/// `LoadCase(...)` struct constructors produce the correct `Value::Map` shape,
/// and that the accessors flowing from `MultiCaseResult` work end-to-end, once
/// struct-constructor eval lands.
///
/// Currently `#[ignore]`d because struct-constructor eval is not yet
/// implemented: `reify_stdlib::eval_builtin` returns `Value::Undef` for
/// unrecognised names (including `MultiCaseResult` and `LoadCase`), so the
/// `Value::Map`-shape assertions panic with the current engine.
///
/// Both structs are probed symmetrically so that a partial Stage-2 landing
/// (e.g. `MultiCaseResult` ctor works but `LoadCase` doesn't) is caught.
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
    let lc  = LoadCase(name: "tracking", loads: [], supports: [])
    let names      = case_names(mcr)
    let op_result  = result_for(mcr, "operating")
    let miss_result = result_for(mcr, "missing")
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

    // 3. `lc` is a `Value::Map` (LoadCase struct-instance shape) whose `"name"`
    //    key maps to `Value::String("tracking")`. Probed symmetrically with `mcr`
    //    so a partial Stage-2 landing (e.g. MultiCaseResult ctor works but
    //    LoadCase doesn't) is caught. Pinning the `name` field distinguishes a
    //    real LoadCase ctor result from any incidental empty-Map or wrong-named Map.
    let lc = get_value(v, "lc");
    match lc {
        Value::Map(outer) => match outer.get(&Value::String("name".to_string())) {
            Some(Value::String(name)) if name == "tracking" => {}
            Some(other) => panic!(
                "lc[\"name\"] should be Value::String(\"tracking\"), got: {other:?}"
            ),
            None => panic!("lc should have a \"name\" key, got: {lc:?}"),
        },
        _ => panic!("lc (LoadCase ctor) should be Value::Map, got: {lc:?}"),
    }

    // 4. `case_names(mcr)` returns ["operating", "overload"] in lexicographic order.
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

    // 5. `result_for(mcr, "missing")` returns Undef (silent-Undef contract per
    //    PRD task #10 deferral, re-flowed through the ctor path so Stage-2 ctor-built
    //    MCR values certify the missing-key behavior end-to-end — mirrors the
    //    map-literal coverage in `multi_load_case_stdlib_smoke_e2e`).
    let miss_result = get_value(v, "miss_result");
    assert!(
        miss_result.is_undef(),
        "result_for(mcr, \"missing\") should return Undef for a missing key on a \
         ctor-built MultiCaseResult, got: {miss_result:?}"
    );

    // 6. `result_for(mcr, "operating")` returns Value::Int(42).
    let op_result = get_value(v, "op_result");
    assert_eq!(
        op_result,
        &Value::Int(42),
        "result_for(mcr, \"operating\") should return Value::Int(42), got: {op_result:?}"
    );
}
