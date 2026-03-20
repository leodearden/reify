//! Lambda evaluation tests.

use reify_expr::eval_expr;
use reify_types::{BinOp, CompiledExpr, CompiledExprKind, Type, Value, ValueCellId, ValueMap};

/// step-13: Evaluate a lambda expression `|x| x * 2` — verify it produces
/// Value::Lambda with the correct params and empty captures.
#[test]
fn eval_lambda_simple_no_captures() {
    // Build: |x| x * 2
    // Lambda params: [("x", None)]
    // Body: BinOp(Mul, ValueRef($lambda.x), Literal(2))
    // Captures: []
    let x_id = ValueCellId::new("$lambda", "x");
    let body = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(x_id.clone(), Type::Real),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Real,
    );
    let lambda_expr = CompiledExpr::lambda(
        vec![("x".to_string(), None)],
        body,
        vec![], // no captures
        Type::Function {
            params: vec![Type::Real],
            return_type: Box::new(Type::Real),
        },
    );

    let values = ValueMap::new();
    let result = eval_expr(&lambda_expr, &values);

    match &result {
        Value::Lambda {
            params,
            body: _,
            captures,
        } => {
            assert_eq!(params, &["x".to_string()]);
            assert!(captures.is_empty(), "no captures expected");
        }
        other => panic!("expected Value::Lambda, got {:?}", other),
    }
}
