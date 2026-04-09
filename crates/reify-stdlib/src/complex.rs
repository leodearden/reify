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
    fn complex_dispatch_constructor() {
        let result = dispatch("complex", &[Value::Real(3.0), Value::Real(4.0)]);
        assert_eq!(
            result,
            Some(Value::Complex {
                re: 3.0,
                im: 4.0,
                dimension: DimensionVector::DIMENSIONLESS,
            })
        );
    }

    #[test]
    fn complex_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
