use reify_types::{DimensionVector, Value};

/// Evaluate a built-in stdlib function by name.
///
/// Returns `Value::Undef` for unknown functions or wrong argument types/counts.
pub fn eval_builtin(name: &str, args: &[Value]) -> Value {
    match name {
        // --- Single-arg numeric functions ---
        "abs" => unary(args, |v| match v {
            Value::Int(i) => Value::Int(i.abs()),
            Value::Real(r) => Value::Real(r.abs()),
            Value::Scalar { si_value, dimension } => Value::Scalar {
                si_value: si_value.abs(),
                dimension: *dimension,
            },
            _ => Value::Undef,
        }),
        "sqrt" => unary(args, |v| match v {
            Value::Scalar { si_value, dimension } => sanitize_value(Value::Scalar {
                si_value: si_value.sqrt(),
                dimension: dimension.root(2),
            }),
            _ => match v.as_f64() {
                Some(x) => sanitize_value(Value::Real(x.sqrt())),
                None => Value::Undef,
            },
        }),
        "floor" => unary_f64(args, |x| Value::Int(x.floor() as i64)),
        "ceil" => unary_f64(args, |x| Value::Int(x.ceil() as i64)),
        "round" => unary_f64(args, |x| Value::Int(x.round() as i64)),
        "sign" => unary_f64(args, |x| Value::Real(x.signum())),
        "log" => unary_f64(args, |x| Value::Real(x.ln())),
        "log10" => unary_f64(args, |x| Value::Real(x.log10())),
        "exp" => unary_f64(args, |x| Value::Real(x.exp())),

        // --- Two-arg numeric functions ---
        "min" => binary(args, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(*x.min(y)),
            (Value::Real(x), Value::Real(y)) => Value::Real(x.min(*y)),
            (Value::Scalar { si_value: x, dimension: d1 }, Value::Scalar { si_value: y, dimension: d2 })
                if d1 == d2 => Value::Scalar { si_value: x.min(*y), dimension: *d1 },
            _ => {
                match (a.as_f64(), b.as_f64()) {
                    (Some(x), Some(y)) => Value::Real(x.min(y)),
                    _ => Value::Undef,
                }
            }
        }),
        "max" => binary(args, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(*x.max(y)),
            (Value::Real(x), Value::Real(y)) => Value::Real(x.max(*y)),
            (Value::Scalar { si_value: x, dimension: d1 }, Value::Scalar { si_value: y, dimension: d2 })
                if d1 == d2 => Value::Scalar { si_value: x.max(*y), dimension: *d1 },
            _ => {
                match (a.as_f64(), b.as_f64()) {
                    (Some(x), Some(y)) => Value::Real(x.max(y)),
                    _ => Value::Undef,
                }
            }
        }),
        "pow" => binary_f64(args, |x, y| Value::Real(x.powf(y))),
        "mod" => binary(args, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => {
                if *y == 0 || (*x == i64::MIN && *y == -1) {
                    Value::Undef
                } else {
                    Value::Int(x % y)
                }
            }
            _ => Value::Undef,
        }),

        // --- Three-arg numeric functions ---
        "clamp" => ternary(args, |x, lo, hi| match (x, lo, hi) {
            (Value::Int(xv), Value::Int(lov), Value::Int(hiv)) => {
                if lov > hiv {
                    Value::Undef
                } else {
                    Value::Int((*xv).clamp(*lov, *hiv))
                }
            }
            (Value::Real(xv), Value::Real(lov), Value::Real(hiv)) => {
                if xv.is_nan() || !valid_f64_range(*lov, *hiv) {
                    Value::Undef
                } else {
                    sanitize_value(Value::Real(xv.clamp(*lov, *hiv)))
                }
            }
            (
                Value::Scalar { si_value: xv, dimension: dx },
                Value::Scalar { si_value: lov, dimension: dlo },
                Value::Scalar { si_value: hiv, dimension: dhi },
            ) => {
                if dx != dlo || dx != dhi {
                    return Value::Undef;
                }
                if xv.is_nan() || !valid_f64_range(*lov, *hiv) {
                    return Value::Undef;
                }
                sanitize_value(Value::Scalar {
                    si_value: xv.clamp(*lov, *hiv),
                    dimension: *dx,
                })
            }
            _ => {
                // Fallback: try to extract f64 from all three args.
                // If all three were Scalar with matching dimension, the explicit Scalar arm above
                // handles them. In this fallback, at least one arg is non-Scalar. Non-Scalar
                // Value::dimension() always returns DIMENSIONLESS, so any non-DIMENSIONLESS
                // dimension means a type mismatch that would silently drop the dimension.
                // Return Undef to keep logic errors noisy.
                if x.dimension() != DimensionVector::DIMENSIONLESS
                    || lo.dimension() != DimensionVector::DIMENSIONLESS
                    || hi.dimension() != DimensionVector::DIMENSIONLESS
                {
                    return Value::Undef;
                }
                let (xv, lov, hiv) = match (x.as_f64(), lo.as_f64(), hi.as_f64()) {
                    (Some(a), Some(b), Some(c)) => (a, b, c),
                    _ => return Value::Undef,
                };
                if xv.is_nan() || !valid_f64_range(lov, hiv) {
                    return Value::Undef;
                }
                sanitize_value(Value::Real(xv.clamp(lov, hiv)))
            }
        }),

        "lerp" => ternary(args, |a, b, t| {
            // t must be dimensionless (Real or Int; reject dimensioned Scalar)
            if let Value::Scalar { dimension, .. } = t
                && *dimension != DimensionVector::DIMENSIONLESS
            {
                return Value::Undef;
            }
            let tv = match t.as_f64() {
                Some(v) => v,
                None => return Value::Undef,
            };
            if tv.is_nan() {
                return Value::Undef;
            }
            match (a, b) {
                (Value::Real(av), Value::Real(bv)) => {
                    sanitize_value(Value::Real(lerp_f64(*av, *bv, tv)))
                }
                (
                    Value::Scalar { si_value: av, dimension: da },
                    Value::Scalar { si_value: bv, dimension: db },
                ) => {
                    if da != db {
                        return Value::Undef;
                    }
                    sanitize_value(Value::Scalar {
                        si_value: lerp_f64(*av, *bv, tv),
                        dimension: *da,
                    })
                }
                // Int fast path: documents the explicit Int->Real coercion
                (Value::Int(av), Value::Int(bv)) => {
                    sanitize_value(Value::Real(lerp_f64(*av as f64, *bv as f64, tv)))
                }
                _ => {
                    // Fallback: extract f64 from a and b.
                    // If both a and b were Scalar with matching dimension, the explicit Scalar
                    // arm above handles them. In this fallback, at least one is non-Scalar.
                    // Non-Scalar dimension() always returns DIMENSIONLESS, so any
                    // non-DIMENSIONLESS dimension would be silently dropped — return Undef
                    // to keep logic errors noisy (per feedback_silent_defaults_pattern).
                    if a.dimension() != DimensionVector::DIMENSIONLESS
                        || b.dimension() != DimensionVector::DIMENSIONLESS
                    {
                        return Value::Undef;
                    }
                    let av = match a.as_f64() {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    let bv = match b.as_f64() {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    sanitize_value(Value::Real(lerp_f64(av, bv, tv)))
                }
            }
        }),

        "remap" => {
            if args.len() != 5 {
                return Value::Undef;
            }
            let (x, from_lo, from_hi, to_lo, to_hi) =
                (&args[0], &args[1], &args[2], &args[3], &args[4]);

            // Dimension-aware path: activate when any arg is a Scalar
            let any_scalar = args.iter().any(|a| matches!(a, Value::Scalar { .. }));
            if any_scalar {
                // x/from_lo/from_hi must share a dimension (input space)
                let from_dim = from_lo.dimension();
                if from_hi.dimension() != from_dim || x.dimension() != from_dim {
                    return Value::Undef;
                }
                // to_lo/to_hi must share a dimension (output space)
                let to_dim = to_lo.dimension();
                if to_hi.dimension() != to_dim {
                    return Value::Undef;
                }
                // Extract si_values via as_f64()
                let (xv, flov, fhiv, tlov, thiv) = match (
                    x.as_f64(), from_lo.as_f64(), from_hi.as_f64(),
                    to_lo.as_f64(), to_hi.as_f64(),
                ) {
                    (Some(a), Some(b), Some(c), Some(d), Some(e)) => (a, b, c, d, e),
                    _ => return Value::Undef,
                };
                if flov == fhiv {
                    return Value::Undef; // early-exit: division by zero
                }
                let result = tlov + (xv - flov) * (thiv - tlov) / (fhiv - flov);
                return sanitize_value(Value::Scalar { si_value: result, dimension: to_dim });
            }

            // Non-Scalar path: use quinary_f64 helper
            quinary_f64(args, |x, from_lo, from_hi, to_lo, to_hi| {
                if from_lo == from_hi {
                    return Value::Undef; // early-exit: division by zero
                }
                let result = to_lo + (x - from_lo) * (to_hi - to_lo) / (from_hi - from_lo);
                Value::Real(result)
            })
        }

        // --- Trig functions: accept Angle Scalar or bare Real (radians) ---
        "sin" => unary(args, |v| trig_input(v).map_or(Value::Undef, |r| Value::Real(r.sin()))),
        "cos" => unary(args, |v| trig_input(v).map_or(Value::Undef, |r| Value::Real(r.cos()))),
        "tan" => unary(args, |v| trig_input(v).map_or(Value::Undef, |r| Value::Real(r.tan()))),

        // --- Inverse trig: accept Real, return Angle Scalar ---
        "asin" => unary_f64(args, |x| Value::Scalar {
            si_value: x.asin(),
            dimension: DimensionVector::ANGLE,
        }),
        "acos" => unary_f64(args, |x| Value::Scalar {
            si_value: x.acos(),
            dimension: DimensionVector::ANGLE,
        }),
        "atan" => unary_f64(args, |x| Value::Scalar {
            si_value: x.atan(),
            dimension: DimensionVector::ANGLE,
        }),
        "atan2" => binary_f64(args, |y, x| Value::Scalar {
            si_value: y.atan2(x),
            dimension: DimensionVector::ANGLE,
        }),

        // --- Hyperbolic: accept Real, return Real ---
        "sinh" => unary_f64(args, |x| Value::Real(x.sinh())),
        "cosh" => unary_f64(args, |x| Value::Real(x.cosh())),
        "tanh" => unary_f64(args, |x| Value::Real(x.tanh())),

        // --- Linear algebra: dot, cross, magnitude, normalize ---

        "normalize" => unary(args, |v| {
            let (vals, _dim) = match tensor_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            // Reject non-finite inputs early — a partially-Undef Tensor is not
            // a meaningful unit vector, so we return a single Undef for the
            // whole result rather than per-component sanitization.
            if vals.iter().any(|x| !x.is_finite()) {
                return Value::Undef;
            }
            let sum_sq: f64 = vals.iter().map(|x| x * x).sum();
            let mag = sum_sq.sqrt();
            // mag is finite here, but squaring can still overflow to Inf.
            if !mag.is_finite() || mag == 0.0 {
                return Value::Undef;
            }
            Value::Tensor(vals.iter().map(|x| Value::Real(x / mag)).collect())
        }),

        "magnitude" => unary(args, |v| {
            let (vals, dim) = match tensor_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            let sum_sq: f64 = vals.iter().map(|x| x * x).sum();
            let mag = sum_sq.sqrt();
            if dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(mag))
            } else {
                sanitize_value(Value::Scalar { si_value: mag, dimension: dim })
            }
        }),

        "cross" => binary(args, |a, b| {
            let (a_vals, a_dim) = match tensor_components_f64(a) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let (b_vals, b_dim) = match tensor_components_f64(b) {
                Some(v) => v,
                None => return Value::Undef,
            };
            if a_vals.len() != 3 || b_vals.len() != 3 {
                return Value::Undef;
            }
            let (a0, a1, a2) = (a_vals[0], a_vals[1], a_vals[2]);
            let (b0, b1, b2) = (b_vals[0], b_vals[1], b_vals[2]);
            let cx = a1 * b2 - a2 * b1;
            let cy = a2 * b0 - a0 * b2;
            let cz = a0 * b1 - a1 * b0;
            let result_dim = a_dim.mul(&b_dim);
            let make_component = |v: f64| -> Value {
                if result_dim == DimensionVector::DIMENSIONLESS {
                    sanitize_value(Value::Real(v))
                } else {
                    sanitize_value(Value::Scalar { si_value: v, dimension: result_dim })
                }
            };
            Value::Tensor(vec![make_component(cx), make_component(cy), make_component(cz)])
        }),

        "dot" => binary(args, |a, b| {
            let (a_vals, a_dim) = match tensor_components_f64(a) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let (b_vals, b_dim) = match tensor_components_f64(b) {
                Some(v) => v,
                None => return Value::Undef,
            };
            if a_vals.len() != b_vals.len() {
                return Value::Undef;
            }
            let sum: f64 = a_vals.iter().zip(b_vals.iter()).map(|(x, y)| x * y).sum();
            let result_dim = a_dim.mul(&b_dim);
            if result_dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(sum))
            } else {
                sanitize_value(Value::Scalar { si_value: sum, dimension: result_dim })
            }
        }),

        // --- Determinacy predicates (stubs) ---
        // These predicates inspect DeterminacyState which is tracked in the Engine's
        // snapshot, not in Value itself. Like sample(), the actual behavior is
        // intercepted at the eval layer (reify-expr/reify-eval) where snapshot state
        // is available. These stubs serve as documentation and fallback.
        "determined" => Value::Undef,
        "undetermined" => Value::Undef,
        "constrained" => Value::Undef,
        "partially_determined" => Value::Undef,

        // --- Orientation constructors ---
        "orient_identity" => {
            if args.is_empty() {
                Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 }
            } else {
                Value::Undef
            }
        }
        "orient_quaternion" => {
            if args.len() != 4 {
                return Value::Undef;
            }
            match (args[0].as_f64(), args[1].as_f64(), args[2].as_f64(), args[3].as_f64()) {
                (Some(w), Some(x), Some(y), Some(z)) => {
                    normalize_quaternion(w, x, y, z).unwrap_or(Value::Undef)
                }
                _ => Value::Undef,
            }
        }
        "orient_euler" => {
            if args.len() != 4 {
                return Value::Undef;
            }
            let convention = match &args[0] {
                Value::String(s) => s.as_str(),
                _ => return Value::Undef,
            };
            let a = match trig_input(&args[1]) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let b = match trig_input(&args[2]) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let c = match trig_input(&args[3]) {
                Some(v) => v,
                None => return Value::Undef,
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
                _ => return Value::Undef,
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
                return Value::Undef;
            }
            let (xc, _) = match tensor_components_f64(&args[0]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Value::Undef,
            };
            let (yc, _) = match tensor_components_f64(&args[1]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Value::Undef,
            };
            let (zc, _) = match tensor_components_f64(&args[2]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Value::Undef,
            };
            // Verify approximate orthonormality
            let tol = 1e-6;
            let mag_x = (xc[0]*xc[0] + xc[1]*xc[1] + xc[2]*xc[2]).sqrt();
            let mag_y = (yc[0]*yc[0] + yc[1]*yc[1] + yc[2]*yc[2]).sqrt();
            let mag_z = (zc[0]*zc[0] + zc[1]*zc[1] + zc[2]*zc[2]).sqrt();
            if (mag_x - 1.0).abs() > tol || (mag_y - 1.0).abs() > tol || (mag_z - 1.0).abs() > tol {
                return Value::Undef;
            }
            let dot_xy = xc[0]*yc[0] + xc[1]*yc[1] + xc[2]*yc[2];
            let dot_xz = xc[0]*zc[0] + xc[1]*zc[1] + xc[2]*zc[2];
            let dot_yz = yc[0]*zc[0] + yc[1]*zc[1] + yc[2]*zc[2];
            if dot_xy.abs() > tol || dot_xz.abs() > tol || dot_yz.abs() > tol {
                return Value::Undef;
            }
            // Rotation matrix from basis vectors (columns are the new axes)
            // R = [xc | yc | zc], where row i, col j = R[i][j]
            // R[0][0]=xc[0], R[1][0]=xc[1], R[2][0]=xc[2]
            // R[0][1]=yc[0], R[1][1]=yc[1], R[2][1]=yc[2]
            // R[0][2]=zc[0], R[1][2]=zc[1], R[2][2]=zc[2]
            let r00 = xc[0]; let r01 = yc[0]; let r02 = zc[0];
            let r10 = xc[1]; let r11 = yc[1]; let r12 = zc[1];
            let r20 = xc[2]; let r21 = yc[2]; let r22 = zc[2];
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
                return Value::Undef;
            }
            let (comps, _dim) = match tensor_components_f64(&args[0]) {
                Some(c) if c.0.len() == 3 => c,
                _ => return Value::Undef,
            };
            let theta = match trig_input(&args[1]) {
                Some(t) => t,
                None => return Value::Undef,
            };
            // Normalize axis
            let ax = comps[0];
            let ay = comps[1];
            let az = comps[2];
            let axis_norm = (ax * ax + ay * ay + az * az).sqrt();
            if axis_norm == 0.0 || !axis_norm.is_finite() {
                return Value::Undef;
            }
            let nax = ax / axis_norm;
            let nay = ay / axis_norm;
            let naz = az / axis_norm;
            let half = theta / 2.0;
            let c = half.cos();
            let s = half.sin();
            normalize_quaternion(c, s * nax, s * nay, s * naz).unwrap_or(Value::Undef)
        }

        // --- Field operations (stubs) ---
        // These are handled by reify-expr's eval_expr FunctionCall interceptor
        // for actual lambda application; the stdlib entries serve as documentation
        // and fallback for direct stdlib calls.
        "sample" => Value::Undef,     // Requires EvalContext for lambda application
        "gradient" => Value::Undef,   // Numeric differentiation not yet implemented
        "divergence" => Value::Undef, // Numeric differentiation not yet implemented
        "curl" => Value::Undef,       // Numeric differentiation not yet implemented

        _ => Value::Undef,
    }
}

/// Apply a function to a single argument (by reference, for pattern matching).
fn unary(args: &[Value], f: impl FnOnce(&Value) -> Value) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    f(&args[0])
}

/// Normalize a quaternion (w, x, y, z) to unit length.
///
/// Returns `None` if any component is non-finite or the quaternion has zero length.
fn normalize_quaternion(w: f64, x: f64, y: f64, z: f64) -> Option<Value> {
    if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() {
        return None;
    }
    let norm = (w * w + x * x + y * y + z * z).sqrt();
    if norm == 0.0 {
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
///
/// `axis`: 0=X, 1=Y, 2=Z. `angle`: rotation in radians.
/// Returns (w, x, y, z) quaternion.
fn elementary_rotation_quat(axis: usize, angle: f64) -> (f64, f64, f64, f64) {
    let half = angle / 2.0;
    let c = half.cos();
    let s = half.sin();
    match axis {
        0 => (c, s, 0.0, 0.0),
        1 => (c, 0.0, s, 0.0),
        2 => (c, 0.0, 0.0, s),
        _ => (1.0, 0.0, 0.0, 0.0), // identity fallback
    }
}

/// Hamilton product of two quaternions.
fn quat_mul(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (
        a.0 * b.0 - a.1 * b.1 - a.2 * b.2 - a.3 * b.3,
        a.0 * b.1 + a.1 * b.0 + a.2 * b.3 - a.3 * b.2,
        a.0 * b.2 - a.1 * b.3 + a.2 * b.0 + a.3 * b.1,
        a.0 * b.3 + a.1 * b.2 - a.2 * b.1 + a.3 * b.0,
    )
}

/// Convert non-finite f64 values (NaN, inf) to Undef.
///
/// This is a defense-in-depth catch-all applied at the return point of
/// `unary_f64` and `binary_f64` to ensure domain errors (e.g., sqrt(-1),
/// log(0), exp(1000) overflow) produce Undef instead of silently propagating
/// NaN or infinity through the evaluation graph.
fn sanitize_value(v: Value) -> Value {
    match &v {
        Value::Real(x) if x.is_nan() || x.is_infinite() => Value::Undef,
        Value::Scalar { si_value, .. } if si_value.is_nan() || si_value.is_infinite() => {
            Value::Undef
        }
        _ => v,
    }
}

/// Apply a function to a single f64 argument (extracted from any numeric Value).
fn unary_f64(args: &[Value], f: impl FnOnce(f64) -> Value) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    match args[0].as_f64() {
        Some(x) => sanitize_value(f(x)),
        None => Value::Undef,
    }
}

/// Apply a function to two arguments (by reference).
fn binary(args: &[Value], f: impl FnOnce(&Value, &Value) -> Value) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    f(&args[0], &args[1])
}

/// Extract radians from a trig function argument.
/// Accepts: Angle Scalar (si_value is already radians) or bare Real (treated as radians).
/// Rejects: non-ANGLE Scalar (dimension error).
fn trig_input(v: &Value) -> Option<f64> {
    match v {
        Value::Scalar { si_value, dimension } => {
            if *dimension == DimensionVector::ANGLE {
                Some(*si_value)
            } else {
                None // dimension error: sin(5mm) is meaningless
            }
        }
        Value::Real(r) => Some(*r),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

/// Apply a function to three arguments (by reference).
fn ternary(args: &[Value], f: impl FnOnce(&Value, &Value, &Value) -> Value) -> Value {
    if args.len() != 3 {
        return Value::Undef;
    }
    f(&args[0], &args[1], &args[2])
}

/// Apply a function to two f64 arguments.
fn binary_f64(args: &[Value], f: impl FnOnce(f64, f64) -> Value) -> Value {
    if args.len() != 2 {
        return Value::Undef;
    }
    match (args[0].as_f64(), args[1].as_f64()) {
        (Some(x), Some(y)) => sanitize_value(f(x, y)),
        _ => Value::Undef,
    }
}

/// Returns true iff `lo` and `hi` form a valid (non-NaN, non-inverted) range.
///
/// Used by clamp Real/Scalar/fallback arms instead of inline `lo.is_nan() || hi.is_nan() || lo > hi`.
fn valid_f64_range(lo: f64, hi: f64) -> bool {
    !lo.is_nan() && !hi.is_nan() && lo <= hi
}

/// Linear interpolation: `a + t * (b - a)`.
fn lerp_f64(a: f64, b: f64, t: f64) -> f64 {
    a + t * (b - a)
}

/// Extract numeric components and consistent dimension from a Tensor value.
///
/// Returns `Some((values, dimension))` if:
/// - `v` is a `Value::Tensor` with at least one element.
/// - All components support `as_f64()`.
/// - All components share the same dimension (or all are dimensionless).
///
/// Returns `None` for non-Tensor values, empty Tensors, non-numeric components,
/// or Tensors with mixed dimensions.
fn tensor_components_f64(v: &Value) -> Option<(Vec<f64>, DimensionVector)> {
    let items = match v {
        Value::Tensor(items) if !items.is_empty() => items,
        _ => return None,
    };
    let first_dim = items[0].dimension();
    let mut vals = Vec::with_capacity(items.len());
    for item in items {
        if item.dimension() != first_dim {
            return None; // mixed dimensions
        }
        match item.as_f64() {
            Some(x) => vals.push(x),
            None => return None, // non-numeric component
        }
    }
    Some((vals, first_dim))
}

/// Apply a function to five f64 arguments (extracted via `as_f64()`).
///
/// Returns `Undef` on wrong argument count or extraction failure.
/// Applies `sanitize_value` to the result.
fn quinary_f64(args: &[Value], f: impl FnOnce(f64, f64, f64, f64, f64) -> Value) -> Value {
    if args.len() != 5 {
        return Value::Undef;
    }
    match (
        args[0].as_f64(),
        args[1].as_f64(),
        args[2].as_f64(),
        args[3].as_f64(),
        args[4].as_f64(),
    ) {
        (Some(a), Some(b), Some(c), Some(d), Some(e)) => sanitize_value(f(a, b, c, d, e)),
        _ => Value::Undef,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::DimensionVector;

    /// Assert that an expression evaluates to `Value::Real(v)` where `|v - expected| < 1e-12`.
    macro_rules! assert_real_approx {
        ($expr:expr, $expected:expr) => {
            match $expr {
                Value::Real(v) => assert!(
                    (v - $expected).abs() < 1e-12,
                    "expected Real({}) got Real({})",
                    $expected,
                    v
                ),
                other => panic!("expected Real({}), got {:?}", $expected, other),
            }
        };
    }

    /// Assert that an expression evaluates to `Value::Scalar { si_value, dimension }` where
    /// `|si_value - expected_si| < 1e-12` and `dimension == expected_dim`.
    macro_rules! assert_scalar_approx {
        ($expr:expr, $expected_si:expr, $expected_dim:expr) => {
            match $expr {
                Value::Scalar { si_value, dimension } => {
                    assert!(
                        (si_value - $expected_si).abs() < 1e-12,
                        "expected si_value={}, got {}",
                        $expected_si,
                        si_value
                    );
                    assert_eq!(dimension, $expected_dim);
                }
                other => panic!(
                    "expected Scalar{{si={}, dim={:?}}}, got {:?}",
                    $expected_si, $expected_dim, other
                ),
            }
        };
    }

    #[test]
    fn abs_real_negative() {
        let result = eval_builtin("abs", &[Value::Real(-5.0)]);
        match result {
            Value::Real(v) => assert!((v - 5.0).abs() < 1e-12),
            other => panic!("expected Real(5.0), got {:?}", other),
        }
    }

    #[test]
    fn abs_int_negative() {
        let result = eval_builtin("abs", &[Value::Int(-3)]);
        match result {
            Value::Int(3) => {}
            other => panic!("expected Int(3), got {:?}", other),
        }
    }

    #[test]
    fn abs_scalar_preserves_dimension() {
        let result = eval_builtin(
            "abs",
            &[Value::Scalar {
                si_value: -0.005,
                dimension: DimensionVector::LENGTH,
            }],
        );
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - 0.005).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_real() {
        let result = eval_builtin("sqrt", &[Value::Real(9.0)]);
        match result {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12),
            other => panic!("expected Real(3.0), got {:?}", other),
        }
    }

    #[test]
    fn min_real() {
        let result = eval_builtin("min", &[Value::Real(3.0), Value::Real(7.0)]);
        match result {
            Value::Real(v) => assert!((v - 3.0).abs() < 1e-12),
            other => panic!("expected Real(3.0), got {:?}", other),
        }
    }

    #[test]
    fn max_int() {
        let result = eval_builtin("max", &[Value::Int(3), Value::Int(7)]);
        match result {
            Value::Int(7) => {}
            other => panic!("expected Int(7), got {:?}", other),
        }
    }

    #[test]
    fn floor_real() {
        let result = eval_builtin("floor", &[Value::Real(3.7)]);
        match result {
            Value::Int(3) => {}
            other => panic!("expected Int(3), got {:?}", other),
        }
    }

    #[test]
    fn ceil_real() {
        let result = eval_builtin("ceil", &[Value::Real(3.2)]);
        match result {
            Value::Int(4) => {}
            other => panic!("expected Int(4), got {:?}", other),
        }
    }

    #[test]
    fn round_real() {
        let result = eval_builtin("round", &[Value::Real(3.5)]);
        match result {
            Value::Int(4) => {}
            other => panic!("expected Int(4), got {:?}", other),
        }
    }

    #[test]
    fn log_e() {
        let result = eval_builtin("log", &[Value::Real(std::f64::consts::E)]);
        match result {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-12),
            other => panic!("expected Real(~1.0), got {:?}", other),
        }
    }

    #[test]
    fn exp_zero() {
        let result = eval_builtin("exp", &[Value::Real(0.0)]);
        match result {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-12),
            other => panic!("expected Real(1.0), got {:?}", other),
        }
    }

    #[test]
    fn sign_negative() {
        let result = eval_builtin("sign", &[Value::Real(-5.0)]);
        match result {
            Value::Real(v) => assert!((v - (-1.0)).abs() < 1e-12),
            other => panic!("expected Real(-1.0), got {:?}", other),
        }
    }

    #[test]
    fn unknown_function_returns_undef() {
        let result = eval_builtin("foo", &[Value::Real(1.0)]);
        assert!(result.is_undef());
    }

    // --- Trig function tests ---

    #[test]
    fn sin_angle_scalar() {
        let result = eval_builtin(
            "sin",
            &[Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_4,
                dimension: DimensionVector::ANGLE,
            }],
        );
        match result {
            Value::Real(v) => assert!((v - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-10),
            other => panic!("expected Real(~0.7071), got {:?}", other),
        }
    }

    #[test]
    fn cos_angle_zero() {
        let result = eval_builtin(
            "cos",
            &[Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::ANGLE,
            }],
        );
        match result {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-12),
            other => panic!("expected Real(1.0), got {:?}", other),
        }
    }

    #[test]
    fn tan_angle_pi_over_4() {
        let result = eval_builtin(
            "tan",
            &[Value::Scalar {
                si_value: std::f64::consts::FRAC_PI_4,
                dimension: DimensionVector::ANGLE,
            }],
        );
        match result {
            Value::Real(v) => assert!((v - 1.0).abs() < 1e-10),
            other => panic!("expected Real(~1.0), got {:?}", other),
        }
    }

    #[test]
    fn asin_returns_angle() {
        let result = eval_builtin("asin", &[Value::Real(1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn acos_returns_angle() {
        let result = eval_builtin("acos", &[Value::Real(0.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn atan_returns_angle() {
        let result = eval_builtin("atan", &[Value::Real(1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn atan2_returns_angle() {
        let result = eval_builtin("atan2", &[Value::Real(1.0), Value::Real(1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
                assert!((si_value - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    #[test]
    fn sinh_real() {
        let result = eval_builtin("sinh", &[Value::Real(0.0)]);
        match result {
            Value::Real(v) => assert!((v - 0.0).abs() < 1e-12),
            other => panic!("expected Real(0.0), got {:?}", other),
        }
    }

    #[test]
    fn sin_non_angle_scalar_returns_undef() {
        // A LENGTH scalar should not be accepted by sin
        let result = eval_builtin(
            "sin",
            &[Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            }],
        );
        assert!(result.is_undef(), "sin of LENGTH scalar should be Undef");
    }

    // --- Domain-error NaN/inf hardening tests (step-21) ---

    #[test]
    fn sqrt_negative_returns_undef() {
        let result = eval_builtin("sqrt", &[Value::Real(-1.0)]);
        assert!(result.is_undef(), "sqrt(-1) should be Undef, got {:?}", result);
    }

    #[test]
    fn log_zero_returns_undef() {
        let result = eval_builtin("log", &[Value::Real(0.0)]);
        assert!(result.is_undef(), "log(0) should be Undef, got {:?}", result);
    }

    #[test]
    fn log_negative_returns_undef() {
        let result = eval_builtin("log", &[Value::Real(-1.0)]);
        assert!(result.is_undef(), "log(-1) should be Undef, got {:?}", result);
    }

    #[test]
    fn log10_zero_returns_undef() {
        let result = eval_builtin("log10", &[Value::Real(0.0)]);
        assert!(result.is_undef(), "log10(0) should be Undef, got {:?}", result);
    }

    #[test]
    fn log10_negative_returns_undef() {
        let result = eval_builtin("log10", &[Value::Real(-1.0)]);
        assert!(result.is_undef(), "log10(-1) should be Undef, got {:?}", result);
    }

    #[test]
    fn exp_overflow_returns_undef() {
        let result = eval_builtin("exp", &[Value::Real(1000.0)]);
        assert!(result.is_undef(), "exp(1000) should be Undef (inf), got {:?}", result);
    }

    #[test]
    fn pow_negative_base_fractional_exp_returns_undef() {
        let result = eval_builtin("pow", &[Value::Real(-2.0), Value::Real(0.5)]);
        assert!(result.is_undef(), "pow(-2, 0.5) should be Undef (NaN), got {:?}", result);
    }

    // --- Inverse-trig domain errors and hyperbolic overflow (step-23) ---

    #[test]
    fn asin_out_of_range_positive() {
        let result = eval_builtin("asin", &[Value::Real(2.0)]);
        assert!(result.is_undef(), "asin(2.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn asin_out_of_range_negative() {
        let result = eval_builtin("asin", &[Value::Real(-2.0)]);
        assert!(result.is_undef(), "asin(-2.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn acos_out_of_range_positive() {
        let result = eval_builtin("acos", &[Value::Real(2.0)]);
        assert!(result.is_undef(), "acos(2.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn acos_out_of_range_negative() {
        let result = eval_builtin("acos", &[Value::Real(-2.0)]);
        assert!(result.is_undef(), "acos(-2.0) should be Undef, got {:?}", result);
    }

    #[test]
    fn sinh_overflow_returns_undef() {
        let result = eval_builtin("sinh", &[Value::Real(1000.0)]);
        assert!(result.is_undef(), "sinh(1000) should be Undef (inf), got {:?}", result);
    }

    #[test]
    fn cosh_overflow_returns_undef() {
        let result = eval_builtin("cosh", &[Value::Real(1000.0)]);
        assert!(result.is_undef(), "cosh(1000) should be Undef (inf), got {:?}", result);
    }

    // Boundary valid inputs: confirm no regressions on valid inputs

    #[test]
    fn asin_boundary_valid() {
        let result = eval_builtin("asin", &[Value::Real(1.0)]);
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    // --- sqrt dimension-awareness tests (step-3, task 39) ---

    #[test]
    fn sqrt_scalar_area_to_length() {
        // sqrt(Scalar{4.0, AREA}) must return Scalar{2.0, LENGTH}
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::AREA,
            }],
        );
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 2.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Scalar{{2.0, LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_scalar_length4_to_length2() {
        // sqrt(Scalar{9.0, LENGTH^4}) must return Scalar{3.0, LENGTH^2}
        let len4 = DimensionVector::LENGTH.pow(4);
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: 9.0,
                dimension: len4,
            }],
        );
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 3.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::AREA); // LENGTH^2 == AREA
            }
            other => panic!("expected Scalar{{3.0, AREA}}, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_scalar_length_to_fractional_exponent() {
        use reify_types::Rational;
        // sqrt(Scalar{4.0, LENGTH}) must return Scalar{2.0, LENGTH^(1/2)}
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            }],
        );
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - 2.0).abs() < 1e-12);
                assert_eq!(dimension.0[0], Rational::new(1, 2));
                for i in 1..9 {
                    assert!(dimension.0[i].is_zero());
                }
            }
            other => panic!("expected Scalar{{2.0, LENGTH^(1/2)}}, got {:?}", other),
        }
    }

    #[test]
    fn sqrt_negative_scalar_returns_undef() {
        // sqrt of negative Scalar must return Undef (via sanitize_value catching NaN)
        let result = eval_builtin(
            "sqrt",
            &[Value::Scalar {
                si_value: -4.0,
                dimension: DimensionVector::AREA,
            }],
        );
        assert!(result.is_undef(), "sqrt of negative Scalar should be Undef, got {:?}", result);
    }

    #[test]
    fn acos_boundary_valid() {
        let result = eval_builtin("acos", &[Value::Real(-1.0)]);
        match result {
            Value::Scalar { si_value, dimension } => {
                assert!((si_value - std::f64::consts::PI).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::ANGLE);
            }
            other => panic!("expected Angle Scalar, got {:?}", other),
        }
    }

    // --- Determinacy predicate stubs (step-7) ---

    #[test]
    fn determined_stub_returns_undef() {
        // determined() is handled at the eval layer where DeterminacyState is available.
        // The stdlib stub returns Undef as a fallback.
        let result = eval_builtin("determined", &[Value::Real(42.0)]);
        assert!(result.is_undef(), "determined stub should return Undef, got {:?}", result);
    }

    #[test]
    fn undetermined_stub_returns_undef() {
        let result = eval_builtin("undetermined", &[Value::Real(42.0)]);
        assert!(result.is_undef(), "undetermined stub should return Undef, got {:?}", result);
    }

    #[test]
    fn constrained_stub_returns_undef() {
        let result = eval_builtin("constrained", &[Value::Real(42.0)]);
        assert!(result.is_undef(), "constrained stub should return Undef, got {:?}", result);
    }

    #[test]
    fn partially_determined_stub_returns_undef() {
        let result = eval_builtin("partially_determined", &[Value::Real(42.0)]);
        assert!(result.is_undef(), "partially_determined stub should return Undef, got {:?}", result);
    }

    // --- Field operation stubs (step-25) ---

    #[test]
    fn gradient_scalar_field_returns_undef() {
        // gradient(field) on a scalar field should return Undef (stub).
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::length(),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("gradient", &[field]);
        assert!(result.is_undef(), "gradient stub should return Undef, got {:?}", result);
    }

    #[test]
    fn divergence_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::StructureRef("Vector3".into()),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("divergence", &[field]);
        assert!(result.is_undef(), "divergence stub should return Undef, got {:?}", result);
    }

    #[test]
    fn curl_field_returns_undef() {
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::StructureRef("Vector3".into()),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("curl", &[field]);
        assert!(result.is_undef(), "curl stub should return Undef, got {:?}", result);
    }

    #[test]
    fn sample_in_stdlib_returns_undef() {
        // sample() in stdlib returns Undef because lambda application
        // needs an EvalContext (handled in reify-expr instead).
        let field = Value::Field {
            domain_type: reify_types::Type::StructureRef("Point3".into()),
            codomain_type: reify_types::Type::length(),
            source: reify_types::FieldSourceKind::Analytical,
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("sample", &[field, Value::Int(42)]);
        assert!(result.is_undef(), "sample in stdlib should return Undef (handled in eval_expr), got {:?}", result);
    }

    // --- mod builtin tests (step-1) ---

    #[test]
    fn mod_basic() {
        let result = eval_builtin("mod", &[Value::Int(7), Value::Int(3)]);
        match result {
            Value::Int(1) => {}
            other => panic!("expected Int(1), got {:?}", other),
        }
    }

    #[test]
    fn mod_exact_division() {
        let result = eval_builtin("mod", &[Value::Int(6), Value::Int(3)]);
        match result {
            Value::Int(0) => {}
            other => panic!("expected Int(0), got {:?}", other),
        }
    }

    #[test]
    fn mod_negative_dividend() {
        // Rust's % truncates toward zero: -7 % 3 == -1
        let result = eval_builtin("mod", &[Value::Int(-7), Value::Int(3)]);
        match result {
            Value::Int(-1) => {}
            other => panic!("expected Int(-1), got {:?}", other),
        }
    }

    #[test]
    fn mod_negative_divisor() {
        // -7 % -3 == -1 (truncation toward zero)
        let result = eval_builtin("mod", &[Value::Int(-7), Value::Int(-3)]);
        match result {
            Value::Int(-1) => {}
            other => panic!("expected Int(-1), got {:?}", other),
        }
    }

    #[test]
    fn mod_by_zero_returns_undef() {
        let result = eval_builtin("mod", &[Value::Int(7), Value::Int(0)]);
        assert!(result.is_undef(), "mod by zero should be Undef, got {:?}", result);
    }

    #[test]
    fn mod_non_int_returns_undef() {
        let result = eval_builtin("mod", &[Value::Real(3.5), Value::Real(2.0)]);
        assert!(result.is_undef(), "mod on Real should be Undef, got {:?}", result);
    }

    #[test]
    fn mod_wrong_arg_count_returns_undef() {
        let result = eval_builtin("mod", &[Value::Int(7)]);
        assert!(result.is_undef(), "mod with 1 arg should be Undef, got {:?}", result);
    }

    #[test]
    fn mod_i64_min_neg1_returns_undef() {
        // i64::MIN % -1 overflows in Rust (panics in debug mode)
        let result = eval_builtin("mod", &[Value::Int(i64::MIN), Value::Int(-1)]);
        assert!(result.is_undef(), "mod(i64::MIN, -1) should be Undef (overflow), got {:?}", result);
    }

    // --- clamp Real tests (step-3) ---

    #[test]
    fn clamp_real_within_range() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)]),
            5.0
        );
    }

    #[test]
    fn clamp_real_below_lo() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(-3.0), Value::Real(0.0), Value::Real(10.0)]),
            0.0
        );
    }

    #[test]
    fn clamp_real_above_hi() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(15.0), Value::Real(0.0), Value::Real(10.0)]),
            10.0
        );
    }

    #[test]
    fn clamp_at_lo_boundary() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(0.0), Value::Real(0.0), Value::Real(10.0)]),
            0.0
        );
    }

    #[test]
    fn clamp_at_hi_boundary() {
        assert_real_approx!(
            eval_builtin("clamp", &[Value::Real(10.0), Value::Real(0.0), Value::Real(10.0)]),
            10.0
        );
    }

    #[test]
    fn clamp_nan_x_returns_undef() {
        // x is NaN — explicit x.is_nan() guard
        let result = eval_builtin("clamp", &[Value::Real(f64::NAN), Value::Real(0.0), Value::Real(10.0)]);
        assert!(result.is_undef(), "clamp(NaN, 0, 10) should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_nan_lo_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(f64::NAN), Value::Real(10.0)]);
        assert!(result.is_undef(), "clamp(5, NaN, 10) should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_nan_hi_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0), Value::Real(f64::NAN)]);
        assert!(result.is_undef(), "clamp(5, 0, NaN) should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_inverted_range_real_returns_undef() {
        // lo > hi is invalid
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(10.0), Value::Real(0.0)]);
        assert!(result.is_undef(), "clamp with inverted range should be Undef, got {:?}", result);
    }

    // --- clamp Int tests (step-5) ---

    #[test]
    fn clamp_int_preserves_type() {
        // within range: value passes through, returns Int
        let result = eval_builtin("clamp", &[Value::Int(5), Value::Int(0), Value::Int(10)]);
        match result {
            Value::Int(5) => {}
            other => panic!("expected Int(5), got {:?}", other),
        }
    }

    #[test]
    fn clamp_int_below_lo() {
        let result = eval_builtin("clamp", &[Value::Int(-3), Value::Int(0), Value::Int(10)]);
        match result {
            Value::Int(0) => {}
            other => panic!("expected Int(0) (clamped to lo), got {:?}", other),
        }
    }

    #[test]
    fn clamp_int_above_hi() {
        let result = eval_builtin("clamp", &[Value::Int(15), Value::Int(0), Value::Int(10)]);
        match result {
            Value::Int(10) => {}
            other => panic!("expected Int(10) (clamped to hi), got {:?}", other),
        }
    }

    #[test]
    fn clamp_inverted_range_int_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Int(5), Value::Int(10), Value::Int(0)]);
        assert!(result.is_undef(), "clamp Int with inverted range should be Undef, got {:?}", result);
    }

    // --- clamp Scalar + fallback tests (step-7) ---

    #[test]
    fn clamp_scalar_preserves_dimension() {
        // All three args: same LENGTH dimension, result should be LENGTH Scalar
        assert_scalar_approx!(
            eval_builtin(
                "clamp",
                &[
                    Value::Scalar { si_value: 0.005, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 0.001, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 0.010, dimension: DimensionVector::LENGTH },
                ]
            ),
            0.005,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn clamp_dimension_mismatch_returns_undef() {
        // lo/hi have different dimensions -> Undef
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 10.0, dimension: DimensionVector::TIME },
            ],
        );
        assert!(result.is_undef(), "clamp with dimension mismatch should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_inverted_range_scalar_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 10.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(result.is_undef(), "clamp Scalar with inverted range should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_scalar_nan_x_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 10.0, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(result.is_undef(), "clamp Scalar NaN x should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_wrong_arg_count_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0)]);
        assert!(result.is_undef(), "clamp with 2 args should be Undef, got {:?}", result);
    }

    #[test]
    fn clamp_fallback_dimension_mismatch_returns_undef() {
        // Fallback arm: x is Real (DIMENSIONLESS) but lo/hi are Scalar LENGTH.
        // The fallback cannot silently drop LENGTH → must return Undef.
        let result = eval_builtin(
            "clamp",
            &[
                Value::Real(5.0),
                Value::Scalar { si_value: 0.001, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.010, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(
            result.is_undef(),
            "clamp with mismatched dimensions should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_fallback_all_dimensionless_returns_real() {
        // Fallback arm: x is Int, lo/hi are Real → all DIMENSIONLESS → clamp coerces to Real.
        let result = eval_builtin(
            "clamp",
            &[Value::Int(5), Value::Real(0.0), Value::Real(10.0)],
        );
        assert_real_approx!(result, 5.0);
    }

    // --- lerp Real tests (step-9) ---

    #[test]
    fn lerp_midpoint() {
        // lerp(0, 10, 0.5) = 5
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(0.5)]),
            5.0
        );
    }

    #[test]
    fn lerp_t_zero() {
        // lerp(a, b, 0) = a
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Real(3.0), Value::Real(7.0), Value::Real(0.0)]),
            3.0
        );
    }

    #[test]
    fn lerp_t_one() {
        // lerp(a, b, 1) = b
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Real(3.0), Value::Real(7.0), Value::Real(1.0)]),
            7.0
        );
    }

    #[test]
    fn lerp_negative_t_extrapolation() {
        // lerp(0, 10, -0.5) = -5 (extrapolation below)
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(-0.5)]),
            -5.0
        );
    }

    #[test]
    fn lerp_nan_t_returns_undef() {
        // t is NaN — explicit NaN check after extraction
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0), Value::Real(f64::NAN)]);
        assert!(result.is_undef(), "lerp with NaN t should be Undef, got {:?}", result);
    }

    // --- lerp Scalar + dimension tests (step-11) ---

    #[test]
    fn lerp_scalar_preserves_dimension() {
        // lerp(Scalar{0.0, LENGTH}, Scalar{1.0, LENGTH}, Real(0.5)) = Scalar{0.5, LENGTH}
        assert_scalar_approx!(
            eval_builtin(
                "lerp",
                &[
                    Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
                    Value::Real(0.5),
                ]
            ),
            0.5,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn lerp_dimension_mismatch_a_b_returns_undef() {
        // a and b have different dimensions -> Undef
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 1.0, dimension: DimensionVector::TIME },
                Value::Real(0.5),
            ],
        );
        assert!(result.is_undef(), "lerp dimension mismatch a/b should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_t_dimensioned_returns_undef() {
        // t must be dimensionless; a LENGTH t is invalid
        let result = eval_builtin(
            "lerp",
            &[
                Value::Real(0.0),
                Value::Real(10.0),
                Value::Scalar { si_value: 0.5, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(result.is_undef(), "lerp with dimensioned t should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_nan_a_returns_undef() {
        // NaN in a -> Undef (via sanitize_value)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
                Value::Real(0.5),
            ],
        );
        assert!(result.is_undef(), "lerp with NaN a should be Undef, got {:?}", result);
    }

    #[test]
    fn lerp_nan_b_returns_undef() {
        // NaN in b -> Undef (via sanitize_value)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: f64::NAN, dimension: DimensionVector::LENGTH },
                Value::Real(0.5),
            ],
        );
        assert!(result.is_undef(), "lerp with NaN b should be Undef, got {:?}", result);
    }

    // --- lerp Int/edge tests (step-13) ---

    #[test]
    fn lerp_int_inputs_coerce_to_real() {
        // lerp(Int(0), Int(10), Real(0.5)) -> Real(5.0)
        // The Int fast path extracts as f64, computes, returns Real
        assert_real_approx!(
            eval_builtin("lerp", &[Value::Int(0), Value::Int(10), Value::Real(0.5)]),
            5.0
        );
    }

    #[test]
    fn lerp_wrong_arg_count_returns_undef() {
        let result = eval_builtin("lerp", &[Value::Real(0.0), Value::Real(10.0)]);
        assert!(result.is_undef(), "lerp with 2 args should be Undef, got {:?}", result);
    }

    // --- lerp fallback tests (step-21) ---

    #[test]
    fn lerp_fallback_scalar_a_real_b_returns_undef() {
        // Fallback arm: a is Scalar LENGTH, b is Real → a's dimension would be silently
        // dropped if we returned Real. Per feedback_silent_defaults_pattern, must return Undef.
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH },
                Value::Real(3.0),
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with Scalar a and Real b should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_fallback_real_a_scalar_b_returns_undef() {
        // Fallback arm: a is Real, b is Scalar LENGTH → symmetric case, also must be Undef.
        let result = eval_builtin(
            "lerp",
            &[
                Value::Real(3.0),
                Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH },
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with Real a and Scalar b should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_fallback_all_dimensionless_returns_real() {
        // Fallback arm: a is Int, b is Real → both DIMENSIONLESS → valid coercion to Real.
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Int(0), Value::Real(10.0), Value::Real(0.5)],
            ),
            5.0
        );
    }

    // --- remap Real tests (step-15) ---
    // remap(x, from_lo, from_hi, to_lo, to_hi)
    // formula: to_lo + (x - from_lo) * (to_hi - to_lo) / (from_hi - from_lo)

    #[test]
    fn remap_midpoint() {
        // remap(5, 0, 10, 0, 100) = 50
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)]
            ),
            50.0
        );
    }

    #[test]
    fn remap_at_from_lo() {
        // remap(from_lo, from_lo, from_hi, to_lo, to_hi) = to_lo
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(0.0), Value::Real(0.0), Value::Real(10.0), Value::Real(20.0), Value::Real(30.0)]
            ),
            20.0
        );
    }

    #[test]
    fn remap_at_from_hi() {
        // remap(from_hi, from_lo, from_hi, to_lo, to_hi) = to_hi
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(10.0), Value::Real(0.0), Value::Real(10.0), Value::Real(20.0), Value::Real(30.0)]
            ),
            30.0
        );
    }

    #[test]
    fn remap_extrapolation() {
        // x outside [from_lo, from_hi] extrapolates
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(15.0), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)]
            ),
            150.0
        );
    }

    #[test]
    fn remap_inverse() {
        // remap from [0,100] to [0,10] — inverse of remap_midpoint
        assert_real_approx!(
            eval_builtin(
                "remap",
                &[Value::Real(50.0), Value::Real(0.0), Value::Real(100.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            5.0
        );
    }

    #[test]
    fn remap_division_by_zero_returns_undef() {
        // from_lo == from_hi -> division by zero -> Undef (early-exit)
        let result = eval_builtin(
            "remap",
            &[Value::Real(5.0), Value::Real(3.0), Value::Real(3.0), Value::Real(0.0), Value::Real(10.0)],
        );
        assert!(result.is_undef(), "remap with from_lo==from_hi should be Undef, got {:?}", result);
    }

    #[test]
    fn remap_nan_returns_undef() {
        let result = eval_builtin(
            "remap",
            &[Value::Real(f64::NAN), Value::Real(0.0), Value::Real(10.0), Value::Real(0.0), Value::Real(100.0)],
        );
        assert!(result.is_undef(), "remap with NaN x should be Undef, got {:?}", result);
    }

    #[test]
    fn remap_wrong_arg_count_returns_undef() {
        let result = eval_builtin("remap", &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)]);
        assert!(result.is_undef(), "remap with 3 args should be Undef, got {:?}", result);
    }

    // --- remap Scalar tests (step-17) ---
    // remap(x, from_lo, from_hi, to_lo, to_hi)

    #[test]
    fn remap_scalar_preserves_dimension() {
        // All 5 args LENGTH -> result is LENGTH
        // remap(Scalar{5m}, Scalar{0m}, Scalar{10m}, Scalar{0m}, Scalar{100m}) = Scalar{50m}
        assert_scalar_approx!(
            eval_builtin(
                "remap",
                &[
                    Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 10.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 100.0, dimension: DimensionVector::LENGTH },
                ]
            ),
            50.0,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn remap_scalar_cross_dimension() {
        // x in LENGTH, from in LENGTH, to in TIME -> result is TIME
        // remap(Scalar{5m, LENGTH}, Scalar{0m}, Scalar{10m}, Scalar{0s, TIME}, Scalar{100s, TIME}) = Scalar{50s, TIME}
        assert_scalar_approx!(
            eval_builtin(
                "remap",
                &[
                    Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 10.0, dimension: DimensionVector::LENGTH },
                    Value::Scalar { si_value: 0.0, dimension: DimensionVector::TIME },
                    Value::Scalar { si_value: 100.0, dimension: DimensionVector::TIME },
                ]
            ),
            50.0,
            DimensionVector::TIME
        );
    }

    #[test]
    fn remap_scalar_dimension_mismatch_x_from_returns_undef() {
        // x has TIME dimension but from_lo/from_hi are LENGTH -> Undef
        let result = eval_builtin(
            "remap",
            &[
                Value::Scalar { si_value: 5.0, dimension: DimensionVector::TIME },
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 10.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 100.0, dimension: DimensionVector::LENGTH },
            ],
        );
        assert!(result.is_undef(), "remap with x dim != from dim should be Undef, got {:?}", result);
    }

    #[test]
    fn remap_scalar_to_range_mismatch_returns_undef() {
        // to_lo and to_hi have different dimensions -> Undef
        let result = eval_builtin(
            "remap",
            &[
                Value::Scalar { si_value: 5.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 10.0, dimension: DimensionVector::LENGTH },
                Value::Scalar { si_value: 0.0, dimension: DimensionVector::TIME },
                Value::Scalar { si_value: 100.0, dimension: DimensionVector::LENGTH }, // mismatch
            ],
        );
        assert!(result.is_undef(), "remap with to_lo/to_hi dim mismatch should be Undef, got {:?}", result);
    }

    // --- dot() tests: dimensionless vectors (step-1) ---

    #[test]
    fn dot_orthogonal_dimensionless() {
        // dot([1,0,0], [0,1,0]) == 0.0
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 0.0);
    }

    #[test]
    fn dot_dimensionless_vec3() {
        // dot([1,2,3], [4,5,6]) == 4+10+18 == 32
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Tensor(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 32.0);
    }

    #[test]
    fn dot_mismatched_lengths_returns_undef() {
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(eval_builtin("dot", &[a, b]).is_undef(), "mismatched lengths should be Undef");
    }

    #[test]
    fn dot_non_tensor_arg_returns_undef() {
        assert!(
            eval_builtin("dot", &[Value::Real(1.0), Value::Real(2.0)]).is_undef(),
            "dot of non-Tensor args should be Undef"
        );
    }

    // --- normalize() tests (step-9) ---

    #[test]
    fn normalize_3_4_0() {
        // normalize([3,4,0]) ≈ [0.6, 0.8, 0.0]
        let v = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let result = eval_builtin("normalize", &[v]);
        match result {
            Value::Tensor(items) => {
                assert_eq!(items.len(), 3, "normalize must return 3 components");
                let vals: Vec<f64> = items.iter().map(|x| x.as_f64().unwrap()).collect();
                assert!((vals[0] - 0.6).abs() < 1e-12, "x: expected 0.6, got {}", vals[0]);
                assert!((vals[1] - 0.8).abs() < 1e-12, "y: expected 0.8, got {}", vals[1]);
                assert!((vals[2] - 0.0).abs() < 1e-12, "z: expected 0.0, got {}", vals[2]);
                // Components must be Real (dimensionless)
                assert!(
                    items.iter().all(|x| matches!(x, Value::Real(_))),
                    "normalize must return Real components"
                );
            }
            other => panic!("expected Tensor, got {:?}", other),
        }
    }

    #[test]
    fn normalize_zero_vector_returns_undef() {
        let v = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("normalize", &[v]).is_undef(), "normalize of zero vector should be Undef");
    }

    #[test]
    fn normalize_dimensioned_vector_returns_real_components() {
        // normalize([3m,4m,0m]) should return Real components (dimensionless direction)
        let v = Value::Tensor(vec![
            Value::Scalar { si_value: 3.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 4.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
        ]);
        let result = eval_builtin("normalize", &[v]);
        match result {
            Value::Tensor(items) => {
                assert_eq!(items.len(), 3);
                let vals: Vec<f64> = items.iter().map(|x| x.as_f64().unwrap()).collect();
                assert!((vals[0] - 0.6).abs() < 1e-12, "x: expected 0.6, got {}", vals[0]);
                assert!((vals[1] - 0.8).abs() < 1e-12, "y: expected 0.8, got {}", vals[1]);
                assert!((vals[2] - 0.0).abs() < 1e-12, "z: expected 0.0, got {}", vals[2]);
                assert!(
                    items.iter().all(|x| matches!(x, Value::Real(_))),
                    "normalize must return Real (dimensionless) components"
                );
            }
            other => panic!("expected Tensor, got {:?}", other),
        }
    }

    #[test]
    fn normalize_non_tensor_returns_undef() {
        assert!(
            eval_builtin("normalize", &[Value::Real(5.0)]).is_undef(),
            "normalize of non-Tensor should be Undef"
        );
    }

    #[test]
    fn normalize_single_element_tensor() {
        // normalize([5.0]) == [1.0]
        let v = Value::Tensor(vec![Value::Real(5.0)]);
        let result = eval_builtin("normalize", &[v]);
        match result {
            Value::Tensor(items) => {
                assert_eq!(items.len(), 1);
                let val = items[0].as_f64().unwrap();
                assert!((val - 1.0).abs() < 1e-12, "expected 1.0, got {}", val);
            }
            other => panic!("expected Tensor([1.0]), got {:?}", other),
        }
    }

    // --- normalize() sanitization tests (step-13) ---

    #[test]
    fn normalize_nan_component_returns_undef() {
        // A NaN component makes sum_sq NaN → mag NaN → mag==0.0 is false →
        // without an up-front guard we'd produce a Tensor with NaN Real values.
        let v = Value::Tensor(vec![
            Value::Real(f64::NAN),
            Value::Real(1.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of a Tensor containing NaN should return Undef"
        );
    }

    #[test]
    fn normalize_inf_component_returns_undef() {
        // An Inf component makes sum_sq Inf → mag Inf → Inf/Inf = NaN for the
        // Inf component, other components become 0.0 (finite/Inf).  Without a
        // guard we'd produce a mixed Tensor instead of Undef.
        let v = Value::Tensor(vec![
            Value::Real(f64::INFINITY),
            Value::Real(1.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of a Tensor containing Inf should return Undef"
        );
    }

    #[test]
    fn normalize_overflow_returns_undef() {
        // Squaring f64::MAX overflows to Inf → sum_sq = Inf → mag = Inf →
        // x / mag produces NaN or 0.0 — the result is not a valid unit vector.
        let v = Value::Tensor(vec![
            Value::Real(f64::MAX),
            Value::Real(f64::MAX),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of a Tensor whose magnitude overflows to Inf should return Undef"
        );
    }

    // --- magnitude() tests (step-7) ---

    #[test]
    fn magnitude_3_4_0_equals_5() {
        // magnitude([3,4,0]) == 5.0
        let v = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("magnitude", &[v]), 5.0);
    }

    #[test]
    fn magnitude_dimensioned_vector() {
        // magnitude([3mm,4mm,0mm]) == 5mm = 0.005m as Scalar{LENGTH}
        let v = Value::Tensor(vec![
            Value::Scalar { si_value: 0.003, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.004, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.000, dimension: DimensionVector::LENGTH },
        ]);
        assert_scalar_approx!(eval_builtin("magnitude", &[v]), 0.005, DimensionVector::LENGTH);
    }

    #[test]
    fn magnitude_zero_vector_returns_zero() {
        let v = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("magnitude", &[v]), 0.0);
    }

    #[test]
    fn magnitude_2d_vector() {
        // magnitude([3,4]) == 5.0
        let v = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0)]);
        assert_real_approx!(eval_builtin("magnitude", &[v]), 5.0);
    }

    #[test]
    fn magnitude_non_tensor_returns_undef() {
        assert!(eval_builtin("magnitude", &[Value::Real(5.0)]).is_undef(), "magnitude of non-Tensor should be Undef");
    }

    #[test]
    fn magnitude_empty_tensor_returns_undef() {
        let v = Value::Tensor(vec![]);
        assert!(eval_builtin("magnitude", &[v]).is_undef(), "magnitude of empty Tensor should be Undef");
    }

    // --- cross() tests: dimensionless vectors (step-4) ---

    #[test]
    fn cross_x_hat_y_hat_equals_z_hat() {
        // cross([1,0,0], [0,1,0]) == [0,0,1]
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        match result {
            Value::Tensor(items) => {
                assert_eq!(items.len(), 3, "cross product must have 3 components");
                let v: Vec<f64> = items.iter().map(|x| x.as_f64().unwrap()).collect();
                assert!((v[0] - 0.0).abs() < 1e-12, "x component: expected 0.0, got {}", v[0]);
                assert!((v[1] - 0.0).abs() < 1e-12, "y component: expected 0.0, got {}", v[1]);
                assert!((v[2] - 1.0).abs() < 1e-12, "z component: expected 1.0, got {}", v[2]);
            }
            other => panic!("expected Tensor([0,0,1]), got {:?}", other),
        }
    }

    #[test]
    fn cross_anti_commutativity() {
        // cross(a,b) == -cross(b,a)
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Tensor(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        let ab = eval_builtin("cross", &[a.clone(), b.clone()]);
        let ba = eval_builtin("cross", &[b, a]);
        match (ab, ba) {
            (Value::Tensor(ab_items), Value::Tensor(ba_items)) => {
                for (ai, bi) in ab_items.iter().zip(ba_items.iter()) {
                    let av = ai.as_f64().unwrap();
                    let bv = bi.as_f64().unwrap();
                    assert!((av + bv).abs() < 1e-12, "anti-commutativity failed: {} + {} != 0", av, bv);
                }
            }
            other => panic!("expected two Tensors, got {:?}", other),
        }
    }

    #[test]
    fn cross_orthogonality() {
        // dot(a, cross(a, b)) == 0
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Tensor(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        let c = eval_builtin("cross", &[a.clone(), b]);
        let dot_result = eval_builtin("dot", &[a, c]);
        assert_real_approx!(dot_result, 0.0);
    }

    #[test]
    fn cross_length_2_tensor_returns_undef() {
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(eval_builtin("cross", &[a, b]).is_undef(), "cross on 2-element Tensor should be Undef");
    }

    #[test]
    fn cross_length_4_tensor_returns_undef() {
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(eval_builtin("cross", &[a, b]).is_undef(), "cross on 4-element Tensor should be Undef");
    }

    #[test]
    fn cross_non_tensor_returns_undef() {
        assert!(
            eval_builtin("cross", &[Value::Real(1.0), Value::Real(2.0)]).is_undef(),
            "cross of non-Tensor args should be Undef"
        );
    }

    // --- cross() tests: dimensioned vectors (step-5) ---

    #[test]
    fn cross_length_force_vectors() {
        // cross([1m,0,0], [0,1N,0]) == [0,0,1 m·N] each component has Length*Force dimension
        let length_force = DimensionVector::LENGTH.mul(&reify_types::dimension::FORCE);
        let a = Value::Tensor(vec![
            Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar { si_value: 0.0, dimension: reify_types::dimension::FORCE },
            Value::Scalar { si_value: 1.0, dimension: reify_types::dimension::FORCE },
            Value::Scalar { si_value: 0.0, dimension: reify_types::dimension::FORCE },
        ]);
        let result = eval_builtin("cross", &[a, b]);
        match result {
            Value::Tensor(items) => {
                assert_eq!(items.len(), 3, "cross product must have 3 components");
                // [1,0,0] x [0,1,0] = [0*0-0*1, 0*0-1*0, 1*1-0*0] = [0, 0, 1]
                for (i, item) in items.iter().enumerate() {
                    match item {
                        Value::Scalar { si_value, dimension } => {
                            assert_eq!(*dimension, length_force, "component {} dimension mismatch", i);
                            let expected = if i == 2 { 1.0 } else { 0.0 };
                            assert!(
                                (si_value - expected).abs() < 1e-12,
                                "component {}: expected {}, got {}", i, expected, si_value
                            );
                        }
                        other => panic!("expected Scalar at component {}, got {:?}", i, other),
                    }
                }
            }
            other => panic!("expected Tensor, got {:?}", other),
        }
    }

    // --- dot() tests: dimensioned vectors (step-2) ---

    #[test]
    fn dot_length_force_vectors() {
        // dot([1m, 0, 0], [1N, 0, 0]) -> Scalar { si_value: 1.0, dimension: Length*Force }
        let length_force = DimensionVector::LENGTH.mul(&reify_types::dimension::FORCE);
        let a = Value::Tensor(vec![
            Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar { si_value: 1.0, dimension: reify_types::dimension::FORCE },
            Value::Scalar { si_value: 0.0, dimension: reify_types::dimension::FORCE },
            Value::Scalar { si_value: 0.0, dimension: reify_types::dimension::FORCE },
        ]);
        assert_scalar_approx!(eval_builtin("dot", &[a, b]), 1.0, length_force);
    }

    /// Assert that an expression evaluates to `Value::Orientation { w, x, y, z }` where each
    /// component is within `1e-12` of the expected value.
    macro_rules! assert_orientation_approx {
        ($expr:expr, $ew:expr, $ex:expr, $ey:expr, $ez:expr) => {
            match $expr {
                Value::Orientation { w, x, y, z } => {
                    assert!(
                        (w - $ew).abs() < 1e-12 &&
                        (x - $ex).abs() < 1e-12 &&
                        (y - $ey).abs() < 1e-12 &&
                        (z - $ez).abs() < 1e-12,
                        "expected Orientation({}, {}, {}, {}), got Orientation({}, {}, {}, {})",
                        $ew, $ex, $ey, $ez, w, x, y, z
                    );
                }
                other => panic!(
                    "expected Orientation({}, {}, {}, {}), got {:?}",
                    $ew, $ex, $ey, $ez, other
                ),
            }
        };
    }

    // ── orient_identity tests (step-6) ──────────────────────────────────────

    #[test]
    fn orient_identity_no_args() {
        assert_orientation_approx!(
            eval_builtin("orient_identity", &[]),
            1.0, 0.0, 0.0, 0.0
        );
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
            eval_builtin("orient_quaternion", &[
                Value::Real(2.0), Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)
            ]),
            1.0, 0.0, 0.0, 0.0
        );
    }

    #[test]
    fn orient_quaternion_preserves_normalized() {
        assert_orientation_approx!(
            eval_builtin("orient_quaternion", &[
                Value::Real(1.0), Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)
            ]),
            1.0, 0.0, 0.0, 0.0
        );
    }

    #[test]
    fn orient_quaternion_arbitrary_normalizes() {
        // (1,1,1,1) norm = 2, normalized = (0.5, 0.5, 0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin("orient_quaternion", &[
                Value::Real(1.0), Value::Real(1.0), Value::Real(1.0), Value::Real(1.0)
            ]),
            0.5, 0.5, 0.5, 0.5
        );
    }

    #[test]
    fn orient_quaternion_zero_returns_undef() {
        assert!(eval_builtin("orient_quaternion", &[
            Value::Real(0.0), Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)
        ]).is_undef());
    }

    #[test]
    fn orient_quaternion_nan_returns_undef() {
        assert!(eval_builtin("orient_quaternion", &[
            Value::Real(f64::NAN), Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)
        ]).is_undef());
    }

    #[test]
    fn orient_quaternion_inf_returns_undef() {
        assert!(eval_builtin("orient_quaternion", &[
            Value::Real(f64::INFINITY), Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)
        ]).is_undef());
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
            cos_pi_4, 0.0, 0.0, sin_pi_4
        );
    }

    #[test]
    fn orient_axis_angle_180deg_around_x() {
        // 180° around X: q = (cos(π/2), sin(π/2), 0, 0) = (0, 1, 0, 0)
        let axis = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let angle = Value::Real(std::f64::consts::PI);
        assert_orientation_approx!(
            eval_builtin("orient_axis_angle", &[axis, angle]),
            0.0, 1.0, 0.0, 0.0
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
            cos_pi_4, 0.0, 0.0, sin_pi_4
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
        assert!(eval_builtin("orient_axis_angle", &[axis.clone(), Value::Real(1.0), Value::Real(2.0)]).is_undef());
    }

    // ── orient_euler tests (step-12) ──────────────────────────────────────

    #[test]
    fn orient_euler_xyz_single_axis() {
        // Intrinsic xyz with (π/2, 0, 0): rotation of π/2 about X
        // = quaternion (cos(π/4), sin(π/4), 0, 0)
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_euler", &[
                Value::String("xyz".into()),
                Value::Real(std::f64::consts::FRAC_PI_2),
                Value::Real(0.0),
                Value::Real(0.0),
            ]),
            cos_pi_4, sin_pi_4, 0.0, 0.0
        );
    }

    #[test]
    fn orient_euler_zyx_single_axis() {
        // Intrinsic zyx with (π/2, 0, 0): rotation of π/2 about Z
        // = quaternion (cos(π/4), 0, 0, sin(π/4))
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_euler", &[
                Value::String("zyx".into()),
                Value::Real(std::f64::consts::FRAC_PI_2),
                Value::Real(0.0),
                Value::Real(0.0),
            ]),
            cos_pi_4, 0.0, 0.0, sin_pi_4
        );
    }

    #[test]
    fn orient_euler_zero_angles_is_identity() {
        assert_orientation_approx!(
            eval_builtin("orient_euler", &[
                Value::String("xyz".into()),
                Value::Real(0.0),
                Value::Real(0.0),
                Value::Real(0.0),
            ]),
            1.0, 0.0, 0.0, 0.0
        );
    }

    #[test]
    fn orient_euler_invalid_convention_returns_undef() {
        assert!(eval_builtin("orient_euler", &[
            Value::String("abc".into()),
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]).is_undef());
    }

    #[test]
    fn orient_euler_non_string_convention_returns_undef() {
        assert!(eval_builtin("orient_euler", &[
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]).is_undef());
    }

    #[test]
    fn orient_euler_angle_scalar_accepted() {
        // Same as xyz (π/2, 0, 0) but with Angle Scalar
        let cos_pi_4 = std::f64::consts::FRAC_PI_4.cos();
        let sin_pi_4 = std::f64::consts::FRAC_PI_4.sin();
        assert_orientation_approx!(
            eval_builtin("orient_euler", &[
                Value::String("xyz".into()),
                Value::Scalar {
                    si_value: std::f64::consts::FRAC_PI_2,
                    dimension: DimensionVector::ANGLE,
                },
                Value::Real(0.0),
                Value::Real(0.0),
            ]),
            cos_pi_4, sin_pi_4, 0.0, 0.0
        );
    }

    #[test]
    fn orient_euler_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("orient_euler", &[]).is_undef());
        assert!(eval_builtin("orient_euler", &[
            Value::String("xyz".into()),
            Value::Real(0.0),
        ]).is_undef());
    }

    // ── orient_euler compound rotation tests (step-16) ───────────────────

    #[test]
    fn orient_euler_xyz_two_nonzero_angles() {
        // orient_euler('xyz', π/2, π/2, 0): q_x(π/2) * q_y(π/2) * q_z(0)
        // Two non-zero angles exercise quat_mul with non-identity operands.
        // Expected: (0.5, 0.5, 0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin("orient_euler", &[
                Value::String("xyz".into()),
                Value::Real(std::f64::consts::FRAC_PI_2),
                Value::Real(std::f64::consts::FRAC_PI_2),
                Value::Real(0.0),
            ]),
            0.5, 0.5, 0.5, 0.5
        );
    }

    #[test]
    fn orient_euler_zyx_three_nonzero_angles() {
        // orient_euler('zyx', π/3, π/4, π/6): q_z(π/3) * q_y(π/4) * q_x(π/6)
        // Three non-zero angles exercise full three-way quat_mul composition.
        // Analytically computed via Hamilton product of elementary rotations.
        assert_orientation_approx!(
            eval_builtin("orient_euler", &[
                Value::String("zyx".into()),
                Value::Real(std::f64::consts::FRAC_PI_3),
                Value::Real(std::f64::consts::FRAC_PI_4),
                Value::Real(std::f64::consts::FRAC_PI_6),
            ]),
            0.82236317190599939,
            0.02226002671473384,
            0.43967973954090955,
            0.36042340565035591
        );
    }

    #[test]
    fn orient_euler_xzx_proper_euler_compound() {
        // orient_euler('xzx', π/2, π/2, 0): q_x(π/2) * q_z(π/2) * q_x(0)
        // Proper Euler convention with compound rotation.
        // Expected: (0.5, 0.5, -0.5, 0.5)
        assert_orientation_approx!(
            eval_builtin("orient_euler", &[
                Value::String("xzx".into()),
                Value::Real(std::f64::consts::FRAC_PI_2),
                Value::Real(std::f64::consts::FRAC_PI_2),
                Value::Real(0.0),
            ]),
            0.5, 0.5, -0.5, 0.5
        );
    }

    // ── orient_basis tests (step-14) ──────────────────────────────────────

    #[test]
    fn orient_basis_identity_basis() {
        // Standard basis = identity rotation
        let x = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let y = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let z = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)]);
        assert_orientation_approx!(
            eval_builtin("orient_basis", &[x, y, z]),
            1.0, 0.0, 0.0, 0.0
        );
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
            cos_pi_4, 0.0, 0.0, sin_pi_4
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

    #[test]
    fn dot_mixed_component_dimensions_returns_undef() {
        // A Tensor with mixed dimensions is not a valid physical vector
        let a = Value::Tensor(vec![
            Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::MASS },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar { si_value: 1.0, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.0, dimension: DimensionVector::LENGTH },
        ]);
        assert!(
            eval_builtin("dot", &[a, b]).is_undef(),
            "dot of vector with mixed component dimensions should be Undef"
        );
    }
}
