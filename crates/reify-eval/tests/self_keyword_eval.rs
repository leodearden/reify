//! Eval tests for `self` keyword in structures.
//!
//! These tests verify end-to-end evaluation of structures using `self.param`
//! references: that `self.x` evaluates to the same value as `x`, arithmetic
//! with self-references works correctly, and constraints using self compile
//! and evaluate without violations.

use reify_test_support::{check_source, eval_source};
use reify_core::ValueCellId;
use reify_ir::{Satisfaction, Value};

// ─── step-9: self.param eval produces correct value ───

#[test]
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

    // Both should be Scalar values equal to 0.005 (5mm in SI meters)
    match (thickness_val, mirror_val) {
        (Value::Scalar { si_value: t, .. }, Value::Scalar { si_value: m, .. }) => {
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
                (t - m).abs() < 1e-9,
                "mirror should equal thickness: {} vs {}",
                t,
                m
            );
        }
        _ => panic!(
            "expected Scalar values, got thickness={:?}, mirror={:?}",
            thickness_val, mirror_val
        ),
    }
}

// ─── step-10: self in let arithmetic eval ───

#[test]
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
        Value::Scalar { si_value: v, .. } => {
            assert!(
                (v - 0.010).abs() < 1e-9,
                "sum should be 0.010 (10mm SI), got {}",
                v
            );
        }
        _ => panic!("expected Scalar value for sum, got {:?}", sum_val),
    }
}

// ─── step-12: self in constraint eval ───

#[test]
fn self_in_constraint_eval_satisfied() {
    // `constraint self.x > 2mm` with x = 5mm should be satisfied.
    let result = check_source(
        r#"structure S {
    param x : Scalar = 5mm
    constraint self.x > 2mm
}"#,
    );

    // Constraint checking should produce at least one entry
    assert!(
        !result.constraint_results.is_empty(),
        "expected at least one constraint result, got none"
    );
    // All constraints should be satisfied (5mm > 2mm)
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be satisfied (5mm > 2mm), got {:?}",
            entry.id,
            entry.satisfaction
        );
    }

    // Also verify x evaluated correctly
    let x_id = ValueCellId::new("S", "x");
    let x_val = result
        .values
        .get(&x_id)
        .expect("x should be in check result");
    match x_val {
        Value::Scalar { si_value: v, .. } => {
            assert!(
                (v - 0.005).abs() < 1e-9,
                "x should be 0.005 (5mm SI), got {}",
                v
            );
        }
        _ => panic!("expected Scalar value for x, got {:?}", x_val),
    }
}

#[test]
fn self_in_constraint_eval_violated() {
    // `constraint self.x > 2mm` with x = 1mm should be violated.
    let result = check_source(
        r#"structure S {
    param x : Scalar = 1mm
    constraint self.x > 2mm
}"#,
    );

    // Constraint checking should produce at least one entry
    assert!(
        !result.constraint_results.is_empty(),
        "expected at least one constraint result, got none"
    );
    // The constraint should be violated (1mm is NOT > 2mm)
    assert!(
        result
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "expected at least one violated constraint (1mm > 2mm is false), got: {:?}",
        result
            .constraint_results
            .iter()
            .map(|e| &e.satisfaction)
            .collect::<Vec<_>>()
    );
}
