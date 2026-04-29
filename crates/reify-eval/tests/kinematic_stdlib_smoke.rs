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
use reify_types::{DimensionVector, Value, ValueCellId, ValueMap};

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
///   `t_composed_op`  = `t_unit_x * t_unit_x` (operator-level path) — must
///                      equal `t_composed` value-for-value.
///   `t_inv`          = `transform_inverse(t_unit_x)` → translation [-1mm, 0, 0]
///   `twist`          = `transform_log(t_unit_x)` → Map { angular=[0,0,0], linear=[1mm,0,0] }
///   `t_round`        = `transform_exp(twist)` → Transform ≈ t_unit_x
///   `prism_jac`      = `joint_jacobian(prismatic([1,0,0], 0mm..1mm))`
///                     → Map { angular=[0,0,0], linear=[1,0,0] }
///   `rev_jac`        = `joint_jacobian(revolute([0,0,1], 0rad..pi))`
///                     → Map { angular=[0,0,1], linear=[0,0,0] }
///   `fixed_joint`    = `fixed()` — 0-DOF group-only joint, Map { kind="fixed" }
///   `fixed_xform`    = `transform_at(fixed_joint, 0)` → identity Transform
///   `fixed_jac`      = `joint_jacobian(fixed_joint)` → zero-twist Map
///   `planar_joint`   = `planar(vec3(1,0,0), vec3(0,1,0), 0mm..1m, 0mm..1m, 0rad..6.283185rad)`
///                     → Map { kind="planar", axis_x, axis_y, range_x, range_y, range_theta } (6 keys)
///   `planar_xform`   = `transform_at(planar_joint, [0.5m, 0.3m, 0.5rad])`
///                     → Transform { translation=[0.5m, 0.3m, 0], rotation=quat(+Z, 0.5rad) }
///   `planar_jac`     = `joint_jacobian(planar_joint)` → zero-twist Map (FD-fallback placeholder)
const SMOKE_SOURCE: &str = r#"
structure def Kinematic {
    let r_id       = orient_identity()
    let r_z90      = orient_axis_angle(vec3(0, 0, 1), 1.5707963267948966)
    let r_composed = orient_compose(r_z90, r_z90)
    let r_inv      = orient_inverse(r_z90)

    let t_id          = transform3_identity()
    let t_unit_x      = transform3(r_id, vec3(1mm, 0mm, 0mm))
    let t_composed    = transform_compose(t_unit_x, t_unit_x)
    let t_composed_op = t_unit_x * t_unit_x
    let t_inv         = transform_inverse(t_unit_x)

    let twist      = transform_log(t_unit_x)
    let t_round    = transform_exp(twist)

    let prism      = prismatic(vec3(1, 0, 0), 0mm .. 1mm)
    let rev        = revolute(vec3(0, 0, 1), 0rad .. 3.141592653589793rad)
    let prism_jac  = joint_jacobian(prism)
    let rev_jac    = joint_jacobian(rev)

    let fixed_joint = fixed()
    let fixed_xform = transform_at(fixed_joint, 0)
    let fixed_jac   = joint_jacobian(fixed_joint)

    let planar_joint = planar(vec3(1, 0, 0), vec3(0, 1, 0), 0mm .. 1m, 0mm .. 1m, 0rad .. 6.283185rad)
    let planar_xform = transform_at(planar_joint, [0.5m, 0.3m, 0.5rad])
    let planar_jac   = joint_jacobian(planar_joint)
}
"#;

/// Resolve a binding by name from the eval result.
fn get_value<'a>(values: &'a ValueMap, name: &str) -> &'a Value {
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

/// Assert that a `Value::Vector` of three components carries the expected
/// dimension on each component. Catches regressions where the eval pipeline
/// strips dimension tags through compose / inverse / log / exp.
fn assert_vec3_dim(actual: &Value, expected: DimensionVector, label: &str) {
    let items = match actual {
        Value::Vector(v) if v.len() == 3 => v,
        other => panic!("{label}: expected Vector3, got {other:?}"),
    };
    for (i, comp) in items.iter().enumerate() {
        assert_eq!(
            comp.dimension(),
            expected,
            "{label}: component[{i}] dimension {:?}, expected {:?}",
            comp.dimension(),
            expected
        );
    }
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
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("t_id: expected Transform, got {other:?}"),
    };
    assert_orientation_close(t_id_rot, (1.0, 0.0, 0.0, 0.0), 1e-12, "t_id rotation");
    assert_vec3_close(t_id_trans, [0.0, 0.0, 0.0], 1e-12, "t_id translation");

    // t_unit_x = transform3(r_id, vec3(1mm, 0mm, 0mm))
    let t_ux = get_value(v, "t_unit_x");
    let (t_ux_rot, t_ux_trans) = match t_ux {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("t_unit_x: expected Transform, got {other:?}"),
    };
    assert_orientation_close(t_ux_rot, (1.0, 0.0, 0.0, 0.0), 1e-12, "t_unit_x rotation");
    // 1 mm = 1e-3 m
    assert_vec3_close(t_ux_trans, [1e-3, 0.0, 0.0], 1e-15, "t_unit_x translation");

    // t_composed = compose(t_unit_x, t_unit_x) → translation [2mm, 0, 0]
    let t_co = get_value(v, "t_composed");
    let (t_co_rot, t_co_trans) = match t_co {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("t_composed: expected Transform, got {other:?}"),
    };
    assert_vec3_close(
        t_co_trans,
        [2e-3, 0.0, 0.0],
        1e-15,
        "t_composed translation",
    );
    // Dimension-tag regression guard: compose must preserve LENGTH on the
    // translation. A regression that silently drops dim tags through the
    // eval pipeline would make the value-equality assert above pass while
    // emitting bare Real components.
    assert_vec3_dim(
        t_co_trans,
        DimensionVector::LENGTH,
        "t_composed translation dim",
    );

    // t_composed_op = t_unit_x * t_unit_x must agree with
    // transform_compose(t_unit_x, t_unit_x). This is the regression test
    // that the named-function path and the operator path stay in sync —
    // it lives at the eval-pipeline level because reify-expr's eval_mul
    // is private to that crate and not callable from reify-stdlib unit
    // tests. For the current source (identity rotation, [1mm,0,0]
    // translation), both code paths produce bit-identical f64s, so the
    // 1e-15 tolerance is effectively as tight as bit-exact while keeping
    // the component-wise style consistent with the rest of this test.
    // Tighten further or revert to assert_eq! if either path begins
    // producing non-identical results for these specific inputs.
    let t_co_op = get_value(v, "t_composed_op");
    let (t_co_op_rot, t_co_op_trans) = match t_co_op {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("t_composed_op: expected Transform, got {other:?}"),
    };
    // Extract expected rotation/translation from the already-verified t_co values.
    let exp_rot = match t_co_rot {
        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
        other => panic!("t_composed rotation: expected Orientation, got {other:?}"),
    };
    let exp_trans = {
        let items = match t_co_trans {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("t_composed translation: expected Vector3, got {other:?}"),
        };
        [
            items[0].as_f64().expect("t_composed trans[0]"),
            items[1].as_f64().expect("t_composed trans[1]"),
            items[2].as_f64().expect("t_composed trans[2]"),
        ]
    };
    assert_orientation_close(
        t_co_op_rot,
        exp_rot,
        1e-15,
        "t_composed_op rotation matches t_composed",
    );
    assert_vec3_close(
        t_co_op_trans,
        exp_trans,
        1e-15,
        "t_composed_op translation matches t_composed",
    );
    // Dimension-tag regression guard (operator path): restores the dim-tag
    // coverage that the previous bit-exact assert_eq!(t_co_op, t_co) provided
    // implicitly through Value equality. assert_vec3_close is blind to dim tags
    // because Value::as_f64() strips them.
    assert_vec3_dim(
        t_co_op_trans,
        DimensionVector::LENGTH,
        "t_composed_op translation dim",
    );

    // t_inv = inverse(t_unit_x) → translation [-1mm, 0, 0]
    let t_in = get_value(v, "t_inv");
    let t_in_trans = match t_in {
        Value::Transform { translation, .. } => translation.as_ref(),
        other => panic!("t_inv: expected Transform, got {other:?}"),
    };
    assert_vec3_close(t_in_trans, [-1e-3, 0.0, 0.0], 1e-15, "t_inv translation");
    assert_vec3_dim(t_in_trans, DimensionVector::LENGTH, "t_inv translation dim");

    // ── transform_log/exp round-trip on t_unit_x ──────────────────────
    // twist = log(t_unit_x) → Map { angular=[0,0,0], linear=[1mm,0,0] }
    let twist = get_value(v, "twist");
    let ang = map_vec3(twist, "angular", "twist.angular");
    let lin = map_vec3(twist, "linear", "twist.linear");
    assert_vec3_close(ang, [0.0, 0.0, 0.0], 1e-12, "twist.angular");
    assert_vec3_close(lin, [1e-3, 0.0, 0.0], 1e-15, "twist.linear");
    // Twist convention: angular=DIMENSIONLESS (axis*angle in radians, but
    // dimensionless because the angle is implicit), linear=LENGTH because
    // t_unit_x's translation was LENGTH-typed.
    assert_vec3_dim(ang, DimensionVector::DIMENSIONLESS, "twist.angular dim");
    assert_vec3_dim(lin, DimensionVector::LENGTH, "twist.linear dim");

    // t_round = exp(twist) → ≈ t_unit_x (Transform with identity rotation, [1mm,0,0] translation)
    let t_round = get_value(v, "t_round");
    let (t_round_rot, t_round_trans) = match t_round {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("t_round: expected Transform, got {other:?}"),
    };
    assert_orientation_close(t_round_rot, (1.0, 0.0, 0.0, 0.0), 1e-12, "t_round rotation");
    assert_vec3_close(
        t_round_trans,
        [1e-3, 0.0, 0.0],
        1e-15,
        "t_round translation",
    );

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

    // ── fixed joint (0-DOF sub-assembly grouping) ─────────────────────
    // fixed_joint = fixed() → Map { kind: "fixed" } (single key, no axis/range)
    let fixed_joint = get_value(v, "fixed_joint");
    let fj_map = match fixed_joint {
        Value::Map(m) => m,
        other => panic!("fixed_joint: expected Map, got {other:?}"),
    };
    assert_eq!(
        fj_map.get(&Value::String("kind".to_string())),
        Some(&Value::String("fixed".to_string())),
        "fixed_joint: kind field should be 'fixed'"
    );
    assert_eq!(
        fj_map.len(),
        1,
        "fixed_joint: Map should have exactly 1 key"
    );

    // fixed_xform = transform_at(fixed_joint, 0) → identity Transform
    let fixed_xform = get_value(v, "fixed_xform");
    let (fx_rot, fx_trans) = match fixed_xform {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("fixed_xform: expected Transform, got {other:?}"),
    };
    assert_orientation_close(fx_rot, (1.0, 0.0, 0.0, 0.0), 1e-12, "fixed_xform rotation");
    assert_vec3_close(fx_trans, [0.0, 0.0, 0.0], 1e-12, "fixed_xform translation");

    // fixed_jac = joint_jacobian(fixed_joint) → zero-twist Map
    let fixed_jac = get_value(v, "fixed_jac");
    assert_vec3_close(
        map_vec3(fixed_jac, "angular", "fixed_jac.angular"),
        [0.0, 0.0, 0.0],
        1e-12,
        "fixed_jac.angular",
    );
    assert_vec3_close(
        map_vec3(fixed_jac, "linear", "fixed_jac.linear"),
        [0.0, 0.0, 0.0],
        1e-12,
        "fixed_jac.linear",
    );

    // ── planar joint (3-DOF: two prismatic + one revolute, all in-plane) ─
    // planar_joint = planar(vec3(1,0,0), vec3(0,1,0), 0mm..1m, 0mm..1m, 0rad..6.283185rad)
    // → Map { kind="planar", axis_x, axis_y, range_x, range_y, range_theta } (6 keys)
    let planar_joint = get_value(v, "planar_joint");
    let pj_map = match planar_joint {
        Value::Map(m) => m,
        other => panic!("planar_joint: expected Map, got {other:?}"),
    };
    assert_eq!(
        pj_map.get(&Value::String("kind".to_string())),
        Some(&Value::String("planar".to_string())),
        "planar_joint: kind field should be 'planar'"
    );
    assert_eq!(
        pj_map.len(),
        6,
        "planar_joint: Map should have exactly 6 keys (kind, axis_x, axis_y, range_x, range_y, range_theta)"
    );

    // planar_xform = transform_at(planar_joint, [0.5m, 0.3m, 0.5rad])
    // → Transform { translation=[0.5, 0.3, 0] m, rotation=quat(+Z, 0.5 rad) }
    // Since axis_x=[1,0,0], axis_y=[0,1,0] → plane normal = +Z.
    // T_planar = T_x(0.5m along +X) · T_y(0.3m along +Y) · T_theta(0.5 rad about +Z)
    // Translation: T_x and T_y have identity rotation, so translations add: [0.5, 0.3, 0] m.
    // Rotation: T_theta contributes quat(+Z, 0.5 rad) = (cos(0.25), 0, 0, sin(0.25)).
    let planar_xform = get_value(v, "planar_xform");
    let (px_rot, px_trans) = match planar_xform {
        Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("planar_xform: expected Transform, got {other:?}"),
    };
    let cos_half = (0.5_f64 / 2.0).cos();
    let sin_half = (0.5_f64 / 2.0).sin();
    assert_orientation_close(px_rot, (cos_half, 0.0, 0.0, sin_half), 1e-12, "planar_xform rotation");
    assert_vec3_close(px_trans, [0.5, 0.3, 0.0], 1e-12, "planar_xform translation");
    assert_vec3_dim(px_trans, DimensionVector::LENGTH, "planar_xform translation dim");

    // planar_jac = joint_jacobian(planar_joint) → zero-twist Map (FD-fallback placeholder)
    // PRD task 2: "finite-difference fallback for spherical, cylindrical, planar until
    // analytic forms are derived." Zero column preserves uniform { angular, linear } shape.
    let planar_jac = get_value(v, "planar_jac");
    assert_vec3_close(
        map_vec3(planar_jac, "angular", "planar_jac.angular"),
        [0.0, 0.0, 0.0],
        1e-12,
        "planar_jac.angular",
    );
    assert_vec3_close(
        map_vec3(planar_jac, "linear", "planar_jac.linear"),
        [0.0, 0.0, 0.0],
        1e-12,
        "planar_jac.linear",
    );
}
