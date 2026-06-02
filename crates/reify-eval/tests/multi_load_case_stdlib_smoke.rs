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

use reify_core::ValueCellId;
use reify_ir::{Value, ValueMap};
use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};

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
        .filter(|d| d.severity == reify_core::Severity::Error)
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

    // ── struct-ctor tripwire: FIRED — Stage-2 landed via SIR-α (task 3540) ───
    //
    // This tripwire was designed to fire the moment struct-ctor eval started
    // returning a non-Undef value. SIR-α (task 3540: `Value::StructureInstance`
    // foundation slice) is exactly that landing: `MultiCaseResult(...)` is a
    // `structure def` in `crates/reify-compiler/stdlib/fea_multi_case.ri`, so
    // the function-call lowering now emits `CompiledExprKind::StructureInstanceCtor`
    // (precedence over stdlib `eval_builtin`, design-decision-2) and the
    // eval handler returns a `Value::StructureInstance` — NOT a `Value::Map`.
    //
    // The tripwire is updated here to pin the SIR-α reality (struct ctor →
    // `Value::StructureInstance` named after the structure-def). Note this
    // SUPERSEDES the `#[ignore]`d `struct_ctor_eval_stage_2_readiness`
    // companion below, which still encodes the pre-SIR-α `Value::Map`
    // expectation. Reconciling that ignored test's `Value::Map` shape with
    // SIR-α's `Value::StructureInstance` is the multi-load-case FEA PRD
    // owner's call (cross-PRD seam — out of SIR-α scope; flagged via
    // escalate_info esc-3540 for steward/PRD-owner follow-up). SIR-α only
    // updates the *active* tripwire so the suite stays green.
    let mcr_ctor = get_value(v, "mcr_ctor");
    match mcr_ctor {
        Value::StructureInstance(data) => {
            assert_eq!(
                data.type_name, "MultiCaseResult",
                "MultiCaseResult(...) struct-ctor must eval to a \
                 Value::StructureInstance named \"MultiCaseResult\" (SIR-α); \
                 got type_name={:?}",
                data.type_name
            );
        }
        other => panic!(
            "struct-ctor eval has landed via SIR-α (task 3540): \
             MultiCaseResult(cases: map{{}}) must now eval to \
             Value::StructureInstance {{ type_name: \"MultiCaseResult\", .. }}, \
             got: {other:?}"
        ),
    }
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

/// Smoke test: `worst_case(mcr, |f| f)` returns the case name with the
/// largest per-case displacement-field max (engineered: operating=50,
/// overload=200, transport=100 → winner = "overload").
///
/// The fixture binds each per-case value directly to a Sampled Field
/// (rather than to an `ElasticResult`-shaped Map), so the identity lambda
/// `|f| f` exercises the full dispatch contract — see
/// `WORST_CASE_SOURCE`'s docstring for why the natural shape
/// `worst_case(mcr, |e| e["displacement"])` is not yet expressible from
/// Reify source.
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

/// Look up a `WorstCaseNegativesFixture` binding from an eval result map by
/// member name.
fn get_worst_case_negative_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("WorstCaseNegativesFixture", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("WorstCaseNegativesFixture.{name} not found in eval result"))
}

/// Compile `source`, eval it, assert no Error-severity diagnostics, and assert
/// that the named `WorstCaseNegativesFixture` binding evaluates to
/// `Value::Undef`.
///
/// `call_ctx` is a short description of the `worst_case(...)` call under test
/// (e.g. `"worst_case(mcr)"`) used in assertion failure messages.
///
/// Shared by all 7 per-guard negative tests below; each test supplies only its
/// own SOURCE constant and binding name, keeping the tests to one-liner calls.
fn check_worst_case_negative(source: &str, binding: &str, call_ctx: &str) {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "{call_ctx}: eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = get_worst_case_negative_value(&result.values, binding);
    assert!(
        v.is_undef(),
        "{call_ctx}: expected Value::Undef from guard fall-through, got: {v:?}"
    );
}

/// Reify source for the `arity_one` negative: `worst_case(mcr)` has only one
/// argument. The inline dispatch arm in `eval_expr` guards
/// `evaluated_args.len() == 2`, so 1-arg calls fall through to
/// `reify_stdlib::eval_builtin` → `eval_fea` → the permanent `worst_case`
/// Undef stub.
const WORST_CASE_ARITY_ONE_SOURCE: &str = r#"
field def disp_neg : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [10.0, 20.0, 50.0] } }

structure def WorstCaseNegativesFixture {
    let cases = map{"a" => disp_neg}
    let mcr = map{"cases" => cases}
    let arity_one = worst_case(mcr)
}
"#;

/// Pins the wrong-arity (1 arg) fall-through: `worst_case(mcr)` must return
/// `Value::Undef` because the inline dispatch arm only fires for
/// `evaluated_args.len() == 2`; 1-arg calls fall through to the `eval_fea`
/// permanent Undef stub.
#[test]
fn worst_case_arity_one_returns_undef() {
    check_worst_case_negative(
        WORST_CASE_ARITY_ONE_SOURCE,
        "arity_one",
        "worst_case(mcr) — 1 arg",
    );
}

/// Reify source for the `arity_three` negative: `worst_case(mcr, |f| f, |f| f)`
/// has three arguments. The same wrong-arity fall-through as `arity_one` applies.
const WORST_CASE_ARITY_THREE_SOURCE: &str = r#"
field def disp_neg : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [10.0, 20.0, 50.0] } }

structure def WorstCaseNegativesFixture {
    let cases = map{"a" => disp_neg}
    let mcr = map{"cases" => cases}
    let arity_three = worst_case(mcr, |f| f, |f| f)
}
"#;

/// Pins the wrong-arity (3 args) fall-through: `worst_case(mcr, |f| f, |f| f)`
/// must return `Value::Undef` for the same reason as the 1-arg case — the inline
/// dispatch arm requires exactly 2 evaluated args.
#[test]
fn worst_case_arity_three_returns_undef() {
    check_worst_case_negative(
        WORST_CASE_ARITY_THREE_SOURCE,
        "arity_three",
        "worst_case(mcr, |f| f, |f| f) — 3 args",
    );
}

/// Reify source for the `non_map_first` negative: `worst_case(42, |f| f)` passes
/// a scalar as the first arg. Pins the `match &args[0] { Value::Map(m) => m, _ =>
/// return Value::Undef }` guard in `eval_worst_case_dispatch`.
const WORST_CASE_NON_MAP_FIRST_SOURCE: &str = r#"
structure def WorstCaseNegativesFixture {
    let non_map_first = worst_case(42, |f| f)
}
"#;

/// Pins the non-Map first-arg guard in `eval_worst_case_dispatch`:
/// `worst_case(42, |f| f)` must return `Value::Undef` because the first-arg
/// pattern match requires `Value::Map`.
#[test]
fn worst_case_non_map_first_arg_returns_undef() {
    check_worst_case_negative(
        WORST_CASE_NON_MAP_FIRST_SOURCE,
        "non_map_first",
        "worst_case(42, |f| f) — non-Map first arg",
    );
}

/// Reify source for the `non_lambda_second` negative: `worst_case(mcr, 42)` passes
/// a scalar as the second arg. Pins the `matches!(&args[1], Value::Lambda { .. })`
/// guard in `eval_worst_case_dispatch`.
const WORST_CASE_NON_LAMBDA_SECOND_SOURCE: &str = r#"
field def disp_neg : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [10.0, 20.0, 50.0] } }

structure def WorstCaseNegativesFixture {
    let cases = map{"a" => disp_neg}
    let mcr = map{"cases" => cases}
    let non_lambda_second = worst_case(mcr, 42)
}
"#;

/// Pins the non-Lambda second-arg guard in `eval_worst_case_dispatch`:
/// `worst_case(mcr, 42)` must return `Value::Undef` because the `matches!`
/// check requires `Value::Lambda { .. }`.
#[test]
fn worst_case_non_lambda_second_arg_returns_undef() {
    check_worst_case_negative(
        WORST_CASE_NON_LAMBDA_SECOND_SOURCE,
        "non_lambda_second",
        "worst_case(mcr, 42) — non-Lambda second arg",
    );
}

/// Reify source for the `no_cases_key` negative: `worst_case(map{"foo" => 1}, |f| f)`
/// passes a Map without a `"cases"` key. Pins `outer.get(&Value::String("cases"))`
/// returning `None` → `_ => return Value::Undef` in `eval_worst_case_dispatch`.
const WORST_CASE_MISSING_CASES_KEY_SOURCE: &str = r#"
structure def WorstCaseNegativesFixture {
    let no_cases_key = worst_case(map{"foo" => 1}, |f| f)
}
"#;

/// Pins the missing-`"cases"`-key guard in `eval_worst_case_dispatch`:
/// `worst_case(map{"foo" => 1}, |f| f)` must return `Value::Undef` because
/// `outer.get(&Value::String("cases"))` returns `None`.
#[test]
fn worst_case_missing_cases_key_returns_undef() {
    check_worst_case_negative(
        WORST_CASE_MISSING_CASES_KEY_SOURCE,
        "no_cases_key",
        "worst_case(map{\"foo\" => 1}, |f| f) — missing \"cases\" key",
    );
}

/// Reify source for the `cases_not_map` negative:
/// `worst_case(map{"cases" => 42}, |f| f)` passes a Map whose `"cases"` key holds
/// a scalar. Pins the `Some(Value::Map(c))` arm: non-Map `"cases"` value hits
/// `_ => return Value::Undef` in `eval_worst_case_dispatch`.
const WORST_CASE_CASES_VALUE_NOT_MAP_SOURCE: &str = r#"
structure def WorstCaseNegativesFixture {
    let cases_not_map = worst_case(map{"cases" => 42}, |f| f)
}
"#;

/// Pins the non-Map `"cases"`-value guard in `eval_worst_case_dispatch`:
/// `worst_case(map{"cases" => 42}, |f| f)` must return `Value::Undef` because
/// the `cases` match arm requires `Some(Value::Map(c))` and `42` is not a Map.
#[test]
fn worst_case_cases_value_not_map_returns_undef() {
    check_worst_case_negative(
        WORST_CASE_CASES_VALUE_NOT_MAP_SOURCE,
        "cases_not_map",
        "worst_case(map{\"cases\" => 42}, |f| f) — cases not a Map",
    );
}

/// Reify source for the `lambda_non_field` negative: `worst_case(mcr, |f| 42)`
/// uses a lambda that returns a scalar. Pins `field_reductions::compute_max`
/// returning Undef on non-Field input → `as_f64()` returns None → case skipped
/// via `_ => continue`. With all cases skipped, `best` stays None and the
/// dispatch arm returns `Value::Undef`.
const WORST_CASE_LAMBDA_NON_FIELD_SOURCE: &str = r#"
field def disp_neg : Length -> Real { source = sampled { grid = "RegularGrid1" bounds = bbox(point3(0.0m, 0.0m, 0.0m), point3(2.0m, 0.0m, 0.0m)) spacing = 1.0m interpolation = "Linear" data = [10.0, 20.0, 50.0] } }

structure def WorstCaseNegativesFixture {
    let cases = map{"a" => disp_neg}
    let mcr = map{"cases" => cases}
    let lambda_non_field = worst_case(mcr, |f| 42)
}
"#;

/// Pins the non-Field lambda-return guard in `eval_worst_case_dispatch`:
/// `worst_case(mcr, |f| 42)` must return `Value::Undef` because `compute_max`
/// returns Undef on the scalar `42`, `as_f64()` returns None, the single case
/// is skipped, and with no case yielding a finite max the function returns
/// `Value::Undef`.
#[test]
fn worst_case_lambda_returns_non_field_returns_undef() {
    check_worst_case_negative(
        WORST_CASE_LAMBDA_NON_FIELD_SOURCE,
        "lambda_non_field",
        "worst_case(mcr, |f| 42) — lambda returns non-Field",
    );
}

// ── solve_load_cases e2e smoke tests (task 3005) ─────────────────────────────

/// Reify source exercising `solve_load_cases` with two `LoadCase` instances
/// (different loads, shared supports and options).
///
/// # What this source exercises
///
/// - `solve_load_cases` is compiled from the stdlib declaration in
///   `fea_multi_case.ri` and resolved by the name-dispatch interceptor in
///   `crates/reify-expr/src/lib.rs::eval_solve_load_cases`.
/// - The two cases differ only in the `force` field of their `PointLoad`
///   so they share the same geometry/material/options and exercise the
///   "different loads, same mesh" scenario.
/// - `ElasticOptions()` is passed explicitly (not via the default) so the
///   test is independent of whether default-padding fires.
///
/// # Bindings
///   `result`       = `solve_load_cases(...)` → `MultiCaseResult`-shaped Map
///   `case_list`    = `case_names(result)` → `["operating", "overload"]`
///   `op_case`      = `result_for(result, "operating")` → per-case ElasticResult
///   `missing_case` = `result_for(result, "missing")` → `Value::Undef`
const SOLVE_LOAD_CASES_SOURCE: &str = r#"
structure def SolveLoadCasesFixture {
    let ci        = ConstitutiveLawInput(law: Steel_AISI_1045())
    let lc1       = LoadCase(
        name:     "operating",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let lc2       = LoadCase(
        name:     "overload",
        loads:    [PointLoad(point: "tip", force: 2000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let result        = solve_load_cases(ci.law, 1000mm, 100mm, 100mm, [lc1, lc2], ElasticOptions())
    let case_list     = case_names(result)
    let op_case       = result_for(result, "operating")
    let missing_case  = result_for(result, "missing")
}
"#;

fn get_slcf_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("SolveLoadCasesFixture", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("SolveLoadCasesFixture.{name} not found in eval result"))
}

/// step-1 RED: `solve_load_cases` with two cases returns a
/// `MultiCaseResult`-shaped `Value::Map` with exactly the two case keys.
/// Also verifies the silent-Undef contract via `result_for(result, "missing")`.
///
/// Fails RED until step-2 adds the `eval_solve_load_cases` interceptor in
/// `crates/reify-expr/src/lib.rs` — without it the contract body
/// `{ MultiCaseResult(cases: map{}) }` evaluates to a `Value::StructureInstance`
/// with an empty cases map, so `case_names` returns `[]` not `["operating",
/// "overload"]` and `result_for(result, "operating")` returns `Undef`.
#[test]
fn solve_load_cases_two_cases_returns_mcr_shape() {
    let compiled = parse_and_compile_with_stdlib(SOLVE_LOAD_CASES_SOURCE);
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;

    // ── result is a Value::Map with a "cases" key ─────────────────────────────
    // The eval interceptor (eval_solve_load_cases) builds a
    // Value::Map{"cases" -> Value::Map<String, Value>}, not a
    // Value::StructureInstance. This shape is required for compatibility with
    // extract_cases_map in crates/reify-stdlib/src/fea.rs (used by
    // case_names / result_for / envelope_* / linear_combine).
    let result_val = get_slcf_value(v, "result");
    match result_val {
        Value::Map(outer) => {
            assert!(
                outer.contains_key(&Value::String("cases".to_string())),
                "solve_load_cases result must be Value::Map with a \"cases\" key; \
                 outer keys: {:?}",
                outer.keys().collect::<Vec<_>>()
            );
            match outer.get(&Value::String("cases".to_string())) {
                Some(Value::Map(cases)) => {
                    assert_eq!(
                        cases.len(),
                        2,
                        "cases map must have exactly 2 entries (\"operating\", \"overload\"); \
                         got {} entries: {:?}",
                        cases.len(),
                        cases.keys().collect::<Vec<_>>()
                    );
                    assert!(
                        cases.contains_key(&Value::String("operating".to_string())),
                        "cases map must contain \"operating\" key; got: {:?}",
                        cases.keys().collect::<Vec<_>>()
                    );
                    assert!(
                        cases.contains_key(&Value::String("overload".to_string())),
                        "cases map must contain \"overload\" key; got: {:?}",
                        cases.keys().collect::<Vec<_>>()
                    );
                    // Per-case values must be non-Undef (some ElasticResult shape)
                    let op_val = cases.get(&Value::String("operating".to_string())).unwrap();
                    assert!(
                        !op_val.is_undef(),
                        "per-case ElasticResult for \"operating\" must be non-Undef; got: {op_val:?}"
                    );
                    let ov_val = cases.get(&Value::String("overload".to_string())).unwrap();
                    assert!(
                        !ov_val.is_undef(),
                        "per-case ElasticResult for \"overload\" must be non-Undef; got: {ov_val:?}"
                    );
                }
                other => {
                    panic!("solve_load_cases result[\"cases\"] must be Value::Map, got: {other:?}")
                }
            }
        }
        other => panic!(
            "solve_load_cases must return Value::Map (not {:?}); \
             without the eval interceptor the contract body returns \
             Value::StructureInstance{{MultiCaseResult}} — add \
             eval_solve_load_cases to crates/reify-expr/src/lib.rs",
            std::mem::discriminant(other)
        ),
    }

    // ── case_names(result) must return ["operating", "overload"] ─────────────
    // Relies on extract_cases_map, which only accepts Value::Map.
    let case_list = get_slcf_value(v, "case_list");
    assert_eq!(
        case_list,
        &Value::List(vec![
            Value::String("operating".to_string()),
            Value::String("overload".to_string()),
        ]),
        "case_names(result) must return [\"operating\", \"overload\"] in \
         lexicographic order; got: {case_list:?}"
    );

    // ── result_for(result, "operating") must be non-Undef ────────────────────
    let op_case = get_slcf_value(v, "op_case");
    assert!(
        !op_case.is_undef(),
        "result_for(result, \"operating\") must return a non-Undef per-case \
         value (ElasticResult shape); got: {op_case:?}"
    );

    // ── result_for(result, "missing") → Undef (silent-Undef contract) ────────
    let missing_case = get_slcf_value(v, "missing_case");
    assert!(
        missing_case.is_undef(),
        "result_for(result, \"missing\") must return Undef for a missing key \
         (silent-Undef per PRD task #10 deferral); got: {missing_case:?}"
    );
}

// ── Per-case options test (step-3) ───────────────────────────────────────────

/// Reify source exercising per-case `ElasticOptions` resolution.
///
/// Case A (`"high_res"`) has an explicit `options: some(ElasticOptions(...))` with a
/// non-default `max_iter` value. Case B (`"low_res"`) omits `options`, defaulting to
/// `none`, so it should inherit the shared `ElasticOptions()` passed to
/// `solve_load_cases`.
///
/// Both cases should appear in the `MultiCaseResult`-shaped map with non-Undef
/// per-case results — verifying that the `options: some(...)` syntax compiles, evaluates,
/// and does not break the per-case solve dispatch.
///
/// # Architecture note (RED/GREEN boundary)
///
/// With `make_simple_engine()`, `invoke_solve_elastic_static` evaluates the
/// `solve_elastic_static` contract body (`ElasticResult()`), which always returns a
/// `Value::StructureInstance{ElasticResult}` regardless of which `ElasticOptions` were
/// used. Therefore this test CANNOT verify at the E2E level that per-case options were
/// actually passed to the solver — it only verifies structural correctness (both cases
/// appear and are non-Undef). True per-case options observability requires the
/// `@optimized` FEA trampoline (out of scope for `make_simple_engine`).
///
/// The RED/GREEN distinction for step-3 vs step-4 is therefore expressed through the
/// correctness of per-case options RESOLUTION in `eval_solve_load_cases`
/// (`crates/reify-expr/src/lib.rs`): step-2 uses shared options for all cases; step-4
/// reads each case's own `options` field.
///
/// Bindings:
///   `lc_high`  = `LoadCase(name: "high_res", ..., options: some(ElasticOptions(max_iter: 500)))`
///   `lc_low`   = `LoadCase(name: "low_res", ...)` (options: none, defaults to shared)
///   `result`   = `solve_load_cases(ci.law, ..., [lc_high, lc_low], ElasticOptions())`
///   `case_list` = `case_names(result)` → `["high_res", "low_res"]`
const PER_CASE_OPTIONS_SOURCE: &str = r#"
structure def PerCaseOptionsFixture {
    let ci       = ConstitutiveLawInput(law: Steel_AISI_1045())
    let lc_high  = LoadCase(
        name:     "high_res",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
        options:  some(ElasticOptions(max_iter: 500)),
    )
    let lc_low   = LoadCase(
        name:     "low_res",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let result     = solve_load_cases(ci.law, 1000mm, 100mm, 100mm, [lc_high, lc_low], ElasticOptions())
    let case_list  = case_names(result)
}
"#;

fn get_pcof_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("PerCaseOptionsFixture", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("PerCaseOptionsFixture.{name} not found in eval result"))
}

/// step-3: `solve_load_cases` with two cases — one with per-case
/// `options: some(ElasticOptions(max_iter: 500))` and one with default `options: none` —
/// returns a `MultiCaseResult`-shaped map with exactly two keys.
///
/// Verifies:
///  - The per-case `options: some(...)` syntax compiles without Error diagnostics.
///  - Both `"high_res"` and `"low_res"` appear in the result map.
///  - Both per-case values are non-Undef (the solve did not fall through to Undef on
///    the per-case options path).
///  - `case_names(result)` returns the keys in lexicographic order.
///
/// See `PER_CASE_OPTIONS_SOURCE` for the architecture note explaining why this test
/// cannot distinguish step-2 (shared options for all cases) from step-4 (per-case
/// options resolution) using `make_simple_engine()`.
#[test]
fn solve_load_cases_per_case_options_both_cases_appear() {
    let compiled = parse_and_compile_with_stdlib(PER_CASE_OPTIONS_SOURCE);
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics with per-case options, \
         got: {eval_errors:?}"
    );

    let v = &result.values;

    // ── result is a MultiCaseResult-shaped map with "cases" key ──────────────
    let result_val = get_pcof_value(v, "result");
    match result_val {
        Value::Map(outer) => {
            match outer.get(&Value::String("cases".to_string())) {
                Some(Value::Map(cases)) => {
                    assert_eq!(
                        cases.len(),
                        2,
                        "cases map must have exactly 2 entries; \
                         got {} entries: {:?}",
                        cases.len(),
                        cases.keys().collect::<Vec<_>>()
                    );
                    // "high_res" (per-case options: some) must be present and non-Undef
                    let high = cases
                        .get(&Value::String("high_res".to_string()))
                        .unwrap_or_else(|| {
                            panic!(
                                "cases map must contain \"high_res\" key; got: {:?}",
                                cases.keys().collect::<Vec<_>>()
                            )
                        });
                    assert!(
                        !high.is_undef(),
                        "per-case result for \"high_res\" (options: some) must be non-Undef; \
                         got: {high:?}"
                    );
                    // "low_res" (per-case options: none → uses shared) must be present and non-Undef
                    let low = cases
                        .get(&Value::String("low_res".to_string()))
                        .unwrap_or_else(|| {
                            panic!(
                                "cases map must contain \"low_res\" key; got: {:?}",
                                cases.keys().collect::<Vec<_>>()
                            )
                        });
                    assert!(
                        !low.is_undef(),
                        "per-case result for \"low_res\" (options: none → shared) must be non-Undef; \
                         got: {low:?}"
                    );
                }
                other => panic!("result[\"cases\"] must be Value::Map, got: {other:?}"),
            }
        }
        other => panic!(
            "solve_load_cases must return Value::Map, got discriminant: {:?}",
            std::mem::discriminant(other)
        ),
    }

    // ── case_names returns both keys in lexicographic order ──────────────────
    let case_list = get_pcof_value(v, "case_list");
    assert_eq!(
        case_list,
        &Value::List(vec![
            Value::String("high_res".to_string()),
            Value::String("low_res".to_string()),
        ]),
        "case_names must return [\"high_res\", \"low_res\"] in lexicographic order; \
         got: {case_list:?}"
    );
}

// ── Volume-mesh cache-reuse regression (step-5) ──────────────────────────────

/// Reify source exercising a 1-case `solve_load_cases` for the baseline.
///
/// Used by `solve_load_cases_two_case_reuse_no_extra_realization` to verify
/// that the 2-case solve produces the same number of per-case mesh realizations
/// as the 1-case solve (modulo the different `loads` per case).
///
/// Bindings:
///   `result` = `solve_load_cases(ci.law, 1000mm, 100mm, 100mm, [lc1], ElasticOptions())`
const SINGLE_CASE_SOURCE: &str = r#"
structure def SingleCaseFixture {
    let ci   = ConstitutiveLawInput(law: Steel_AISI_1045())
    let lc1  = LoadCase(
        name:     "only",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let result   = solve_load_cases(ci.law, 1000mm, 100mm, 100mm, [lc1], ElasticOptions())
}
"#;

/// Reify source exercising a 2-case `solve_load_cases` with DIFFERENT loads
/// but identical geometry/material/options — the scenario where the
/// volume-mesh ComputeNode would be reused across cases in a real FEA engine.
///
/// Bindings:
///   `result` = `solve_load_cases(ci.law, 1000mm, 100mm, 100mm, [lc1, lc2], ElasticOptions())`
const TWO_CASE_SHARED_MESH_SOURCE: &str = r#"
structure def TwoCaseSharedMeshFixture {
    let ci   = ConstitutiveLawInput(law: Steel_AISI_1045())
    let lc1  = LoadCase(
        name:     "light",
        loads:    [PointLoad(point: "tip", force: 1000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let lc2  = LoadCase(
        name:     "heavy",
        loads:    [PointLoad(point: "tip", force: 5000.0)],
        supports: [FixedSupport(target: "root")],
    )
    let result   = solve_load_cases(ci.law, 1000mm, 100mm, 100mm, [lc1, lc2], ElasticOptions())
}
"#;

fn get_single_case_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("SingleCaseFixture", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("SingleCaseFixture.{name} not found in eval result"))
}

fn get_two_case_shared_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
    let id = ValueCellId::new("TwoCaseSharedMeshFixture", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("TwoCaseSharedMeshFixture.{name} not found in eval result"))
}

/// Entry-count guard: 1-case and 2-case `solve_load_cases` produce the
/// expected number of per-case entries (1 and 2 respectively), both non-Undef.
///
/// # What this verifies
///
/// - A 1-case solve (baseline) produces a MultiCaseResult with exactly 1 entry.
/// - A 2-case solve on the SAME body/material/options but DIFFERENT loads produces
///   exactly 2 distinct entries — verifying each case is solved independently.
/// - Neither case result is `Value::Undef` — neither solve fell through to the
///   silent-Undef path.
///
/// # What this does NOT verify (and why the name was corrected)
///
/// This test was originally named `solve_load_cases_two_case_reuse_no_extra_realization`
/// and framed as a mesh cache-reuse regression guard. That framing was misleading:
/// the required `CacheStats.realization_entries` API does not exist in the current
/// `reify_eval::CacheStats` (which only exposes `cache_hits`, `cache_misses`,
/// `early_cutoffs`), and `invoke_solve_elastic_static` evaluates the contract body
/// DIRECTLY without routing through the `@optimized` trampoline, so no volume-mesh
/// ComputeNode is created. The 1-case/2-case entry-count comparison is the only
/// observable assertion available with `make_simple_engine()`, and it is fully
/// covered by `solve_load_cases_two_cases_returns_mcr_shape` — this test adds the
/// 1-case baseline count for completeness.
///
/// True mesh-reuse verification requires either:
///   (a) Routing the per-case solve through the `@optimized` trampoline, OR
///   (b) A new `CacheStats.realization_entries` counter.
/// Both are out of scope for task 3005 and deferred to a follow-up task.
#[test]
fn solve_load_cases_one_vs_two_cases_entry_count() {
    // ── 1-case baseline ───────────────────────────────────────────────────────
    let compiled1 = parse_and_compile_with_stdlib(SINGLE_CASE_SOURCE);
    let mut engine1 = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine1);
    let result1 = engine1.eval(&compiled1);

    let eval_errors1 = collect_errors(&result1.diagnostics);
    assert!(
        eval_errors1.is_empty(),
        "1-case solve: no Error-severity diagnostics expected, got: {eval_errors1:?}"
    );

    let single_result = get_single_case_value(&result1.values, "result");
    let single_cases = match single_result {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(c)) => c,
            other => panic!("1-case result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!("1-case result must be Value::Map, got: {other:?}"),
    };
    assert_eq!(
        single_cases.len(),
        1,
        "1-case solve must produce exactly 1 case entry; \
         got {} entries: {:?}",
        single_cases.len(),
        single_cases.keys().collect::<Vec<_>>()
    );
    assert!(
        single_cases.contains_key(&Value::String("only".to_string())),
        "1-case solve must contain key \"only\"; got: {:?}",
        single_cases.keys().collect::<Vec<_>>()
    );
    let only_val = single_cases
        .get(&Value::String("only".to_string()))
        .unwrap();
    assert!(
        !only_val.is_undef(),
        "1-case \"only\" result must be non-Undef; got: {only_val:?}"
    );

    // ── 2-case solve (same body, different loads) ─────────────────────────────
    let compiled2 = parse_and_compile_with_stdlib(TWO_CASE_SHARED_MESH_SOURCE);
    let mut engine2 = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine2);
    let result2 = engine2.eval(&compiled2);

    let eval_errors2 = collect_errors(&result2.diagnostics);
    assert!(
        eval_errors2.is_empty(),
        "2-case solve: no Error-severity diagnostics expected, got: {eval_errors2:?}"
    );

    let two_result = get_two_case_shared_value(&result2.values, "result");
    let two_cases = match two_result {
        Value::Map(outer) => match outer.get(&Value::String("cases".to_string())) {
            Some(Value::Map(c)) => c,
            other => panic!("2-case result[\"cases\"] must be Value::Map, got: {other:?}"),
        },
        other => panic!("2-case result must be Value::Map, got: {other:?}"),
    };
    assert_eq!(
        two_cases.len(),
        2,
        "2-case solve must produce exactly 2 case entries; \
         got {} entries: {:?}",
        two_cases.len(),
        two_cases.keys().collect::<Vec<_>>()
    );
    for key in ["light", "heavy"] {
        let val = two_cases
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("2-case result must contain key \"{key}\""));
        assert!(
            !val.is_undef(),
            "2-case \"{key}\" result must be non-Undef; got: {val:?}"
        );
    }
}

// ── Stage-2 readiness probe ───────────────────────────────────────────────────

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
            Some(other) => {
                panic!("lc[\"name\"] should be Value::String(\"tracking\"), got: {other:?}")
            }
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
