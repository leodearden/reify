//! `Value` → `.ri` source-literal serializer (task #5095, ai-native-editing β).
//!
//! Placeholder body — implemented in step-4.

use crate::Value;

/// Why a [`Value`] cannot be written as a `.ri` source literal.
#[derive(Debug, Clone, PartialEq)]
pub enum RiLiteralError {
    /// The value is a kind that has no `.ri` literal form at all.
    UnsupportedValueKind {
        /// Stable discriminant name (e.g. `"List"`), safe to show a user.
        kind: &'static str,
    },
}

/// Serialize a [`Value`] as `.ri` source text that re-parses to that same value.
///
/// Placeholder — implemented in step-4.
pub fn value_to_ri_literal(_value: &Value) -> Result<String, RiLiteralError> {
    Err(RiLiteralError::UnsupportedValueKind {
        kind: "unimplemented",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_core::DimensionVector;

    fn lit(v: &Value) -> String {
        value_to_ri_literal(v).unwrap_or_else(|e| panic!("expected Ok for {v:?}, got {e:?}"))
    }

    #[test]
    fn bool_literals_round_trip_as_keywords() {
        assert_eq!(lit(&Value::Bool(true)), "true");
        assert_eq!(lit(&Value::Bool(false)), "false");
    }

    #[test]
    fn int_literals_are_bare_decimal() {
        assert_eq!(lit(&Value::Int(-42)), "-42");
        assert_eq!(lit(&Value::Int(0)), "0");
        assert_eq!(lit(&Value::Int(7)), "7");
    }

    /// A whole `Real` MUST keep its `.0`.
    ///
    /// The lexer classifies a number literal as real by
    /// `is_real = text.contains('.') || text.contains('e') || text.contains('E')`
    /// (`ts_parser.rs` `parse_number_literal_text`), and
    /// `classify_number_literal` (`reify-ast/src/decl.rs`) then routes an
    /// `is_real == false` token whose value is an exact `i64` to
    /// `NumberClass::Int`. So a bare `80` would come back as `Value::Int(80)`
    /// — a silent type change, not a rounding wobble. `Value`'s own `Display`
    /// makes exactly this mistake (`{:.0}` for whole reals).
    #[test]
    fn whole_real_keeps_its_decimal_point() {
        assert_eq!(lit(&Value::Real(80.0)), "80.0");
        assert_eq!(lit(&Value::Real(0.0)), "0.0");
        assert_eq!(lit(&Value::Real(-3.0)), "-3.0");
    }

    #[test]
    fn fractional_real_uses_shortest_round_tripping_form() {
        assert_eq!(lit(&Value::Real(1.5)), "1.5");
        assert_eq!(lit(&Value::Real(-0.25)), "-0.25");
    }

    #[test]
    fn string_is_quoted() {
        assert_eq!(lit(&Value::String("PLA".into())), "\"PLA\"");
        assert_eq!(lit(&Value::String(String::new())), "\"\"");
    }

    /// The PRD's headline G3 case: a length whose SI value is `0.08` must be
    /// written back as the literal a human would have typed.
    #[test]
    fn length_emits_a_contiguous_quantity_literal() {
        assert_eq!(lit(&Value::length(0.08)), "80mm");
        assert_eq!(lit(&Value::length(0.0)), "0mm");
        assert_eq!(lit(&Value::length(-0.08)), "-80mm");
    }

    #[test]
    fn angle_prefers_degrees() {
        assert_eq!(lit(&Value::angle(std::f64::consts::FRAC_PI_2)), "90deg");
    }

    #[test]
    fn dimensionless_scalar_emits_a_bare_number() {
        assert_eq!(
            lit(&Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::DIMENSIONLESS,
            }),
            "2.5"
        );
    }

    /// LADDER-FALLBACK REGRESSION — the earned-exactness witness.
    ///
    /// `-0.5566166674539299 / 0.001` then re-multiplied by `0.001` does NOT
    /// return the original f64, so the `mm` rung must be REJECTED; the `cm`
    /// rung is bit-exact and is what gets emitted. A naive `si_value / factor`
    /// implementation emits a lossy `mm` form and fails here.
    #[test]
    fn ladder_falls_through_a_bit_inexact_rung() {
        let si = -0.5566166674539299_f64;
        // Pin the premise itself, so a future reader can see the fallback is
        // exercised for a real arithmetic reason and not by coincidence.
        assert_ne!((si / 0.001) * 0.001, si, "mm rung must be inexact here");
        assert_eq!((si / 0.01) * 0.01, si, "cm rung must be exact here");

        assert_eq!(lit(&Value::length(si)), "-55.66166674539299cm");
    }

    /// CONTIGUITY — a quantity literal may not contain whitespace.
    ///
    /// `_unit_expr_start` in `tree-sitter-reify/src/scanner.c` is a zero-width
    /// external token that refuses to fire across whitespace, so `80 mm` does
    /// not parse as a `quantity_literal` at all. `impl Display for Value`
    /// emits `"{si_value} {dimension}"` — SI base plus a space — which is
    /// precisely the gap this serializer exists to close.
    #[test]
    fn emitted_scalars_never_contain_a_space() {
        let cases = [
            Value::length(0.08),
            Value::length(-0.5566166674539299),
            Value::angle(std::f64::consts::FRAC_PI_2),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::MASS,
            },
            Value::Scalar {
                si_value: 2.5,
                dimension: DimensionVector::DIMENSIONLESS,
            },
        ];
        for v in cases {
            let s = lit(&v);
            assert!(
                !s.contains(' '),
                "emitted literal {s:?} for {v:?} contains a space; \
                 the scanner will not fire _unit_expr_start across whitespace"
            );
        }
    }
}
