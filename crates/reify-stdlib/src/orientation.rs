use std::collections::BTreeMap;

use reify_core::DimensionVector;
use reify_ir::{Value, quaternion_is_finite};

use crate::helpers::{sanitize_value, tensor_components_f64, trig_input};

pub(crate) fn eval_orientation(name: &str, args: &[Value]) -> Option<Value> {
    Some(match name {
        "orient_identity" => {
            if args.is_empty() {
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                }
            } else {
                Value::Undef
            }
        }
        "orient_quaternion" => {
            if args.len() != 4 {
                return Some(Value::Undef);
            }
            // Quaternion components are pure numbers — reject dimensioned Scalars.
            if args[0].dimension() != DimensionVector::DIMENSIONLESS
                || args[1].dimension() != DimensionVector::DIMENSIONLESS
                || args[2].dimension() != DimensionVector::DIMENSIONLESS
                || args[3].dimension() != DimensionVector::DIMENSIONLESS
            {
                return Some(Value::Undef);
            }
            match (
                args[0].as_f64(),
                args[1].as_f64(),
                args[2].as_f64(),
                args[3].as_f64(),
            ) {
                (Some(w), Some(x), Some(y), Some(z)) => {
                    normalize_quaternion(w, x, y, z).unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        "orient_euler" => {
            if args.len() != 4 {
                return Some(Value::Undef);
            }
            // Accept either a lowercase string or a qualified EulerConvention enum value.
            // The string path is case-sensitive (String "XYZ" → Undef).
            // Enum variants are uppercased in source; we lowercase them to feed the dispatch table.
            let convention_owned: String;
            let convention: &str = match &args[0] {
                Value::String(s) => s.as_str(),
                Value::Enum { type_name, variant } if type_name == "EulerConvention" => {
                    convention_owned = variant.to_lowercase();
                    &convention_owned
                }
                _ => return Some(Value::Undef),
            };
            let a = match trig_input(&args[1]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let b = match trig_input(&args[2]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let c = match trig_input(&args[3]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let axes: [usize; 3] = match convention {
                "xyz" => [0, 1, 2],
                "xzy" => [0, 2, 1],
                "yxz" => [1, 0, 2],
                "yzx" => [1, 2, 0],
                "zxy" => [2, 0, 1],
                "zyx" => [2, 1, 0],
                "xyx" => [0, 1, 0],
                "xzx" => [0, 2, 0],
                "yxy" => [1, 0, 1],
                "yzy" => [1, 2, 1],
                "zxz" => [2, 0, 2],
                "zyz" => [2, 1, 2],
                _ => return Some(Value::Undef),
            };
            // Compose q = q_a * q_b * q_c (intrinsic: multiply left-to-right)
            let q1 = elementary_rotation_quat(axes[0], a);
            let q2 = elementary_rotation_quat(axes[1], b);
            let q3 = elementary_rotation_quat(axes[2], c);
            let q12 = quat_mul(q1, q2);
            let q = quat_mul(q12, q3);
            normalize_quaternion(q.0, q.1, q.2, q.3).unwrap_or(Value::Undef)
        }
        "orient_basis" => {
            if args.len() != 3 {
                return Some(Value::Undef);
            }
            let (xc, _) = match tensor_components_f64(&args[0]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Some(Value::Undef),
            };
            let (yc, _) = match tensor_components_f64(&args[1]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Some(Value::Undef),
            };
            let (zc, _) = match tensor_components_f64(&args[2]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Some(Value::Undef),
            };
            // Defense-in-depth: reject NaN/Inf early
            if xc
                .iter()
                .chain(yc.iter())
                .chain(zc.iter())
                .any(|v| !v.is_finite())
            {
                return Some(Value::Undef);
            }
            // Verify approximate orthonormality
            let tol = 1e-6;
            let mag_x = vec3_norm(xc[0], xc[1], xc[2]);
            let mag_y = vec3_norm(yc[0], yc[1], yc[2]);
            let mag_z = vec3_norm(zc[0], zc[1], zc[2]);
            if (mag_x - 1.0).abs() > tol || (mag_y - 1.0).abs() > tol || (mag_z - 1.0).abs() > tol {
                return Some(Value::Undef);
            }
            let dot_xy = xc[0] * yc[0] + xc[1] * yc[1] + xc[2] * yc[2];
            let dot_xz = xc[0] * zc[0] + xc[1] * zc[1] + xc[2] * zc[2];
            let dot_yz = yc[0] * zc[0] + yc[1] * zc[1] + yc[2] * zc[2];
            if dot_xy.abs() > tol || dot_xz.abs() > tol || dot_yz.abs() > tol {
                return Some(Value::Undef);
            }
            // Verify right-handedness via scalar triple product (determinant).
            let det = xc[0] * (yc[1] * zc[2] - yc[2] * zc[1])
                + xc[1] * (yc[2] * zc[0] - yc[0] * zc[2])
                + xc[2] * (yc[0] * zc[1] - yc[1] * zc[0]);
            if (det - 1.0).abs() > tol {
                return Some(Value::Undef);
            }
            // Rotation matrix from basis vectors
            let r00 = xc[0];
            let r01 = yc[0];
            let r02 = zc[0];
            let r10 = xc[1];
            let r11 = yc[1];
            let r12 = zc[1];
            let r20 = xc[2];
            let r21 = yc[2];
            let r22 = zc[2];
            // Shepperd's method
            let trace = r00 + r11 + r22;
            let (w, x, y, z) = if trace > 0.0 {
                let s = (trace + 1.0).sqrt() * 2.0;
                (0.25 * s, (r21 - r12) / s, (r02 - r20) / s, (r10 - r01) / s)
            } else if r00 > r11 && r00 > r22 {
                let s = (1.0 + r00 - r11 - r22).sqrt() * 2.0;
                ((r21 - r12) / s, 0.25 * s, (r01 + r10) / s, (r02 + r20) / s)
            } else if r11 > r22 {
                let s = (1.0 - r00 + r11 - r22).sqrt() * 2.0;
                ((r02 - r20) / s, (r01 + r10) / s, 0.25 * s, (r12 + r21) / s)
            } else {
                let s = (1.0 - r00 - r11 + r22).sqrt() * 2.0;
                ((r10 - r01) / s, (r02 + r20) / s, (r12 + r21) / s, 0.25 * s)
            };
            normalize_quaternion(w, x, y, z).unwrap_or(Value::Undef)
        }
        "orient_look_at" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let (fc, _) = match tensor_components_f64(&args[0]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Some(Value::Undef),
            };
            let (uc, _) = match tensor_components_f64(&args[1]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Some(Value::Undef),
            };
            // Gram-Schmidt: z = normalize(forward), x = normalize(cross(up, z)), y = cross(z, x)
            let z = match normalize_vec3(fc[0], fc[1], fc[2]) {
                Some(v) => v,
                None => return Some(Value::Undef),
            };
            let x = match normalize_vec3_arr(cross3([uc[0], uc[1], uc[2]], z)) {
                Some(v) => v,
                None => return Some(Value::Undef), // forward ∥ up or zero up
            };
            let y = cross3(z, x);
            // Delegate to the orient_basis arm (reuses Shepperd's method + all guards).
            eval_orientation(
                "orient_basis",
                &[
                    Value::Vector(vec![Value::Real(x[0]), Value::Real(x[1]), Value::Real(x[2])]),
                    Value::Vector(vec![Value::Real(y[0]), Value::Real(y[1]), Value::Real(y[2])]),
                    Value::Vector(vec![Value::Real(z[0]), Value::Real(z[1]), Value::Real(z[2])]),
                ],
            )
            .unwrap_or(Value::Undef)
        }
        "orient_log" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (w, x, y, z) = match &args[0] {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(w, x, y, z) {
                return Some(Value::Undef);
            }
            // log(q) = (axis * angle) where angle = 2*atan2(|v|, w), axis = v/|v|.
            // Near identity (|v| ≈ 0), use leading-order Taylor: log ≈ 2*(x,y,z).
            let v_norm = (x * x + y * y + z * z).sqrt();
            const EPS: f64 = 1e-12;
            let (lx, ly, lz) = if v_norm < EPS {
                (2.0 * x, 2.0 * y, 2.0 * z)
            } else {
                let angle = 2.0 * v_norm.atan2(w);
                let scale = angle / v_norm;
                (scale * x, scale * y, scale * z)
            };
            if !lx.is_finite() || !ly.is_finite() || !lz.is_finite() {
                return Some(Value::Undef);
            }
            Value::Vector(vec![Value::Real(lx), Value::Real(ly), Value::Real(lz)])
        }
        "orient_inverse" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (w, x, y, z) = match &args[0] {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(w, x, y, z) {
                return Some(Value::Undef);
            }
            // For unit quaternion q=(w,x,y,z), inverse equals conjugate (w,-x,-y,-z).
            sanitize_value(normalize_quaternion(w, -x, -y, -z).unwrap_or(Value::Undef))
        }
        "orient_compose" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let a = match &args[0] {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            let b = match &args[1] {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(a.0, a.1, a.2, a.3)
                || !quaternion_is_finite(b.0, b.1, b.2, b.3)
            {
                return Some(Value::Undef);
            }
            let p = quat_mul(a, b);
            sanitize_value(normalize_quaternion(p.0, p.1, p.2, p.3).unwrap_or(Value::Undef))
        }
        "orient_to_euler" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            // Accept either a lowercase string or a qualified EulerConvention enum value.
            // The string path is case-sensitive (String "XYZ" → Undef).
            // Enum variants are uppercased in source; we lowercase them to feed the dispatch table.
            let convention_owned: String;
            let convention: &str = match &args[0] {
                Value::String(s) => s.as_str(),
                Value::Enum { type_name, variant } if type_name == "EulerConvention" => {
                    convention_owned = variant.to_lowercase();
                    &convention_owned
                }
                _ => return Some(Value::Undef),
            };
            let (w, x, y, z) = match &args[1] {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(w, x, y, z) {
                return Some(Value::Undef);
            }
            // Quaternion → rotation matrix R (row-major).
            let r00 = 1.0 - 2.0 * (y * y + z * z);
            let r01 = 2.0 * (x * y - w * z);
            let r02 = 2.0 * (x * z + w * y);
            let r10 = 2.0 * (x * y + w * z);
            let r11 = 1.0 - 2.0 * (x * x + z * z);
            let r12 = 2.0 * (y * z - w * x);
            let r20 = 2.0 * (x * z - w * y);
            let r21 = 2.0 * (y * z + w * x);
            let r22 = 1.0 - 2.0 * (x * x + y * y);
            // Tolerance for asin/acos clamping near singularities. We clamp the
            // input to [-1, 1] for numerical safety, then detect the singularity
            // if the clamped value is within EPS_SING of ±1.
            const EPS_SING: f64 = 1.0e-7;
            let clamp = |v: f64| v.clamp(-1.0, 1.0);
            let (a, b, c) = match convention {
                // ── Tait-Bryan ───────────────────────────────────────────────
                "xyz" => {
                    let s = clamp(r02);
                    let bb = s.asin();
                    if (s.abs() - 1.0).abs() < EPS_SING {
                        // sin(b) ≈ ±1 → cos(b) ≈ 0
                        (0.0, bb, r10.atan2(r11))
                    } else {
                        ((-r12).atan2(r22), bb, (-r01).atan2(r00))
                    }
                }
                "xzy" => {
                    let s = clamp(-r01);
                    let bb = s.asin();
                    if (s.abs() - 1.0).abs() < EPS_SING {
                        (0.0, bb, (-r20).atan2(r22))
                    } else {
                        (r21.atan2(r11), bb, r02.atan2(r00))
                    }
                }
                "yxz" => {
                    let s = clamp(-r12);
                    let bb = s.asin();
                    if (s.abs() - 1.0).abs() < EPS_SING {
                        (0.0, bb, (-r01).atan2(r00))
                    } else {
                        (r02.atan2(r22), bb, r10.atan2(r11))
                    }
                }
                "yzx" => {
                    let s = clamp(r10);
                    let bb = s.asin();
                    if (s.abs() - 1.0).abs() < EPS_SING {
                        (0.0, bb, r21.atan2(r22))
                    } else {
                        ((-r20).atan2(r00), bb, (-r12).atan2(r11))
                    }
                }
                "zxy" => {
                    let s = clamp(r21);
                    let bb = s.asin();
                    if (s.abs() - 1.0).abs() < EPS_SING {
                        (0.0, bb, r02.atan2(r00))
                    } else {
                        ((-r01).atan2(r11), bb, (-r20).atan2(r22))
                    }
                }
                "zyx" => {
                    let s = clamp(-r20);
                    let bb = s.asin();
                    if (s.abs() - 1.0).abs() < EPS_SING {
                        (0.0, bb, (-r12).atan2(r11))
                    } else {
                        (r10.atan2(r00), bb, r21.atan2(r22))
                    }
                }
                // ── Proper Euler ─────────────────────────────────────────────
                "xyx" => {
                    let bb = clamp(r00).acos();
                    if bb.sin().abs() < EPS_SING {
                        // β ≈ 0 or π → singularity. Set α = 0.
                        if r00 > 0.0 {
                            (0.0, bb, r21.atan2(r11))
                        } else {
                            (0.0, bb, (-r21).atan2(r11))
                        }
                    } else {
                        (r10.atan2(-r20), bb, r01.atan2(r02))
                    }
                }
                "xzx" => {
                    let bb = clamp(r00).acos();
                    if bb.sin().abs() < EPS_SING {
                        if r00 > 0.0 {
                            (0.0, bb, (-r12).atan2(r11))
                        } else {
                            (0.0, bb, r12.atan2(r11))
                        }
                    } else {
                        (r20.atan2(r10), bb, r02.atan2(-r01))
                    }
                }
                "yxy" => {
                    let bb = clamp(r11).acos();
                    if bb.sin().abs() < EPS_SING {
                        if r11 > 0.0 {
                            (0.0, bb, (-r20).atan2(r00))
                        } else {
                            (0.0, bb, r20.atan2(r00))
                        }
                    } else {
                        (r01.atan2(r21), bb, r10.atan2(-r12))
                    }
                }
                "yzy" => {
                    let bb = clamp(r11).acos();
                    if bb.sin().abs() < EPS_SING {
                        if r11 > 0.0 {
                            (0.0, bb, r20.atan2(r00))
                        } else {
                            (0.0, bb, (-r20).atan2(r00))
                        }
                    } else {
                        (r21.atan2(-r01), bb, r12.atan2(r10))
                    }
                }
                "zxz" => {
                    let bb = clamp(r22).acos();
                    if bb.sin().abs() < EPS_SING {
                        if r22 > 0.0 {
                            (0.0, bb, r10.atan2(r11))
                        } else {
                            (0.0, bb, (-r10).atan2(r11))
                        }
                    } else {
                        (r02.atan2(-r12), bb, r20.atan2(r21))
                    }
                }
                "zyz" => {
                    let bb = clamp(r22).acos();
                    if bb.sin().abs() < EPS_SING {
                        if r22 > 0.0 {
                            (0.0, bb, r10.atan2(r00))
                        } else {
                            (0.0, bb, (-r10).atan2(r00))
                        }
                    } else {
                        (r12.atan2(r02), bb, r21.atan2(-r20))
                    }
                }
                _ => return Some(Value::Undef),
            };
            if !a.is_finite() || !b.is_finite() || !c.is_finite() {
                return Some(Value::Undef);
            }
            Value::List(vec![Value::angle(a), Value::angle(b), Value::angle(c)])
        }
        "orient_to_axis_angle" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (w, x, y, z) = match &args[0] {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(w, x, y, z) {
                return Some(Value::Undef);
            }
            let v_norm = (x * x + y * y + z * z).sqrt();
            const EPS: f64 = 1e-12;
            let (axis, angle) = if v_norm < EPS {
                // Identity: canonical [1,0,0] axis with zero angle.
                ([1.0, 0.0, 0.0], 0.0)
            } else {
                let a = 2.0 * v_norm.atan2(w);
                ([x / v_norm, y / v_norm, z / v_norm], a)
            };
            let mut m = BTreeMap::new();
            m.insert(Value::String("angle".to_string()), Value::angle(angle));
            m.insert(
                Value::String("axis".to_string()),
                Value::Vector(vec![
                    Value::Real(axis[0]),
                    Value::Real(axis[1]),
                    Value::Real(axis[2]),
                ]),
            );
            Value::Map(m)
        }
        "orient_slerp" => {
            if args.len() != 3 {
                return Some(Value::Undef);
            }
            let (aw, ax, ay, az) = match &args[0] {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            let (mut bw, mut bx, mut by, mut bz) = match &args[1] {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            if !quaternion_is_finite(aw, ax, ay, az) || !quaternion_is_finite(bw, bx, by, bz) {
                return Some(Value::Undef);
            }
            // t must be a pure number (Real or DIMENSIONLESS Scalar). Dimensioned
            // Scalars (e.g. Angle) are rejected.
            if args[2].dimension() != DimensionVector::DIMENSIONLESS {
                return Some(Value::Undef);
            }
            let t = match args[2].as_f64() {
                Some(t) => t,
                None => return Some(Value::Undef),
            };
            if !t.is_finite() {
                return Some(Value::Undef);
            }
            // Choose short-path on the 3-sphere by negating b if dot(a, b) < 0.
            let mut dot = aw * bw + ax * bx + ay * by + az * bz;
            if dot < 0.0 {
                bw = -bw;
                bx = -bx;
                by = -by;
                bz = -bz;
                dot = -dot;
            }
            // Clamp dot to [-1, 1] for numerical safety before acos.
            if dot > 1.0 {
                dot = 1.0;
            }
            const EPS: f64 = 1e-10;
            let (w_a, w_b) = if 1.0 - dot < EPS {
                // Near-collinear: fall back to linear interpolation, normalize after.
                (1.0 - t, t)
            } else {
                let theta = dot.acos();
                let s = theta.sin();
                (((1.0 - t) * theta).sin() / s, (t * theta).sin() / s)
            };
            let w = w_a * aw + w_b * bw;
            let x = w_a * ax + w_b * bx;
            let y = w_a * ay + w_b * by;
            let z = w_a * az + w_b * bz;
            sanitize_value(normalize_quaternion(w, x, y, z).unwrap_or(Value::Undef))
        }
        "orient_exp" => {
            if args.len() != 1 {
                return Some(Value::Undef);
            }
            let (comps, dim) = match tensor_components_f64(&args[0]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Some(Value::Undef),
            };
            if dim != DimensionVector::DIMENSIONLESS {
                return Some(Value::Undef);
            }
            let vx = comps[0];
            let vy = comps[1];
            let vz = comps[2];
            if !vx.is_finite() || !vy.is_finite() || !vz.is_finite() {
                return Some(Value::Undef);
            }
            // exp(omega) = quaternion (cos(|omega|/2), sin(|omega|/2)/|omega| * omega)
            // For |omega| ≈ 0, return identity (sin(half)/angle → 1/2 limit, but we
            // shortcut to avoid 0/0).
            let angle = (vx * vx + vy * vy + vz * vz).sqrt();
            const EPS: f64 = 1e-12;
            if angle < EPS {
                return Some(Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                });
            }
            let half = angle / 2.0;
            let s = half.sin() / angle;
            sanitize_value(
                normalize_quaternion(half.cos(), s * vx, s * vy, s * vz).unwrap_or(Value::Undef),
            )
        }
        "orient_axis_angle" => {
            if args.len() != 2 {
                return Some(Value::Undef);
            }
            let (comps, _dim) = match tensor_components_f64(&args[0]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Some(Value::Undef),
            };
            let theta = match trig_input(&args[1]) {
                Some(t) => t,
                None => return Some(Value::Undef),
            };
            // Normalize axis
            let ax = comps[0];
            let ay = comps[1];
            let az = comps[2];
            let axis_norm = vec3_norm(ax, ay, az);
            if axis_norm == 0.0 || !axis_norm.is_finite() {
                return Some(Value::Undef);
            }
            let nax = ax / axis_norm;
            let nay = ay / axis_norm;
            let naz = az / axis_norm;
            let half = theta / 2.0;
            let c = half.cos();
            let s = half.sin();
            normalize_quaternion(c, s * nax, s * nay, s * naz).unwrap_or(Value::Undef)
        }

        _ => return None,
    })
}

/// Normalize a quaternion (w, x, y, z) to unit length.
pub(crate) fn normalize_quaternion(w: f64, x: f64, y: f64, z: f64) -> Option<Value> {
    if !quaternion_is_finite(w, x, y, z) {
        return None;
    }
    let norm = (w * w + x * x + y * y + z * z).sqrt();
    if norm < f64::EPSILON {
        return None;
    }
    Some(Value::Orientation {
        w: w / norm,
        x: x / norm,
        y: y / norm,
        z: z / norm,
    })
}

/// Create an elementary rotation quaternion for a single axis.
fn elementary_rotation_quat(axis: usize, angle: f64) -> (f64, f64, f64, f64) {
    let half = angle / 2.0;
    let c = half.cos();
    let s = half.sin();
    match axis {
        0 => (c, s, 0.0, 0.0),
        1 => (c, 0.0, s, 0.0),
        2 => (c, 0.0, 0.0, s),
        _ => unreachable!(
            "elementary_rotation_quat called with axis > 2 — axes always come from orient_euler match"
        ),
    }
}

/// Hamilton product of two quaternions.
pub(crate) fn quat_mul(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (
        a.0 * b.0 - a.1 * b.1 - a.2 * b.2 - a.3 * b.3,
        a.0 * b.1 + a.1 * b.0 + a.2 * b.3 - a.3 * b.2,
        a.0 * b.2 - a.1 * b.3 + a.2 * b.0 + a.3 * b.1,
        a.0 * b.3 + a.1 * b.2 - a.2 * b.1 + a.3 * b.0,
    )
}

/// Conjugate of a unit quaternion (equivalent to inverse for unit quaternions).
pub(crate) fn quat_conj(q: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (q.0, -q.1, -q.2, -q.3)
}

/// Rotate a 3D vector by a unit quaternion: q * (0,v) * conj(q).
pub(crate) fn quat_rotate(q: (f64, f64, f64, f64), vx: f64, vy: f64, vz: f64) -> (f64, f64, f64) {
    let v_quat = (0.0, vx, vy, vz);
    let tmp = quat_mul(q, v_quat);
    let result = quat_mul(tmp, quat_conj(q));
    (result.1, result.2, result.3)
}

/// Compute the Euclidean norm (magnitude) of a 3D vector.
#[inline]
fn vec3_norm(x: f64, y: f64, z: f64) -> f64 {
    (x * x + y * y + z * z).sqrt()
}

/// Cross product of two 3D vectors.
#[inline]
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Normalize a 3D vector given as scalars. Returns None if norm is 0 or non-finite.
#[inline]
fn normalize_vec3(x: f64, y: f64, z: f64) -> Option<[f64; 3]> {
    let n = vec3_norm(x, y, z);
    if n == 0.0 || !n.is_finite() {
        return None;
    }
    let inv = 1.0 / n;
    let r = [x * inv, y * inv, z * inv];
    if r.iter().any(|v| !v.is_finite()) {
        return None;
    }
    Some(r)
}

/// Normalize a 3D vector given as an array. Returns None if norm is 0 or non-finite.
#[inline]
fn normalize_vec3_arr(v: [f64; 3]) -> Option<[f64; 3]> {
    normalize_vec3(v[0], v[1], v[2])
}

#[cfg(test)]
mod tests {
    use super::{elementary_rotation_quat, normalize_quaternion};
    use crate::eval_builtin;
    use reify_core::DimensionVector;
    use reify_ir::Value;

    // ── assert_orientation_approx diagnostic tests ──────────────────────────

    #[test]
    fn orient_per_component_diagnostics() {
        // Table-driven replacement for the four per-component diagnostic tests.
        // Each entry: (expected label in panic message, closure that triggers the wrong component).
        let cases: [(&str, fn()); 4] = [
            ("w:", || {
                assert_orientation_approx!(
                    Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    },
                    0.5,
                    0.0,
                    0.0,
                    0.0 // wrong w
                );
            }),
            ("x:", || {
                assert_orientation_approx!(
                    Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    },
                    1.0,
                    0.5,
                    0.0,
                    0.0 // wrong x
                );
            }),
            ("y:", || {
                assert_orientation_approx!(
                    Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    },
                    1.0,
                    0.0,
                    0.5,
                    0.0 // wrong y
                );
            }),
            ("z:", || {
                assert_orientation_approx!(
                    Value::Orientation {
                        w: 1.0,
                        x: 0.0,
                        y: 0.0,
                        z: 0.0
                    },
                    1.0,
                    0.0,
                    0.0,
                    0.5 // wrong z
                );
            }),
        ];
        // Collect all label mismatches rather than short-circuiting, so a regression
        // that breaks multiple components surfaces every broken label in one run.
        // A silent-pass regression (case stops panicking at all) is also surfaced here.
        let mut failures: Vec<String> = Vec::new();
        for (label, case) in cases {
            match std::panic::catch_unwind(case) {
                Err(err) => {
                    let msg = err
                        .downcast_ref::<String>()
                        .map(|s| s.as_str())
                        .or_else(|| err.downcast_ref::<&str>().copied())
                        .unwrap_or("");
                    if !msg.contains(label) {
                        failures.push(format!("label {label:?}: panic message was {msg:?}"));
                    }
                }
                Ok(_) => failures.push(format!("label {label:?}: case did not panic")),
            }
        }
        assert!(
            failures.is_empty(),
            "per-component diagnostic failures:\n{}",
            failures.join("\n")
        );
    }

    // ── assert_orientation_approx tol = tests ───────────────────────────────

    #[test]
    fn explicit_tolerance_loose_passes() {
        // tol=1e-2 allows x=1e-3 to pass; the default 1e-12 tolerance would reject this.
        assert_orientation_approx!(
            Value::Orientation {
                w: 1.0,
                x: 1.0e-3,
                y: 0.0,
                z: 0.0
            },
            1.0,
            0.0,
            0.0,
            0.0,
            tol = 1e-2
        );
    }

    #[test]
    fn explicit_tolerance_tight_panics() {
        // tol=1e-6 is tighter than the x offset of 1e-5 — macro must panic with "x:" in message.
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 1.0,
                    x: 1e-5,
                    y: 0.0,
                    z: 0.0
                },
                1.0,
                0.0,
                0.0,
                0.0,
                tol = 1e-6
            );
        });
        let err = result.expect_err("expected assert_orientation_approx with tight tol to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("x:"),
            "expected panic message to contain 'x:', got: {msg:?}"
        );
    }

    // ── assert_orientation_approx sign_insensitive = tests ──────────────────

    #[test]
    fn sign_insensitive_macro_positive() {
        // Positive-sign identity: should pass with positive-sign expected values.
        assert_orientation_approx!(
            Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            1.0,
            0.0,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
    }

    #[test]
    fn sign_insensitive_macro_negative() {
        // Negated identity quaternion: w=-1,x=0,y=0,z=0 represents the same rotation as identity.
        // The sign-insensitive macro should accept it when expected values are (1,0,0,0).
        assert_orientation_approx!(
            Value::Orientation {
                w: -1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            1.0,
            0.0,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
    }

    #[test]
    fn sign_insensitive_macro_non_trivial_quaternion() {
        // 90° rotation quaternion: (s, s, 0, 0) where s = FRAC_1_SQRT_2.
        // Tests that the sign-flip handles non-zero x component, not just the trivial
        // w-only identity case.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        // Positive form: actual (s, s, 0, 0) should match expected (s, s, 0, 0).
        assert_orientation_approx!(
            Value::Orientation {
                w: s,
                x: s,
                y: 0.0,
                z: 0.0
            },
            s,
            s,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
        // Negated form: actual (-s, -s, 0, 0) should also match expected (s, s, 0, 0).
        assert_orientation_approx!(
            Value::Orientation {
                w: -s,
                x: -s,
                y: 0.0,
                z: 0.0
            },
            s,
            s,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
    }

    #[test]
    fn sign_insensitive_macro_fully_populated_quaternion() {
        // Exercises both pos_ok and neg_ok branches with all four components non-zero.
        // Unlike sign_insensitive_macro_non_trivial_quaternion (y=z=0), the non-zero y/z here
        // force the pos_ok (y - $ey) and (z - $ez) and the neg_ok (y + $ey) and (z + $ez)
        // comparisons to run against non-zero expected operands — coverage not reached by
        // sign_insensitive_macro_positive (expected (1,0,0,0), so $ey=$ez=0).
        assert_orientation_approx!(
            Value::Orientation {
                w: 0.5,
                x: 0.5,
                y: 0.5,
                z: 0.5
            },
            0.5,
            0.5,
            0.5,
            0.5,
            sign_insensitive = 1e-10
        );
        assert_orientation_approx!(
            Value::Orientation {
                w: -0.5,
                x: -0.5,
                y: -0.5,
                z: -0.5
            },
            0.5,
            0.5,
            0.5,
            0.5,
            sign_insensitive = 1e-10
        );
    }

    #[test]
    fn sign_insensitive_macro_rejects_wrong_value() {
        // w=0.5,x=0.5,y=0.5,z=0.5 does not match ±(1,0,0,0) — macro should panic.
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 0.5,
                    x: 0.5,
                    y: 0.5,
                    z: 0.5
                },
                1.0,
                0.0,
                0.0,
                0.0,
                sign_insensitive = 1e-10
            );
        });
        let err = result.expect_err(
            "expected assert_orientation_approx sign_insensitive to panic for wrong value",
        );
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("expected Orientation(\u{b1}"),
            "expected panic message to contain 'expected Orientation(\u{b1}', got: {msg:?}"
        );
        assert!(
            msg.contains("got"),
            "expected panic message to contain 'got', got: {msg:?}"
        );
    }

    // ── orient_identity tests (step-6) ──────────────────────────────────────

    #[test]
    fn orient_identity_no_args() {
        assert_orientation_approx!(eval_builtin("orient_identity", &[]), 1.0, 0.0, 0.0, 0.0);
    }

    #[test]
    fn orient_identity_with_args_returns_undef() {
        assert!(eval_builtin("orient_identity", &[Value::Real(1.0)]).is_undef());
    }

    // ── orient_quaternion tests (step-8) ────────────────────────────────────

    #[test]
    fn orient_quaternion_normalizes_unnormalized() {
        // (2,0,0,0) should normalize to (1,0,0,0)
        assert_orientation_approx!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(2.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            ),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_quaternion_preserves_normalized() {
        assert_orientation_approx!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(1.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            ),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_quaternion_arbitrary_normalizes() {
        // (1,1,1,1) norm = 2, normalized = (0.5, 0.5, 0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(1.0),
                    Value::Real(1.0),
                    Value::Real(1.0),
                    Value::Real(1.0)
                ]
            ),
            0.5,
            0.5,
            0.5,
            0.5
        );
    }

    #[test]
    fn orient_quaternion_zero_returns_undef() {
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_quaternion_nan_returns_undef() {
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(f64::NAN),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_quaternion_inf_returns_undef() {
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Real(f64::INFINITY),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0)
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_quaternion_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_quaternion", &[Value::Real(1.0)]).is_undef());
        assert!(eval_builtin("orient_quaternion", &[]).is_undef());
    }

    // ── orient_axis_angle tests (step-10) ─────────────────────────────────

    #[test]
    fn orient_axis_angle_90deg_around_z() {
        // 90° around Z: q = (cos(π/4), 0, 0, sin(π/4))
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Real(std::f64::consts::FRAC_PI_2);
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    #[test]
    fn orient_axis_angle_180deg_around_x() {
        // 180° around X: q = (cos(π/2), sin(π/2), 0, 0) = (0, 1, 0, 0)
        let axis = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let angle = Value::Real(std::f64::consts::PI);
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            0.0,
            1.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_axis_angle_accepts_angle_scalar() {
        // Same as 90° around Z but angle is an Angle Scalar
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Scalar {
            si_value: std::f64::consts::FRAC_PI_2,
            dimension: DimensionVector::ANGLE,
        };
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    #[test]
    fn orient_axis_angle_zero_axis_returns_undef() {
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let angle = Value::Real(1.0);
        assert!(eval_builtin("orient_axis_angle", &[axis, angle]).is_undef());
    }

    #[test]
    fn orient_axis_angle_non_3d_axis_returns_undef() {
        let axis = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]);
        let angle = Value::Real(1.0);
        assert!(eval_builtin("orient_axis_angle", &[axis, angle]).is_undef());
    }

    #[test]
    fn orient_axis_angle_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_axis_angle", &[]).is_undef());
        assert!(eval_builtin("orient_axis_angle", &[Value::Real(1.0)]).is_undef());
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin(
                "orient_axis_angle",
                &[axis.clone(), Value::Real(1.0), Value::Real(2.0)]
            )
            .is_undef()
        );
    }

    // ── orient_euler tests (step-12) ──────────────────────────────────────

    #[test]
    fn orient_euler_xyz_single_axis() {
        // Intrinsic xyz with (π/2, 0, 0): rotation of π/2 about X
        // = quaternion (cos(π/4), sin(π/4), 0, 0)
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            ),
            cos_pi_4,
            sin_pi_4,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_euler_zyx_single_axis() {
        // Intrinsic zyx with (π/2, 0, 0): rotation of π/2 about Z
        // = quaternion (cos(π/4), 0, 0, sin(π/4))
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("zyx".into()),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            ),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    #[test]
    fn orient_euler_zero_angles_is_identity() {
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            ),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_euler_invalid_convention_returns_undef() {
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("abc".into()),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_euler_non_string_convention_returns_undef() {
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_euler_angle_scalar_accepted() {
        // Same as xyz (π/2, 0, 0) but with Angle Scalar
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Scalar {
                        si_value: std::f64::consts::FRAC_PI_2,
                        dimension: DimensionVector::ANGLE,
                    },
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            ),
            cos_pi_4,
            sin_pi_4,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_euler_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_euler", &[]).is_undef());
        assert!(
            eval_builtin(
                "orient_euler",
                &[Value::String("xyz".into()), Value::Real(0.0),]
            )
            .is_undef()
        );
    }

    // ── orient_euler compound rotation tests (step-16) ───────────────────

    #[test]
    fn orient_euler_xyz_two_nonzero_angles() {
        // orient_euler('xyz', π/2, π/2, 0): q_x(π/2) * q_y(π/2) * q_z(0)
        // Two non-zero angles exercise quat_mul with non-identity operands.
        // Expected: (0.5, 0.5, 0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(0.0),
                ]
            ),
            0.5,
            0.5,
            0.5,
            0.5
        );
    }

    #[test]
    fn orient_euler_zyx_three_nonzero_angles() {
        // orient_euler('zyx', π/3, π/4, π/6): q_z(π/3) * q_y(π/4) * q_x(π/6)
        // Three non-zero angles exercise full three-way quat_mul composition.
        // Analytically computed via Hamilton product of elementary rotations.
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("zyx".into()),
                    Value::Real(std::f64::consts::FRAC_PI_3),
                    Value::Real(std::f64::consts::FRAC_PI_4),
                    Value::Real(std::f64::consts::FRAC_PI_6),
                ]
            ),
            0.822_363_171_905_999_4,
            0.02226002671473384,
            0.43967973954090955,
            0.360_423_405_650_355_9
        );
    }

    #[test]
    fn orient_euler_xzx_proper_euler_compound() {
        // orient_euler('xzx', π/2, π/2, 0): q_x(π/2) * q_z(π/2) * q_x(0)
        // Proper Euler convention with compound rotation.
        // Expected: (0.5, 0.5, -0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xzx".into()),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(std::f64::consts::FRAC_PI_2),
                    Value::Real(0.0),
                ]
            ),
            0.5,
            0.5,
            -0.5,
            0.5
        );
    }

    // ── orient_basis tests (step-14) ──────────────────────────────────────

    #[test]
    fn orient_basis_identity_basis() {
        // Standard basis = identity rotation
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert_orientation_approx!(eval_builtin("orient_basis", &[x, y, z]), 1.0, 0.0, 0.0, 0.0);
    }

    #[test]
    fn orient_basis_90deg_rotated() {
        // 90° rotation around Z: X→Y, Y→-X, Z→Z
        // = quaternion (cos(π/4), 0, 0, sin(π/4))
        let x = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(-1.0), Value::Real(0.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_basis", &[x, y, z]),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    #[test]
    fn orient_basis_non_orthogonal_returns_undef() {
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(1.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(eval_builtin("orient_basis", &[x, y, z]).is_undef());
    }

    #[test]
    fn orient_basis_non_3d_returns_undef() {
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("orient_basis", &[x, y, z]).is_undef());
    }

    #[test]
    fn orient_basis_zero_length_returns_undef() {
        let x = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(eval_builtin("orient_basis", &[x, y, z]).is_undef());
    }

    #[test]
    fn orient_basis_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_basis", &[]).is_undef());
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("orient_basis", &[x]).is_undef());
    }

    // ── orient_basis left-handed rejection tests (step-17) ───────────────

    #[test]
    fn orient_basis_left_handed_reflection_xy_plane_returns_undef() {
        // x=(1,0,0), y=(0,1,0), z=(0,0,-1): reflection through XY plane, det=-1
        // Orthonormal but left-handed — must return Undef (not in SO(3)).
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(-1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "left-handed basis (z-reflection) should be rejected"
        );
    }

    #[test]
    fn orient_basis_left_handed_swapped_yz_returns_undef() {
        // x=(1,0,0), y=(0,0,1), z=(0,1,0): another left-handed basis, det=-1
        // Y and Z swapped relative to right-handed standard.
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "left-handed basis (y-z swap) should be rejected"
        );
    }

    #[test]
    fn orient_basis_right_handed_near_tolerance_passes() {
        // A valid right-handed basis that's slightly off from exact (within tolerance).
        // Should still produce a valid orientation.
        let eps = 1e-8; // well within the 1e-6 tolerance
        let x = Value::Tensor(vec![
            Value::Real(1.0 - eps),
            Value::Real(eps),
            Value::Real(0.0),
        ]);
        let y = Value::Tensor(vec![
            Value::Real(-eps),
            Value::Real(1.0 - eps),
            Value::Real(0.0),
        ]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let result = eval_builtin("orient_basis", &[x, y, z]);
        assert!(
            !result.is_undef(),
            "right-handed basis near tolerance should produce valid orientation, got {:?}",
            result
        );
    }

    // ── orient NaN/Inf/edge-case tests (task-359) ─────────────────────────

    #[test]
    fn orient_euler_uppercase_convention_returns_undef() {
        // Convention matching is case-sensitive: 'XYZ' is not recognized, only 'xyz'.
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("XYZ".into()),
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef(),
            "uppercase convention 'XYZ' should be rejected"
        );
    }

    #[test]
    fn orient_basis_nan_component_returns_undef() {
        // NaN in a basis vector must be rejected — NaN bypasses IEEE 754 comparisons.
        let x = Value::Tensor(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "NaN component should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_nan_angle_returns_undef() {
        // NaN angle must be rejected — trig_input should guard against non-finite values.
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Real(f64::NAN);
        assert!(
            eval_builtin("orient_axis_angle", &[axis, angle]).is_undef(),
            "NaN angle should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_inf_angle_returns_undef() {
        // Inf angle must be rejected — cos/sin of Inf produce NaN.
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Real(f64::INFINITY);
        assert!(
            eval_builtin("orient_axis_angle", &[axis, angle]).is_undef(),
            "Inf angle should be rejected"
        );
    }

    #[test]
    fn orient_euler_nan_angle_returns_undef() {
        // NaN angle must be rejected in orient_euler.
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(f64::NAN),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef(),
            "NaN euler angle should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_non_unit_axis_normalizes() {
        // orient_axis_angle normalizes the axis vector — [2,0,0] with π/2 should
        // produce the same rotation as [1,0,0] with π/2: q = (cos(π/4), sin(π/4), 0, 0)
        let axis_scaled = Value::Tensor(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let axis_unit = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let angle = Value::Real(std::f64::consts::FRAC_PI_2);
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis_scaled, angle.clone()]),
            cos_pi_4,
            sin_pi_4,
            0.0,
            0.0
        );
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis_unit, angle]),
            cos_pi_4,
            sin_pi_4,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_quaternion_dimensioned_scalar_returns_undef() {
        // Dimensioned Scalars (e.g. LENGTH) must be rejected — quaternion components
        // are pure numbers and should not carry physical dimensions.
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::LENGTH,
                    },
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_quaternion_accepts_dimensionless_scalar() {
        // Dimensionless Scalars should be accepted — they are pure numbers.
        assert_orientation_approx!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::DIMENSIONLESS,
                    },
                ]
            ),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    #[test]
    fn orient_quaternion_rejects_angle_dimension() {
        // ANGLE-dimensioned Scalars must also be rejected — quaternion components
        // are dimensionless, not angles.
        assert!(
            eval_builtin(
                "orient_quaternion",
                &[
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::ANGLE,
                    },
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_euler_inf_angle_returns_undef() {
        // Inf angle must be rejected in orient_euler.
        assert!(
            eval_builtin(
                "orient_euler",
                &[
                    Value::String("xyz".into()),
                    Value::Real(f64::INFINITY),
                    Value::Real(0.0),
                    Value::Real(0.0),
                ]
            )
            .is_undef(),
            "Inf euler angle should be rejected"
        );
    }

    #[test]
    fn orient_basis_inf_component_returns_undef() {
        // Inf in a basis vector must be rejected — magnitude would be Inf, not ≈1.
        let x = Value::Tensor(vec![
            Value::Real(f64::INFINITY),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "Inf component should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_nan_axis_returns_undef() {
        // NaN in axis must be rejected — vec3_norm(NaN, 0, 0) = sqrt(NaN) = NaN, not finite.
        let axis = Value::Tensor(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let angle = Value::Real(std::f64::consts::FRAC_PI_2);
        assert!(
            eval_builtin("orient_axis_angle", &[axis, angle]).is_undef(),
            "NaN axis component should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_inf_axis_returns_undef() {
        // Inf in axis must be rejected — vec3_norm(Inf, 0, 0) = sqrt(Inf) = Inf, not finite.
        let axis = Value::Tensor(vec![
            Value::Real(f64::INFINITY),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let angle = Value::Real(std::f64::consts::FRAC_PI_2);
        assert!(
            eval_builtin("orient_axis_angle", &[axis, angle]).is_undef(),
            "Inf axis component should be rejected"
        );
    }

    #[test]
    fn orient_basis_non_unit_vector_returns_undef() {
        // Orthogonal but non-unit x=[2,0,0] must be rejected — isolates the magnitude
        // check (|x|=2.0, |2.0-1.0|=1.0 > 1e-6) from the orthogonality check.
        let x = Value::Tensor(vec![Value::Real(2.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "Non-unit basis vector should be rejected"
        );
    }

    #[test]
    fn orient_basis_non_unit_y_returns_undef() {
        // Orthogonal but non-unit y=[0,2,0] must be rejected — isolates the mag_y
        // branch of the unit-length guard (|y|=2.0, |2.0-1.0|=1.0 > 1e-6).
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "Non-unit y basis vector should be rejected"
        );
    }

    #[test]
    fn orient_basis_non_unit_z_returns_undef() {
        // Orthogonal but non-unit z=[0,0,2] must be rejected — isolates the mag_z
        // branch of the unit-length guard (|z|=2.0, |2.0-1.0|=1.0 > 1e-6).
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(2.0)]);
        assert!(
            eval_builtin("orient_basis", &[x, y, z]).is_undef(),
            "Non-unit z basis vector should be rejected"
        );
    }

    #[test]
    fn orient_axis_angle_integer_angle_accepted() {
        // Value::Int(1) = 1 radian, exercises the Value::Int(i) => Some(*i as f64) arm
        // in trig_input. Expected: half=0.5, q=(cos(0.5), 0, 0, sin(0.5)).
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Int(1);
        let half = 0.5_f64;
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            half.cos(),
            0.0,
            0.0,
            half.sin()
        );
    }

    #[test]
    fn orient_axis_angle_integer_angle_zero_is_identity() {
        // Value::Int(0) = 0 radians, exercises the zero-angle boundary of
        // half-angle trig: cos(0)=1, sin(0)=0 → identity quaternion (1,0,0,0).
        let axis = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        let angle = Value::Int(0);
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            1.0,
            0.0,
            0.0,
            0.0
        );
    }

    // ── orient_compose tests (step-1) ──────────────────────────────────────

    /// Identity composed on the left should yield the right operand.
    #[test]
    fn orient_compose_identity_left() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_orientation_approx!(eval_builtin("orient_compose", &[id, q]), 0.5, 0.5, 0.5, 0.5);
    }

    /// Identity composed on the right should yield the left operand.
    #[test]
    fn orient_compose_identity_right() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_orientation_approx!(eval_builtin("orient_compose", &[q, id]), 0.5, 0.5, 0.5, 0.5);
    }

    /// Composing two 90° rotations about the same axis yields a 180° rotation
    /// about that axis. For axis=z: q90 = (cos(π/4), 0, 0, sin(π/4)),
    /// q180 = (cos(π/2), 0, 0, sin(π/2)) = (0, 0, 0, 1).
    /// Sign-insensitive because the macro must absorb the antipodal double-cover.
    #[test]
    fn orient_compose_two_90deg_z_equals_180deg_z() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let q90 = Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        };
        assert_orientation_approx!(
            eval_builtin("orient_compose", &[q90.clone(), q90]),
            0.0,
            0.0,
            0.0,
            1.0,
            sign_insensitive = 1e-12
        );
    }

    #[test]
    fn orient_compose_wrong_arg_count_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_compose", &[]).is_undef());
        assert!(eval_builtin("orient_compose", std::slice::from_ref(&q)).is_undef());
        assert!(
            eval_builtin("orient_compose", &[q.clone(), q.clone(), q]).is_undef(),
            "3 args should return Undef"
        );
    }

    #[test]
    fn orient_compose_non_orientation_first_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_compose", &[Value::Real(1.0), q]).is_undef());
    }

    #[test]
    fn orient_compose_non_orientation_second_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_compose", &[q, Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn orient_compose_nan_component_returns_undef() {
        let nan_q = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_compose", &[nan_q, q]).is_undef());
    }

    #[test]
    fn orient_compose_inf_component_returns_undef() {
        let inf_q = Value::Orientation {
            w: 1.0,
            x: f64::INFINITY,
            y: 0.0,
            z: 0.0,
        };
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_compose", &[q, inf_q]).is_undef());
    }

    // ── orient_inverse tests (step-3) ──────────────────────────────────────

    #[test]
    fn orient_inverse_identity_is_identity() {
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_orientation_approx!(eval_builtin("orient_inverse", &[id]), 1.0, 0.0, 0.0, 0.0);
    }

    /// Inverse of 90°z = (cos(π/4), 0, 0, sin(π/4)) is (cos(π/4), 0, 0, -sin(π/4)),
    /// representing -90°z (axis-angle equivalent of conjugate for a unit quaternion).
    #[test]
    fn orient_inverse_90deg_z_is_conjugate() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let q90z = Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        };
        assert_orientation_approx!(eval_builtin("orient_inverse", &[q90z]), s, 0.0, 0.0, -s);
    }

    /// q ∘ inverse(q) ≈ identity (sign-insensitive due to double-cover).
    #[test]
    fn orient_inverse_compose_q_inv_q_is_identity() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let q_inv = eval_builtin("orient_inverse", std::slice::from_ref(&q));
        assert_orientation_approx!(
            eval_builtin("orient_compose", &[q, q_inv]),
            1.0,
            0.0,
            0.0,
            0.0,
            sign_insensitive = 1e-12
        );
    }

    #[test]
    fn orient_inverse_wrong_arg_count_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_inverse", &[]).is_undef());
        assert!(
            eval_builtin("orient_inverse", &[q.clone(), q]).is_undef(),
            "2 args should return Undef"
        );
    }

    #[test]
    fn orient_inverse_non_orientation_returns_undef() {
        assert!(eval_builtin("orient_inverse", &[Value::Real(1.0)]).is_undef());
    }

    // ── orient_log tests (step-5) ──────────────────────────────────────────

    #[test]
    fn orient_log_identity_is_zero_vector() {
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_vector3_approx!(Vector, eval_builtin("orient_log", &[id]), [0.0, 0.0, 0.0]);
    }

    /// log of 90°z = (cos(π/4), 0, 0, sin(π/4)) is [0, 0, π/2].
    #[test]
    fn orient_log_90deg_z_returns_z_pi_half() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let q90z = Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        };
        assert_vector3_approx!(
            Vector,
            eval_builtin("orient_log", &[q90z]),
            [0.0, 0.0, std::f64::consts::FRAC_PI_2]
        );
    }

    /// log of 180°x = (0, 1, 0, 0) is [π, 0, 0]. Tests the boundary case w=0.
    #[test]
    fn orient_log_180deg_x_returns_x_pi() {
        let q180x = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        assert_vector3_approx!(
            Vector,
            eval_builtin("orient_log", &[q180x]),
            [std::f64::consts::PI, 0.0, 0.0]
        );
    }

    /// Near-identity quaternion (small angle) — Taylor fallback should produce
    /// finite values approximately equal to 2*(x,y,z).
    #[test]
    fn orient_log_near_identity_uses_taylor_fallback() {
        // q ≈ (1, 5e-9, 0, 0) — w stays close to 1, x is tiny but the rotation
        // vector should be roughly 2*x = 1e-8.
        let q = Value::Orientation {
            w: 1.0,
            x: 5e-9,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("orient_log", &[q]);
        match result {
            Value::Vector(items) if items.len() == 3 => {
                let v0 = items[0].as_f64().unwrap();
                let v1 = items[1].as_f64().unwrap();
                let v2 = items[2].as_f64().unwrap();
                assert!(
                    v0.is_finite() && v1.is_finite() && v2.is_finite(),
                    "near-identity log must be finite, got [{v0}, {v1}, {v2}]"
                );
                // Leading-order Taylor: log ≈ 2*(x,y,z); verify within 1% relative tolerance
                assert!(
                    (v0 - 1e-8).abs() < 1e-9,
                    "near-identity x-component expected ~1e-8 got {v0}"
                );
                assert!(v1.abs() < 1e-15);
                assert!(v2.abs() < 1e-15);
            }
            other => panic!("expected Vector(3), got {:?}", other),
        }
    }

    #[test]
    fn orient_log_wrong_arg_count_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_log", &[]).is_undef());
        assert!(eval_builtin("orient_log", &[q.clone(), q]).is_undef());
    }

    #[test]
    fn orient_log_non_orientation_returns_undef() {
        assert!(eval_builtin("orient_log", &[Value::Real(1.0)]).is_undef());
    }

    // ── orient_exp tests (step-7) ──────────────────────────────────────────

    #[test]
    fn orient_exp_zero_vector_is_identity() {
        let zero = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert_orientation_approx!(eval_builtin("orient_exp", &[zero]), 1.0, 0.0, 0.0, 0.0);
    }

    /// exp([0,0,π/2]) = (cos(π/4), 0, 0, sin(π/4)) — 90°z rotation.
    #[test]
    fn orient_exp_z_pi_half_is_90deg_z_quaternion() {
        let v = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(std::f64::consts::FRAC_PI_2),
        ]);
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_exp", &[v]),
            cos_pi_4,
            0.0,
            0.0,
            sin_pi_4
        );
    }

    /// log(exp(v)) ≈ v for several non-trivial rotation vectors.
    #[test]
    fn orient_exp_then_log_round_trip() {
        let cases: [[f64; 3]; 4] = [
            [0.1, 0.2, 0.3],
            [1.0, 0.0, 0.0],
            [0.0, std::f64::consts::FRAC_PI_2, 0.0],
            [-0.5, 0.7, -0.3],
        ];
        for case in cases.iter() {
            let v = Value::Vector(vec![
                Value::Real(case[0]),
                Value::Real(case[1]),
                Value::Real(case[2]),
            ]);
            let q = eval_builtin("orient_exp", std::slice::from_ref(&v));
            let v_back = eval_builtin("orient_log", &[q]);
            match v_back {
                Value::Vector(items) if items.len() == 3 => {
                    let v0 = items[0].as_f64().unwrap();
                    let v1 = items[1].as_f64().unwrap();
                    let v2 = items[2].as_f64().unwrap();
                    assert!(
                        (v0 - case[0]).abs() < 1e-10,
                        "round-trip x: expected {} got {}",
                        case[0],
                        v0
                    );
                    assert!(
                        (v1 - case[1]).abs() < 1e-10,
                        "round-trip y: expected {} got {}",
                        case[1],
                        v1
                    );
                    assert!(
                        (v2 - case[2]).abs() < 1e-10,
                        "round-trip z: expected {} got {}",
                        case[2],
                        v2
                    );
                }
                other => panic!("expected Vector(3), got {:?}", other),
            }
        }
    }

    /// exp(log(q)) ≈ q for arbitrary q (sign-insensitive).
    #[test]
    fn orient_log_then_exp_round_trip() {
        let q = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let v = eval_builtin("orient_log", std::slice::from_ref(&q));
        let q_back = eval_builtin("orient_exp", &[v]);
        assert_orientation_approx!(q_back, 0.5, 0.5, 0.5, 0.5, sign_insensitive = 1e-12);
    }

    #[test]
    fn orient_exp_wrong_arg_count_returns_undef() {
        let v = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("orient_exp", &[]).is_undef());
        assert!(eval_builtin("orient_exp", &[v.clone(), v]).is_undef());
    }

    #[test]
    fn orient_exp_non_vector_returns_undef() {
        assert!(eval_builtin("orient_exp", &[Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn orient_exp_non_3d_vector_returns_undef() {
        let v2 = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]);
        assert!(eval_builtin("orient_exp", &[v2]).is_undef());
    }

    #[test]
    fn orient_exp_nan_component_returns_undef() {
        let nan_v = Value::Vector(vec![
            Value::Real(f64::NAN),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(eval_builtin("orient_exp", &[nan_v]).is_undef());
    }

    #[test]
    fn orient_exp_inf_component_returns_undef() {
        let inf_v = Value::Vector(vec![
            Value::Real(0.0),
            Value::Real(f64::INFINITY),
            Value::Real(0.0),
        ]);
        assert!(eval_builtin("orient_exp", &[inf_v]).is_undef());
    }

    // ── orient_slerp tests (step-9) ────────────────────────────────────────

    /// slerp(a, b, 0) == a (start endpoint).
    #[test]
    fn orient_slerp_t_zero_returns_a() {
        let a = Value::Orientation {
            w: 0.5,
            x: 0.5,
            y: 0.5,
            z: 0.5,
        };
        let b = Value::Orientation {
            w: std::f64::consts::FRAC_1_SQRT_2,
            x: 0.0,
            y: 0.0,
            z: std::f64::consts::FRAC_1_SQRT_2,
        };
        assert_orientation_approx!(
            eval_builtin("orient_slerp", &[a, b, Value::Real(0.0)]),
            0.5,
            0.5,
            0.5,
            0.5,
            sign_insensitive = 1e-12
        );
    }

    /// slerp(a, b, 1) == b (end endpoint).
    #[test]
    fn orient_slerp_t_one_returns_b() {
        let a = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let bw = std::f64::consts::FRAC_1_SQRT_2;
        let b = Value::Orientation {
            w: bw,
            x: 0.0,
            y: 0.0,
            z: bw,
        };
        assert_orientation_approx!(
            eval_builtin("orient_slerp", &[a, b, Value::Real(1.0)]),
            bw,
            0.0,
            0.0,
            bw,
            sign_insensitive = 1e-12
        );
    }

    /// slerp(identity, 90°z, 0.5) == 45°z quaternion (cos(π/8), 0, 0, sin(π/8)).
    #[test]
    fn orient_slerp_midpoint_identity_to_90deg_z_is_45deg_z() {
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let q90 = Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        };
        let cos_pi_8 = (std::f64::consts::PI / 8.0).cos();
        let sin_pi_8 = (std::f64::consts::PI / 8.0).sin();
        assert_orientation_approx!(
            eval_builtin("orient_slerp", &[id, q90, Value::Real(0.5)]),
            cos_pi_8,
            0.0,
            0.0,
            sin_pi_8,
            sign_insensitive = 1e-10
        );
    }

    /// slerp with antipodal endpoints: slerp(identity, -identity, 0.5) takes the
    /// short path → returned quaternion is close to identity (not far from it).
    #[test]
    fn orient_slerp_antipodal_short_path() {
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let neg_id = Value::Orientation {
            w: -1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        // Antipodal pair represents the same rotation; midpoint should be close
        // to identity (sign-insensitive).
        assert_orientation_approx!(
            eval_builtin("orient_slerp", &[id, neg_id, Value::Real(0.5)]),
            1.0,
            0.0,
            0.0,
            0.0,
            sign_insensitive = 1e-10
        );
    }

    /// t accepts a DIMENSIONLESS Scalar in addition to a Real.
    #[test]
    fn orient_slerp_accepts_dimensionless_scalar_t() {
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let q90 = Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        };
        let t = Value::Scalar {
            si_value: 0.5,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let cos_pi_8 = (std::f64::consts::PI / 8.0).cos();
        let sin_pi_8 = (std::f64::consts::PI / 8.0).sin();
        assert_orientation_approx!(
            eval_builtin("orient_slerp", &[id, q90, t]),
            cos_pi_8,
            0.0,
            0.0,
            sin_pi_8,
            sign_insensitive = 1e-10
        );
    }

    #[test]
    fn orient_slerp_wrong_arg_count_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_slerp", &[]).is_undef());
        assert!(eval_builtin("orient_slerp", std::slice::from_ref(&q)).is_undef());
        assert!(eval_builtin("orient_slerp", &[q.clone(), q.clone()]).is_undef());
        assert!(
            eval_builtin(
                "orient_slerp",
                &[q.clone(), q.clone(), Value::Real(0.5), Value::Real(0.0)],
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_slerp_non_orientation_a_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_slerp", &[Value::Real(1.0), q, Value::Real(0.5)]).is_undef());
    }

    #[test]
    fn orient_slerp_non_orientation_b_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_slerp", &[q, Value::Real(1.0), Value::Real(0.5)]).is_undef());
    }

    #[test]
    fn orient_slerp_non_numeric_t_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            eval_builtin(
                "orient_slerp",
                &[q.clone(), q, Value::String("half".to_string())],
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_slerp_dimensioned_t_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let t_angle = Value::Scalar {
            si_value: 0.5,
            dimension: DimensionVector::ANGLE,
        };
        assert!(eval_builtin("orient_slerp", &[q.clone(), q, t_angle]).is_undef());
    }

    #[test]
    fn orient_slerp_nan_t_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_slerp", &[q.clone(), q, Value::Real(f64::NAN)]).is_undef());
    }

    #[test]
    fn orient_slerp_inf_t_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            eval_builtin("orient_slerp", &[q.clone(), q, Value::Real(f64::INFINITY)]).is_undef()
        );
    }

    // ── orient_to_axis_angle tests (step-11) ───────────────────────────────

    /// Helper: extract (axis_components, angle_si) from an axis-angle Map.
    fn axis_angle_extract(v: &Value) -> Option<([f64; 3], f64)> {
        let m = match v {
            Value::Map(m) => m,
            _ => return None,
        };
        let axis_v = m.get(&Value::String("axis".to_string()))?;
        let angle_v = m.get(&Value::String("angle".to_string()))?;
        let comps = match axis_v {
            Value::Vector(items) | Value::Tensor(items) | Value::Point(items)
                if items.len() == 3 =>
            {
                [items[0].as_f64()?, items[1].as_f64()?, items[2].as_f64()?]
            }
            _ => return None,
        };
        // angle should be Angle Scalar
        let angle = match angle_v {
            Value::Scalar {
                si_value,
                dimension,
            } if *dimension == DimensionVector::ANGLE => *si_value,
            _ => return None,
        };
        Some((comps, angle))
    }

    #[test]
    fn orient_to_axis_angle_identity_canonical_fallback() {
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("orient_to_axis_angle", &[id]);
        let (axis, angle) = axis_angle_extract(&result)
            .unwrap_or_else(|| panic!("expected axis-angle Map, got {:?}", result));
        assert!((axis[0] - 1.0).abs() < 1e-12);
        assert!(axis[1].abs() < 1e-12);
        assert!(axis[2].abs() < 1e-12);
        assert!(angle.abs() < 1e-12);
    }

    #[test]
    fn orient_to_axis_angle_90deg_z() {
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let q = Value::Orientation {
            w: s,
            x: 0.0,
            y: 0.0,
            z: s,
        };
        let result = eval_builtin("orient_to_axis_angle", &[q]);
        let (axis, angle) = axis_angle_extract(&result)
            .unwrap_or_else(|| panic!("expected axis-angle Map, got {:?}", result));
        assert!(axis[0].abs() < 1e-12);
        assert!(axis[1].abs() < 1e-12);
        assert!((axis[2] - 1.0).abs() < 1e-12);
        assert!((angle - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    }

    #[test]
    fn orient_to_axis_angle_180deg_x_boundary() {
        // 180°x: q = (0, 1, 0, 0), w=0 boundary
        let q = Value::Orientation {
            w: 0.0,
            x: 1.0,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("orient_to_axis_angle", &[q]);
        let (axis, angle) = axis_angle_extract(&result)
            .unwrap_or_else(|| panic!("expected axis-angle Map, got {:?}", result));
        assert!((axis[0] - 1.0).abs() < 1e-12);
        assert!(axis[1].abs() < 1e-12);
        assert!(axis[2].abs() < 1e-12);
        assert!((angle - std::f64::consts::PI).abs() < 1e-12);
    }

    #[test]
    fn orient_to_axis_angle_round_trip() {
        // For each (axis, angle) input, build q via orient_axis_angle, then decompose
        // and verify the recovered axis/angle match within tolerance.
        let cases: [([f64; 3], f64); 4] = [
            ([0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2),
            ([1.0, 0.0, 0.0], std::f64::consts::FRAC_PI_4),
            ([0.0, 1.0, 0.0], 1.234),
            (
                {
                    // Normalized arbitrary axis (1,2,3)
                    let n = (1.0_f64.powi(2) + 2.0_f64.powi(2) + 3.0_f64.powi(2)).sqrt();
                    [1.0 / n, 2.0 / n, 3.0 / n]
                },
                0.7,
            ),
        ];
        for (axis_in, angle_in) in cases.iter() {
            let axis_v = Value::Tensor(vec![
                Value::Real(axis_in[0]),
                Value::Real(axis_in[1]),
                Value::Real(axis_in[2]),
            ]);
            let q = eval_builtin("orient_axis_angle", &[axis_v, Value::Real(*angle_in)]);
            let result = eval_builtin("orient_to_axis_angle", &[q]);
            let (axis_out, angle_out) = axis_angle_extract(&result)
                .unwrap_or_else(|| panic!("expected axis-angle Map, got {:?}", result));
            assert!(
                (axis_out[0] - axis_in[0]).abs() < 1e-10
                    && (axis_out[1] - axis_in[1]).abs() < 1e-10
                    && (axis_out[2] - axis_in[2]).abs() < 1e-10,
                "axis round-trip: in={:?} out={:?}",
                axis_in,
                axis_out
            );
            assert!(
                (angle_out - angle_in).abs() < 1e-10,
                "angle round-trip: in={} out={}",
                angle_in,
                angle_out
            );
        }
    }

    #[test]
    fn orient_to_axis_angle_wrong_arg_count_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_to_axis_angle", &[]).is_undef());
        assert!(eval_builtin("orient_to_axis_angle", &[q.clone(), q]).is_undef());
    }

    #[test]
    fn orient_to_axis_angle_non_orientation_returns_undef() {
        assert!(eval_builtin("orient_to_axis_angle", &[Value::Real(1.0)]).is_undef());
    }

    // ── orient_to_euler tests (step-13) ────────────────────────────────────

    /// Helper: extract three Angle Scalars (radians) from a List<Angle> result.
    fn euler_extract(v: &Value) -> Option<[f64; 3]> {
        let items = match v {
            Value::List(items) if items.len() == 3 => items,
            _ => return None,
        };
        let mut out = [0.0; 3];
        for (i, item) in items.iter().enumerate() {
            match item {
                Value::Scalar {
                    si_value,
                    dimension,
                } if *dimension == DimensionVector::ANGLE => {
                    out[i] = *si_value;
                }
                _ => return None,
            }
        }
        Some(out)
    }

    #[test]
    fn orient_to_euler_identity_xyz_zero_angles() {
        let id = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let result = eval_builtin("orient_to_euler", &[Value::String("xyz".to_string()), id]);
        let angles = euler_extract(&result)
            .unwrap_or_else(|| panic!("expected List<Angle> of 3, got {:?}", result));
        assert!(angles[0].abs() < 1e-12);
        assert!(angles[1].abs() < 1e-12);
        assert!(angles[2].abs() < 1e-12);
    }

    #[test]
    fn orient_to_euler_xyz_round_trip() {
        let a = std::f64::consts::FRAC_PI_4;
        let b = std::f64::consts::FRAC_PI_6;
        let c = std::f64::consts::FRAC_PI_3;
        let q = eval_builtin(
            "orient_euler",
            &[
                Value::String("xyz".to_string()),
                Value::Real(a),
                Value::Real(b),
                Value::Real(c),
            ],
        );
        let result = eval_builtin("orient_to_euler", &[Value::String("xyz".to_string()), q]);
        let angles = euler_extract(&result)
            .unwrap_or_else(|| panic!("expected List<Angle> of 3, got {:?}", result));
        assert!(
            (angles[0] - a).abs() < 1e-10,
            "xyz a: in={} out={}",
            a,
            angles[0]
        );
        assert!(
            (angles[1] - b).abs() < 1e-10,
            "xyz b: in={} out={}",
            b,
            angles[1]
        );
        assert!(
            (angles[2] - c).abs() < 1e-10,
            "xyz c: in={} out={}",
            c,
            angles[2]
        );
    }

    #[test]
    fn orient_to_euler_zyx_round_trip_three_nonzero() {
        let a = 0.3;
        let b = 0.5;
        let c = -0.7;
        let q = eval_builtin(
            "orient_euler",
            &[
                Value::String("zyx".to_string()),
                Value::Real(a),
                Value::Real(b),
                Value::Real(c),
            ],
        );
        let result = eval_builtin("orient_to_euler", &[Value::String("zyx".to_string()), q]);
        let angles = euler_extract(&result)
            .unwrap_or_else(|| panic!("expected List<Angle> of 3, got {:?}", result));
        assert!((angles[0] - a).abs() < 1e-10);
        assert!((angles[1] - b).abs() < 1e-10);
        assert!((angles[2] - c).abs() < 1e-10);
    }

    #[test]
    fn orient_to_euler_invalid_convention_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_to_euler", &[Value::String("abc".to_string()), q]).is_undef());
    }

    #[test]
    fn orient_to_euler_wrong_arg_count_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_to_euler", &[]).is_undef());
        assert!(eval_builtin("orient_to_euler", &[Value::String("xyz".to_string())]).is_undef());
        assert!(
            eval_builtin(
                "orient_to_euler",
                &[Value::String("xyz".to_string()), q.clone(), q]
            )
            .is_undef()
        );
    }

    #[test]
    fn orient_to_euler_non_string_convention_returns_undef() {
        let q = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert!(eval_builtin("orient_to_euler", &[Value::Real(1.0), q]).is_undef());
    }

    /// Gimbal-lock case for "xyz" Tait-Bryan: middle angle b = π/2.
    /// At this singularity the decomposition is non-unique; the implementation
    /// must return a deterministic triple whose recomposition equals the
    /// original quaternion (sign-insensitive).
    #[test]
    fn orient_to_euler_xyz_gimbal_lock_deterministic_recomposition() {
        let pi_2 = std::f64::consts::FRAC_PI_2;
        let q = eval_builtin(
            "orient_euler",
            &[
                Value::String("xyz".to_string()),
                Value::Real(0.3),
                Value::Real(pi_2),
                Value::Real(0.7),
            ],
        );
        // Capture quaternion components for later comparison.
        let (qw, qx, qy, qz) = match q.clone() {
            Value::Orientation { w, x, y, z } => (w, x, y, z),
            other => panic!("expected Orientation, got {:?}", other),
        };
        let result = eval_builtin("orient_to_euler", &[Value::String("xyz".to_string()), q]);
        let angles = euler_extract(&result)
            .unwrap_or_else(|| panic!("expected List<Angle> of 3, got {:?}", result));
        // Recompose and verify equivalence to the original quaternion.
        let q_back = eval_builtin(
            "orient_euler",
            &[
                Value::String("xyz".to_string()),
                Value::Real(angles[0]),
                Value::Real(angles[1]),
                Value::Real(angles[2]),
            ],
        );
        assert_orientation_approx!(q_back, qw, qx, qy, qz, sign_insensitive = 1e-10);
    }

    // ── normalize_quaternion near-zero tests ────────────────────────────────

    /// normalize_quaternion with near-zero norm (1e-17 < f64::EPSILON) should return None.
    /// Currently passes because norm != 0.0 is true for 1e-17.
    #[test]
    fn normalize_quaternion_near_zero_returns_none() {
        assert!(
            normalize_quaternion(1e-17, 0.0, 0.0, 0.0).is_none(),
            "near-zero quaternion (norm=1e-17) should return None"
        );
    }

    /// normalize_quaternion with all near-zero components should return None.
    #[test]
    fn normalize_quaternion_all_near_zero_returns_none() {
        assert!(
            normalize_quaternion(1e-18, 1e-18, 1e-18, 1e-18).is_none(),
            "all near-zero components should return None"
        );
    }

    // ── elementary_rotation_quat invalid-axis test ──────────────────────────

    /// Calling elementary_rotation_quat with axis > 2 must panic loudly.
    /// This ensures the previously-silent catch-all is now an unreachable!() guard.
    #[test]
    fn elementary_rotation_quat_invalid_axis_panics_loudly() {
        let result = std::panic::catch_unwind(|| {
            elementary_rotation_quat(3, 0.0);
        });
        let err = result.expect_err("expected elementary_rotation_quat(3, ...) to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("elementary_rotation_quat called with axis > 2"),
            "expected panic message to contain 'elementary_rotation_quat called with axis > 2', got: {msg:?}"
        );
    }

    // ── orient_look_at tests (step-1 RED) ────────────────────────────────────

    /// Helper: build a 3-component Tensor value.
    fn t3(x: f64, y: f64, z: f64) -> Value {
        Value::Tensor(vec![Value::Real(x), Value::Real(y), Value::Real(z)])
    }

    #[test]
    fn orient_look_at_forward_z_up_y_is_identity() {
        // forward=(0,0,1), up=(0,1,0):
        // z=(0,0,1), x=cross((0,1,0),(0,0,1))=(1,0,0), y=(0,1,0) → identity → (1,0,0,0)
        let fwd = t3(0.0, 0.0, 1.0);
        let up = t3(0.0, 1.0, 0.0);
        assert_orientation_approx!(
            eval_builtin("orient_look_at", &[fwd, up]),
            1.0, 0.0, 0.0, 0.0
        );
    }

    #[test]
    fn orient_look_at_forward_x_up_z() {
        // forward=(1,0,0), up=(0,0,1):
        // z=(1,0,0), x=cross((0,0,1),(1,0,0))=(0,1,0), y=cross((1,0,0),(0,1,0))=(0,0,1)
        // basis cols [(0,1,0),(0,0,1),(1,0,0)]: trace=0, r00=r11=r22=0
        // → Shepperd final-else (z-dominant) branch → (0.5,0.5,0.5,0.5)
        let fwd = t3(1.0, 0.0, 0.0);
        let up = t3(0.0, 0.0, 1.0);
        assert_orientation_approx!(
            eval_builtin("orient_look_at", &[fwd, up]),
            0.5, 0.5, 0.5, 0.5
        );
    }

    #[test]
    fn orient_look_at_non_unit_up_gives_same_result() {
        // Non-unit up=(0,5,0) should give same result as unit up=(0,1,0)
        let fwd = t3(0.0, 0.0, 1.0);
        let up = t3(0.0, 5.0, 0.0);
        assert_orientation_approx!(
            eval_builtin("orient_look_at", &[fwd, up]),
            1.0, 0.0, 0.0, 0.0
        );
    }

    #[test]
    fn orient_look_at_parallel_forward_up_returns_undef() {
        // forward=(0,0,1), up=(0,0,1): parallel → cross product = 0 → Undef
        let fwd = t3(0.0, 0.0, 1.0);
        let up = t3(0.0, 0.0, 1.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "parallel forward and up should return Undef"
        );
    }

    #[test]
    fn orient_look_at_parallel_scaled_forward_up_returns_undef() {
        // forward=(0,0,2), up=(0,0,5): same direction, scaled → Undef
        let fwd = t3(0.0, 0.0, 2.0);
        let up = t3(0.0, 0.0, 5.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "parallel scaled forward and up should return Undef"
        );
    }

    #[test]
    fn orient_look_at_zero_forward_returns_undef() {
        let fwd = t3(0.0, 0.0, 0.0);
        let up = t3(0.0, 1.0, 0.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "zero forward should return Undef"
        );
    }

    #[test]
    fn orient_look_at_zero_up_returns_undef() {
        let fwd = t3(0.0, 0.0, 1.0);
        let up = t3(0.0, 0.0, 0.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "zero up should return Undef"
        );
    }

    #[test]
    fn orient_look_at_non_3d_forward_returns_undef() {
        // 2-component vector for forward → Undef
        let fwd = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0)]);
        let up = t3(0.0, 1.0, 0.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "non-3D forward should return Undef"
        );
    }

    #[test]
    fn orient_look_at_non_3d_up_returns_undef() {
        // 2-component vector for up → Undef
        let fwd = t3(0.0, 0.0, 1.0);
        let up = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "non-3D up should return Undef"
        );
    }

    #[test]
    fn orient_look_at_nan_component_returns_undef() {
        let fwd = t3(0.0, 0.0, f64::NAN);
        let up = t3(0.0, 1.0, 0.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "NaN in forward should return Undef"
        );
    }

    #[test]
    fn orient_look_at_inf_component_returns_undef() {
        let fwd = t3(0.0, 0.0, f64::INFINITY);
        let up = t3(0.0, 1.0, 0.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "Inf in forward should return Undef"
        );
    }

    #[test]
    fn orient_look_at_nan_component_in_up_returns_undef() {
        // NaN in up propagates through cross3(up, z), making the cross product
        // non-finite → normalize_vec3_arr returns None → Undef.
        let fwd = t3(0.0, 0.0, 1.0);
        let up = t3(0.0, f64::NAN, 0.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "NaN in up should return Undef"
        );
    }

    #[test]
    fn orient_look_at_inf_component_in_up_returns_undef() {
        // Inf in up propagates through cross3(up, z), making the cross product
        // non-finite → normalize_vec3_arr returns None → Undef.
        let fwd = t3(0.0, 0.0, 1.0);
        let up = t3(f64::INFINITY, 0.0, 0.0);
        assert!(
            eval_builtin("orient_look_at", &[fwd, up]).is_undef(),
            "Inf in up should return Undef"
        );
    }

    #[test]
    fn orient_look_at_wrong_arg_count_returns_undef() {
        assert!(
            eval_builtin("orient_look_at", &[]).is_undef(),
            "0 args should return Undef"
        );
        let v = t3(0.0, 0.0, 1.0);
        assert!(
            eval_builtin("orient_look_at", std::slice::from_ref(&v)).is_undef(),
            "1 arg should return Undef"
        );
        assert!(
            eval_builtin("orient_look_at", &[v.clone(), v.clone(), v.clone()]).is_undef(),
            "3 args should return Undef"
        );
    }

    // ── orient_euler EulerConvention enum-value tests (step-3 RED) ──────────

    /// For each Tait-Bryan variant, assert enum path matches lowercase-string path.
    #[test]
    fn orient_euler_enum_xyz_matches_string_xyz() {
        let a = 0.1_f64;
        let b = 0.2_f64;
        let c = 0.3_f64;
        let by_enum = eval_builtin(
            "orient_euler",
            &[
                Value::Enum { type_name: "EulerConvention".to_string(), variant: "XYZ".to_string() },
                Value::Real(a), Value::Real(b), Value::Real(c),
            ],
        );
        let by_str = eval_builtin(
            "orient_euler",
            &[Value::String("xyz".to_string()), Value::Real(a), Value::Real(b), Value::Real(c)],
        );
        assert!(!by_enum.is_undef(), "EulerConvention.XYZ should not return Undef");
        assert_eq!(by_enum, by_str, "EulerConvention.XYZ should equal string 'xyz'");
    }

    #[test]
    fn orient_euler_enum_xzy_matches_string_xzy() {
        let a = 0.1_f64; let b = 0.2_f64; let c = 0.3_f64;
        let by_enum = eval_builtin("orient_euler", &[
            Value::Enum { type_name: "EulerConvention".to_string(), variant: "XZY".to_string() },
            Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        let by_str = eval_builtin("orient_euler", &[
            Value::String("xzy".to_string()), Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        assert!(!by_enum.is_undef(), "EulerConvention.XZY should not return Undef");
        assert_eq!(by_enum, by_str);
    }

    #[test]
    fn orient_euler_enum_yxz_matches_string_yxz() {
        let a = 0.1_f64; let b = 0.2_f64; let c = 0.3_f64;
        let by_enum = eval_builtin("orient_euler", &[
            Value::Enum { type_name: "EulerConvention".to_string(), variant: "YXZ".to_string() },
            Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        let by_str = eval_builtin("orient_euler", &[
            Value::String("yxz".to_string()), Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        assert!(!by_enum.is_undef(), "EulerConvention.YXZ should not return Undef");
        assert_eq!(by_enum, by_str);
    }

    #[test]
    fn orient_euler_enum_yzx_matches_string_yzx() {
        let a = 0.1_f64; let b = 0.2_f64; let c = 0.3_f64;
        let by_enum = eval_builtin("orient_euler", &[
            Value::Enum { type_name: "EulerConvention".to_string(), variant: "YZX".to_string() },
            Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        let by_str = eval_builtin("orient_euler", &[
            Value::String("yzx".to_string()), Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        assert!(!by_enum.is_undef(), "EulerConvention.YZX should not return Undef");
        assert_eq!(by_enum, by_str);
    }

    #[test]
    fn orient_euler_enum_zxy_matches_string_zxy() {
        let a = 0.1_f64; let b = 0.2_f64; let c = 0.3_f64;
        let by_enum = eval_builtin("orient_euler", &[
            Value::Enum { type_name: "EulerConvention".to_string(), variant: "ZXY".to_string() },
            Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        let by_str = eval_builtin("orient_euler", &[
            Value::String("zxy".to_string()), Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        assert!(!by_enum.is_undef(), "EulerConvention.ZXY should not return Undef");
        assert_eq!(by_enum, by_str);
    }

    #[test]
    fn orient_euler_enum_zyx_matches_string_zyx() {
        let a = 0.1_f64; let b = 0.2_f64; let c = 0.3_f64;
        let by_enum = eval_builtin("orient_euler", &[
            Value::Enum { type_name: "EulerConvention".to_string(), variant: "ZYX".to_string() },
            Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        let by_str = eval_builtin("orient_euler", &[
            Value::String("zyx".to_string()), Value::Real(a), Value::Real(b), Value::Real(c),
        ]);
        assert!(!by_enum.is_undef(), "EulerConvention.ZYX should not return Undef");
        assert_eq!(by_enum, by_str);
    }

    #[test]
    fn orient_euler_enum_wrong_type_name_returns_undef() {
        // Enum from a different type must be rejected.
        assert!(
            eval_builtin("orient_euler", &[
                Value::Enum { type_name: "OutputFormat".to_string(), variant: "STEP".to_string() },
                Value::Real(0.1), Value::Real(0.2), Value::Real(0.3),
            ]).is_undef(),
            "Enum with wrong type_name should return Undef"
        );
    }

    #[test]
    fn orient_euler_enum_unknown_variant_returns_undef() {
        // Unknown variant should fall through to Undef.
        assert!(
            eval_builtin("orient_euler", &[
                Value::Enum { type_name: "EulerConvention".to_string(), variant: "ABC".to_string() },
                Value::Real(0.1), Value::Real(0.2), Value::Real(0.3),
            ]).is_undef(),
            "Unknown EulerConvention variant should return Undef"
        );
    }

    #[test]
    fn orient_euler_uppercase_string_still_returns_undef() {
        // Regression: String "XYZ" (not enum) must still be rejected (case-sensitive).
        // This test mirrors orient_euler_uppercase_convention_returns_undef at line 1426.
        assert!(
            eval_builtin("orient_euler", &[
                Value::String("XYZ".to_string()),
                Value::Real(0.1), Value::Real(0.2), Value::Real(0.3),
            ]).is_undef(),
            "Uppercase String 'XYZ' must still return Undef (enum path only case-folds)"
        );
    }

    // ── orient_to_euler EulerConvention enum-value tests (step-5 RED) ────────

    #[test]
    fn orient_to_euler_enum_zyx_matches_string_zyx() {
        // Build a known quaternion with ZYX convention, then decode with enum and string paths.
        let q = eval_builtin(
            "orient_euler",
            &[
                Value::String("zyx".to_string()),
                Value::Real(0.3), Value::Real(0.5), Value::Real(-0.7),
            ],
        );
        let by_enum = eval_builtin(
            "orient_to_euler",
            &[
                Value::Enum { type_name: "EulerConvention".to_string(), variant: "ZYX".to_string() },
                q.clone(),
            ],
        );
        let by_str = eval_builtin(
            "orient_to_euler",
            &[Value::String("zyx".to_string()), q.clone()],
        );
        assert!(
            euler_extract(&by_enum).is_some(),
            "EulerConvention.ZYX orient_to_euler should return a 3-element Angle list, got {:?}",
            by_enum
        );
        assert_eq!(by_enum, by_str, "EulerConvention.ZYX should equal string 'zyx'");
    }

    #[test]
    fn orient_to_euler_enum_xyz_matches_string_xyz() {
        let q = eval_builtin(
            "orient_euler",
            &[
                Value::String("xyz".to_string()),
                Value::Real(0.1), Value::Real(0.2), Value::Real(0.3),
            ],
        );
        let by_enum = eval_builtin(
            "orient_to_euler",
            &[
                Value::Enum { type_name: "EulerConvention".to_string(), variant: "XYZ".to_string() },
                q.clone(),
            ],
        );
        let by_str = eval_builtin(
            "orient_to_euler",
            &[Value::String("xyz".to_string()), q.clone()],
        );
        assert!(
            euler_extract(&by_enum).is_some(),
            "EulerConvention.XYZ orient_to_euler should return a 3-element Angle list, got {:?}",
            by_enum
        );
        assert_eq!(by_enum, by_str, "EulerConvention.XYZ should equal string 'xyz'");
    }

    #[test]
    fn orient_to_euler_enum_wrong_type_name_returns_undef() {
        let q = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert!(
            eval_builtin("orient_to_euler", &[
                Value::Enum { type_name: "OutputFormat".to_string(), variant: "STEP".to_string() },
                q,
            ]).is_undef(),
            "Enum with wrong type_name should return Undef"
        );
    }

    #[test]
    fn orient_to_euler_enum_unknown_variant_returns_undef() {
        let q = Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };
        assert!(
            eval_builtin("orient_to_euler", &[
                Value::Enum { type_name: "EulerConvention".to_string(), variant: "ABC".to_string() },
                q,
            ]).is_undef(),
            "Unknown EulerConvention variant should return Undef"
        );
    }
}
