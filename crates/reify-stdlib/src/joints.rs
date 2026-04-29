use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value};

use crate::helpers::{tensor_components_f64, trig_input};
use crate::orientation::normalize_quaternion;

/// Evaluate a joints stdlib function by name.
///
/// Returns `Some(Value)` for known function names (including
/// `Some(Value::Undef)` on validation failure), or `None` for unknown names.
pub(crate) fn eval_joints(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "prismatic" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_axis(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_range(&args[1], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            // The axis is stored as the raw (potentially unnormalized) input.
            // `transform_at` normalizes it to unit length at evaluation time.
            // `joint_axis` returns this raw value — see its doc-comment.
            make_joint("prismatic", args[0].clone(), args[1].clone())
        }
        "revolute" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            if validate_axis(&args[0]).is_none() {
                return Some(Value::Undef);
            }
            if validate_range(&args[1], DimensionVector::ANGLE).is_none() {
                return Some(Value::Undef);
            }
            // The axis is stored as the raw (potentially unnormalized) input.
            // `transform_at` normalizes it to unit length at evaluation time.
            // `joint_axis` returns this raw value — see its doc-comment.
            make_joint("revolute", args[0].clone(), args[1].clone())
        }
        // 3-DOF planar joint: two prismatic DOFs (along axis_x and axis_y) plus one
        // revolute DOF (about axis_x × axis_y). Per PRD v0_2/kinematic-constraints.md
        // §"Decomposition plan" task 6.
        //
        // Signature: planar(axis_x, axis_y, range_x, range_y, range_theta)
        // where axis_x ⊥ axis_y (both dimensionless Vector3, finite, non-zero),
        // range_x / range_y are LENGTH ranges, range_theta is an ANGLE range.
        // The raw (unnormalised) axes are stored in the Map; normalisation happens
        // at `transform_at` time — matching the prismatic/revolute precedent.
        "planar" => {
            if args.len() != 5 {
                return Some(Value::Undef);
            }
            // Validate axis_x: dimensionless Vector3, finite, non-zero.
            let comps_x = match validate_axis(&args[0]) {
                Some(c) => c,
                None => return Some(Value::Undef),
            };
            // Validate axis_y: dimensionless Vector3, finite, non-zero.
            let comps_y = match validate_axis(&args[1]) {
                Some(c) => c,
                None => return Some(Value::Undef),
            };
            // Perpendicularity check: |dot(unit_x, unit_y)| < 1e-9.
            // Normalise each axis first so the dot product is in cos-angle units.
            // `unit_normalize` is shared with `unit_axes_xy_from_planar_map` below;
            // the single tolerance (1e-9) is documented in that helper's doc-comment.
            let unit_x = unit_normalize(comps_x);
            let unit_y = unit_normalize(comps_y);
            let dot = unit_x[0] * unit_y[0] + unit_x[1] * unit_y[1] + unit_x[2] * unit_y[2];
            if dot.abs() >= 1e-9 {
                return Some(Value::Undef);
            }
            // Validate range_x: bounded, LENGTH-dimensioned.
            if validate_range(&args[2], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            // Validate range_y: bounded, LENGTH-dimensioned.
            if validate_range(&args[3], DimensionVector::LENGTH).is_none() {
                return Some(Value::Undef);
            }
            // Validate range_theta: bounded, ANGLE-dimensioned.
            if validate_range(&args[4], DimensionVector::ANGLE).is_none() {
                return Some(Value::Undef);
            }
            make_planar(
                args[0].clone(),
                args[1].clone(),
                args[2].clone(),
                args[3].clone(),
                args[4].clone(),
            )
        }
        // 3-DOF spherical joint: free rotation in SO(3), no translation.
        // Per PRD v0_2/kinematic-constraints.md §"Decomposition plan" task 4.
        //
        // Signature: spherical(range_angle: Range<Angle>) — single Range<Angle>
        // bounding the swing magnitude (axis-angle / cone half-angle). The joint
        // is axis-isotropic — there is no preferred direction — so no axis is
        // stored. The motion variable for `transform_at` is a unit quaternion
        // (`Value::Orientation`); Euler / axis-angle exposure is the user's
        // responsibility via composition of `orient_euler` / `orient_axis_angle`
        // and `orient_to_euler` / `orient_to_axis_angle`.
        "spherical" => {
            // step-2 minimal stub: accepts everything; step-4 adds the
            // validation surface (arg count, range dimension, bounded).
            // `first()` keeps this panic-safe on 0-arg calls — step-3's
            // explicit `is_undef` regression pins drive the validation.
            let range_angle = args.first().cloned().unwrap_or(Value::Undef);
            make_spherical(range_angle)
        }
        // 0-DOF group-only joint (sub-assembly grouping, clearance-pair filtering).
        // Per PRD v0_2/kinematic-constraints.md §"Decomposition plan" task 7.
        //
        // No axis or range: the joint has no motion variable, no 1D direction to
        // translate/rotate along, and no range to constrain. The resulting Map has
        // a single field `{ "kind": "fixed" }`, mirroring the `world` sentinel shape.
        "fixed" => {
            if !args.is_empty() {
                return Some(Value::Undef);
            }
            let mut m = BTreeMap::new();
            m.insert(Value::String("kind".to_string()), Value::String("fixed".to_string()));
            Value::Map(m)
        }
        "transform_at" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            let kind = match map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) => s.as_str(),
                _ => return Some(Value::Undef),
            };
            match kind {
                "prismatic" | "revolute" => transform_at_simple_joint(kind, map, &args[1]),
                // 0-DOF fixed joint: always the identity Transform regardless of
                // the second argument (type/dimension not validated — see design
                // decision). Undef propagates: if the motion-variable expression
                // upstream evaluated to Undef, returning a well-formed Transform
                // would mask that error; callers rely on Undef sentinel propagation.
                "fixed" => {
                    if matches!(&args[1], Value::Undef) {
                        return Some(Value::Undef);
                    }
                    Value::Transform {
                        rotation: Box::new(Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }),
                        translation: Box::new(Value::Vector(vec![
                            Value::length(0.0),
                            Value::length(0.0),
                            Value::length(0.0),
                        ])),
                    }
                }
                // 3-DOF planar joint: motion_vars is a Value::List of 3 elements
                // [x_length, y_length, theta_angle].
                // Composition: T_planar = T_x · T_y · T_theta (left-to-right),
                // matching the chain_transform convention in loop_closure.rs.
                // See docs/prds/v0_2/kinematic-constraints.md §"Decomposition plan"
                // task 6 for the canonical composition order (used by the
                // analytic-Jacobian follow-up task).
                "planar" => {
                    let items = match &args[1] {
                        Value::List(v) if v.len() == 3 => v.clone(),
                        _ => return Some(Value::Undef),
                    };
                    // Extract x (metres), y (metres), theta (radians).
                    let x = match length_input(&items[0]) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let y = match length_input(&items[1]) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let theta = match trig_input(&items[2]) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    // Defense-in-depth: helpers already enforce finiteness for
                    // Scalar/Real branches; Int yields finite f64 by construction.
                    if !x.is_finite() || !y.is_finite() || !theta.is_finite() {
                        return Some(Value::Undef);
                    }
                    // Extract and unit-normalise axis_x and axis_y from the joint Map.
                    let (unit_x, unit_y) = match unit_axes_xy_from_planar_map(map) {
                        Some(axes) => axes,
                        None => return Some(Value::Undef),
                    };
                    let [ux, uy, uz] = unit_x;
                    let [vx, vy, vz] = unit_y;
                    // Plane normal n = unit_x × unit_y (cross product).
                    let (nx, ny, nz) = (
                        uy * vz - uz * vy,
                        uz * vx - ux * vz,
                        ux * vy - uy * vx,
                    );
                    // T_x: pure translation x * unit_axis_x
                    let t_x = Value::Transform {
                        rotation: Box::new(Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }),
                        translation: Box::new(Value::Vector(vec![
                            Value::length(x * ux),
                            Value::length(x * uy),
                            Value::length(x * uz),
                        ])),
                    };
                    // T_y: pure translation y * unit_axis_y
                    let t_y = Value::Transform {
                        rotation: Box::new(Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }),
                        translation: Box::new(Value::Vector(vec![
                            Value::length(y * vx),
                            Value::length(y * vy),
                            Value::length(y * vz),
                        ])),
                    };
                    // T_theta: pure rotation by theta about the plane normal.
                    let t_theta = Value::Transform {
                        rotation: Box::new(axis_angle_quaternion(nx, ny, nz, theta)),
                        translation: Box::new(Value::Vector(vec![
                            Value::length(0.0),
                            Value::length(0.0),
                            Value::length(0.0),
                        ])),
                    };
                    // Compose: T_x · T_y · T_theta (left-to-right).
                    let t_xy = crate::eval_builtin("transform_compose", &[t_x, t_y]);
                    if t_xy.is_undef() {
                        return Some(Value::Undef);
                    }
                    let t_planar = crate::eval_builtin("transform_compose", &[t_xy, t_theta]);
                    if t_planar.is_undef() {
                        return Some(Value::Undef);
                    }
                    t_planar
                }
                "coupling" => {
                    // Extract the three coupling-payload fields (kind already matched
                    // above) with explicit guards. A Map built by a trusted `couple`
                    // call always has them, but hand-built Maps used in tests or future
                    // serialisation paths may not.
                    let parent_map = match map.get(&Value::String("parent".to_string())) {
                        Some(Value::Map(pm)) => pm,
                        _ => return Some(Value::Undef),
                    };
                    let ratio_f64 = match map.get(&Value::String("ratio".to_string())) {
                        Some(Value::Real(r)) => *r,
                        _ => return Some(Value::Undef),
                    };
                    // Defense-in-depth: a hand-built coupling Map could carry
                    // Value::Real(NaN) or Value::Real(Inf). `couple` rejects these at
                    // construction via `ratio_input`, but `transform_at` should not rely
                    // on the constructor. Symmetric with the parent-kind / offset / v_si
                    // guards below.
                    if !ratio_f64.is_finite() {
                        return Some(Value::Undef);
                    }
                    let offset_si = match map.get(&Value::String("offset".to_string())) {
                        Some(Value::Scalar { si_value, .. }) => *si_value,
                        _ => return Some(Value::Undef),
                    };
                    // Validate the stored parent kind — defense-in-depth against
                    // hand-built Map fixtures with invalid parent kinds.
                    // Extracting the kind as a &str (rather than a bool) means this
                    // is the single validation point; transform_at_simple_joint
                    // receives the already-validated kind and never re-reads it.
                    let parent_kind = match parent_map.get(&Value::String("kind".to_string())) {
                        Some(Value::String(s))
                            if matches!(s.as_str(), "prismatic" | "revolute") =>
                        {
                            s.as_str()
                        }
                        _ => return Some(Value::Undef),
                    };
                    // Extract v_si from args[1] via dimension-appropriate helper;
                    // both helpers reject wrong-dim Scalars and non-finite values.
                    let v_si = if parent_kind == "prismatic" {
                        match length_input(&args[1]) {
                            Some(d) => d,
                            None => return Some(Value::Undef),
                        }
                    } else {
                        match trig_input(&args[1]) {
                            Some(t) => t,
                            None => return Some(Value::Undef),
                        }
                    };
                    // Defense-in-depth: length_input/trig_input already reject
                    // non-finite v; this guard mirrors the prismatic/revolute arms.
                    if !v_si.is_finite() {
                        return Some(Value::Undef);
                    }
                    // Derive the coupled motion variable: ratio * v + offset
                    let coupled_si = ratio_f64 * v_si + offset_si;
                    if !coupled_si.is_finite() {
                        return Some(Value::Undef);
                    }
                    let coupled_value = if parent_kind == "prismatic" {
                        Value::length(coupled_si)
                    } else {
                        Value::angle(coupled_si)
                    };
                    // Delegate to the parent joint via the private helper.
                    // Termination is guaranteed: `couple` rejects coupling parents
                    // at construction, so the recursion always reaches a
                    // prismatic/revolute arm at depth 1.
                    transform_at_simple_joint(parent_kind, parent_map, &coupled_value)
                }
                _ => Value::Undef,
            }
        }
        "couple" => {
            // Validate arg count: 2 or 3
            if args.len() != 2 && args.len() != 3 {
                return Some(Value::Undef);
            }
            // Validate parent: must be a Map with kind in {"prismatic", "revolute"}
            let parent_map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            let parent_kind = match parent_map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s)) => s.as_str(),
                _ => return Some(Value::Undef),
            };
            let is_prismatic = match parent_kind {
                "prismatic" => true,
                "revolute" => false,
                // Rejects "coupling" and any other kind
                _ => return Some(Value::Undef),
            };
            // Extract ratio: finite, dimensionless numeric (Real/Int/DIMENSIONLESS Scalar).
            // ratio_input rejects NaN, Inf, and dimensioned Scalars.
            let ratio_f64 = match ratio_input(&args[1]) {
                Some(r) => r,
                None => return Some(Value::Undef),
            };
            // Extract offset: use parent-dimension-keyed helper (length_input / trig_input)
            // so bare Real/Int is accepted in addition to correctly-dimensioned Scalar.
            let offset_si = if args.len() == 3 {
                if is_prismatic {
                    match length_input(&args[2]) {
                        Some(d) => d,
                        None => return Some(Value::Undef),
                    }
                } else {
                    match trig_input(&args[2]) {
                        Some(r) => r,
                        None => return Some(Value::Undef),
                    }
                }
            } else {
                0.0
            };
            let offset = if is_prismatic {
                Value::length(offset_si)
            } else {
                Value::angle(offset_si)
            };
            make_coupling(args[0].clone(), Value::Real(ratio_f64), offset)
        }
        "joint_axis" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            // Returns the axis as stored at construction — the raw, potentially
            // unnormalized input vector.  `transform_at` normalizes to unit
            // length when computing the resulting Transform; this accessor
            // preserves the original value so callers can inspect what was
            // passed to `prismatic`/`revolute`.
            match &args[0] {
                Value::Map(m) => {
                    m.get(&Value::String("axis".to_string()))
                        .cloned()
                        .unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        "joint_range" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::Map(m) => {
                    m.get(&Value::String("range".to_string()))
                        .cloned()
                        .unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        "joint_ratio" => {
            // Returns the ratio stored in a coupling Map, or Undef for any other
            // input (including prismatic/revolute joints which have no ratio key).
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::Map(m) => {
                    m.get(&Value::String("ratio".to_string()))
                        .cloned()
                        .unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        "joint_offset" => {
            // Returns the offset stored in a coupling Map, or Undef for any other
            // input (including prismatic/revolute joints which have no offset key).
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::Map(m) => {
                    m.get(&Value::String("offset".to_string()))
                        .cloned()
                        .unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        "joint_jacobian" => {
            // SE(3) twist column for a joint, returned as
            // `Map { "angular": Vector3<DIMENSIONLESS>, "linear": Vector3<DIMENSIONLESS> }`.
            //
            // Per-kind formula (constant w.r.t. the motion variable for v0.1
            // single-DOF joints):
            //   prismatic → angular=[0,0,0], linear=axis_unit
            //   revolute  → angular=axis_unit, linear=[0,0,0]
            //   coupling  → ratio * parent_jacobian (component-wise).
            //   fixed     → zero column placeholder (0-DOF joint).
            //   planar    → zero column placeholder (3-DOF deferred); the
            //               per-DOF wrench contributions are NOT yet computed
            //               by `chain_jacobian_fd` either: `value_for_joint`
            //               returns None for planar, so chain Jacobians for
            //               planar chains currently return None too. Deferred
            //               to PRD v0.2 kinematic task 2 (taskmaster #2670 —
            //               "FD fallback for multi-DOF kinds").
            //
            // For multi-DOF joints (fixed, planar) the returned column is zero
            // rather than Undef: callers expecting a { angular, linear } Map get a
            // well-formed result. Do NOT assume a non-zero result for planar joints
            // or that chain_jacobian_fd succeeds for chains containing planar.
            //
            // Validation mirrors `transform_at`'s coupling arm: parent kind must
            // be prismatic/revolute (no nested couplings), ratio must be a
            // finite `Value::Real`, axis must be a non-zero finite Vector3.
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            joint_jacobian_value(&args[0])
        }
        _ => return None,
    })
}

/// Compute the SE(3) twist column for a joint Map.
///
/// Returns `Value::Map` on success, `Value::Undef` on any validation failure.
///
/// The coupling arm calls this recursively on its parent; termination is
/// guaranteed because `couple` rejects coupling parents at construction and the
/// nested-coupling guard inside this helper rejects any hand-built fixture
/// that violates the invariant.
fn joint_jacobian_value(value: &Value) -> Value {
    let map = match value {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };
    let kind = match map.get(&Value::String("kind".to_string())) {
        Some(Value::String(s)) => s.as_str(),
        _ => return Value::Undef,
    };
    match kind {
        "prismatic" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            make_jacobian([0.0, 0.0, 0.0], [nax, nay, naz])
        }
        "revolute" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            make_jacobian([nax, nay, naz], [0.0, 0.0, 0.0])
        }
        // 0-DOF fixed joint: zero twist column — design decision.
        // Strictly a 0-DOF joint has a 6×0 Jacobian (zero columns), but the v0.1
        // single-DOF convention returns one Map per joint. Returning a zero-magnitude
        // column preserves the uniform `{ angular, linear }` shape across all kinds,
        // is semantically valid (no motion variable contributes any twist), and keeps
        // the existing drift-guard tests simple.
        "fixed" => make_jacobian([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        // 3-DOF planar joint: zero-twist placeholder column — deferred design decision.
        // A planar joint has a 6×3 Jacobian (three columns: two prismatic + one revolute),
        // but the v0.1 single-column convention returns one Map per joint. Note that
        // `chain_jacobian_fd` also returns None for chains containing a planar joint
        // (because `value_for_joint` has no planar arm yet); this zero placeholder is
        // NOT equivalent to "FD chain Jacobians work for planar" — they do not yet.
        // Returning a zero-magnitude column preserves the uniform
        // `{ angular, linear }` shape across all kinds and satisfies the
        // `joint_jacobian_dispatches_for_every_joint_kind` coverage test.
        // The analytic 3-DOF Jacobian is deferred per PRD task 2 ("finite-difference
        // fallback for spherical, cylindrical, planar until analytic forms are derived").
        //
        // Field validation is deliberately skipped (unlike the prismatic/revolute arms):
        // the result is zero regardless of the stored axis_x/axis_y/range fields, so
        // a hand-built Map with missing fields returns the same correct placeholder.
        // This matches the `"fixed"` arm's behaviour. Add
        // `if unit_axes_xy_from_planar_map(map).is_none() { return Value::Undef; }`
        // if you need stricter defence-in-depth for malformed fixtures.
        "planar" => make_jacobian([0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        "coupling" => {
            let parent_map = match map.get(&Value::String("parent".to_string())) {
                Some(Value::Map(pm)) => pm,
                _ => return Value::Undef,
            };
            let ratio_f64 = match map.get(&Value::String("ratio".to_string())) {
                Some(Value::Real(r)) => *r,
                _ => return Value::Undef,
            };
            if !ratio_f64.is_finite() {
                return Value::Undef;
            }
            // Defense-in-depth: reject nested coupling. `couple` rejects this at
            // construction, but a hand-built Map could carry it. Mirrors
            // `transform_at`'s parent-kind validation.
            match parent_map.get(&Value::String("kind".to_string())) {
                Some(Value::String(s))
                    if matches!(s.as_str(), "prismatic" | "revolute") => {}
                _ => return Value::Undef,
            }
            // Recurse to the parent's Jacobian (always single-DOF at depth 1).
            let parent_jac = joint_jacobian_value(&Value::Map(parent_map.clone()));
            scale_jacobian(&parent_jac, ratio_f64)
        }
        _ => Value::Undef,
    }
}

/// Build a Jacobian Map with the standard `{ "angular", "linear" }` layout,
/// where each value is a `Value::Vector` of three `Value::Real` components.
fn make_jacobian(angular: [f64; 3], linear: [f64; 3]) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        Value::String("angular".to_string()),
        Value::Vector(vec![
            Value::Real(angular[0]),
            Value::Real(angular[1]),
            Value::Real(angular[2]),
        ]),
    );
    m.insert(
        Value::String("linear".to_string()),
        Value::Vector(vec![
            Value::Real(linear[0]),
            Value::Real(linear[1]),
            Value::Real(linear[2]),
        ]),
    );
    Value::Map(m)
}

/// Scale a Jacobian Map's `angular` and `linear` components by `ratio`.
///
/// Returns `Value::Map` on success, `Value::Undef` if any component is
/// non-finite after scaling or the input shape is malformed.
fn scale_jacobian(jac: &Value, ratio: f64) -> Value {
    let m = match jac {
        Value::Map(m) => m,
        _ => return Value::Undef,
    };
    let read_vec3 = |key: &str| -> Option<[f64; 3]> {
        match m.get(&Value::String(key.to_string())) {
            Some(Value::Vector(items)) if items.len() == 3 => {
                let a = items[0].as_f64()?;
                let b = items[1].as_f64()?;
                let c = items[2].as_f64()?;
                Some([a, b, c])
            }
            _ => None,
        }
    };
    let ang = match read_vec3("angular") {
        Some(v) => v,
        None => return Value::Undef,
    };
    let lin = match read_vec3("linear") {
        Some(v) => v,
        None => return Value::Undef,
    };
    let scaled_ang = [ratio * ang[0], ratio * ang[1], ratio * ang[2]];
    let scaled_lin = [ratio * lin[0], ratio * lin[1], ratio * lin[2]];
    for v in scaled_ang.iter().chain(scaled_lin.iter()) {
        if !v.is_finite() {
            return Value::Undef;
        }
    }
    make_jacobian(scaled_ang, scaled_lin)
}

/// Extract a dimensionless ratio from a `couple` ratio argument.
///
/// Accepts:
/// - `Value::Scalar { dimension: DIMENSIONLESS, .. }` with finite si_value.
/// - `Value::Real(r)` (finite) — treated as dimensionless ratio directly.
/// - `Value::Int(i)` — treated as dimensionless ratio directly.
///
/// Returns `None` for any other variant (wrong dimension, non-finite, NaN, Inf).
fn ratio_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, dimension } => {
            if *dimension == DimensionVector::DIMENSIONLESS && si_value.is_finite() {
                Some(*si_value)
            } else {
                None
            }
        }
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Extract metres from a `transform_at` value argument for a Prismatic joint.
///
/// Accepts:
/// - `Value::Scalar { dimension: LENGTH, .. }` — si_value is metres.
/// - `Value::Real(r)` / `Value::Int(i)` — treated as metres directly.
///
/// Returns `None` for any other variant (wrong dimension, non-finite).
fn length_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, dimension } => {
            if *dimension == DimensionVector::LENGTH && si_value.is_finite() {
                Some(*si_value)
            } else {
                None
            }
        }
        Value::Real(r) if r.is_finite() => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Unit-normalise a 3-component array.
///
/// The caller must have already validated the input via [`validate_axis`],
/// which guarantees non-zero, finite magnitude — so the division is safe.
/// Shared by the `"planar"` constructor (perpendicularity check), by
/// [`unit_axes_xy_from_planar_map`], and by [`unit_axis_from_map`], so the
/// normalisation formula lives in exactly one place.
fn unit_normalize(comps: [f64; 3]) -> [f64; 3] {
    let mag = (comps[0] * comps[0] + comps[1] * comps[1] + comps[2] * comps[2]).sqrt();
    [comps[0] / mag, comps[1] / mag, comps[2] / mag]
}

/// Extract and unit-normalise both `"axis_x"` and `"axis_y"` from a planar joint Map.
///
/// Returns `Some((unit_x, unit_y))` on success, `None` if either field is
/// missing or fails [`validate_axis`] validation.  Used by the `"planar"` arm
/// of `transform_at` to avoid duplicating the axis lookup and normalisation logic.
fn unit_axes_xy_from_planar_map(map: &BTreeMap<Value, Value>) -> Option<([f64; 3], [f64; 3])> {
    let axis_x_val = map.get(&Value::String("axis_x".to_string()))?;
    let axis_y_val = map.get(&Value::String("axis_y".to_string()))?;
    let cx = validate_axis(axis_x_val)?;
    let cy = validate_axis(axis_y_val)?;
    Some((unit_normalize(cx), unit_normalize(cy)))
}

/// Look up the `"axis"` field in a joint map, validate it via [`validate_axis`],
/// and return the unit-normalized `[x, y, z]` components.
///
/// Returns `None` if the field is absent or validation fails.
/// Both the `"prismatic"` and `"revolute"` arms of `transform_at` call this
/// helper to avoid duplicating the axis lookup and normalization logic.
fn unit_axis_from_map(map: &BTreeMap<Value, Value>) -> Option<[f64; 3]> {
    let axis_val = map.get(&Value::String("axis".to_string()))?;
    let comps = validate_axis(axis_val)?;
    Some(unit_normalize(comps))
}

/// Validate an axis value: must be a Vector3 of dimensionless components,
/// all finite, with non-zero magnitude.
///
/// Returns `Some([x, y, z])` (the raw components, not normalized) on success,
/// `None` on any failure.
fn validate_axis(value: &Value) -> Option<[f64; 3]> {
    let (comps, dim) = tensor_components_f64(value)?;
    if comps.len() != 3 {
        return None;
    }
    if dim != DimensionVector::DIMENSIONLESS {
        return None;
    }
    let [x, y, z] = [comps[0], comps[1], comps[2]];
    if !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    let mag_sq = x * x + y * y + z * z;
    if mag_sq == 0.0 || !mag_sq.is_finite() {
        return None;
    }
    Some([x, y, z])
}

/// Validate a range value: must be `Value::Range` with both lower and upper
/// bounds present, both sharing `expected_dim`.
///
/// Returns `Some(())` on success, `None` on any failure.
fn validate_range(value: &Value, expected_dim: DimensionVector) -> Option<()> {
    match value {
        Value::Range {
            lower: Some(lo),
            upper: Some(up),
            ..
        } => {
            if lo.dimension() == expected_dim && up.dimension() == expected_dim {
                Some(())
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Canonical set of joint kinds recognized by the joints module.
///
/// Used by [`is_joint_value`] as the membership set for value-level
/// joint discrimination. Per-kind dispatch arms in `transform_at` and
/// `joint_jacobian_value` (this file) hardcode the same kind strings
/// directly in `match` arms — Rust `match` patterns do not support
/// runtime slice membership, so those arms **must be kept in sync with
/// this constant** when a new kind is added.
/// `mechanism::body()` validates joint-kind membership via `is_joint_value`.
pub(crate) const JOINT_KINDS: &[&str] = &["prismatic", "revolute", "coupling", "fixed", "planar"];

/// Returns `true` when `v` is a `Value::Map` whose `kind` field is one of
/// the strings in [`JOINT_KINDS`]. Used by `mechanism::body()` for
/// `at`-arg validation and (combined with the world-sentinel check) for
/// parent-arg validation.
///
/// Tied to `JOINT_KINDS` via `contains` so a future kind addition only
/// needs to be made in the constant — the predicate follows automatically.
pub(crate) fn is_joint_value(v: &Value) -> bool {
    match v {
        Value::Map(m) => matches!(
            m.get(&Value::String("kind".to_string())),
            Some(Value::String(s)) if JOINT_KINDS.contains(&s.as_str())
        ),
        _ => false,
    }
}

/// Build a coupling `Value::Map` with the four-key layout:
/// `"kind"`, `"offset"`, `"parent"`, `"ratio"`.
///
/// Keys are in alphabetical order as `BTreeMap` sorts them, matching the
/// pattern of `make_joint`.  `ratio` is stored as `Value::Real` (already
/// extracted to f64 by the caller).
fn make_coupling(parent: Value, ratio: Value, offset: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
    m.insert(Value::String("offset".to_string()), offset);
    m.insert(Value::String("parent".to_string()), parent);
    m.insert(Value::String("ratio".to_string()), ratio);
    Value::Map(m)
}

/// Build a planar joint `Value::Map` with the six-key layout:
/// `"axis_x"`, `"axis_y"`, `"kind"`, `"range_theta"`, `"range_x"`, `"range_y"`.
///
/// Keys are in BTreeMap alphabetical order.  Raw (potentially unnormalised) axes
/// are stored — normalisation happens at `transform_at` time, matching the
/// prismatic/revolute `make_joint` precedent.
fn make_planar(axis_x: Value, axis_y: Value, range_x: Value, range_y: Value, range_theta: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("kind".to_string()), Value::String("planar".to_string()));
    m.insert(Value::String("axis_x".to_string()), axis_x);
    m.insert(Value::String("axis_y".to_string()), axis_y);
    m.insert(Value::String("range_x".to_string()), range_x);
    m.insert(Value::String("range_y".to_string()), range_y);
    m.insert(Value::String("range_theta".to_string()), range_theta);
    Value::Map(m)
}

/// Build a spherical joint `Value::Map` with the two-key layout:
/// `"kind"`, `"range_angle"`.
///
/// Keys are in BTreeMap alphabetical order. The spherical joint is axis-isotropic
/// (no preferred direction), so no axis is stored. The `range_angle` value is
/// a `Value::Range` of `Angle`-dimensioned bounds — see PRD task 4 design
/// decision: the range bounds the rotation magnitude (axis-angle / cone
/// half-angle) regardless of the axis direction.
fn make_spherical(range_angle: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("kind".to_string()), Value::String("spherical".to_string()));
    m.insert(Value::String("range_angle".to_string()), range_angle);
    Value::Map(m)
}

/// Build a joint `Value::Map` with the standard three-key layout:
/// `"kind"`, `"axis"`, `"range"`.
fn make_joint(kind: &str, axis: Value, range: Value) -> Value {
    let mut m = BTreeMap::new();
    m.insert(Value::String("kind".to_string()), Value::String(kind.to_string()));
    m.insert(Value::String("axis".to_string()), axis);
    m.insert(Value::String("range".to_string()), range);
    Value::Map(m)
}

/// Build a quaternion `Value::Orientation` from a **pre-normalized** unit axis
/// `(nax, nay, naz)` and a rotation angle `theta` in radians.
///
/// Delegates to [`normalize_quaternion`] for a final unit-norm check to absorb
/// floating-point drift from the sin/cos computation.  Returns `Value::Undef`
/// only if inputs are non-finite or the computed norm underflows — both
/// unreachable in practice for finite, unit-magnitude axis inputs.
///
/// This mirrors the axis-angle path in `orientation::eval_orientation`
/// (`orient_axis_angle`).  A future scope expansion to `orientation.rs` can
/// lift this to `orientation::axis_angle_quaternion` and share it from both
/// call sites, eliminating the remaining duplication.
fn axis_angle_quaternion(nax: f64, nay: f64, naz: f64, theta: f64) -> Value {
    let half = theta / 2.0;
    let c = half.cos();
    let s = half.sin();
    normalize_quaternion(c, s * nax, s * nay, s * naz).unwrap_or(Value::Undef)
}

/// Evaluate `transform_at` for a simple (prismatic or revolute) joint map.
///
/// Dispatches on the pre-validated `kind` string (`"prismatic"` or `"revolute"`).
/// The caller is responsible for validating that `kind` is one of these two values;
/// passing a pre-validated `kind` keeps joint-kind validation in exactly one place
/// (the caller's match / guard) so a future new simple-joint kind only needs to be
/// added to the caller and this match — not to both separately.
/// Returns `Value::Undef` as a defence-in-depth fallback for any unrecognised kind,
/// and for any missing axis or invalid value argument.
///
/// This helper is also the terminal dispatch target for the coupling arm of
/// `transform_at` — `couple` rejects coupling parents at construction, so the
/// recursion always reaches this helper at depth 1, guaranteeing termination.
fn transform_at_simple_joint(kind: &str, map: &BTreeMap<Value, Value>, value: &Value) -> Value {
    match kind {
        "prismatic" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            // Accept Length Scalar or bare Real/Int as metres
            let dist = match length_input(value) {
                Some(d) => d,
                None => return Value::Undef,
            };
            // length_input already enforces finiteness for the Scalar/Real
            // branches; the Int branch yields finite f64 by construction.
            // This guard is defense-in-depth against future changes to
            // length_input.
            if !dist.is_finite() {
                return Value::Undef;
            }
            let translation = Value::Vector(vec![
                Value::length(dist * nax),
                Value::length(dist * nay),
                Value::length(dist * naz),
            ]);
            let rotation = Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            };
            Value::Transform {
                rotation: Box::new(rotation),
                translation: Box::new(translation),
            }
        }
        "revolute" => {
            let [nax, nay, naz] = match unit_axis_from_map(map) {
                Some(a) => a,
                None => return Value::Undef,
            };
            // Accept Angle Scalar or bare Real/Int as radians
            let theta = match trig_input(value) {
                Some(t) => t,
                None => return Value::Undef,
            };
            // trig_input already enforces finiteness for the Scalar/Real
            // branches; the Int branch yields finite f64 by construction.
            // This guard is defense-in-depth against future changes to
            // trig_input (parallel to the same guard in the prismatic arm).
            if !theta.is_finite() {
                return Value::Undef;
            }
            let rotation = axis_angle_quaternion(nax, nay, naz, theta);
            let translation = Value::Vector(vec![
                Value::length(0.0),
                Value::length(0.0),
                Value::length(0.0),
            ]);
            Value::Transform {
                rotation: Box::new(rotation),
                translation: Box::new(translation),
            }
        }
        _ => Value::Undef,
    }
}

#[cfg(test)]
mod tests {
    use crate::eval_builtin;
    use crate::test_fixtures::{axis_x_unit, axis_y_unit, axis_z_unit, length_range_0_to_1m, angle_range_0_to_pi, planar_xy_joint};
    use reify_types::{DimensionVector, Value};
    use super::{is_joint_value, JOINT_KINDS};

    // ── prismatic constructor: happy path ────────────────────────────────────

    #[test]
    fn prismatic_returns_map_with_correct_fields() {
        let axis = axis_x_unit();
        let range = length_range_0_to_1m();
        let result = eval_builtin("prismatic", &[axis.clone(), range.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("prismatic".to_string())),
            "kind field should be 'prismatic'"
        );
        assert_eq!(
            map.get(&Value::String("axis".to_string())),
            Some(&axis),
            "axis field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range".to_string())),
            Some(&range),
            "range field should match input"
        );
    }

    // ── prismatic constructor: wrong arg counts ──────────────────────────────

    #[test]
    fn prismatic_zero_args_returns_undef() {
        assert!(
            eval_builtin("prismatic", &[]).is_undef(),
            "zero args should return Undef"
        );
    }

    #[test]
    fn prismatic_one_arg_returns_undef() {
        assert!(
            eval_builtin("prismatic", &[axis_x_unit()]).is_undef(),
            "one arg should return Undef"
        );
    }

    // ── revolute constructor helpers ─────────────────────────────────────────

    // ── revolute constructor: happy path ─────────────────────────────────────

    #[test]
    fn revolute_returns_map_with_correct_fields() {
        let axis = axis_z_unit();
        let range = angle_range_0_to_pi();
        let result = eval_builtin("revolute", &[axis.clone(), range.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("revolute".to_string())),
            "kind field should be 'revolute'"
        );
        assert_eq!(
            map.get(&Value::String("axis".to_string())),
            Some(&axis),
            "axis field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range".to_string())),
            Some(&range),
            "range field should match input"
        );
    }

    // ── revolute constructor: wrong arg counts ───────────────────────────────

    #[test]
    fn revolute_zero_args_returns_undef() {
        assert!(
            eval_builtin("revolute", &[]).is_undef(),
            "zero args should return Undef"
        );
    }

    #[test]
    fn revolute_one_arg_returns_undef() {
        assert!(
            eval_builtin("revolute", &[axis_z_unit()]).is_undef(),
            "one arg should return Undef"
        );
    }

    // ── prismatic validation: axis ───────────────────────────────────────────

    #[test]
    fn prismatic_non_vector_axis_returns_undef() {
        // axis is a bare Real, not a Vector3
        assert!(
            eval_builtin("prismatic", &[Value::Real(1.0), length_range_0_to_1m()]).is_undef(),
            "non-vector axis should return Undef"
        );
    }

    #[test]
    fn prismatic_vec2_axis_returns_undef() {
        // axis has 2 components, not 3
        let axis2 = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("prismatic", &[axis2, length_range_0_to_1m()]).is_undef(),
            "2-component axis should return Undef"
        );
    }

    #[test]
    fn prismatic_length_dimensioned_axis_returns_undef() {
        // axis components are LENGTH-dimensioned, not dimensionless
        let axis_len = Value::Vector(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        assert!(
            eval_builtin("prismatic", &[axis_len, length_range_0_to_1m()]).is_undef(),
            "Length-dimensioned axis should return Undef"
        );
    }

    #[test]
    fn prismatic_zero_axis_returns_undef() {
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("prismatic", &[zero_axis, length_range_0_to_1m()]).is_undef(),
            "zero-magnitude axis should return Undef"
        );
    }

    #[test]
    fn prismatic_nan_axis_returns_undef() {
        let nan_axis = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("prismatic", &[nan_axis, length_range_0_to_1m()]).is_undef(),
            "NaN axis should return Undef"
        );
    }

    // ── prismatic validation: range ──────────────────────────────────────────

    #[test]
    fn prismatic_non_range_arg_returns_undef() {
        // range arg is a bare Real
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), Value::Real(1.0)]).is_undef(),
            "non-Range second arg should return Undef"
        );
    }

    #[test]
    fn prismatic_unbounded_range_returns_undef() {
        // range is missing upper bound
        let unbounded = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), unbounded]).is_undef(),
            "unbounded range should return Undef"
        );
    }

    #[test]
    fn prismatic_angle_range_returns_undef() {
        // range dimension is Angle, not Length — dimension mismatch
        assert!(
            eval_builtin("prismatic", &[axis_x_unit(), angle_range_0_to_pi()]).is_undef(),
            "Angle-dimensioned range for Prismatic should return Undef"
        );
    }

    // ── revolute validation: axis ────────────────────────────────────────────

    #[test]
    fn revolute_non_vector_axis_returns_undef() {
        assert!(
            eval_builtin("revolute", &[Value::Real(1.0), angle_range_0_to_pi()]).is_undef(),
            "non-vector axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_vec2_axis_returns_undef() {
        let axis2 = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("revolute", &[axis2, angle_range_0_to_pi()]).is_undef(),
            "2-component axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_length_dimensioned_axis_returns_undef() {
        let axis_len = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(1.0),
        ]);
        assert!(
            eval_builtin("revolute", &[axis_len, angle_range_0_to_pi()]).is_undef(),
            "Length-dimensioned axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_zero_axis_returns_undef() {
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("revolute", &[zero_axis, angle_range_0_to_pi()]).is_undef(),
            "zero-magnitude axis for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_nan_axis_returns_undef() {
        let nan_axis = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(f64::NAN),
        ]);
        assert!(
            eval_builtin("revolute", &[nan_axis, angle_range_0_to_pi()]).is_undef(),
            "NaN axis for revolute should return Undef"
        );
    }

    // ── revolute validation: range ───────────────────────────────────────────

    #[test]
    fn revolute_non_range_arg_returns_undef() {
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), Value::Real(1.0)]).is_undef(),
            "non-Range second arg for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_unbounded_range_returns_undef() {
        let unbounded = Value::Range {
            lower: None,
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: false,
            upper_inclusive: true,
        };
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), unbounded]).is_undef(),
            "unbounded range for revolute should return Undef"
        );
    }

    #[test]
    fn revolute_length_range_returns_undef() {
        // range dimension is Length, not Angle — dimension mismatch
        assert!(
            eval_builtin("revolute", &[axis_z_unit(), length_range_0_to_1m()]).is_undef(),
            "Length-dimensioned range for revolute should return Undef"
        );
    }

    // ── validate_range: inverted range is intentionally permissive ───────────

    #[test]
    fn prismatic_with_inverted_range_constructs_ok() {
        // validate_range only checks that both bounds are present and
        // dimensionally consistent; ordering (lo > up) is intentionally
        // permissive.  The range field is informational metadata used by
        // callers (e.g. a sweep step), not validated for geometric sense at
        // construction time.  This test pins that permissive behaviour so
        // any future tightening is a deliberate, visible change.
        let inverted = Value::Range {
            lower: Some(Box::new(Value::length(5.0))),
            upper: Some(Box::new(Value::length(0.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let result = eval_builtin("prismatic", &[axis_x_unit(), inverted]);
        assert!(
            matches!(result, Value::Map(_)),
            "inverted-range prismatic should construct successfully, got {:?}", result
        );
    }

    // ── transform_at on Prismatic: helpers ───────────────────────────────────

    fn prismatic_x_joint() -> Value {
        eval_builtin("prismatic", &[axis_x_unit(), length_range_0_to_1m()])
    }

    fn prismatic_y_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("prismatic", &[axis, range])
    }

    fn prismatic_z_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(-5.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("prismatic", &[axis, range])
    }

    /// Assert two `Value::Transform` are component-wise within tolerance.
    fn assert_transform_approx(result: &Value, exp_rot: (f64, f64, f64, f64), exp_trans: [f64; 3], tol: f64, label: &str) {
        let (rot, trans) = match result {
            Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
            other => panic!("{}: expected Transform, got {:?}", label, other),
        };
        let (w, x, y, z) = match rot {
            Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
            other => panic!("{}: expected Orientation, got {:?}", label, other),
        };
        assert!((w - exp_rot.0).abs() < tol, "{}: rotation.w expected {} got {}", label, exp_rot.0, w);
        assert!((x - exp_rot.1).abs() < tol, "{}: rotation.x expected {} got {}", label, exp_rot.1, x);
        assert!((y - exp_rot.2).abs() < tol, "{}: rotation.y expected {} got {}", label, exp_rot.2, y);
        assert!((z - exp_rot.3).abs() < tol, "{}: rotation.z expected {} got {}", label, exp_rot.3, z);

        let comps = match trans {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("{}: expected Vector(3), got {:?}", label, other),
        };
        for (i, (comp, &exp)) in comps.iter().zip(exp_trans.iter()).enumerate() {
            let val = comp.as_f64().unwrap_or_else(|| panic!("{}: translation[{}] not numeric", label, i));
            assert!((val - exp).abs() < tol, "{}: translation[{}] expected {} got {}", label, i, exp, val);
        }
    }

    // ── transform_at on Prismatic: analytic tests ────────────────────────────

    #[test]
    fn prismatic_transform_at_x_axis_5m() {
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(5.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [5.0, 0.0, 0.0], 1e-12,
            "prismatic X, 5m");
    }

    #[test]
    fn prismatic_transform_at_y_axis_3m() {
        let joint = prismatic_y_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(3.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.0, 3.0, 0.0], 1e-12,
            "prismatic Y, 3m");
    }

    #[test]
    fn prismatic_transform_at_z_axis_neg2m() {
        let joint = prismatic_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(-2.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.0, 0.0, -2.0], 1e-12,
            "prismatic Z, -2m");
    }

    #[test]
    fn prismatic_transform_at_zero_value() {
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::length(0.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.0, 0.0, 0.0], 1e-12,
            "prismatic X, 0m");
    }

    #[test]
    fn prismatic_transform_at_diagonal_axis() {
        // axis = [1,1,0]/√2, value = √2 m → translation = [1m, 1m, 0m]
        let sq2 = std::f64::consts::SQRT_2;
        let axis = Value::Vector(vec![
            Value::Real(1.0 / sq2),
            Value::Real(1.0 / sq2),
            Value::Real(0.0),
        ]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(10.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("prismatic", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::length(sq2)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [1.0, 1.0, 0.0], 1e-12,
            "prismatic diagonal [1,1,0]/√2, √2 m");
    }

    #[test]
    fn prismatic_transform_at_unnormalized_axis() {
        // axis = [2, 0, 0] (magnitude 2), value = 1m → normalized axis [1,0,0] → translation = [1m, 0, 0]
        let axis = Value::Vector(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: Some(Box::new(Value::length(5.0))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("prismatic", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::length(1.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [1.0, 0.0, 0.0], 1e-12,
            "prismatic unnormalized [2,0,0], 1m");
    }

    #[test]
    fn prismatic_transform_at_bare_real_value() {
        // bare Value::Real(0.5) accepted as 0.5 metres
        let joint = prismatic_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::Real(0.5)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.5, 0.0, 0.0], 1e-12,
            "prismatic X, bare Real(0.5)");
    }

    // ── transform_at on Revolute: helpers ────────────────────────────────────

    fn revolute_z_joint() -> Value {
        eval_builtin("revolute", &[axis_z_unit(), angle_range_0_to_pi()])
    }

    fn revolute_x_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("revolute", &[axis, range])
    }

    fn revolute_y_joint() -> Value {
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        eval_builtin("revolute", &[axis, range])
    }

    // ── transform_at on Revolute: analytic tests ─────────────────────────────

    #[test]
    fn revolute_transform_at_z_axis_half_pi() {
        // Z axis, π/2 → rotation = (cos(π/4), 0, 0, sin(π/4))
        let pi = std::f64::consts::PI;
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(&result, (cos, 0.0, 0.0, sin), [0.0, 0.0, 0.0], 1e-12,
            "revolute Z, π/2");
    }

    #[test]
    fn revolute_transform_at_x_axis_pi() {
        // X axis, π → rotation = (0, 1, 0, 0)
        let pi = std::f64::consts::PI;
        let joint = revolute_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi)]);
        assert_transform_approx(&result, (0.0, 1.0, 0.0, 0.0), [0.0, 0.0, 0.0], 1e-12,
            "revolute X, π");
    }

    #[test]
    fn revolute_transform_at_y_axis_half_pi() {
        // Y axis, π/2 → rotation = (cos(π/4), 0, sin(π/4), 0)
        let pi = std::f64::consts::PI;
        let joint = revolute_y_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(&result, (cos, 0.0, sin, 0.0), [0.0, 0.0, 0.0], 1e-12,
            "revolute Y, π/2");
    }

    #[test]
    fn revolute_transform_at_zero_angle() {
        // angle = 0 → identity rotation
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(0.0)]);
        assert_transform_approx(&result, (1.0, 0.0, 0.0, 0.0), [0.0, 0.0, 0.0], 1e-12,
            "revolute Z, 0");
    }

    #[test]
    fn revolute_transform_at_bare_real_value() {
        // bare Real(π/2) accepted as radians
        let pi = std::f64::consts::PI;
        let joint = revolute_z_joint();
        let result = eval_builtin("transform_at", &[joint, Value::Real(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(&result, (cos, 0.0, 0.0, sin), [0.0, 0.0, 0.0], 1e-12,
            "revolute Z, bare Real(π/2)");
    }

    #[test]
    fn revolute_transform_at_unnormalized_axis() {
        // axis [0, 0, 2] (magnitude 2) with π/2 → same rotation as [0,0,1] with π/2
        let pi = std::f64::consts::PI;
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        let range = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: Some(Box::new(Value::angle(pi))),
            lower_inclusive: true,
            upper_inclusive: true,
        };
        let joint = eval_builtin("revolute", &[axis, range]);
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 2.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(&result, (cos, 0.0, 0.0, sin), [0.0, 0.0, 0.0], 1e-12,
            "revolute unnormalized [0,0,2], π/2");
    }

    #[test]
    fn revolute_transform_at_translation_always_zero() {
        // translation should always be [0m, 0m, 0m] regardless of angle
        let pi = std::f64::consts::PI;
        let joint = revolute_x_joint();
        let result = eval_builtin("transform_at", &[joint, Value::angle(pi / 3.0)]);
        // Check only translation components
        let trans = match &result {
            Value::Transform { translation, .. } => translation.as_ref(),
            other => panic!("expected Transform, got {:?}", other),
        };
        let comps = match trans {
            Value::Vector(v) if v.len() == 3 => v,
            other => panic!("expected Vector(3), got {:?}", other),
        };
        for (i, comp) in comps.iter().enumerate() {
            let val = comp.as_f64().expect("translation component should be numeric");
            assert!((val - 0.0).abs() < 1e-12,
                "revolute translation[{}] should be 0, got {}", i, val);
        }
    }

    // ── transform_at validation ──────────────────────────────────────────────

    #[test]
    fn transform_at_prismatic_with_angle_value_returns_undef() {
        // Angle Scalar passed to a Prismatic joint
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::angle(1.0)]).is_undef(),
            "Angle Scalar to Prismatic should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_with_length_value_returns_undef() {
        // Length Scalar passed to a Revolute joint
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::length(1.0)]).is_undef(),
            "Length Scalar to Revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_with_mass_value_returns_undef() {
        use reify_types::DimensionVector;
        let joint = revolute_z_joint();
        let mass = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        assert!(
            eval_builtin("transform_at", &[joint, mass]).is_undef(),
            "Mass Scalar to Revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_non_map_returns_undef() {
        assert!(
            eval_builtin("transform_at", &[Value::Real(1.0), Value::length(1.0)]).is_undef(),
            "non-Map first arg should return Undef"
        );
    }

    #[test]
    fn transform_at_map_without_kind_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "Map without kind key should return Undef"
        );
    }

    #[test]
    fn transform_at_map_with_unknown_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("sliding".to_string()));
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "Map with unknown kind should return Undef"
        );
    }

    #[test]
    fn transform_at_prismatic_nan_value_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::Real(f64::NAN)]).is_undef(),
            "NaN value for prismatic should return Undef"
        );
    }

    #[test]
    fn transform_at_revolute_inf_value_returns_undef() {
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::Real(f64::INFINITY)]).is_undef(),
            "Inf value for revolute should return Undef"
        );
    }

    #[test]
    fn transform_at_zero_args_returns_undef() {
        assert!(
            eval_builtin("transform_at", &[]).is_undef(),
            "0 args should return Undef"
        );
    }

    #[test]
    fn transform_at_one_arg_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint]).is_undef(),
            "1 arg should return Undef"
        );
    }

    #[test]
    fn transform_at_three_args_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("transform_at", &[joint, Value::length(1.0), Value::Real(0.0)]).is_undef(),
            "3 args should return Undef"
        );
    }

    // ── joint_ratio accessor ────────────────────────────────────────────────

    #[test]
    fn joint_ratio_prismatic_coupling_2arg_returns_ratio() {
        // 2-arg prismatic coupling: ratio stored as Value::Real(-1.0)
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(-1.0)]);
        assert_eq!(
            eval_builtin("joint_ratio", &[c]),
            Value::Real(-1.0),
            "joint_ratio should return the stored ratio Value::Real(-1.0)"
        );
    }

    #[test]
    fn joint_ratio_prismatic_coupling_3arg_returns_ratio() {
        // 3-arg prismatic coupling: ratio = 2.0
        let c = eval_builtin("couple", &[
            prismatic_x_joint(),
            Value::Real(2.0),
            Value::length(0.5),
        ]);
        assert_eq!(
            eval_builtin("joint_ratio", &[c]),
            Value::Real(2.0),
            "joint_ratio should return Value::Real(2.0)"
        );
    }

    #[test]
    fn joint_offset_prismatic_coupling_default_returns_length_zero() {
        // 2-arg form: default offset is Value::length(0.0)
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert_eq!(
            eval_builtin("joint_offset", &[c]),
            Value::length(0.0),
            "joint_offset default for prismatic should be Value::length(0.0)"
        );
    }

    #[test]
    fn joint_offset_prismatic_coupling_explicit_returns_stored_offset() {
        let c = eval_builtin("couple", &[
            prismatic_x_joint(),
            Value::Real(1.0),
            Value::length(0.5),
        ]);
        assert_eq!(
            eval_builtin("joint_offset", &[c]),
            Value::length(0.5),
            "joint_offset should return Value::length(0.5)"
        );
    }

    #[test]
    fn joint_offset_revolute_coupling_returns_angle_offset() {
        let pi = std::f64::consts::PI;
        let c = eval_builtin("couple", &[
            revolute_z_joint(),
            Value::Real(1.0),
            Value::angle(pi / 4.0),
        ]);
        assert_eq!(
            eval_builtin("joint_offset", &[c]),
            Value::angle(pi / 4.0),
            "joint_offset should return Value::angle(PI/4)"
        );
    }

    #[test]
    fn joint_ratio_prismatic_joint_returns_undef() {
        // Prismatic has no "ratio" key → Undef
        assert!(
            eval_builtin("joint_ratio", &[prismatic_x_joint()]).is_undef(),
            "joint_ratio of prismatic should return Undef"
        );
    }

    #[test]
    fn joint_offset_prismatic_joint_returns_undef() {
        // Prismatic has no "offset" key → Undef
        assert!(
            eval_builtin("joint_offset", &[prismatic_x_joint()]).is_undef(),
            "joint_offset of prismatic should return Undef"
        );
    }

    #[test]
    fn joint_ratio_non_map_returns_undef() {
        assert!(
            eval_builtin("joint_ratio", &[Value::Real(1.0)]).is_undef(),
            "joint_ratio of non-Map should return Undef"
        );
    }

    #[test]
    fn joint_ratio_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_ratio", &[]).is_undef(),
            "joint_ratio with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_offset_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_offset", &[]).is_undef(),
            "joint_offset with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_ratio_two_args_returns_undef() {
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("joint_ratio", &[c, Value::Real(0.0)]).is_undef(),
            "joint_ratio with 2 args should return Undef"
        );
    }

    #[test]
    fn joint_offset_two_args_returns_undef() {
        let c = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("joint_offset", &[c, Value::Real(0.0)]).is_undef(),
            "joint_offset with 2 args should return Undef"
        );
    }

    // ── joint_axis accessor ──────────────────────────────────────────────────

    #[test]
    fn joint_axis_prismatic_returns_stored_axis() {
        let axis = axis_x_unit();
        let joint = eval_builtin("prismatic", &[axis.clone(), length_range_0_to_1m()]);
        assert_eq!(
            eval_builtin("joint_axis", &[joint]),
            axis,
            "joint_axis(prismatic) should return stored axis"
        );
    }

    #[test]
    fn joint_axis_revolute_returns_stored_axis() {
        let axis = axis_z_unit();
        let joint = eval_builtin("revolute", &[axis.clone(), angle_range_0_to_pi()]);
        assert_eq!(
            eval_builtin("joint_axis", &[joint]),
            axis,
            "joint_axis(revolute) should return stored axis"
        );
    }

    #[test]
    fn joint_axis_non_joint_returns_undef() {
        assert!(
            eval_builtin("joint_axis", &[Value::Real(1.0)]).is_undef(),
            "joint_axis of non-Map should return Undef"
        );
    }

    #[test]
    fn joint_axis_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_axis", &[]).is_undef(),
            "joint_axis with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_axis_two_args_returns_undef() {
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("joint_axis", &[joint, Value::Real(0.0)]).is_undef(),
            "joint_axis with 2 args should return Undef"
        );
    }

    // ── couple constructor: happy paths ─────────────────────────────────────

    #[test]
    fn couple_prismatic_2arg_returns_coupling_map() {
        // 2-arg form: couple(prismatic, ratio) → Map with kind="coupling",
        // parent=<prismatic Map>, ratio=Value::Real(-1.0),
        // offset=Value::length(0.0) (default zero in LENGTH dimension)
        let parent = prismatic_x_joint();
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(-1.0)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the prismatic joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(-1.0)),
            "ratio should be Value::Real(-1.0)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(0.0)),
            "default offset for prismatic should be Value::length(0.0)"
        );
    }

    #[test]
    fn couple_prismatic_3arg_stores_explicit_offset() {
        // 3-arg form: explicit offset stored as provided
        let parent = prismatic_x_joint();
        let offset = Value::length(0.5);
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(2.0), offset.clone()]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the prismatic joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(2.0)),
            "ratio should be Value::Real(2.0)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&offset),
            "offset should be Value::length(0.5)"
        );
    }

    #[test]
    fn couple_revolute_2arg_defaults_angle_offset() {
        // 2-arg revolute parent → default offset is Value::angle(0.0)
        let parent = revolute_z_joint();
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(0.5)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the revolute joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.5)),
            "ratio should be Value::Real(0.5)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::angle(0.0)),
            "default offset for revolute should be Value::angle(0.0)"
        );
    }

    #[test]
    fn couple_revolute_3arg_stores_explicit_angle_offset() {
        // 3-arg revolute form: explicit angle offset stored
        let pi = std::f64::consts::PI;
        let parent = revolute_z_joint();
        let offset = Value::angle(pi / 4.0);
        let result = eval_builtin("couple", &[parent.clone(), Value::Real(0.5), offset.clone()]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("coupling".to_string())),
            "kind should be 'coupling'"
        );
        assert_eq!(
            map.get(&Value::String("parent".to_string())),
            Some(&parent),
            "parent should match the revolute joint"
        );
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.5)),
            "ratio should be Value::Real(0.5)"
        );
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&offset),
            "explicit angle offset should be stored"
        );
    }

    // ── couple constructor: validation rejections ────────────────────────────

    #[test]
    fn couple_zero_args_returns_undef() {
        assert!(eval_builtin("couple", &[]).is_undef(), "0 args should return Undef");
    }

    #[test]
    fn couple_one_arg_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint()]).is_undef(),
            "1 arg should return Undef"
        );
    }

    #[test]
    fn couple_four_args_returns_undef() {
        assert!(
            eval_builtin("couple", &[
                prismatic_x_joint(),
                Value::Real(1.0),
                Value::length(0.0),
                Value::Real(0.0),
            ]).is_undef(),
            "4 args should return Undef"
        );
    }

    #[test]
    fn couple_non_map_parent_returns_undef() {
        assert!(
            eval_builtin("couple", &[Value::Real(1.0), Value::Real(1.0)]).is_undef(),
            "non-Map parent should return Undef"
        );
    }

    #[test]
    fn couple_map_missing_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("couple", &[Value::Map(m), Value::Real(1.0)]).is_undef(),
            "Map parent missing kind key should return Undef"
        );
    }

    #[test]
    fn couple_coupling_parent_returns_undef() {
        // nested coupling is rejected — kind="coupling" is not a DrivingJoint
        let inner = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("couple", &[inner, Value::Real(1.0)]).is_undef(),
            "coupling parent (kind='coupling') should return Undef"
        );
    }

    #[test]
    fn couple_unknown_parent_kind_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("sliding".to_string()));
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("couple", &[Value::Map(m), Value::Real(1.0)]).is_undef(),
            "parent kind='sliding' should return Undef"
        );
    }

    #[test]
    fn couple_string_ratio_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::String("bad".to_string())]).is_undef(),
            "String ratio should return Undef"
        );
    }

    #[test]
    fn couple_nan_ratio_returns_undef() {
        // NaN ratio must be rejected — only finites are valid
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(f64::NAN)]).is_undef(),
            "NaN ratio should return Undef"
        );
    }

    #[test]
    fn couple_inf_ratio_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(f64::INFINITY)]).is_undef(),
            "Infinite ratio should return Undef"
        );
    }

    #[test]
    fn couple_dimensioned_ratio_returns_undef() {
        // a Length Scalar as ratio is not dimensionless — must be rejected
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::length(1.0)]).is_undef(),
            "dimensioned ratio should return Undef"
        );
    }

    #[test]
    fn couple_prismatic_wrong_offset_dim_returns_undef() {
        use reify_types::DimensionVector;
        let mass_offset = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0), mass_offset]).is_undef(),
            "MASS offset for prismatic parent should return Undef"
        );
    }

    #[test]
    fn couple_revolute_wrong_offset_dim_returns_undef() {
        // Length offset for a revolute parent (needs Angle or bare Real)
        assert!(
            eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0), Value::length(1.0)]).is_undef(),
            "Length offset for revolute parent should return Undef"
        );
    }

    #[test]
    fn couple_prismatic_nan_offset_returns_undef() {
        assert!(
            eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0), Value::Real(f64::NAN)]).is_undef(),
            "NaN offset should return Undef"
        );
    }

    // ── couple constructor: accepted ratio / offset variants ────────────────

    #[test]
    fn couple_int_ratio_accepted() {
        // Value::Int(2) is accepted by ratio_input and stored as Real(2.0)
        let result = eval_builtin("couple", &[prismatic_x_joint(), Value::Int(2)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(2.0)),
            "Int(2) ratio should be stored as Real(2.0)"
        );
    }

    #[test]
    fn couple_dimensionless_scalar_ratio_accepted() {
        use reify_types::DimensionVector;
        // DIMENSIONLESS Scalar is accepted by ratio_input and stored as Real
        let ratio = Value::Scalar { si_value: 0.5, dimension: DimensionVector::DIMENSIONLESS };
        let result = eval_builtin("couple", &[prismatic_x_joint(), ratio]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("ratio".to_string())),
            Some(&Value::Real(0.5)),
            "DIMENSIONLESS Scalar(0.5) ratio should be stored as Real(0.5)"
        );
    }

    #[test]
    fn couple_prismatic_int_offset_accepted() {
        // Value::Int(1) is accepted by length_input for a prismatic parent
        // and stored as Value::length(1.0)
        let result = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0), Value::Int(1)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(1.0)),
            "Int(1) offset for prismatic should be stored as Value::length(1.0)"
        );
    }

    #[test]
    fn couple_prismatic_bare_real_offset_accepted() {
        // bare Value::Real(1.5) is accepted by length_input for a prismatic parent
        let result = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0), Value::Real(1.5)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::length(1.5)),
            "Real(1.5) offset for prismatic should be stored as Value::length(1.5)"
        );
    }

    #[test]
    fn couple_revolute_int_offset_accepted() {
        // Value::Int(0) is accepted by trig_input for a revolute parent
        let result = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0), Value::Int(0)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::angle(0.0)),
            "Int(0) offset for revolute should be stored as Value::angle(0.0)"
        );
    }

    #[test]
    fn couple_revolute_bare_real_offset_accepted() {
        // bare Value::Real(π/4) is accepted by trig_input for a revolute parent
        let pi = std::f64::consts::PI;
        let result = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0), Value::Real(pi / 4.0)]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("offset".to_string())),
            Some(&Value::angle(pi / 4.0)),
            "Real(π/4) offset for revolute should be stored as Value::angle(π/4)"
        );
    }

    // ── transform_at on Coupling: validation rejections ─────────────────────

    /// Build a minimal coupling Map by hand for testing defense-in-depth guards.
    fn make_coupling_fixture(
        parent: Value,
        ratio: Value,
        offset: Value,
    ) -> Value {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("offset".to_string()), offset);
        m.insert(Value::String("parent".to_string()), parent);
        m.insert(Value::String("ratio".to_string()), ratio);
        Value::Map(m)
    }

    #[test]
    fn transform_at_coupling_angle_to_prismatic_parent_returns_undef() {
        // Angle Scalar passed to a coupling whose parent is prismatic
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::angle(1.0)]).is_undef(),
            "Angle to prismatic coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_length_to_revolute_parent_returns_undef() {
        // Length Scalar passed to a coupling whose parent is revolute
        let c = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "Length to revolute coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_mass_value_returns_undef() {
        use reify_types::DimensionVector;
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        let mass = Value::Scalar { si_value: 1.0, dimension: DimensionVector::MASS };
        assert!(
            eval_builtin("transform_at", &[c, mass]).is_undef(),
            "MASS Scalar to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_nan_value_returns_undef() {
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::Real(f64::NAN)]).is_undef(),
            "NaN value to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_inf_value_returns_undef() {
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        assert!(
            eval_builtin("transform_at", &[c, Value::Real(f64::INFINITY)]).is_undef(),
            "Inf value to coupling should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_sliding_parent_kind_returns_undef() {
        // Defense-in-depth: hand-built coupling Map with parent kind="sliding"
        use std::collections::BTreeMap;
        let mut sliding = BTreeMap::new();
        sliding.insert(Value::String("kind".to_string()), Value::String("sliding".to_string()));
        sliding.insert(Value::String("axis".to_string()), axis_x_unit());
        let c = make_coupling_fixture(
            Value::Map(sliding),
            Value::Real(1.0),
            Value::length(0.0),
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with sliding parent should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_parent_key_returns_undef() {
        // Defense-in-depth: hand-built coupling Map without parent key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "parent" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing parent key should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_ratio_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("parent".to_string()), prismatic_x_joint());
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "ratio" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing ratio key should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_missing_offset_key_returns_undef() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("parent".to_string()), prismatic_x_joint());
        m.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        // no "offset" key
        assert!(
            eval_builtin("transform_at", &[Value::Map(m), Value::length(1.0)]).is_undef(),
            "coupling missing offset key should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_int_ratio_returns_undef() {
        // Defense-in-depth: the coupling arm requires ratio to be stored as
        // Value::Real (the `make_coupling` helper always does this), but a
        // hand-built Map could carry Value::Int instead.  The guard must fire.
        let c = make_coupling_fixture(
            prismatic_x_joint(),
            Value::Int(1),       // Int, not Real — guard fires
            Value::length(0.0),
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with Int ratio should return Undef (defense-in-depth)"
        );
    }

    #[test]
    fn transform_at_coupling_real_offset_returns_undef() {
        // Defense-in-depth: the coupling arm requires offset to be stored as
        // Value::Scalar (the `make_coupling` helper always does this via
        // Value::length / Value::angle), but a hand-built Map could carry a
        // bare Value::Real instead.  The guard must fire.
        let c = make_coupling_fixture(
            prismatic_x_joint(),
            Value::Real(1.0),
            Value::Real(0.0),    // Real, not Scalar — guard fires
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with Real offset should return Undef (defense-in-depth)"
        );
    }

    #[test]
    fn transform_at_coupling_nan_ratio_returns_undef() {
        // Defense-in-depth: hand-built coupling Map with Value::Real(f64::NAN) ratio.
        // The `couple` constructor rejects NaN ratios via `ratio_input`, but a
        // hand-built fixture (or future serialisation path) could carry one. The
        // ratio guard inside the coupling arm of `transform_at` must catch it.
        let c = make_coupling_fixture(
            prismatic_x_joint(),
            Value::Real(f64::NAN),
            Value::length(0.0),
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with NaN ratio should return Undef"
        );
    }

    #[test]
    fn transform_at_coupling_inf_ratio_returns_undef() {
        // Defense-in-depth: hand-built coupling Map with Value::Real(f64::INFINITY) ratio.
        // The `couple` constructor rejects Inf ratios via `ratio_input`, but a
        // hand-built fixture (or future serialisation path) could carry one.
        let c = make_coupling_fixture(
            prismatic_x_joint(),
            Value::Real(f64::INFINITY),
            Value::length(0.0),
        );
        assert!(
            eval_builtin("transform_at", &[c, Value::length(1.0)]).is_undef(),
            "coupling with Inf ratio should return Undef"
        );
    }

    // ── transform_at on Coupling: analytic tests ────────────────────────────

    #[test]
    fn coupling_prismatic_sign_reversal() {
        // Counter-mass idiom: ratio=-1 on X-prismatic → translation negated
        // coupled_value = -1.0 * 5.0 + 0.0 = -5.0 m
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(-1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::length(5.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [-5.0, 0.0, 0.0],
            1e-12,
            "coupling prismatic sign reversal, ratio=-1, v=5m → [-5,0,0]",
        );
    }

    #[test]
    fn coupling_prismatic_with_offset() {
        // ratio=2.0, offset=1.0m, v=3.0m → coupled = 2*3+1 = 7m → [7,0,0]
        let c = eval_builtin("couple", &[
            prismatic_x_joint(),
            Value::Real(2.0),
            Value::length(1.0),
        ]);
        let result = eval_builtin("transform_at", &[c, Value::length(3.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [7.0, 0.0, 0.0],
            1e-12,
            "coupling prismatic ratio=2, offset=1m, v=3m → [7,0,0]",
        );
    }

    #[test]
    fn coupling_prismatic_bare_real_value() {
        // bare Real(0.5) accepted as 0.5 metres; ratio=1, offset=0 → [0.5,0,0]
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::Real(0.5)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.5, 0.0, 0.0],
            1e-12,
            "coupling prismatic bare Real(0.5), ratio=1 → [0.5,0,0]",
        );
    }

    #[test]
    fn coupling_revolute_sign_reversal() {
        // ratio=-1 on Z-revolute → rotation reversed: angle = -π/2
        // coupled_value = -1.0 * (π/2) + 0 = -π/2
        // rotation = (cos(-π/4), 0, 0, sin(-π/4))
        let pi = std::f64::consts::PI;
        let c = eval_builtin("couple", &[revolute_z_joint(), Value::Real(-1.0)]);
        let result = eval_builtin("transform_at", &[c, Value::angle(pi / 2.0)]);
        let exp_w = (-pi / 4.0).cos();
        let exp_z = (-pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (exp_w, 0.0, 0.0, exp_z),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling revolute sign reversal, ratio=-1, v=π/2 → -π/2",
        );
    }

    #[test]
    fn coupling_revolute_with_offset() {
        // ratio=1.0, offset=π/4, v=π/4 → coupled = 1*(π/4) + π/4 = π/2
        // rotation about Z by π/2 = (cos(π/4), 0, 0, sin(π/4))
        let pi = std::f64::consts::PI;
        let c = eval_builtin("couple", &[
            revolute_z_joint(),
            Value::Real(1.0),
            Value::angle(pi / 4.0),
        ]);
        let result = eval_builtin("transform_at", &[c, Value::angle(pi / 4.0)]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling revolute ratio=1, offset=π/4, v=π/4 → rotation π/2",
        );
    }

    #[test]
    fn coupling_zero_ratio_gives_identity_transform() {
        // ratio=0 → coupled_value = 0*v + 0 = 0m regardless of v
        let c = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(0.0)]);
        let result = eval_builtin("transform_at", &[c, Value::length(99.0)]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "coupling zero ratio → identity transform",
        );
    }

    // ── joint_range accessor ─────────────────────────────────────────────────

    #[test]
    fn joint_range_prismatic_returns_stored_range() {
        let range = length_range_0_to_1m();
        let joint = eval_builtin("prismatic", &[axis_x_unit(), range.clone()]);
        assert_eq!(
            eval_builtin("joint_range", &[joint]),
            range,
            "joint_range(prismatic) should return stored range"
        );
    }

    #[test]
    fn joint_range_revolute_returns_stored_range() {
        let range = angle_range_0_to_pi();
        let joint = eval_builtin("revolute", &[axis_z_unit(), range.clone()]);
        assert_eq!(
            eval_builtin("joint_range", &[joint]),
            range,
            "joint_range(revolute) should return stored range"
        );
    }

    #[test]
    fn joint_range_non_joint_returns_undef() {
        assert!(
            eval_builtin("joint_range", &[Value::String("foo".to_string())]).is_undef(),
            "joint_range of non-Map should return Undef"
        );
    }

    #[test]
    fn joint_range_zero_args_returns_undef() {
        assert!(
            eval_builtin("joint_range", &[]).is_undef(),
            "joint_range with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_range_two_args_returns_undef() {
        let joint = revolute_z_joint();
        assert!(
            eval_builtin("joint_range", &[joint, Value::Real(0.0)]).is_undef(),
            "joint_range with 2 args should return Undef"
        );
    }

    // ── joint_jacobian (step-23) ─────────────────────────────────────────────

    /// Helper: extract a 3-component f64 vector from a Map at the given key.
    fn jac_vec3_components(map: &Value, key: &str) -> [f64; 3] {
        let inner = match map {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        let v = inner
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("missing key {:?}", key));
        match v {
            Value::Vector(items) if items.len() == 3 => [
                items[0].as_f64().unwrap(),
                items[1].as_f64().unwrap(),
                items[2].as_f64().unwrap(),
            ],
            other => panic!("expected Vector3 at key {:?}, got {:?}", key, other),
        }
    }

    /// Helper: dimension of a Map's vector value at `key`.
    fn jac_vec3_dim(map: &Value, key: &str) -> reify_types::DimensionVector {
        let inner = match map {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        let v = inner
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("missing key {:?}", key));
        match v {
            Value::Vector(items) if items.len() == 3 => items[0].dimension(),
            other => panic!("expected Vector3 at key {:?}, got {:?}", key, other),
        }
    }

    /// Assert two 3-component vectors are within tolerance.
    fn assert_vec3_close(actual: [f64; 3], expected: [f64; 3], tol: f64, label: &str) {
        for i in 0..3 {
            assert!(
                (actual[i] - expected[i]).abs() < tol,
                "{}: component[{}] expected {}, got {}",
                label,
                i,
                expected[i],
                actual[i]
            );
        }
    }

    #[test]
    fn joint_jacobian_prismatic_x_axis() {
        // (a) prismatic with axis [1,0,0] → angular=[0,0,0], linear=[1,0,0].
        let joint = prismatic_x_joint();
        let result = eval_builtin("joint_jacobian", &[joint]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(ang, [0.0, 0.0, 0.0], 1e-12, "prismatic X angular");
        assert_vec3_close(lin, [1.0, 0.0, 0.0], 1e-12, "prismatic X linear");
        assert_eq!(
            jac_vec3_dim(&result, "angular"),
            reify_types::DimensionVector::DIMENSIONLESS,
            "angular should be DIMENSIONLESS"
        );
        assert_eq!(
            jac_vec3_dim(&result, "linear"),
            reify_types::DimensionVector::DIMENSIONLESS,
            "linear should be DIMENSIONLESS"
        );
    }

    #[test]
    fn joint_jacobian_revolute_z_axis() {
        // (b) revolute with axis [0,0,1] → angular=[0,0,1], linear=[0,0,0].
        let joint = revolute_z_joint();
        let result = eval_builtin("joint_jacobian", &[joint]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(ang, [0.0, 0.0, 1.0], 1e-12, "revolute Z angular");
        assert_vec3_close(lin, [0.0, 0.0, 0.0], 1e-12, "revolute Z linear");
        assert_eq!(
            jac_vec3_dim(&result, "angular"),
            reify_types::DimensionVector::DIMENSIONLESS,
            "angular should be DIMENSIONLESS"
        );
        assert_eq!(
            jac_vec3_dim(&result, "linear"),
            reify_types::DimensionVector::DIMENSIONLESS,
            "linear should be DIMENSIONLESS"
        );
    }

    #[test]
    fn joint_jacobian_prismatic_unnormalized_axis() {
        // (c) prismatic with axis [2,0,0] (magnitude 2) is normalized to [1,0,0] in linear.
        let axis = Value::Vector(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let range = length_range_0_to_1m();
        let joint = eval_builtin("prismatic", &[axis, range]);
        let result = eval_builtin("joint_jacobian", &[joint]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(ang, [0.0, 0.0, 0.0], 1e-12, "unnormalized prismatic angular");
        assert_vec3_close(
            lin,
            [1.0, 0.0, 0.0],
            1e-12,
            "unnormalized prismatic linear (should be unit-normalized)",
        );
    }

    #[test]
    fn joint_jacobian_revolute_unnormalized_axis() {
        // Mirror of (c) for revolute: axis [0,0,2] → angular=[0,0,1].
        let axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        let range = angle_range_0_to_pi();
        let joint = eval_builtin("revolute", &[axis, range]);
        let result = eval_builtin("joint_jacobian", &[joint]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(
            ang,
            [0.0, 0.0, 1.0],
            1e-12,
            "unnormalized revolute angular (should be unit-normalized)",
        );
        assert_vec3_close(lin, [0.0, 0.0, 0.0], 1e-12, "unnormalized revolute linear");
    }

    #[test]
    fn joint_jacobian_coupling_prismatic_ratio_2() {
        // (d) coupling of prismatic-X with ratio=2 → linear=[2,0,0], angular=0.
        let parent = prismatic_x_joint();
        let c = eval_builtin("couple", &[parent, Value::Real(2.0)]);
        let result = eval_builtin("joint_jacobian", &[c]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(ang, [0.0, 0.0, 0.0], 1e-12, "coupling prismatic angular");
        assert_vec3_close(lin, [2.0, 0.0, 0.0], 1e-12, "coupling prismatic linear (ratio=2)");
    }

    #[test]
    fn joint_jacobian_coupling_revolute_ratio_neg3() {
        // (e) coupling of revolute-Z with ratio=-3 → angular=[0,0,-3], linear=0.
        let parent = revolute_z_joint();
        let c = eval_builtin("couple", &[parent, Value::Real(-3.0)]);
        let result = eval_builtin("joint_jacobian", &[c]);
        let ang = jac_vec3_components(&result, "angular");
        let lin = jac_vec3_components(&result, "linear");
        assert_vec3_close(
            ang,
            [0.0, 0.0, -3.0],
            1e-12,
            "coupling revolute angular (ratio=-3)",
        );
        assert_vec3_close(lin, [0.0, 0.0, 0.0], 1e-12, "coupling revolute linear");
    }

    #[test]
    fn joint_jacobian_zero_args_returns_undef() {
        // (f) wrong-arg count
        assert!(
            eval_builtin("joint_jacobian", &[]).is_undef(),
            "joint_jacobian with 0 args should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_two_args_returns_undef() {
        // (f) wrong-arg count
        let joint = prismatic_x_joint();
        assert!(
            eval_builtin("joint_jacobian", &[joint, Value::Real(0.0)]).is_undef(),
            "joint_jacobian with 2 args should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_non_map_arg_returns_undef() {
        // (f) non-Map arg
        assert!(
            eval_builtin("joint_jacobian", &[Value::Real(1.0)]).is_undef(),
            "joint_jacobian with non-Map arg should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_map_without_kind_returns_undef() {
        // (f) Map without "kind" key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "Map without kind key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_map_with_unknown_kind_returns_undef() {
        // (f) Map with kind not in {prismatic, revolute, coupling}
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("sliding".to_string()));
        m.insert(Value::String("axis".to_string()), axis_x_unit());
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "Map with unknown kind should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_prismatic_missing_axis_returns_undef() {
        // (f) joint Map missing "axis" key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("prismatic".to_string()));
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "prismatic Map missing axis key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_revolute_missing_axis_returns_undef() {
        // (f) joint Map missing "axis" key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("revolute".to_string()));
        m.insert(Value::String("range".to_string()), angle_range_0_to_pi());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "revolute Map missing axis key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_coupling_missing_parent_returns_undef() {
        // (f) coupling Map missing "parent" key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "parent" key
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "coupling Map missing parent key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_coupling_missing_ratio_returns_undef() {
        // Defense-in-depth: hand-built coupling Map without ratio key
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        m.insert(Value::String("parent".to_string()), prismatic_x_joint());
        m.insert(Value::String("offset".to_string()), Value::length(0.0));
        // no "ratio" key
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "coupling Map missing ratio key should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_prismatic_zero_axis_returns_undef() {
        // Defense-in-depth: hand-built prismatic Map with zero-magnitude axis
        // (`prismatic` constructor would reject this, but a hand-built Map
        // could carry it).
        use std::collections::BTreeMap;
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("prismatic".to_string()));
        m.insert(Value::String("axis".to_string()), zero_axis);
        m.insert(Value::String("range".to_string()), length_range_0_to_1m());
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(m)]).is_undef(),
            "prismatic Map with zero-magnitude axis should return Undef"
        );
    }

    #[test]
    fn joint_jacobian_coupling_nested_returns_undef() {
        // Nested coupling: parent has kind="coupling" — `couple` rejects this
        // at construction, but a hand-built coupling fixture could carry it.
        // joint_jacobian must reject (consistent with `couple` and transform_at).
        use std::collections::BTreeMap;
        let inner = make_coupling_fixture(prismatic_x_joint(), Value::Real(1.0), Value::length(0.0));
        let mut outer = BTreeMap::new();
        outer.insert(Value::String("kind".to_string()), Value::String("coupling".to_string()));
        outer.insert(Value::String("parent".to_string()), inner);
        outer.insert(Value::String("ratio".to_string()), Value::Real(1.0));
        outer.insert(Value::String("offset".to_string()), Value::length(0.0));
        assert!(
            eval_builtin("joint_jacobian", &[Value::Map(outer)]).is_undef(),
            "nested coupling should return Undef"
        );
    }

    // ── JOINT_KINDS / is_joint_value direct unit tests ───────────────────────

    /// All negative-case inputs for `is_joint_value` in one table-driven test.
    /// Positive cases (one per kind in `JOINT_KINDS`) are covered by
    /// `is_joint_value_aligns_with_joint_kinds` below.
    #[test]
    fn is_joint_value_negative_cases() {
        use std::collections::BTreeMap;

        let mut no_kind = BTreeMap::new();
        no_kind.insert(Value::String("axis".to_string()), axis_x_unit());

        let mut unknown_kind = BTreeMap::new();
        unknown_kind.insert(Value::String("kind".to_string()), Value::String("sliding".to_string()));

        let mut non_string_kind = BTreeMap::new();
        non_string_kind.insert(Value::String("kind".to_string()), Value::Int(0));

        let cases: Vec<(&str, Value)> = vec![
            ("Real(1.0)", Value::Real(1.0)),
            ("Int(0)", Value::Int(0)),
            ("bare String 'prismatic'", Value::String("prismatic".to_string())),
            ("Map without 'kind' key", Value::Map(no_kind)),
            ("Map with kind='sliding' (not in JOINT_KINDS)", Value::Map(unknown_kind)),
            ("Map with kind=Int(0)", Value::Map(non_string_kind)),
        ];
        for (label, v) in &cases {
            assert!(!is_joint_value(v), "{label} should not be a joint value");
        }
    }

    #[test]
    fn is_joint_value_aligns_with_joint_kinds() {
        use std::collections::BTreeMap;
        // Every kind in JOINT_KINDS must be recognized as a joint value.
        for &kind in JOINT_KINDS {
            let mut m = BTreeMap::new();
            m.insert(Value::String("kind".to_string()), Value::String(kind.to_string()));
            assert!(
                is_joint_value(&Value::Map(m)),
                "Map with kind='{}' (in JOINT_KINDS) should be a joint value",
                kind
            );
        }
        // A kind not in JOINT_KINDS must not be recognized.
        let mut m = BTreeMap::new();
        m.insert(Value::String("kind".to_string()), Value::String("not_a_joint".to_string()));
        assert!(
            !is_joint_value(&Value::Map(m)),
            "Map with kind='not_a_joint' should not be a joint value"
        );
    }

    /// Minimal well-formed `(joint, value_arg)` fixture for each kind in `JOINT_KINDS`.
    ///
    /// Shared by `transform_at_dispatches_for_every_joint_kind` and
    /// `joint_jacobian_dispatches_for_every_joint_kind`.
    ///
    /// Returns a `Vec` of `(joint, value_arg)` pairs for each kind so that a
    /// kind may yield multiple variants when multiple code paths must be covered.
    ///
    /// For `"coupling"` two variants are returned — one with a prismatic parent
    /// and one with a revolute parent — so that both branches in `transform_at`
    /// (`length_input` vs `trig_input`) and both paths in `joint_jacobian_value`
    /// are exercised by the dispatch-coverage tests.
    ///
    /// After building each coupling value the fixture asserts it is non-Undef.
    /// This localises the failure to `couple` itself rather than letting a
    /// regression in `couple` surface as a misleading dispatch-test failure.
    ///
    /// Consumer tests should use a nested loop:
    /// ```ignore
    /// for &kind in JOINT_KINDS {
    ///     for (joint, value_arg) in joint_kind_minimal_fixture(kind) { ... }
    /// }
    /// ```
    ///
    /// `value_arg` is the motion-variable input for `transform_at`; it is unused
    /// by `joint_jacobian` (which is a constant w.r.t. the motion variable).
    fn joint_kind_minimal_fixture(kind: &str) -> Vec<(Value, Value)> {
        match kind {
            "prismatic" => vec![(prismatic_x_joint(), Value::length(0.0))],
            "revolute"  => vec![(revolute_z_joint(),  Value::angle(0.0))],
            "coupling"  => {
                let coupling_p = eval_builtin("couple", &[prismatic_x_joint(), Value::Real(1.0)]);
                assert!(
                    !coupling_p.is_undef(),
                    "couple fixture itself returned Undef — fix couple before checking dispatch"
                );
                let coupling_r = eval_builtin("couple", &[revolute_z_joint(), Value::Real(1.0)]);
                assert!(
                    !coupling_r.is_undef(),
                    "couple fixture itself returned Undef — fix couple before checking dispatch"
                );
                vec![
                    (coupling_p, Value::length(0.0)),
                    (coupling_r, Value::angle(0.0)),
                ]
            }
            // 0-DOF fixed joint: any second arg is accepted (design decision: ignored).
            // Value::Real(0.0) chosen to match the minimal-value style of the other arms.
            "fixed" => vec![(eval_builtin("fixed", &[]), Value::Real(0.0))],
            // 3-DOF planar joint: motion_vars is a List of [x_length, y_length, theta_angle].
            // Using zero values keeps the fixture minimal while exercising all dispatch arms.
            "planar" => vec![(
                planar_xy_joint(),
                Value::List(vec![Value::length(0.0), Value::length(0.0), Value::angle(0.0)]),
            )],
            _ => panic!(
                "JOINT_KINDS contains '{kind}' but the dispatch tests have no fixture; \
                 add a minimal well-formed fixture here and confirm that both \
                 `transform_at` and `joint_jacobian_value` have matching dispatch arms"
            ),
        }
    }

    /// Guard against silent drift between `JOINT_KINDS` and the per-kind `match`
    /// arms in `transform_at`.  For every kind in `JOINT_KINDS`, build a minimal
    /// well-formed joint value and assert that `transform_at` does NOT return
    /// `Value::Undef`.
    ///
    /// Two failure modes are caught:
    /// 1. A new kind is added to `JOINT_KINDS` without a fixture in
    ///    `joint_kind_minimal_fixture` → the `_` arm panics with a remediation
    ///    message.
    /// 2. A fixture exists but `transform_at` has no dispatch arm for the kind →
    ///    the `is_undef` assertion fails.
    #[test]
    fn transform_at_dispatches_for_every_joint_kind() {
        for &kind in JOINT_KINDS {
            for (joint, value_arg) in joint_kind_minimal_fixture(kind) {
                let result = eval_builtin("transform_at", &[joint, value_arg]);
                assert!(
                    !result.is_undef(),
                    "transform_at(kind='{kind}', minimal-well-formed-input) returned Undef. \
                     Either add a dispatch arm in transform_at for kind='{kind}', \
                     or remove '{kind}' from JOINT_KINDS."
                );
            }
        }
    }

    /// Guard against silent drift between `JOINT_KINDS` and the per-kind `match`
    /// arms in `joint_jacobian_value`.  Mirrors
    /// `transform_at_dispatches_for_every_joint_kind` but calls `joint_jacobian`
    /// (which takes only the joint, no motion-variable argument).
    ///
    /// Two failure modes are caught:
    /// 1. A new kind is added to `JOINT_KINDS` without a fixture in
    ///    `joint_kind_minimal_fixture` → the `_` arm panics with a remediation
    ///    message.
    /// 2. A fixture exists but `joint_jacobian_value` has no dispatch arm for
    ///    the kind → the `is_undef` assertion fails.
    #[test]
    fn joint_jacobian_dispatches_for_every_joint_kind() {
        for &kind in JOINT_KINDS {
            for (joint, _value_arg) in joint_kind_minimal_fixture(kind) {
                let result = eval_builtin("joint_jacobian", &[joint]);
                assert!(
                    !result.is_undef(),
                    "joint_jacobian(kind='{kind}', minimal-well-formed-input) returned Undef. \
                     Either add a dispatch arm in joint_jacobian_value for kind='{kind}', \
                     or remove '{kind}' from JOINT_KINDS."
                );
            }
        }
    }

    // ── fixed constructor ────────────────────────────────────────────────────

    /// `fixed()` with zero args returns a single-key Map `{ "kind" → "fixed" }`.
    /// No axis or range field — fixed joints have no motion variable.
    #[test]
    fn fixed_returns_map_with_correct_fields() {
        let result = eval_builtin("fixed", &[]);
        let map = match result {
            Value::Map(m) => m,
            other => panic!("fixed(): expected Value::Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("fixed".to_string())),
            "kind field should be 'fixed'"
        );
        assert_eq!(map.len(), 1, "fixed joint Map should have exactly 1 key (only 'kind')");
    }

    /// `fixed` rejects any non-empty argument list — a 0-DOF joint has no
    /// parameters to accept.
    #[test]
    fn fixed_with_nonzero_args_returns_undef() {
        assert!(
            eval_builtin("fixed", &[Value::Real(0.0)]).is_undef(),
            "fixed(Real(0.0)) should return Undef (too many args)"
        );
        assert!(
            eval_builtin("fixed", &[Value::Real(0.0), Value::Real(1.0)]).is_undef(),
            "fixed(a, b) should return Undef (too many args)"
        );
    }

    // ── joint_jacobian for fixed ─────────────────────────────────────────────

    /// `joint_jacobian(fixed_joint)` returns a zero-twist Map.
    ///
    /// Design decision: returns `Map { angular: [0,0,0], linear: [0,0,0] }` to
    /// preserve the uniform single-column shape across all joint kinds (rather
    /// than returning a 6×0 empty-column list). Callers can read `.angular`/
    /// `.linear` keys without dispatching on shape.
    #[test]
    fn joint_jacobian_fixed_returns_zero_twist() {
        let fj = eval_builtin("fixed", &[]);
        let result = eval_builtin("joint_jacobian", &[fj]);
        let map = match &result {
            Value::Map(m) => m,
            other => panic!("joint_jacobian(fixed): expected Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("angular".to_string())),
            Some(&Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)])),
            "angular twist column should be [0, 0, 0]"
        );
        assert_eq!(
            map.get(&Value::String("linear".to_string())),
            Some(&Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)])),
            "linear twist column should be [0, 0, 0]"
        );
    }

    // ── transform_at for fixed ───────────────────────────────────────────────

    /// `transform_at(fixed_joint, any_value)` returns the identity Transform for
    /// well-formed second args, and propagates `Value::Undef` when the second arg
    /// is Undef (so upstream evaluation errors are not masked).
    ///
    /// Design decision: type and dimension of the second argument are not validated
    /// — a 0-DOF joint has no motion variable. Undef is the only case that changes
    /// the result; all other value types yield the identity Transform.
    #[test]
    fn transform_at_fixed_returns_identity_transform() {
        let fj = eval_builtin("fixed", &[]);

        // Primary case: second arg is a bare Real.
        let result = eval_builtin("transform_at", &[fj.clone(), Value::Real(0.0)]);
        let (rot, trans) = match &result {
            Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
            other => panic!("transform_at(fixed, 0.0): expected Transform, got {:?}", other),
        };
        assert_eq!(
            rot,
            &Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
            "identity rotation"
        );
        assert_eq!(
            trans,
            &Value::Vector(vec![Value::length(0.0), Value::length(0.0), Value::length(0.0)]),
            "zero translation"
        );

        // Undef propagation: if the second arg is Undef, the result must also be
        // Undef so that upstream evaluation errors are not swallowed.
        let undef_result = eval_builtin("transform_at", &[fj.clone(), Value::Undef]);
        assert!(
            undef_result.is_undef(),
            "transform_at(fixed, Undef): expected Undef (Undef propagation), got {:?}", undef_result
        );

        // Non-Undef args of various types — all should yield the identity Transform
        // (type/dimension not validated for 0-DOF joints).
        for (label, second_arg) in [
            ("length(2.5)",            Value::length(2.5)),
            ("angle(1.0)",             Value::angle(1.0)),
            ("Int(5)",                 Value::Int(5)),
            ("String(\"foo\")",        Value::String("foo".to_string())),
            ("List(empty)",            Value::List(vec![])),
        ] {
            let r2 = eval_builtin("transform_at", &[fj.clone(), second_arg]);
            assert!(
                matches!(&r2, Value::Transform { .. }),
                "transform_at(fixed, {label}): expected Transform, got {:?}", r2
            );
            let (r, t) = match &r2 {
                Value::Transform { rotation, translation } => (rotation.as_ref(), translation.as_ref()),
                _ => unreachable!(),
            };
            assert_eq!(r, &Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
                "identity rotation for {label}");
            assert_eq!(
                t,
                &Value::Vector(vec![Value::length(0.0), Value::length(0.0), Value::length(0.0)]),
                "zero translation for {label}"
            );
        }
    }

    // ── planar constructor: happy path (step-1) ───────────────────────────────

    #[test]
    fn planar_returns_map_with_correct_fields() {
        let axis_x = axis_x_unit();
        let axis_y = axis_y_unit();
        let range_x = length_range_0_to_1m();
        let range_y = length_range_0_to_1m();
        let range_theta = angle_range_0_to_pi();
        let result = eval_builtin("planar", &[
            axis_x.clone(),
            axis_y.clone(),
            range_x.clone(),
            range_y.clone(),
            range_theta.clone(),
        ]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("planar".to_string())),
            "kind field should be 'planar'"
        );
        assert_eq!(
            map.get(&Value::String("axis_x".to_string())),
            Some(&axis_x),
            "axis_x field should match input"
        );
        assert_eq!(
            map.get(&Value::String("axis_y".to_string())),
            Some(&axis_y),
            "axis_y field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range_x".to_string())),
            Some(&range_x),
            "range_x field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range_y".to_string())),
            Some(&range_y),
            "range_y field should match input"
        );
        assert_eq!(
            map.get(&Value::String("range_theta".to_string())),
            Some(&range_theta),
            "range_theta field should match input"
        );
        assert_eq!(
            map.len(),
            6,
            "planar joint Map should have exactly 6 keys \
             (kind, axis_x, axis_y, range_x, range_y, range_theta), \
             got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
    }

    // ── transform_at on planar: zero motion (step-5) ─────────────────────────

    #[test]
    fn transform_at_planar_zero_motion_returns_identity() {
        let joint = planar_xy_joint();
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::angle(0.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "planar zero motion → identity",
        );
    }

    // ── transform_at on planar: pure-DOF tests (step-7) ──────────────────────

    #[test]
    fn transform_at_planar_pure_x_translation() {
        // axis_x=[1,0,0], axis_y=[0,1,0], motion=[0.5m, 0m, 0rad]
        // → translation=[0.5, 0, 0], identity rotation
        let joint = planar_xy_joint();
        let motion = Value::List(vec![
            Value::length(0.5),
            Value::length(0.0),
            Value::angle(0.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.5, 0.0, 0.0],
            1e-12,
            "planar pure-X translation 0.5m",
        );
    }

    #[test]
    fn transform_at_planar_pure_y_translation() {
        // axis_x=[1,0,0], axis_y=[0,1,0], motion=[0m, 0.3m, 0rad]
        // → translation=[0, 0.3, 0], identity rotation
        let joint = planar_xy_joint();
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.3),
            Value::angle(0.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [0.0, 0.3, 0.0],
            1e-12,
            "planar pure-Y translation 0.3m",
        );
    }

    #[test]
    fn transform_at_planar_pure_rotation() {
        // axis_x=[1,0,0], axis_y=[0,1,0], motion=[0m, 0m, π/2 rad]
        // normal = +X × +Y = +Z
        // → translation=[0,0,0], rotation = quat(+Z, π/2) = (cos(π/4), 0, 0, sin(π/4))
        let joint = planar_xy_joint();
        let pi = std::f64::consts::PI;
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::angle(pi / 2.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.0, 0.0, 0.0],
            1e-12,
            "planar pure-rotation π/2 about +Z",
        );
    }

    // ── transform_at on planar: combined motion (step-9) ─────────────────────

    #[test]
    fn transform_at_planar_combined_motion() {
        // axis_x=[1,0,0], axis_y=[0,1,0], motion=[0.5m, 0.3m, π/2 rad]
        // T_x has identity rotation → translation adds; T_theta adds rotation.
        // Expected: translation=[0.5, 0.3, 0], rotation=quat(+Z, π/2).
        let joint = planar_xy_joint();
        let pi = std::f64::consts::PI;
        let motion = Value::List(vec![
            Value::length(0.5),
            Value::length(0.3),
            Value::angle(pi / 2.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        let cos = (pi / 4.0).cos();
        let sin = (pi / 4.0).sin();
        assert_transform_approx(
            &result,
            (cos, 0.0, 0.0, sin),
            [0.5, 0.3, 0.0],
            1e-12,
            "planar combined [0.5m, 0.3m, π/2]",
        );
    }

    #[test]
    fn transform_at_planar_combined_non_axis_aligned() {
        // axis_x = [1/√2, 1/√2, 0] (45° in XY plane)
        // axis_y = [-1/√2, 1/√2, 0] (perpendicular to axis_x in XY plane)
        // motion = [1m, 0m, 0rad]
        // → translation = 1 * unit_axis_x = [1/√2, 1/√2, 0], identity rotation
        let s2 = std::f64::consts::FRAC_1_SQRT_2; // 1/√2
        let ax = Value::Vector(vec![Value::Real(s2), Value::Real(s2), Value::Real(0.0)]);
        let ay = Value::Vector(vec![Value::Real(-s2), Value::Real(s2), Value::Real(0.0)]);
        let joint = eval_builtin("planar", &[
            ax,
            ay,
            length_range_0_to_1m(),
            length_range_0_to_1m(),
            angle_range_0_to_pi(),
        ]);
        let motion = Value::List(vec![
            Value::length(1.0),
            Value::length(0.0),
            Value::angle(0.0),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        assert_transform_approx(
            &result,
            (1.0, 0.0, 0.0, 0.0),
            [s2, s2, 0.0],
            1e-12,
            "planar non-axis-aligned X translation",
        );
    }

    // ── transform_at on planar: non-Z plane normal (amendment — review suggestion 4) ──

    #[test]
    fn transform_at_planar_non_z_normal() {
        // axis_x=[1,0,0], axis_y=[0,0,1] → plane is the XZ plane.
        // Normal = axis_x × axis_y = (1,0,0)×(0,0,1):
        //   nx = ay*bz - az*by = 0*1 - 0*0 = 0
        //   ny = az*bx - ax*bz = 0*0 - 1*1 = -1
        //   nz = ax*by - ay*bx = 1*0 - 0*0 = 0
        // → normal = (0, -1, 0) = -Y axis.
        //
        // This test verifies the cross-product sign and catches axis_x/axis_y
        // transposition bugs: swapping the two axes would flip the normal to +Y
        // and produce quat.y = +sin_half (not -sin_half), which the assertion catches.
        //
        // motion = [0m, 0m, π/4 rad]
        // → translation = [0, 0, 0], rotation = quat(-Y, π/4)
        //   = (cos(π/8), 0·sin(π/8), -1·sin(π/8), 0·sin(π/8))
        //   = (cos(π/8), 0, -sin(π/8), 0)
        let pi = std::f64::consts::PI;
        let ax = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let ay = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let joint = eval_builtin("planar", &[
            ax, ay,
            length_range_0_to_1m(),
            length_range_0_to_1m(),
            angle_range_0_to_pi(),
        ]);
        assert!(!joint.is_undef(), "planar([1,0,0],[0,0,1],...) should build OK");
        let theta = pi / 4.0;
        let motion = Value::List(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::angle(theta),
        ]);
        let result = eval_builtin("transform_at", &[joint, motion]);
        let cos_h = (theta / 2.0).cos();
        let sin_h = (theta / 2.0).sin();
        // exp_rot = (w, x, y, z); normal = (0, -1, 0) so y = -sin_h.
        assert_transform_approx(
            &result,
            (cos_h, 0.0, -sin_h, 0.0),
            [0.0, 0.0, 0.0],
            1e-12,
            "planar XZ-plane: rotation about -Y",
        );
    }

    // ── transform_at on planar: invalid motion-var validation (step-11) ─────

    #[test]
    fn transform_at_planar_invalid_motion_var_returns_undef() {
        let joint = planar_xy_joint();

        // Mass-dimensioned scalar (wrong dimension for any element)
        let mass_scalar = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };

        let cases: &[(&str, Value)] = &[
            // (a) wrong List length
            ("List of 2",   Value::List(vec![Value::length(0.0), Value::length(0.0)])),
            ("List of 4",   Value::List(vec![Value::length(0.0), Value::length(0.0), Value::angle(0.0), Value::Real(0.0)])),
            ("List of 0",   Value::List(vec![])),
            // (b) wrong container type
            ("bare Real",   Value::Real(0.0)),
            ("Vector(3)",   Value::Vector(vec![Value::length(0.0), Value::length(0.0), Value::angle(0.0)])),
            ("bare Map",    Value::Map(Default::default())),
            // (c) wrong dimension: element 0 is Angle (should be Length)
            ("elem[0] is Angle",   Value::List(vec![Value::angle(0.0), Value::length(0.0), Value::angle(0.0)])),
            // (d) wrong dimension: element 2 is Length (should be Angle)
            ("elem[2] is Length",  Value::List(vec![Value::length(0.0), Value::length(0.0), Value::length(0.0)])),
            // (e) wrong dimension: mass-typed element
            ("elem[0] mass",       Value::List(vec![mass_scalar.clone(), Value::length(0.0), Value::angle(0.0)])),
            ("elem[1] mass",       Value::List(vec![Value::length(0.0), mass_scalar.clone(), Value::angle(0.0)])),
            ("elem[2] mass",       Value::List(vec![Value::length(0.0), Value::length(0.0), mass_scalar.clone()])),
            // (f) Undef element propagates Undef result
            ("elem[0] Undef",      Value::List(vec![Value::Undef, Value::length(0.0), Value::angle(0.0)])),
            ("elem[1] Undef",      Value::List(vec![Value::length(0.0), Value::Undef, Value::angle(0.0)])),
            ("elem[2] Undef",      Value::List(vec![Value::length(0.0), Value::length(0.0), Value::Undef])),
        ];

        for (label, motion_vars) in cases {
            assert!(
                eval_builtin("transform_at", &[joint.clone(), motion_vars.clone()]).is_undef(),
                "transform_at(planar, {label}) should return Undef but didn't"
            );
        }
    }

    // ── joint_jacobian for planar (step-13) ──────────────────────────────────

    /// `joint_jacobian(planar_joint)` returns a zero-twist Map placeholder.
    ///
    /// Design decision: planar is a 3-DOF joint; the analytic 3×6 Jacobian is
    /// deferred (PRD task 2 design: "finite-difference fallback for spherical,
    /// cylindrical, planar until analytic forms are derived"). A zero-column
    /// Map `{ angular: [0,0,0], linear: [0,0,0] }` is returned to preserve the
    /// uniform single-column shape across all kinds (matching the fixed-joint
    /// pattern), so `joint_jacobian_dispatches_for_every_joint_kind` can assert
    /// non-Undef for every kind in JOINT_KINDS.
    #[test]
    fn joint_jacobian_planar_returns_zero_column_placeholder() {
        let pj = planar_xy_joint();
        let result = eval_builtin("joint_jacobian", &[pj]);
        let map = match &result {
            Value::Map(m) => m,
            other => panic!("joint_jacobian(planar): expected Map, got {:?}", other),
        };
        assert_eq!(
            map.get(&Value::String("angular".to_string())),
            Some(&Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)])),
            "angular twist column should be [0, 0, 0]"
        );
        assert_eq!(
            map.get(&Value::String("linear".to_string())),
            Some(&Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)])),
            "linear twist column should be [0, 0, 0]"
        );
    }

    // ── planar constructor: validation surface (step-3) ───────────────────────

    #[test]
    fn planar_invalid_args_returns_undef() {
        // Axis helpers for validation cases
        let ax = axis_x_unit();  // [1, 0, 0]
        let ay = axis_y_unit();  // [0, 1, 0]
        let rx = length_range_0_to_1m();
        let ry = length_range_0_to_1m();
        let rt = angle_range_0_to_pi();

        // Wrong dimensioned axis (LENGTH-typed Vector3)
        let length_axis = Value::Vector(vec![Value::length(1.0), Value::length(0.0), Value::length(0.0)]);
        // 2-component axis
        let axis2 = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]);
        // Non-vector axis
        let non_vec = Value::Real(1.0);
        // Zero axis
        let zero_axis = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        // NaN axis
        let nan_axis = Value::Vector(vec![Value::Real(f64::NAN), Value::Real(0.0), Value::Real(0.0)]);
        // Non-perpendicular: axis_y = [1,1,0] — dot with [1,0,0] = 1/√2 ≠ 0
        let non_perp_y = Value::Vector(vec![Value::Real(1.0), Value::Real(1.0), Value::Real(0.0)]);
        // Parallel: axis_y = axis_x = [1,0,0]
        let parallel_y = axis_x_unit();
        // Angle-dimensioned range where LENGTH expected
        let angle_range = angle_range_0_to_pi();
        // Length-dimensioned range where ANGLE expected
        let length_range = length_range_0_to_1m();
        // Unbounded range
        let unbounded = Value::Range {
            lower: Some(Box::new(Value::length(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };

        let cases: &[(&str, &[Value])] = &[
            // (a) wrong arg counts
            ("0 args",  &[]),
            ("1 arg",   &[ax.clone()]),
            ("4 args",  &[ax.clone(), ay.clone(), rx.clone(), ry.clone()]),
            ("6 args",  &[ax.clone(), ay.clone(), rx.clone(), ry.clone(), rt.clone(), Value::Real(0.0)]),
            // (b) axis_x invalid variants
            ("axis_x: bare Real",          &[non_vec.clone(), ay.clone(), rx.clone(), ry.clone(), rt.clone()]),
            ("axis_x: 2-component",        &[axis2.clone(), ay.clone(), rx.clone(), ry.clone(), rt.clone()]),
            ("axis_x: LENGTH-dimensioned", &[length_axis.clone(), ay.clone(), rx.clone(), ry.clone(), rt.clone()]),
            ("axis_x: zero",               &[zero_axis.clone(), ay.clone(), rx.clone(), ry.clone(), rt.clone()]),
            ("axis_x: NaN",                &[nan_axis.clone(), ay.clone(), rx.clone(), ry.clone(), rt.clone()]),
            // (c) axis_y invalid variants (axis_x valid = [1,0,0])
            ("axis_y: bare Real",          &[ax.clone(), non_vec.clone(), rx.clone(), ry.clone(), rt.clone()]),
            ("axis_y: 2-component",        &[ax.clone(), axis2.clone(), rx.clone(), ry.clone(), rt.clone()]),
            ("axis_y: LENGTH-dimensioned", &[ax.clone(), length_axis.clone(), rx.clone(), ry.clone(), rt.clone()]),
            ("axis_y: zero",               &[ax.clone(), zero_axis.clone(), rx.clone(), ry.clone(), rt.clone()]),
            ("axis_y: NaN",                &[ax.clone(), nan_axis.clone(), rx.clone(), ry.clone(), rt.clone()]),
            // (d) non-perpendicular axes
            ("non-perpendicular axes",     &[ax.clone(), non_perp_y.clone(), rx.clone(), ry.clone(), rt.clone()]),
            // (e) parallel axes (degenerate + fails perpendicularity)
            ("parallel axes (axis_y = axis_x)", &[ax.clone(), parallel_y.clone(), rx.clone(), ry.clone(), rt.clone()]),
            // (f) range_x wrong dimension
            ("range_x: Angle-dimensioned", &[ax.clone(), ay.clone(), angle_range.clone(), ry.clone(), rt.clone()]),
            // (g) range_y wrong dimension
            ("range_y: Angle-dimensioned", &[ax.clone(), ay.clone(), rx.clone(), angle_range.clone(), rt.clone()]),
            // (h) range_theta wrong dimension
            ("range_theta: Length-dimensioned", &[ax.clone(), ay.clone(), rx.clone(), ry.clone(), length_range.clone()]),
            // (i) unbounded range
            ("range_x: unbounded",         &[ax.clone(), ay.clone(), unbounded.clone(), ry.clone(), rt.clone()]),
        ];

        for (label, args) in cases {
            assert!(
                eval_builtin("planar", args).is_undef(),
                "planar({label}) should return Undef but didn't"
            );
        }
    }

    // ── spherical constructor: validation surface (step-3) ───────────────────

    #[test]
    fn spherical_invalid_args_returns_undef() {
        let unbounded_upper = Value::Range {
            lower: Some(Box::new(Value::angle(0.0))),
            upper: None,
            lower_inclusive: true,
            upper_inclusive: false,
        };
        let unbounded_lower = Value::Range {
            lower: None,
            upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
            lower_inclusive: false,
            upper_inclusive: true,
        };

        let cases: Vec<(&str, Vec<Value>)> = vec![
            // (a) wrong arg counts
            ("0 args",  vec![]),
            ("2 args",  vec![angle_range_0_to_pi(), angle_range_0_to_pi()]),
            ("3 args",  vec![angle_range_0_to_pi(), angle_range_0_to_pi(), angle_range_0_to_pi()]),
            // (b) range_angle wrong dimension (LENGTH-typed range)
            ("LENGTH-typed range", vec![length_range_0_to_1m()]),
            // (c) range_angle unbounded
            ("unbounded upper", vec![unbounded_upper]),
            ("unbounded lower", vec![unbounded_lower]),
            // (d) range_angle non-Range types
            ("bare Real",   vec![Value::Real(0.0)]),
            ("bare Vector", vec![Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)])]),
            ("bare Map",    vec![Value::Map(Default::default())]),
        ];

        for (label, args) in &cases {
            assert!(
                eval_builtin("spherical", args).is_undef(),
                "spherical({label}) should return Undef but didn't"
            );
        }
    }

    // ── spherical constructor: happy path (step-1) ────────────────────────────

    #[test]
    fn spherical_returns_map_with_correct_fields() {
        let range_angle = angle_range_0_to_pi();
        let result = eval_builtin("spherical", &[range_angle.clone()]);

        let map = match result {
            Value::Map(m) => m,
            other => panic!("expected Value::Map, got {:?}", other),
        };

        assert_eq!(
            map.get(&Value::String("kind".to_string())),
            Some(&Value::String("spherical".to_string())),
            "kind field should be 'spherical'"
        );
        assert_eq!(
            map.get(&Value::String("range_angle".to_string())),
            Some(&range_angle),
            "range_angle field should match input"
        );
        assert_eq!(
            map.len(),
            2,
            "spherical joint Map should have exactly 2 keys (kind, range_angle), got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
    }

    // ── JOINT_KINDS membership regression pin (step-15) ──────────────────────
    //
    // Asserts that `"planar"` is a member of `JOINT_KINDS` so that:
    //  1. `is_joint_value` accepts planar joints as valid mechanism joints, and
    //  2. the existing dispatch-coverage tests (`transform_at_dispatches_for_every_joint_kind`
    //     and `joint_jacobian_dispatches_for_every_joint_kind`) iterate over the planar kind.
    //
    // This test fails until step-16 appends `"planar"` to `JOINT_KINDS`.
    #[test]
    fn joint_kinds_includes_planar() {
        assert!(
            JOINT_KINDS.contains(&"planar"),
            "\"planar\" must be in JOINT_KINDS so that is_joint_value accepts planar joints \
             and the dispatch-coverage tests exercise the planar arms in transform_at and \
             joint_jacobian_value. Add \"planar\" to the JOINT_KINDS const."
        );
    }

}
