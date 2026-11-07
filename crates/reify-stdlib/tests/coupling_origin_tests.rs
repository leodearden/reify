//! A coupling re-drives its parent joint, mount included:
//! `transform_at(couple(p, r, o), v) == transform_at(p, r·v + o)` (task 7187).
//! Exercised only through the public `eval_builtin` / `set_mount_origin` surface.

use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2, FRAC_PI_3, PI};

use reify_ir::Value;
use reify_stdlib::{eval_builtin, set_mount_origin};

const TOL: f64 = 1e-12;

/// Input that turns the p2 dogfood lead screw through ten 5 mm leads (50 mm of travel).
const TEN_LEADS_M: f64 = 20.0 * PI;

type Quaternion = (f64, f64, f64, f64);

const IDENTITY: Quaternion = (1.0, 0.0, 0.0, 0.0);
const R_Z_90: Quaternion = (FRAC_1_SQRT_2, 0.0, 0.0, FRAC_1_SQRT_2);
const R_Z_MINUS_90: Quaternion = (FRAC_1_SQRT_2, 0.0, 0.0, -FRAC_1_SQRT_2);

fn len_point3(x_m: f64, y_m: f64, z_m: f64) -> Value {
    eval_builtin(
        "point3",
        &[Value::length(x_m), Value::length(y_m), Value::length(z_m)],
    )
}

fn axis(x: f64, y: f64, z: f64) -> Value {
    Value::Vector(vec![Value::Real(x), Value::Real(y), Value::Real(z)])
}

fn closed_range(lower: Value, upper: Value) -> Value {
    Value::Range {
        lower: Some(Box::new(lower)),
        upper: Some(Box::new(upper)),
        lower_inclusive: true,
        upper_inclusive: true,
    }
}

fn r_z(theta: f64) -> Value {
    eval_builtin(
        "orient_axis_angle",
        &[axis(0.0, 0.0, 1.0), Value::angle(theta)],
    )
}

fn frame_at_origin(basis: Value) -> Value {
    eval_builtin("frame3", &[len_point3(0.0, 0.0, 0.0), basis])
}

/// The p2 dogfood lift: +Z travel, pivoted at the (100 mm, 50 mm) corner.
fn corner_lift() -> Value {
    eval_builtin(
        "prismatic",
        &[
            axis(0.0, 0.0, 1.0),
            closed_range(Value::length(0.0), Value::length(100.0)),
            len_point3(0.1, 0.05, 0.0),
        ],
    )
}

fn lead_screw_nut(lift: &Value) -> Value {
    eval_builtin("screw", &[lift.clone(), Value::length(0.005)])
}

fn revolute_pivoted_at_300mm() -> Value {
    eval_builtin(
        "revolute",
        &[
            axis(0.0, 0.0, 1.0),
            closed_range(Value::angle(0.0), Value::angle(PI)),
            len_point3(0.3, 0.0, 0.0),
        ],
    )
}

fn prismatic_x_mounted_turned_90_about_z() -> Value {
    eval_builtin(
        "prismatic",
        &[
            axis(1.0, 0.0, 0.0),
            closed_range(Value::length(0.0), Value::length(1.0)),
            frame_at_origin(r_z(FRAC_PI_2)),
        ],
    )
}

fn unpivoted_prismatic_x() -> Value {
    eval_builtin(
        "prismatic",
        &[
            axis(1.0, 0.0, 0.0),
            closed_range(Value::length(0.0), Value::length(1.0)),
        ],
    )
}

fn decompose(transform: &Value, label: &str) -> (Quaternion, [f64; 3]) {
    let (rotation, translation) = match transform {
        Value::Transform {
            rotation,
            translation,
        } => (rotation.as_ref(), translation.as_ref()),
        other => panic!("{label}: expected Transform, got {other:?}"),
    };
    let quaternion = match rotation {
        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
        other => panic!("{label}: expected Orientation rotation, got {other:?}"),
    };
    let offsets = match translation {
        Value::Vector(c) if c.len() == 3 => [0, 1, 2].map(|i| {
            c[i].as_f64()
                .unwrap_or_else(|| panic!("{label}: translation[{i}] not numeric: {:?}", c[i]))
        }),
        other => panic!("{label}: expected Vector(3) translation, got {other:?}"),
    };
    (quaternion, offsets)
}

/// Quaternions are compared up to sign: `q` and `-q` are the same rotation.
fn assert_transform_close(
    actual: &Value,
    rotation: Quaternion,
    translation: [f64; 3],
    label: &str,
) {
    let ((w, x, y, z), offsets) = decompose(actual, label);
    let (ew, ex, ey, ez) = rotation;
    let near = |a: f64, b: f64| (a - b).abs() < TOL;
    let same_sign = near(w, ew) && near(x, ex) && near(y, ey) && near(z, ez);
    let flipped = near(w, -ew) && near(x, -ex) && near(y, -ey) && near(z, -ez);
    assert!(
        same_sign || flipped,
        "{label}: rotation expected ±({ew}, {ex}, {ey}, {ez}), got ({w}, {x}, {y}, {z})"
    );
    assert!(
        (0..3).all(|i| near(offsets[i], translation[i])),
        "{label}: translation expected {translation:?}, got {offsets:?}"
    );
}

fn map_field<'a>(value: &'a Value, key: &str) -> &'a Value {
    match value {
        Value::Map(m) => m
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("expected a {key:?} entry in {value:?}")),
        other => panic!("expected a Map carrying {key:?}, got {other:?}"),
    }
}

fn first_body_world_transform(snapshot: &Value) -> &Value {
    let first_body = match map_field(snapshot, "bodies") {
        Value::List(bodies) if !bodies.is_empty() => &bodies[0],
        other => panic!("expected a non-empty bodies List, got {other:?}"),
    };
    map_field(first_body, "world_transform")
}

#[test]
fn screw_follower_carries_parent_pivot() {
    let nut = lead_screw_nut(&corner_lift());
    let result = eval_builtin("transform_at", &[nut, Value::length(TEN_LEADS_M)]);
    assert_transform_close(
        &result,
        IDENTITY,
        [0.1, 0.05, 0.05],
        "screw follower of a corner-pivoted lift",
    );
}

#[test]
fn gear_follower_turns_about_parent_pivot() {
    let gear = eval_builtin(
        "gear",
        &[revolute_pivoted_at_300mm(), Value::Int(20), Value::Int(30)],
    );
    let result = eval_builtin("transform_at", &[gear, Value::angle(FRAC_PI_3)]);
    assert_transform_close(
        &result,
        R_Z_MINUS_90,
        [0.3, 0.0, 0.0],
        "gear follower of a revolute pivoted at 300 mm",
    );
}

#[test]
fn coupling_moves_along_parent_oriented_axis() {
    let coupling = eval_builtin(
        "couple",
        &[prismatic_x_mounted_turned_90_about_z(), Value::Real(2.0)],
    );
    let result = eval_builtin("transform_at", &[coupling, Value::length(0.25)]);
    assert_transform_close(
        &result,
        R_Z_90,
        [0.0, 0.5, 0.0],
        "coupling of a prismatic +X mounted turned 90° about Z",
    );
}

/// `couple(parent, ratio, offset)` driven at `input`; `motion_value` wraps an SI
/// number in the parent's motion dimension (length or angle).
struct CouplingCase {
    label: &'static str,
    parent: Value,
    ratio: f64,
    offset_si: f64,
    input_si: f64,
    motion_value: fn(f64) -> Value,
}

#[test]
fn coupling_transform_is_parent_transform_at_coupled_value() {
    let cases = [
        CouplingCase {
            label: "pivoted prismatic",
            parent: corner_lift(),
            ratio: 2.0,
            offset_si: 0.01,
            input_si: 0.02,
            motion_value: Value::length,
        },
        CouplingCase {
            label: "pivoted revolute",
            parent: revolute_pivoted_at_300mm(),
            ratio: -1.0,
            offset_si: 0.1,
            input_si: 0.3,
            motion_value: Value::angle,
        },
        CouplingCase {
            label: "oriented prismatic",
            parent: prismatic_x_mounted_turned_90_about_z(),
            ratio: 2.0,
            offset_si: 0.01,
            input_si: 0.25,
            motion_value: Value::length,
        },
        CouplingCase {
            label: "unpivoted prismatic (back-compat control)",
            parent: unpivoted_prismatic_x(),
            ratio: -1.0,
            offset_si: 0.01,
            input_si: 0.2,
            motion_value: Value::length,
        },
    ];
    for CouplingCase {
        label,
        parent,
        ratio,
        offset_si,
        input_si,
        motion_value,
    } in cases
    {
        let coupling = eval_builtin(
            "couple",
            &[parent.clone(), Value::Real(ratio), motion_value(offset_si)],
        );
        let via_coupling = eval_builtin("transform_at", &[coupling, motion_value(input_si)]);
        let direct = eval_builtin(
            "transform_at",
            &[parent, motion_value(ratio * input_si + offset_si)],
        );
        let (rotation, translation) = decompose(&direct, label);
        assert_transform_close(&via_coupling, rotation, translation, label);
    }
}

#[test]
fn coupling_own_origin_composes_outside_inherited_parent_mount() {
    let parent = eval_builtin(
        "prismatic",
        &[
            axis(1.0, 0.0, 0.0),
            closed_range(Value::length(0.0), Value::length(1.0)),
            len_point3(0.1, 0.0, 0.0),
        ],
    );
    let coupling = eval_builtin("couple", &[parent, Value::Real(1.0)]);
    let mounted = set_mount_origin(coupling.clone(), &frame_at_origin(r_z(FRAC_PI_2)));
    assert_ne!(
        mounted, coupling,
        "set_mount_origin must write the coupling's own origin"
    );

    let result = eval_builtin("transform_at", &[mounted, Value::length(0.3)]);
    assert_transform_close(
        &result,
        R_Z_90,
        [0.0, 0.4, 0.0],
        "own origin ∘ parent origin ∘ motion",
    );
}

#[test]
fn snapshot_places_screw_follower_body_at_parent_pivot() {
    let lift = corner_lift();
    let mechanism = eval_builtin(
        "body",
        &[
            eval_builtin("mechanism", &[]),
            Value::String("follower".to_string()),
            lead_screw_nut(&lift),
        ],
    );
    let bindings = Value::List(vec![eval_builtin(
        "bind",
        &[lift, Value::length(TEN_LEADS_M)],
    )]);
    let snapshot = eval_builtin("snapshot", &[mechanism, bindings]);
    assert_transform_close(
        first_body_world_transform(&snapshot),
        IDENTITY,
        [0.1, 0.05, 0.05],
        "follower body bound only through its lift",
    );
}
