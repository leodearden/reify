//! `Value` → `.ri` source-literal serializer (task #5095, ai-native-editing β).
//!
//! Writes a [`Value`] back out as `.ri` source text that **re-parses to that
//! same value**, so an agent (or the GUI) can rewrite a `param` default in
//! place without changing its type, its unit, or its last bit.
//!
//! Structured rejections and the caller-supplied unit hint arrive in
//! steps 5-8; the full contract note lands in step-10.

use crate::Value;
use reify_core::units::{ri_emittable_units, unit_symbol_to_si};

/// Why a [`Value`] cannot be written as a `.ri` source literal.
#[derive(Debug, Clone, PartialEq)]
pub enum RiLiteralError {
    /// The value is a kind that has no `.ri` literal form at all.
    UnsupportedValueKind {
        /// Stable discriminant name (e.g. `"List"`), safe to show a user.
        kind: &'static str,
    },
}

/// Format an `f64` as the shortest decimal text that parses back to the same
/// bits, matching the lexer's own `f64::from_str`.
///
/// Rust's `{:?}` for floats is shortest-round-tripping and always contains a
/// `.` or an `e`, which is exactly the `is_real` predicate the lexer applies
/// (`text.contains('.'|'e'|'E')`). When `force_decimal_point` is false a
/// trailing `".0"` is stripped, so a scalar reads `80mm` rather than
/// `80.0mm` — safe because `lower_quantity_literal` discards `is_real`, and
/// it keeps the rewritten source idiomatic. `Value::Real` passes `true`,
/// because there the `is_real` bit is what separates `Real` from `Int`.
///
/// Always DECIMAL: the parser's `0x`/`0b` radix branches force
/// `is_real = false` and are unreachable from this emitter by construction.
fn format_f64_shortest(x: f64, force_decimal_point: bool) -> String {
    let s = format!("{x:?}");
    if force_decimal_point {
        return s;
    }
    s.strip_suffix(".0").map(str::to_owned).unwrap_or(s)
}

/// Serialize a [`Value`] as `.ri` source text that re-parses to that same value.
pub fn value_to_ri_literal(value: &Value) -> Result<String, RiLiteralError> {
    match value {
        Value::Bool(b) => Ok(if *b {
            "true".to_owned()
        } else {
            "false".to_owned()
        }),
        Value::Int(i) => Ok(i.to_string()),
        Value::Real(r) => Ok(format_f64_shortest(*r, true)),
        Value::String(s) => Ok(format!("\"{s}\"")),
        Value::Scalar {
            si_value,
            dimension,
        } => {
            // A dimensionless scalar has no unit to write, so it goes out as a
            // bare real. That re-parses as `Value::Real`, whose `dimension()`
            // is `DIMENSIONLESS` — dimensionally identical.
            if dimension.is_dimensionless() {
                return Ok(format_f64_shortest(*si_value, true));
            }
            let ladder = ri_emittable_units(dimension);
            for sym in ladder {
                if let Some(magnitude) = exact_magnitude(*si_value, sym) {
                    return Ok(format!("{}{sym}", format_f64_shortest(magnitude, false)));
                }
            }
            // Unreachable for a finite `si_value` on a non-empty ladder: the
            // last rung's factor is exactly 1.0 (pinned by reify-core's
            // `ri_emittable_ladders_resolve_to_their_dimension_and_end_at_factor_one`),
            // so `mag * 1.0 == si_value` holds by IEEE identity. Empty ladders
            // and non-finite inputs are rejected in step-6.
            Err(RiLiteralError::UnsupportedValueKind { kind: "Scalar" })
        }
        _ => Err(RiLiteralError::UnsupportedValueKind {
            kind: "Unsupported",
        }),
    }
}

/// The magnitude that would be written in front of `unit`, but ONLY when it
/// recovers `si_value` bit-identically.
///
/// This reproduces the compiler's own arithmetic rather than approximating it:
/// the same `f64` factor from the same [`unit_symbol_to_si`] table, applied by
/// the same single multiply the compiler performs for a bare unit literal
/// (`unit_to_scalar` delegates to that table). So a `Some` here is a *proof*
/// that the literal re-parses to the original bits, not an estimate.
///
/// Naive `si_value / factor` is emphatically NOT enough — measured over 200k
/// uniform magnitudes per unit, it fails to round-trip for ~2.1% of `mm`,
/// ~13% of `cm`, ~14% of `in` and ~9% of `deg` values, while factor-1.0 units
/// (`m`, `rad`, `kg`) never fail.
fn exact_magnitude(si_value: f64, unit: &str) -> Option<f64> {
    let (factor, _dim) = unit_symbol_to_si(unit)?;
    let magnitude = si_value / factor;
    (magnitude * factor == si_value).then_some(magnitude)
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
