//! Point/Vector component access and arithmetic evaluation tests.

use reify_expr::{EvalContext, eval_expr};
use reify_core::{DimensionVector, Type, ValueCellId};
use reify_ir::{BinOp, CompiledExpr, UnOp, Value, ValueMap};

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
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0)
        ])
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
        Value::Tensor(vec![
            Value::length(4.0),
            Value::length(5.0),
            Value::length(6.0)
        ])
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
    assert_eq!(
        result,
        Value::Tensor(vec![Value::length(7.0), Value::length(8.0)])
    );
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
    assert_eq!(
        result,
        Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)])
    );
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
        Value::Tensor(vec![Value::angle(0.5), Value::angle(1.0)]),
        Type::vec2(Type::angle()),
    );
    let expr_x = CompiledExpr::method_call(tensor.clone(), "x".to_string(), vec![], Type::angle());
    let expr_y = CompiledExpr::method_call(tensor, "y".to_string(), vec![], Type::angle());
    let values = ValueMap::new();
    assert_eq!(
        eval_expr(&expr_x, &EvalContext::simple(&values)),
        Value::angle(0.5)
    );
    assert_eq!(
        eval_expr(&expr_y, &EvalContext::simple(&values)),
        Value::angle(1.0)
    );
}

/// .x on a Point2<Scalar[m]> returns the first component (2D types work for valid components).
#[test]
fn eval_point2_x_returns_first_component() {
    let tensor = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(5.0), Value::length(6.0)]),
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
        Value::Tensor(vec![Value::length(1.0), Value::length(2.0)]),
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
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(4.0),
            Value::length(5.0),
            Value::length(6.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::length(5.0),
            Value::length(7.0),
            Value::length(9.0)
        ])
    );
}

/// Vector3<Length> - Vector3<Length> produces component-wise difference.
#[test]
fn vector3_sub_vector3_componentwise() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(10.0),
            Value::length(20.0),
            Value::length(30.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::length(9.0),
            Value::length(18.0),
            Value::length(27.0)
        ])
    );
}

/// Point3<Length> + Vector3<Length> returns a Tensor (point displaced by vector).
#[test]
fn point3_add_vector3_returns_point() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(1.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0)
        ])
    );
}

/// Vector3<Length> + Point3<Length> returns a Tensor (commutative, point + vector).
#[test]
fn vector3_add_point3_returns_point() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(1.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0)
        ])
    );
}

/// Point3<Length> - Point3<Length> returns a Tensor (displacement vector).
#[test]
fn point3_sub_point3_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(3.0),
            Value::length(4.0),
            Value::length(5.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::length(2.0),
            Value::length(2.0),
            Value::length(2.0)
        ])
    );
}

/// Spec 3.3.1: Point3<Length> - Point3<Length> gives Vector3<Length>.
/// The specific named case from the specification.
#[test]
fn point3_length_sub_point3_length_gives_vector3_length() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(10.0),
            Value::length(20.0),
            Value::length(30.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::length(9.0),
            Value::length(18.0),
            Value::length(27.0)
        ])
    );
}

/// Adding tensors of different N (3 vs 2) returns Undef.
#[test]
fn vector3_add_vector2_n_mismatch_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
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
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
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
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let area = DimensionVector::AREA;
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Scalar {
                si_value: 2.0,
                dimension: area
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: area
            },
            Value::Scalar {
                si_value: 6.0,
                dimension: area
            },
        ])
    );
}

/// Vector3<Length> * Scalar<Length> is commutative — same result as scalar_mul_vector3.
#[test]
fn vector3_mul_scalar_commutative() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::length(2.0), Type::length());
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    let area = DimensionVector::AREA;
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Scalar {
                si_value: 2.0,
                dimension: area
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: area
            },
            Value::Scalar {
                si_value: 6.0,
                dimension: area
            },
        ])
    );
}

/// Vector3<Length> / Scalar<dimensionless> divides each component; dimension preserved.
#[test]
fn vector3_div_scalar_divides_components() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(10.0),
            Value::length(20.0),
            Value::length(30.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::length(5.0),
            Value::length(10.0),
            Value::length(15.0)
        ])
    );
}

// --- NaN denominator propagation (task 459) ---

/// Real(1.0) / Real(NaN) → Undef (NaN denominator must not propagate).
#[test]
fn real_div_nan_returns_undef() {
    let left = CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar());
    let right = CompiledExpr::literal(Value::Real(f64::NAN), Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Vector3<Length> / Real(NaN) → Undef (NaN must not silently infect components).
#[test]
fn vector_div_nan_real_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Real(f64::NAN), Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Point3<Length> / Real(NaN) → Undef (NaN must not silently infect components).
#[test]
fn point_div_nan_real_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Real(f64::NAN), Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// --- Dimension checking and Undef propagation ---

/// Vector3<Length> + Vector3<Angle> returns Undef — dimension mismatch per component.
#[test]
fn vector3_add_different_dimension_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::angle(0.1),
            Value::angle(0.2),
            Value::angle(0.3),
        ]),
        Type::vec3(Type::angle()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Scalar<Mass> * Vector3<Length> gives Tensor with Mass*Length dimension per component.
#[test]
fn scalar_times_vector_combines_dimensions() {
    let mass_length = DimensionVector::MASS.mul(&DimensionVector::LENGTH);
    let left = CompiledExpr::literal(
        Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::MASS,
        },
        Type::Scalar {
            dimension: DimensionVector::MASS,
        },
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Scalar {
                si_value: 3.0,
                dimension: mass_length
            },
            Value::Scalar {
                si_value: 6.0,
                dimension: mass_length
            },
            Value::Scalar {
                si_value: 9.0,
                dimension: mass_length
            },
        ])
    );
}

/// Undef + Vector3 propagates Undef (strict Undef propagation in eval_binop).
#[test]
fn undef_operand_in_tensor_binop_propagates() {
    let undef_ref = {
        let missing = ValueCellId::new("S", "missing");
        CompiledExpr::value_ref(missing, Type::vec3(Type::length()))
    };
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, undef_ref, right, Type::vec3(Type::length()));
    let values = ValueMap::new(); // missing is absent → Undef
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Tensor with a Undef component + Tensor propagates Undef through component-wise add.
#[test]
fn tensor_with_undef_component_in_add_propagates() {
    let left = CompiledExpr::literal(
        Value::Tensor(vec![Value::length(1.0), Value::Undef, Value::length(3.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(4.0),
            Value::length(5.0),
            Value::length(6.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// --- Tensor negation ---

/// Negating a rank-2 tensor (Tensor of Tensors with Real elements) returns
/// a Tensor with all elements negated recursively.
/// Currently fails: negate_components returns Undef for inner Tensor elements.
#[test]
fn negate_rank2_tensor_negates_all_inner_elements() {
    // Build a 2×2 nested Tensor: [[1.0, 2.0], [3.0, 4.0]]
    let operand = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]),
            Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0)]),
        ]),
        Type::tensor(2, 2, Type::dimensionless_scalar()),
    );
    let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::tensor(2, 2, Type::dimensionless_scalar()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(-1.0), Value::Real(-2.0)]),
            Value::Tensor(vec![Value::Real(-3.0), Value::Real(-4.0)]),
        ])
    );
}

/// Negating a Tensor containing Complex elements returns a Tensor with
/// both re and im negated in each Complex element.
/// Currently fails: negate_components returns Undef for Complex elements.
#[test]
fn negate_tensor_of_complex_negates_re_and_im() {
    let operand = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::Complex {
                re: 1.0,
                im: 2.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Value::Complex {
                re: 3.0,
                im: -4.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        ]),
        Type::tensor(1, 2, Type::complex(Type::dimensionless_scalar())),
    );
    let expr = CompiledExpr::unop(
        UnOp::Neg,
        operand,
        Type::tensor(1, 2, Type::complex(Type::dimensionless_scalar())),
    );
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Complex {
                re: -1.0,
                im: -2.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
            Value::Complex {
                re: -3.0,
                im: 4.0,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        ])
    );
}

/// Negating a Value::Matrix directly produces a rank-2 Tensor with negated elements
/// (canonicalized from Matrix to nested Tensor).
/// Currently fails: eval_unop has no Value::Matrix arm, falls to catch-all Undef.
#[test]
fn negate_matrix_returns_negated_rank2_tensor() {
    // Build a 2×2 Matrix: [[1, 2], [3, 4]]
    let operand = CompiledExpr::literal(
        Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(2.0)],
            vec![Value::Real(3.0), Value::Real(4.0)],
        ]),
        Type::matrix(2, 2, Type::dimensionless_scalar()),
    );
    let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::tensor(2, 2, Type::dimensionless_scalar()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    // Matrix canonicalizes to nested Tensor, then negation applies
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(-1.0), Value::Real(-2.0)]),
            Value::Tensor(vec![Value::Real(-3.0), Value::Real(-4.0)]),
        ])
    );
}

/// Negating a Tensor with mixed Int and Real elements returns a Tensor
/// with each element negated according to its variant.
#[test]
fn negate_tensor_mixed_int_real() {
    let operand = CompiledExpr::literal(
        Value::Tensor(vec![Value::Int(1), Value::Real(2.5), Value::Int(-3)]),
        Type::tensor(1, 3, Type::dimensionless_scalar()),
    );
    let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::tensor(1, 3, Type::dimensionless_scalar()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![Value::Int(-1), Value::Real(-2.5), Value::Int(3)])
    );
}

/// Negating a Matrix of Scalar (Length) elements canonicalizes to a rank-2
/// Tensor with negated Scalar values preserving their dimensions.
#[test]
fn negate_matrix_of_scalars_preserves_dimension() {
    let operand = CompiledExpr::literal(
        Value::Matrix(vec![
            vec![
                Value::Scalar {
                    si_value: 0.001,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.002,
                    dimension: DimensionVector::LENGTH,
                },
            ],
            vec![
                Value::Scalar {
                    si_value: 0.003,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.004,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        ]),
        Type::matrix(2, 2, Type::length()),
    );
    let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::tensor(2, 2, Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::Tensor(vec![
                Value::Scalar {
                    si_value: -0.001,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: -0.002,
                    dimension: DimensionVector::LENGTH,
                },
            ]),
            Value::Tensor(vec![
                Value::Scalar {
                    si_value: -0.003,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: -0.004,
                    dimension: DimensionVector::LENGTH,
                },
            ]),
        ])
    );
}

// ─── step-1 (task 398): Value::Point / Value::Vector addition ───

/// Value::Vector + Value::Vector → Value::Vector (component-wise).
#[test]
fn value_vector_add_vector_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(4.0),
            Value::length(5.0),
            Value::length(6.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(5.0),
            Value::length(7.0),
            Value::length(9.0)
        ])
    );
}

/// Value::Point + Value::Vector → Value::Point (point displaced by vector).
#[test]
fn value_point_add_vector_returns_point() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(1.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Point(vec![
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0)
        ])
    );
}

/// Value::Vector + Value::Point → Value::Point (commutative).
#[test]
fn value_vector_add_point_returns_point() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(1.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Point(vec![
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0)
        ])
    );
}

/// Value::Point + Value::Point → Undef (affine: point addition undefined).
#[test]
fn value_point_add_point_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// ─── step-3 (task 398): Value::Point / Value::Vector subtraction ───

/// Value::Point - Value::Point → Value::Vector (displacement).
#[test]
fn value_point_sub_point_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(3.0),
            Value::length(4.0),
            Value::length(5.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(2.0),
            Value::length(2.0),
            Value::length(2.0)
        ])
    );
}

/// Value::Point2 - Value::Point2 → Value::Vector (2D displacement).
#[test]
fn value_point2_sub_point2_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Point(vec![Value::length(5.0), Value::length(8.0)]),
        Type::point2(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Point(vec![Value::length(2.0), Value::length(3.0)]),
        Type::point2(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec2(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![Value::length(3.0), Value::length(5.0)])
    );
}

/// Value::Point - Value::Vector → Value::Point (point displaced backwards).
#[test]
fn value_point_sub_vector_returns_point() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(3.0),
            Value::length(4.0),
            Value::length(5.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(1.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Point(vec![
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0)
        ])
    );
}

/// Value::Vector - Value::Vector → Value::Vector (component-wise).
#[test]
fn value_vector_sub_vector_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(10.0),
            Value::length(20.0),
            Value::length(30.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(9.0),
            Value::length(18.0),
            Value::length(27.0)
        ])
    );
}

/// Value::Vector - Value::Point → Undef (no geometric meaning).
#[test]
fn value_vector_sub_point_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// ─── step-9 (task 398): Value::Point / Value::Vector division ───

/// Value::Vector / Scalar(Real) → Value::Vector.
#[test]
fn value_vector_div_scalar_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(10.0),
            Value::length(20.0),
            Value::length(30.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(5.0),
            Value::length(10.0),
            Value::length(15.0)
        ])
    );
}

/// Value::Vector / Int → Value::Vector.
#[test]
fn value_vector_div_int_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(9.0),
            Value::length(12.0),
            Value::length(15.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Int(3), Type::Int);
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(3.0),
            Value::length(4.0),
            Value::length(5.0)
        ])
    );
}

/// Value::Point / Scalar(Real) → Value::Point (pragmatic deviation).
#[test]
fn value_point_div_scalar_returns_point() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(10.0),
            Value::length(20.0),
            Value::length(30.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Point(vec![
            Value::length(5.0),
            Value::length(10.0),
            Value::length(15.0)
        ])
    );
}

/// Value::Point / Value::Point → Undef (no geometric meaning).
#[test]
fn value_point_div_point_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Point(vec![Value::length(10.0), Value::length(20.0)]),
        Type::point2(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Point(vec![Value::length(2.0), Value::length(4.0)]),
        Type::point2(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::point2(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Point / Value::Vector → Undef (no geometric meaning).
#[test]
fn value_point_div_vector_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Point(vec![Value::length(10.0), Value::length(20.0)]),
        Type::point2(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![Value::length(2.0), Value::length(4.0)]),
        Type::vec2(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::point2(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// ─── step-5 (task 398): Value::Point / Value::Vector negation ───

/// -Value::Vector → Value::Vector with negated components.
#[test]
fn value_negate_vector3_returns_negated_vector() {
    let operand = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(-1.0),
            Value::length(-2.0),
            Value::length(-3.0)
        ])
    );
}

/// -Value::Point → Undef (affine: point negation is undefined per spec 3.3.1).
#[test]
fn value_negate_point3_returns_undef() {
    let operand = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

// ─── step-7 (task 398): Length * Value::Point / Value::Vector multiplication ───

/// Scalar(Real) * Value::Vector → Value::Vector (scaled components).
#[test]
fn value_scalar_mul_vector_returns_vector() {
    let left = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(2.0),
            Value::length(4.0),
            Value::length(6.0)
        ])
    );
}

/// Value::Vector * Scalar(Real) → Value::Vector (commutative).
#[test]
fn value_vector_mul_scalar_returns_vector() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(2.0),
            Value::length(4.0),
            Value::length(6.0)
        ])
    );
}

/// Scalar(Real) * Value::Point → Value::Point (pragmatic deviation for interpolation).
#[test]
fn value_scalar_mul_point_returns_point() {
    let left = CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let right = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Point(vec![
            Value::length(2.0),
            Value::length(4.0),
            Value::length(6.0)
        ])
    );
}

/// Int * Value::Vector → Value::Vector.
#[test]
fn value_int_mul_vector_returns_vector() {
    let left = CompiledExpr::literal(Value::Int(3), Type::Int);
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Vector(vec![
            Value::length(3.0),
            Value::length(6.0),
            Value::length(9.0)
        ])
    );
}

/// Real * Value::Point → Value::Point (pragmatic deviation for interpolation).
#[test]
fn value_real_mul_point_returns_point() {
    let left = CompiledExpr::literal(Value::Real(0.5), Type::dimensionless_scalar());
    let right = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(8.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Point(vec![
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0)
        ])
    );
}

/// Negating Vector3<Length> negates all components.
#[test]
fn negate_vector3_negates_all_components() {
    let operand = CompiledExpr::literal(
        Value::Tensor(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::unop(UnOp::Neg, operand, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Tensor(vec![
            Value::length(-1.0),
            Value::length(-2.0),
            Value::length(-3.0)
        ])
    );
}

// ─── task 446: edge-case coverage ───

/// Value::Point(3 components) + Value::Vector(2 components) → Undef due to length mismatch.
/// Exercises componentwise_binop length guard on the Point+Vector arm of eval_add.
#[test]
fn value_point3_add_vector2_mismatched_length_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![Value::length(1.0), Value::length(2.0)]),
        Type::vec2(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Point(3 components) - Value::Vector(2 components) → Undef due to length mismatch.
/// Mirror of add test, confirming componentwise_binop length guard applies to eval_sub's Point-Vector arm.
#[test]
fn value_point3_sub_vector2_mismatched_length_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![Value::length(1.0), Value::length(2.0)]),
        Type::vec2(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Vector with an Undef component + Value::Vector → Undef.
/// Exercises componentwise_binop's `results.iter().any(|v| v.is_undef())` check on Vector+Vector arm.
#[test]
fn value_vector_with_undef_component_add_propagates() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![Value::length(1.0), Value::Undef, Value::length(3.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(1.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Point with an Undef component + Value::Vector → Undef.
/// Same Undef component propagation for the Point+Vector arm of eval_add.
#[test]
fn value_point_with_undef_component_add_vector_propagates() {
    let left = CompiledExpr::literal(
        Value::Point(vec![Value::length(1.0), Value::Undef, Value::length(3.0)]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(1.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Vector * Value::Point → Undef.
/// eval_mul's Vector arm guard rejects Point as scalar, falling through to _ → Undef.
#[test]
fn value_vector_mul_point_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Point * Value::Vector → Undef.
/// eval_mul's Point arm guard rejects Vector as scalar. Tests the symmetric rejection case.
#[test]
fn value_point_mul_vector_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Mul, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Point / Int → Value::Point (exercises scale_components with Int divisor).
#[test]
fn value_point_div_int_returns_point() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(6.0),
            Value::length(9.0),
            Value::length(12.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(Value::Int(3), Type::Int);
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Point(vec![
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0)
        ])
    );
}

// ─── div-zero helpers (task 993 cleanup) ───

/// Helper: asserts that Value::Vector / zero → Undef.
fn assert_vector_div_zero_returns_undef(zero: Value, zero_ty: Type) {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(zero, zero_ty);
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Helper: asserts that Value::Point / zero → Undef.
fn assert_point_div_zero_returns_undef(zero: Value, zero_ty: Type) {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(zero, zero_ty);
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::point3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Vector / Real(0.0) → Undef (division-by-zero early check).
#[test]
fn value_vector_div_real_zero_returns_undef() {
    assert_vector_div_zero_returns_undef(Value::Real(0.0), Type::dimensionless_scalar());
}

/// Value::Point / Real(0.0) → Undef (division-by-zero early check for Point).
#[test]
fn value_point_div_real_zero_returns_undef() {
    assert_point_div_zero_returns_undef(Value::Real(0.0), Type::dimensionless_scalar());
}

/// Scalar/Scalar division producing a dimensionless result must return
/// Value::Scalar { dimension: DIMENSIONLESS }, not Value::Real.
/// This ensures consistency with eval_mul which always returns Scalar.
#[test]
fn scalar_div_scalar_dimensionless_returns_scalar() {
    // 4m / 2m = 2 (dimensionless), should be Scalar not Real
    let left = CompiledExpr::literal(Value::length(4.0), Type::length());
    let right = CompiledExpr::literal(Value::length(2.0), Type::length());
    let expr = CompiledExpr::binop(BinOp::Div, left, right, Type::dimensionless_scalar());
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(
        result,
        Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
        "Scalar/Scalar with same dimension must produce Scalar{{dimensionless}}, not Real"
    );
}

// --- Task 458: additional edge-case tests ---

/// Value::Vector(3 components) + Value::Vector(2 components) → Undef (length mismatch).
/// Mirrors vector3_add_vector2_n_mismatch_returns_undef which uses Value::Tensor.
#[test]
fn value_vector3_add_vector2_mismatched_length_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![Value::length(1.0), Value::length(2.0)]),
        Type::vec2(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Vector with Undef component - Value::Vector → Undef.
/// Mirrors value_vector_with_undef_component_add_propagates but uses BinOp::Sub
/// to confirm componentwise_binop Undef propagation in the Sub direction.
#[test]
fn value_vector_with_undef_component_sub_propagates() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![Value::length(1.0), Value::Undef, Value::length(3.0)]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(1.0),
            Value::length(1.0),
        ]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Vector / Int(0) → Undef (integer zero caught by as_f64() == 0.0 guard).
#[test]
fn value_vector_div_int_zero_returns_undef() {
    assert_vector_div_zero_returns_undef(Value::Int(0), Type::Int);
}

/// Value::Point / Int(0) → Undef (integer zero caught by as_f64() == 0.0 guard).
#[test]
fn value_point_div_int_zero_returns_undef() {
    assert_point_div_zero_returns_undef(Value::Int(0), Type::Int);
}

// ─── task 993: missing guard coverage ───

/// Empty Value::Vector + Value::Vector → Undef.
/// Exercises the `if a.is_empty() { return Value::Undef }` guard in componentwise_binop.
#[test]
fn value_empty_vector_add_returns_undef() {
    let left = CompiledExpr::literal(Value::Vector(vec![]), Type::vec2(Type::length()));
    let right = CompiledExpr::literal(Value::Vector(vec![]), Type::vec2(Type::length()));
    let expr = CompiledExpr::binop(BinOp::Add, left, right, Type::vec2(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Vector - Value::Vector(with Undef component on RIGHT) → Undef.
/// Existing test (value_vector_with_undef_component_sub_propagates) only covers Undef
/// on the LEFT operand of Sub; this confirms the RIGHT operand path.
#[test]
fn value_vector_with_undef_component_sub_right_propagates() {
    let left = CompiledExpr::literal(
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::vec3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Vector(vec![Value::length(1.0), Value::Undef, Value::length(1.0)]),
        Type::vec3(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Vector / Scalar{ si_value: 0.0 } → Undef.
/// Confirms the as_f64() == 0.0 guard in eval_div catches Scalar zero
/// (existing tests only cover Int(0) and Real(0.0) zero variants).
#[test]
fn value_vector_div_scalar_zero_returns_undef() {
    assert_vector_div_zero_returns_undef(
        Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        },
        Type::dimensionless_scalar(),
    );
}

/// Value::Point3 - Value::Point2 → Undef (length mismatch in Point-Point subtraction).
/// Exercises componentwise_binop length guard in the Point-Point arm of eval_sub.
/// Existing mismatch tests only cover Vector-Vector and Point-Vector arms.
#[test]
fn value_point3_sub_point2_mismatched_length_returns_undef() {
    let left = CompiledExpr::literal(
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]),
        Type::point3(Type::length()),
    );
    let right = CompiledExpr::literal(
        Value::Point(vec![Value::length(1.0), Value::length(2.0)]),
        Type::point2(Type::length()),
    );
    let expr = CompiledExpr::binop(BinOp::Sub, left, right, Type::vec3(Type::length()));
    let values = ValueMap::new();
    let result = eval_expr(&expr, &EvalContext::simple(&values));
    assert_eq!(result, Value::Undef);
}

/// Value::Vector / Real(-0.0) → Undef (negative zero is still zero).
/// Pins IEEE 754 -0.0 divisor behaviour: -0.0 == 0.0, so the guard must catch it.
#[test]
fn value_vector_div_negative_zero_returns_undef() {
    assert_vector_div_zero_returns_undef(Value::Real(-0.0), Type::dimensionless_scalar());
}
