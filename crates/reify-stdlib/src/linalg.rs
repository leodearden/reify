use reify_types::{DimensionVector, Value};

pub(crate) fn dispatch(_name: &str, _args: &[Value]) -> Option<Value> {
    None
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    fn make_identity_matrix_2x2() -> Value {
        // Build a 2x2 identity matrix as Value::Tensor([[1.0, 0.0], [0.0, 1.0]])
        Value::Tensor(vec![
            Value::Tensor(vec![Value::Real(1.0), Value::Real(0.0)]),
            Value::Tensor(vec![Value::Real(0.0), Value::Real(1.0)]),
        ])
    }

    #[test]
    fn linalg_dispatch_determinant_identity() {
        let mat = make_identity_matrix_2x2();
        let result = dispatch("determinant", &[mat]);
        assert!(result.is_some(), "determinant should be handled by linalg dispatch");
        assert!(
            matches!(result, Some(Value::Real(v)) if (v - 1.0).abs() < 1e-12),
            "determinant of identity matrix should be 1.0"
        );
    }

    #[test]
    fn linalg_dispatch_unknown_returns_none() {
        assert!(dispatch("nope", &[]).is_none());
    }
}
