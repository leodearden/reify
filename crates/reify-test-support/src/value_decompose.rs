use reify_types::Value;

/// Read a numeric component (Real, Scalar, or Int) as f64 SI value.
pub fn read_f64(v: &Value, label: &str) -> f64 {
    match v {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        Value::Int(i) => *i as f64,
        other => panic!("{label}: expected numeric component, got {other:?}"),
    }
}

/// Decompose a `Value::Point` of three numeric components into `[f64; 3]` (SI).
pub fn decompose_point3(v: &Value, label: &str) -> [f64; 3] {
    let comps = match v {
        Value::Point(c) if c.len() == 3 => c,
        other => panic!("{label}: expected Value::Point len=3, got {other:?}"),
    };
    [
        read_f64(&comps[0], &format!("{label}.p[0]")),
        read_f64(&comps[1], &format!("{label}.p[1]")),
        read_f64(&comps[2], &format!("{label}.p[2]")),
    ]
}

#[cfg(test)]
mod tests {
    use reify_types::{Value, dimension::DimensionVector};

    use super::{decompose_point3, read_f64};

    // ── read_f64 tests ────────────────────────────────────────────────────────

    #[test]
    fn read_f64_real_returns_inner_f64() {
        assert_eq!(read_f64(&Value::Real(2.5), "x"), 2.5);
    }

    #[test]
    fn read_f64_scalar_returns_si_value() {
        let v = Value::Scalar {
            si_value: 3.14,
            dimension: DimensionVector::LENGTH,
        };
        assert_eq!(read_f64(&v, "y"), 3.14);
    }

    #[test]
    fn read_f64_int_returns_cast_f64() {
        assert_eq!(read_f64(&Value::Int(7), "z"), 7.0);
    }

    #[test]
    #[should_panic(expected = "lbl: expected numeric component")]
    fn read_f64_panics_on_non_numeric_with_label() {
        read_f64(&Value::Bool(true), "lbl");
    }

    // ── decompose_point3 tests ────────────────────────────────────────────────

    #[test]
    fn decompose_point3_three_reals_returns_array() {
        let v = Value::Point(vec![Value::Real(1.0), Value::Real(2.0), Value::Real(3.0)]);
        assert_eq!(decompose_point3(&v, "pt"), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn decompose_point3_three_length_scalars_returns_si_array() {
        let v = Value::Point(vec![
            Value::Scalar { si_value: 0.001, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.002, dimension: DimensionVector::LENGTH },
            Value::Scalar { si_value: 0.003, dimension: DimensionVector::LENGTH },
        ]);
        assert_eq!(decompose_point3(&v, "pt"), [0.001, 0.002, 0.003]);
    }

    #[test]
    #[should_panic(expected = "label: expected Value::Point len=3")]
    fn decompose_point3_panics_on_non_point() {
        let v = Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)]);
        decompose_point3(&v, "label");
    }

    #[test]
    #[should_panic(expected = "label: expected Value::Point len=3")]
    fn decompose_point3_panics_on_wrong_length() {
        let v = Value::Point(vec![Value::Real(0.0), Value::Real(0.0)]);
        decompose_point3(&v, "label");
    }
}
