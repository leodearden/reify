//! Point/Vector component access and arithmetic evaluation tests.

use reify_expr::{eval_expr, EvalContext};
use reify_types::{BinOp, CompiledExpr, DimensionVector, Type, UnOp, Value, ValueCellId, ValueMap};

// --- Construction ---

/// Construction of Point3<Length> as Value::Tensor with 3 length components.
#[test]
fn construct_point3_length_tensor() {
    let expr = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)])
    );
}

/// Construction of Vector3<Length> as Value::Tensor with 3 length components.
#[test]
fn construct_vector3_length_tensor() {
    let expr = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(4.0),
            Value::length(5.0),
            Value::length(6.0),
        ]),
        Type::vec3(Type::length()),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(4.0), Value::length(5.0), Value::length(6.0)])
    );
}

/// Construction of Point2<Length> as Value::Tensor with 2 length components.
#[test]
fn construct_point2_length_tensor() {
    let expr = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(7.0), Value::length(8.0)]),
        Type::point2(Type::length()),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Tensor(vec![Value::length(7.0), Value::length(8.0)]));
}

/// Construction of Vector2<Dimensionless> as Value::Tensor with 2 Real components.
#[test]
fn construct_vector2_dimensionless_tensor() {
    let expr = CompiledExpr::literal(
        Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]),
        Type::vec2(Type::dimensionless_scalar()),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]));
}

// ─── step-1: Basic .x / .y / .z on Point3<Scalar[m]> ───

/// .x on a Point3 Tensor returns component[0].
#[test]
fn eval_point3_x_returns_first_component() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::method_call(tensor, "x".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::length(1.0));
}

/// .y on a Point3 Tensor returns component[1].
#[test]
fn eval_point3_y_returns_second_component() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::method_call(tensor, "y".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::length(2.0));
}

/// .z on a Point3 Tensor returns component[2].
#[test]
fn eval_point3_z_returns_third_component() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::method_call(tensor, "z".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::length(3.0));
}

// ─── step-3: Vector component access and dimension preservation ───

/// .x on a Vector3<Scalar[m]> returns the first component.
/// Verifies that Vector and Point share the same Tensor runtime representation.
#[test]
fn eval_vector3_x_returns_first_component() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(10.0),
            Value::length(20.0),
            Value::length(30.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::method_call(tensor, "x".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::length(10.0));
}

/// .x and .y on a Vector2<Scalar[angle]> preserve the angle dimension.
#[test]
fn eval_vector2_xy_preserves_angle_dimension() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::angle(0.5),
            Value::angle(1.0),
        ]),
        Type::vec2(Type::angle()),
    );
    let expr_x = CompiledExpr::method_call(tensor.clone(), "x".to_string(), vec![], Type::angle());
    let expr_y = CompiledExpr::method_call(tensor, "y".to_string(), vec![], Type::angle());
    let values = ValueMap::new();
    assert_eq!(eval_expr(&expr_x, &EvalContext::simple(&values)), Value::angle(0.5));
    assert_eq!(eval_expr(&expr_y, &EvalContext::simple(&values)), Value::angle(1.0));
}

/// .x on a Point2<Scalar[m]> returns the first component (2D types work for valid components).
#[test]
fn eval_point2_x_returns_first_component() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(5.0),
            Value::length(6.0),
        ]),
        Type::point2(Type::length()),
    );
    let expr = CompiledExpr::method_call(tensor, "x".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::length(5.0));
}

// ─── step-4: Bounds-checking and error cases ───

/// .z on a Point2 (N=2, index 2 out of bounds) returns Undef.
#[test]
fn eval_point2_z_out_of_bounds_returns_undef() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
        ]),
        Type::point2(Type::length()),
    );
    let expr = CompiledExpr::method_call(tensor, "z".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// .y on a 1-component Tensor (N=1, index 1 out of bounds) returns Undef.
#[test]
fn eval_tensor1_y_out_of_bounds_returns_undef() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0)]),
        Type::point2(Type::length()), // type annotation irrelevant to eval
    );
    let expr = CompiledExpr::method_call(tensor, "y".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// .z on a 1-component Tensor (N=1, index 2 out of bounds) returns Undef.
#[test]
fn eval_tensor1_z_out_of_bounds_returns_undef() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0)]),
        Type::point2(Type::length()),
    );
    let expr = CompiledExpr::method_call(tensor, "z".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// .x on a non-Tensor value (Value::Int) returns Undef.
#[test]
fn eval_x_on_non_tensor_int_returns_undef() {
    let int_val = CompiledExpr::literal(Value::Int(42), Type::Int);
    let expr = CompiledExpr::method_call(int_val, "x".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// .x on a Value::List (distinct from Tensor) returns Undef.
/// Verifies we only match Value::Tensor, not Value::List.
#[test]
fn eval_x_on_list_returns_undef() {
    let list_val = CompiledExpr::literal(
        Value::List(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::List(Box::new(Type::length())),
    );
    let expr = CompiledExpr::method_call(list_val, "x".to_string(), vec![], Type::length());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// ─── step-5: Undef object propagation ───

/// .x on an Undef object (missing ValueRef) returns Undef.
/// Verifies the existing Undef short-circuit in MethodCall dispatch applies to component access.
#[test]
fn eval_x_on_undef_object_returns_undef() {
    let missing_id = ValueCellId::new("S", "missing_point");
    let obj = CompiledExpr::value_ref(missing_id, Type::point3(Type::length()));
    let expr = CompiledExpr::method_call(obj, "x".to_string(), vec![], Type::length());
    let values = ValueMap::new(); // empty — missing_point is not in the map
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// --- Tensor add/sub arithmetic ---

/// Vector3<Length> + Vector3<Length> produces component-wise sum.
#[test]
fn vector3_add_vector3_componentwise() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(4.0), Value::length(5.0), Value::length(6.0)]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(5.0), Value::length(7.0), Value::length(9.0)])
    );
}

/// Vector3<Length> - Vector3<Length> produces component-wise difference.
#[test]
fn vector3_sub_vector3_componentwise() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(10.0), Value::length(20.0), Value::length(30.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(9.0), Value::length(18.0), Value::length(27.0)])
    );
}

/// Point3<Length> + Vector3<Length> returns a Tensor (point displaced by vector).
#[test]
fn point3_add_vector3_returns_point() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(1.0), Value::length(1.0)]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(2.0), Value::length(3.0), Value::length(4.0)])
    );
}

/// Vector3<Length> + Point3<Length> returns a Tensor (commutative, point + vector).
#[test]
fn vector3_add_point3_returns_point() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(1.0), Value::length(1.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(2.0), Value::length(3.0), Value::length(4.0)])
    );
}

/// Point3<Length> - Point3<Length> returns a Tensor (displacement vector).
#[test]
fn point3_sub_point3_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(3.0), Value::length(4.0), Value::length(5.0)]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(2.0), Value::length(2.0), Value::length(2.0)])
    );
}

/// Spec 3.3.1: Point3<Length> - Point3<Length> gives Vector3<Length>.
/// The specific named case from the specification.
#[test]
fn point3_length_sub_point3_length_gives_vector3_length() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(10.0), Value::length(20.0), Value::length(30.0)]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(9.0), Value::length(18.0), Value::length(27.0)])
    );
}

/// Adding tensors of different N (3 vs 2) returns Undef.
#[test]
fn vector3_add_vector2_n_mismatch_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0)]),
        Type::vec2(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// --- Point+Point guard ---

/// Point3<Length> + Point3<Length> must return Undef — point addition is undefined.
/// After step-3 Tensor add works, so without a guard P+P returns a Tensor.
#[test]
fn point3_add_point3_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// --- Scalar-vector multiplication and division ---

/// Scalar<Length> * Vector3<Length> scales each component; dimensions combine to Area.
#[test]
fn scalar_mul_vector3_scales_components() {
    let left = CompiledExpr::literal(Value::length(2.0), Type::length());
    let right = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::Real);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let area = DimensionVector::AREA;
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Scalar { si_value: 2.0, dimension: area },
            Value::Scalar { si_value: 4.0, dimension: area },
            Value::Scalar { si_value: 6.0, dimension: area },
        ])
    );
}

/// Vector3<Length> * Scalar<Length> is commutative — same result as scalar_mul_vector3.
#[test]
fn vector3_mul_scalar_commutative() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0), Value::length(3.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::length(2.0), Type::length());
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::Real);
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let area = DimensionVector::AREA;
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Scalar { si_value: 2.0, dimension: area },
            Value::Scalar { si_value: 4.0, dimension: area },
            Value::Scalar { si_value: 6.0, dimension: area },
        ])
    );
}

/// Vector3<Length> / Scalar<dimensionless> divides each component; dimension preserved.
#[test]
fn vector3_div_scalar_divides_components() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(10.0), Value::length(20.0), Value::length(30.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Real(2.0), Type::Real);
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(5.0), Value::length(10.0), Value::length(15.0)])
    );
}
