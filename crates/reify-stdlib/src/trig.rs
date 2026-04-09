use reify_types::{DimensionVector, Value};
use crate::common::*;

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let _ = (name, args);
    None
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn trig_dispatch_sin_zero() {
        let angle = Value::Scalar {
            si_value: 0.0,
            dimension: DimensionVector::ANGLE,
        };
        assert_eq!(dispatch("sin", &[angle]), Some(Value::Real(0.0)));
    }

    #[test]
    fn trig_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
