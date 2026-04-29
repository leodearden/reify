use reify_types::Value;
use crate::eval_builtin;

pub(crate) fn axis_x_unit() -> Value {
    Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
}

pub(crate) fn axis_y_unit() -> Value {
    Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
}

pub(crate) fn axis_z_unit() -> Value {
    Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
}

pub(crate) fn length_range_0_to_1m() -> Value {
    Value::Range {
        lower: Some(Box::new(Value::length(0.0))),
        upper: Some(Box::new(Value::length(1.0))),
        lower_inclusive: true,
        upper_inclusive: true,
    }
}

pub(crate) fn angle_range_0_to_pi() -> Value {
    Value::Range {
        lower: Some(Box::new(Value::angle(0.0))),
        upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
        lower_inclusive: true,
        upper_inclusive: true,
    }
}

pub(crate) fn identity_transform_value() -> Value {
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
}

pub(crate) fn planar_xy_joint() -> Value {
    eval_builtin("planar", &[
        axis_x_unit(),
        axis_y_unit(),
        length_range_0_to_1m(),
        length_range_0_to_1m(),
        angle_range_0_to_pi(),
    ])
}
