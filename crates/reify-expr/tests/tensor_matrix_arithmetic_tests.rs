//! Tensor/Matrix arithmetic evaluation tests.
//!
//! Matrices are represented as rank-2 tensors: Value::Tensor(Vec<Value>) where
//! each element is itself a Value::Tensor (a row of the matrix).
//!
//! A 2×3 matrix looks like:
//!   Tensor([Tensor([a, b, c]), Tensor([d, e, f])])
//!
//! A 3-element vector is:
//!   Tensor([v0, v1, v2])

use reify_expr::{eval_expr, EvalContext};
use reify_types::{BinOp, CompiledExpr, DimensionVector, Type, UnOp, Value, ValueMap};

// ── Helpers ────────────────────────────────────────────────────────────────

/// Build a rank-2 tensor literal from a Vec<Vec<Value>> (matrix rows).
fn mat(rows: Vec<Vec<Value>>) -> CompiledExpr {
    let row_tensors: Vec<Value> = rows.into_iter().map(|r| Value::Tensor(r)).collect();
    CompiledExpr::literal(Value::Tensor(row_tensors), Type::Real)
}

/// Build a rank-1 tensor literal (vector) from a Vec<Value>.
fn vec_lit(elems: Vec<Value>) -> CompiledExpr {
    CompiledExpr::literal(Value::Tensor(elems), Type::Real)
}

/// Build a scalar literal.
fn scalar_lit(v: Value) -> CompiledExpr {
    CompiledExpr::literal(v, Type::Real)
}

/// Create a Scalar with AREA dimension (Length*Length).
fn area(si_value: f64) -> Value {
    Value::Scalar {
        si_value,
        dimension: DimensionVector::AREA,
    }
}

fn eval(expr: &CompiledExpr) -> Value {
    let values = ValueMap::new();
    eval_expr(expr, &EvalContext::simple(&values))
}

// ── step-1: Matrix+Matrix addition ────────────────────────────────────────

/// 2×3 Length matrix + 2×3 Length matrix = element-wise sums.
#[test]
fn matrix_add_2x3_length_adds_elements() {
    let lhs = mat(vec![
        vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)],
        vec![Value::length(4.0), Value::length(5.0), Value::length(6.0)],
    ]);
    let rhs = mat(vec![
        vec![Value::length(7.0), Value::length(8.0), Value::length(9.0)],
        vec![Value::length(10.0), Value::length(11.0), Value::length(12.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Add, lhs, rhs, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![
                Value::length(8.0),
                Value::length(10.0),
                Value::length(12.0),
            ]),
            Value::Tensor(vec![
                Value::length(14.0),
                Value::length(16.0),
                Value::length(18.0),
            ]),
        ])
    );
}

/// Row-count mismatch: 2×3 + 3×3 → Undef.
#[test]
fn matrix_add_row_mismatch_returns_undef() {
    let lhs = mat(vec![
        vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)],
        vec![Value::length(4.0), Value::length(5.0), Value::length(6.0)],
    ]);
    let rhs = mat(vec![
        vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)],
        vec![Value::length(4.0), Value::length(5.0), Value::length(6.0)],
        vec![Value::length(7.0), Value::length(8.0), Value::length(9.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Add, lhs, rhs, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

/// Col-count mismatch: 2×3 + 2×4 → Undef.
#[test]
fn matrix_add_col_mismatch_returns_undef() {
    let lhs = mat(vec![
        vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)],
        vec![Value::length(4.0), Value::length(5.0), Value::length(6.0)],
    ]);
    let rhs = mat(vec![
        vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0),
        ],
        vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(7.0),
            Value::length(8.0),
        ],
    ]);
    let expr = CompiledExpr::binop(BinOp::Add, lhs, rhs, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

/// Dimension mismatch: 2×2 Length + 2×2 Angle → Undef.
#[test]
fn matrix_add_dimension_mismatch_returns_undef() {
    let lhs = mat(vec![
        vec![Value::length(1.0), Value::length(2.0)],
        vec![Value::length(3.0), Value::length(4.0)],
    ]);
    let rhs = mat(vec![
        vec![Value::angle(1.0), Value::angle(2.0)],
        vec![Value::angle(3.0), Value::angle(4.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Add, lhs, rhs, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── step-3: Matrix-Matrix subtraction ─────────────────────────────────────

/// 2×3 Length matrix - 2×3 Length matrix = element-wise differences.
#[test]
fn matrix_sub_2x3_length_element_differences() {
    let lhs = mat(vec![
        vec![Value::length(10.0), Value::length(20.0), Value::length(30.0)],
        vec![Value::length(40.0), Value::length(50.0), Value::length(60.0)],
    ]);
    let rhs = mat(vec![
        vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)],
        vec![Value::length(4.0), Value::length(5.0), Value::length(6.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Sub, lhs, rhs, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![
                Value::length(9.0),
                Value::length(18.0),
                Value::length(27.0),
            ]),
            Value::Tensor(vec![
                Value::length(36.0),
                Value::length(45.0),
                Value::length(54.0),
            ]),
        ])
    );
}

/// Shape mismatch in subtraction: 2×3 - 3×3 → Undef.
#[test]
fn matrix_sub_shape_mismatch_returns_undef() {
    let lhs = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)],
    ]);
    let rhs = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)],
        vec![Value::Real(7.0), Value::Real(8.0), Value::Real(9.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Sub, lhs, rhs, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── step-5: Scalar * Matrix scaling ───────────────────────────────────────

/// Scalar(Length) * Matrix(2×2 Length) = Matrix(2×2 Area).
#[test]
fn scalar_mul_matrix_scales_elements_and_dimensions() {
    let s = scalar_lit(Value::length(2.0));
    let m = mat(vec![
        vec![Value::length(1.0), Value::length(2.0)],
        vec![Value::length(3.0), Value::length(4.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, s, m, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![area(2.0), area(4.0)]),
            Value::Tensor(vec![area(6.0), area(8.0)]),
        ])
    );
}

/// Matrix * Scalar is commutative (same result as Scalar * Matrix).
#[test]
fn matrix_mul_scalar_is_commutative() {
    let s = scalar_lit(Value::length(2.0));
    let m = mat(vec![
        vec![Value::length(1.0), Value::length(2.0)],
        vec![Value::length(3.0), Value::length(4.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, m, s, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![area(2.0), area(4.0)]),
            Value::Tensor(vec![area(6.0), area(8.0)]),
        ])
    );
}

/// Int * Matrix(2×2 Real) scales by integer.
#[test]
fn int_mul_matrix_scales_elements() {
    let s = scalar_lit(Value::Int(3));
    let m = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0)],
        vec![Value::Real(3.0), Value::Real(4.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, s, m, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(3.0), Value::Real(6.0)]),
            Value::Tensor(vec![Value::Real(9.0), Value::Real(12.0)]),
        ])
    );
}

/// Real * Matrix(2×2 Length) scales by real.
#[test]
fn real_mul_matrix_scales_elements() {
    let s = scalar_lit(Value::Real(0.5));
    let m = mat(vec![
        vec![Value::length(4.0), Value::length(8.0)],
        vec![Value::length(2.0), Value::length(6.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, s, m, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::length(2.0), Value::length(4.0)]),
            Value::Tensor(vec![Value::length(1.0), Value::length(3.0)]),
        ])
    );
}

// ── step-7: Matrix / Scalar division ──────────────────────────────────────

/// Matrix(2×2 Area) / Scalar(Length) = Matrix(2×2 Length).
#[test]
fn matrix_div_scalar_area_div_length_gives_length() {
    let m = mat(vec![
        vec![area(4.0), area(8.0)],
        vec![area(2.0), area(6.0)],
    ]);
    let s = scalar_lit(Value::length(2.0));
    let expr = CompiledExpr::binop(BinOp::Div, m, s, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::length(2.0), Value::length(4.0)]),
            Value::Tensor(vec![Value::length(1.0), Value::length(3.0)]),
        ])
    );
}

/// Matrix / Real divides each element.
#[test]
fn matrix_div_real_divides_elements() {
    let m = mat(vec![
        vec![Value::Real(6.0), Value::Real(9.0)],
        vec![Value::Real(12.0), Value::Real(3.0)],
    ]);
    let s = scalar_lit(Value::Real(3.0));
    let expr = CompiledExpr::binop(BinOp::Div, m, s, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(2.0), Value::Real(3.0)]),
            Value::Tensor(vec![Value::Real(4.0), Value::Real(1.0)]),
        ])
    );
}

/// Matrix / Int(0) → Undef (division by zero).
#[test]
fn matrix_div_zero_returns_undef() {
    let m = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0)],
        vec![Value::Real(3.0), Value::Real(4.0)],
    ]);
    let s = scalar_lit(Value::Int(0));
    let expr = CompiledExpr::binop(BinOp::Div, m, s, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── step-9: -Matrix negation ───────────────────────────────────────────────
//
// These tests will FAIL until step-10 fixes eval_unop to recurse into Tensors.

/// -Matrix(2×3 Length) negates all elements, preserves dimension.
#[test]
fn neg_matrix_length_negates_all_elements() {
    let m = mat(vec![
        vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)],
        vec![Value::length(4.0), Value::length(5.0), Value::length(6.0)],
    ]);
    let expr = CompiledExpr::unop(UnOp::Neg, m, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![
                Value::length(-1.0),
                Value::length(-2.0),
                Value::length(-3.0),
            ]),
            Value::Tensor(vec![
                Value::length(-4.0),
                Value::length(-5.0),
                Value::length(-6.0),
            ]),
        ])
    );
}

/// -Matrix with Int/Real components negates correctly.
#[test]
fn neg_matrix_int_real_negates_correctly() {
    let m = mat(vec![
        vec![Value::Int(1), Value::Real(2.5)],
        vec![Value::Int(-3), Value::Real(-4.0)],
    ]);
    let expr = CompiledExpr::unop(UnOp::Neg, m, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Int(-1), Value::Real(-2.5)]),
            Value::Tensor(vec![Value::Int(3), Value::Real(4.0)]),
        ])
    );
}

// ── step-11: Matrix * Vector multiplication ────────────────────────────────
//
// These tests will FAIL until step-12 adds the mat*vec arm to eval_mul.

/// [[1,2,3],[4,5,6]] * [1,1,1] = [6, 15].
#[test]
fn matrix_vec_mul_real_computes_dot_products() {
    let m = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)],
    ]);
    let v = vec_lit(vec![Value::Real(1.0), Value::Real(1.0), Value::Real(1.0)]);
    let expr = CompiledExpr::binop(BinOp::Mul, m, v, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![Value::Real(6.0), Value::Real(15.0)])
    );
}

/// Matrix(Length) * Vector(Length) → Vector(Area): dimension product Q1*Q2.
#[test]
fn matrix_vec_mul_length_times_length_gives_area() {
    // [[1m, 2m], [3m, 4m]] * [1m, 1m] = [3m², 7m²]
    let m = mat(vec![
        vec![Value::length(1.0), Value::length(2.0)],
        vec![Value::length(3.0), Value::length(4.0)],
    ]);
    let v = vec_lit(vec![Value::length(1.0), Value::length(1.0)]);
    let expr = CompiledExpr::binop(BinOp::Mul, m, v, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![area(3.0), area(7.0)])
    );
}

/// Inner-dimension mismatch: 2×3 * Vector2 → Undef.
#[test]
fn matrix_vec_mul_inner_dim_mismatch_returns_undef() {
    let m = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)],
    ]);
    let v = vec_lit(vec![Value::Real(1.0), Value::Real(2.0)]); // 2 elems, but matrix has 3 cols
    let expr = CompiledExpr::binop(BinOp::Mul, m, v, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── step-13: Matrix * Matrix multiplication ────────────────────────────────
//
// These tests will FAIL until step-14 adds the mat*mat arm to eval_mul.

/// 2×3 Real * 3×4 Real = 2×4 Real, verify several elements.
#[test]
fn matrix_mat_mul_2x3_times_3x4() {
    // A = [[1,2,3],[4,5,6]], B = [[1,0,0,1],[0,1,0,1],[0,0,1,1]]
    // C = A*B = [[1,2,3,6],[4,5,6,15]]
    let a = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)],
    ]);
    let b = mat(vec![
        vec![
            Value::Real(1.0),
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(1.0),
        ],
        vec![
            Value::Real(0.0),
            Value::Real(1.0),
            Value::Real(0.0),
            Value::Real(1.0),
        ],
        vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(1.0),
            Value::Real(1.0),
        ],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, a, b, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![
                Value::Real(1.0),
                Value::Real(2.0),
                Value::Real(3.0),
                Value::Real(6.0),
            ]),
            Value::Tensor(vec![
                Value::Real(4.0),
                Value::Real(5.0),
                Value::Real(6.0),
                Value::Real(15.0),
            ]),
        ])
    );
}

/// Matrix(Length) * Matrix(Length) → Matrix(Area): Q1*Q2 dimension product.
#[test]
fn matrix_mat_mul_length_times_length_gives_area() {
    // A = [[1m, 2m], [3m, 4m]], B = [[1m, 0m], [0m, 1m]]
    // C = [[1m², 2m²], [3m², 4m²]]
    let a = mat(vec![
        vec![Value::length(1.0), Value::length(2.0)],
        vec![Value::length(3.0), Value::length(4.0)],
    ]);
    let b = mat(vec![
        vec![Value::length(1.0), Value::length(0.0)],
        vec![Value::length(0.0), Value::length(1.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, a, b, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![area(1.0), area(2.0)]),
            Value::Tensor(vec![area(3.0), area(4.0)]),
        ])
    );
}

/// Inner-dimension mismatch: 2×3 * 4×2 → Undef.
#[test]
fn matrix_mat_mul_inner_dim_mismatch_returns_undef() {
    let a = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)],
    ]);
    let b = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0)],
        vec![Value::Real(3.0), Value::Real(4.0)],
        vec![Value::Real(5.0), Value::Real(6.0)],
        vec![Value::Real(7.0), Value::Real(8.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, a, b, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

/// Identity matrix: M * I₂ = M.
#[test]
fn matrix_mat_mul_identity_gives_same_matrix() {
    let m = mat(vec![
        vec![Value::Real(3.0), Value::Real(7.0)],
        vec![Value::Real(1.0), Value::Real(5.0)],
    ]);
    let identity = mat(vec![
        vec![Value::Real(1.0), Value::Real(0.0)],
        vec![Value::Real(0.0), Value::Real(1.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, m, identity, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(3.0), Value::Real(7.0)]),
            Value::Tensor(vec![Value::Real(1.0), Value::Real(5.0)]),
        ])
    );
}

// ── step-15: Edge cases ─────────────────────────────────────────────────────

/// Rank-2 Tensor addition works via the existing recursive eval_add.
#[test]
fn rank2_tensor_add_works_via_recursive_eval_add() {
    // 2×3 Real tensor add
    let a = mat(vec![
        vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)],
    ]);
    let b = mat(vec![
        vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)],
        vec![Value::Real(1.0), Value::Real(0.0), Value::Real(1.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Add, a, b, Type::Real);
    let result = eval(&expr);
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(1.0), Value::Real(3.0), Value::Real(3.0)]),
            Value::Tensor(vec![Value::Real(5.0), Value::Real(5.0), Value::Real(7.0)]),
        ])
    );
}

/// Undef element in matrix addition propagates Undef.
#[test]
fn undef_element_in_matrix_add_propagates_undef() {
    // Length + Angle = Undef element → whole result is Undef
    let a = mat(vec![
        vec![Value::length(1.0), Value::length(2.0)],
    ]);
    let b = mat(vec![
        vec![Value::angle(1.0), Value::length(2.0)], // first element is dimension mismatch
    ]);
    let expr = CompiledExpr::binop(BinOp::Add, a, b, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

/// 1×1 matrix * 1-vector = 1-vector (degenerate case).
#[test]
fn matrix_1x1_vec_mul_degenerate() {
    let m = mat(vec![vec![Value::Real(5.0)]]);
    let v = vec_lit(vec![Value::Real(3.0)]);
    let expr = CompiledExpr::binop(BinOp::Mul, m, v, Type::Real);
    let result = eval(&expr);
    assert_eq!(result, Value::Tensor(vec![Value::Real(15.0)]));
}

/// Empty matrix (0 rows) addition returns Undef.
#[test]
fn empty_matrix_add_returns_undef() {
    // Both empty → 0-row matrices are degenerate; arithmetic on them returns Undef.
    let a = mat(vec![]);
    let b = mat(vec![]);
    let expr = CompiledExpr::binop(BinOp::Add, a, b, Type::Real);
    assert_eq!(eval(&expr), Value::Undef);
}

// ── step-17: Jagged-matrix panic in Matrix*Matrix ──────────────────────────
//
// This test will FAIL (panic) until step-18 fixes the safe-indexing in eval_mul.

/// Jagged A matrix (row 0 has 3 cols, row 1 has 2 cols) * well-formed 3×2 B → Undef, not panic.
///
/// Reproduces the index-out-of-bounds panic at lib.rs ~line 925 (`a_elems[kk]`) where
/// the inner-dimension k is derived from row 0 (k=3), but row 1 only has 2 elements,
/// so kk=2 overflows `a_elems` for that row.
#[test]
fn matrix_mat_mul_jagged_a_returns_undef_not_panic() {
    // A has a jagged structure: row 0 has 3 elements, row 1 has only 2.
    let jagged_a = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]),
            Value::Tensor(vec![Value::Real(4.0), Value::Real(5.0)]), // only 2 elements (jagged)
        ]),
        Type::Real,
    );
    // Well-formed 3×2 B matrix.
    let b = mat(vec![
        vec![Value::Real(1.0), Value::Real(0.0)],
        vec![Value::Real(0.0), Value::Real(1.0)],
        vec![Value::Real(1.0), Value::Real(1.0)],
    ]);
    let expr = CompiledExpr::binop(BinOp::Mul, jagged_a, b, Type::Real);
    // Must return Undef rather than panicking with index out of bounds.
    assert_eq!(eval(&expr), Value::Undef);
}
