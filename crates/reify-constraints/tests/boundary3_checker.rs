//! Boundary 3 (eval → constraints) — Checker tests.
//!
//! These tests verify the SimpleConstraintChecker against the ConstraintChecker trait.

use std::borrow::Cow;

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::*;
use reify_core::Type;
use reify_ir::{BinOp, CompiledExpr, ConstraintChecker, ConstraintInput, Satisfaction, Value, ValueMap};

fn thickness_gt_2mm() -> CompiledExpr {
    let thickness = CompiledExpr::value_ref(vcid("Bracket", "thickness"), Type::length());
    let two_mm = CompiledExpr::literal(mm(2.0), Type::length());
    CompiledExpr::binop(BinOp::Gt, thickness, two_mm, Type::Bool)
}

#[test]
fn satisfied() {
    let checker = SimpleConstraintChecker;
    let expr = thickness_gt_2mm();
    let mut values = ValueMap::new();
    values.insert(vcid("Bracket", "thickness"), mm(5.0));

    let input = ConstraintInput {
        constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
        values: &values,
        functions: &[],
        determinacy: None,
    };

    let results = checker.check(&input);
    assert_eq!(results[0].satisfaction, Satisfaction::Satisfied);
}

#[test]
fn violated() {
    let checker = SimpleConstraintChecker;
    let expr = thickness_gt_2mm();
    let mut values = ValueMap::new();
    values.insert(vcid("Bracket", "thickness"), mm(1.0));

    let input = ConstraintInput {
        constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
        values: &values,
        functions: &[],
        determinacy: None,
    };

    let results = checker.check(&input);
    assert_eq!(results[0].satisfaction, Satisfaction::Violated);
}

#[test]
fn indeterminate_undef() {
    let checker = SimpleConstraintChecker;
    let expr = thickness_gt_2mm();
    let values = ValueMap::new(); // thickness is Undef

    let input = ConstraintInput {
        constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
        values: &values,
        functions: &[],
        determinacy: None,
    };

    let results = checker.check(&input);
    assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
}

#[test]
fn compound_constraint_satisfied() {
    let checker = SimpleConstraintChecker;
    // thickness < width / 4: 5mm < 80mm / 4 = 20mm → satisfied
    let thickness = CompiledExpr::value_ref(vcid("Bracket", "thickness"), Type::length());
    let width = CompiledExpr::value_ref(vcid("Bracket", "width"), Type::length());
    let four = CompiledExpr::literal(Value::Int(4), Type::Int);
    let width_div_4 = CompiledExpr::binop(BinOp::Div, width, four, Type::length());
    let expr = CompiledExpr::binop(BinOp::Lt, thickness, width_div_4, Type::Bool);

    let mut values = ValueMap::new();
    values.insert(vcid("Bracket", "thickness"), mm(5.0));
    values.insert(vcid("Bracket", "width"), mm(80.0));

    let input = ConstraintInput {
        constraints: Cow::Owned(vec![(cnid("Bracket", 0), &expr)]),
        values: &values,
        functions: &[],
        determinacy: None,
    };

    let results = checker.check(&input);
    assert_eq!(results[0].satisfaction, Satisfaction::Satisfied);
}

#[test]
fn batch_independent_results() {
    let checker = SimpleConstraintChecker;

    let expr1 = thickness_gt_2mm(); // 5mm > 2mm → satisfied

    // width > 100mm: 80mm > 100mm → violated
    let width = CompiledExpr::value_ref(vcid("Bracket", "width"), Type::length());
    let hundred_mm = CompiledExpr::literal(mm(100.0), Type::length());
    let expr2 = CompiledExpr::binop(BinOp::Gt, width, hundred_mm, Type::Bool);

    let mut values = ValueMap::new();
    values.insert(vcid("Bracket", "thickness"), mm(5.0));
    values.insert(vcid("Bracket", "width"), mm(80.0));

    let input = ConstraintInput {
        constraints: Cow::Owned(vec![
            (cnid("Bracket", 0), &expr1),
            (cnid("Bracket", 1), &expr2),
        ]),
        values: &values,
        functions: &[],
        determinacy: None,
    };

    let results = checker.check(&input);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].satisfaction, Satisfaction::Satisfied);
    assert_eq!(results[1].satisfaction, Satisfaction::Violated);
}

#[test]
fn division_by_zero_does_not_panic() {
    let checker = SimpleConstraintChecker;

    // x > (y / 0)
    let x = CompiledExpr::value_ref(vcid("B", "x"), Type::length());
    let y = CompiledExpr::value_ref(vcid("B", "y"), Type::length());
    let zero = CompiledExpr::literal(Value::Int(0), Type::Int);
    let div = CompiledExpr::binop(BinOp::Div, y, zero, Type::length());
    let expr = CompiledExpr::binop(BinOp::Gt, x, div, Type::Bool);

    let mut values = ValueMap::new();
    values.insert(vcid("B", "x"), mm(5.0));
    values.insert(vcid("B", "y"), mm(10.0));

    let input = ConstraintInput {
        constraints: Cow::Owned(vec![(cnid("B", 0), &expr)]),
        values: &values,
        functions: &[],
        determinacy: None,
    };

    let results = checker.check(&input);
    assert_eq!(results.len(), 1);
    // Div by zero → Undef → Indeterminate
    assert_eq!(results[0].satisfaction, Satisfaction::Indeterminate);
}

/// Full bracket: all 3 constraints satisfied with default values.
#[test]
fn bracket_all_constraints_satisfied() {
    let checker = SimpleConstraintChecker;
    let module = bracket_compiled_module();
    let template = &module.templates[0];

    // Set up values matching defaults
    let mut values = ValueMap::new();
    values.insert(vcid("Bracket", "width"), mm(80.0));
    values.insert(vcid("Bracket", "height"), mm(100.0));
    values.insert(vcid("Bracket", "thickness"), mm(5.0));
    values.insert(vcid("Bracket", "fillet_radius"), mm(3.0));
    values.insert(vcid("Bracket", "hole_diameter"), mm(6.0));

    let constraint_exprs: Vec<_> = template
        .constraints
        .iter()
        .map(|c| (c.id.clone(), &c.expr))
        .collect();

    let input = ConstraintInput {
        constraints: Cow::Owned(constraint_exprs),
        values: &values,
        functions: &[],
        determinacy: None,
    };

    let results = checker.check(&input);
    assert_eq!(results.len(), 3);
    for result in &results {
        assert_eq!(
            result.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            result.id
        );
    }
}
