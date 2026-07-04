//! Fallible string→quantity parse ops (task #4535): `parse_length` /
//! `parse_length_r`. Pure `String -> Value` builtins (no `EvalContext`), so
//! they live in their own `eval_builtin` sub-dispatcher rather than in
//! `reify-expr`'s context-needing intercepts.

use reify_core::DimensionVector;
use reify_ir::Value;

/// Evaluate a `parse_*` builtin. Returns `None` when `name` is not one of
/// this sub-dispatcher's names, or when `args` doesn't match the expected
/// shape (a single `Value::String`) — the "this sub-dispatcher declines"
/// signal that lets `eval_builtin`'s dispatch chain fall through to its own
/// `Value::Undef` default, mirroring the other `eval_*` sub-dispatchers'
/// `Option<Value>` contract.
pub(crate) fn eval_parse(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "parse_length" => {
            let s = single_string_arg(args)?;
            Some(Value::Option(parse_length_str(s).map(Box::new)))
        }
        _ => None,
    }
}

/// Extract the sole `&str` from a one-`Value::String`-element arg slice, or
/// `None` for any other arity/shape.
fn single_string_arg(args: &[Value]) -> Option<&str> {
    match args {
        [Value::String(s)] => Some(s.as_str()),
        _ => None,
    }
}

/// Parse a `"<number><unit>"` quantity-literal string (e.g. `"12mm"`,
/// `"3.5 m"`) into a `Value::Scalar`.
///
/// Trims surrounding whitespace, then splits at the first ASCII-alphabetic
/// character — the numeric prefix (with any single internal space before the
/// unit trimmed off) is parsed as `f64`, and the remaining unit suffix is
/// looked up in the shared built-in unit table
/// (`reify_core::units::unit_symbol_to_si`), the same table
/// `reify-compiler::units::unit_to_scalar` delegates to.
///
/// Returns `None` for a missing/malformed numeric prefix, an unrecognized
/// unit, OR a recognized unit whose dimension is not `LENGTH` (e.g.
/// `"12kg"` — a units-mismatch, distinct from malformed input, but
/// collapsed to the same `None` here; `parse_length_r` distinguishes the two
/// via `parse_length_error_reason`).
fn parse_length_str(s: &str) -> Option<Value> {
    let trimmed = s.trim();
    let split_idx = trimmed.find(|c: char| c.is_ascii_alphabetic())?;
    let (num_part, unit_part) = trimmed.split_at(split_idx);
    let num_part = num_part.trim_end();
    if num_part.is_empty() {
        return None;
    }
    let num: f64 = num_part.parse().ok()?;
    let (factor, dimension) = reify_core::units::unit_symbol_to_si(unit_part)?;
    if dimension != DimensionVector::LENGTH {
        return None;
    }
    Some(Value::Scalar {
        si_value: num * factor,
        dimension,
    })
}

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

    // ── parse_length_r (step-5 RED / step-6 GREEN) ───────────────────────────
    //
    // Reuses the landed-#4035 PRELUDE `Result<T,E>` shape verbatim:
    // `Value::Enum{type_name:"Result", variant:"Ok"/"Err", payload:[("value"/
    // "error", _)]}` (see result_prelude_enum_tests.rs). Pinning the exact
    // "value"/"error" field names here means a divergent payload shape fails.

    /// Assert `result` is `Some(Value::Enum{type_name:"Result",
    /// variant:"Ok", payload:[("value", Scalar)]})` with the given SI value
    /// (1e-9 tolerance, same rationale as `assert_parses_to_length`) and
    /// `DimensionVector::LENGTH`.
    fn assert_ok_length(result: Option<Value>, expected_si: f64) {
        match result {
            Some(Value::Enum {
                type_name,
                variant,
                payload,
            }) => {
                assert_eq!(type_name, "Result", "enum type_name");
                assert_eq!(variant, "Ok", "constructed variant");
                assert_eq!(
                    payload.len(),
                    1,
                    "Ok payload should carry exactly the 'value' field, got {payload:?}"
                );
                assert_eq!(payload[0].0, "value", "payload field name");
                match &payload[0].1 {
                    Value::Scalar {
                        si_value,
                        dimension,
                    } => {
                        assert!(
                            (si_value - expected_si).abs() < 1e-9,
                            "si_value {si_value} not within 1e-9 of expected {expected_si}"
                        );
                        assert_eq!(*dimension, DimensionVector::LENGTH);
                    }
                    other => panic!(
                        "expected Value::Scalar for the 'value' payload field, got {other:?}"
                    ),
                }
            }
            other => panic!("expected Some(Value::Enum{{Result::Ok}}), got {other:?}"),
        }
    }

    /// Assert `result` is `Some(Value::Enum{type_name:"Result",
    /// variant:"Err", payload:[("error", Value::String(_))]})`. Only the
    /// shape and field name are pinned here — the exact reason text is an
    /// implementation detail of `parse_length_error_reason`.
    fn assert_err_reason(result: Option<Value>) {
        match result {
            Some(Value::Enum {
                type_name,
                variant,
                payload,
            }) => {
                assert_eq!(type_name, "Result", "enum type_name");
                assert_eq!(variant, "Err", "constructed variant");
                assert_eq!(
                    payload.len(),
                    1,
                    "Err payload should carry exactly the 'error' field, got {payload:?}"
                );
                assert_eq!(payload[0].0, "error", "payload field name");
                assert!(
                    matches!(payload[0].1, Value::String(_)),
                    "expected Value::String for the 'error' payload field, got {:?}",
                    payload[0].1
                );
            }
            other => panic!("expected Some(Value::Enum{{Result::Err}}), got {other:?}"),
        }
    }

    #[test]
    fn parse_length_r_recognizes_millimetres_as_ok() {
        let result = eval_parse("parse_length_r", &[Value::String("12mm".to_string())]);
        assert_ok_length(result, 0.012);
    }

    #[test]
    fn parse_length_r_returns_err_for_malformed_input() {
        let result = eval_parse("parse_length_r", &[Value::String("bogus".to_string())]);
        assert_err_reason(result);
    }

    #[test]
    fn parse_length_r_returns_err_for_recognized_non_length_unit() {
        // "kg" is a recognized built-in unit, but its dimension is Mass —
        // a units-mismatch, distinct from malformed/unknown-unit input.
        let result = eval_parse("parse_length_r", &[Value::String("12kg".to_string())]);
        assert_err_reason(result);
    }
}
