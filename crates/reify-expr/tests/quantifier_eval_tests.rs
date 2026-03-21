//! Quantifier evaluation tests.

use reify_expr::{eval_expr, EvalContext};
use reify_types::{
    BinOp, CompiledExpr, CompiledExprKind, QuantifierKind, Type, Value, ValueCellId, ValueMap,
};

/// Helper: create a quantifier CompiledExpr.
fn make_quantifier(
    kind: QuantifierKind,
    var_name: &str,
    var_id: ValueCellId,
    collection: CompiledExpr,
    predicate: CompiledExpr,
) -> CompiledExpr {
    CompiledExpr::quantifier(kind, var_name.to_string(), var_id, collection, predicate)
}

/// step-5: forall over [1,2,3] with x>0 -> true
#[test]
fn forall_all_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-5: forall over [1,-1,3] with x>0 -> false
#[test]
fn forall_has_false() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(-1), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

/// step-5: exists over [1,2,3] with x>2 -> true
#[test]
fn exists_has_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-5: exists over [1,2,3] with x>5 -> false
#[test]
fn exists_all_false() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Int(2), Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(5), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

/// step-7: forall over empty list -> true (vacuous truth)
#[test]
fn forall_empty_list_vacuous_truth() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-7: exists over empty list -> false (vacuous falsity)
#[test]
fn exists_empty_list_vacuous_falsity() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(vec![], Type::List(Box::new(Type::Int)));
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false));
}

/// step-9: forall over [1, Undef, 3] with x>0 -> Undef (no false, but undef present)
#[test]
fn forall_with_undef_no_false() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Undef, Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::ForAll, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// step-9: exists over [1, Undef, 3] with x>2 -> true (short-circuit on 3>2=true)
#[test]
fn exists_with_undef_has_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Int(1), Type::Int),
            CompiledExpr::literal(Value::Undef, Type::Int),
            CompiledExpr::literal(Value::Int(3), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true));
}

/// step-9: exists over [Undef, -1] with x>0 -> Undef (no true, undef present)
#[test]
fn exists_with_undef_no_true() {
    let x_id = ValueCellId::new("$quant0.S", "x");
    let collection = CompiledExpr::list_literal(
        vec![
            CompiledExpr::literal(Value::Undef, Type::Int),
            CompiledExpr::literal(Value::Int(-1), Type::Int),
        ],
        Type::List(Box::new(Type::Int)),
    );
    let predicate = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(x_id.clone(), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let expr = make_quantifier(QuantifierKind::Exists, "x", x_id, collection, predicate);

    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// ─── step-11: Integration test: parse + compile + eval quantifier in constraint context ───

/// step-11: End-to-end integration test — parse a structure with a list-typed
/// let binding and a `forall` constraint, compile it, then evaluate the compiled
/// constraint expression with concrete values. Verifies the full pipeline:
/// grammar -> parser -> compiler -> evaluator.
#[test]
fn integration_forall_constraint_parse_compile_eval() {
    // Parse source with a list and a forall constraint
    let source = r#"
structure S {
    let grades = [9.0, 8.8, 9.5]
    constraint forall g in grades: g >= 8.8
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("integ_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];
    assert!(!template.constraints.is_empty(), "should have at least one constraint");

    // Find the grades value cell and the constraint
    let grades_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "grades")
        .expect("should have 'grades' value cell");
    let constraint_expr = &template.constraints[0].expr;

    // Verify the constraint compiled to a Quantifier
    assert!(
        matches!(&constraint_expr.kind, CompiledExprKind::Quantifier { .. }),
        "expected Quantifier, got {:?}",
        constraint_expr.kind,
    );

    // Evaluate the grades default expression to get the list value
    let empty_values = ValueMap::new();
    let grades_value = eval_expr(
        grades_cell.default_expr.as_ref().unwrap(),
        &EvalContext::simple(&empty_values),
    );
    assert!(
        matches!(&grades_value, Value::List(_)),
        "grades should eval to a list, got {:?}",
        grades_value,
    );

    // Now evaluate the constraint with grades in scope
    let mut values = ValueMap::new();
    values.insert(grades_cell.id.clone(), grades_value);
    let result = eval_expr(constraint_expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(true), "all grades >= 8.8 should be true");
}

/// step-11: Integration test for exists — parse + compile + eval with a false result
#[test]
fn integration_exists_constraint_parse_compile_eval() {
    let source = r#"
structure S {
    let scores = [1, 2, 3, 4, 5]
    let found = exists s in scores: s > 10
}
"#;
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("integ_test2"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let template = &compiled.templates[0];
    let found_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "found")
        .expect("should have 'found' value cell");
    let found_expr = found_cell.default_expr.as_ref().unwrap();

    // Verify the expression compiled to a Quantifier
    assert!(
        matches!(&found_expr.kind, CompiledExprKind::Quantifier { .. }),
        "expected Quantifier, got {:?}",
        found_expr.kind,
    );

    // Evaluate the scores list first
    let scores_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "scores")
        .expect("should have 'scores' value cell");
    let empty_values = ValueMap::new();
    let scores_value = eval_expr(
        scores_cell.default_expr.as_ref().unwrap(),
        &EvalContext::simple(&empty_values),
    );

    // Now evaluate the exists expression with scores in scope
    let mut values = ValueMap::new();
    values.insert(scores_cell.id.clone(), scores_value);
    let result = eval_expr(found_expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Bool(false), "no score > 10, should be false");
}
