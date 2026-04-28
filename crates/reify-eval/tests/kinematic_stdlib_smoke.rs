//! End-to-end smoke test for the v0.2 kinematic stdlib expansion (task 2583).
//!
//! Drives the new SO(3)/SE(3) builtins added in this task — `orient_compose`,
//! `orient_inverse`, `transform_compose`, `transform_inverse`, `transform_log`,
//! `transform_exp`, plus `joint_jacobian` on prismatic and revolute joints —
//! through the full `parse → compile_with_stdlib → eval` pipeline. Each new
//! binding is checked against its expected `Value` variant via `ValueCellId`
//! lookup, and rotation/translation components are checked against analytic
//! answers. Mirrors the binding-level eval pattern in
//! `m10_combined.rs::frame_transform_lets_and_port_frames_present`.
//!
//! See docs/prds/v0_2/kinematic-constraints.md, §"Decomposition plan", task 1.
//!
//! Ignored compiler warnings: zero-arg `orient_identity()` triggers the
//! "cannot infer return type" warning under the same conditions documented in
//! `m10_combined.rs`. Warnings are non-fatal; only Error-severity diagnostics
//! would fail this test.

use reify_test_support::{collect_errors, make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{Value, ValueCellId};

/// Source: a `Kinematic` structure that exercises every new builtin.
///
/// Bindings:
///   `r_id`           = `orient_identity()` (Orientation, w=1)
///   `r_z90`          = `orient_axis_angle([0,0,1], pi/2)` (90° about +Z)
///   `r_composed`     = `orient_compose(r_z90, r_z90)` → 180° about +Z (q ≈ (0,0,0,1))
///   `r_inv`          = `orient_inverse(r_z90)` → −90° about +Z
///   `t_id`           = `transform3_identity()`
///   `t_unit_x`       = `transform3(r_id, vec3(1mm, 0mm, 0mm))`
///   `t_composed`     = `transform_compose(t_unit_x, t_unit_x)` → translation [2mm, 0, 0]
///   `t_inv`          = `transform_inverse(t_unit_x)` → translation [-1mm, 0, 0]
///   `twist`          = `transform_log(t_unit_x)` → Map { angular=[0,0,0], linear=[1mm,0,0] }
///   `t_round`        = `transform_exp(twist)` → Transform ≈ t_unit_x
///   `prism_jac`      = `joint_jacobian(prismatic([1,0,0], 0mm..1mm))`
///                     → Map { angular=[0,0,0], linear=[1,0,0] }
///   `rev_jac`        = `joint_jacobian(revolute([0,0,1], 0rad..pi))`
///                     → Map { angular=[0,0,1], linear=[0,0,0] }
const SMOKE_SOURCE: &str = r#"
structure def Kinematic {
    let r_id       = orient_identity()
    let r_z90      = orient_axis_angle(vec3(0, 0, 1), 1.5707963267948966)
    let r_composed = orient_compose(r_z90, r_z90)
    let r_inv      = orient_inverse(r_z90)

    let t_id       = transform3_identity()
    let t_unit_x   = transform3(r_id, vec3(1mm, 0mm, 0mm))
    let t_composed = transform_compose(t_unit_x, t_unit_x)
    let t_inv      = transform_inverse(t_unit_x)

    let twist      = transform_log(t_unit_x)
    let t_round    = transform_exp(twist)

    let prism      = prismatic(vec3(1, 0, 0), 0mm .. 1mm)
    let rev        = revolute(vec3(0, 0, 1), 0rad .. 3.141592653589793rad)
    let prism_jac  = joint_jacobian(prism)
    let rev_jac    = joint_jacobian(rev)
}
"#;

/// Resolve a binding by name from the eval result.
fn get_value<'a>(values: &'a std::collections::BTreeMap<ValueCellId, Value>, name: &str) -> &'a Value {
    let id = ValueCellId::new("Kinematic", name);
    values
        .get(&id)
        .unwrap_or_else(|| panic!("Kinematic.{name} not found in eval result"))
}

/// Assert a `Value::Orientation` is approximately `(w, x, y, z)` (sign-insensitive).
fn assert_orientation_close(actual: &Value, exp: (f64, f64, f64, f64), tol: f64, label: &str) {
    let (aw, ax, ay, az) = match actual {
        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
        other => panic!("{label}: expected Orientation, got {other:?}"),
    };
    let (ew, ex, ey, ez) = exp;
    let pos_diff = (aw - ew).abs() + (ax - ex).abs() + (ay - ey).abs() + (az - ez).abs();
    let neg_diff = (aw + ew).abs() + (ax + ex).abs() + (ay + ey).abs() + (az + ez).abs();
    assert!(
        pos_diff < tol || neg_diff < tol,
        "{label}: orientation expected ±({ew}, {ex}, {ey}, {ez}), got ({aw}, {ax}, {ay}, {az})"
    );
}

/// Assert a `Value::Vector` of three numeric components is approximately `expected`.
fn assert_vec3_close(actual: &Value, expected: [f64; 3], tol: f64, label: &str) {
    let items = match actual {
        Value::Vector(v) if v.len() == 3 => v,
        other => panic!("{label}: expected Vector3, got {other:?}"),
    };
    for (i, comp) in items.iter().enumerate() {
        let v = comp
            .as_f64()
            .unwrap_or_else(|| panic!("{label}: component[{i}] not numeric: {comp:?}"));
        assert!(
            (v - expected[i]).abs() < tol,
            "{label}: component[{i}] expected {}, got {}",
            expected[i],
            v
        );
    }
}

/// Read a Vector3 component vector at `key` from a `Value::Map`.
fn map_vec3<'a>(actual: &'a Value, key: &str, label: &str) -> &'a Value {
    let map = match actual {
        Value::Map(m) => m,
        other => panic!("{label}: expected Map, got {other:?}"),
    };
    map.get(&Value::String(key.to_string()))
        .unwrap_or_else(|| panic!("{label}: missing key {key:?} in Map"))
}

/// Smoke test: drive every new builtin through compile + eval, assert bindings
/// have their expected Value variant and analytic component values.
#[test]
fn kinematic_stdlib_smoke_e2e() {
    // Compile (errors panic via parse_and_compile_with_stdlib's internal assert).
    let compiled = parse_and_compile_with_stdlib(SMOKE_SOURCE);

    // Eval and capture results.
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // No Error-severity diagnostics from eval.
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no Error-severity diagnostics, got: {eval_errors:?}"
    );

    let v = &result.values;

    // ── Orientation bindings ──────────────────────────────────────────
    // r_id = orient_identity() → (1, 0, 0, 0)
    assert_orientation_close(get_value(v, "r_id"), (1.0, 0.0, 0.0, 0.0), 1e-12, "r_id");
    // r_z90 = orient_axis_angle([0,0,1], π/2) → (cos(π/4), 0, 0, sin(π/4))
    let cos_q = (std::f64::consts::FRAC_PI_4).cos();
    let sin_q = (std::f64::consts::FRAC_PI_4).sin();
    assert_orientation_close(
        get_value(v, "r_z90"),
        (cos_q, 0.0, 0.0, sin_q),
        1e-12,
        "r_z90",
    );
    // r_composed = compose(r_z90, r_z90) → 180° about +Z = (0, 0, 0, 1) up to sign
    assert_orientation_close(
        get_value(v, "r_composed"),
        (0.0, 0.0, 0.0, 1.0),
        1e-12,
        "r_composed",
    );
    // r_inv = inverse(r_z90) → (cos, 0, 0, -sin)
    assert_orientation_close(
        get_value(v, "r_inv"),
        (cos_q, 0.0, 0.0, -sin_q),
        1e-12,
        "r_inv",
    );

    // ── Transform bindings ────────────────────────────────────────────
    // t_id = transform3_identity()
    let t_id = get_value(v, "t_id");
    let (t_id_rot, t_id_trans) = match t_id {
        Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("t_id: expected Transform, got {other:?}"),
    };
    assert_orientation_close(t_id_rot, (1.0, 0.0, 0.0, 0.0), 1e-12, "t_id rotation");
    assert_vec3_close(t_id_trans, [0.0, 0.0, 0.0], 1e-12, "t_id translation");

    // t_unit_x = transform3(r_id, vec3(1mm, 0mm, 0mm))
    let t_ux = get_value(v, "t_unit_x");
    let (t_ux_rot, t_ux_trans) = match t_ux {
        Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("t_unit_x: expected Transform, got {other:?}"),
    };
    assert_orientation_close(t_ux_rot, (1.0, 0.0, 0.0, 0.0), 1e-12, "t_unit_x rotation");
    // 1 mm = 1e-3 m
    assert_vec3_close(t_ux_trans, [1e-3, 0.0, 0.0], 1e-15, "t_unit_x translation");

    // t_composed = compose(t_unit_x, t_unit_x) → translation [2mm, 0, 0]
    let t_co = get_value(v, "t_composed");
    let t_co_trans = match t_co {
        Value::Transform { translation, .. } => translation.as_ref(),
        other => panic!("t_composed: expected Transform, got {other:?}"),
    };
    assert_vec3_close(t_co_trans, [2e-3, 0.0, 0.0], 1e-15, "t_composed translation");

    // t_inv = inverse(t_unit_x) → translation [-1mm, 0, 0]
    let t_in = get_value(v, "t_inv");
    let t_in_trans = match t_in {
        Value::Transform { translation, .. } => translation.as_ref(),
        other => panic!("t_inv: expected Transform, got {other:?}"),
    };
    assert_vec3_close(t_in_trans, [-1e-3, 0.0, 0.0], 1e-15, "t_inv translation");

    // ── transform_log/exp round-trip on t_unit_x ──────────────────────
    // twist = log(t_unit_x) → Map { angular=[0,0,0], linear=[1mm,0,0] }
    let twist = get_value(v, "twist");
    let ang = map_vec3(twist, "angular", "twist.angular");
    let lin = map_vec3(twist, "linear", "twist.linear");
    assert_vec3_close(ang, [0.0, 0.0, 0.0], 1e-12, "twist.angular");
    assert_vec3_close(lin, [1e-3, 0.0, 0.0], 1e-15, "twist.linear");

    // t_round = exp(twist) → ≈ t_unit_x (Transform with identity rotation, [1mm,0,0] translation)
    let t_round = get_value(v, "t_round");
    let (t_round_rot, t_round_trans) = match t_round {
        Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("t_round: expected Transform, got {other:?}"),
    };
    assert_orientation_close(t_round_rot, (1.0, 0.0, 0.0, 0.0), 1e-12, "t_round rotation");
    assert_vec3_close(t_round_trans, [1e-3, 0.0, 0.0], 1e-15, "t_round translation");

    // ── joint_jacobian on prismatic and revolute ──────────────────────
    let prism_jac = get_value(v, "prism_jac");
    assert_vec3_close(
        map_vec3(prism_jac, "angular", "prism_jac.angular"),
        [0.0, 0.0, 0.0],
        1e-12,
        "prism_jac.angular",
    );
    assert_vec3_close(
        map_vec3(prism_jac, "linear", "prism_jac.linear"),
        [1.0, 0.0, 0.0],
        1e-12,
        "prism_jac.linear",
    );

    let rev_jac = get_value(v, "rev_jac");
    assert_vec3_close(
        map_vec3(rev_jac, "angular", "rev_jac.angular"),
        [0.0, 0.0, 1.0],
        1e-12,
        "rev_jac.angular",
    );
    assert_vec3_close(
        map_vec3(rev_jac, "linear", "rev_jac.linear"),
        [0.0, 0.0, 0.0],
        1e-12,
        "rev_jac.linear",
    );
}
