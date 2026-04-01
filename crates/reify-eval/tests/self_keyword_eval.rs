//! Eval tests for `self` keyword in structures.
//!
//! These tests verify end-to-end evaluation of structures using `self.param`
//! references: that `self.x` evaluates to the same value as `x`, arithmetic
//! with self-references works correctly, and constraints using self compile
//! and evaluate without violations.

use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Severity, Value, ValueCellId};

/// Helper: parse, compile, and eval source, return eval result.
fn eval_source(source: &str) -> reify_eval::EvalResult {
    let parsed = reify_syntax::parse(source, ModulePath::single("self_eval_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    engine.eval(&compiled)
}

// ─── step-9: self.param eval produces correct value ───

#[test]
#[ignore = "requires task 153: self keyword compiler support"]
fn self_param_eval_produces_correct_value() {
    // `self.thickness` should evaluate to the same value as `thickness`.
    // 5mm = 0.005 in SI (meters).
    let result = eval_source(
        r#"structure S {
    param thickness : Scalar = 5mm
    let mirror = self.thickness
}"#,
    );

    let thickness_id = ValueCellId::new("S", "thickness");
    let mirror_id = ValueCellId::new("S", "mirror");

    let thickness_val = result
        .values
        .get(&thickness_id)
        .expect("thickness should be in eval result");
    let mirror_val = result
        .values
        .get(&mirror_id)
        .expect("mirror should be in eval result");

    // Both should be Real values equal to 0.005 (5mm in SI meters)
    match (thickness_val, mirror_val) {
        (Value::Real(t), Value::Real(m)) => {
            assert!(
                (t - 0.005).abs() < 1e-9,
                "thickness should be 0.005, got {}",
                t
            );
            assert!(
                (m - 0.005).abs() < 1e-9,
                "mirror should be 0.005, got {}",
                m
            );
            assert!(
                (t - m).abs() < 1e-15,
                "mirror should equal thickness: {} vs {}",
                t,
                m
            );
        }
        _ => panic!(
            "expected Real values, got thickness={:?}, mirror={:?}",
            thickness_val, mirror_val
        ),
    }
}

// ─── step-10: self in let arithmetic eval ───

#[test]
#[ignore = "requires task 153: self keyword compiler support"]
fn self_in_let_arithmetic_eval() {
    // `self.a + self.b` should evaluate to the sum: 3mm + 7mm = 10mm = 0.010 SI.
    let result = eval_source(
        r#"structure S {
    param a : Scalar = 3mm
    param b : Scalar = 7mm
    let sum = self.a + self.b
}"#,
    );

    let sum_id = ValueCellId::new("S", "sum");
    let sum_val = result
        .values
        .get(&sum_id)
        .expect("sum should be in eval result");

    match sum_val {
        Value::Real(v) => {
            assert!(
                (v - 0.010).abs() < 1e-9,
                "sum should be 0.010 (10mm SI), got {}",
                v
            );
        }
        _ => panic!("expected Real value for sum, got {:?}", sum_val),
    }
}

// ─── step-12: self in constraint eval ───

#[test]
#[ignore = "requires task 153: self keyword compiler support"]
fn self_in_constraint_eval() {
    // `constraint self.x > 2mm` should evaluate without errors.
    // x = 5mm > 2mm, so the constraint should be satisfied.
    let result = eval_source(
        r#"structure S {
    param x : Scalar = 5mm
    constraint self.x > 2mm
}"#,
    );

    let x_id = ValueCellId::new("S", "x");
    let x_val = result
        .values
        .get(&x_id)
        .expect("x should be in eval result");

    // x should be 0.005 (5mm in SI meters)
    match x_val {
        Value::Real(v) => {
            assert!(
                (v - 0.005).abs() < 1e-9,
                "x should be 0.005 (5mm SI), got {}",
                v
            );
        }
        _ => panic!("expected Real value for x, got {:?}", x_val),
    }
}
