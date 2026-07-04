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
            Some(Value::Option(parse_length_value(s).ok().map(Box::new)))
        }
        "parse_length_r" => {
            let s = single_string_arg(args)?;
            Some(match parse_length_value(s) {
                Ok(value) => Value::Enum {
                    type_name: "Result".to_string(),
                    variant: "Ok".to_string(),
                    payload: vec![("value".to_string(), value)],
                },
                Err(err) => Value::Enum {
                    type_name: "Result".to_string(),
                    variant: "Err".to_string(),
                    payload: vec![("error".to_string(), Value::String(err.reason(s)))],
                },
            })
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

/// Why [`parse_length_value`] failed to parse its input as a length
/// quantity. Distinguishing the two causes lets `parse_length_r` build a
/// specific diagnostic reason from a SINGLE parse pass (amendment: reviewer
/// suggestion #1, task #4535) — `parse_length` (which only needs an
/// `Option`) discards the distinction via `.ok()`.
#[derive(Debug, Clone, PartialEq)]
enum ParseLengthError {
    /// No numeric prefix, an unparsable number, or a unit not in the shared
    /// built-in table (`reify_core::units::unit_symbol_to_si`) — including
    /// the scientific-notation gap documented on [`parse_length_value`].
    Malformed,
    /// The unit WAS recognized by `unit_symbol_to_si`, but its dimension is
    /// not `LENGTH` (e.g. `"12kg"` — `kg` is a recognized Mass unit).
    WrongDimension {
        unit: String,
        dimension: DimensionVector,
    },
}

impl ParseLengthError {
    /// Diagnostic-friendly reason string for `parse_length_r`'s `Err`
    /// payload. `input` is the original string as passed to
    /// `parse_length_r`; the `Malformed` message reports its TRIMMED form
    /// (matching what `parse_length_value` actually attempted to parse) —
    /// not the raw untrimmed string (reviewer suggestion #2, task #4535
    /// amendment round 2).
    fn reason(&self, input: &str) -> String {
        match self {
            ParseLengthError::Malformed => {
                format!("could not parse '{}' as a length", input.trim())
            }
            ParseLengthError::WrongDimension { unit, dimension } => {
                let dim_name = dimension.canonical_name().unwrap_or("non-length");
                format!("'{unit}' is a {dim_name} unit, expected a length")
            }
        }
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
/// Returns [`ParseLengthError::Malformed`] for a missing/malformed numeric
/// prefix or an unrecognized unit, and [`ParseLengthError::WrongDimension`]
/// for a recognized unit whose dimension is not `LENGTH` (e.g. `"12kg"` — a
/// units-mismatch, distinct from malformed input). `parse_length` collapses
/// both variants to `None` via `.ok()`; `parse_length_r` reports the
/// distinction via [`ParseLengthError::reason`].
///
/// **Limitation**: scientific notation is NOT supported. Splitting at the
/// *first* ASCII-alphabetic character treats an exponent marker (`e`/`E`) as
/// the start of the unit suffix rather than part of the number — e.g.
/// `"1e3mm"` splits into `num_part="1"`, `unit_part="e3mm"` (unrecognized ⇒
/// `Malformed`), and a signed exponent like `"1.0e-2m"` fails the same way.
/// Neither parses to its scientific value. Supporting it would require
/// splitting on the unit-symbol boundary instead of the first alphabetic
/// character (reviewer suggestion #2, task #4535 amendment).
fn parse_length_value(s: &str) -> Result<Value, ParseLengthError> {
    let trimmed = s.trim();
    let split_idx = trimmed
        .find(|c: char| c.is_ascii_alphabetic())
        .ok_or(ParseLengthError::Malformed)?;
    let (num_part, unit_part) = trimmed.split_at(split_idx);
    let num_part = num_part.trim_end();
    if num_part.is_empty() {
        return Err(ParseLengthError::Malformed);
    }
    let num: f64 = num_part.parse().map_err(|_| ParseLengthError::Malformed)?;
    let (factor, dimension) =
        reify_core::units::unit_symbol_to_si(unit_part).ok_or(ParseLengthError::Malformed)?;
    if dimension != DimensionVector::LENGTH {
        return Err(ParseLengthError::WrongDimension {
            unit: unit_part.to_string(),
            dimension,
        });
    }
    Ok(Value::Scalar {
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

    // ── Numeric-prefix edge cases (reviewer suggestion #4, amendment round 2) ──
    //
    // `parse_length_value` splits at the first ASCII-alphabetic char, so a
    // leading sign or leading-dot numeric prefix is handled by `f64`'s own
    // `FromStr` rather than any bespoke logic here. Locking these pins the
    // behavior deliberately rather than leaving it an untested side effect.

    #[test]
    fn parse_length_recognizes_negative_sign_prefixed_millimetres() {
        let result = eval_parse("parse_length", &[Value::String("-5mm".to_string())]);
        assert_parses_to_length(result, -0.005);
    }

    #[test]
    fn parse_length_recognizes_positive_sign_prefixed_millimetres() {
        let result = eval_parse("parse_length", &[Value::String("+5mm".to_string())]);
        assert_parses_to_length(result, 0.005);
    }

    #[test]
    fn parse_length_recognizes_leading_dot_metres() {
        let result = eval_parse("parse_length", &[Value::String(".5m".to_string())]);
        assert_parses_to_length(result, 0.5);
    }

    #[test]
    fn parse_length_recognizes_zero_millimetres() {
        let result = eval_parse("parse_length", &[Value::String("0mm".to_string())]);
        assert_parses_to_length(result, 0.0);
    }

    // ── Scientific notation (documented limitation, reviewer suggestion #2) ──
    //
    // Tokenization splits at the FIRST ASCII-alphabetic character, so an
    // exponent marker ('e'/'E') is treated as the start of the unit suffix
    // rather than part of the number. Pinned here as a regression lock: if
    // this ever starts parsing as scientific notation, these assertions catch
    // the behavior change so it's a deliberate decision, not an accident.

    #[test]
    fn parse_length_returns_inner_none_for_unsigned_exponent_scientific_notation() {
        // "1e3mm": splits into num_part="1", unit_part="e3mm" (unrecognized
        // unit) — None, NOT 1000mm.
        let result = eval_parse("parse_length", &[Value::String("1e3mm".to_string())]);
        assert_parses_to_none(result);
    }

    #[test]
    fn parse_length_returns_inner_none_for_signed_exponent_scientific_notation() {
        // "1.0e-2m": splits into num_part="1.0", unit_part="e-2m"
        // (unrecognized unit) — None, NOT 0.01m.
        let result = eval_parse("parse_length", &[Value::String("1.0e-2m".to_string())]);
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
    /// variant:"Err", payload:[("error", Value::String(_))]})` and return the
    /// extracted reason string so callers can also pin its CONTENT (reviewer
    /// suggestion #1, task #4535 amendment round 2) — shape alone can't
    /// distinguish a `Malformed` reason from a `WrongDimension` one.
    fn assert_err_reason(result: Option<Value>) -> String {
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
                match &payload[0].1 {
                    Value::String(reason) => reason.clone(),
                    other => panic!(
                        "expected Value::String for the 'error' payload field, got {other:?}"
                    ),
                }
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
        let reason = assert_err_reason(result);
        assert!(
            reason.to_lowercase().contains("could not parse"),
            "malformed reason should describe a parse failure, got {reason:?}"
        );
    }

    #[test]
    fn parse_length_r_malformed_reason_reports_trimmed_input() {
        // Reviewer suggestion #2 (task #4535 amendment round 2): the
        // Malformed message should quote the TRIMMED input, not echo the
        // raw untrimmed string with its surrounding whitespace.
        let result = eval_parse("parse_length_r", &[Value::String(" bogus ".to_string())]);
        let reason = assert_err_reason(result);
        assert!(
            reason.contains("'bogus'"),
            "reason should quote the trimmed input, got {reason:?}"
        );
        assert!(
            !reason.contains(" bogus "),
            "reason should not echo the untrimmed input with surrounding whitespace, got {reason:?}"
        );
    }

    #[test]
    fn parse_length_r_returns_err_for_recognized_non_length_unit() {
        // "kg" is a recognized built-in unit, but its dimension is Mass —
        // a units-mismatch, distinct from malformed/unknown-unit input.
        // Reviewer suggestion #1 (task #4535 amendment round 2): pin the
        // REASON TEXT so a regression collapsing `WrongDimension` into
        // `Malformed` (or swapping the branch) fails, not just the shape.
        let result = eval_parse("parse_length_r", &[Value::String("12kg".to_string())]);
        let reason = assert_err_reason(result);
        assert!(
            reason.to_lowercase().contains("mass"),
            "wrong-dimension reason should name the mismatched dimension, got {reason:?}"
        );
        assert!(
            !reason.to_lowercase().contains("could not parse"),
            "wrong-dimension reason should read differently from the malformed-input message, got {reason:?}"
        );
    }
}
