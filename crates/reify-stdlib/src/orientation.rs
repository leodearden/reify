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
