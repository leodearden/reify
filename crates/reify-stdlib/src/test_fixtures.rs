use crate::eval_builtin;
use reify_ir::Value;

pub(crate) fn axis_x_unit() -> Value {
    Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
}

pub(crate) fn axis_y_unit() -> Value {
    Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
}

pub(crate) fn axis_z_unit() -> Value {
    Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
}

pub(crate) fn length_range_0_to_1m() -> Value {
    Value::Range {
        lower: Some(Box::new(Value::length(0.0))),
        upper: Some(Box::new(Value::length(1.0))),
        lower_inclusive: true,
        upper_inclusive: true,
    }
}

pub(crate) fn angle_range_0_to_pi() -> Value {
    Value::Range {
        lower: Some(Box::new(Value::angle(0.0))),
        upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
        lower_inclusive: true,
        upper_inclusive: true,
    }
}

pub(crate) fn identity_transform_value() -> Value {
    Value::Transform {
        rotation: Box::new(Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
        translation: Box::new(Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
    }
}

pub(crate) fn planar_xy_joint() -> Value {
    eval_builtin(
        "planar",
        &[
            axis_x_unit(),
            axis_y_unit(),
            length_range_0_to_1m(),
            length_range_0_to_1m(),
            angle_range_0_to_pi(),
        ],
    )
}

pub(crate) fn spherical_joint() -> Value {
    eval_builtin("spherical", &[angle_range_0_to_pi()])
}

pub(crate) fn cylindrical_z_joint() -> Value {
    eval_builtin(
        "cylindrical",
        &[axis_z_unit(), length_range_0_to_1m(), angle_range_0_to_pi()],
    )
}

// ── KIN-OFFSET γ shared offset fixtures ──────────────────────────────────────
//
// Shared offset-joint constructors used by the joints/loop_closure/snapshot/
// dynamics test modules. The pivot offsets equal the link lengths so the 2-link
// FK reduces to the classic planar-arm closed form (PRD §7.2 design decision 4).

/// A revolute-Z joint with a pure-translation origin at (len_m, 0, 0) m.
///
/// Constructed via the 3-arg `revolute(axis_z, 0..π, point3(len, 0, 0))`
/// form added in α (task 4331).  `transform_at(joint, θ)` returns
/// `{rotation = R_z(θ), translation = (len, 0, 0)}` — the pivot is baked
/// into the "origin" key.
pub(crate) fn offset_revolute_z(len_m: f64) -> Value {
    let pivot = eval_builtin(
        "point3",
        &[Value::length(len_m), Value::length(0.0), Value::length(0.0)],
    );
    eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi(), pivot])
}

/// A prismatic-X joint with a pure-translation origin at (len_m, 0, 0) m.
///
/// `transform_at(joint, d)` returns `{rotation = I, translation = (len+d, 0, 0)}`.
pub(crate) fn offset_prismatic_x(len_m: f64) -> Value {
    let pivot = eval_builtin(
        "point3",
        &[Value::length(len_m), Value::length(0.0), Value::length(0.0)],
    );
    eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m(), pivot])
}

/// Two offset revolute-Z joints for the 2-link planar arm fixture (B3/B5).
///
/// Returns `(joint_a, joint_b)` where:
///   - `joint_a = offset_revolute_z(L_a = 0.3 m)` — parented to world in tests
///   - `joint_b = offset_revolute_z(L_b = 0.2 m)` — parented to joint_a
///
/// With θ_a = π/6 and θ_b = π/3, the link-B tip world position is:
///   `(L_a + L_b·cos θ_a, L_b·sin θ_a, 0)` = `(0.3 + 0.2·cos 30°, 0.2·sin 30°, 0)`
///                                            `≈ (0.473205, 0.1, 0)`
/// and the accumulated rotation is `R_z(θ_a + θ_b) = R_z(90°)`.
pub(crate) fn two_link_offset_chain() -> (Value, Value) {
    (offset_revolute_z(0.3), offset_revolute_z(0.2))
}

/// Extract the `world_transform` value for body at `idx` from a `snapshot` result.
///
/// Panics with a descriptive message if the snapshot is not a Map, the `"bodies"` field
/// is not a List, the body at `idx` is not a Map, or `"world_transform"` is absent.
/// Shared by snapshot.rs and dynamics/eval.rs tests to avoid repeating the
/// nested-extraction boilerplate.
pub(crate) fn body_world_transform(snap: &Value, idx: usize) -> &Value {
    let m = match snap {
        Value::Map(m) => m,
        other => panic!("expected Snapshot Map, got {:?}", other),
    };
    let bodies = match m.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        other => panic!("expected bodies List in snapshot, got {:?}", other),
    };
    let body = match &bodies[idx] {
        Value::Map(b) => b,
        other => panic!("expected body record Map at index {idx}, got {:?}", other),
    };
    body.get(&Value::String("world_transform".to_string()))
        .expect("body record must carry world_transform")
}
