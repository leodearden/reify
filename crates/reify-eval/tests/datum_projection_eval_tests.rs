//! Eval-side datum-projection tests (task 4382 β, step-11 RED / step-12 GREEN).
//!
//! The compiler (β/step-10) lowers a datum projection such as `a.dir` into a
//! `CompiledExprKind::MethodCall { object, method: "dir", args: [] }` node whose
//! `result_type` is the projection codomain (`Direction`, `Point3<Length>`,
//! `Plane`, …). Here we construct datum `Value`s (`Axis`/`Plane`/`Frame`)
//! directly, build that `MethodCall` node by hand, and drive it through the
//! same evaluator the engine uses — `reify_expr::eval_expr` — asserting the
//! projected `Value`.
//!
//! RED (step-11): the evaluator has no datum-projection dispatch yet, so every
//! projection currently evaluates to `Value::Undef` (the method names fall
//! through `eval_method_call`'s collection/tensor arms to the `_ => Undef`
//! catch-all), and these assertions fail. Step-12 adds the dispatch.

use reify_core::Type;
use reify_expr::{eval_expr, EvalContext};
use reify_ir::{CompiledExpr, Value, ValueMap};
use reify_test_support::{
    axis_val, frame_val, orientation_val, plane_val, point3, vec3_dimensionless,
};

const EPS: f64 = 1e-12;

/// Evaluate `receiver.<member>` as a `MethodCall` node and return the result.
///
/// `receiver_ty` is the static type of the receiver literal; `result_ty` is the
/// projection codomain the compiler would stamp on the `MethodCall`.
fn project(receiver: Value, receiver_ty: Type, member: &str, result_ty: Type) -> Value {
    let object = CompiledExpr::literal(receiver, receiver_ty);
    let call = CompiledExpr::method_call(object, member.to_string(), vec![], result_ty);
    let values = ValueMap::new();
    eval_expr(&call, &EvalContext::simple(&values))
}

/// Assert `v` is a `Value::Direction` with components close to `expected`.
fn assert_direction(v: &Value, expected: [f64; 3]) {
    match v {
        Value::Direction { x, y, z } => assert!(
            (x - expected[0]).abs() < EPS
                && (y - expected[1]).abs() < EPS
                && (z - expected[2]).abs() < EPS,
            "expected Direction {expected:?}, got Direction {{ {x}, {y}, {z} }}",
        ),
        other => panic!("expected Value::Direction, got {other:?}"),
    }
}

/// Extract the three numeric components of a 3-component `Value::Point`.
fn point_components(v: &Value) -> [f64; 3] {
    match v {
        Value::Point(comps) if comps.len() == 3 => [
            comps[0].as_f64().expect("point component 0 is numeric"),
            comps[1].as_f64().expect("point component 1 is numeric"),
            comps[2].as_f64().expect("point component 2 is numeric"),
        ],
        other => panic!("expected a 3-component Value::Point, got {other:?}"),
    }
}

fn assert_point(v: &Value, expected: [f64; 3]) {
    let comps = point_components(v);
    assert!(
        (comps[0] - expected[0]).abs() < EPS
            && (comps[1] - expected[1]).abs() < EPS
            && (comps[2] - expected[2]).abs() < EPS,
        "expected Point {expected:?}, got {comps:?}",
    );
}

// ─── Axis projections ────────────────────────────────────────────────────────

#[test]
fn axis_dir_projects_to_direction() {
    let axis = axis_val(point3(1.0, 2.0, 3.0), vec3_dimensionless(1.0, 0.0, 0.0));
    let result = project(axis, Type::Axis, "dir", Type::Direction);
    assert_direction(&result, [1.0, 0.0, 0.0]);
}

#[test]
fn axis_origin_projects_to_point() {
    let axis = axis_val(point3(1.0, 2.0, 3.0), vec3_dimensionless(1.0, 0.0, 0.0));
    let result = project(axis, Type::Axis, "origin", Type::point3(Type::length()));
    assert_point(&result, [1.0, 2.0, 3.0]);
}

// ─── Plane projections ───────────────────────────────────────────────────────

#[test]
fn plane_normal_projects_to_direction() {
    let plane = plane_val(point3(0.0, 0.0, 0.0), vec3_dimensionless(0.0, 0.0, 1.0));
    let result = project(plane, Type::Plane, "normal", Type::Direction);
    assert_direction(&result, [0.0, 0.0, 1.0]);
}

// ─── Frame projections ───────────────────────────────────────────────────────

#[test]
fn frame_z_projects_to_direction() {
    // Identity orientation → basis columns are the world axes; z = [0, 0, 1].
    let frame = frame_val(point3(4.0, 5.0, 6.0), orientation_val(1.0, 0.0, 0.0, 0.0));
    let result = project(frame, Type::Frame(3), "z", Type::Direction);
    assert_direction(&result, [0.0, 0.0, 1.0]);
}

#[test]
fn frame_origin_projects_to_point() {
    let frame = frame_val(point3(4.0, 5.0, 6.0), orientation_val(1.0, 0.0, 0.0, 0.0));
    let result = project(frame, Type::Frame(3), "origin", Type::point3(Type::length()));
    assert_point(&result, [4.0, 5.0, 6.0]);
}

#[test]
fn frame_xy_plane_projects_to_plane() {
    let frame = frame_val(point3(4.0, 5.0, 6.0), orientation_val(1.0, 0.0, 0.0, 0.0));
    let result = project(frame, Type::Frame(3), "xy_plane", Type::Plane);
    match &result {
        Value::Plane { origin, .. } => assert_point(origin, [4.0, 5.0, 6.0]),
        other => panic!("expected Value::Plane, got {other:?}"),
    }
}

// ─── Direction component projections ─────────────────────────────────────────

#[test]
fn direction_x_projects_to_real_component() {
    // A Direction's `.x/.y/.z` yield dimensionless `Real` components. This pins
    // the interaction with `eval_method_call`'s existing `"x"|"y"|"z"` arm,
    // which only handles `Value::Tensor` today and would otherwise return Undef.
    let dir = Value::Direction {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    let result = project(dir, Type::Direction, "x", Type::dimensionless_scalar());
    match result {
        Value::Real(c) => assert!((c - 1.0).abs() < EPS, "expected Real(1.0), got Real({c})"),
        other => panic!("expected Value::Real, got {other:?}"),
    }
}
