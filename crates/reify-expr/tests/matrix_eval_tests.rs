//! Matrix arithmetic evaluation tests: rank-2 detection, empty-tensor guards, jagged validation.

use reify_expr::{EvalContext, eval_expr};
use reify_core::Type;
use reify_ir::{BinOp, CompiledExpr, Value, ValueMap};

/// Helper to build a literal expression.
fn lit(v: Value, ty: Type) -> CompiledExpr {
    CompiledExpr::literal(v, ty)
}

/// Simple tensor type for test expressions.
fn tensor_ty() -> Type {
    Type::Tensor {
        rank: 1,
        n: 0,
        quantity: Box::new(Type::Int),
    }
}

/// Helper to build an add expression.
fn add(a: CompiledExpr, b: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Add, a, b, tensor_ty())
}

/// Helper to build a sub expression.
fn sub(a: CompiledExpr, b: CompiledExpr) -> CompiledExpr {
    CompiledExpr::binop(BinOp::Sub, a, b, tensor_ty())
}

/// Helper to evaluate an expression with an empty ValueMap.
fn eval(expr: &CompiledExpr) -> Value {
    let values = ValueMap::new();
    eval_expr(expr, &EvalContext::simple(&values))
}

// ── Rank-1 empty tensor baseline ────────────────────────────────────────────

/// Rank-1 empty tensors: Tensor([]) + Tensor([]) → Undef.
/// Empty components are malformed data; returning Undef makes this visible
/// rather than silently producing a zero-length composite.
#[test]
fn empty_rank1_tensor_add_returns_undef() {
    let a = lit(Value::Tensor(vec![]), tensor_ty());
    let b = lit(Value::Tensor(vec![]), tensor_ty());
    let expr = add(a, b);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── Rank-2 empty tensor guards ──────────────────────────────────────────────

/// Rank-2 mismatched: one operand is non-empty rank-2, the other is empty → Undef.
/// Tensor([Tensor([Int(1)])]) + Tensor([]) → Undef (length mismatch handled by
/// componentwise_binop, but the rank-2 guard should also catch it).
#[test]
fn empty_rank2_tensor_add_mismatched_returns_undef() {
    let a = lit(
        Value::Tensor(vec![Value::Tensor(vec![Value::Int(1)])]),
        tensor_ty(),
    );
    let b = lit(Value::Tensor(vec![]), tensor_ty());
    let expr = add(a, b);
    assert_eq!(eval(&expr), Value::Undef);
}

/// Rank-2 with empty inner rows: Tensor([Tensor([])]) + Tensor([Tensor([])])
/// should return Undef because the inner rows are empty (0-column matrix).
#[test]
fn empty_inner_rank2_tensor_add_returns_undef() {
    let a = lit(Value::Tensor(vec![Value::Tensor(vec![])]), tensor_ty());
    let b = lit(Value::Tensor(vec![Value::Tensor(vec![])]), tensor_ty());
    let expr = add(a, b);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── Heterogeneous tensor rank-2 detection ───────────────────────────────────

/// Heterogeneous tensor: first row is Tensor but second is Int.
/// Tensor([Tensor([1,2]), Int(3)]) + Tensor([Tensor([4,5]), Int(6)]) → Undef.
/// Validates that .all() check catches mixed-type tensors.
#[test]
fn heterogeneous_tensor_first_is_tensor_rest_int_add_returns_undef() {
    let a = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Int(3),
        ]),
        tensor_ty(),
    );
    let b = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(4), Value::Int(5)]),
            Value::Int(6),
        ]),
        tensor_ty(),
    );
    let expr = add(a, b);
    assert_eq!(eval(&expr), Value::Undef);
}

/// Mixed-type tensor where first is Tensor but second is not.
/// Validates that rank-2 detection uses .all() not just .first().
#[test]
fn rank2_detection_checks_all_rows() {
    let a = lit(
        Value::Tensor(vec![Value::Tensor(vec![Value::Int(1)]), Value::Int(2)]),
        tensor_ty(),
    );
    let b = lit(
        Value::Tensor(vec![Value::Tensor(vec![Value::Int(3)]), Value::Int(4)]),
        tensor_ty(),
    );
    let expr = add(a, b);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── Jagged matrix validation ────────────────────────────────────────────────

/// Jagged A: rows of different lengths in the first operand → Undef.
/// Tensor([Tensor([1,2,3]), Tensor([4,5])]) + Tensor([Tensor([6,7,8]), Tensor([9,10,11])]) → Undef.
#[test]
fn jagged_a_matrix_add_returns_undef() {
    let a = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
            Value::Tensor(vec![Value::Int(4), Value::Int(5)]),
        ]),
        tensor_ty(),
    );
    let b = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(6), Value::Int(7), Value::Int(8)]),
            Value::Tensor(vec![Value::Int(9), Value::Int(10), Value::Int(11)]),
        ]),
        tensor_ty(),
    );
    let expr = add(a, b);
    assert_eq!(eval(&expr), Value::Undef);
}

/// Jagged B: A is regular but B has rows of different lengths → Undef.
#[test]
fn jagged_b_matrix_add_returns_undef() {
    let a = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]),
        tensor_ty(),
    );
    let b = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(5), Value::Int(6)]),
            Value::Tensor(vec![Value::Int(7)]),
        ]),
        tensor_ty(),
    );
    let expr = add(a, b);
    assert_eq!(eval(&expr), Value::Undef);
}

/// Jagged B in subtraction: same behavior as addition → Undef.
#[test]
fn jagged_b_matrix_sub_returns_undef() {
    let a = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(10), Value::Int(20)]),
            Value::Tensor(vec![Value::Int(30), Value::Int(40)]),
        ]),
        tensor_ty(),
    );
    let b = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3)]),
        ]),
        tensor_ty(),
    );
    let expr = sub(a, b);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── Valid rank-2 matrix arithmetic ─────────────────────────────────────────

/// Valid 2×2 matrix addition produces correct element-wise result.
/// [[1,2],[3,4]] + [[5,6],[7,8]] → [[6,8],[10,12]]
#[test]
fn valid_rank2_matrix_add_returns_correct_result() {
    let a = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]),
        tensor_ty(),
    );
    let b = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(5), Value::Int(6)]),
            Value::Tensor(vec![Value::Int(7), Value::Int(8)]),
        ]),
        tensor_ty(),
    );
    let expr = add(a, b);
    assert_eq!(
        eval(&expr),
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(6), Value::Int(8)]),
            Value::Tensor(vec![Value::Int(10), Value::Int(12)]),
        ])
    );
}

/// Valid 2×2 matrix subtraction produces correct element-wise result.
/// [[10,20],[30,40]] - [[1,2],[3,4]] → [[9,18],[27,36]]
#[test]
fn valid_rank2_matrix_sub_returns_correct_result() {
    let a = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(10), Value::Int(20)]),
            Value::Tensor(vec![Value::Int(30), Value::Int(40)]),
        ]),
        tensor_ty(),
    );
    let b = lit(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(1), Value::Int(2)]),
            Value::Tensor(vec![Value::Int(3), Value::Int(4)]),
        ]),
        tensor_ty(),
    );
    let expr = sub(a, b);
    assert_eq!(
        eval(&expr),
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(9), Value::Int(18)]),
            Value::Tensor(vec![Value::Int(27), Value::Int(36)]),
        ])
    );
}
