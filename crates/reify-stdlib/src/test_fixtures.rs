use reify_types::Value;

pub(crate) fn axis_x_unit() -> Value {
    unimplemented!("step-2")
}

pub(crate) fn axis_y_unit() -> Value {
    unimplemented!("step-2")
}

pub(crate) fn axis_z_unit() -> Value {
    unimplemented!("step-2")
}

pub(crate) fn length_range_0_to_1m() -> Value {
    unimplemented!("step-2")
}

pub(crate) fn angle_range_0_to_pi() -> Value {
    unimplemented!("step-2")
}

pub(crate) fn identity_transform_value() -> Value {
    unimplemented!("step-2")
}

pub(crate) fn planar_xy_joint() -> Value {
    unimplemented!("step-2")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_x_unit_has_canonical_shape() {
        assert_eq!(
            axis_x_unit(),
            Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
        );
    }

    #[test]
    fn axis_y_unit_has_canonical_shape() {
        assert_eq!(
            axis_y_unit(),
            Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
        );
    }

    #[test]
    fn axis_z_unit_has_canonical_shape() {
        assert_eq!(
            axis_z_unit(),
            Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
        );
    }

    #[test]
    fn length_range_0_to_1m_has_canonical_shape() {
        assert_eq!(
            length_range_0_to_1m(),
            Value::Range {
                lower: Some(Box::new(Value::length(0.0))),
                upper: Some(Box::new(Value::length(1.0))),
                lower_inclusive: true,
                upper_inclusive: true,
            }
        );
    }

    #[test]
    fn angle_range_0_to_pi_has_canonical_shape() {
        assert_eq!(
            angle_range_0_to_pi(),
            Value::Range {
                lower: Some(Box::new(Value::angle(0.0))),
                upper: Some(Box::new(Value::angle(std::f64::consts::PI))),
                lower_inclusive: true,
                upper_inclusive: true,
            }
        );
    }

    #[test]
    fn identity_transform_value_has_canonical_shape() {
        assert_eq!(
            identity_transform_value(),
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
        );
    }

    #[test]
    fn planar_xy_joint_has_canonical_kind() {
        let joint = planar_xy_joint();
        match joint {
            Value::Map(ref m) => {
                assert_eq!(
                    m.get(&Value::String("kind".to_string())),
                    Some(&Value::String("planar".to_string())),
                    "planar_xy_joint should have kind='planar'"
                );
            }
            other => panic!("expected Value::Map, got {:?}", other),
        }
    }
}
