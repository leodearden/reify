use reify_types::Value;
use crate::common::*;

pub(crate) fn dispatch(name: &str, args: &[Value]) -> Option<Value> {
    let _ = (name, args);
    None
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn vector_dispatch_dot_orthogonal() {
        let a = Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]);
        let b = Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)]);
        assert_eq!(dispatch("dot", &[a, b]), Some(Value::Real(0.0)));
    }

    #[test]
    fn vector_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
