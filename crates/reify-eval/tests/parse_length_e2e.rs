//! End-to-end eval gate for the fallible parse builtins (task #4535):
//! `parse_length` / `parse_length_r`, evaluated via the CLI-eval path.
//!
//! Mirrors `crates/reify-eval/tests/generic_enum_erasure_e2e.rs`'s manual
//! pipeline (`parse_and_compile_with_stdlib` + a fresh `Engine`) rather than
//! `reify_test_support::eval_source`, since there is no `eval_source_with_stdlib`
//! helper and the PRELUDE `Result<T,E>` (dependency task #4035) must be in
//! scope for `parse_length_r`'s `Ok`/`Err` construction to resolve.
//!
//! Four cases: `parse_length("12mm")` → `Option::Some`, `parse_length("bogus")`
//! → `Option::None`, `parse_length_r("12mm")` → `Result::Ok`,
//! `parse_length_r("bogus")` → `Result::Err`.

use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{cell_value, mm, parse_and_compile_with_stdlib};

const SOURCE: &str = r#"
    structure S {
        let some_len = parse_length("12mm")
        let none_len = parse_length("bogus")
        let ok_result = parse_length_r("12mm")
        let err_result = parse_length_r("bogus")
    }
"#;

fn eval_source_with_stdlib(source: &str) -> reify_eval::EvalResult {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = reify_eval::Engine::new(Box::new(MockConstraintChecker::new()), None);
    engine.eval(&compiled)
}

/// `parse_length("12mm")` → `Value::Option(Some(Scalar{0.012m, LENGTH}))`.
///
/// `mm(12.0)` computes the identical `12.0 * 0.001` multiplication that
/// `parse_length_str` performs, so this is a bit-exact (not tolerance-based)
/// comparison.
#[test]
fn parse_length_some_case_evaluates_to_option_some_scalar() {
    let result = eval_source_with_stdlib(SOURCE);
    assert_eq!(
        cell_value(&result, "S", "some_len"),
        Value::Option(Some(Box::new(mm(12.0))))
    );
}

/// `parse_length("bogus")` → `Value::Option(None)`.
#[test]
fn parse_length_none_case_evaluates_to_option_none() {
    let result = eval_source_with_stdlib(SOURCE);
    assert_eq!(cell_value(&result, "S", "none_len"), Value::Option(None));
}

/// `parse_length_r("12mm")` → `Value::Enum{Result, Ok, [("value", 0.012m)]}`.
#[test]
fn parse_length_r_ok_case_evaluates_to_result_ok_enum() {
    let result = eval_source_with_stdlib(SOURCE);
    match cell_value(&result, "S", "ok_result") {
        Value::Enum {
            type_name,
            variant,
            payload,
        } => {
            assert_eq!(type_name, "Result");
            assert_eq!(variant, "Ok");
            assert_eq!(payload.len(), 1, "Ok payload should carry exactly one field, got {payload:?}");
            assert_eq!(payload[0].0, "value");
            assert_eq!(payload[0].1, mm(12.0));
        }
        other => panic!("expected Value::Enum{{Result::Ok}}, got {other:?}"),
    }
}

/// `parse_length_r("bogus")` → `Value::Enum{Result, Err, [("error", String)]}`.
#[test]
fn parse_length_r_err_case_evaluates_to_result_err_enum() {
    let result = eval_source_with_stdlib(SOURCE);
    match cell_value(&result, "S", "err_result") {
        Value::Enum {
            type_name,
            variant,
            payload,
        } => {
            assert_eq!(type_name, "Result");
            assert_eq!(variant, "Err");
            assert_eq!(payload.len(), 1, "Err payload should carry exactly one field, got {payload:?}");
            assert_eq!(payload[0].0, "error");
            assert!(
                matches!(payload[0].1, Value::String(_)),
                "expected Value::String for the 'error' payload field, got {:?}",
                payload[0].1
            );
        }
        other => panic!("expected Value::Enum{{Result::Err}}, got {other:?}"),
    }
}
