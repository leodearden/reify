//! Grounding / self-datum eval tests (geometric-relations η, task 4387).
//!
//! Two surfaces live here:
//!  - step-9 (this file): the intrinsic `self.*` datum VALUES. A structure's
//!    `self` is its own identity frame (design §6), so `self.origin` is the
//!    world origin, `self.frame` the identity frame, `self.x/.y/.z` the unit
//!    axes, and `self.xy_plane/.yz_plane/.zx_plane` the principal planes —
//!    all kernel-free constants. The compiler (η step-8) lowers `self.<datum>`
//!    to a `MethodCall { object: ValueRef(__self : StructureRef), method, [] }`;
//!    eval (η step-10) intercepts it and yields the intrinsic constant.
//!  - step-17 (added later): the trace-to-ground / global-float check.
//!
//! RED (step-9): eval has no self-datum projection handler, so every `self.*`
//! datum cell evaluates to `Value::Undef` (the `__self` receiver resolves to
//! Undef and the `MethodCall` short-circuits). These value assertions fail
//! until step-10 lands the handler.

use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::eval_source;

const EPS: f64 = 1e-12;

/// Extract the three numeric components (SI for dimensioned) of a 3-component
/// `Value::Point` or `Value::Vector`.
fn comps3(v: &Value) -> [f64; 3] {
    let comps = match v {
        Value::Point(c) | Value::Vector(c) => c,
        other => panic!("expected a 3-component Point/Vector, got {other:?}"),
    };
    assert_eq!(comps.len(), 3, "expected 3 components, got {comps:?}");
    [
        comps[0].as_f64().expect("component 0 numeric"),
        comps[1].as_f64().expect("component 1 numeric"),
        comps[2].as_f64().expect("component 2 numeric"),
    ]
}

fn approx3(actual: [f64; 3], expected: [f64; 3]) {
    for i in 0..3 {
        assert!(
            (actual[i] - expected[i]).abs() < EPS,
            "component {i}: got {}, expected {}",
            actual[i],
            expected[i]
        );
    }
}

/// Compile+eval a one-structure source kernel-free and return the value of cell
/// `member` in structure `S`.
fn eval_cell(source: &str, member: &str) -> Value {
    let result = eval_source(source);
    let id = ValueCellId::new("S", member);
    result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("cell `{member}` missing from eval result"))
        .clone()
}

/// A structure binding every intrinsic self-datum to a let, so each cell can be
/// read back independently.
const SELF_DATUM_SRC: &str = r#"structure S {
    let o = self.origin
    let fr = self.frame
    let dx = self.x
    let dy = self.y
    let dz = self.z
    let pxy = self.xy_plane
    let pyz = self.yz_plane
    let pzx = self.zx_plane
}"#;

#[test]
fn self_origin_is_world_origin() {
    let o = eval_cell(SELF_DATUM_SRC, "o");
    approx3(comps3(&o), [0.0, 0.0, 0.0]);
}

#[test]
fn self_frame_is_identity_frame() {
    let fr = eval_cell(SELF_DATUM_SRC, "fr");
    match fr {
        Value::Frame { origin, basis } => {
            approx3(comps3(&origin), [0.0, 0.0, 0.0]);
            match *basis {
                Value::Orientation { w, x, y, z } => {
                    assert!((w - 1.0).abs() < EPS, "w should be 1, got {w}");
                    assert!(x.abs() < EPS, "x should be 0, got {x}");
                    assert!(y.abs() < EPS, "y should be 0, got {y}");
                    assert!(z.abs() < EPS, "z should be 0, got {z}");
                }
                other => panic!("frame basis should be Orientation, got {other:?}"),
            }
        }
        other => panic!("expected Value::Frame, got {other:?}"),
    }
}

#[test]
fn self_unit_axes_are_directions() {
    // x = (1,0,0), y = (0,1,0), z = (0,0,1)
    for (member, expected) in [
        ("dx", [1.0, 0.0, 0.0]),
        ("dy", [0.0, 1.0, 0.0]),
        ("dz", [0.0, 0.0, 1.0]),
    ] {
        let v = eval_cell(SELF_DATUM_SRC, member);
        match v {
            Value::Direction { x, y, z } => approx3([x, y, z], expected),
            other => panic!("expected Value::Direction for `{member}`, got {other:?}"),
        }
    }
}

#[test]
fn self_principal_planes_have_origin_and_axis_normals() {
    // xy_plane normal (0,0,1); yz_plane normal (1,0,0); zx_plane normal (0,1,0).
    // All three pass through the origin.
    for (member, normal) in [
        ("pxy", [0.0, 0.0, 1.0]),
        ("pyz", [1.0, 0.0, 0.0]),
        ("pzx", [0.0, 1.0, 0.0]),
    ] {
        let v = eval_cell(SELF_DATUM_SRC, member);
        match v {
            Value::Plane { origin, normal: n } => {
                approx3(comps3(&origin), [0.0, 0.0, 0.0]);
                approx3(comps3(&n), normal);
            }
            other => panic!("expected Value::Plane for `{member}`, got {other:?}"),
        }
    }
}
