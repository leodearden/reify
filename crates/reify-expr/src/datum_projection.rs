//! Pure evaluation of datum-projection member access on datum values; the
//! typing table lives in `crates/reify-compiler/src/datum_projection.rs`.

use reify_core::Type;
use reify_ir::{UnitVec3, Value, decode_direction, frame_basis};

/// Extract three finite numeric components from a 3-component `Point` or
/// `Vector` value. Returns `None` for any other shape or a non-finite
/// component.
pub(crate) fn datum_vec3(v: &Value) -> Option<[f64; 3]> {
    let comps = match v {
        Value::Point(c) | Value::Vector(c) if c.len() == 3 => c,
        _ => return None,
    };
    let a = comps[0].as_f64().filter(|f| f.is_finite())?;
    let b = comps[1].as_f64().filter(|f| f.is_finite())?;
    let c = comps[2].as_f64().filter(|f| f.is_finite())?;
    Some([a, b, c])
}

/// Basis axis `axis` (0=x, 1=y, 2=z) of a `Frame` as a unit vector, or `None`
/// for a non-Frame or a malformed basis.
fn frame_axis_unit(frame: &Value, axis: usize) -> Option<UnitVec3> {
    frame_basis(frame)
        .ok()
        .and_then(|rows| UnitVec3::from_vector(rows[axis]).ok())
}

/// Project a `Frame` basis axis (`.x`/`.y`/`.z`) to a unit `Direction`.
fn frame_axis_direction(frame: &Value, axis: usize) -> Value {
    frame_axis_unit(frame, axis).map_or(Value::Undef, Value::Direction)
}

/// Project a `Frame` to one of its principal planes: a `Plane` at the frame
/// origin whose `Direction` normal is basis axis `normal_axis` (0=x, 1=y, 2=z).
fn frame_plane(frame: &Value, origin: &Value, normal_axis: usize) -> Value {
    match frame_axis_unit(frame, normal_axis) {
        Some(normal) => Value::Plane {
            origin: Box::new(origin.clone()),
            normal: Box::new(Value::Direction(normal)),
        },
        None => Value::Undef,
    }
}

/// Evaluate a datum-projection member access (task 4382 β) on a datum receiver
/// (`Axis`/`Plane`/`Frame`/`Direction`); a `Frame` projects its basis axes, its
/// origin and its three principal planes (`xy_plane` normal = z, `yz_plane`
/// normal = x, `zx_plane` normal = y). Returns `Some(value)` when `obj` is a
/// datum and `method` is one of that datum's projection members (the projection
/// is `Undef` if the datum's internals are degenerate/malformed); returns
/// `None` for any non-datum receiver or unrecognized member, so the regular
/// collection/tensor method dispatch proceeds unchanged.
///
/// The compiler (expr.rs `MemberAccess` datum-projection branch) only lowers
/// *valid* projections to a `MethodCall`, so the disallowed cases (e.g. the
/// ambiguous `frame.dir`, or `axis.x`) never reach eval; the `_ => None` arms
/// are defensive.
pub(crate) fn eval_datum_projection(obj: &Value, method: &str) -> Option<Value> {
    match obj {
        Value::Axis { origin, direction } => match method {
            "dir" => Some(decode_direction(direction).map_or(Value::Undef, Value::Direction)),
            "origin" => Some((**origin).clone()),
            _ => None,
        },
        Value::Plane { origin, normal } => match method {
            "normal" => Some(decode_direction(normal).map_or(Value::Undef, Value::Direction)),
            "origin" => Some((**origin).clone()),
            _ => None,
        },
        Value::Frame { origin, .. } => match method {
            "x" => Some(frame_axis_direction(obj, 0)),
            "y" => Some(frame_axis_direction(obj, 1)),
            "z" => Some(frame_axis_direction(obj, 2)),
            "origin" => Some((**origin).clone()),
            "xy_plane" => Some(frame_plane(obj, origin, 2)),
            "yz_plane" => Some(frame_plane(obj, origin, 0)),
            "zx_plane" => Some(frame_plane(obj, origin, 1)),
            _ => None,
        },
        Value::Direction(u) => {
            let [x, y, z] = u.components();
            match method {
                "x" => Some(Value::Real(x)),
                "y" => Some(Value::Real(y)),
                "z" => Some(Value::Real(z)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// A structure's own frame: the identity frame at the world origin.
fn identity_frame() -> Value {
    Value::Frame {
        origin: Box::new(Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
        basis: Box::new(Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }),
    }
}

/// Evaluate an intrinsic `self.<member>` datum without evaluating its receiver.
/// A structure's `self` is its own identity frame (geometric-relations η design
/// §6), so `self.frame` is that frame and every other member is what the frame
/// projects. The compiler lowers `self.<datum>` to a `MethodCall` on the
/// `__self` `StructureRef` ref (expr.rs η arm), which has no cell at eval time;
/// it is the only `MethodCall` with a `StructureRef` receiver, because a
/// sub-ref's datums lower to `IndexAccess`. Returns `None` for any other
/// receiver type or member, so structural-query accessors (`self.children`, …)
/// fall through to their own expansion.
#[inline(never)]
pub(crate) fn eval_self_datum_projection(receiver_type: &Type, member: &str) -> Option<Value> {
    if !matches!(receiver_type, Type::StructureRef(_)) {
        return None;
    }
    match member {
        "frame" => Some(identity_frame()),
        _ => eval_datum_projection(&identity_frame(), member),
    }
}
