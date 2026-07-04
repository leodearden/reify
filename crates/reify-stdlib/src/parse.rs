//! Fallible string→quantity parse ops (task #4535): `parse_length` /
//! `parse_length_r`. Pure `String -> Value` builtins (no `EvalContext`), so
//! they live in their own `eval_builtin` sub-dispatcher rather than in
//! `reify-expr`'s context-needing intercepts.

#[cfg(test)]
mod tests {
    use reify_core::DimensionVector;
    use reify_ir::Value;

    use super::*;

    /// Assert `result` is `Some(Value::Option(Some(Scalar)))` with the given
    /// SI value (1e-9 tolerance — `Value`'s `PartialEq` is bit-exact, and
    /// `12mm` parsed as `12.0 * 0.001` is not guaranteed bit-identical to a
    /// hand-written `0.012` literal) and `DimensionVector::LENGTH`.
    fn assert_parses_to_length(result: Option<Value>, expected_si: f64) {
        match result {
            Some(Value::Option(Some(boxed))) => match *boxed {
                Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    assert!(
                        (si_value - expected_si).abs() < 1e-9,
                        "si_value {si_value} not within 1e-9 of expected {expected_si}"
                    );
                    assert_eq!(dimension, DimensionVector::LENGTH);
                }
                other => panic!("expected Value::Scalar inside Some(..), got {other:?}"),
            },
            other => panic!("expected Some(Value::Option(Some(Scalar))), got {other:?}"),
        }
    }

    fn assert_parses_to_none(result: Option<Value>) {
        assert!(
            matches!(result, Some(Value::Option(None))),
            "expected Some(Value::Option(None)), got {result:?}"
        );
    }

    #[test]
    fn parse_length_recognizes_millimetres() {
        let result = eval_parse("parse_length", &[Value::String("12mm".to_string())]);
        assert_parses_to_length(result, 0.012);
    }

    #[test]
    fn parse_length_recognizes_metres_with_internal_space() {
        let result = eval_parse("parse_length", &[Value::String("3.5 m".to_string())]);
        assert_parses_to_length(result, 3.5);
    }

    #[test]
    fn parse_length_recognizes_centimetres() {
        let result = eval_parse("parse_length", &[Value::String("2.5cm".to_string())]);
        assert_parses_to_length(result, 0.025);
    }

    #[test]
    fn parse_length_trims_surrounding_whitespace_for_inches() {
        let result = eval_parse("parse_length", &[Value::String(" 1in ".to_string())]);
        assert_parses_to_length(result, 0.0254);
    }

    #[test]
    fn parse_length_returns_inner_none_for_malformed_input() {
        let result = eval_parse("parse_length", &[Value::String("bogus".to_string())]);
        assert_parses_to_none(result);
    }

    #[test]
    fn parse_length_returns_inner_none_for_unknown_unit() {
        let result = eval_parse("parse_length", &[Value::String("5xyz".to_string())]);
        assert_parses_to_none(result);
    }

    #[test]
    fn parse_length_returns_inner_none_for_recognized_non_length_unit() {
        // "kg" is a recognized built-in unit, but its dimension is Mass, not
        // Length — a units-mismatch, distinct from a malformed/unknown unit.
        let result = eval_parse("parse_length", &[Value::String("12kg".to_string())]);
        assert_parses_to_none(result);
    }

    #[test]
    fn eval_parse_does_not_handle_a_non_string_arg() {
        // Wrong arg shape ⇒ this sub-dispatcher declines (returns `None`, not
        // `Some(Value::Undef)`), leaving `eval_builtin`'s chain to fall
        // through to its own `Value::Undef` default.
        let result = eval_parse("parse_length", &[Value::Real(12.0)]);
        assert_eq!(result, None);
    }
}
