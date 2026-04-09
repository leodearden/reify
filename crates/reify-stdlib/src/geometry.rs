use reify_types::{DimensionVector, Value};
use crate::common::tensor_components_f64;

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let v = match name {
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
            // Both args must be Point with exactly 3 components and matching dimensions
            let min_comps = match min {
                Value::Point(comps) if comps.len() == 3 => comps,
                _ => return Some(Value::Undef),
            };
            let max_comps = match max {
                Value::Point(comps) if comps.len() == 3 => comps,
                _ => return Some(Value::Undef),
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
                _ => return Some(Value::Undef),
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
                _ => return Some(Value::Undef),
            }
        }

        _ => return None,
    };
    Some(v)
}

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

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn geometry_dispatch_plane_xy() {
        let result = dispatch("plane_xy", &[Value::length(1.0)]);
        assert!(result.is_some(), "plane_xy should be handled by geometry dispatch");
        assert!(
            matches!(result, Some(Value::Plane { .. })),
            "plane_xy should return a Plane value"
        );
    }

    #[test]
    fn geometry_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
