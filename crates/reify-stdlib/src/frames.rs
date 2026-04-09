use reify_types::Value;
use crate::common::*;

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let v = match name {
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
            // First arg must be a Point with exactly 3 components
            match origin {
                Value::Point(components) if components.len() == 3 => {}
                _ => return Some(Value::Undef),
            }
            // Second arg must be an Orientation
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
            // First arg must be an Orientation
            if !matches!(rotation, Value::Orientation { .. }) {
                return Some(Value::Undef);
            }
            // Second arg must be a Vector with exactly 3 components
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
            // Both args must be Frames
            let (origin_from, basis_from) = match &args[0] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Some(Value::Undef),
            };
            let (origin_to, basis_to) = match &args[1] {
                Value::Frame { origin, basis } => (origin.as_ref(), basis.as_ref()),
                _ => return Some(Value::Undef),
            };
            // Extract quaternions
            let q_from = match basis_from {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            let q_to = match basis_to {
                Value::Orientation { w, x, y, z } => (*w, *x, *y, *z),
                _ => return Some(Value::Undef),
            };
            // Extract origin points as f64 triples with finiteness and dimension validation
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
            // Normalize the result quaternion
            match normalize_quaternion(r.0, r.1, r.2, r.3) {
                Some(rot_val) => {
                    // t = origin_to - R * origin_from
                    if f_dim != t_dim {
                        return Some(Value::Undef);
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

        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn frames_dispatch_transform3_identity() {
        let result = dispatch("transform3_identity", &[]);
        assert!(matches!(result, Some(Value::Transform { .. })));
    }

    #[test]
    fn frames_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
