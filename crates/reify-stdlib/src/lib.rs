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
            Value::Scalar {
                si_value,
                dimension,
            } => Value::Scalar {
                si_value: si_value.abs(),
                dimension: *dimension,
            },
            _ => Value::Undef,
        }),
        "sqrt" => unary(args, |v| match v {
            Value::Scalar {
                si_value,
                dimension,
            } => sanitize_value(Value::Scalar {
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
            (
                Value::Scalar {
                    si_value: x,
                    dimension: d1,
                },
                Value::Scalar {
                    si_value: y,
                    dimension: d2,
                },
            ) if d1 == d2 => Value::Scalar {
                si_value: x.min(*y),
                dimension: *d1,
            },
            _ => match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => Value::Real(x.min(y)),
                _ => Value::Undef,
            },
        }),
        "max" => binary(args, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Value::Int(*x.max(y)),
            (Value::Real(x), Value::Real(y)) => Value::Real(x.max(*y)),
            (
                Value::Scalar {
                    si_value: x,
                    dimension: d1,
                },
                Value::Scalar {
                    si_value: y,
                    dimension: d2,
                },
            ) if d1 == d2 => Value::Scalar {
                si_value: x.max(*y),
                dimension: *d1,
            },
            _ => match (a.as_f64(), b.as_f64()) {
                (Some(x), Some(y)) => Value::Real(x.max(y)),
                _ => Value::Undef,
            },
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
                Value::Scalar {
                    si_value: xv,
                    dimension: dx,
                },
                Value::Scalar {
                    si_value: lov,
                    dimension: dlo,
                },
                Value::Scalar {
                    si_value: hiv,
                    dimension: dhi,
                },
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
                    Value::Scalar {
                        si_value: av,
                        dimension: da,
                    },
                    Value::Scalar {
                        si_value: bv,
                        dimension: db,
                    },
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
                    x.as_f64(),
                    from_lo.as_f64(),
                    from_hi.as_f64(),
                    to_lo.as_f64(),
                    to_hi.as_f64(),
                ) {
                    (Some(a), Some(b), Some(c), Some(d), Some(e)) => (a, b, c, d, e),
                    _ => return Value::Undef,
                };
                if flov == fhiv {
                    return Value::Undef; // early-exit: division by zero
                }
                let result = tlov + (xv - flov) * (thiv - tlov) / (fhiv - flov);
                return sanitize_value(Value::Scalar {
                    si_value: result,
                    dimension: to_dim,
                });
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
        "sin" => unary(args, |v| {
            trig_input(v).map_or(Value::Undef, |r| Value::Real(r.sin()))
        }),
        "cos" => unary(args, |v| {
            trig_input(v).map_or(Value::Undef, |r| Value::Real(r.cos()))
        }),
        "tan" => unary(args, |v| {
            trig_input(v).map_or(Value::Undef, |r| Value::Real(r.tan()))
        }),

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
            // Determine the output wrapper based on input variant.
            let wrap: fn(Vec<Value>) -> Value = match v {
                Value::Vector(_) => Value::Vector,
                Value::Point(_) => Value::Point,
                _ => Value::Tensor,
            };
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
            wrap(vals.iter().map(|x| Value::Real(x / mag)).collect())
        }),

        "magnitude" => unary(args, |v| {
            // Handle Complex before the Tensor fallback.
            if let Value::Complex { re, im, dimension } = v {
                return complex_abs(*re, *im, *dimension);
            }
            let (vals, dim) = match tensor_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            let sum_sq: f64 = vals.iter().map(|x| x * x).sum();
            let mag = sum_sq.sqrt();
            if dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(mag))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: mag,
                    dimension: dim,
                })
            }
        }),

        "cross" => binary(args, |a, b| {
            // Cross product of two vectors → vector; point inputs are
            // semantically invalid (cross is only defined for vectors).
            let wrap: fn(Vec<Value>) -> Value = match (a, b) {
                (Value::Point(_), _) | (_, Value::Point(_)) => return Value::Undef,
                (Value::Vector(_), Value::Vector(_)) => Value::Vector,
                _ => Value::Tensor,
            };
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
                    sanitize_value(Value::Scalar {
                        si_value: v,
                        dimension: result_dim,
                    })
                }
            };
            wrap(vec![
                make_component(cx),
                make_component(cy),
                make_component(cz),
            ])
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
                sanitize_value(Value::Scalar {
                    si_value: sum,
                    dimension: result_dim,
                })
            }
        }),

        // --- Complex number functions ---

        // complex(re, im) constructor: both args must be numeric with matching dimensions.
        // Returns Value::Complex { re, im, dimension }.
        // Returns Undef on: wrong arg count, non-numeric, mismatched dimensions, NaN/Inf.
        "complex" => {
            if args.len() != 2 {
                return Value::Undef;
            }
            let re = match args[0].as_f64() {
                Some(v) => v,
                None => return Value::Undef,
            };
            let im = match args[1].as_f64() {
                Some(v) => v,
                None => return Value::Undef,
            };
            let dim_re = args[0].dimension();
            let dim_im = args[1].dimension();
            if dim_re != dim_im {
                return Value::Undef;
            }
            if !re.is_finite() || !im.is_finite() {
                return Value::Undef;
            }
            Value::Complex {
                re,
                im,
                dimension: dim_re,
            }
        }

        // re(z) / real(z): extract real part. Returns Real if DIMENSIONLESS, Scalar otherwise.
        "re" | "real" => unary(args, |v| {
            sanitize_value(match v {
                Value::Complex { re, dimension, .. } => Value::from_component(*re, *dimension),
                _ => Value::Undef,
            })
        }),

        // im(z) / imag(z): extract imaginary part. Returns Real if DIMENSIONLESS, Scalar otherwise.
        "im" | "imag" => unary(args, |v| {
            sanitize_value(match v {
                Value::Complex { im, dimension, .. } => Value::from_component(*im, *dimension),
                _ => Value::Undef,
            })
        }),

        // conjugate(z): negate the imaginary part, preserve re and dimension.
        "conjugate" => unary(args, |v| match v {
            Value::Complex { re, im, dimension } => sanitize_value(Value::Complex {
                re: *re,
                im: -im,
                dimension: *dimension,
            }),
            _ => Value::Undef,
        }),

        // phase(z): compute atan2(im, re), return Scalar with ANGLE dimension.
        // phase(0+0i) is undefined — zero vector has no direction.
        "phase" => unary(args, |v| match v {
            Value::Complex { re, im, .. } => {
                if *re == 0.0 && *im == 0.0 {
                    return Value::Undef;
                }
                let angle = im.atan2(*re);
                sanitize_value(Value::Scalar {
                    si_value: angle,
                    dimension: DimensionVector::ANGLE,
                })
            }
            _ => Value::Undef,
        }),

        // complex_magnitude(z): compute sqrt(re²+im²) for Complex inputs only.
        // Returns Real if DIMENSIONLESS, Scalar otherwise.
        // Returns Undef for non-Complex inputs (unlike generic `magnitude` which handles Tensors).
        "complex_magnitude" => unary(args, |v| match v {
            Value::Complex { re, im, dimension } => complex_abs(*re, *im, *dimension),
            _ => Value::Undef,
        }),

        // complex_add(a, b): add two complex numbers with matching dimensions.
        "complex_add" => binary(args, |a, b| match (a, b) {
            (
                Value::Complex {
                    re: ar,
                    im: ai,
                    dimension: ad,
                },
                Value::Complex {
                    re: br,
                    im: bi,
                    dimension: bd,
                },
            ) => {
                if ad != bd {
                    return Value::Undef;
                }
                sanitize_value(Value::Complex {
                    re: ar + br,
                    im: ai + bi,
                    dimension: *ad,
                })
            }
            _ => Value::Undef,
        }),

        // complex_mul(a, b): multiply two complex numbers, combining dimensions via mul().
        // (a+bi)(c+di) = (ac-bd) + (ad+bc)i
        "complex_mul" => binary(args, |a, b| match (a, b) {
            (
                Value::Complex {
                    re: ar,
                    im: ai,
                    dimension: ad,
                },
                Value::Complex {
                    re: br,
                    im: bi,
                    dimension: bd,
                },
            ) => {
                let re = ar * br - ai * bi;
                let im = ar * bi + ai * br;
                let dimension = ad.mul(bd);
                sanitize_value(Value::Complex { re, im, dimension })
            }
            _ => Value::Undef,
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
                return Value::Undef;
            }
            let origin = &args[0];
            let basis = &args[1];
            // First arg must be a Point with exactly 3 components
            match origin {
                Value::Point(components) if components.len() == 3 => {}
                _ => return Value::Undef,
            }
            // Second arg must be an Orientation
            if !matches!(basis, Value::Orientation { .. }) {
                return Value::Undef;
            }
            Value::Frame {
                origin: Box::new(origin.clone()),
                basis: Box::new(basis.clone()),
            }
        }

        // --- Transform constructors ---
        "transform3" => {
            if args.len() != 2 {
                return Value::Undef;
            }
            let rotation = &args[0];
            let translation = &args[1];
            // First arg must be an Orientation
            if !matches!(rotation, Value::Orientation { .. }) {
                return Value::Undef;
            }
            // Second arg must be a Vector with exactly 3 components
            match translation {
                Value::Vector(components) if components.len() == 3 => {}
                _ => return Value::Undef,
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
                return Value::Undef;
            }
            // Both args must be Frames
            let (origin_from, basis_from) = match &args[0] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Value::Undef,
            };
            let (origin_to, basis_to) = match &args[1] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Value::Undef,
            };
            // Extract quaternions
            let q_from = match basis_from {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Value::Undef,
            };
            let q_to = match basis_to {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Value::Undef,
            };
            // Extract origin points as f64 triples with finiteness and dimension validation
            let (fx, fy, fz, f_dim) = match origin_from {
                Value::Point(comps) if comps.len() == 3 => {
                    match (comps[0].as_f64(), comps[1].as_f64(), comps[2].as_f64()) {
                        (Some(x), Some(y), Some(z)) => {
                            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                                return Value::Undef;
                            }
                            let dim = comps[0].dimension();
                            if comps[1].dimension() != dim || comps[2].dimension() != dim {
                                return Value::Undef;
                            }
                            (x, y, z, dim)
                        }
                        _ => return Value::Undef,
                    }
                }
                _ => return Value::Undef,
            };
            let (tx, ty, tz, t_dim) = match origin_to {
                Value::Point(comps) if comps.len() == 3 => {
                    match (comps[0].as_f64(), comps[1].as_f64(), comps[2].as_f64()) {
                        (Some(x), Some(y), Some(z)) => {
                            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                                return Value::Undef;
                            }
                            let dim = comps[0].dimension();
                            if comps[1].dimension() != dim || comps[2].dimension() != dim {
                                return Value::Undef;
                            }
                            (x, y, z, dim)
                        }
                        _ => return Value::Undef,
                    }
                }
                _ => return Value::Undef,
            };
            // R = R_to * conj(R_from)
            let r = quat_mul(q_to, quat_conj(q_from));
            // Normalize the result quaternion
            match normalize_quaternion(r.0, r.1, r.2, r.3) {
                Some(rot_val) => {
                    // t = origin_to - R * origin_from
                    if f_dim != t_dim {
                        return Value::Undef;
                    }
                    let dim = f_dim;
                    // Use the normalized quaternion for rotation to ensure
                    // consistency with the stored rotation in the result Transform
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
                return Value::Undef;
            }
            let min = &args[0];
            let max = &args[1];
            // Both args must be Point with exactly 3 components and matching dimensions
            let min_comps = match min {
                Value::Point(comps) if comps.len() == 3 => comps,
                _ => return Value::Undef,
            };
            let max_comps = match max {
                Value::Point(comps) if comps.len() == 3 => comps,
                _ => return Value::Undef,
            };
            // Dimensions must match
            let min_dim = min_comps
                .first()
                .map(|v| v.dimension())
                .unwrap_or(DimensionVector::DIMENSIONLESS);
            let max_dim = max_comps
                .first()
                .map(|v| v.dimension())
                .unwrap_or(DimensionVector::DIMENSIONLESS);
            if min_dim != max_dim {
                return Value::Undef;
            }
            Value::BoundingBox {
                min: Box::new(min.clone()),
                max: Box::new(max.clone()),
            }
        }

        // --- BoundingBox accessors ---
        "bbox_size" => {
            if args.len() != 1 {
                return Value::Undef;
            }
            match &args[0] {
                Value::BoundingBox { min, max } => {
                    let (min_vals, dim) = match tensor_components_f64(min) {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    let (max_vals, _) = match tensor_components_f64(max) {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    if min_vals.len() != 3 || max_vals.len() != 3 {
                        return Value::Undef;
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
                return Value::Undef;
            }
            match &args[0] {
                Value::BoundingBox { min, max } => {
                    let (min_vals, dim) = match tensor_components_f64(min) {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    let (max_vals, _) = match tensor_components_f64(max) {
                        Some(v) => v,
                        None => return Value::Undef,
                    };
                    if min_vals.len() != 3 || max_vals.len() != 3 {
                        return Value::Undef;
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
                return Value::Undef;
            }
            // Quaternion components are pure numbers — reject dimensioned Scalars.
            if args[0].dimension() != DimensionVector::DIMENSIONLESS
                || args[1].dimension() != DimensionVector::DIMENSIONLESS
                || args[2].dimension() != DimensionVector::DIMENSIONLESS
                || args[3].dimension() != DimensionVector::DIMENSIONLESS
            {
                return Value::Undef;
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
            // Defense-in-depth: reject NaN/Inf early (NaN bypasses IEEE 754 comparisons)
            if xc
                .iter()
                .chain(yc.iter())
                .chain(zc.iter())
                .any(|v| !v.is_finite())
            {
                return Value::Undef;
            }
            // Verify approximate orthonormality
            let tol = 1e-6;
            let mag_x = vec3_norm(xc[0], xc[1], xc[2]);
            let mag_y = vec3_norm(yc[0], yc[1], yc[2]);
            let mag_z = vec3_norm(zc[0], zc[1], zc[2]);
            if (mag_x - 1.0).abs() > tol || (mag_y - 1.0).abs() > tol || (mag_z - 1.0).abs() > tol {
                return Value::Undef;
            }
            let dot_xy = xc[0] * yc[0] + xc[1] * yc[1] + xc[2] * yc[2];
            let dot_xz = xc[0] * zc[0] + xc[1] * zc[1] + xc[2] * zc[2];
            let dot_yz = yc[0] * zc[0] + yc[1] * zc[1] + yc[2] * zc[2];
            if dot_xy.abs() > tol || dot_xz.abs() > tol || dot_yz.abs() > tol {
                return Value::Undef;
            }
            // Verify right-handedness via scalar triple product (determinant).
            // det(R) = x · (y × z). For a proper rotation (SO(3)), det ≈ +1.
            // Left-handed orthonormal bases have det = -1 and must be rejected.
            let det = xc[0] * (yc[1] * zc[2] - yc[2] * zc[1])
                + xc[1] * (yc[2] * zc[0] - yc[0] * zc[2])
                + xc[2] * (yc[0] * zc[1] - yc[1] * zc[0]);
            if (det - 1.0).abs() > tol {
                return Value::Undef;
            }
            // Rotation matrix from basis vectors (columns are the new axes)
            // R = [xc | yc | zc], where row i, col j = R[i][j]
            // R[0][0]=xc[0], R[1][0]=xc[1], R[2][0]=xc[2]
            // R[0][1]=yc[0], R[1][1]=yc[1], R[2][1]=yc[2]
            // R[0][2]=zc[0], R[1][2]=zc[1], R[2][2]=zc[2]
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
            let axis_norm = vec3_norm(ax, ay, az);
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

        // --- Point/Vector constructors ---
        "point2" => construct_point_or_vector(args, 2, true),
        "point3" => construct_point_or_vector(args, 3, true),
        "vec2" => construct_point_or_vector(args, 2, false),
        "vec3" => construct_point_or_vector(args, 3, false),

        // --- Field operations (stubs) ---
        // These are handled by reify-expr's eval_expr FunctionCall interceptor
        // for actual lambda application; the stdlib entries serve as documentation
        // and fallback for direct stdlib calls.
        "sample" => Value::Undef, // Requires EvalContext for lambda application
        "gradient" => Value::Undef, // Numeric differentiation not yet implemented
        "divergence" => Value::Undef, // Numeric differentiation not yet implemented
        "curl" => Value::Undef,   // Numeric differentiation not yet implemented

        // --- Advanced linear algebra: determinant, inverse, transpose, outer, trace, eigenvalues ---
        "determinant" => unary(args, |v| {
            let (n, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            if n != ncols {
                return Value::Undef; // must be square
            }
            let det = match n {
                1 => data[0],
                2 => data[0] * data[3] - data[1] * data[2],
                3 => {
                    // Sarrus / cofactor expansion along first row
                    let (a, b, c) = (data[0], data[1], data[2]);
                    let (d, e, f) = (data[3], data[4], data[5]);
                    let (g, h, i) = (data[6], data[7], data[8]);
                    a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
                }
                _ => return Value::Undef, // only 1×1, 2×2, 3×3 supported
            };
            let result_dim = dim.pow(n as i8);
            if result_dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(det))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: det,
                    dimension: result_dim,
                })
            }
        }),

        "inverse" => unary(args, |v| {
            let (n, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            if n != ncols {
                return Value::Undef;
            }
            let inv_dim = if dim == DimensionVector::DIMENSIONLESS {
                DimensionVector::DIMENSIONLESS
            } else {
                DimensionVector::DIMENSIONLESS.div(&dim)
            };
            match n {
                1 => {
                    if data[0] == 0.0 {
                        return Value::Undef;
                    }
                    build_matrix_value(1, 1, &[1.0 / data[0]], inv_dim)
                }
                2 => {
                    let det = data[0] * data[3] - data[1] * data[2];
                    if det == 0.0 {
                        return Value::Undef;
                    }
                    let inv_det = 1.0 / det;
                    let inv_data = [
                        data[3] * inv_det,
                        -data[1] * inv_det,
                        -data[2] * inv_det,
                        data[0] * inv_det,
                    ];
                    build_matrix_value(2, 2, &inv_data, inv_dim)
                }
                3 => {
                    let (a, b, c) = (data[0], data[1], data[2]);
                    let (d, e, f) = (data[3], data[4], data[5]);
                    let (g, h, i) = (data[6], data[7], data[8]);
                    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
                    if det == 0.0 {
                        return Value::Undef;
                    }
                    let inv_det = 1.0 / det;
                    // Cofactor matrix transposed (adjugate), divided by det
                    let inv_data = [
                        (e * i - f * h) * inv_det,
                        (c * h - b * i) * inv_det,
                        (b * f - c * e) * inv_det,
                        (f * g - d * i) * inv_det,
                        (a * i - c * g) * inv_det,
                        (c * d - a * f) * inv_det,
                        (d * h - e * g) * inv_det,
                        (b * g - a * h) * inv_det,
                        (a * e - b * d) * inv_det,
                    ];
                    build_matrix_value(3, 3, &inv_data, inv_dim)
                }
                _ => Value::Undef,
            }
        }),

        "transpose" => unary(args, |v| {
            let (nrows, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            let mut transposed = vec![0.0; nrows * ncols];
            for r in 0..nrows {
                for c in 0..ncols {
                    transposed[c * nrows + r] = data[r * ncols + c];
                }
            }
            build_matrix_value(ncols, nrows, &transposed, dim)
        }),

        "outer" => binary(args, |a, b| {
            let (a_vals, a_dim) = match tensor_components_f64(a) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let (b_vals, b_dim) = match tensor_components_f64(b) {
                Some(v) => v,
                None => return Value::Undef,
            };
            let nrows = a_vals.len();
            let ncols = b_vals.len();
            let result_dim = a_dim.mul(&b_dim);
            let mut data = Vec::with_capacity(nrows * ncols);
            for ai in &a_vals {
                for bj in &b_vals {
                    data.push(ai * bj);
                }
            }
            build_matrix_value(nrows, ncols, &data, result_dim)
        }),

        "trace" => unary(args, |v| {
            let (n, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            if n != ncols {
                return Value::Undef; // must be square
            }
            let tr: f64 = (0..n).map(|i| data[i * n + i]).sum();
            if dim == DimensionVector::DIMENSIONLESS {
                sanitize_value(Value::Real(tr))
            } else {
                sanitize_value(Value::Scalar {
                    si_value: tr,
                    dimension: dim,
                })
            }
        }),

        "eigenvalues" => unary(args, |v| {
            let (n, ncols, data, dim) = match matrix_components_f64(v) {
                Some(c) => c,
                None => return Value::Undef,
            };
            if n != ncols {
                return Value::Undef;
            }
            let make_val = |x: f64| -> Value {
                if dim == DimensionVector::DIMENSIONLESS {
                    sanitize_value(Value::Real(x))
                } else {
                    sanitize_value(Value::Scalar {
                        si_value: x,
                        dimension: dim,
                    })
                }
            };
            match n {
                1 => Value::List(vec![make_val(data[0])]),
                2 => {
                    // char poly: λ² - (a+d)λ + (ad-bc) = 0
                    let (a, b) = (data[0], data[1]);
                    let (c, d) = (data[2], data[3]);
                    let tr = a + d;
                    let det = a * d - b * c;
                    let disc = tr * tr - 4.0 * det;
                    if disc < 0.0 {
                        return Value::Undef; // complex eigenvalues
                    }
                    let sqrt_disc = disc.sqrt();
                    let mut eigs = vec![(tr + sqrt_disc) / 2.0, (tr - sqrt_disc) / 2.0];
                    eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    Value::List(eigs.into_iter().map(make_val).collect())
                }
                3 => {
                    // Characteristic polynomial: λ³ - pλ² + qλ - r = 0
                    // where p = tr(A), q = sum of 2×2 principal minor dets, r = det(A)
                    let (a, b, c) = (data[0], data[1], data[2]);
                    let (d, e, f) = (data[3], data[4], data[5]);
                    let (g, h, i) = (data[6], data[7], data[8]);

                    let p = a + e + i; // trace
                    let q = (a * e - b * d) + (a * i - c * g) + (e * i - f * h);
                    let r = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g); // det

                    // Depressed cubic: t³ + αt + β = 0 where λ = t + p/3
                    let p3 = p / 3.0;
                    let alpha = q - p * p / 3.0;
                    let beta = -2.0 * p * p * p / 27.0 + p * q / 3.0 - r;

                    // Discriminant for three real roots: 4α³ + 27β² ≤ 0
                    // Use trigonometric (Viète) method for the all-real-roots case
                    if alpha >= 0.0 {
                        // At most one real root when α ≥ 0 and β ≠ 0
                        if alpha == 0.0 && beta == 0.0 {
                            // Triple root
                            let root = p3;
                            Value::List(vec![make_val(root), make_val(root), make_val(root)])
                        } else if alpha == 0.0 {
                            // t³ = -β → single real cube root
                            let t = (-beta).cbrt();
                            // One real root + two complex; return Undef
                            // Actually: t³ + 0*t + β = 0 has one real root t = (-β)^(1/3)
                            // and two complex conjugate roots. But if beta = 0 handled above.
                            // For non-zero beta with alpha=0, we have a triple-like scenario
                            // returning only the real eigenvalue as Undef for now.
                            let _ = t;
                            Value::Undef
                        } else {
                            // General case with α > 0: complex eigenvalues possible
                            Value::Undef
                        }
                    } else {
                        // α < 0: use trigonometric method for three real roots
                        let neg_alpha = -alpha;
                        let m = (neg_alpha / 3.0).sqrt(); // sqrt(-α/3)
                        let cos_arg = -beta / (2.0 * m * m * m);
                        // Clamp for numerical stability
                        let cos_arg = cos_arg.clamp(-1.0, 1.0);
                        let theta = cos_arg.acos();
                        let two_m = 2.0 * m;

                        let mut eigs = vec![
                            two_m * (theta / 3.0).cos() + p3,
                            two_m * ((theta + 2.0 * std::f64::consts::PI) / 3.0).cos() + p3,
                            two_m * ((theta + 4.0 * std::f64::consts::PI) / 3.0).cos() + p3,
                        ];
                        eigs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                        Value::List(eigs.into_iter().map(make_val).collect())
                    }
                }
                _ => Value::Undef,
            }
        }),

        _ => Value::Undef,
    }
}

/// Validate args for a point/vector constructor and return `Value::Point` or `Value::Vector`.
///
/// Validates:
/// 1. `args.len() == expected_n`
/// 2. All args are numeric (Int, Real, or Scalar — `as_f64()` returns Some)
/// 3. All args share the same physical dimension
///
/// Returns `Value::Undef` on any validation failure.
/// When `is_point` is `true`, returns `Value::Point`; otherwise returns `Value::Vector`.
fn construct_point_or_vector(args: &[Value], expected_n: usize, is_point: bool) -> Value {
    if args.len() != expected_n {
        return Value::Undef;
    }
    // All args must be numeric
    if !args.iter().all(|a| a.as_f64().is_some()) {
        return Value::Undef;
    }
    // All args must share the same physical dimension
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

/// Apply a function to a single argument (by reference, for pattern matching).
fn unary(args: &[Value], f: impl FnOnce(&Value) -> Value) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    f(&args[0])
}

/// Compute the absolute value (modulus) of a complex number.
///
/// Uses [`f64::hypot`] for overflow-resistant magnitude computation,
/// avoiding premature overflow when components are large but the true
/// magnitude is still representable. Returns `Value::Real(mag)` when
/// `dimension` is dimensionless, or `Value::Scalar { si_value: mag,
/// dimension }` otherwise. Non-finite results are converted to `Undef`
/// by [`sanitize_value`].
fn complex_abs(re: f64, im: f64, dimension: DimensionVector) -> Value {
    let mag = re.hypot(im);
    sanitize_value(Value::from_component(mag, dimension))
}

/// Compute the Euclidean norm (magnitude) of a 3D vector.
///
/// Pure mathematical function — callers are responsible for checking finiteness
/// of the result if needed.
#[inline]
fn vec3_norm(x: f64, y: f64, z: f64) -> f64 {
    (x * x + y * y + z * z).sqrt()
}

/// Normalize a quaternion (w, x, y, z) to unit length.
///
/// Returns `None` if any component is non-finite or the quaternion has zero length.
fn normalize_quaternion(w: f64, x: f64, y: f64, z: f64) -> Option<Value> {
    if !w.is_finite() || !x.is_finite() || !y.is_finite() || !z.is_finite() {
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

/// Conjugate of a unit quaternion (equivalent to inverse for unit quaternions).
fn quat_conj(q: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    (q.0, -q.1, -q.2, -q.3)
}

/// Rotate a 3D vector by a unit quaternion: q * (0,v) * conj(q).
fn quat_rotate(q: (f64, f64, f64, f64), vx: f64, vy: f64, vz: f64) -> (f64, f64, f64) {
    let v_quat = (0.0, vx, vy, vz);
    let tmp = quat_mul(q, v_quat);
    let result = quat_mul(tmp, quat_conj(q));
    (result.1, result.2, result.3)
}

/// Convert non-finite f64 values (NaN, inf) to Undef.
///
/// This is a defense-in-depth catch-all applied at the return point of
/// `unary_f64` and `binary_f64` to ensure domain errors (e.g., sqrt(-1),
/// log(0), exp(1000) overflow) produce Undef instead of silently propagating
/// NaN or infinity through the evaluation graph.
// SYNC: mirror of reify-expr::sanitize_value — keep in sync
fn sanitize_value(v: Value) -> Value {
    match &v {
        Value::Real(x) if x.is_nan() || x.is_infinite() => Value::Undef,
        Value::Scalar { si_value, .. } if si_value.is_nan() || si_value.is_infinite() => {
            Value::Undef
        }
        Value::Complex { re, im, .. }
            if re.is_nan() || re.is_infinite() || im.is_nan() || im.is_infinite() =>
        {
            Value::Undef
        }
        Value::Orientation { w, x, y, z }
            if w.is_nan()
                || w.is_infinite()
                || x.is_nan()
                || x.is_infinite()
                || y.is_nan()
                || y.is_infinite()
                || z.is_nan()
                || z.is_infinite() =>
        {
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
        Value::Scalar {
            si_value,
            dimension,
        } => {
            if *dimension == DimensionVector::ANGLE && si_value.is_finite() {
                Some(*si_value)
            } else {
                None // dimension error or non-finite value
            }
        }
        Value::Real(r) if r.is_finite() => Some(*r),
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
/// Returns `None` for non-Tensor/Point/Vector values, empty containers, non-numeric
/// components, or containers with mixed dimensions.
/// Build a Plane from a single offset argument.
///
/// `offset_index` (0, 1, or 2) controls which component of the origin
/// receives the offset value; the other two components are zero with the
/// same dimension as the offset. `normal` is the dimensionless unit normal.
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
///
/// `direction` is the dimensionless unit direction as [x, y, z].
fn make_axis(args: &[Value], direction: [f64; 3]) -> Value {
    if args.len() != 1 {
        return Value::Undef;
    }
    // Arg must be a Point with exactly 3 components
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

fn tensor_components_f64(v: &Value) -> Option<(Vec<f64>, DimensionVector)> {
    let items = match v {
        Value::Tensor(items) | Value::Point(items) | Value::Vector(items) if !items.is_empty() => {
            items
        }
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

/// Extract a square or rectangular matrix from a `Value` into `(nrows, ncols, flat_data, element_dim)`.
///
/// Handles both `Value::Matrix(rows)` and nested `Value::Tensor` (rank-2 Tensor).
/// All elements must share the same dimension and be numeric.
fn matrix_components_f64(v: &Value) -> Option<(usize, usize, Vec<f64>, DimensionVector)> {
    enum Rows<'a> {
        Matrix(&'a [Vec<Value>]),
        Tensor(&'a [Value]),
    }
    let rows = match v {
        Value::Matrix(r) if !r.is_empty() => Rows::Matrix(r),
        Value::Tensor(items)
            if !items.is_empty() && items.iter().all(|r| matches!(r, Value::Tensor(_))) =>
        {
            Rows::Tensor(items)
        }
        _ => return None,
    };
    let (nrows, ncols) = match &rows {
        Rows::Matrix(r) => {
            let nc = r[0].len();
            if nc == 0 || r.iter().any(|row| row.len() != nc) {
                return None;
            }
            (r.len(), nc)
        }
        Rows::Tensor(items) => {
            let nc = match &items[0] {
                Value::Tensor(elems) => elems.len(),
                _ => return None,
            };
            if nc == 0
                || items.iter().any(|r| match r {
                    Value::Tensor(elems) => elems.len() != nc,
                    _ => true,
                })
            {
                return None;
            }
            (items.len(), nc)
        }
    };
    // Flatten and extract f64 values, checking uniform dimension.
    let first_elem = match &rows {
        Rows::Matrix(r) => &r[0][0],
        Rows::Tensor(items) => match &items[0] {
            Value::Tensor(elems) => &elems[0],
            _ => return None,
        },
    };
    let first_dim = first_elem.dimension();
    let mut data = Vec::with_capacity(nrows * ncols);
    let check_and_push = |elem: &Value, data: &mut Vec<f64>| -> bool {
        if elem.dimension() != first_dim {
            return false;
        }
        match elem.as_f64() {
            Some(x) => {
                data.push(x);
                true
            }
            None => false,
        }
    };
    match &rows {
        Rows::Matrix(r) => {
            for row in *r {
                for elem in row {
                    if !check_and_push(elem, &mut data) {
                        return None;
                    }
                }
            }
        }
        Rows::Tensor(items) => {
            for item in *items {
                if let Value::Tensor(elems) = item {
                    for elem in elems {
                        if !check_and_push(elem, &mut data) {
                            return None;
                        }
                    }
                }
            }
        }
    }
    Some((nrows, ncols, data, first_dim))
}

/// Build a nested `Value::Tensor` (rank-2) from flat f64 data.
fn build_matrix_value(nrows: usize, ncols: usize, data: &[f64], dim: DimensionVector) -> Value {
    let rows: Vec<Value> = (0..nrows)
        .map(|i| {
            let row: Vec<Value> = (0..ncols)
                .map(|j| {
                    let v = data[i * ncols + j];
                    if dim == DimensionVector::DIMENSIONLESS {
                        sanitize_value(Value::Real(v))
                    } else {
                        sanitize_value(Value::Scalar {
                            si_value: v,
                            dimension: dim,
                        })
                    }
                })
                .collect();
            Value::Tensor(row)
        })
        .collect();
    Value::Tensor(rows)
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
                Value::Scalar {
                    si_value,
                    dimension,
                } => {
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

    /// Assert that an expression evaluates to a 3-component wrapper variant
    /// (`Value::Tensor`, `Value::Vector`, or `Value::Point`) where each component
    /// is approximately equal to the expected `[x, y, z]` values within 1e-12.
    macro_rules! assert_vector3_approx {
        ($variant:ident, $expr:expr, [$ex:expr, $ey:expr, $ez:expr]) => {
            match $expr {
                Value::$variant(items) => {
                    assert_eq!(
                        items.len(),
                        3,
                        "expected 3-component {}",
                        stringify!($variant)
                    );
                    let vals: Vec<f64> = items.iter().map(|x| x.as_f64().unwrap()).collect();
                    assert!(
                        (vals[0] - $ex).abs() < 1e-12,
                        "x: expected {}, got {}",
                        $ex,
                        vals[0]
                    );
                    assert!(
                        (vals[1] - $ey).abs() < 1e-12,
                        "y: expected {}, got {}",
                        $ey,
                        vals[1]
                    );
                    assert!(
                        (vals[2] - $ez).abs() < 1e-12,
                        "z: expected {}, got {}",
                        $ez,
                        vals[2]
                    );
                }
                other => panic!(
                    "expected {}([{}, {}, {}]), got {:?}",
                    stringify!($variant),
                    $ex,
                    $ey,
                    $ez,
                    other
                ),
            }
        };
    }

    /// Build a `Value::Vector` of 3 `Value::Scalar` components from `[f64; 3]` and a dimension.
    fn make_scalar_vec3(vals: [f64; 3], dim: DimensionVector) -> Value {
        Value::Vector(
            vals.iter()
                .map(|&v| Value::Scalar {
                    si_value: v,
                    dimension: dim,
                })
                .collect(),
        )
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
        assert!(
            result.is_undef(),
            "sqrt(-1) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn log_zero_returns_undef() {
        let result = eval_builtin("log", &[Value::Real(0.0)]);
        assert!(
            result.is_undef(),
            "log(0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn log_negative_returns_undef() {
        let result = eval_builtin("log", &[Value::Real(-1.0)]);
        assert!(
            result.is_undef(),
            "log(-1) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn log10_zero_returns_undef() {
        let result = eval_builtin("log10", &[Value::Real(0.0)]);
        assert!(
            result.is_undef(),
            "log10(0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn log10_negative_returns_undef() {
        let result = eval_builtin("log10", &[Value::Real(-1.0)]);
        assert!(
            result.is_undef(),
            "log10(-1) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn exp_overflow_returns_undef() {
        let result = eval_builtin("exp", &[Value::Real(1000.0)]);
        assert!(
            result.is_undef(),
            "exp(1000) should be Undef (inf), got {:?}",
            result
        );
    }

    #[test]
    fn pow_negative_base_fractional_exp_returns_undef() {
        let result = eval_builtin("pow", &[Value::Real(-2.0), Value::Real(0.5)]);
        assert!(
            result.is_undef(),
            "pow(-2, 0.5) should be Undef (NaN), got {:?}",
            result
        );
    }

    // --- Inverse-trig domain errors and hyperbolic overflow (step-23) ---

    #[test]
    fn asin_out_of_range_positive() {
        let result = eval_builtin("asin", &[Value::Real(2.0)]);
        assert!(
            result.is_undef(),
            "asin(2.0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn asin_out_of_range_negative() {
        let result = eval_builtin("asin", &[Value::Real(-2.0)]);
        assert!(
            result.is_undef(),
            "asin(-2.0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn acos_out_of_range_positive() {
        let result = eval_builtin("acos", &[Value::Real(2.0)]);
        assert!(
            result.is_undef(),
            "acos(2.0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn acos_out_of_range_negative() {
        let result = eval_builtin("acos", &[Value::Real(-2.0)]);
        assert!(
            result.is_undef(),
            "acos(-2.0) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn sinh_overflow_returns_undef() {
        let result = eval_builtin("sinh", &[Value::Real(1000.0)]);
        assert!(
            result.is_undef(),
            "sinh(1000) should be Undef (inf), got {:?}",
            result
        );
    }

    #[test]
    fn cosh_overflow_returns_undef() {
        let result = eval_builtin("cosh", &[Value::Real(1000.0)]);
        assert!(
            result.is_undef(),
            "cosh(1000) should be Undef (inf), got {:?}",
            result
        );
    }

    // Boundary valid inputs: confirm no regressions on valid inputs

    #[test]
    fn asin_boundary_valid() {
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
            Value::Scalar {
                si_value,
                dimension,
            } => {
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
            Value::Scalar {
                si_value,
                dimension,
            } => {
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
            Value::Scalar {
                si_value,
                dimension,
            } => {
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
        assert!(
            result.is_undef(),
            "sqrt of negative Scalar should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn acos_boundary_valid() {
        let result = eval_builtin("acos", &[Value::Real(-1.0)]);
        match result {
            Value::Scalar {
                si_value,
                dimension,
            } => {
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
            lambda: Box::new(Value::Undef),
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
            lambda: Box::new(Value::Undef),
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
            lambda: Box::new(Value::Undef),
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
            lambda: Box::new(Value::Undef),
        };
        let result = eval_builtin("sample", &[field, Value::Int(42)]);
        assert!(
            result.is_undef(),
            "sample in stdlib should return Undef (handled in eval_expr), got {:?}",
            result
        );
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
        assert!(
            result.is_undef(),
            "mod by zero should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn mod_non_int_returns_undef() {
        let result = eval_builtin("mod", &[Value::Real(3.5), Value::Real(2.0)]);
        assert!(
            result.is_undef(),
            "mod on Real should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn mod_wrong_arg_count_returns_undef() {
        let result = eval_builtin("mod", &[Value::Int(7)]);
        assert!(
            result.is_undef(),
            "mod with 1 arg should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn mod_i64_min_neg1_returns_undef() {
        // i64::MIN % -1 overflows in Rust (panics in debug mode)
        let result = eval_builtin("mod", &[Value::Int(i64::MIN), Value::Int(-1)]);
        assert!(
            result.is_undef(),
            "mod(i64::MIN, -1) should be Undef (overflow), got {:?}",
            result
        );
    }

    // --- clamp Real tests (step-3) ---

    #[test]
    fn clamp_real_within_range() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            5.0
        );
    }

    #[test]
    fn clamp_real_below_lo() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(-3.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            0.0
        );
    }

    #[test]
    fn clamp_real_above_hi() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(15.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            10.0
        );
    }

    #[test]
    fn clamp_at_lo_boundary() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(0.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            0.0
        );
    }

    #[test]
    fn clamp_at_hi_boundary() {
        assert_real_approx!(
            eval_builtin(
                "clamp",
                &[Value::Real(10.0), Value::Real(0.0), Value::Real(10.0)]
            ),
            10.0
        );
    }

    #[test]
    fn clamp_nan_x_returns_undef() {
        // x is NaN — explicit x.is_nan() guard
        let result = eval_builtin(
            "clamp",
            &[Value::Real(f64::NAN), Value::Real(0.0), Value::Real(10.0)],
        );
        assert!(
            result.is_undef(),
            "clamp(NaN, 0, 10) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_nan_lo_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[Value::Real(5.0), Value::Real(f64::NAN), Value::Real(10.0)],
        );
        assert!(
            result.is_undef(),
            "clamp(5, NaN, 10) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_nan_hi_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[Value::Real(5.0), Value::Real(0.0), Value::Real(f64::NAN)],
        );
        assert!(
            result.is_undef(),
            "clamp(5, 0, NaN) should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_inverted_range_real_returns_undef() {
        // lo > hi is invalid
        let result = eval_builtin(
            "clamp",
            &[Value::Real(5.0), Value::Real(10.0), Value::Real(0.0)],
        );
        assert!(
            result.is_undef(),
            "clamp with inverted range should be Undef, got {:?}",
            result
        );
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
        assert!(
            result.is_undef(),
            "clamp Int with inverted range should be Undef, got {:?}",
            result
        );
    }

    // --- clamp Scalar + fallback tests (step-7) ---

    #[test]
    fn clamp_scalar_preserves_dimension() {
        // All three args: same LENGTH dimension, result should be LENGTH Scalar
        assert_scalar_approx!(
            eval_builtin(
                "clamp",
                &[
                    Value::Scalar {
                        si_value: 0.005,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.001,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.010,
                        dimension: DimensionVector::LENGTH
                    },
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
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::TIME,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "clamp with dimension mismatch should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_inverted_range_scalar_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "clamp Scalar with inverted range should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_scalar_nan_x_returns_undef() {
        let result = eval_builtin(
            "clamp",
            &[
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "clamp Scalar NaN x should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_wrong_arg_count_returns_undef() {
        let result = eval_builtin("clamp", &[Value::Real(5.0), Value::Real(0.0)]);
        assert!(
            result.is_undef(),
            "clamp with 2 args should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn clamp_fallback_dimension_mismatch_returns_undef() {
        // Fallback arm: x is Real (DIMENSIONLESS) but lo/hi are Scalar LENGTH.
        // The fallback cannot silently drop LENGTH → must return Undef.
        let result = eval_builtin(
            "clamp",
            &[
                Value::Real(5.0),
                Value::Scalar {
                    si_value: 0.001,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.010,
                    dimension: DimensionVector::LENGTH,
                },
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
            eval_builtin(
                "lerp",
                &[Value::Real(0.0), Value::Real(10.0), Value::Real(0.5)]
            ),
            5.0
        );
    }

    #[test]
    fn lerp_t_zero() {
        // lerp(a, b, 0) = a
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Real(3.0), Value::Real(7.0), Value::Real(0.0)]
            ),
            3.0
        );
    }

    #[test]
    fn lerp_t_one() {
        // lerp(a, b, 1) = b
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Real(3.0), Value::Real(7.0), Value::Real(1.0)]
            ),
            7.0
        );
    }

    #[test]
    fn lerp_negative_t_extrapolation() {
        // lerp(0, 10, -0.5) = -5 (extrapolation below)
        assert_real_approx!(
            eval_builtin(
                "lerp",
                &[Value::Real(0.0), Value::Real(10.0), Value::Real(-0.5)]
            ),
            -5.0
        );
    }

    #[test]
    fn lerp_nan_t_returns_undef() {
        // t is NaN — explicit NaN check after extraction
        let result = eval_builtin(
            "lerp",
            &[Value::Real(0.0), Value::Real(10.0), Value::Real(f64::NAN)],
        );
        assert!(
            result.is_undef(),
            "lerp with NaN t should be Undef, got {:?}",
            result
        );
    }

    // --- lerp Scalar + dimension tests (step-11) ---

    #[test]
    fn lerp_scalar_preserves_dimension() {
        // lerp(Scalar{0.0, LENGTH}, Scalar{1.0, LENGTH}, Real(0.5)) = Scalar{0.5, LENGTH}
        assert_scalar_approx!(
            eval_builtin(
                "lerp",
                &[
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 1.0,
                        dimension: DimensionVector::LENGTH
                    },
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
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::TIME,
                },
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp dimension mismatch a/b should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_t_dimensioned_returns_undef() {
        // t must be dimensionless; a LENGTH t is invalid
        let result = eval_builtin(
            "lerp",
            &[
                Value::Real(0.0),
                Value::Real(10.0),
                Value::Scalar {
                    si_value: 0.5,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with dimensioned t should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_nan_a_returns_undef() {
        // NaN in a -> Undef (via sanitize_value)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 1.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with NaN a should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn lerp_nan_b_returns_undef() {
        // NaN in b -> Undef (via sanitize_value)
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: f64::NAN,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Real(0.5),
            ],
        );
        assert!(
            result.is_undef(),
            "lerp with NaN b should be Undef, got {:?}",
            result
        );
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
        assert!(
            result.is_undef(),
            "lerp with 2 args should be Undef, got {:?}",
            result
        );
    }

    // --- lerp fallback tests (step-21) ---

    #[test]
    fn lerp_fallback_scalar_a_real_b_returns_undef() {
        // Fallback arm: a is Scalar LENGTH, b is Real → a's dimension would be silently
        // dropped if we returned Real. Per feedback_silent_defaults_pattern, must return Undef.
        let result = eval_builtin(
            "lerp",
            &[
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::LENGTH,
                },
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
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::LENGTH,
                },
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
                &[
                    Value::Real(5.0),
                    Value::Real(0.0),
                    Value::Real(10.0),
                    Value::Real(0.0),
                    Value::Real(100.0)
                ]
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
                &[
                    Value::Real(0.0),
                    Value::Real(0.0),
                    Value::Real(10.0),
                    Value::Real(20.0),
                    Value::Real(30.0)
                ]
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
                &[
                    Value::Real(10.0),
                    Value::Real(0.0),
                    Value::Real(10.0),
                    Value::Real(20.0),
                    Value::Real(30.0)
                ]
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
                &[
                    Value::Real(15.0),
                    Value::Real(0.0),
                    Value::Real(10.0),
                    Value::Real(0.0),
                    Value::Real(100.0)
                ]
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
                &[
                    Value::Real(50.0),
                    Value::Real(0.0),
                    Value::Real(100.0),
                    Value::Real(0.0),
                    Value::Real(10.0)
                ]
            ),
            5.0
        );
    }

    #[test]
    fn remap_division_by_zero_returns_undef() {
        // from_lo == from_hi -> division by zero -> Undef (early-exit)
        let result = eval_builtin(
            "remap",
            &[
                Value::Real(5.0),
                Value::Real(3.0),
                Value::Real(3.0),
                Value::Real(0.0),
                Value::Real(10.0),
            ],
        );
        assert!(
            result.is_undef(),
            "remap with from_lo==from_hi should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn remap_nan_returns_undef() {
        let result = eval_builtin(
            "remap",
            &[
                Value::Real(f64::NAN),
                Value::Real(0.0),
                Value::Real(10.0),
                Value::Real(0.0),
                Value::Real(100.0),
            ],
        );
        assert!(
            result.is_undef(),
            "remap with NaN x should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn remap_wrong_arg_count_returns_undef() {
        let result = eval_builtin(
            "remap",
            &[Value::Real(5.0), Value::Real(0.0), Value::Real(10.0)],
        );
        assert!(
            result.is_undef(),
            "remap with 3 args should be Undef, got {:?}",
            result
        );
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
                    Value::Scalar {
                        si_value: 5.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 10.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 100.0,
                        dimension: DimensionVector::LENGTH
                    },
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
                    Value::Scalar {
                        si_value: 5.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 10.0,
                        dimension: DimensionVector::LENGTH
                    },
                    Value::Scalar {
                        si_value: 0.0,
                        dimension: DimensionVector::TIME
                    },
                    Value::Scalar {
                        si_value: 100.0,
                        dimension: DimensionVector::TIME
                    },
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
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::TIME,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 100.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "remap with x dim != from dim should be Undef, got {:?}",
            result
        );
    }

    #[test]
    fn remap_scalar_to_range_mismatch_returns_undef() {
        // to_lo and to_hi have different dimensions -> Undef
        let result = eval_builtin(
            "remap",
            &[
                Value::Scalar {
                    si_value: 5.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 10.0,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.0,
                    dimension: DimensionVector::TIME,
                },
                Value::Scalar {
                    si_value: 100.0,
                    dimension: DimensionVector::LENGTH,
                }, // mismatch
            ],
        );
        assert!(
            result.is_undef(),
            "remap with to_lo/to_hi dim mismatch should be Undef, got {:?}",
            result
        );
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
        assert!(
            eval_builtin("dot", &[a, b]).is_undef(),
            "mismatched lengths should be Undef"
        );
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
        assert_vector3_approx!(Tensor, result, [0.6, 0.8, 0.0]);
    }

    #[test]
    fn normalize_zero_vector_returns_undef() {
        let v = Value::Tensor(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of zero vector should be Undef"
        );
    }

    #[test]
    fn normalize_dimensioned_vector_returns_real_components() {
        // normalize([3m,4m,0m]) should return Real components (dimensionless direction)
        let v = Value::Tensor(vec![
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let result = eval_builtin("normalize", &[v]);
        assert_vector3_approx!(Tensor, result, [0.6, 0.8, 0.0]);
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
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.004,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.000,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert_scalar_approx!(
            eval_builtin("magnitude", &[v]),
            0.005,
            DimensionVector::LENGTH
        );
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
        assert!(
            eval_builtin("magnitude", &[Value::Real(5.0)]).is_undef(),
            "magnitude of non-Tensor should be Undef"
        );
    }

    #[test]
    fn magnitude_empty_tensor_returns_undef() {
        let v = Value::Tensor(vec![]);
        assert!(
            eval_builtin("magnitude", &[v]).is_undef(),
            "magnitude of empty Tensor should be Undef"
        );
    }

    // --- cross() tests: dimensionless vectors (step-4) ---

    #[test]
    fn cross_x_hat_y_hat_equals_z_hat() {
        // cross([1,0,0], [0,1,0]) == [0,0,1]
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        assert_vector3_approx!(Tensor, result, [0.0, 0.0, 1.0]);
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
                    assert!(
                        (av + bv).abs() < 1e-12,
                        "anti-commutativity failed: {} + {} != 0",
                        av,
                        bv
                    );
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
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross on 2-element Tensor should be Undef"
        );
    }

    #[test]
    fn cross_length_4_tensor_returns_undef() {
        let a = Value::Tensor(vec![
            Value::Real(1.0),
            Value::Real(0.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        let b = Value::Tensor(vec![
            Value::Real(0.0),
            Value::Real(1.0),
            Value::Real(0.0),
            Value::Real(0.0),
        ]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross on 4-element Tensor should be Undef"
        );
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
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 0.0,
                dimension: reify_types::dimension::FORCE,
            },
            Value::Scalar {
                si_value: 1.0,
                dimension: reify_types::dimension::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: reify_types::dimension::FORCE,
            },
        ]);
        let result = eval_builtin("cross", &[a, b]);
        match result {
            Value::Tensor(items) => {
                assert_eq!(items.len(), 3, "cross product must have 3 components");
                // [1,0,0] x [0,1,0] = [0*0-0*1, 0*0-1*0, 1*1-0*0] = [0, 0, 1]
                for (i, item) in items.iter().enumerate() {
                    match item {
                        Value::Scalar {
                            si_value,
                            dimension,
                        } => {
                            assert_eq!(
                                *dimension, length_force,
                                "component {} dimension mismatch",
                                i
                            );
                            let expected = if i == 2 { 1.0 } else { 0.0 };
                            assert!(
                                (si_value - expected).abs() < 1e-12,
                                "component {}: expected {}, got {}",
                                i,
                                expected,
                                si_value
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
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: reify_types::dimension::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: reify_types::dimension::FORCE,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: reify_types::dimension::FORCE,
            },
        ]);
        assert_scalar_approx!(eval_builtin("dot", &[a, b]), 1.0, length_force);
    }

    // ── dot() with Value::Vector inputs (step-1) ────────────────────────────

    #[test]
    fn dot_vector_orthogonal() {
        // dot(Vector([1,0,0]), Vector([0,1,0])) == 0.0
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 0.0);
    }

    #[test]
    fn dot_vector_dimensioned() {
        // dot(Vector([1m,0,0]), Vector([1N,0,0])) -> Scalar{1.0, Length*Force}
        let length_force = DimensionVector::LENGTH.mul(&reify_types::dimension::FORCE);
        let a = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::LENGTH);
        let b = make_scalar_vec3([1.0, 0.0, 0.0], reify_types::dimension::FORCE);
        assert_scalar_approx!(eval_builtin("dot", &[a, b]), 1.0, length_force);
    }

    // ── cross() with Value::Vector inputs (step-3) ──────────────────────────

    #[test]
    fn cross_vector_returns_vector_wrapper() {
        // cross(Vector([1,0,0]), Vector([0,1,0])) must return Value::Vector([0,0,1])
        // NOT Value::Tensor — verifies wrapper-preservation at line 312
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        assert_vector3_approx!(Vector, result, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn cross_vector_dimensioned_preserves_dimension() {
        // cross(Vector([1m,0,0]), Vector([0,1N,0])) each component has Length*Force dimension
        let length_force = DimensionVector::LENGTH.mul(&reify_types::dimension::FORCE);
        let a = make_scalar_vec3([1.0, 0.0, 0.0], DimensionVector::LENGTH);
        let b = make_scalar_vec3([0.0, 1.0, 0.0], reify_types::dimension::FORCE);
        let result = eval_builtin("cross", &[a, b]);
        match result {
            Value::Vector(items) => {
                assert_eq!(items.len(), 3);
                // z component should be 1.0 m·N, others 0.0
                for item in &items {
                    match item {
                        Value::Scalar { dimension, .. } => {
                            assert_eq!(
                                *dimension, length_force,
                                "cross component dimension mismatch"
                            );
                        }
                        other => panic!("expected Scalar component, got {:?}", other),
                    }
                }
                let vals: Vec<f64> = items.iter().map(|x| x.as_f64().unwrap()).collect();
                assert!(
                    (vals[2] - 1.0).abs() < 1e-12,
                    "z: expected 1.0, got {}",
                    vals[2]
                );
            }
            other => panic!(
                "expected Value::Vector for dimensioned cross, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn cross_2d_vector_returns_undef() {
        // cross of 2-element Value::Vector returns Undef (cross is only defined for 3-vectors)
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross of 2-element Vector should be Undef"
        );
    }

    // ── normalize() with Value::Vector inputs (step-5) ──────────────────────

    #[test]
    fn normalize_vector_returns_vector_wrapper() {
        // normalize(Vector([3,4,0])) returns Value::Vector([0.6,0.8,0.0]) with Real components
        // NOT Value::Tensor — verifies wrapper-preservation at line 266
        let v = Value::Vector(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let result = eval_builtin("normalize", &[v]);
        assert_vector3_approx!(Vector, result, [0.6, 0.8, 0.0]);
    }

    #[test]
    fn normalize_zero_vector_input_returns_undef() {
        // normalize(Vector([0,0,0])) -> Undef
        let v = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("normalize", &[v]).is_undef(),
            "normalize of zero Vector should be Undef"
        );
    }

    #[test]
    fn normalize_dimensioned_vector_input() {
        // normalize(Vector([3m,4m,0m])) -> Value::Vector with dimensionless Real components
        let v = make_scalar_vec3([3.0, 4.0, 0.0], DimensionVector::LENGTH);
        let result = eval_builtin("normalize", &[v]);
        assert_vector3_approx!(Vector, result, [0.6, 0.8, 0.0]);
    }

    // ── magnitude() with Value::Vector inputs (step-7) ──────────────────────

    #[test]
    fn magnitude_vector_3_4_0() {
        // magnitude(Vector([3,4,0])) == Real(5.0)
        let v = Value::Vector(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        assert_real_approx!(eval_builtin("magnitude", &[v]), 5.0);
    }

    #[test]
    fn magnitude_vector_dimensioned() {
        // magnitude(Vector([3mm,4mm,0])) == Scalar{0.005, LENGTH}
        // 3mm=0.003m, 4mm=0.004m -> magnitude=0.005m
        let v = Value::Vector(vec![
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.004,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert_scalar_approx!(
            eval_builtin("magnitude", &[v]),
            0.005,
            DimensionVector::LENGTH
        );
    }

    // Private helper: emits four per-component closeness asserts given
    // pre-bound locals `tol`, `w`/`x`/`y`/`z` (actual) and expected exprs.
    // Not exported — only used by the arms of `assert_orientation_approx!`.
    macro_rules! __assert_orientation_close {
        ($tol:ident, $w:ident, $x:ident, $y:ident, $z:ident,
         $ew:expr, $ex:expr, $ey:expr, $ez:expr) => {
            assert!(($w - $ew).abs() < $tol, "w: expected {}, got {}", $ew, $w);
            assert!(($x - $ex).abs() < $tol, "x: expected {}, got {}", $ex, $x);
            assert!(($y - $ey).abs() < $tol, "y: expected {}, got {}", $ey, $y);
            assert!(($z - $ez).abs() < $tol, "z: expected {}, got {}", $ez, $z);
        };
    }

    /// Assert that an expression evaluates to `Value::Orientation { w, x, y, z }`.
    ///
    /// Three calling forms:
    /// - `assert_orientation_approx!(expr, w, x, y, z)` — sign-sensitive, tolerance 1e-12,
    ///   emits per-component labeled diagnostics.
    /// - `assert_orientation_approx!(expr, w, x, y, z, tol = T)` — sign-sensitive with
    ///   explicit tolerance, same per-component diagnostics.
    /// - `assert_orientation_approx!(expr, w, x, y, z, sign_insensitive = T)` — accepts
    ///   ±quaternion within explicit tolerance, single combined diagnostic.
    macro_rules! assert_orientation_approx {
        // Default tolerance (1e-12), sign-sensitive, per-component diagnostics.
        ($expr:expr, $ew:expr, $ex:expr, $ey:expr, $ez:expr) => {
            assert_orientation_approx!($expr, $ew, $ex, $ey, $ez, tol = 1e-12)
        };
        // Explicit tolerance, sign-sensitive, per-component diagnostics.
        ($expr:expr, $ew:expr, $ex:expr, $ey:expr, $ez:expr, tol = $tol:expr) => {
            let tol = $tol;
            match $expr {
                Value::Orientation { w, x, y, z } => {
                    __assert_orientation_close!(tol, w, x, y, z, $ew, $ex, $ey, $ez);
                }
                other => panic!(
                    "expected Orientation({}, {}, {}, {}), got {:?}",
                    $ew, $ex, $ey, $ez, other
                ),
            }
        };
        // Sign-insensitive: accepts ±quaternion within explicit tolerance.
        ($expr:expr, $ew:expr, $ex:expr, $ey:expr, $ez:expr, sign_insensitive = $tol:expr) => {
            let tol = $tol;
            match $expr {
                Value::Orientation { w, x, y, z } => {
                    let pos_ok = (w - $ew).abs() < tol
                        && (x - $ex).abs() < tol
                        && (y - $ey).abs() < tol
                        && (z - $ez).abs() < tol;
                    let neg_ok = (w + $ew).abs() < tol
                        && (x + $ex).abs() < tol
                        && (y + $ey).abs() < tol
                        && (z + $ez).abs() < tol;
                    assert!(
                        pos_ok || neg_ok,
                        "expected Orientation(\u{b1}{}, \u{b1}{}, \u{b1}{}, \u{b1}{}) within {}, got ({}, {}, {}, {})",
                        $ew, $ex, $ey, $ez, tol, w, x, y, z
                    );
                }
                other => panic!(
                    "expected Orientation(\u{b1}{}, \u{b1}{}, \u{b1}{}, \u{b1}{}), got {:?}",
                    $ew, $ex, $ey, $ez, other
                ),
            }
        };
    }

    // ── assert_orientation_approx diagnostic tests ──────────────────────────

    #[test]
    fn orient_identity_per_component_diagnostic() {
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                0.5, // wrong w
                0.0,
                0.0,
                0.0
            );
        });
        let err = result.expect_err("expected assert_orientation_approx to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("w:"),
            "expected panic message to contain 'w:', got: {msg:?}"
        );
    }

    #[test]
    fn orient_per_component_diagnostic_x() {
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                1.0,
                0.5, // wrong x
                0.0,
                0.0
            );
        });
        let err = result.expect_err("expected assert_orientation_approx to panic");
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

    #[test]
    fn orient_per_component_diagnostic_y() {
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Orientation {
                    w: 1.0,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                1.0,
                0.0,
                0.5, // wrong y
                0.0
            );
        });
        let err = result.expect_err("expected assert_orientation_approx to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("y:"),
            "expected panic message to contain 'y:', got: {msg:?}"
        );
    }

    #[test]
    fn orient_per_component_diagnostic_z() {
        let result = std::panic::catch_unwind(|| {
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
        });
        let err = result.expect_err("expected assert_orientation_approx to panic");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("z:"),
            "expected panic message to contain 'z:', got: {msg:?}"
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
        let err = result.expect_err("expected assert_orientation_approx sign_insensitive to panic for wrong value");
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

    // ── non-Orientation arm regression tests (step-1) ───────────────────────

    #[test]
    fn sign_insensitive_macro_rejects_non_orientation() {
        // Passing Value::Real(1.0) to the sign_insensitive arm must hit the
        // `other => panic!` branch and produce a message that mentions
        // "expected Orientation(±" and "got".
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Real(1.0),
                1.0,
                0.0,
                0.0,
                0.0,
                sign_insensitive = 1e-10
            );
        });
        let err = result.expect_err("expected assert_orientation_approx sign_insensitive to panic for non-Orientation value");
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

    #[test]
    fn strict_macro_rejects_non_orientation() {
        // Passing Value::Real(1.0) to the strict tol arm must hit the
        // `other => panic!` branch and produce a message that mentions
        // "expected Orientation(" and "got".
        let result = std::panic::catch_unwind(|| {
            assert_orientation_approx!(
                Value::Real(1.0),
                1.0,
                0.0,
                0.0,
                0.0,
                tol = 1e-10
            );
        });
        let err = result.expect_err("expected assert_orientation_approx tol arm to panic for non-Orientation value");
        let msg = err
            .downcast_ref::<String>()
            .map(|s| s.as_str())
            .or_else(|| err.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            msg.contains("expected Orientation("),
            "expected panic message to contain 'expected Orientation(', got: {msg:?}"
        );
        assert!(
            msg.contains("got"),
            "expected panic message to contain 'got', got: {msg:?}"
        );
    }

    // ── tol eval-once tests (steps 2 & 4) ───────────────────────────────────

    #[test]
    fn strict_macro_evaluates_tol_once() {
        // $tol is referenced 4 times in the strict tol arm today, so a
        // side-effecting expression would be evaluated 4 times before the
        // fix in step-5.  After the fix this test asserts exactly 1 evaluation.
        let counter = std::cell::Cell::new(0usize);
        assert_orientation_approx!(
            Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
            1.0, 0.0, 0.0, 0.0,
            tol = { counter.set(counter.get() + 1); 1e-12 }
        );
        assert_eq!(counter.get(), 1, "tol expression must be evaluated exactly once");
    }

    #[test]
    fn sign_insensitive_macro_evaluates_tol_once() {
        // $tol is referenced 9 times in the sign_insensitive arm today, so
        // a side-effecting expression would be evaluated 9 times before the
        // fix in step-3.  After the fix this test asserts exactly 1 evaluation.
        let counter = std::cell::Cell::new(0usize);
        assert_orientation_approx!(
            Value::Orientation { w: 1.0, x: 0.0, y: 0.0, z: 0.0 },
            1.0, 0.0, 0.0, 0.0,
            sign_insensitive = { counter.set(counter.get() + 1); 1e-10 }
        );
        assert_eq!(counter.get(), 1, "tol expression must be evaluated exactly once");
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

    #[test]
    fn dot_mixed_component_dimensions_returns_undef() {
        // A Tensor with mixed dimensions is not a valid physical vector
        let a = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::MASS,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert!(
            eval_builtin("dot", &[a, b]).is_undef(),
            "dot of vector with mixed component dimensions should be Undef"
        );
    }

    // ── complex() constructor tests (step-1) ──────────────────────────────────

    #[test]
    fn complex_real_real_returns_dimensionless() {
        // complex(Real, Real) → Complex with DIMENSIONLESS dimension
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(4.0)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "expected re=3.0, got {}", re);
                assert!((im - 4.0).abs() < 1e-12, "expected im=4.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{3,4,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_int_int_returns_dimensionless() {
        // complex(Int, Int) → Complex with DIMENSIONLESS dimension
        let result = eval_builtin("complex", &[Value::Int(5), Value::Int(-2)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 5.0).abs() < 1e-12, "expected re=5.0, got {}", re);
                assert!((im - (-2.0)).abs() < 1e-12, "expected im=-2.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{5,-2,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_int_real_mixed_coercion_dimensionless() {
        // complex(Int, Real) → Complex with DIMENSIONLESS dimension (both dimensionless)
        let result = eval_builtin("complex", &[Value::Int(1), Value::Real(2.5)]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 1.0).abs() < 1e-12, "expected re=1.0, got {}", re);
                assert!((im - 2.5).abs() < 1e-12, "expected im=2.5, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{1,2.5,DIMLESS}}, got {:?}", other),
        }
    }

    // ── complex() with Scalar args (step-3) ───────────────────────────────────

    #[test]
    fn complex_scalar_mm_preserves_length_dimension() {
        // complex(Scalar{5mm}, Scalar{3mm}) → Complex{0.005, 0.003, LENGTH}
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 0.005,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 0.003,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 0.005).abs() < 1e-15, "expected re=0.005, got {}", re);
                assert!((im - 0.003).abs() < 1e-15, "expected im=0.003, got {}", im);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{0.005,0.003,LENGTH}}, got {:?}", other),
        }
    }

    // ── complex() error cases (step-5) ────────────────────────────────────────

    #[test]
    fn complex_dimension_mismatch_returns_undef() {
        // complex(3mm, 4s) → Undef (LENGTH ≠ TIME)
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 0.003,
                    dimension: DimensionVector::LENGTH,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: DimensionVector::TIME,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for dimension mismatch, got {:?}",
            result
        );
    }

    #[test]
    fn complex_real_with_scalar_dimension_mismatch_returns_undef() {
        // complex(Real(3.0), Scalar{4, LENGTH}) → Undef
        // Real is DIMENSIONLESS, Scalar{LENGTH} is not — mismatch
        let result = eval_builtin(
            "complex",
            &[
                Value::Real(3.0),
                Value::Scalar {
                    si_value: 4.0,
                    dimension: DimensionVector::LENGTH,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for Real+Scalar mismatch, got {:?}",
            result
        );
    }

    #[test]
    fn complex_zero_args_returns_undef() {
        let result = eval_builtin("complex", &[]);
        assert!(
            result.is_undef(),
            "expected Undef for 0 args, got {:?}",
            result
        );
    }

    #[test]
    fn complex_three_args_returns_undef() {
        let result = eval_builtin(
            "complex",
            &[Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)],
        );
        assert!(
            result.is_undef(),
            "expected Undef for 3 args, got {:?}",
            result
        );
    }

    #[test]
    fn complex_non_numeric_re_returns_undef() {
        let result = eval_builtin("complex", &[Value::Bool(true), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for non-numeric re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_nan_arg_returns_undef() {
        let result = eval_builtin("complex", &[Value::Real(f64::NAN), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for NaN re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_inf_arg_returns_undef() {
        let result = eval_builtin("complex", &[Value::Real(f64::INFINITY), Value::Real(3.0)]);
        assert!(
            result.is_undef(),
            "expected Undef for Inf re, got {:?}",
            result
        );
    }

    #[test]
    fn complex_nan_im_arg_returns_undef() {
        // NaN in the imaginary (second) arg should also produce Undef
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(f64::NAN)]);
        assert!(
            result.is_undef(),
            "expected Undef for NaN im, got {:?}",
            result
        );
    }

    #[test]
    fn complex_inf_im_arg_returns_undef() {
        // Infinity in the imaginary (second) arg should also produce Undef
        let result = eval_builtin("complex", &[Value::Real(3.0), Value::Real(f64::INFINITY)]);
        assert!(
            result.is_undef(),
            "expected Undef for Inf im, got {:?}",
            result
        );
    }

    // ── re() and im() accessor tests (step-7) ────────────────────────────────

    #[test]
    fn re_dimensionless_returns_real() {
        // re(Complex{3,4,DIMLESS}) → Real(3.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("re", &[z]), 3.0);
    }

    #[test]
    fn im_dimensionless_returns_real() {
        // im(Complex{3,4,DIMLESS}) → Real(4.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("im", &[z]), 4.0);
    }

    #[test]
    fn re_dimensioned_returns_scalar() {
        // re(Complex{5,3,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("re", &[z]), 5.0, DimensionVector::LENGTH);
    }

    #[test]
    fn im_dimensioned_returns_scalar() {
        // im(Complex{5,3,LENGTH}) → Scalar{3.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("im", &[z]), 3.0, DimensionVector::LENGTH);
    }

    #[test]
    fn re_non_complex_returns_undef() {
        assert!(eval_builtin("re", &[Value::Real(3.0)]).is_undef());
    }

    #[test]
    fn im_non_complex_returns_undef() {
        assert!(eval_builtin("im", &[Value::Real(3.0)]).is_undef());
    }

    // ── conjugate() tests (step-9) ────────────────────────────────────────────

    #[test]
    fn conjugate_dimensionless_negates_im() {
        // conjugate(Complex{3,4,DIMLESS}) → Complex{3,-4,DIMLESS}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12);
                assert!((im - (-4.0)).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{3,-4,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn conjugate_dimensioned_preserves_dimension() {
        // conjugate(Complex{5,3,LENGTH}) → Complex{5,-3,LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 5.0).abs() < 1e-12);
                assert!((im - (-3.0)).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{5,-3,LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn conjugate_non_complex_returns_undef() {
        assert!(eval_builtin("conjugate", &[Value::Real(3.0)]).is_undef());
    }

    #[test]
    fn conjugate_nan_re_returns_undef() {
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with NaN re must return Undef"
        );
    }

    #[test]
    fn conjugate_nan_im_returns_undef() {
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with NaN im must return Undef"
        );
    }

    #[test]
    fn conjugate_inf_re_returns_undef() {
        let z = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with Inf re must return Undef"
        );
    }

    #[test]
    fn conjugate_inf_im_returns_undef() {
        let z = Value::Complex {
            re: 1.0,
            im: f64::NEG_INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("conjugate", &[z]).is_undef(),
            "conjugate of Complex with -Inf im must return Undef"
        );
    }

    // ── magnitude on Complex tests (step-11) ─────────────────────────────────

    #[test]
    fn magnitude_complex_dimensionless_3_4_returns_5() {
        // magnitude(Complex{3,4,DIMLESS}) → Real(5.0) (3-4-5 Pythagorean triple)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 5.0);
    }

    #[test]
    fn magnitude_complex_dimensioned_3_4_returns_scalar_5() {
        // magnitude(Complex{3,4,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("magnitude", &[z]),
            5.0,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn magnitude_large_representable_complex_no_overflow() {
        // magnitude(Complex{1e200, 0, DIMLESS}) must return Real(1e200), not Undef.
        // Covers the generic 'magnitude' builtin path to complex_abs.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 1e200);
    }

    #[test]
    fn magnitude_zero_complex_returns_zero() {
        // magnitude(0+0i) = 0.0 (zero vector has zero magnitude, unlike phase which is undef)
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("magnitude", &[z]), 0.0);
    }

    #[test]
    fn complex_magnitude_zero_complex_returns_zero() {
        // complex_magnitude(0+0i) = 0.0
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("complex_magnitude", &[z]), 0.0);
    }

    // ── phase() tests (step-13) ───────────────────────────────────────────────

    #[test]
    fn phase_complex_1_1_returns_pi_over_4() {
        // phase(1+1i) = π/4
        let z = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_4,
            DimensionVector::ANGLE
        );
    }

    #[test]
    fn phase_complex_1_0_returns_0() {
        // phase(1+0i) = 0
        let z = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(eval_builtin("phase", &[z]), 0.0, DimensionVector::ANGLE);
    }

    #[test]
    fn phase_complex_0_1_returns_pi_over_2() {
        // phase(0+1i) = π/2
        let z = Value::Complex {
            re: 0.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_2,
            DimensionVector::ANGLE
        );
    }

    #[test]
    fn phase_non_complex_returns_undef() {
        assert!(eval_builtin("phase", &[Value::Real(1.0)]).is_undef());
    }

    #[test]
    fn phase_zero_complex_returns_undef() {
        // phase(0+0i) is mathematically undefined (zero vector has no direction)
        let z = Value::Complex {
            re: 0.0,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("phase", &[z]).is_undef(),
            "phase(0+0i) should be Undef, not Scalar{{0.0, ANGLE}}"
        );
    }

    // ── complex_add() tests (step-15) ─────────────────────────────────────────

    #[test]
    fn complex_add_dimensionless() {
        // complex_add(1+2i, 3+4i) = 4+6i
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_add", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 4.0).abs() < 1e-12);
                assert!((im - 6.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{4,6,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_add_dimensioned_preserves_dimension() {
        // complex_add(a+bi [LENGTH], c+di [LENGTH]) = (a+c)+(b+d)i [LENGTH]
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("complex_add", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 4.0).abs() < 1e-12);
                assert!((im - 6.0).abs() < 1e-12);
                assert_eq!(dimension, DimensionVector::LENGTH);
            }
            other => panic!("expected Complex{{4,6,LENGTH}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_add_dimension_mismatch_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_add", &[a, b]).is_undef());
    }

    #[test]
    fn complex_add_non_complex_arg_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_add", &[a, Value::Real(3.0)]).is_undef());
    }

    // ── complex_mul() tests (step-17) ─────────────────────────────────────────

    #[test]
    fn complex_mul_dimensionless() {
        // (1+2i)(3+4i) = (3-8) + (4+6)i = -5 + 10i
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_mul", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - (-5.0)).abs() < 1e-12, "expected re=-5.0, got {}", re);
                assert!((im - 10.0).abs() < 1e-12, "expected im=10.0, got {}", im);
                assert_eq!(dimension, DimensionVector::DIMENSIONLESS);
            }
            other => panic!("expected Complex{{-5,10,DIMLESS}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_mul_dimensioned_combines_dimensions() {
        // complex_mul(LENGTH, LENGTH) → result dimension is LENGTH^2 (AREA)
        let area_dim = DimensionVector::LENGTH.mul(&DimensionVector::LENGTH);
        let a = Value::Complex {
            re: 1.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let b = Value::Complex {
            re: 2.0,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        let result = eval_builtin("complex_mul", &[a, b]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 2.0).abs() < 1e-12, "expected re=2.0, got {}", re);
                assert!((im - 0.0).abs() < 1e-12, "expected im=0.0, got {}", im);
                assert_eq!(dimension, area_dim, "expected AREA dimension");
            }
            other => panic!("expected Complex{{2,0,AREA}}, got {:?}", other),
        }
    }

    #[test]
    fn complex_mul_non_complex_returns_undef() {
        let a = Value::Complex {
            re: 1.0,
            im: 2.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(eval_builtin("complex_mul", &[a, Value::Real(3.0)]).is_undef());
    }

    // ── Complex<Impedance> integration test (step-19) ─────────────────────────

    #[test]
    fn complex_impedance_integration() {
        // Impedance = kg·m²·s⁻³·A⁻² = MASS·LENGTH²·TIME⁻³·CURRENT⁻²
        // Build as MASS * LENGTH^2 * TIME^-3 * CURRENT^-2
        use reify_types::DimensionVector;
        let mass_dim = DimensionVector::MASS;
        let length_dim = DimensionVector::LENGTH;
        let area = length_dim.mul(&length_dim);
        let mass_area = mass_dim.mul(&area);
        let time3 = DimensionVector::TIME.pow(3);
        let current2 = DimensionVector::CURRENT.pow(2);
        let impedance = mass_area.div(&time3).div(&current2);

        // Create 50 Ω (real part) + -25j Ω (imaginary part)
        let z = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 50.0,
                    dimension: impedance,
                },
                Value::Scalar {
                    si_value: -25.0,
                    dimension: impedance,
                },
            ],
        );
        match &z {
            Value::Complex { re, im, dimension } => {
                assert!((re - 50.0).abs() < 1e-12, "re={}", re);
                assert!((im - (-25.0)).abs() < 1e-12, "im={}", im);
                assert_eq!(*dimension, impedance);
            }
            other => panic!("expected Complex (impedance), got {:?}", other),
        }

        // re accessor → Scalar{50, IMPEDANCE}
        assert_scalar_approx!(
            eval_builtin("re", std::slice::from_ref(&z)),
            50.0,
            impedance
        );

        // im accessor → Scalar{-25, IMPEDANCE}
        assert_scalar_approx!(
            eval_builtin("im", std::slice::from_ref(&z)),
            -25.0,
            impedance
        );

        // magnitude → Scalar{sqrt(50²+25²), IMPEDANCE} = Scalar{sqrt(3125), IMPEDANCE}
        let expected_mag = (50.0_f64 * 50.0 + 25.0 * 25.0).sqrt();
        assert_scalar_approx!(
            eval_builtin("magnitude", std::slice::from_ref(&z)),
            expected_mag,
            impedance
        );

        // conjugate → Complex{50, 25, IMPEDANCE}
        let conj = eval_builtin("conjugate", std::slice::from_ref(&z));
        match &conj {
            Value::Complex { re, im, dimension } => {
                assert!((re - 50.0).abs() < 1e-12);
                assert!((im - 25.0).abs() < 1e-12);
                assert_eq!(*dimension, impedance);
            }
            other => panic!("expected conjugate Complex, got {:?}", other),
        }

        // phase → Scalar{atan2(-25, 50), ANGLE}
        let expected_phase = (-25.0_f64).atan2(50.0);
        assert_scalar_approx!(
            eval_builtin("phase", std::slice::from_ref(&z)),
            expected_phase,
            DimensionVector::ANGLE
        );
    }

    // ── Voltage dimension spec tests (step-7) ────────────────────────────────

    /// Build Voltage dimension: V = kg·m²·s⁻³·A⁻¹
    fn voltage_dim() -> DimensionVector {
        let mass = DimensionVector::MASS;
        let length = DimensionVector::LENGTH;
        let area = length.mul(&length);
        let mass_area = mass.mul(&area);
        let time3 = DimensionVector::TIME.pow(3);
        let current1 = DimensionVector::CURRENT.pow(1);
        mass_area.div(&time3).div(&current1)
    }

    #[test]
    fn complex_voltage_preserves_dimension() {
        // complex(Scalar{3,V}, Scalar{4,V}) → Complex{3,4,V}
        let v = voltage_dim();
        let z = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 3.0,
                    dimension: v,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: v,
                },
            ],
        );
        match &z {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "re={}", re);
                assert!((im - 4.0).abs() < 1e-12, "im={}", im);
                assert_eq!(*dimension, v, "dimension should be Voltage");
            }
            other => panic!("expected Complex{{3,4,V}}, got {:?}", other),
        }
    }

    #[test]
    fn real_voltage_returns_scalar() {
        // real(complex_voltage) → Scalar{3, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("real", &[z]), 3.0, v);
    }

    #[test]
    fn imag_voltage_returns_scalar() {
        // imag(complex_voltage) → Scalar{4, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("imag", &[z]), 4.0, v);
    }

    #[test]
    fn complex_magnitude_voltage() {
        // complex_magnitude(Complex{3,4,V}) → Scalar{5.0, V}
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        assert_scalar_approx!(eval_builtin("complex_magnitude", &[z]), 5.0, v);
    }

    #[test]
    fn conjugate_voltage_preserves_dim() {
        // conjugate flips im sign, preserves voltage dimension
        let v = voltage_dim();
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: v,
        };
        let result = eval_builtin("conjugate", &[z]);
        match result {
            Value::Complex { re, im, dimension } => {
                assert!((re - 3.0).abs() < 1e-12, "re={}", re);
                assert!((im - (-4.0)).abs() < 1e-12, "im={}", im);
                assert_eq!(dimension, v, "dimension should be Voltage");
            }
            other => panic!("expected Complex{{3,-4,V}}, got {:?}", other),
        }
    }

    // ── Dimension mismatch spec test (step-8) ─────────────────────────────────

    #[test]
    fn complex_voltage_current_mismatch_returns_undef() {
        // complex(Scalar{3, Voltage}, Scalar{4, Current}) → Undef (mismatched dims)
        let voltage = voltage_dim();
        // Current dimension: A (SI base, exponent 1 in CURRENT slot)
        let current = DimensionVector::CURRENT;
        let result = eval_builtin(
            "complex",
            &[
                Value::Scalar {
                    si_value: 3.0,
                    dimension: voltage,
                },
                Value::Scalar {
                    si_value: 4.0,
                    dimension: current,
                },
            ],
        );
        assert!(
            result.is_undef(),
            "expected Undef for V/A mismatch, got {:?}",
            result
        );
    }

    // ── Phase degree-equivalent spec test (step-9) ───────────────────────────

    #[test]
    fn phase_1_plus_i_approx_45_deg() {
        // phase(1+i) = atan2(1,1) = π/4 ≈ 0.7854 rad (45°)
        let z = Value::Complex {
            re: 1.0,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_scalar_approx!(
            eval_builtin("phase", &[z]),
            std::f64::consts::FRAC_PI_4, // π/4 ≈ 0.7854 rad ≈ 45°
            DimensionVector::ANGLE
        );
    }

    // ── sanitize_value Complex arm tests (step-20) ────────────────────────────

    #[test]
    fn complex_mul_overflow_returns_undef() {
        // (f64::MAX + f64::MAX*i) * (f64::MAX + f64::MAX*i)
        // re = MAX*MAX - MAX*MAX = 0 (actually NaN-ish), im = MAX*MAX + MAX*MAX = +Inf
        // Either component going Inf/NaN must produce Undef.
        let a = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("complex_mul", &[a, b]).is_undef(),
            "complex_mul with f64::MAX components must return Undef (Inf overflow)"
        );
    }

    #[test]
    fn complex_add_overflow_returns_undef() {
        // f64::MAX + f64::MAX = +Inf overflow
        let a = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let b = Value::Complex {
            re: f64::MAX,
            im: f64::MAX,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("complex_add", &[a, b]).is_undef(),
            "complex_add with f64::MAX components must return Undef (Inf overflow)"
        );
    }

    // ── sanitize_value Orientation arm tests (task-904) ──────────────────────

    #[test]
    fn sanitize_orientation_nan_returns_undef() {
        let v = Value::Orientation {
            w: f64::NAN,
            x: 0.0,
            y: 0.0,
            z: 1.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with NaN component should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_inf_returns_undef() {
        let v = Value::Orientation {
            w: 0.0,
            x: f64::INFINITY,
            y: 0.0,
            z: 0.0,
        };
        assert!(
            sanitize_value(v).is_undef(),
            "Orientation with Inf component should become Undef"
        );
    }

    #[test]
    fn sanitize_orientation_valid_passthrough() {
        let v = Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        assert_orientation_approx!(sanitize_value(v), 1.0, 0.0, 0.0, 0.0);
    }

    // ── re/real sanitize_value tests (task-358 step-1) ─────────────────────────

    #[test]
    fn re_nan_re_component_returns_undef() {
        // re(Complex{NaN, 1.0, DIMLESS}) → Undef (NaN must not propagate)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with NaN real component must return Undef"
        );
    }

    #[test]
    fn re_inf_re_component_returns_undef() {
        // re(Complex{+Inf, 1.0, DIMLESS}) → Undef (Inf must not propagate)
        let z = Value::Complex {
            re: f64::INFINITY,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with Inf real component must return Undef"
        );
    }

    #[test]
    fn re_nan_dimensioned_returns_undef() {
        // re(Complex{NaN, 1.0, LENGTH}) → Undef (dimensioned Scalar path)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            eval_builtin("re", &[z]).is_undef(),
            "re() with NaN dimensioned real component must return Undef"
        );
    }

    #[test]
    fn real_nan_re_component_returns_undef() {
        // real(Complex{NaN, 1.0, DIMLESS}) → Undef (alias coverage)
        let z = Value::Complex {
            re: f64::NAN,
            im: 1.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("real", &[z]).is_undef(),
            "real() with NaN real component must return Undef"
        );
    }

    // ── real() alias tests (step-1) ───────────────────────────────────────────

    #[test]
    fn real_dimensionless_returns_real() {
        // real(Complex{3,4,DIMLESS}) → Real(3.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("real", &[z]), 3.0);
    }

    #[test]
    fn real_dimensioned_returns_scalar() {
        // real(Complex{5,3,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("real", &[z]), 5.0, DimensionVector::LENGTH);
    }

    #[test]
    fn real_non_complex_returns_undef() {
        assert!(eval_builtin("real", &[Value::Real(3.0)]).is_undef());
    }

    // ── im/imag sanitize_value tests (task-358 step-3) ─────────────────────────

    #[test]
    fn im_nan_im_component_returns_undef() {
        // im(Complex{1.0, NaN, DIMLESS}) → Undef (NaN must not propagate)
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with NaN imaginary component must return Undef"
        );
    }

    #[test]
    fn im_inf_im_component_returns_undef() {
        // im(Complex{1.0, +Inf, DIMLESS}) → Undef (Inf must not propagate)
        let z = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with Inf imaginary component must return Undef"
        );
    }

    #[test]
    fn im_inf_dimensioned_returns_undef() {
        // im(Complex{1.0, +Inf, LENGTH}) → Undef (dimensioned Scalar path)
        let z = Value::Complex {
            re: 1.0,
            im: f64::INFINITY,
            dimension: DimensionVector::LENGTH,
        };
        assert!(
            eval_builtin("im", &[z]).is_undef(),
            "im() with Inf dimensioned imaginary component must return Undef"
        );
    }

    #[test]
    fn imag_nan_im_component_returns_undef() {
        // imag(Complex{1.0, NaN, DIMLESS}) → Undef (alias coverage)
        let z = Value::Complex {
            re: 1.0,
            im: f64::NAN,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert!(
            eval_builtin("imag", &[z]).is_undef(),
            "imag() with NaN imaginary component must return Undef"
        );
    }

    // ── imag() alias tests (step-3) ───────────────────────────────────────────

    #[test]
    fn imag_dimensionless_returns_real() {
        // imag(Complex{3,4,DIMLESS}) → Real(4.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("imag", &[z]), 4.0);
    }

    #[test]
    fn imag_dimensioned_returns_scalar() {
        // imag(Complex{5,3,LENGTH}) → Scalar{3.0, LENGTH}
        let z = Value::Complex {
            re: 5.0,
            im: 3.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(eval_builtin("imag", &[z]), 3.0, DimensionVector::LENGTH);
    }

    #[test]
    fn imag_non_complex_returns_undef() {
        assert!(eval_builtin("imag", &[Value::Real(3.0)]).is_undef());
    }

    // ── magnitude / complex_magnitude edge-case tests: overflow, NaN, dimensioned ──

    /// Assert that evaluating `builtin` with a single `Complex { re, im, dimension }` argument
    /// returns `Value::Undef`. Panics with a descriptive message including the builtin name.
    fn assert_complex_builtin_undef(builtin: &str, re: f64, im: f64, dimension: DimensionVector) {
        let z = Value::Complex { re, im, dimension };
        assert!(
            eval_builtin(builtin, &[z]).is_undef(),
            "{builtin} with Complex{{re={re}, im={im}, dimension={dimension:?}}} must return Undef"
        );
    }

    #[test]
    fn complex_overflow_returns_undef_both_builtins() {
        // Both `magnitude` and `complex_magnitude` delegate to complex_abs for Complex
        // inputs; f64::MAX² + f64::MAX² overflows to +Inf; sanitize_value must catch it.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::MAX,
                f64::MAX,
                DimensionVector::DIMENSIONLESS,
            );
        }
    }

    #[test]
    fn complex_overflow_dimensioned_returns_undef_both_builtins() {
        // Same overflow but through the Scalar branch (non-dimensionless).
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(builtin, f64::MAX, f64::MAX, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_nan_component_returns_undef_both_builtins() {
        // A NaN component propagates through re.hypot(im) and sanitize_value catches it.
        for builtin in ["magnitude", "complex_magnitude"] {
            // re=NaN
            assert_complex_builtin_undef(builtin, f64::NAN, 1.0, DimensionVector::DIMENSIONLESS);
            // im=NaN (symmetric case)
            assert_complex_builtin_undef(builtin, 1.0, f64::NAN, DimensionVector::DIMENSIONLESS);
        }
    }

    #[test]
    fn complex_nan_dimensioned_returns_undef_both_builtins() {
        // NaN component with non-dimensionless input exercises the Value::Scalar arm of
        // sanitize_value (rather than Value::Real). Ensures the Scalar path is covered.
        for builtin in ["magnitude", "complex_magnitude"] {
            // re=NaN, im=1.0, LENGTH dimension → hits Scalar arm
            assert_complex_builtin_undef(builtin, f64::NAN, 1.0, DimensionVector::LENGTH);
            // im=NaN, re=1.0, LENGTH dimension → symmetric case
            assert_complex_builtin_undef(builtin, 1.0, f64::NAN, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_both_nan_returns_undef_both_builtins() {
        // hypot(NaN, NaN) = NaN per IEEE 754; test both dimensionless and dimensioned paths.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::NAN,
                f64::NAN,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::NAN, f64::NAN, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_direct_infinity_returns_undef_both_builtins() {
        // Direct ±Infinity inputs (not computed overflow) are also caught by sanitize_value.
        // hypot(±Inf, x) = +Inf for any finite x per IEEE 754.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                0.0,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::INFINITY, 0.0, DimensionVector::LENGTH);
            assert_complex_builtin_undef(
                builtin,
                0.0,
                f64::NEG_INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, 0.0, f64::NEG_INFINITY, DimensionVector::LENGTH);
            // im=+Inf (symmetric of re=+Inf)
            assert_complex_builtin_undef(
                builtin,
                0.0,
                f64::INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, 0.0, f64::INFINITY, DimensionVector::LENGTH);
            // re=-Inf (symmetric of im=-Inf)
            assert_complex_builtin_undef(
                builtin,
                f64::NEG_INFINITY,
                0.0,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(builtin, f64::NEG_INFINITY, 0.0, DimensionVector::LENGTH);
        }
    }

    #[test]
    fn complex_both_infinite_returns_undef_both_builtins() {
        // hypot(Inf, Inf) = +Inf per IEEE 754; sanitize_value must catch it.
        for builtin in ["magnitude", "complex_magnitude"] {
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                f64::INFINITY,
                DimensionVector::DIMENSIONLESS,
            );
            assert_complex_builtin_undef(
                builtin,
                f64::INFINITY,
                f64::INFINITY,
                DimensionVector::LENGTH,
            );
        }
    }

    // ── complex_magnitude() tests ─────────────────────────────────────────────

    #[test]
    fn complex_magnitude_3_4_returns_5() {
        // complex_magnitude(Complex{3,4,DIMLESS}) → Real(5.0)
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("complex_magnitude", &[z]), 5.0);
    }

    #[test]
    fn complex_magnitude_dimensioned_returns_scalar() {
        // complex_magnitude(Complex{3,4,LENGTH}) → Scalar{5.0, LENGTH}
        let z = Value::Complex {
            re: 3.0,
            im: 4.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("complex_magnitude", &[z]),
            5.0,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn complex_magnitude_non_complex_returns_undef() {
        // unlike generic magnitude which handles Tensors, complex_magnitude rejects non-Complex
        assert!(eval_builtin("complex_magnitude", &[Value::Real(5.0)]).is_undef());
    }

    #[test]
    fn complex_magnitude_large_representable_no_overflow() {
        // 1e200 is representable as f64, so |1e200 + 0i| = 1e200 must NOT overflow.
        // The naive (re*re + im*im).sqrt() formula fails because 1e200² = Inf.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        assert_real_approx!(eval_builtin("complex_magnitude", &[z]), 1e200);
    }

    #[test]
    fn complex_magnitude_large_dimensioned_no_overflow() {
        // |1e200 + 0i| with LENGTH dimension must return Scalar{1e200, LENGTH}, not Undef.
        // Covers the dimensioned (Scalar) branch of complex_abs with large values.
        let z = Value::Complex {
            re: 1e200,
            im: 0.0,
            dimension: DimensionVector::LENGTH,
        };
        assert_scalar_approx!(
            eval_builtin("complex_magnitude", &[z]),
            1e200,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn complex_magnitude_large_both_components() {
        // |1e200 + 1e200i| = 1e200 * sqrt(2) ≈ 1.4142e200, fully representable.
        // The naive formula fails because 1e200² + 1e200² overflows.
        let z = Value::Complex {
            re: 1e200,
            im: 1e200,
            dimension: DimensionVector::DIMENSIONLESS,
        };
        let result = eval_builtin("complex_magnitude", &[z]);
        let expected = 1e200 * std::f64::consts::SQRT_2;
        match result {
            Value::Real(v) => {
                let rel_err = ((v - expected) / expected).abs();
                assert!(
                    rel_err < 1e-14,
                    "expected Real({expected}) got Real({v}), relative error {rel_err}"
                );
            }
            other => panic!("expected Real({expected}), got {other:?}"),
        }
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

    // ── tensor_components_f64 with Point/Vector inputs (task 398, step-13) ────

    #[test]
    fn magnitude_point_dimensioned_3m_4m_0m() {
        // magnitude(Point([3m,4m,0m])) ≈ Scalar{0.005, LENGTH}
        // 3mm=0.003m, 4mm=0.004m → |v|=0.005m
        let p = Value::Point(vec![
            Value::Scalar {
                si_value: 0.003,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.004,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        assert_scalar_approx!(
            eval_builtin("magnitude", &[p]),
            0.005,
            DimensionVector::LENGTH
        );
    }

    #[test]
    fn normalize_point_returns_point_wrapper() {
        // normalize(Point([3,4,0])) → Point([0.6,0.8,0.0])
        let p = Value::Point(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(0.0)]);
        let result = eval_builtin("normalize", &[p]);
        assert_vector3_approx!(Point, result, [0.6, 0.8, 0.0]);
    }

    #[test]
    fn dot_point_point_returns_scalar() {
        // dot(Point([1,2,3]), Point([4,5,6])) = 1*4 + 2*5 + 3*6 = 32
        let a = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Point(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 32.0);
    }

    #[test]
    fn cross_point_point_returns_undef() {
        // cross is semantically invalid for points — only defined for vectors
        let a = Value::Point(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Point(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross of two Points should return Undef"
        );
    }

    // ── mixed-type contract tests (task 379) ─────────────────────────────────

    #[test]
    fn cross_vector_tensor_returns_tensor_wrapper() {
        // cross(Vector, Tensor) falls through to Tensor wrapper (line 366: _ => Value::Tensor)
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        assert_vector3_approx!(Tensor, result, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn cross_tensor_vector_returns_tensor_wrapper() {
        // cross(Tensor, Vector) also falls through to Tensor wrapper
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        let result = eval_builtin("cross", &[a, b]);
        assert_vector3_approx!(Tensor, result, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn cross_point_vector_returns_undef() {
        // ANY Point input to cross returns Undef (line 364)
        let a = Value::Point(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross(Point, Vector) should return Undef"
        );
    }

    #[test]
    fn cross_vector_point_returns_undef() {
        // Second-arg Point also returns Undef
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Point(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert!(
            eval_builtin("cross", &[a, b]).is_undef(),
            "cross(Vector, Point) should return Undef"
        );
    }

    #[test]
    fn dot_point_vector_returns_scalar() {
        // dot accepts mixed Point+Vector inputs via tensor_components_f64
        let a = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Vector(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 32.0);
    }

    #[test]
    fn dot_vector_point_returns_scalar() {
        // Argument order symmetry for mixed dot
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        let b = Value::Point(vec![Value::Real(4.0), Value::Real(5.0), Value::Real(6.0)]);
        assert_real_approx!(eval_builtin("dot", &[a, b]), 32.0);
    }

    #[test]
    fn normalize_point_dimensioned_returns_point() {
        // normalize(Point([3m,4m,0m])) → Point([0.6, 0.8, 0.0]) with Real components
        let p = Value::Point(vec![
            Value::Scalar {
                si_value: 3.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: DimensionVector::LENGTH,
            },
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::LENGTH,
            },
        ]);
        let result = eval_builtin("normalize", &[p]);
        assert_vector3_approx!(Point, result, [0.6, 0.8, 0.0]);
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

    // ── Advanced linalg tests (task 337) ─────────────────────────────────────

    /// Helper: build a nested Tensor matrix from a slice of rows.
    fn make_matrix(rows: &[&[f64]]) -> Value {
        Value::Tensor(
            rows.iter()
                .map(|row| Value::Tensor(row.iter().map(|&v| Value::Real(v)).collect()))
                .collect(),
        )
    }

    /// Helper: build a Tensor matrix with all elements having a given dimension.
    fn make_dimensioned_matrix(rows: &[&[f64]], dim: DimensionVector) -> Value {
        Value::Tensor(
            rows.iter()
                .map(|row| {
                    Value::Tensor(
                        row.iter()
                            .map(|&v| Value::Scalar {
                                si_value: v,
                                dimension: dim,
                            })
                            .collect(),
                    )
                })
                .collect(),
        )
    }

    // --- determinant tests ---

    #[test]
    fn det_identity_2x2() {
        let m = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }

    #[test]
    fn det_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }

    #[test]
    fn det_2_times_identity_3x3() {
        // det(2*I₃) = 2³ = 8
        let m = make_matrix(&[&[2.0, 0.0, 0.0], &[0.0, 2.0, 0.0], &[0.0, 0.0, 2.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 8.0);
    }

    #[test]
    fn det_singular_matrix() {
        // Singular: rows are linearly dependent
        let m = make_matrix(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 0.0);
    }

    #[test]
    fn det_dimensioned_3x3() {
        // det(Force_mat) has dimension Force³ for 3×3
        let force_dim = reify_types::dimension::FORCE;
        let m = make_dimensioned_matrix(
            &[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]],
            force_dim,
        );
        let result = eval_builtin("determinant", &[m]);
        let expected_dim = force_dim.pow(3);
        assert_scalar_approx!(result, 1.0, expected_dim);
    }

    #[test]
    fn det_1x1() {
        let m = make_matrix(&[&[42.0]]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 42.0);
    }

    #[test]
    fn det_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("determinant", &[m]).is_undef());
    }

    // --- inverse tests ---

    #[test]
    fn inverse_2x2_identity() {
        let m = make_matrix(&[&[1.0, 0.0], &[0.0, 1.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&m));
        // inv(I) = I — check all four elements
        if let Value::Tensor(rows) = &inv {
            assert_eq!(rows.len(), 2);
            for (i, row) in rows.iter().enumerate() {
                if let Value::Tensor(elems) = row {
                    assert_eq!(elems.len(), 2);
                    for (j, elem) in elems.iter().enumerate() {
                        let expected = if i == j { 1.0 } else { 0.0 };
                        let val = elem.as_f64().unwrap();
                        assert!(
                            (val - expected).abs() < 1e-12,
                            "inv[{i}][{j}]: expected {expected}, got {val}"
                        );
                    }
                } else {
                    panic!("expected Tensor row");
                }
            }
        } else {
            panic!("expected Tensor, got {:?}", inv);
        }
    }

    #[test]
    fn inverse_times_original_approx_identity() {
        // A = [[1,2],[3,4]], verify inv(A)*A ≈ I via manual multiply
        let a = make_matrix(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&a));
        // Extract inv as flat
        let inv_data = matrix_components_f64(&inv).unwrap();
        let a_data = matrix_components_f64(&a).unwrap();
        // Manual 2×2 multiply: product = inv * a
        let (ai, ad) = (inv_data.2, a_data.2);
        let p00 = ai[0] * ad[0] + ai[1] * ad[2];
        let p01 = ai[0] * ad[1] + ai[1] * ad[3];
        let p10 = ai[2] * ad[0] + ai[3] * ad[2];
        let p11 = ai[2] * ad[1] + ai[3] * ad[3];
        assert!((p00 - 1.0).abs() < 1e-10, "p00={p00}");
        assert!((p01).abs() < 1e-10, "p01={p01}");
        assert!((p10).abs() < 1e-10, "p10={p10}");
        assert!((p11 - 1.0).abs() < 1e-10, "p11={p11}");
    }

    #[test]
    fn inverse_3x3() {
        let a = make_matrix(&[&[1.0, 2.0, 3.0], &[0.0, 1.0, 4.0], &[5.0, 6.0, 0.0]]);
        let inv = eval_builtin("inverse", std::slice::from_ref(&a));
        let inv_d = matrix_components_f64(&inv).unwrap();
        let a_d = matrix_components_f64(&a).unwrap();
        // 3×3 multiply to verify ≈ identity
        let (ai, ad) = (inv_d.2, a_d.2);
        for r in 0..3 {
            for c in 0..3 {
                let sum: f64 = (0..3).map(|k| ai[r * 3 + k] * ad[k * 3 + c]).sum();
                let expected = if r == c { 1.0 } else { 0.0 };
                assert!(
                    (sum - expected).abs() < 1e-10,
                    "product[{r}][{c}] = {sum}, expected {expected}"
                );
            }
        }
    }

    #[test]
    fn inverse_singular_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert!(
            eval_builtin("inverse", &[m]).is_undef(),
            "inverse of singular matrix should be Undef"
        );
    }

    // --- transpose tests ---

    #[test]
    fn transpose_symmetric_unchanged() {
        // Symmetric matrix: transpose should equal original
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[2.0, 5.0, 6.0], &[3.0, 6.0, 9.0]]);
        let t = eval_builtin("transpose", std::slice::from_ref(&m));
        let orig_d = matrix_components_f64(&m).unwrap();
        let t_d = matrix_components_f64(&t).unwrap();
        assert_eq!(orig_d.0, t_d.0);
        assert_eq!(orig_d.1, t_d.1);
        for (a, b) in orig_d.2.iter().zip(t_d.2.iter()) {
            assert!((a - b).abs() < 1e-12);
        }
    }

    #[test]
    fn transpose_2x3() {
        // [[1,2,3],[4,5,6]] → [[1,4],[2,5],[3,6]]
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        let t = eval_builtin("transpose", &[m]);
        let t_d = matrix_components_f64(&t).unwrap();
        assert_eq!(t_d.0, 3); // rows
        assert_eq!(t_d.1, 2); // cols
        assert!((t_d.2[0] - 1.0).abs() < 1e-12);
        assert!((t_d.2[1] - 4.0).abs() < 1e-12);
        assert!((t_d.2[2] - 2.0).abs() < 1e-12);
        assert!((t_d.2[3] - 5.0).abs() < 1e-12);
        assert!((t_d.2[4] - 3.0).abs() < 1e-12);
        assert!((t_d.2[5] - 6.0).abs() < 1e-12);
    }

    // --- trace tests ---

    #[test]
    fn trace_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        assert_real_approx!(eval_builtin("trace", &[m]), 3.0);
    }

    #[test]
    fn trace_general_2x2() {
        let m = make_matrix(&[&[5.0, 3.0], &[7.0, 2.0]]);
        assert_real_approx!(eval_builtin("trace", &[m]), 7.0);
    }

    #[test]
    fn trace_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("trace", &[m]).is_undef());
    }

    // --- outer product tests ---

    #[test]
    fn outer_two_vectors() {
        let a = Value::Tensor(vec![Value::Real(1.0), Value::Real(2.0)]);
        let b = Value::Tensor(vec![Value::Real(3.0), Value::Real(4.0), Value::Real(5.0)]);
        let result = eval_builtin("outer", &[a, b]);
        let d = matrix_components_f64(&result).unwrap();
        assert_eq!(d.0, 2);
        assert_eq!(d.1, 3);
        // [[3,4,5],[6,8,10]]
        let expected = [3.0, 4.0, 5.0, 6.0, 8.0, 10.0];
        for (got, exp) in d.2.iter().zip(expected.iter()) {
            assert!((got - exp).abs() < 1e-12);
        }
    }

    #[test]
    fn outer_dimensioned_vectors() {
        let length_dim = DimensionVector::LENGTH;
        let force_dim = reify_types::dimension::FORCE;
        let a = Value::Tensor(vec![
            Value::Scalar {
                si_value: 1.0,
                dimension: length_dim,
            },
            Value::Scalar {
                si_value: 2.0,
                dimension: length_dim,
            },
        ]);
        let b = Value::Tensor(vec![
            Value::Scalar {
                si_value: 3.0,
                dimension: force_dim,
            },
            Value::Scalar {
                si_value: 4.0,
                dimension: force_dim,
            },
        ]);
        let result = eval_builtin("outer", &[a, b]);
        let d = matrix_components_f64(&result).unwrap();
        assert_eq!(d.3, length_dim.mul(&force_dim));
    }

    // --- eigenvalues tests ---

    #[test]
    fn eigenvalues_diagonal_2x2() {
        let m = make_matrix(&[&[3.0, 0.0], &[0.0, 7.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 2);
            // Sorted: [3, 7]
            assert!((items[0].as_f64().unwrap() - 3.0).abs() < 1e-10);
            assert!((items[1].as_f64().unwrap() - 7.0).abs() < 1e-10);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_diagonal_3x3() {
        let m = make_matrix(&[&[2.0, 0.0, 0.0], &[0.0, 5.0, 0.0], &[0.0, 0.0, 8.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            // Sorted: [2, 5, 8]
            assert!((items[0].as_f64().unwrap() - 2.0).abs() < 1e-10);
            assert!((items[1].as_f64().unwrap() - 5.0).abs() < 1e-10);
            assert!((items[2].as_f64().unwrap() - 8.0).abs() < 1e-10);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_symmetric_3x3() {
        // Symmetric matrix always has real eigenvalues
        let m = make_matrix(&[&[2.0, 1.0, 0.0], &[1.0, 3.0, 1.0], &[0.0, 1.0, 2.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            // Eigenvalues of this matrix: 1, 2, 4
            let eigs: Vec<f64> = items.iter().map(|v| v.as_f64().unwrap()).collect();
            assert!((eigs[0] - 1.0).abs() < 1e-10, "eig0={}", eigs[0]);
            assert!((eigs[1] - 2.0).abs() < 1e-10, "eig1={}", eigs[1]);
            assert!((eigs[2] - 4.0).abs() < 1e-10, "eig2={}", eigs[2]);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_1x1() {
        let m = make_matrix(&[&[42.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 1);
            assert!((items[0].as_f64().unwrap() - 42.0).abs() < 1e-12);
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn eigenvalues_identity_3x3() {
        let m = make_matrix(&[&[1.0, 0.0, 0.0], &[0.0, 1.0, 0.0], &[0.0, 0.0, 1.0]]);
        let result = eval_builtin("eigenvalues", &[m]);
        if let Value::List(items) = result {
            assert_eq!(items.len(), 3);
            for item in &items {
                assert!((item.as_f64().unwrap() - 1.0).abs() < 1e-10);
            }
        } else {
            panic!("expected List, got {:?}", result);
        }
    }

    #[test]
    fn inverse_non_square_returns_undef() {
        let m = make_matrix(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]]);
        assert!(eval_builtin("inverse", &[m]).is_undef());
    }

    #[test]
    fn determinant_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("determinant", &[]).is_undef());
    }

    #[test]
    fn inverse_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("inverse", &[]).is_undef());
    }

    #[test]
    fn transpose_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("transpose", &[]).is_undef());
    }

    #[test]
    fn trace_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("trace", &[]).is_undef());
    }

    #[test]
    fn eigenvalues_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("eigenvalues", &[]).is_undef());
    }

    #[test]
    fn outer_wrong_arg_count_returns_undef() {
        assert!(eval_builtin("outer", &[]).is_undef());
    }

    #[test]
    fn determinant_non_matrix_returns_undef() {
        assert!(eval_builtin("determinant", &[Value::Real(5.0)]).is_undef());
    }

    #[test]
    fn inverse_dimensioned_2x2() {
        // inverse of dimensioned matrix has inverse dimension
        let length_dim = DimensionVector::LENGTH;
        let m = make_dimensioned_matrix(&[&[1.0, 0.0], &[0.0, 2.0]], length_dim);
        let inv = eval_builtin("inverse", &[m]);
        let d = matrix_components_f64(&inv).unwrap();
        let expected_dim = DimensionVector::DIMENSIONLESS.div(&length_dim);
        assert_eq!(d.3, expected_dim);
        // Check values: inv of diag(1,2) = diag(1, 0.5)
        assert!((d.2[0] - 1.0).abs() < 1e-12);
        assert!((d.2[1]).abs() < 1e-12);
        assert!((d.2[2]).abs() < 1e-12);
        assert!((d.2[3] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn matrix_value_form_works() {
        // Test that Value::Matrix is also accepted
        let m = Value::Matrix(vec![
            vec![Value::Real(1.0), Value::Real(0.0)],
            vec![Value::Real(0.0), Value::Real(1.0)],
        ]);
        assert_real_approx!(eval_builtin("determinant", &[m]), 1.0);
    }
}
