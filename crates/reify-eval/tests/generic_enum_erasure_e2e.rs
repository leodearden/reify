//! Generic-enum end-to-end integration gate (task ε #4033, step-1).
//!
//! Confirms the full generic data-carrying-enum eval pipeline — construction
//! type-arg inference (γ #4031), type-preserving pattern binders (δ #4032),
//! and DCE payload-binding eval (ζ #3946) — is wired end-to-end, and that eval
//! is UNCHANGED under F-Mono type erasure (D1/INV-2, PRD
//! docs/prds/v0_6/generic-data-carrying-enums.md §8).
//!
//! Mirrors crates/reify-eval/tests/m6_data_carrying_enum.rs: parse → compile →
//! eval; extract Demo.bore / Demo.r.
//!
//! Tests:
//!   1. bore_ok_default_is_5mm (PRIMARY §1 signal) — `Ok { value: 5mm }` default
//!      → Demo.bore = 0.005 m; Demo.r = Result::Ok.
//!   2. tree_sum_total_is_3mm — recursive `Tree<Length>` (1mm + 2mm leaves)
//!      summed via nested two-arm match → Demo.total = 0.003 m (INV-5 e2e).
//!   3. bore_err_switch_is_6mm — `Err { error: "bad" }` default switch
//!      → Demo.bore = 0.006 m; Demo.r = Result::Err.
//!   4. recursive_tree_decl_emits_no_error_diagnostics — INV-5/D5: an
//!      isolated recursive generic-enum decl (Tree<T> alone, no Demo
//!      structure) emits no static-termination error (there is no
//!      termination checker).
//!   5. edit_param_err_override_of_applied_result_param_is_accepted — PRD
//!      generic-enum-type-arg-retention §7 B12 / task θ #8017: `edit_param`
//!      admits an `Err` override of a `param r : Result<Length, String>`.
//!   6. cold_eval_honours_err_override_of_applied_result_param — same B12
//!      defect on the `set_param_and_invalidate` + `eval` path (task θ #8017).
//!   7. edit_param_override_payload_is_not_checked_against_applied_type_args_c5
//!      — PRD C-5: the override gate is name-only, so an `Ok` carrying a mass
//!      is accepted for `Result<Length, String>` (task θ #8017).

use reify_core::{DimensionVector, Severity, Type, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::parse_and_compile;

// ── helper ───────────────────────────────────────────────────────────────────

fn eval_source(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile(source);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

/// Read + eval `examples/m6_generic_enum.ri`. Centralizes the path and
/// expect message shared by the example-integration tests below (amend:
/// review — dedup of repeated read_to_string + eval_source call sites).
fn eval_example() -> reify_eval::EvalResult {
    let source = std::fs::read_to_string("../../examples/m6_generic_enum.ri")
        .expect("examples/m6_generic_enum.ri should exist");
    eval_source(&source)
}

// ── test 1: PRIMARY §1 signal ─────────────────────────────────────────────────

/// `reify eval examples/m6_generic_enum.ri` → Demo.bore = 0.005 m
/// (`Ok { value: 5mm }` default, extracted through the Ok arm's `v` binder).
///
/// The PRD §1/§8 user-observable signal. RED until step-2 authors
/// examples/m6_generic_enum.ri: `read_to_string(...)` panics.
#[test]
fn bore_ok_default_is_5mm() {
    let result = eval_example();

    let bore_id = ValueCellId::new("Demo", "bore");
    let bore_val = result
        .values
        .get(&bore_id)
        .unwrap_or_else(|| panic!("Demo.bore not found in eval result"));

    match bore_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "expected Demo.bore ≈ 0.005 m (5mm), got {si_value} m"
            );
        }
        other => panic!("expected Value::Scalar for Demo.bore, got {:?}", other),
    }

    let r_id = ValueCellId::new("Demo", "r");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("Demo.r not found in eval result"));
    match r_val {
        Value::Enum { variant, .. } => {
            assert_eq!(
                variant, "Ok",
                "Demo.r should be Result::Ok (default variant)"
            );
        }
        other => panic!("expected Value::Enum for Demo.r, got {:?}", other),
    }
}

// ── test 2: recursive Tree<Length> summed via nested match (INV-5 e2e) ──────

/// `reify eval examples/m6_generic_enum.ri` → Demo.total = 0.003 m: the
/// recursive `Tree<Length>` value (`Node { left: Leaf{1mm}, right: Leaf{2mm} }`)
/// is built bottom-up and summed via a nested two-arm match — INV-5 end-to-end.
#[test]
fn tree_sum_total_is_3mm() {
    let result = eval_example();

    let total_id = ValueCellId::new("Demo", "total");
    let total_val = result
        .values
        .get(&total_id)
        .unwrap_or_else(|| panic!("Demo.total not found in eval result"));

    match total_val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.003).abs() < 1e-12,
                "expected Demo.total ≈ 0.003 m (1mm+2mm Tree<Length> leaves), got {si_value} m"
            );
            // amend: review — pin the dimension too, not just the SI magnitude,
            // so a regression that sums Tree<Length> leaves as dimensionless (or
            // with the wrong unit exponent) can't land 0.003 silently.
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "expected Demo.total to carry LENGTH dimension (Tree<Length> leaves), got {dimension:?}"
            );
        }
        other => panic!("expected Value::Scalar for Demo.total, got {:?}", other),
    }
}

// ── test 3: Err-switch — the §1 default-switch signal ────────────────────────

/// Inline source with `Err { error: "bad" }` default → Demo.bore = 0.006 m
/// (the Err arm's literal fallback), and Demo.r reports the Err tag.
#[test]
fn bore_err_switch_is_6mm() {
    let source = r#"
module m6_test_generic_enum_err

enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}

structure def Demo {
    param r : Result<Length, String> = Err { error: "bad" }

    let bore = match r {
        Ok { value: v } => v,
        Err { error: m } => 6mm
    }
}
"#;

    let result = eval_source(source);

    let bore_id = ValueCellId::new("Demo", "bore");
    let bore_val = result
        .values
        .get(&bore_id)
        .unwrap_or_else(|| panic!("Demo.bore not found in eval result"));
    match bore_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.006).abs() < 1e-12,
                "expected Demo.bore ≈ 0.006 m (Err-arm fallback 6mm), got {si_value} m"
            );
        }
        other => panic!("expected Value::Scalar for Demo.bore, got {:?}", other),
    }

    let r_id = ValueCellId::new("Demo", "r");
    let r_val = result
        .values
        .get(&r_id)
        .unwrap_or_else(|| panic!("Demo.r not found in eval result"));
    match r_val {
        Value::Enum { variant, .. } => {
            assert_eq!(variant, "Err", "Demo.r should switch to Result::Err");
        }
        other => panic!("expected Value::Enum for Demo.r, got {:?}", other),
    }
}

// ── test 4: INV-5 — recursive generic-enum decl compiles clean ──────────────

/// INV-5/D5: a well-formed recursive generic-enum declaration (`Tree<T>`'s
/// `Node` variant recurring on itself) emits NO static-termination error —
/// there is no termination checker; a `Value::Enum` is a finite, bottom-up-built
/// value (pinned at runtime by `tree_sum_total_is_3mm` above).
///
/// Uses an ISOLATED inline source containing ONLY the `Tree<T>` decl (no
/// `Demo` structure / bore / total lets), so this assertion pins the
/// recursive-decl behavior itself rather than "the whole example has zero
/// errors" (amend: review — the previous version compiled the full example,
/// which would also pass/fail on unrelated diagnostics elsewhere in the
/// file). Full-example compile-cleanliness is separately covered by
/// `examples_smoke.rs`'s glob-based gate and is implied by the eval tests
/// above (a broken compile would make them panic).
#[test]
fn recursive_tree_decl_emits_no_error_diagnostics() {
    let source = r#"
module m6_test_tree_decl_only

enum Tree<T> {
    Leaf { value: T },
    Node { left: Tree<T>, right: Tree<T> },
}
"#;

    let compiled = parse_and_compile(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        0,
        "recursive Tree<T> declaration must compile with zero Error diagnostics \
         (INV-5: no static-termination checker); got {:?}",
        errors
    );
}

// ── tests 5–6: B12 — override of an Applied-typed generic-enum param ─────────
//
// An annotated generic-enum param keeps `Type::Applied { name: "Result", .. }`
// (`resolve_enum_type_with_args` uses the bare enum name), while the runtime
// `Value::Enum` carries only the bare `type_name` and no type args (PRD
// generic-enum-type-arg-retention INV-1). Both override entry points funnel
// through `validate_param_override` → `value_type_kind_matches`.

/// `Ok` default (5mm), so an `Err` override is distinguishable by value:
/// `Demo.bore` is 5mm under the default and 6mm only if the override is honoured.
const RESULT_PARAM_SOURCE: &str = r#"
module theta_applied_result_override

enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}

structure def Demo {
    param r : Result<Length, String> = Ok { value: 5mm }

    let bore = match r {
        Ok { value: v } => v,
        Err { error: m } => 6mm
    }
}
"#;

/// The `Err` value the evaluator itself constructs: bare enum name, named
/// payload, no type-arg tag (INV-1).
fn err_override(msg: &str) -> Value {
    Value::Enum {
        type_name: "Result".into(),
        variant: "Err".into(),
        payload: vec![("error".into(), Value::String(msg.into()))],
    }
}

/// Anti-vacuity precondition. B12 exists only for an `Applied`-typed cell: were
/// `Demo.r` to resolve to a bare `Type::Enum`, the pre-θ arm would accept the
/// override and tests 5–6 would pass without exercising θ.
fn assert_r_cell_is_applied_result(compiled: &reify_compiler::CompiledModule) {
    let demo = compiled
        .templates
        .iter()
        .find(|t| t.name == "Demo")
        .expect("Demo template must be in the compiled module");
    let r_cell = demo
        .value_cells
        .iter()
        .find(|c| c.id.member == "r")
        .expect("Demo.r value cell must be in the Demo template");
    assert!(
        matches!(&r_cell.cell_type, Type::Applied { name, .. } if name == "Result"),
        "precondition: Demo.r must be typed Type::Applied {{ name: \"Result\", .. }}, got {:?}",
        r_cell.cell_type
    );
}

fn demo_value<'a>(result: &'a reify_eval::EvalResult, member: &str) -> &'a Value {
    result
        .values
        .get(&ValueCellId::new("Demo", member))
        .unwrap_or_else(|| panic!("Demo.{member} not found in eval result"))
}

fn assert_demo_bore_is(result: &reify_eval::EvalResult, expected_m: f64) {
    match demo_value(result, "bore") {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - expected_m).abs() < 1e-12,
                "expected Demo.bore ≈ {expected_m} m, got {si_value} m"
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "expected Demo.bore to carry LENGTH dimension, got {dimension:?}"
            );
        }
        other => panic!("expected Value::Scalar for Demo.bore, got {:?}", other),
    }
}

/// Asserts the `Err` override was honoured, on values (not warning prose):
/// `Demo.r` holds `Result::Err` and `Demo.bore` took the Err arm's 6mm. Under
/// the `Ok` default the same cells hold `Result::Ok` and 5mm.
fn assert_err_override_took_effect(result: &reify_eval::EvalResult) {
    match demo_value(result, "r") {
        Value::Enum { variant, .. } => assert_eq!(
            variant, "Err",
            "Demo.r must hold the overriding Result::Err, not the Ok default"
        ),
        other => panic!("expected Value::Enum for Demo.r, got {:?}", other),
    }
    assert_demo_bore_is(result, 0.006);
}

// ── test 5: edit_param path ──────────────────────────────────────────────────

/// B12, `Engine::edit_param`: overriding an `Applied`-typed `Result` param with
/// an `Err` value is ACCEPTED and the incremental re-eval honours it. Pre-θ it
/// returns `Err(EngineError::TypeKindMismatch)`: `value_type_kind_matches`
/// admitted a `Value::Enum` only into a bare `Type::Enum` cell.
#[test]
fn edit_param_err_override_of_applied_result_param_is_accepted() {
    let compiled = parse_and_compile(RESULT_PARAM_SOURCE);
    assert_r_cell_is_applied_result(&compiled);

    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    assert_demo_bore_is(&engine.eval(&compiled), 0.005);

    let edited = engine
        .edit_param(ValueCellId::new("Demo", "r"), err_override("x"))
        .unwrap_or_else(|e| panic!("B12: override of an Applied-typed Result param rejected: {e}"));

    assert_err_override_took_effect(&edited);
}

// ── test 6: cold-eval path ───────────────────────────────────────────────────

/// B12, `Engine::set_param_and_invalidate` + `Engine::eval`: the same `Err`
/// override is honoured. Pre-θ the override is skipped with a type-kind warning
/// and the `Ok` default survives (`Demo.bore` = 0.005, not 0.006).
#[test]
fn cold_eval_honours_err_override_of_applied_result_param() {
    let compiled = parse_and_compile(RESULT_PARAM_SOURCE);
    assert_r_cell_is_applied_result(&compiled);

    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    assert_demo_bore_is(&engine.eval(&compiled), 0.005);

    engine.set_param_and_invalidate(&ValueCellId::new("Demo", "r"), err_override("x"));
    let result = engine.eval(&compiled);

    assert_err_override_took_effect(&result);
}

// ── test 7: the boundary of what test 5 admits ───────────────────────────────

/// PRD generic-enum-type-arg-retention C-5: type args are compile-time only, so
/// the override gate compares the enum NAME and never the payload. An `Ok`
/// carrying a mass is therefore ACCEPTED for `Result<Length, String>`, and
/// `Demo.r` holds it unchanged. This pins where the accepted inputs stop: a
/// runtime payload check would reject here, and has to flip this test to do so.
#[test]
fn edit_param_override_payload_is_not_checked_against_applied_type_args_c5() {
    let compiled = parse_and_compile(RESULT_PARAM_SOURCE);
    assert_r_cell_is_applied_result(&compiled);

    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    assert_demo_bore_is(&engine.eval(&compiled), 0.005);

    let ok_carrying_mass = Value::Enum {
        type_name: "Result".into(),
        variant: "Ok".into(),
        payload: vec![(
            "value".into(),
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::MASS,
            },
        )],
    };
    let edited = engine
        .edit_param(ValueCellId::new("Demo", "r"), ok_carrying_mass.clone())
        .unwrap_or_else(|e| panic!("C-5: override payload was checked against the type args: {e}"));

    assert_eq!(
        demo_value(&edited, "r"),
        &ok_carrying_mass,
        "Demo.r must hold the overriding value unchanged"
    );
}
