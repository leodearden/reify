use reify_types::{DimensionVector, Value};
use crate::common::*;

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let v = match name {
        // --- Orientation constructors ---
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
            let convention = match &args[0] {
                Value::String(s) => s.as_str(),
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
            // Map convention letters to axis indices for elementary rotations
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
            // Defense-in-depth: reject NaN/Inf early (NaN bypasses IEEE 754 comparisons)
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
            if (mag_x - 1.0).abs() > tol
                || (mag_y - 1.0).abs() > tol
                || (mag_z - 1.0).abs() > tol
            {
                return Some(Value::Undef);
            }
            let dot_xy = xc[0] * yc[0] + xc[1] * yc[1] + xc[2] * yc[2];
            let dot_xz = xc[0] * zc[0] + xc[1] * zc[1] + xc[2] * zc[2];
            let dot_yz = yc[0] * zc[0] + yc[1] * zc[1] + yc[2] * zc[2];
            if dot_xy.abs() > tol || dot_xz.abs() > tol || dot_yz.abs() > tol {
                return Some(Value::Undef);
            }
            // Verify right-handedness via scalar triple product (determinant).
            // det(R) = x · (y × z). For a proper rotation (SO(3)), det ≈ +1.
            // Left-handed orthonormal bases have det = -1 and must be rejected.
            let det = xc[0] * (yc[1] * zc[2] - yc[2] * zc[1])
                + xc[1] * (yc[2] * zc[0] - yc[0] * zc[2])
                + xc[2] * (yc[0] * zc[1] - yc[1] * zc[0]);
            if (det - 1.0).abs() > tol {
                return Some(Value::Undef);
            }
            // Rotation matrix from basis vectors (columns are the new axes)
            let r00 = xc[0];
            let r01 = yc[0];
            let r02 = zc[0];
            let r10 = xc[1];
            let r11 = yc[1];
            let r12 = zc[1];
            let r20 = xc[2];
            let r21 = yc[2];
            let r22 = zc[2];
            // Shepperd's method: find the largest of the 4 diagonal sums
            let trace = r00 + r11 + r22;
            let (w, x, y, z) = if trace > 0.0 {
                let s = (trace + 1.0).sqrt() * 2.0; // s = 4*w
                (0.25 * s, (r21 - r12) / s, (r02 - r20) / s, (r10 - r01) / s)
            } else if r00 > r11 && r00 > r22 {
                let s = (1.0 + r00 - r11 - r22).sqrt() * 2.0; // s = 4*x
                ((r21 - r12) / s, 0.25 * s, (r01 + r10) / s, (r02 + r20) / s)
            } else if r11 > r22 {
                let s = (1.0 - r00 + r11 - r22).sqrt() * 2.0; // s = 4*y
                ((r02 - r20) / s, (r01 + r10) / s, 0.25 * s, (r12 + r21) / s)
            } else {
                let s = (1.0 - r00 - r11 + r22).sqrt() * 2.0; // s = 4*z
                ((r10 - r01) / s, (r02 + r20) / s, (r12 + r21) / s, 0.25 * s)
            };
            normalize_quaternion(w, x, y, z).unwrap_or(Value::Undef)
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
    };
    Some(v)
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn orientation_dispatch_identity() {
        assert_eq!(
            dispatch("orient_identity", &[]),
            Some(Value::Orientation {
                w: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            })
        );
    }

    #[test]
    fn orientation_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
