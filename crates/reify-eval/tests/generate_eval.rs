//! End-to-end eval tests for the free-function `generate(n, |i| expr)` combinator
//! (task 3994 / structural-query ζ, PRD §5.9 / §2.3).
//!
//! `generate(n, |i| expr)` applies the lambda to indices `0..n-1` in order and
//! collects the results into a `List`.  Model: parse → `reify_compiler::compile`
//! → `Engine::eval` → assert `result.values`, mirroring `structural_query_eval.rs`.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_eval::{Engine, EvalResult};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;

/// Parse + compile + eval `source`, asserting no parse/compile Error diagnostics,
/// and return the evaluated result.
fn eval_source(source: &str) -> EvalResult {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ─── step-7: eval core ───

/// `generate(4, |i| i)` evaluates to `[Int(0), Int(1), Int(2), Int(3)]` — the
/// lambda is applied to indices 0..3 in order.
///
/// RED today: there is no free-fn `generate` arm in eval_expr's FunctionCall
/// dispatch, so it falls through to `reify_stdlib::eval_builtin` (which has no
/// `generate` builtin) → `Value::Undef`. The eval dispatch (step-8) makes this GREEN.
#[test]
fn generate_positive_count_yields_index_list() {
    let result = eval_source(
        r#"
        structure S {
            let a = generate(4, |i| i)
        }
    "#,
    );
    let a = result.values.get(&ValueCellId::new("S", "a"));
    assert_eq!(
        a,
        Some(&Value::List(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ])),
        "generate(4, |i| i) should be [0,1,2,3]; got: {:?}",
        a,
    );
}

/// `generate(0, |i| i)` evaluates to the empty list `[]`.
#[test]
fn generate_zero_count_yields_empty_list() {
    let result = eval_source(
        r#"
        structure S {
            let b = generate(0, |i| i)
        }
    "#,
    );
    let b = result.values.get(&ValueCellId::new("S", "b"));
    assert_eq!(
        b,
        Some(&Value::List(vec![])),
        "generate(0, |i| i) should be the empty list; got: {:?}",
        b,
    );
}

/// `generate(3, |i| i * 1mm)` evaluates to a 3-element list of `Length`s
/// `[0mm, 1mm, 2mm]` (in SI metres: 0.0, 0.001, 0.002) — proving a non-Int body
/// type flows through and length is preserved.
#[test]
fn generate_length_body_yields_list_of_lengths() {
    let result = eval_source(
        r#"
        structure S {
            let c = generate(3, |i| i * 1mm)
        }
    "#,
    );
    match result.values.get(&ValueCellId::new("S", "c")) {
        Some(Value::List(items)) => {
            assert_eq!(items.len(), 3, "expected 3 elements; got: {:?}", items);
            for (idx, item) in items.iter().enumerate() {
                match item {
                    Value::Scalar { si_value, .. } => {
                        let expected = idx as f64 * 0.001; // idx mm in SI metres
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "element {} should be {} m (= {}mm); got si_value {}",
                            idx,
                            expected,
                            idx,
                            si_value,
                        );
                    }
                    other => panic!("element {} should be a Length scalar; got {:?}", idx, other),
                }
            }
        }
        other => panic!("S.c should be a List of 3 lengths; got: {:?}", other),
    }
}
