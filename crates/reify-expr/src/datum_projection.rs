//! Pure evaluation of datum-projection member access on datum values; the
//! typing table lives in `crates/reify-compiler/src/datum_projection.rs`.

use reify_core::Type;
use reify_ir::Value;

/// Extract three finite numeric components from a 3-component `Point` or
/// `Vector` value. Returns `None` for any other shape or a non-finite
/// component. Mirrors `reify_eval::geometry_ops::point3_components` (which is
/// `pub(crate)` to reify-eval and so unreachable across the crate boundary).
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

/// Normalize a 3-vector to unit length. `None` if the magnitude is non-finite
/// or below the degeneracy floor (a zero/near-zero direction is not a valid
/// unit `Direction`).
fn unit3(v: [f64; 3]) -> Option<[f64; 3]> {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if mag.is_finite() && mag > 1e-12 {
        Some([v[0] / mag, v[1] / mag, v[2] / mag])
    } else {
        None
    }
}

/// Project a stored direction/normal `Point`/`Vector` to a unit `Direction`,
/// or `Undef` if the vector is degenerate or malformed.
fn vec3_to_direction(v: &Value) -> Value {
    match datum_vec3(v).and_then(unit3) {
        Some([x, y, z]) => Value::Direction { x, y, z },
        None => Value::Undef,
    }
}

/// Column `axis` (0=x, 1=y, 2=z) of the rotation matrix for the unit quaternion
/// `(w, x, y, z)` — i.e. the rotated world basis vector, a unit `Direction`.
/// The quaternion is normalized first; `None` for a non-finite or zero
/// quaternion. The column formulas match `reify_stdlib::orientation`'s
/// row-major `R` (and `quat_rotate`'s active `q·v·q*` convention).
fn quat_basis_axis(w: f64, x: f64, y: f64, z: f64, axis: usize) -> Option<[f64; 3]> {
    let n = (w * w + x * x + y * y + z * z).sqrt();
    if !n.is_finite() || n < 1e-12 {
        return None;
    }
    let (w, x, y, z) = (w / n, x / n, y / n, z / n);
    let col = match axis {
        0 => [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + w * z),
            2.0 * (x * z - w * y),
        ],
        1 => [
            2.0 * (x * y - w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + w * x),
        ],
        2 => [
            2.0 * (x * z + w * y),
            2.0 * (y * z - w * x),
            1.0 - 2.0 * (x * x + y * y),
        ],
        _ => return None,
    };
    Some(col)
}

/// Project a `Frame` basis axis (`.x`/`.y`/`.z`) to a unit `Direction`.
fn frame_axis_direction(basis: &Value, axis: usize) -> Value {
    match basis {
        Value::Orientation { w, x, y, z } => match quat_basis_axis(*w, *x, *y, *z, axis) {
            Some([dx, dy, dz]) => Value::Direction {
                x: dx,
                y: dy,
                z: dz,
            },
            None => Value::Undef,
        },
        _ => Value::Undef,
    }
}

/// Project a `Frame` to one of its principal planes: a `Plane` at the frame
/// origin whose normal is basis axis `normal_axis` (0=x, 1=y, 2=z), kept as the
/// 3-component `Vector` shape `decode_plane` requires.
fn frame_plane(origin: &Value, basis: &Value, normal_axis: usize) -> Value {
    match basis {
        Value::Orientation { w, x, y, z } => match quat_basis_axis(*w, *x, *y, *z, normal_axis) {
            Some([nx, ny, nz]) => Value::Plane {
                origin: Box::new(origin.clone()),
                normal: Box::new(Value::Vector(vec![
                    Value::Real(nx),
                    Value::Real(ny),
                    Value::Real(nz),
                ])),
            },
            None => Value::Undef,
        },
        _ => Value::Undef,
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
            "dir" => Some(vec3_to_direction(direction)),
            "origin" => Some((**origin).clone()),
            _ => None,
        },
        Value::Plane { origin, normal } => match method {
            "normal" => Some(vec3_to_direction(normal)),
            "origin" => Some((**origin).clone()),
            _ => None,
        },
        Value::Frame { origin, basis } => match method {
            "x" => Some(frame_axis_direction(basis, 0)),
            "y" => Some(frame_axis_direction(basis, 1)),
            "z" => Some(frame_axis_direction(basis, 2)),
            "origin" => Some((**origin).clone()),
            "xy_plane" => Some(frame_plane(origin, basis, 2)),
            "yz_plane" => Some(frame_plane(origin, basis, 0)),
            "zx_plane" => Some(frame_plane(origin, basis, 1)),
            _ => None,
        },
        Value::Direction { x, y, z } => match method {
            "x" => Some(Value::Real(*x)),
            "y" => Some(Value::Real(*y)),
            "z" => Some(Value::Real(*z)),
            _ => None,
        },
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
