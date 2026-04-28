use std::collections::BTreeMap;

use reify_types::{DimensionVector, Value, quaternion_is_finite};

use crate::helpers::tensor_components_f64;

pub(crate) fn eval_geometry(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        // --- Determinacy predicates (stubs) ---
        "determined" => Value::Undef,
        "undetermined" => Value::Undef,
        "constrained" => Value::Undef,
        "partially_determined" => Value::Undef,

        // --- Frame constructors ---
        "frame3_identity" => {
            if args.is_empty() {
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
            } else {
                Value::Undef
            }
        }
        "frame3" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let origin = &args[0];
            let basis = &args[1];
            match origin {
                Value::Point(components) if components.len() == 3 => {}
                _ => return Some(Value::Undef),
            }
            if !matches!(basis, Value::Orientation { .. }) {
                return Some(Value::Undef);
            }
            Value::Frame {
                origin: Box::new(origin.clone()),
                basis: Box::new(basis.clone()),
            }
        }

        // --- Transform constructors ---
        "transform3" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let rotation = &args[0];
            let translation = &args[1];
            if !matches!(rotation, Value::Orientation { .. }) {
                return Some(Value::Undef);
            }
            match translation {
                Value::Vector(components) if components.len() == 3 => {}
                _ => return Some(Value::Undef),
            }
            Value::Transform {
                rotation: Box::new(rotation.clone()),
                translation: Box::new(translation.clone()),
            }
        }
        "transform3_identity" => {
            if args.is_empty() {
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
            } else {
                Value::Undef
            }
        }

        // --- Transform operations ---
        "frame_to_frame" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let (origin_from, basis_from) = match &args[0] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Some(Value::Undef),
            };
            let (origin_to, basis_to) = match &args[1] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Some(Value::Undef),
            };
            let q_from = match basis_from {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            let q_to = match basis_to {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            let (fx, fy, fz, f_dim) = match origin_from {
                Value::Point(comps) if comps.len() == 3 => {
                    match (comps[0].as_f64(), comps[1].as_f64(), comps[2].as_f64()) {
                        (Some(x), Some(y), Some(z)) => {
                            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                                return Some(Value::Undef);
                            }
                            let dim = comps[0].dimension();
                            if comps[1].dimension() != dim || comps[2].dimension() != dim {
                                return Some(Value::Undef);
                            }
                            (x, y, z, dim)
                        }
                        _ => return Some(Value::Undef),
                    }
                }
                _ => return Some(Value::Undef),
            };
            let (tx, ty, tz, t_dim) = match origin_to {
                Value::Point(comps) if comps.len() == 3 => {
                    match (comps[0].as_f64(), comps[1].as_f64(), comps[2].as_f64()) {
                        (Some(x), Some(y), Some(z)) => {
                            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                                return Some(Value::Undef);
                            }
                            let dim = comps[0].dimension();
                            if comps[1].dimension() != dim || comps[2].dimension() != dim {
                                return Some(Value::Undef);
                            }
                            (x, y, z, dim)
                        }
                        _ => return Some(Value::Undef),
                    }
                }
                _ => return Some(Value::Undef),
            };
            // R = R_to * conj(R_from)
            let r = quat_mul(q_to, quat_conj(q_from));
            match normalize_quaternion(r.0, r.1, r.2, r.3) {
                Some(rot_val) => {
                    // t = origin_to - R * origin_from
                    if f_dim != t_dim {
                        return Some(Value::Undef);
                    }
                    let dim = f_dim;
                    let r_norm = match &rot_val {
                        Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                        _ => unreachable!(),
                    };
                    let (rfx, rfy, rfz) = quat_rotate(r_norm, fx, fy, fz);
                    let trans = Value::Vector(vec![
                        Value::Scalar {
                            si_value: tx - rfx,
                            dimension: dim,
                        },
                        Value::Scalar {
                            si_value: ty - rfy,
                            dimension: dim,
                        },
                        Value::Scalar {
                            si_value: tz - rfz,
                            dimension: dim,
                        },
                    ]);
                    Value::Transform {
                        rotation: Box::new(rot_val),
                        translation: Box::new(trans),
                    }
                }
                None => Value::Undef,
            }
        }

        "transform_exp" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let map = match &args[0] {
                Value::Map(m) => m,
                _ => return Some(Value::Undef),
            };
            let angular_val = match map.get(&Value::String("angular".to_string())) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let linear_val = match map.get(&Value::String("linear".to_string())) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // Extract angular: must be Vector3<DIMENSIONLESS>.
            let (ang_comps, ang_dim) = match tensor_components_f64(angular_val) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Some(Value::Undef),
            };
            if ang_dim != DimensionVector::DIMENSIONLESS {
                return Some(Value::Undef);
            }
            let wx = ang_comps[0];
            let wy = ang_comps[1];
            let wz = ang_comps[2];
            if !wx.is_finite() || !wy.is_finite() || !wz.is_finite() {
                return Some(Value::Undef);
            }
            // Extract linear: must be Vector3 (preserve dimension; for now, accept LENGTH or DIMENSIONLESS).
            // Spec: linear must be LENGTH (or DIMENSIONLESS for unit-less twists).
            let (lin_items, _lin_dim_first) = match linear_val {
                Value::Vector(items) if items.len() == 3 => {
                    let dim0 = items[0].dimension();
                    if items[1].dimension() != dim0 || items[2].dimension() != dim0 {
                        return Some(Value::Undef);
                    }
                    (items.clone(), dim0)
                }
                _ => return Some(Value::Undef),
            };
            let lin_dim = lin_items[0].dimension();
            // Permit LENGTH or DIMENSIONLESS (test expects strict LENGTH for typed cases; ANGLE is rejected).
            if lin_dim != DimensionVector::LENGTH && lin_dim != DimensionVector::DIMENSIONLESS {
                return Some(Value::Undef);
            }
            let (lx, ly, lz) = match (
                lin_items[0].as_f64(),
                lin_items[1].as_f64(),
                lin_items[2].as_f64(),
            ) {
                (Some(a), Some(b), Some(c)) if a.is_finite() && b.is_finite() && c.is_finite() => (a, b, c),
                _ => return Some(Value::Undef),
            };
            // Compute R = orient_exp(angular).
            let theta_sq = wx * wx + wy * wy + wz * wz;
            let theta = theta_sq.sqrt();
            const EPS: f64 = 1e-12;
            let r_val = if theta < EPS {
                Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }
            } else {
                let half = theta / 2.0;
                let s = half.sin() / theta;
                match normalize_quaternion(half.cos(), s * wx, s * wy, s * wz) {
                    Some(v) => v,
                    None => return Some(Value::Undef),
                }
            };
            // Compute V * linear, where V is the SE(3) left Jacobian:
            //   V = I + ((1−cos|ω|)/|ω|²) [ω]× + ((|ω|−sin|ω|)/|ω|³) [ω]×²
            // For |ω| ≈ 0, use Taylor: V ≈ I + 0.5*[ω]× + (1/6)*[ω]×² + ...
            let (a_coef, b_coef) = if theta < 1.0e-4 {
                // Taylor series:
                //   (1 − cos|ω|)/|ω|² ≈ 1/2 − |ω|²/24 + |ω|⁴/720 − ...
                //   (|ω| − sin|ω|)/|ω|³ ≈ 1/6 − |ω|²/120 + ...
                (
                    0.5 - theta_sq / 24.0,
                    1.0 / 6.0 - theta_sq / 120.0,
                )
            } else {
                ((1.0 - theta.cos()) / theta_sq, (theta - theta.sin()) / (theta_sq * theta))
            };
            // [ω]× linear = ω × linear.
            let cx = wy * lz - wz * ly;
            let cy = wz * lx - wx * lz;
            let cz = wx * ly - wy * lx;
            // [ω]×² linear = ω × (ω × linear).
            let ccx = wy * cz - wz * cy;
            let ccy = wz * cx - wx * cz;
            let ccz = wx * cy - wy * cx;
            let tx = lx + a_coef * cx + b_coef * ccx;
            let ty = ly + a_coef * cy + b_coef * ccy;
            let tz = lz + a_coef * cz + b_coef * ccz;
            if !tx.is_finite() || !ty.is_finite() || !tz.is_finite() {
                return Some(Value::Undef);
            }
            let dim = lin_dim;
            let make_t = |v: f64| -> Value {
                if dim.is_dimensionless() {
                    Value::Real(v)
                } else {
                    Value::Scalar { si_value: v, dimension: dim }
                }
            };
            Value::Transform {
                rotation: Box::new(r_val),
                translation: Box::new(Value::Vector(vec![make_t(tx), make_t(ty), make_t(tz)])),
            }
        }

        "transform_log" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (r_q, t_items) = match &args[0] {
                Value::Transform { rotation, translation } => {
                    match (rotation.as_ref(), translation.as_ref()) {
                        (Value::Orientation { w, x, y, z }, Value::Vector(items)) => {
                            ((*w, *x, *y, *z), items.clone())
                        }
                        _ => return Some(Value::Undef),
                    }
                }
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(r_q.0, r_q.1, r_q.2, r_q.3) {
                return Some(Value::Undef);
            }
            if t_items.len() != 3 {
                return Some(Value::Undef);
            }
            let t_dim = t_items[0].dimension();
            if t_items[1].dimension() != t_dim || t_items[2].dimension() != t_dim {
                return Some(Value::Undef);
            }
            let (tx, ty, tz) = match (
                t_items[0].as_f64(),
                t_items[1].as_f64(),
                t_items[2].as_f64(),
            ) {
                (Some(a), Some(b), Some(c)) if a.is_finite() && b.is_finite() && c.is_finite() => (a, b, c),
                _ => return Some(Value::Undef),
            };
            // Compute angular = orient_log(R): rotation vector ω.
            let (rw, rx, ry, rz) = r_q;
            // Normalize quaternion first.
            let r_norm_sq = rw * rw + rx * rx + ry * ry + rz * rz;
            if r_norm_sq < f64::EPSILON {
                return Some(Value::Undef);
            }
            let r_norm = r_norm_sq.sqrt();
            let (nw, nx, ny, nz) = (rw / r_norm, rx / r_norm, ry / r_norm, rz / r_norm);
            let v_norm = (nx * nx + ny * ny + nz * nz).sqrt();
            const EPS: f64 = 1e-12;
            let (wx, wy, wz) = if v_norm < EPS {
                // Near-identity → ω ≈ 2*(x,y,z) (Taylor leading order).
                (2.0 * nx, 2.0 * ny, 2.0 * nz)
            } else {
                let angle = 2.0 * v_norm.atan2(nw);
                let scale = angle / v_norm;
                (scale * nx, scale * ny, scale * nz)
            };
            if !wx.is_finite() || !wy.is_finite() || !wz.is_finite() {
                return Some(Value::Undef);
            }
            // Compute V_inv * t where V is the SE(3) left Jacobian.
            // |ω|² is the squared magnitude of the rotation vector.
            let theta_sq = wx * wx + wy * wy + wz * wz;
            let theta = theta_sq.sqrt();
            // Apply V_inv = I − 0.5*[ω]× + α*[ω]×², where
            //   α = 1/|ω|² − cot(|ω|/2)/(2|ω|).
            // For small |ω|, use Taylor: α ≈ 1/12 + |ω|²/720 + ...
            // Use the small-angle Taylor when theta < ~1e-4 to keep FP accurate.
            let alpha = if theta < 1.0e-4 {
                1.0 / 12.0 + theta_sq / 720.0
            } else {
                let half = theta / 2.0;
                let cot_half = half.cos() / half.sin();
                1.0 / theta_sq - cot_half / (2.0 * theta)
            };
            // [ω]× t = ω × t (cross product).
            let cx = wy * tz - wz * ty;
            let cy = wz * tx - wx * tz;
            let cz = wx * ty - wy * tx;
            // [ω]×² t = ω × (ω × t).
            let ccx = wy * cz - wz * cy;
            let ccy = wz * cx - wx * cz;
            let ccz = wx * cy - wy * cx;
            let lx = tx - 0.5 * cx + alpha * ccx;
            let ly = ty - 0.5 * cy + alpha * ccy;
            let lz = tz - 0.5 * cz + alpha * ccz;
            if !lx.is_finite() || !ly.is_finite() || !lz.is_finite() {
                return Some(Value::Undef);
            }
            let dim = t_dim;
            let make_lin = |v: f64| -> Value {
                if dim.is_dimensionless() {
                    Value::Real(v)
                } else {
                    Value::Scalar { si_value: v, dimension: dim }
                }
            };
            let mut m = BTreeMap::new();
            m.insert(
                Value::String("angular".to_string()),
                Value::Vector(vec![
                    Value::Real(wx),
                    Value::Real(wy),
                    Value::Real(wz),
                ]),
            );
            m.insert(
                Value::String("linear".to_string()),
                Value::Vector(vec![make_lin(lx), make_lin(ly), make_lin(lz)]),
            );
            Value::Map(m)
        }

        "transform_inverse" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (r_q, t_items) = match &args[0] {
                Value::Transform { rotation, translation } => {
                    match (rotation.as_ref(), translation.as_ref()) {
                        (Value::Orientation { w, x, y, z }, Value::Vector(items)) => {
                            ((*w, *x, *y, *z), items.clone())
                        }
                        _ => return Some(Value::Undef),
                    }
                }
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(r_q.0, r_q.1, r_q.2, r_q.3) {
                return Some(Value::Undef);
            }
            if t_items.len() != 3 {
                return Some(Value::Undef);
            }
            let t_dim = t_items[0].dimension();
            if t_items[1].dimension() != t_dim || t_items[2].dimension() != t_dim {
                return Some(Value::Undef);
            }
            let (tx, ty, tz) = match (
                t_items[0].as_f64(),
                t_items[1].as_f64(),
                t_items[2].as_f64(),
            ) {
                (Some(a), Some(b), Some(c)) if a.is_finite() && b.is_finite() && c.is_finite() => (a, b, c),
                _ => return Some(Value::Undef),
            };
            // Normalize R first to be safe with non-unit input quaternions.
            let r_norm_sq = r_q.0 * r_q.0 + r_q.1 * r_q.1 + r_q.2 * r_q.2 + r_q.3 * r_q.3;
            if r_norm_sq < f64::EPSILON {
                return Some(Value::Undef);
            }
            let r_norm = r_norm_sq.sqrt();
            let r_n = (
                r_q.0 / r_norm,
                r_q.1 / r_norm,
                r_q.2 / r_norm,
                r_q.3 / r_norm,
            );
            // Inverse rotation = conjugate (for unit quaternion).
            let r_inv = quat_conj(r_n);
            let r_inv_val = match normalize_quaternion(r_inv.0, r_inv.1, r_inv.2, r_inv.3) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // Inverse translation: t_inv = -R^-1 * t.
            let (rtx, rty, rtz) = quat_rotate(r_inv, tx, ty, tz);
            let dim = t_dim;
            let make_component = |v: f64| -> Value {
                if dim.is_dimensionless() {
                    Value::Real(v)
                } else {
                    Value::Scalar { si_value: v, dimension: dim }
                }
            };
            Value::Transform {
                rotation: Box::new(r_inv_val),
                translation: Box::new(Value::Vector(vec![
                    make_component(-rtx),
                    make_component(-rty),
                    make_component(-rtz),
                ])),
            }
        }

        "transform_compose" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let (r1_q, t1_items) = match &args[0] {
                Value::Transform { rotation, translation } => {
                    match (rotation.as_ref(), translation.as_ref()) {
                        (Value::Orientation { w, x, y, z }, Value::Vector(items)) => {
                            ((*w, *x, *y, *z), items.clone())
                        }
                        _ => return Some(Value::Undef),
                    }
                }
                _ => return Some(Value::Undef),
            };
            let (r2_q, t2_items) = match &args[1] {
                Value::Transform { rotation, translation } => {
                    match (rotation.as_ref(), translation.as_ref()) {
                        (Value::Orientation { w, x, y, z }, Value::Vector(items)) => {
                            ((*w, *x, *y, *z), items.clone())
                        }
                        _ => return Some(Value::Undef),
                    }
                }
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(r1_q.0, r1_q.1, r1_q.2, r1_q.3)
                || !quaternion_is_finite(r2_q.0, r2_q.1, r2_q.2, r2_q.3)
            {
                return Some(Value::Undef);
            }
            // Extract translation components and shared dimension.
            if t1_items.len() != 3 || t2_items.len() != 3 {
                return Some(Value::Undef);
            }
            let t1_dim = t1_items[0].dimension();
            if t1_items[1].dimension() != t1_dim || t1_items[2].dimension() != t1_dim {
                return Some(Value::Undef);
            }
            let t2_dim = t2_items[0].dimension();
            if t2_items[1].dimension() != t2_dim || t2_items[2].dimension() != t2_dim {
                return Some(Value::Undef);
            }
            if t1_dim != t2_dim {
                return Some(Value::Undef);
            }
            let (t1x, t1y, t1z) = match (t1_items[0].as_f64(), t1_items[1].as_f64(), t1_items[2].as_f64()) {
                (Some(a), Some(b), Some(c)) if a.is_finite() && b.is_finite() && c.is_finite() => (a, b, c),
                _ => return Some(Value::Undef),
            };
            let (t2x, t2y, t2z) = match (t2_items[0].as_f64(), t2_items[1].as_f64(), t2_items[2].as_f64()) {
                (Some(a), Some(b), Some(c)) if a.is_finite() && b.is_finite() && c.is_finite() => (a, b, c),
                _ => return Some(Value::Undef),
            };
            // Normalize R1 (matches operator-level semantics in reify-expr).
            let r1_norm_sq =
                r1_q.0 * r1_q.0 + r1_q.1 * r1_q.1 + r1_q.2 * r1_q.2 + r1_q.3 * r1_q.3;
            if r1_norm_sq < f64::EPSILON {
                return Some(Value::Undef);
            }
            let r1_norm = r1_norm_sq.sqrt();
            let r1_n = (
                r1_q.0 / r1_norm,
                r1_q.1 / r1_norm,
                r1_q.2 / r1_norm,
                r1_q.3 / r1_norm,
            );
            // R = R1 * R2 (Hamilton product), then normalize.
            let composed_r = quat_mul(r1_n, r2_q);
            let r_val = match normalize_quaternion(composed_r.0, composed_r.1, composed_r.2, composed_r.3) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            // t = R1 * t2 + t1.
            let (rt2x, rt2y, rt2z) = quat_rotate(r1_n, t2x, t2y, t2z);
            let dim = t1_dim;
            let make_component = |v: f64| -> Value {
                if dim.is_dimensionless() {
                    Value::Real(v)
                } else {
                    Value::Scalar { si_value: v, dimension: dim }
                }
            };
            Value::Transform {
                rotation: Box::new(r_val),
                translation: Box::new(Value::Vector(vec![
                    make_component(rt2x + t1x),
                    make_component(rt2y + t1y),
                    make_component(rt2z + t1z),
                ])),
            }
        }

        // --- Plane constructors ---
        "plane_xy" => make_plane(args, 2, [0.0, 0.0, 1.0]),
        "plane_xz" => make_plane(args, 1, [0.0, 1.0, 0.0]),
        "plane_yz" => make_plane(args, 0, [1.0, 0.0, 0.0]),

        // --- Axis constructors ---
        "axis_x" => make_axis(args, [1.0, 0.0, 0.0]),
        "axis_y" => make_axis(args, [0.0, 1.0, 0.0]),
        "axis_z" => make_axis(args, [0.0, 0.0, 1.0]),

        // --- BoundingBox constructors ---
        "bbox" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let min = &args[0];
            let max = &args[1];
            let min_comps = match min {
                Value::Point(comps) if comps.len() == 3 => comps,
                _ => return Some(Value::Undef),
            };
            let max_comps = match max {
                Value::Point(comps) if comps.len() == 3 => comps,
                _ => return Some(Value::Undef),
            };
            let min_dim = min_comps
                .first()
                .map(|v| v.dimension())
                .unwrap_or(DimensionVector::DIMENSIONLESS);
            let max_dim = max_comps
                .first()
                .map(|v| v.dimension())
                .unwrap_or(DimensionVector::DIMENSIONLESS);
            if min_dim != max_dim {
                return Some(Value::Undef);
            }
            Value::BoundingBox {
                min: Box::new(min.clone()),
                max: Box::new(max.clone()),
            }
        }

        // --- BoundingBox accessors ---
        "bbox_size" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::BoundingBox { min, max } => {
                    let (min_vals, dim) = match tensor_components_f64(min) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let (max_vals, _) = match tensor_components_f64(max) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    if min_vals.len() != 3 || max_vals.len() != 3 {
                        return Some(Value::Undef);
                    }
                    let make_component = |v: f64| -> Value {
                        if dim.is_dimensionless() {
                            Value::Real(v)
                        } else {
                            Value::Scalar {
                                si_value: v,
                                dimension: dim,
                            }
                        }
                    };
                    Value::Vector(vec![
                        make_component(max_vals[0] - min_vals[0]),
                        make_component(max_vals[1] - min_vals[1]),
                        make_component(max_vals[2] - min_vals[2]),
                    ])
                }
                _ => Value::Undef,
            }
        }
        "bbox_center" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            match &args[0] {
                Value::BoundingBox { min, max } => {
                    let (min_vals, dim) = match tensor_components_f64(min) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    let (max_vals, _) = match tensor_components_f64(max) {
                        Some(v) => v,
                        None => return Some(Value::Undef),
                    };
                    if min_vals.len() != 3 || max_vals.len() != 3 {
                        return Some(Value::Undef);
                    }
                    let make_component = |v: f64| -> Value {
                        if dim.is_dimensionless() {
                            Value::Real(v)
                        } else {
                            Value::Scalar {
                                si_value: v,
                                dimension: dim,
                            }
                        }
                    };
                    Value::Point(vec![
                        make_component((min_vals[0] + max_vals[0]) / 2.0),
                        make_component((min_vals[1] + max_vals[1]) / 2.0),
                        make_component((min_vals[2] + max_vals[2]) / 2.0),
                    ])
                }
                _ => Value::Undef,
            }
        }

        // --- Point/Vector constructors ---
        "point2" => construct_point_or_vector(args, 2, true),
        "point3" => construct_point_or_vector(args, 3, true),
        "vec2" => construct_point_or_vector(args, 2, false),
        "vec3" => construct_point_or_vector(args, 3, false),

        // --- Field operations (stubs) ---
        "sample" => Value::Undef,
        "gradient" => Value::Undef,
        "divergence" => Value::Undef,
        "curl" => Value::Undef,

        _ => return None,
    })
}

/// Validate args for a point/vector constructor and return `Value::Point` or `Value::Vector`.
fn construct_point_or_vector(args: &[Value], expected_n: usize, is_point: bool) -> Value {
    if args.len() != expected_n {
        return Value::Undef;
    }
    if !args.iter().all(|a| a.as_f64().is_some()) {
        return Value::Undef;
    }
    let first_dim = match args.first() {
        Some(v) => v.dimension(),
        None => return Value::Undef,
    };
    if !args.iter().all(|a| a.dimension() == first_dim) {
        return Value::Undef;
    }
    if is_point {
        Value::Point(args.to_vec())
    } else {
        Value::Vector(args.to_vec())
    }
}

/// Build a Plane from a single offset argument.
fn make_plane(args: &[Value], offset_index: usize, normal: [f64; 3]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    let offset_val = &args[0];
    let offset_f = match offset_val.as_f64() {
        Some(v) => v,
        None => return Value::Undef,
    };
    if !offset_f.is_finite() {
        return Value::Undef;
    }
    let dim = offset_val.dimension();
    let make_zero = || -> Value {
        if dim.is_dimensionless() {
            Value::Real(0.0)
        } else {
            Value::Scalar {
                si_value: 0.0,
                dimension: dim,
            }
        }
    };
    let offset_component = offset_val.clone();
    let zero = make_zero();
    let mut comps = [zero.clone(), zero.clone(), zero];
    comps[offset_index] = offset_component;
    let origin = Value::Point(comps.to_vec());
    let normal_vec = Value::Vector(vec![
        Value::Real(normal[0]),
        Value::Real(normal[1]),
        Value::Real(normal[2]),
    ]);
    Value::Plane {
        origin: Box::new(origin),
        normal: Box::new(normal_vec),
    }
}

/// Build an Axis from a single Point3 origin argument.
fn make_axis(args: &[Value], direction: [f64; 3]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    match &args[0] {
        Value::Point(comps) if comps.len() == 3 => {}
        _ => return Value::Undef,
    }
    let dir_vec = Value::Vector(vec![
        Value::Real(direction[0]),
        Value::Real(direction[1]),
        Value::Real(direction[2]),
    ]);
    Value::Axis {
        origin: Box::new(args[0].clone()),
        direction: Box::new(dir_vec),
    }
}

// Quaternion helpers used by frame_to_frame — re-imported from orientation module.
use crate::orientation::{normalize_quaternion, quat_conj, quat_mul, quat_rotate};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::construct_point_or_vector;
    use crate::eval_builtin;
    use reify_types::{DimensionVector, Value};

    // --- Determinacy predicate stubs (step-7) ---

    #[test]
    fn determined_stub_returns_undef() {
        // determined() is handled at the eval layer where DeterminacyState is available.
        // The stdlib stub returns Undef as a fallback.
        let result = eval_builtin("determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "determined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn undetermined_stub_returns_undef() {
        let result = eval_builtin("undetermined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "undetermined stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn constrained_stub_returns_undef() {
        let result = eval_builtin("constrained", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "constrained stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn partially_determined_stub_returns_undef() {
        let result = eval_builtin("partially_determined", &[Value::Real(42.0)]);
        assert!(
            result.is_undef(),
            "partially_determined stub should return Undef, got {:?}",
            result
        );
    }

    // --- Field operation stubs (step-25) ---

    #[test]
    fn gradient_scalar_field_returns_undef() {
        // gradient(field) on a scalar field should return Undef (stub).
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::length(),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = eval_builtin("gradient", &[field]);
        assert!(
            result.is_undef(),
            "gradient stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn divergence_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::StructureRef("Vector3".into()),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = eval_builtin("divergence", &[field]);
        assert!(
            result.is_undef(),
            "divergence stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn curl_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::StructureRef("Vector3".into()),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = eval_builtin("curl", &[field]);
        assert!(
            result.is_undef(),
            "curl stub should return Undef, got {:?}",
            result
        );
    }

    #[test]
    fn sample_in_stdlib_returns_undef() {
        // sample() in stdlib returns Undef because lambda application
        // needs an EvalContext (handled in reify-expr instead).
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::length(),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Arc::new(Value::Undef),
        };
        let result = eval_builtin("sample", &[field, Value::Int(42)]);
        assert!(
            result.is_undef(),
            "sample in stdlib should return Undef (handled in eval_expr), got {:?}",
            result
        );
    }

    // --- non-numeric args → Undef ---

    #[test]
    fn point3_non_numeric_undef() {
        // point3(String, Scalar, Scalar) → Undef
        let args = vec![
            Value::String("hello".to_string()),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        assert!(
            eval_builtin("point3", &args).is_undef(),
            "non-numeric first arg must return Undef"
        );
    }

    #[test]
    fn vec2_non_numeric_undef() {
        // vec2(Bool, Bool) → Undef
        let args = vec![Value::Bool(true), Value::Bool(false)];
        assert!(
            eval_builtin("vec2", &args).is_undef(),
            "Bool args must return Undef"
        );
    }

    // --- wrong arg count → Undef ---

    #[test]
    fn point3_wrong_arg_count_undef() {
        // point3 with 2 args → Undef
        let args2 = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        assert!(
            eval_builtin("point3", &args2).is_undef(),
            "point3 with 2 args must be Undef"
        );
        // point3 with 0 args → Undef
        assert!(
            eval_builtin("point3", &[]).is_undef(),
            "point3 with 0 args must be Undef"
        );
        // point3 with 4 args → Undef
        let args4 = vec![
            Value::Real(1.0),
            Value::Real(2.0),
            Value::Real(3.0),
            Value::Real(4.0),
        ];
        assert!(
            eval_builtin("point3", &args4).is_undef(),
            "point3 with 4 args must be Undef"
        );
    }

    #[test]
    fn point2_wrong_arg_count_undef() {
        // point2 with 3 args → Undef
        let args3 = vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)];
        assert!(
            eval_builtin("point2", &args3).is_undef(),
            "point2 with 3 args must be Undef"
        );
        // point2 with 1 arg → Undef
        assert!(
            eval_builtin("point2", &[Value::Real(1.0)]).is_undef(),
            "point2 with 1 arg must be Undef"
        );
    }

    #[test]
    fn vec3_wrong_arg_count_undef() {
        assert!(
            eval_builtin("vec3", &[]).is_undef(),
            "vec3 with 0 args must be Undef"
        );
        let args2 = vec![Value::Real(1.0), Value::Real(2.0)];
        assert!(
            eval_builtin("vec3", &args2).is_undef(),
            "vec3 with 2 args must be Undef"
        );
    }

    #[test]
    fn vec2_wrong_arg_count_undef() {
        assert!(
            eval_builtin("vec2", &[]).is_undef(),
            "vec2 with 0 args must be Undef"
        );
        let args3 = vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)];
        assert!(
            eval_builtin("vec2", &args3).is_undef(),
            "vec2 with 3 args must be Undef"
        );
    }

    // --- dimension mismatch → Undef ---

    #[test]
    fn point3_dimension_mismatch_undef() {
        // point3(Scalar(1,LENGTH), Scalar(2,MASS), Scalar(3,LENGTH)) → Undef
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        assert!(
            eval_builtin("point3", &args).is_undef(),
            "mixed dimensions must return Undef"
        );
    }

    #[test]
    fn vec3_dimension_mismatch_undef() {
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::MASS,
            },
        ];
        assert!(
            eval_builtin("vec3", &args).is_undef(),
            "mixed dimensions must return Undef"
        );
    }

    #[test]
    fn point2_dimension_mismatch_undef() {
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::MASS,
            },
        ];
        assert!(
            eval_builtin("point2", &args).is_undef(),
            "mixed dimensions must return Undef"
        );
    }

    #[test]
    fn vec2_dimension_mismatch_undef() {
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        assert!(
            eval_builtin("vec2", &args).is_undef(),
            "mixed dimensions must return Undef"
        );
    }

    // --- dimensionless components ---

    #[test]
    fn point3_dimensionless() {
        // point3(Real(1.0), Real(2.0), Real(3.0)) → Value::Point with Real components preserved
        let args = vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)];
        let result = eval_builtin("point3", &args);
        match result {
            Value::Point(ref items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(&items[0], Value::Real(v) if (*v - 1.0).abs() < 1e-12));
                assert!(matches!(&items[1], Value::Real(v) if (*v - 2.0).abs() < 1e-12));
                assert!(matches!(&items[2], Value::Real(v) if (*v - 3.0).abs() < 1e-12));
            }
            other => panic!("expected Point with Real components, got {:?}", other),
        }
    }

    // --- vec2 ---

    #[test]
    fn vec2_basic() {
        // vec2(9.0, 10.0) → Value::Vector([Real(9.0), Real(10.0)])
        let args = vec![Value::Real(9.0), Value::Real(10.0)];
        let result = eval_builtin("vec2", &args);
        match result {
            Value::Vector(ref items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[0], Value::Real(v) if (*v - 9.0).abs() < 1e-12));
                assert!(matches!(&items[1], Value::Real(v) if (*v - 10.0).abs() < 1e-12));
            }
            other => panic!("expected Vector, got {:?}", other),
        }
    }

    // --- point2 ---

    #[test]
    fn point2_basic() {
        // point2(7m, 8m) → Value::Point([Scalar(7,L), Scalar(8,L)])
        let args = vec![
            Value::Scalar {
                si_value: 7.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 8.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        let result = eval_builtin("point2", &args);
        match result {
            Value::Point(ref items) => {
                assert_eq!(items.len(), 2);
                assert_scalar_approx!(items[0].clone(), 7.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[1].clone(), 8.0, DimensionVector::LENGTH);
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    // --- vec3 ---

    #[test]
    fn vec3_basic() {
        // vec3(4m, 5m, 6m) → Value::Vector([Scalar(4,L), Scalar(5,L), Scalar(6,L)])
        let args = vec![
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 5.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 6.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        let result = eval_builtin("vec3", &args);
        match result {
            Value::Vector(ref items) => {
                assert_eq!(items.len(), 3);
                assert_scalar_approx!(items[0].clone(), 4.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[1].clone(), 5.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[2].clone(), 6.0, DimensionVector::LENGTH);
            }
            other => panic!("expected Vector, got {:?}", other),
        }
    }

    // --- point3 ---

    #[test]
    fn point3_basic() {
        // point3(1m, 2m, 3m) → Value::Point([Scalar(1,L), Scalar(2,L), Scalar(3,L)])
        let args = vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
        ];
        let result = eval_builtin("point3", &args);
        match result {
            Value::Point(ref items) => {
                assert_eq!(items.len(), 3);
                assert_scalar_approx!(items[0].clone(), 1.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[1].clone(), 2.0, DimensionVector::LENGTH);
                assert_scalar_approx!(items[2].clone(), 3.0, DimensionVector::LENGTH);
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    // --- Semantic distinction: point vs vector ---

    #[test]
    fn point_vector_semantic_distinction() {
        // point2 and vec2 with identical args must produce distinct Value variants
        let a = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Scalar {
            si_value: 2.0,
            dimension: DimensionVector::LENGTH,
        };

        let p2 = eval_builtin("point2", &[a.clone(), b.clone()]);
        let v2 = eval_builtin("vec2", &[a.clone(), b.clone()]);

        // point2 must produce Value::Point
        assert!(
            matches!(&p2, Value::Point(items) if items.len() == 2),
            "expected Value::Point(2), got {:?}",
            p2
        );

        // vec2 must produce Value::Vector
        assert!(
            matches!(&v2, Value::Vector(items) if items.len() == 2),
            "expected Value::Vector(2), got {:?}",
            v2
        );

        // point2(a,b) != vec2(a,b) — different variants
        assert_ne!(p2, v2, "point2 and vec2 with identical args must differ");

        // point3 vs vec3
        let c = Value::Scalar {
            si_value: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        let p3 = eval_builtin("point3", &[a.clone(), b.clone(), c.clone()]);
        let v3 = eval_builtin("vec3", &[a.clone(), b.clone(), c.clone()]);

        assert!(
            matches!(&p3, Value::Point(items) if items.len() == 3),
            "expected Value::Point(3), got {:?}",
            p3
        );
        assert!(
            matches!(&v3, Value::Vector(items) if items.len() == 3),
            "expected Value::Vector(3), got {:?}",
            v3
        );
        assert_ne!(p3, v3, "point3 and vec3 with identical args must differ");

        // content_hash: Point and Vector with same components produce different hashes
        assert_ne!(
            p2.content_hash(),
            v2.content_hash(),
            "point2 and vec2 content_hash must differ"
        );
        assert_ne!(
            p3.content_hash(),
            v3.content_hash(),
            "point3 and vec3 content_hash must differ"
        );

        // Display: point(...) vs vec(...)
        let p2_display = format!("{}", p2);
        let v2_display = format!("{}", v2);
        assert!(
            p2_display.starts_with("point("),
            "Point2 Display should start with 'point(', got {:?}",
            p2_display
        );
        assert!(
            v2_display.starts_with("vec("),
            "Vector2 Display should start with 'vec(', got {:?}",
            v2_display
        );
    }

    // ── construct_point_or_vector edge cases (task 398, step-11) ──────────────

    #[test]
    fn construct_point_or_vector_empty_args_returns_undef() {
        // When expected_n=0 and args=[], should return Undef, not panic.
        let result = construct_point_or_vector(&[], 0, true);
        assert!(
            result.is_undef(),
            "expected Undef for empty args with expected_n=0, got {:?}",
            result
        );

        let result = construct_point_or_vector(&[], 0, false);
        assert!(
            result.is_undef(),
            "expected Undef for empty vector args with expected_n=0, got {:?}",
            result
        );
    }

    // ── frame3 tests (step-5) ────────────────────────────────────────────────

    fn make_point3_len() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_identity_orientation() -> Value {
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    #[test]
    fn frame3_valid_args_returns_frame() {
        let origin = make_point3_len();
        let basis = make_identity_orientation();
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        match result {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert_eq!(*o, origin);
                assert_eq!(*b, basis);
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_stores_origin_and_basis_correctly() {
        let origin = Value::Point(vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(7.0),
        ]);
        let basis = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        match result {
            Value::Frame {
                origin: o,
                basis: b,
            } => {
                assert_eq!(*o, origin, "origin should be stored exactly");
                assert_eq!(*b, basis, "basis should be stored exactly");
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_no_args_returns_undef() {
        assert!(eval_builtin("frame3", &[]).is_undef());
    }

    #[test]
    fn frame3_one_arg_returns_undef() {
        assert!(eval_builtin("frame3", &[make_point3_len()]).is_undef());
    }

    #[test]
    fn frame3_three_args_returns_undef() {
        let o = make_point3_len();
        let b = make_identity_orientation();
        assert!(eval_builtin("frame3", &[o.clone(), b.clone(), Value::Real(0.0)]).is_undef());
    }

    #[test]
    fn frame3_non_point_first_arg_returns_undef() {
        let basis = make_identity_orientation();
        // First arg is Real, not Point
        assert!(eval_builtin("frame3", &[Value::Real(1.0), basis]).is_undef());
    }

    #[test]
    fn frame3_non_orientation_second_arg_returns_undef() {
        let origin = make_point3_len();
        // Second arg is Real, not Orientation
        assert!(eval_builtin("frame3", &[origin, Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn frame3_point2_origin_returns_undef() {
        // Point2 (wrong component count) should be rejected
        let origin_2d = Value::Point(vec![Value::length(1.0), Value::length(2.0)]);
        let basis = make_identity_orientation();
        assert!(eval_builtin("frame3", &[origin_2d, basis]).is_undef());
    }

    #[test]
    fn frame3_point4_origin_returns_undef() {
        // Point4 (wrong component count) should be rejected
        let origin_4d = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
            Value::length(4.0),
        ]);
        let basis = make_identity_orientation();
        assert!(eval_builtin("frame3", &[origin_4d, basis]).is_undef());
    }

    #[test]
    fn frame3_dimensionless_point3_is_accepted() {
        // Point3 with dimensionless (Real) components is accepted
        let origin = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let basis = make_identity_orientation();
        let result = eval_builtin("frame3", &[origin.clone(), basis.clone()]);
        assert!(
            matches!(&result, Value::Frame { .. }),
            "expected Value::Frame for dimensionless Point3 origin, got {:?}",
            result
        );
    }

    // ── frame3_identity tests (step-7) ────────────────────────────────────────

    #[test]
    fn frame3_identity_no_args_returns_frame() {
        let result = eval_builtin("frame3_identity", &[]);
        assert!(
            matches!(&result, Value::Frame { .. }),
            "expected Value::Frame, got {:?}",
            result
        );
    }

    #[test]
    fn frame3_identity_origin_is_zero_length_point3() {
        let result = eval_builtin("frame3_identity", &[]);
        match result {
            Value::Frame { origin, .. } => {
                let expected_origin = Value::Point(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::length(0.0),
                ]);
                assert_eq!(
                    *origin, expected_origin,
                    "identity origin should be zero Point3<Length>"
                );
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_identity_basis_is_identity_quaternion() {
        let result = eval_builtin("frame3_identity", &[]);
        match result {
            Value::Frame { basis, .. } => {
                let expected_basis = Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                };
                assert_eq!(
                    *basis, expected_basis,
                    "identity basis should be (w:1,x:0,y:0,z:0)"
                );
            }
            other => panic!("expected Value::Frame, got {:?}", other),
        }
    }

    #[test]
    fn frame3_identity_with_any_args_returns_undef() {
        assert!(eval_builtin("frame3_identity", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("frame3_identity", &[Value::Real(1.0), Value::Real(2.0)]).is_undef());
        assert!(
            eval_builtin(
                "frame3_identity",
                &[Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]
            )
            .is_undef()
        );
        assert!(
            eval_builtin(
                "frame3_identity",
                &[
                    Value::Real(1.0),
                    Value::Real(2.0),
                    Value::Real(3.0),
                    Value::Real(4.0)
                ]
            )
            .is_undef()
        );
    }

    // ── transform3 tests (step-5) ─────────────────────────────────────────────

    fn make_vec3_length() -> Value {
        Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    #[test]
    fn transform3_valid_args_returns_transform() {
        let rotation = make_identity_orientation();
        let translation = make_vec3_length();
        let result = eval_builtin("transform3", &[rotation.clone(), translation.clone()]);
        match result {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert_eq!(*r, rotation);
                assert_eq!(*t, translation);
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_stores_rotation_and_translation_correctly() {
        let rotation = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let translation = Value::Vector(vec![
            Value::length(5.0),
            Value::length(6.0),
            Value::length(7.0),
        ]);
        let result = eval_builtin("transform3", &[rotation.clone(), translation.clone()]);
        match result {
            Value::Transform {
                rotation: r,
                translation: t,
            } => {
                assert_eq!(*r, rotation, "rotation should be stored exactly");
                assert_eq!(*t, translation, "translation should be stored exactly");
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_no_args_returns_undef() {
        assert!(eval_builtin("transform3", &[]).is_undef());
    }

    #[test]
    fn transform3_one_arg_returns_undef() {
        assert!(eval_builtin("transform3", &[make_identity_orientation()]).is_undef());
    }

    #[test]
    fn transform3_three_args_returns_undef() {
        let r = make_identity_orientation();
        let t = make_vec3_length();
        assert!(eval_builtin("transform3", &[r.clone(), t.clone(), Value::Real(0.0)]).is_undef());
    }

    #[test]
    fn transform3_non_orientation_first_arg_returns_undef() {
        // First arg is Real, not Orientation
        assert!(eval_builtin("transform3", &[Value::Real(1.0), make_vec3_length()]).is_undef());
    }

    #[test]
    fn transform3_non_vector_second_arg_returns_undef() {
        // Second arg is Real, not Vector
        assert!(
            eval_builtin(
                "transform3",
                &[make_identity_orientation(), Value::Real(1.0)]
            )
            .is_undef()
        );
    }

    #[test]
    fn transform3_point3_second_arg_returns_undef() {
        // Second arg is Point3, not Vector3
        let pt3 = Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        assert!(eval_builtin("transform3", &[make_identity_orientation(), pt3]).is_undef());
    }

    #[test]
    fn transform3_orientation_second_arg_returns_undef() {
        // Second arg is Orientation, not Vector3
        assert!(
            eval_builtin(
                "transform3",
                &[make_identity_orientation(), make_identity_orientation()]
            )
            .is_undef()
        );
    }

    #[test]
    fn transform3_vector2_translation_returns_undef() {
        // Vector2 (wrong component count) should be rejected
        let vec2 = Value::Vector(vec![Value::length(1.0), Value::length(2.0)]);
        assert!(eval_builtin("transform3", &[make_identity_orientation(), vec2]).is_undef());
    }

    #[test]
    fn transform3_dimensionless_vector3_is_accepted() {
        // Vector3 with dimensionless (Real) components is accepted
        let translation = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let result = eval_builtin(
            "transform3",
            &[make_identity_orientation(), translation.clone()],
        );
        assert!(
            matches!(&result, Value::Transform { .. }),
            "expected Value::Transform for dimensionless Vector3 translation, got {:?}",
            result
        );
    }

    // ── transform3_identity tests (step-7) ────────────────────────────────────

    #[test]
    fn transform3_identity_no_args_returns_transform() {
        let result = eval_builtin("transform3_identity", &[]);
        assert!(
            matches!(&result, Value::Transform { .. }),
            "expected Value::Transform, got {:?}",
            result
        );
    }

    #[test]
    fn transform3_identity_rotation_is_identity_quaternion() {
        let result = eval_builtin("transform3_identity", &[]);
        match result {
            Value::Transform { rotation, .. } => {
                let expected = Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                };
                assert_eq!(
                    *rotation, expected,
                    "identity rotation should be (w:1,x:0,y:0,z:0)"
                );
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_identity_translation_is_zero_length_vector3() {
        let result = eval_builtin("transform3_identity", &[]);
        match result {
            Value::Transform { translation, .. } => {
                let expected = Value::Vector(vec![
                    Value::length(0.0),
                    Value::length(0.0),
                    Value::length(0.0),
                ]);
                assert_eq!(
                    *translation, expected,
                    "identity translation should be zero Vector3<Length>"
                );
            }
            other => panic!("expected Value::Transform, got {:?}", other),
        }
    }

    #[test]
    fn transform3_identity_with_any_args_returns_undef() {
        assert!(eval_builtin("transform3_identity", &[Value::Real(1.0)]).is_undef());
        assert!(
            eval_builtin("transform3_identity", &[Value::Real(1.0), Value::Real(2.0)]).is_undef()
        );
    }

    // ── axis_z tests (step-5) ────────────────────────────────────────────────

    fn make_point3_length() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_point2_length() -> Value {
        Value::Point(vec![Value::length(1.0), Value::length(2.0)])
    }

    #[test]
    fn axis_z_with_point3_returns_axis() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", std::slice::from_ref(&origin));
        assert!(
            matches!(result, Value::Axis { .. }),
            "expected Value::Axis, got {:?}",
            result
        );
    }

    #[test]
    fn axis_z_stores_origin_correctly() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", std::slice::from_ref(&origin));
        match result {
            Value::Axis { origin: o, .. } => assert_eq!(*o, origin),
            other => panic!("expected Value::Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_z_direction_is_z() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_z", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps.len(), 3);
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(1.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_z_no_args_returns_undef() {
        assert!(eval_builtin("axis_z", &[]).is_undef());
    }

    #[test]
    fn axis_z_real_arg_returns_undef() {
        assert!(eval_builtin("axis_z", &[Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn axis_z_point2_returns_undef() {
        assert!(eval_builtin("axis_z", &[make_point2_length()]).is_undef());
    }

    #[test]
    fn axis_z_vector3_returns_undef() {
        let vec3 = Value::Vector(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ]);
        assert!(eval_builtin("axis_z", &[vec3]).is_undef());
    }

    // ── axis_x / axis_y tests (step-7) ───────────────────────────────────────

    #[test]
    fn axis_x_direction_is_x() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_x", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps[0], Value::Real(1.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_y_direction_is_y() {
        let origin = make_point3_length();
        let result = eval_builtin("axis_y", &[origin]);
        match result {
            Value::Axis { direction, .. } => match *direction {
                Value::Vector(ref comps) => {
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(1.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Vector, got {:?}", other),
            },
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    #[test]
    fn axis_x_no_args_returns_undef() {
        assert!(eval_builtin("axis_x", &[]).is_undef());
    }

    #[test]
    fn axis_y_two_args_returns_undef() {
        assert!(eval_builtin("axis_y", &[make_point3_length(), make_point3_length()]).is_undef());
    }

    #[test]
    fn axis_x_with_dimensionless_point3() {
        let origin = Value::Point(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let result = eval_builtin("axis_x", std::slice::from_ref(&origin));
        match result {
            Value::Axis { origin: o, .. } => assert_eq!(*o, origin),
            other => panic!("expected Axis, got {:?}", other),
        }
    }

    // ── bbox tests (step-9) ──────────────────────────────────────────────────

    fn make_point3_min() -> Value {
        Value::Point(vec![
            Value::length(1.0),
            Value::length(2.0),
            Value::length(3.0),
        ])
    }

    fn make_point3_max() -> Value {
        Value::Point(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(9.0),
        ])
    }

    #[test]
    fn bbox_with_two_point3_returns_bounding_box() {
        let result = eval_builtin("bbox", &[make_point3_min(), make_point3_max()]);
        assert!(
            matches!(result, Value::BoundingBox { .. }),
            "expected BoundingBox, got {:?}",
            result
        );
    }

    #[test]
    fn bbox_stores_min_and_max() {
        let min = make_point3_min();
        let max = make_point3_max();
        let result = eval_builtin("bbox", &[min.clone(), max.clone()]);
        match result {
            Value::BoundingBox { min: mn, max: mx } => {
                assert_eq!(*mn, min);
                assert_eq!(*mx, max);
            }
            other => panic!("expected BoundingBox, got {:?}", other),
        }
    }

    #[test]
    fn bbox_mismatched_dimensions_returns_undef() {
        let min = Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let max = Value::Point(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
        ]);
        assert!(eval_builtin("bbox", &[min, max]).is_undef());
    }

    #[test]
    fn bbox_non_point_arg_returns_undef() {
        let vec3 = Value::Vector(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]);
        let pt3 = make_point3_min();
        assert!(eval_builtin("bbox", &[vec3, pt3]).is_undef());
    }

    #[test]
    fn bbox_point2_returns_undef() {
        let pt2 = make_point2_length();
        let pt3 = make_point3_min();
        assert!(eval_builtin("bbox", &[pt2, pt3]).is_undef());
    }

    #[test]
    fn bbox_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox", &[]).is_undef());
        assert!(eval_builtin("bbox", &[make_point3_min()]).is_undef());
        assert!(
            eval_builtin(
                "bbox",
                &[make_point3_min(), make_point3_max(), make_point3_min()]
            )
            .is_undef()
        );
    }

    #[test]
    fn bbox_one_point_one_vector_returns_undef() {
        let pt3 = make_point3_min();
        let vec3 = Value::Vector(vec![
            Value::length(4.0),
            Value::length(6.0),
            Value::length(9.0),
        ]);
        assert!(eval_builtin("bbox", &[pt3, vec3]).is_undef());
    }

    // ── bbox_size / bbox_center tests (step-11) ──────────────────────────────

    fn make_bbox() -> Value {
        Value::BoundingBox {
            min: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::length(2.0),
                Value::length(3.0),
            ])),
            max: Box::new(Value::Point(vec![
                Value::length(4.0),
                Value::length(6.0),
                Value::length(9.0),
            ])),
        }
    }

    #[test]
    fn bbox_size_returns_correct_vector() {
        // min=(1m,2m,3m), max=(4m,6m,9m) → size=(3m,4m,6m)
        let result = eval_builtin("bbox_size", &[make_bbox()]);
        match result {
            Value::Vector(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(3.0));
                assert_eq!(comps[1], Value::length(4.0));
                assert_eq!(comps[2], Value::length(6.0));
            }
            other => panic!("expected Vector, got {:?}", other),
        }
    }

    #[test]
    fn bbox_center_returns_correct_point() {
        // min=(1m,2m,3m), max=(4m,6m,9m) → center=(2.5m,4m,6m)
        let result = eval_builtin("bbox_center", &[make_bbox()]);
        match result {
            Value::Point(ref comps) => {
                assert_eq!(comps.len(), 3);
                assert_eq!(comps[0], Value::length(2.5));
                assert_eq!(comps[1], Value::length(4.0));
                assert_eq!(comps[2], Value::length(6.0));
            }
            other => panic!("expected Point, got {:?}", other),
        }
    }

    #[test]
    fn bbox_size_non_bounding_box_returns_undef() {
        assert!(eval_builtin("bbox_size", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("bbox_size", &[make_point3_min()]).is_undef());
    }

    #[test]
    fn bbox_center_non_bounding_box_returns_undef() {
        assert!(eval_builtin("bbox_center", &[Value::Undef]).is_undef());
        assert!(eval_builtin("bbox_center", &[make_point3_min()]).is_undef());
    }

    #[test]
    fn bbox_size_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox_size", &[]).is_undef());
        assert!(eval_builtin("bbox_size", &[make_bbox(), make_bbox()]).is_undef());
    }

    #[test]
    fn bbox_center_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("bbox_center", &[]).is_undef());
        assert!(eval_builtin("bbox_center", &[make_bbox(), make_bbox()]).is_undef());
    }

    #[test]
    fn bbox_size_dimensionless_bbox() {
        let bbox = Value::BoundingBox {
            min: Box::new(Value::Point(vec![
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ])),
            max: Box::new(Value::Point(vec![
                Value::Real(2.0),
                Value::Real(4.0),
                Value::Real(6.0),
            ])),
        };
        let result = eval_builtin("bbox_size", &[bbox]);
        match result {
            Value::Vector(ref comps) => {
                assert_eq!(comps[0], Value::Real(2.0));
                assert_eq!(comps[1], Value::Real(4.0));
                assert_eq!(comps[2], Value::Real(6.0));
            }
            other => panic!("expected Vector of Reals, got {:?}", other),
        }
    }

    // ── plane_xz / plane_yz tests (step-3) ───────────────────────────────────

    #[test]
    fn plane_xz_with_length_offset_returns_plane() {
        let result = eval_builtin("plane_xz", &[Value::length(0.003)]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_xz_correct_origin_and_normal() {
        // plane_xz(3mm) → origin=(0m, 3mm, 0m), normal=(0,1,0)
        let result = eval_builtin("plane_xz", &[Value::length(0.003)]);
        match result {
            Value::Plane { origin, normal } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3);
                        assert_eq!(comps[0], Value::length(0.0), "x should be 0m");
                        assert_eq!(comps[1], Value::length(0.003), "y should be 3mm");
                        assert_eq!(comps[2], Value::length(0.0), "z should be 0m");
                    }
                    other => panic!("expected Point, got {:?}", other),
                }
                match *normal {
                    Value::Vector(ref comps) => {
                        assert_eq!(comps[0], Value::Real(0.0));
                        assert_eq!(comps[1], Value::Real(1.0));
                        assert_eq!(comps[2], Value::Real(0.0));
                    }
                    other => panic!("expected Vector, got {:?}", other),
                }
            }
            other => panic!("expected Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_yz_with_length_offset_returns_plane() {
        let result = eval_builtin("plane_yz", &[Value::length(0.007)]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_yz_correct_origin_and_normal() {
        // plane_yz(7mm) → origin=(7mm, 0m, 0m), normal=(1,0,0)
        let result = eval_builtin("plane_yz", &[Value::length(0.007)]);
        match result {
            Value::Plane { origin, normal } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3);
                        assert_eq!(comps[0], Value::length(0.007), "x should be 7mm");
                        assert_eq!(comps[1], Value::length(0.0), "y should be 0m");
                        assert_eq!(comps[2], Value::length(0.0), "z should be 0m");
                    }
                    other => panic!("expected Point, got {:?}", other),
                }
                match *normal {
                    Value::Vector(ref comps) => {
                        assert_eq!(comps[0], Value::Real(1.0));
                        assert_eq!(comps[1], Value::Real(0.0));
                        assert_eq!(comps[2], Value::Real(0.0));
                    }
                    other => panic!("expected Vector, got {:?}", other),
                }
            }
            other => panic!("expected Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xz_no_args_returns_undef() {
        assert!(eval_builtin("plane_xz", &[]).is_undef());
    }

    #[test]
    fn plane_yz_no_args_returns_undef() {
        assert!(eval_builtin("plane_yz", &[]).is_undef());
    }

    #[test]
    fn plane_xz_nan_returns_undef() {
        assert!(eval_builtin("plane_xz", &[Value::Real(f64::NAN)]).is_undef());
    }

    #[test]
    fn plane_yz_two_args_returns_undef() {
        assert!(eval_builtin("plane_yz", &[Value::length(0.0), Value::length(0.0)]).is_undef());
    }

    // ── plane_xy tests (step-1) ───────────────────────────────────────────────

    #[test]
    fn plane_xy_with_length_offset_returns_plane() {
        // plane_xy(5mm) → Plane with origin=(0m,0m,5mm) and normal=(0,0,1)
        let offset = Value::length(0.005); // 5mm in SI (meters)
        let result = eval_builtin("plane_xy", &[offset]);
        assert!(
            matches!(result, Value::Plane { .. }),
            "expected Value::Plane, got {:?}",
            result
        );
    }

    #[test]
    fn plane_xy_with_length_offset_correct_origin() {
        let offset = Value::length(0.005); // 5mm
        let result = eval_builtin("plane_xy", &[offset]);
        match result {
            Value::Plane { origin, .. } => {
                match *origin {
                    Value::Point(ref comps) => {
                        assert_eq!(comps.len(), 3, "origin should be 3D");
                        // x=0m, y=0m, z=5mm
                        assert_eq!(comps[0], Value::length(0.0), "origin.x should be 0m");
                        assert_eq!(comps[1], Value::length(0.0), "origin.y should be 0m");
                        assert_eq!(comps[2], Value::length(0.005), "origin.z should be 5mm");
                    }
                    other => panic!("origin should be Point, got {:?}", other),
                }
            }
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xy_with_length_offset_correct_normal() {
        let offset = Value::length(0.005);
        let result = eval_builtin("plane_xy", &[offset]);
        match result {
            Value::Plane { normal, .. } => match *normal {
                Value::Vector(ref comps) => {
                    assert_eq!(comps.len(), 3, "normal should be 3D");
                    assert_eq!(comps[0], Value::Real(0.0), "normal.x should be 0");
                    assert_eq!(comps[1], Value::Real(0.0), "normal.y should be 0");
                    assert_eq!(comps[2], Value::Real(1.0), "normal.z should be 1");
                }
                other => panic!("normal should be Vector, got {:?}", other),
            },
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    #[test]
    fn plane_xy_no_args_returns_undef() {
        assert!(eval_builtin("plane_xy", &[]).is_undef());
    }

    #[test]
    fn plane_xy_bool_arg_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Bool(true)]).is_undef());
    }

    #[test]
    fn plane_xy_two_args_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::length(0.0), Value::length(0.0)]).is_undef());
    }

    #[test]
    fn plane_xy_nan_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Real(f64::NAN)]).is_undef());
    }

    #[test]
    fn plane_xy_inf_returns_undef() {
        assert!(eval_builtin("plane_xy", &[Value::Real(f64::INFINITY)]).is_undef());
    }

    #[test]
    fn plane_xy_real_zero_produces_dimensionless_origin() {
        // plane_xy(Real(0.0)) → dimensionless origin with Real(0.0) components
        let result = eval_builtin("plane_xy", &[Value::Real(0.0)]);
        match result {
            Value::Plane { origin, .. } => match *origin {
                Value::Point(ref comps) => {
                    assert_eq!(comps.len(), 3);
                    assert_eq!(comps[0], Value::Real(0.0));
                    assert_eq!(comps[1], Value::Real(0.0));
                    assert_eq!(comps[2], Value::Real(0.0));
                }
                other => panic!("expected Point, got {:?}", other),
            },
            other => panic!("expected Value::Plane, got {:?}", other),
        }
    }

    // ── step-7: frame_to_frame tests ─────────────────────────────────────────

    /// Helper: build a Frame with given origin (LENGTH) and orientation.
    fn make_frame(ox: f64, oy: f64, oz: f64, orientation: Value) -> Value {
        Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(ox),
                Value::length(oy),
                Value::length(oz),
            ])),
            basis: Box::new(orientation),
        }
    }

    /// Helper: 90-degree Z rotation quaternion.
    fn make_rot90z() -> Value {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        }
    }

    /// frame_to_frame(F, F) should return an identity transform.
    #[test]
    fn frame_to_frame_same_gives_identity() {
        let f = make_frame(5.0, 3.0, 1.0, make_identity_orientation());
        let result = eval_builtin("frame_to_frame", &[f.clone(), f]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // Identity rotation
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-10);
                // Zero translation
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected ~0");
                        }
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame(origin_frame, translated_frame) gives pure translation.
    #[test]
    fn frame_to_frame_translated() {
        let from = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(5.0, 0.0, 0.0, make_identity_orientation());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // Identity rotation
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-10);
                // Translation = (5,0,0)
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 5.0).abs() < 1e-10, "tx = {tx}, expected 5");
                        assert!(ty.abs() < 1e-10, "ty = {ty}, expected 0");
                        assert!(tz.abs() < 1e-10, "tz = {tz}, expected 0");
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame(identity_frame, rotated_frame) gives pure rotation.
    #[test]
    fn frame_to_frame_rotated() {
        let from = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(0.0, 0.0, 0.0, make_rot90z());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                // 90Z rotation
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-10);
                // Zero translation
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected ~0");
                        }
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// frame_to_frame with both rotation and translation.
    /// From: origin=(1,0,0), identity rotation
    /// To: origin=(0,0,0), 90Z rotation
    /// R = R_to * conj(R_from) = 90Z * identity = 90Z
    /// t = origin_to - R * origin_from = (0,0,0) - 90Z*(1,0,0) = (0,0,0) - (0,1,0) = (0,-1,0)
    #[test]
    fn frame_to_frame_general() {
        let from = make_frame(1.0, 0.0, 0.0, make_identity_orientation());
        let to = make_frame(0.0, 0.0, 0.0, make_rot90z());
        let result = eval_builtin("frame_to_frame", &[from, to]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-10);
                match *translation {
                    Value::Vector(ref items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(tx.abs() < 1e-10, "tx = {tx}, expected 0");
                        assert!((ty + 1.0).abs() < 1e-10, "ty = {ty}, expected -1");
                        assert!(tz.abs() < 1e-10, "tz = {tz}, expected 0");
                    }
                    ref other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Wrong argument count or non-Frame args return Undef.
    #[test]
    fn frame_to_frame_wrong_args_undef() {
        // No args
        assert!(eval_builtin("frame_to_frame", &[]).is_undef());
        // One arg
        let f = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(eval_builtin("frame_to_frame", std::slice::from_ref(&f)).is_undef());
        // Three args
        assert!(eval_builtin("frame_to_frame", &[f.clone(), f.clone(), f.clone()]).is_undef());
        // Non-Frame args
        assert!(eval_builtin("frame_to_frame", &[Value::Real(1.0), f.clone()]).is_undef());
        assert!(eval_builtin("frame_to_frame", &[f, Value::Real(1.0)]).is_undef());
    }

    /// frame_to_frame with NaN in origin_from x-component should return Undef.
    #[test]
    fn frame_to_frame_nan_origin_from_returns_undef() {
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::length(0.0),
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        let to = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "NaN in origin_from should return Undef"
        );
    }

    /// frame_to_frame with NaN in origin_to y-component should return Undef.
    #[test]
    fn frame_to_frame_nan_origin_to_returns_undef() {
        let from = make_frame(1.0, 0.0, 0.0, make_identity_orientation());
        let to = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(0.0),
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "NaN in origin_to should return Undef"
        );
    }

    /// frame_to_frame with mixed-dimension origin (length, angle, length) should return Undef.
    #[test]
    fn frame_to_frame_mixed_dimension_origin_returns_undef() {
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::angle(0.0), // dimension mismatch within same origin
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        let to = make_frame(0.0, 0.0, 0.0, make_identity_orientation());
        assert!(
            eval_builtin("frame_to_frame", &[from, to]).is_undef(),
            "mixed-dimension origin should return Undef"
        );
    }

    /// frame_to_frame with mismatched origin dimensions (LENGTH vs ANGLE) returns Undef.
    #[test]
    fn frame_to_frame_mismatched_origin_dimensions_undef() {
        // from-frame: LENGTH-dimensioned origin
        let from = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::length(1.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        // to-frame: ANGLE-dimensioned origin
        let to = Value::Frame {
            origin: Box::new(Value::Point(vec![
                Value::angle(1.0),
                Value::angle(0.0),
                Value::angle(0.0),
            ])),
            basis: Box::new(make_identity_orientation()),
        };
        assert!(eval_builtin("frame_to_frame", &[from, to]).is_undef());
    }

    // ── transform_compose tests (step-15) ────────────────────────────────────

    /// Helper: build a Transform from rotation quaternion and translation (LENGTH).
    fn make_transform(rotation: Value, tx: f64, ty: f64, tz: f64) -> Value {
        Value::Transform {
            rotation: Box::new(rotation),
            translation: Box::new(Value::Vector(vec![
                Value::length(tx),
                Value::length(ty),
                Value::length(tz),
            ])),
        }
    }

    /// transform_compose(identity, T) == T
    #[test]
    fn transform_compose_identity_left() {
        let id = eval_builtin("transform3_identity", &[]);
        let t = make_transform(make_rot90z(), 1.0, 2.0, 3.0);
        let result = eval_builtin("transform_compose", &[id, t.clone()]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.0).abs() < 1e-12, "tx = {tx}, expected 1");
                        assert!((ty - 2.0).abs() < 1e-12, "ty = {ty}, expected 2");
                        assert!((tz - 3.0).abs() < 1e-12, "tz = {tz}, expected 3");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_compose(T, identity) == T
    #[test]
    fn transform_compose_identity_right() {
        let id = eval_builtin("transform3_identity", &[]);
        let t = make_transform(make_rot90z(), 1.0, 2.0, 3.0);
        let result = eval_builtin("transform_compose", &[t.clone(), id]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.0).abs() < 1e-12, "tx = {tx}, expected 1");
                        assert!((ty - 2.0).abs() < 1e-12, "ty = {ty}, expected 2");
                        assert!((tz - 3.0).abs() < 1e-12, "tz = {tz}, expected 3");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Pure translation composition: (R=I, t=[1,0,0]) * (R=I, t=[0,2,0]) == (R=I, t=[1,2,0]).
    #[test]
    fn transform_compose_pure_translation() {
        let t1 = make_transform(make_identity_orientation(), 1.0, 0.0, 0.0);
        let t2 = make_transform(make_identity_orientation(), 0.0, 2.0, 0.0);
        let result = eval_builtin("transform_compose", &[t1, t2]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.0).abs() < 1e-12, "tx = {tx}, expected 1");
                        assert!((ty - 2.0).abs() < 1e-12, "ty = {ty}, expected 2");
                        assert!(tz.abs() < 1e-12, "tz = {tz}, expected 0");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Translation rotated by R1: (R=90Z, t=0) * (R=I, t=[1,0,0]) == (R=90Z, t=[0,1,0]).
    /// Composition formula: t = R1*t2 + t1 = 90Z*(1,0,0) + 0 = (0,1,0).
    #[test]
    fn transform_compose_rotation_then_translation() {
        let t1 = make_transform(make_rot90z(), 0.0, 0.0, 0.0);
        let t2 = make_transform(make_identity_orientation(), 1.0, 0.0, 0.0);
        let result = eval_builtin("transform_compose", &[t1, t2]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(tx.abs() < 1e-12, "tx = {tx}, expected 0");
                        assert!((ty - 1.0).abs() < 1e-12, "ty = {ty}, expected 1");
                        assert!(tz.abs() < 1e-12, "tz = {tz}, expected 0");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_compose(T1, T2) must be bit-equal to T1 * T2 (operator-level).
    /// Both must use identical algebra (R1*R2, R1*t2+t1).
    #[test]
    fn transform_compose_matches_operator_path() {
        // Use transform3_identity-derived inputs that don't already pre-normalize quaternions.
        let q1 = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let q2 = Value::Orientation {
            w: 0.5,
            x: -0.5,
            y: 0.5,
            z: 0.5,
        };
        let t1 = make_transform(q1, 1.0, 2.0, 3.0);
        let t2 = make_transform(q2, 4.0, 5.0, 6.0);
        let composed = eval_builtin("transform_compose", &[t1.clone(), t2.clone()]);
        // The named function and the * operator must produce identical Value
        // (decision-record: regression test asserts equality).
        // We mirror the exact algebra used by the operator-level path:
        //   R = normalize(q1) * q2
        //   t = normalize(q1) * t2 + t1  (vector rotation)
        // Construct the expected result component-by-component and compare.
        let q1_t = (0.5, 0.5, 0.5, 0.5);
        let q2_t = (0.5, -0.5, 0.5, 0.5);
        // q1 already has norm 1 → no-op.
        let (rw, rx, ry, rz) = super::quat_mul(q1_t, q2_t);
        let norm = (rw * rw + rx * rx + ry * ry + rz * rz).sqrt();
        let (rw, rx, ry, rz) = (rw / norm, rx / norm, ry / norm, rz / norm);
        let (rt2x, rt2y, rt2z) = super::quat_rotate(q1_t, 4.0, 5.0, 6.0);
        match composed {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, rw, rx, ry, rz, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(
                            (tx - (rt2x + 1.0)).abs() < 1e-12,
                            "tx = {tx}, expected {}",
                            rt2x + 1.0
                        );
                        assert!(
                            (ty - (rt2y + 2.0)).abs() < 1e-12,
                            "ty = {ty}, expected {}",
                            rt2y + 2.0
                        );
                        assert!(
                            (tz - (rt2z + 3.0)).abs() < 1e-12,
                            "tz = {tz}, expected {}",
                            rt2z + 3.0
                        );
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_compose with wrong arg count → Undef.
    #[test]
    fn transform_compose_wrong_arg_count_returns_undef() {
        let t = make_transform(make_identity_orientation(), 0.0, 0.0, 0.0);
        assert!(eval_builtin("transform_compose", &[]).is_undef());
        assert!(eval_builtin("transform_compose", std::slice::from_ref(&t)).is_undef());
        assert!(
            eval_builtin("transform_compose", &[t.clone(), t.clone(), t.clone()]).is_undef()
        );
    }

    /// transform_compose with non-Transform arg → Undef.
    #[test]
    fn transform_compose_non_transform_arg_returns_undef() {
        let t = make_transform(make_identity_orientation(), 0.0, 0.0, 0.0);
        assert!(eval_builtin("transform_compose", &[Value::Real(1.0), t.clone()]).is_undef());
        assert!(eval_builtin("transform_compose", &[t, Value::Real(1.0)]).is_undef());
    }

    /// transform_compose with mixed-dimension translations → Undef.
    /// (LENGTH translation in T1, ANGLE translation in T2)
    #[test]
    fn transform_compose_mixed_dimension_translations_returns_undef() {
        let t1 = Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(vec![
                Value::length(1.0),
                Value::length(0.0),
                Value::length(0.0),
            ])),
        };
        let t2 = Value::Transform {
            rotation: Box::new(make_identity_orientation()),
            translation: Box::new(Value::Vector(vec![
                Value::angle(0.0),
                Value::angle(0.0),
                Value::angle(0.0),
            ])),
        };
        assert!(eval_builtin("transform_compose", &[t1, t2]).is_undef());
    }

    // ── transform_inverse tests (step-17) ────────────────────────────────────

    /// transform_inverse(identity) == identity.
    #[test]
    fn transform_inverse_identity_is_identity() {
        let id = eval_builtin("transform3_identity", &[]);
        let result = eval_builtin("transform_inverse", &[id]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-12, "translation[{i}] = {v}, expected 0");
                        }
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_inverse((R=90Z, t=[1,0,0])) has R = -90Z (conjugate of 90Z) and t = -R^-1 * (1,0,0) = (0,1,0).
    /// Computation: R^-1 = conj(R) = (s, 0, 0, -s). R^-1 * (1,0,0) = quat_rotate(R^-1, (1,0,0)) = (0,-1,0).
    /// t_inv = -R^-1 * t = -(0,-1,0) = (0,1,0).
    #[test]
    fn transform_inverse_90z_with_translation() {
        let t = make_transform(make_rot90z(), 1.0, 0.0, 0.0);
        let result = eval_builtin("transform_inverse", &[t]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, -s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!(tx.abs() < 1e-12, "tx = {tx}, expected 0");
                        assert!((ty - 1.0).abs() < 1e-12, "ty = {ty}, expected 1");
                        assert!(tz.abs() < 1e-12, "tz = {tz}, expected 0");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// inverse(inverse(T)) ≈ T (round-trip with sign_insensitive on rotation).
    #[test]
    fn transform_inverse_round_trip() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let t = make_transform(q.clone(), 1.5, 2.5, -3.5);
        let inv = eval_builtin("transform_inverse", &[t.clone()]);
        let back = eval_builtin("transform_inverse", &[inv]);
        match back {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 0.5, 0.5, 0.5, 0.5, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.5).abs() < 1e-12, "tx = {tx}, expected 1.5");
                        assert!((ty - 2.5).abs() < 1e-12, "ty = {ty}, expected 2.5");
                        assert!((tz - (-3.5)).abs() < 1e-12, "tz = {tz}, expected -3.5");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// compose(T, inverse(T)) ≈ identity for an arbitrary T.
    #[test]
    fn transform_inverse_compose_t_inv_t_is_identity() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let t = make_transform(q, 1.5, 2.5, -3.5);
        let inv = eval_builtin("transform_inverse", &[t.clone()]);
        let composed = eval_builtin("transform_compose", &[t, inv]);
        match composed {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-10);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected ~0");
                        }
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_inverse with wrong arg count → Undef.
    #[test]
    fn transform_inverse_wrong_arg_count_returns_undef() {
        let t = make_transform(make_identity_orientation(), 0.0, 0.0, 0.0);
        assert!(eval_builtin("transform_inverse", &[]).is_undef());
        assert!(eval_builtin("transform_inverse", &[t.clone(), t]).is_undef());
    }

    /// transform_inverse with non-Transform arg → Undef.
    #[test]
    fn transform_inverse_non_transform_arg_returns_undef() {
        assert!(eval_builtin("transform_inverse", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("transform_inverse", &[make_identity_orientation()]).is_undef());
    }

    // ── transform_log tests (step-19) ────────────────────────────────────────

    /// Helper: extract a Vector3's three f64 components from a Map's value at `key`.
    fn map_vec3_components(map: &Value, key: &str) -> [f64; 3] {
        let map_inner = match map {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        let v = map_inner
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
    fn map_vec3_dim(map: &Value, key: &str) -> DimensionVector {
        let map_inner = match map {
            Value::Map(m) => m,
            other => panic!("expected Map, got {:?}", other),
        };
        let v = map_inner
            .get(&Value::String(key.to_string()))
            .unwrap_or_else(|| panic!("missing key {:?}", key));
        match v {
            Value::Vector(items) if items.len() == 3 => items[0].dimension(),
            other => panic!("expected Vector3 at key {:?}, got {:?}", key, other),
        }
    }

    /// transform_log(identity) == Map { angular=[0,0,0] DIMENSIONLESS, linear=[0,0,0] LENGTH }.
    #[test]
    fn transform_log_identity_is_zero_twist() {
        let id = eval_builtin("transform3_identity", &[]);
        let result = eval_builtin("transform_log", &[id]);
        let ang = map_vec3_components(&result, "angular");
        let lin = map_vec3_components(&result, "linear");
        for (i, v) in ang.iter().enumerate() {
            assert!(v.abs() < 1e-12, "angular[{i}] = {v}, expected 0");
        }
        for (i, v) in lin.iter().enumerate() {
            assert!(v.abs() < 1e-12, "linear[{i}] = {v}, expected 0");
        }
        assert_eq!(map_vec3_dim(&result, "angular"), DimensionVector::DIMENSIONLESS);
        assert_eq!(map_vec3_dim(&result, "linear"), DimensionVector::LENGTH);
    }

    /// Pure translation: T=(identity, [1,2,3] m) → angular=[0,0,0], linear=[1,2,3].
    /// (When ω=0, V=I, so V_inv*t = t.)
    #[test]
    fn transform_log_pure_translation() {
        let t = make_transform(make_identity_orientation(), 1.0, 2.0, 3.0);
        let result = eval_builtin("transform_log", &[t]);
        let ang = map_vec3_components(&result, "angular");
        let lin = map_vec3_components(&result, "linear");
        for (i, v) in ang.iter().enumerate() {
            assert!(v.abs() < 1e-12, "angular[{i}] = {v}, expected 0");
        }
        assert!((lin[0] - 1.0).abs() < 1e-12, "linear[0] = {}, expected 1", lin[0]);
        assert!((lin[1] - 2.0).abs() < 1e-12, "linear[1] = {}, expected 2", lin[1]);
        assert!((lin[2] - 3.0).abs() < 1e-12, "linear[2] = {}, expected 3", lin[2]);
        assert_eq!(map_vec3_dim(&result, "angular"), DimensionVector::DIMENSIONLESS);
        assert_eq!(map_vec3_dim(&result, "linear"), DimensionVector::LENGTH);
    }

    /// Pure 90°z rotation, no translation: angular=[0,0,π/2], linear=[0,0,0].
    #[test]
    fn transform_log_pure_rotation() {
        let t = make_transform(make_rot90z(), 0.0, 0.0, 0.0);
        let result = eval_builtin("transform_log", &[t]);
        let ang = map_vec3_components(&result, "angular");
        let lin = map_vec3_components(&result, "linear");
        let expected_z = std::f64::consts::FRAC_PI_2;
        assert!(ang[0].abs() < 1e-12, "angular[0] = {}, expected 0", ang[0]);
        assert!(ang[1].abs() < 1e-12, "angular[1] = {}, expected 0", ang[1]);
        assert!(
            (ang[2] - expected_z).abs() < 1e-12,
            "angular[2] = {}, expected π/2",
            ang[2]
        );
        for (i, v) in lin.iter().enumerate() {
            assert!(v.abs() < 1e-12, "linear[{i}] = {v}, expected 0");
        }
    }

    /// Small-rotation transform: angular components match rotation vector linearly.
    /// For a small angle ε about axis (0,0,1) with no translation, angular ≈ [0, 0, ε].
    #[test]
    fn transform_log_small_rotation() {
        let eps: f64 = 1e-6;
        // Build a small-z rotation manually: q = (cos(eps/2), 0, 0, sin(eps/2)).
        let half = eps / 2.0;
        let q = Value::Orientation {
            w: half.cos(),
            x: 0.0,
            y: 0.0,
            z: half.sin(),
        };
        let t = make_transform(q, 0.0, 0.0, 0.0);
        let result = eval_builtin("transform_log", &[t]);
        let ang = map_vec3_components(&result, "angular");
        // angular[2] should be ≈ eps within ~1e-12.
        assert!(ang[0].abs() < 1e-10, "angular[0] = {}, expected ~0", ang[0]);
        assert!(ang[1].abs() < 1e-10, "angular[1] = {}, expected ~0", ang[1]);
        assert!(
            (ang[2] - eps).abs() < 1e-12,
            "angular[2] = {}, expected {}",
            ang[2],
            eps
        );
    }

    /// transform_log with wrong arg count → Undef.
    #[test]
    fn transform_log_wrong_arg_count_returns_undef() {
        let t = make_transform(make_identity_orientation(), 0.0, 0.0, 0.0);
        assert!(eval_builtin("transform_log", &[]).is_undef());
        assert!(eval_builtin("transform_log", &[t.clone(), t]).is_undef());
    }

    /// transform_log with non-Transform arg → Undef.
    #[test]
    fn transform_log_non_transform_arg_returns_undef() {
        assert!(eval_builtin("transform_log", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("transform_log", &[make_identity_orientation()]).is_undef());
    }

    // ── transform_exp tests (step-21) ────────────────────────────────────────

    /// Helper: build a twist Map with given angular & linear vectors.
    fn make_twist(angular: [f64; 3], linear: [f64; 3], linear_dim: DimensionVector) -> Value {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![
                Value::Real(angular[0]),
                Value::Real(angular[1]),
                Value::Real(angular[2]),
            ]),
        );
        let make_lin = |v: f64| -> Value {
            if linear_dim.is_dimensionless() {
                Value::Real(v)
            } else {
                Value::Scalar {
                    si_value: v,
                    dimension: linear_dim,
                }
            }
        };
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![make_lin(linear[0]), make_lin(linear[1]), make_lin(linear[2])]),
        );
        Value::Map(m)
    }

    /// transform_exp(zero twist) == identity transform.
    #[test]
    fn transform_exp_zero_twist_is_identity() {
        let zero = make_twist([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], DimensionVector::LENGTH);
        let result = eval_builtin("transform_exp", &[zero]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-12, "translation[{i}] = {v}, expected 0");
                        }
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_exp(angular=[0,0,π/2], linear=0) → 90°z rotation, zero translation.
    #[test]
    fn transform_exp_pure_rotation() {
        let twist = make_twist(
            [0.0, 0.0, std::f64::consts::FRAC_PI_2],
            [0.0, 0.0, 0.0],
            DimensionVector::LENGTH,
        );
        let result = eval_builtin("transform_exp", &[twist]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                let s = std::f64::consts::FRAC_1_SQRT_2;
                assert_orientation_approx!(*rotation, s, 0.0, 0.0, s, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        for (i, item) in items.iter().enumerate() {
                            let v = item.as_f64().unwrap();
                            assert!(v.abs() < 1e-10, "translation[{i}] = {v}, expected 0");
                        }
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_exp(angular=0, linear=[1,2,3]) → (identity, [1,2,3] m).
    #[test]
    fn transform_exp_pure_translation() {
        let twist = make_twist([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], DimensionVector::LENGTH);
        let result = eval_builtin("transform_exp", &[twist]);
        match result {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 1.0, 0.0, 0.0, 0.0, sign_insensitive = 1e-12);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.0).abs() < 1e-12, "tx = {tx}, expected 1");
                        assert!((ty - 2.0).abs() < 1e-12, "ty = {ty}, expected 2");
                        assert!((tz - 3.0).abs() < 1e-12, "tz = {tz}, expected 3");
                        // Verify dimension is LENGTH.
                        assert_eq!(items[0].dimension(), DimensionVector::LENGTH);
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// Round-trip: transform_log(transform_exp(twist)) ≈ twist for several non-trivial twists.
    #[test]
    fn transform_exp_log_round_trip() {
        let twists = [
            ([0.1, 0.2, 0.3], [1.0, 2.0, 3.0]),
            ([0.5, -0.3, 0.7], [-1.0, 0.5, 2.0]),
            ([0.01, 0.02, 0.03], [0.1, 0.1, 0.1]),
        ];
        for (i, (ang, lin)) in twists.iter().enumerate() {
            let twist = make_twist(*ang, *lin, DimensionVector::LENGTH);
            let t = eval_builtin("transform_exp", &[twist]);
            let back = eval_builtin("transform_log", &[t]);
            let ang_back = map_vec3_components(&back, "angular");
            let lin_back = map_vec3_components(&back, "linear");
            for j in 0..3 {
                assert!(
                    (ang_back[j] - ang[j]).abs() < 1e-10,
                    "case {i}: angular[{j}] = {}, expected {}",
                    ang_back[j],
                    ang[j]
                );
                assert!(
                    (lin_back[j] - lin[j]).abs() < 1e-10,
                    "case {i}: linear[{j}] = {}, expected {}",
                    lin_back[j],
                    lin[j]
                );
            }
        }
    }

    /// Round-trip: transform_exp(transform_log(T)) ≈ T (with sign_insensitive on rotation).
    #[test]
    fn transform_log_exp_round_trip() {
        // Use a non-axis-aligned rotation to exercise the general case.
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let t = make_transform(q.clone(), 1.5, -2.5, 3.0);
        let twist = eval_builtin("transform_log", &[t.clone()]);
        let back = eval_builtin("transform_exp", &[twist]);
        match back {
            Value::Transform {
                rotation,
                translation,
            } => {
                assert_orientation_approx!(*rotation, 0.5, 0.5, 0.5, 0.5, sign_insensitive = 1e-10);
                match *translation {
                    Value::Vector(items) if items.len() == 3 => {
                        let tx = items[0].as_f64().unwrap();
                        let ty = items[1].as_f64().unwrap();
                        let tz = items[2].as_f64().unwrap();
                        assert!((tx - 1.5).abs() < 1e-10, "tx = {tx}, expected 1.5");
                        assert!((ty - (-2.5)).abs() < 1e-10, "ty = {ty}, expected -2.5");
                        assert!((tz - 3.0).abs() < 1e-10, "tz = {tz}, expected 3");
                    }
                    other => panic!("expected Vector3, got {:?}", other),
                }
            }
            other => panic!("expected Transform, got {:?}", other),
        }
    }

    /// transform_exp with Map missing "angular" key → Undef.
    #[test]
    fn transform_exp_missing_angular_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with Map missing "linear" key → Undef.
    #[test]
    fn transform_exp_missing_linear_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::Real(0.0); 3]),
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with non-DIMENSIONLESS angular dimension → Undef.
    #[test]
    fn transform_exp_angular_wrong_dimension_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]), // LENGTH instead of DIMENSIONLESS
        );
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::length(0.0); 3]),
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with non-LENGTH linear dimension → Undef.
    #[test]
    fn transform_exp_linear_wrong_dimension_returns_undef() {
        let mut m = std::collections::BTreeMap::new();
        m.insert(
            Value::String("angular".to_string()),
            Value::Vector(vec![Value::Real(0.0); 3]),
        );
        m.insert(
            Value::String("linear".to_string()),
            Value::Vector(vec![Value::angle(0.0); 3]), // ANGLE instead of LENGTH
        );
        assert!(eval_builtin("transform_exp", &[Value::Map(m)]).is_undef());
    }

    /// transform_exp with NaN component → Undef.
    #[test]
    fn transform_exp_nan_angular_returns_undef() {
        let twist = make_twist([f64::NAN, 0.0, 0.0], [0.0, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(eval_builtin("transform_exp", &[twist]).is_undef());
    }

    #[test]
    fn transform_exp_inf_linear_returns_undef() {
        let twist = make_twist([0.0, 0.0, 0.0], [f64::INFINITY, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(eval_builtin("transform_exp", &[twist]).is_undef());
    }

    /// transform_exp with wrong arg count → Undef.
    #[test]
    fn transform_exp_wrong_arg_count_returns_undef() {
        let twist = make_twist([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], DimensionVector::LENGTH);
        assert!(eval_builtin("transform_exp", &[]).is_undef());
        assert!(eval_builtin("transform_exp", &[twist.clone(), twist]).is_undef());
    }
}
