//! `Value` → `.ri` source-literal serializer (task #5095, ai-native-editing β;
//! PRD `docs/prds/v0_6/ai-native-editing.md`, INV-GUI-3 substrate).
//!
//! Writes a [`Value`] back out as `.ri` source text, so an agent (or the GUI)
//! can rewrite a `param` default in place. The contract is narrow and total:
//!
//! > For every value [`value_to_ri_literal`] returns `Ok` for, re-parsing
//! > that text yields the **same `Value` variant, the same dimension, and the
//! > identical f64 bits**. Everything else is a structured [`RiLiteralError`]
//! > — never a best-effort string.
//!
//! There is no third outcome. A "close enough" literal would silently corrupt
//! a user's design file, which is strictly worse than refusing to edit it.
//!
//! # Why not `Display for Value`
//!
//! `Display` is a human-facing rendering and breaks the round-trip in three
//! separate ways, each of which this module exists to avoid:
//!
//! 1. Its `Scalar` arm emits `"{si_value} {dimension}"` — the SI base value,
//!    a space, and a dimension string. `0.08 m` is not the `80mm` the user
//!    wrote; and the space alone is fatal, because the `_unit_expr_start`
//!    scanner token is zero-width and refuses to fire across whitespace, so
//!    `80 mm` does not lex as a `quantity_literal` at all.
//! 2. It renders a whole `Real` with `{:.0}`, so `Real(80.0)` becomes `80` —
//!    which re-parses as `Value::Int(80)`. That is a silent *type* change: the
//!    lexer sets `is_real` from `text.contains('.'|'e'|'E')`, and
//!    `classify_number_literal` routes an `is_real == false` token with an
//!    exact-i64 value to `NumberClass::Int`.
//! 3. It emits a `String`'s contents unescaped and unchecked, so a quote, a
//!    backslash or a brace produces text that either fails to lex or (for
//!    `{`/`}`) diverts to `interpolated_string`, where `{expr}` is a hole.
//!
//! # Earned exactness
//!
//! Bit-exactness here is achieved BY CONFIGURATION, not assumed. The obvious
//! `magnitude = si_value / factor` does **not** round-trip: measured over
//! 200k uniform magnitudes per unit, it fails for 4252/200000 `mm` values,
//! 25905/200000 `cm`, 27707/200000 `in` and 18085/200000 `deg` — while
//! factor-1.0 units (`m`, `rad`, `kg`) fail 0/200000.
//!
//! So the serializer walks an ordered ladder of candidate symbols and accepts
//! a rung only when `magnitude * factor == si_value` holds **bit-identically**
//! — the same f64 factor from the same `unit_symbol_to_si` table, applied by
//! the same single multiply the compiler performs for a bare unit literal. An
//! acceptance is therefore a proof, not an estimate. The ladder's last rung
//! has factor exactly 1.0, so `mag * 1.0 == si_value` closes it by IEEE
//! identity and it can never exhaust for a finite input.
//!
//! Neither half is removable. Drop the multiply-back check and ~2% of `mm`
//! and ~9% of `deg` writes corrupt silently; drop the factor-1.0 terminator
//! and the ladder starts falling off its end.
//!
//! # Why the emission ladder is not `display_units::unit_ladders()`
//!
//! `reify_core::display_units` already ships ordered per-dimension unit
//! ladders, and unifying the two tables would look like an obvious cleanup.
//! It is not. That table answers "what may a human pick in the GUI unit
//! picker"; `ri_emittable_units` answers "what may be written into source and
//! read back unchanged". Concretely: the display labels include forms that
//! neither `unit_symbol_to_si` nor the lexer can handle (`mm²`, `cm³`, `L`,
//! `Pa`, `MPa`, `kg/m³`, `N`, `J`, `W`); its Length rungs end at `in`
//! (0.0254), not at a factor-1.0 terminator; and its coverage differs in both
//! directions. Emission order is load-bearing for correctness, display order
//! is picker ergonomics and is free to change — coupling them would let a
//! cosmetic reorder reintroduce lossy write-back. The two are cross-guarded
//! for physical agreement only, in reify-core's `units` tests.

use crate::Value;
use reify_core::DimensionVector;
use reify_core::units::{ri_emittable_units, unit_symbol_to_si};
use std::fmt;

/// Why a [`Value`] cannot be written as a `.ri` source literal.
///
/// Every rejection is structured rather than a best-effort string, because
/// the alternative — emitting something *close* — silently corrupts a user's
/// design file (PRD §7 B7).
#[derive(Debug, Clone, PartialEq)]
pub enum RiLiteralError {
    /// NaN or ±∞. The grammar has no `inf`/`nan` token, and
    /// `classify_number_literal` guards `value.is_finite()`.
    NonFiniteNumber,
    /// An `i64` outside the exactly-`f64`-representable range (±2^53). Every
    /// decimal `.ri` number literal is lowered through `f64::from_str`, so a
    /// larger integer would come back narrowed.
    IntNotRepresentable {
        /// The integer that could not round-trip.
        value: i64,
    },
    /// A string carrying a character `.ri` string literals cannot hold
    /// verbatim — `"`, `\`, `{`, `}`, or any control character.
    StringNotRepresentable {
        /// The first offending character.
        offending: char,
    },
    /// A dimension with no bare built-in unit symbol, so it would need a
    /// compound `UnitExpr` that only a seeded per-module registry can
    /// resolve.
    UnrepresentableDimension {
        /// The dimension that has no emittable bare symbol.
        dimension: DimensionVector,
    },
    /// The value is a kind that has no `.ri` literal form at all.
    UnsupportedValueKind {
        /// Stable discriminant name (e.g. `"List"`), safe to show a user.
        kind: &'static str,
    },
}

impl fmt::Display for RiLiteralError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiLiteralError::NonFiniteNumber => {
                write!(f, "non-finite number has no .ri literal form")
            }
            RiLiteralError::IntNotRepresentable { value } => write!(
                f,
                "integer {value} is outside the exactly-f64-representable range (±2^53)"
            ),
            RiLiteralError::StringNotRepresentable { offending } => write!(
                f,
                "string contains {offending:?}, which a .ri string literal cannot carry verbatim"
            ),
            RiLiteralError::UnrepresentableDimension { dimension } => write!(
                f,
                "dimension {dimension} has no bare built-in unit symbol to emit"
            ),
            RiLiteralError::UnsupportedValueKind { kind } => {
                write!(f, "value kind `{kind}` has no .ri literal form")
            }
        }
    }
}

impl std::error::Error for RiLiteralError {}

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
///
/// Equivalent to [`value_to_ri_literal_with_unit`] with no unit preference.
pub fn value_to_ri_literal(value: &Value) -> Result<String, RiLiteralError> {
    value_to_ri_literal_with_unit(value, None)
}

/// Serialize a [`Value`] as `.ri` source text, preferring `preferred_unit`
/// for a dimensioned scalar when — and only when — that unit is honourable.
///
/// The hint is the unit symbol the caller read off the literal being
/// replaced, so an edit to `width = 50mm` keeps writing millimetres instead
/// of hopping to whatever the canonical ladder prefers. It is deliberately a
/// plain `&str` resolved through [`unit_symbol_to_si`] rather than a span or
/// a registry handle: that keeps this module free of any reify-compiler
/// coupling, and independent of the span-resolution work in sibling task
/// #5094.
///
/// The hint is **advisory** — it is taken only when it resolves as a bare
/// built-in, its dimension matches the value's, and its magnitude is
/// bit-exact by the same test the ladder applies. Anything else (an unknown
/// symbol, a dimension mismatch, an affine unit like `degC` whose offset the
/// built-in table cannot represent, or a rung that would round) silently
/// falls back to the canonical ladder. A hint can therefore change *which*
/// exact literal is written, never *whether* the write is exact.
///
/// Every arm checks finiteness and representability BEFORE it formats
/// anything, so no partially-formed literal can escape on the error path.
pub fn value_to_ri_literal_with_unit(
    value: &Value,
    preferred_unit: Option<&str>,
) -> Result<String, RiLiteralError> {
    match value {
        Value::Bool(b) => Ok(if *b {
            "true".to_owned()
        } else {
            "false".to_owned()
        }),
        Value::Int(i) => {
            if !int_is_exactly_f64_representable(*i) {
                return Err(RiLiteralError::IntNotRepresentable { value: *i });
            }
            Ok(i.to_string())
        }
        Value::Real(r) => {
            if !r.is_finite() {
                return Err(RiLiteralError::NonFiniteNumber);
            }
            Ok(format_f64_shortest(*r, true))
        }
        Value::String(s) => {
            if let Some(offending) = first_unrepresentable_char(s) {
                return Err(RiLiteralError::StringNotRepresentable { offending });
            }
            Ok(format!("\"{s}\""))
        }
        Value::Scalar {
            si_value,
            dimension,
        } => {
            if !si_value.is_finite() {
                return Err(RiLiteralError::NonFiniteNumber);
            }
            // A dimensionless scalar has no unit to write, so it goes out as a
            // bare real. That re-parses as `Value::Real`, whose `dimension()`
            // is `DIMENSIONLESS` — dimensionally identical.
            if dimension.is_dimensionless() {
                return Ok(format_f64_shortest(*si_value, true));
            }
            let ladder = ri_emittable_units(dimension);
            if ladder.is_empty() {
                return Err(RiLiteralError::UnrepresentableDimension {
                    dimension: *dimension,
                });
            }
            // Honour the caller's unit first, but only on the ladder's own
            // terms: same dimension, and bit-exact. Note this is checked
            // AFTER the empty-ladder rejection, so a hint can never smuggle a
            // symbol onto a dimension the canonical table refuses to emit.
            if let Some(hint) = preferred_unit
                && let Some((_, hint_dim)) = unit_symbol_to_si(hint)
                && hint_dim == *dimension
                && let Some(magnitude) = exact_magnitude(*si_value, hint)
            {
                return Ok(format!("{}{hint}", format_f64_shortest(magnitude, false)));
            }
            for sym in ladder {
                if let Some(magnitude) = exact_magnitude(*si_value, sym) {
                    return Ok(format!("{}{sym}", format_f64_shortest(magnitude, false)));
                }
            }
            // Unreachable for a finite `si_value` on a non-empty ladder: the
            // last rung's factor is exactly 1.0 (pinned by reify-core's
            // `ri_emittable_ladders_resolve_to_their_dimension_and_end_at_factor_one`),
            // so `mag * 1.0 == si_value` holds by IEEE identity. Kept as a
            // structured error rather than an `unreachable!()` so a future
            // table edit degrades to a refusal, never to a lossy write.
            Err(RiLiteralError::UnrepresentableDimension {
                dimension: *dimension,
            })
        }
        other => Err(RiLiteralError::UnsupportedValueKind {
            kind: value_kind_name(other),
        }),
    }
}

/// Whether an `i64` survives the `i64 → f64 → i64` trip the parser forces.
///
/// The magnitude clause is load-bearing on its own: `i as f64 as i64 == i`
/// is *true* for `i64::MIN`, because the saturating `as` cast lands back on
/// `i64::MIN` — the equality alone would wave through a value the parser
/// cannot reproduce.
fn int_is_exactly_f64_representable(i: i64) -> bool {
    let as_f64 = i as f64;
    as_f64 as i64 == i && as_f64.abs() <= (2f64).powi(53)
}

/// The first character a `.ri` string literal cannot carry verbatim.
///
/// `lower_string_literal` strips the outer quotes and performs NO escape
/// decoding, so `"` and `\` cannot appear at all, and neither can a control
/// character (a raw newline/tab would not even lex). `{` and `}` are excluded
/// separately: they fail the `string_literal` token and divert the text to
/// `interpolated_string`, which DOES decode escapes and treats `{expr}` as a
/// hole — a brace-bearing string could therefore evaluate source.
fn first_unrepresentable_char(s: &str) -> Option<char> {
    s.chars()
        .find(|c| matches!(c, '"' | '\\' | '{' | '}') || c.is_control())
}

/// Stable discriminant name for a value kind with no `.ri` literal form.
///
/// Deliberately a fixed `&'static str` per variant rather than `{value:?}`:
/// a `SampledField` or `Matrix` payload could be enormous, and this string
/// ends up in a user-facing MCP error.
fn value_kind_name(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Real(_) => "Real",
        Value::String(_) => "String",
        Value::Scalar { .. } => "Scalar",
        Value::Enum { .. } => "Enum",
        Value::List(_) => "List",
        Value::Set(_) => "Set",
        Value::Map(_) => "Map",
        Value::Option(_) => "Option",
        Value::Point(_) => "Point",
        Value::Vector(_) => "Vector",
        Value::Tensor(_) => "Tensor",
        Value::Matrix(_) => "Matrix",
        Value::Undef => "Undef",
        _ => "Unsupported",
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

    // ─── Caller-supplied unit hint (PRD §12 Q2) ──────────────────────────────
    //
    // γ reads the unit off the existing default literal and passes it here, so
    // rewriting `width = 50mm` to 60mm keeps writing `mm` rather than jumping
    // to whatever the canonical ladder prefers. The hint is ADVISORY: it is
    // taken only when it resolves, matches the dimension, AND is bit-exact.
    // It is a plain `&str` resolved through the built-in table — no span, no
    // registry handle — which is what keeps this independent of sibling α.

    fn lit_with(v: &Value, unit: Option<&str>) -> String {
        value_to_ri_literal_with_unit(v, unit)
            .unwrap_or_else(|e| panic!("expected Ok for {v:?} + {unit:?}, got {e:?}"))
    }

    #[test]
    fn an_exact_hint_overrides_the_canonical_ladder() {
        // Canonical would be `50mm` — the hint keeps the edit in centimetres.
        assert_eq!(lit_with(&Value::length(0.05), Some("cm")), "5cm");
        // `in` is not on the canonical ladder at all; canonical is `25.4mm`.
        assert_eq!(lit_with(&Value::length(0.0254), Some("in")), "1in");
        // Canonical would be `90deg`.
        assert_eq!(
            lit_with(&Value::angle(std::f64::consts::FRAC_PI_2), Some("rad")),
            "1.5707963267948966rad"
        );
    }

    /// A hint the built-in table cannot honour is never fatal — it falls back
    /// to the canonical ladder and still returns `Ok`.
    ///
    /// `degC` stands in for the affine units: those live only in the
    /// compiler's registry and carry an offset `unit_symbol_to_si` has no way
    /// to represent, so they can never be taken as a hint here.
    #[test]
    fn an_unusable_hint_is_advisory_and_falls_back() {
        for hint in [
            Some("furlong"), // unresolvable
            Some("deg"),     // resolvable, but wrong dimension for a Length
            Some("degC"),    // affine — registry-only, offset-bearing
            Some(""),        // degenerate
            None,
        ] {
            assert_eq!(
                lit_with(&Value::length(0.08), hint),
                "80mm",
                "hint {hint:?} must fall back to the canonical ladder"
            );
        }
    }

    /// A hint is never allowed to make the write LOSSY. The `mm` rung is
    /// bit-inexact for this magnitude, so the hint is dropped and the ladder
    /// picks `cm` — the same answer the unhinted call gives.
    #[test]
    fn a_bit_inexact_hint_is_refused() {
        let v = Value::length(-0.5566166674539299);
        assert_eq!(lit_with(&v, Some("mm")), "-55.66166674539299cm");
    }

    #[test]
    fn the_hint_is_ignored_for_non_scalar_values() {
        assert_eq!(lit_with(&Value::Int(7), Some("mm")), "7");
        assert_eq!(lit_with(&Value::Real(1.5), Some("mm")), "1.5");
        assert_eq!(lit_with(&Value::Bool(true), Some("mm")), "true");
        assert_eq!(
            lit_with(&Value::String("PLA".into()), Some("mm")),
            "\"PLA\""
        );
        assert_eq!(
            lit_with(
                &Value::Scalar {
                    si_value: 2.5,
                    dimension: DimensionVector::DIMENSIONLESS,
                },
                Some("mm")
            ),
            "2.5"
        );
    }

    /// `value_to_ri_literal(v)` must be exactly `..._with_unit(v, None)`,
    /// including on the error paths.
    #[test]
    fn the_unhinted_entry_point_delegates() {
        let cases = [
            Value::Bool(false),
            Value::Int(-42),
            Value::Real(80.0),
            Value::String("M3x0.5".into()),
            Value::length(0.08),
            Value::length(-0.5566166674539299),
            Value::angle(std::f64::consts::FRAC_PI_2),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::PRESSURE,
            },
            Value::Real(f64::NAN),
            Value::Undef,
        ];
        for v in cases {
            assert_eq!(
                value_to_ri_literal(&v),
                value_to_ri_literal_with_unit(&v, None),
                "delegation mismatch for {v:?}"
            );
        }
    }

    // ─── Structured rejection contract (PRD §7 B7) ───────────────────────────
    //
    // The serializer must never emit a lossy or unparseable literal. Every
    // value it cannot write back EXACTLY is a structured `Err` a caller can
    // render, never a best-effort string.

    fn err(v: &Value) -> RiLiteralError {
        value_to_ri_literal(v).expect_err(&format!("expected Err for {v:?}"))
    }

    /// The grammar has no `inf`/`nan` token, and `classify_number_literal`
    /// explicitly guards `value.is_finite()`.
    #[test]
    fn non_finite_numbers_are_rejected() {
        for v in [
            Value::Real(f64::NAN),
            Value::Real(f64::INFINITY),
            Value::Real(f64::NEG_INFINITY),
            Value::length(f64::NAN),
            Value::length(f64::INFINITY),
        ] {
            assert!(
                matches!(err(&v), RiLiteralError::NonFiniteNumber),
                "{v:?} must be rejected as non-finite"
            );
        }
    }

    /// Every DECIMAL `.ri` number literal is lowered through `f64::from_str`,
    /// and `parse_number_literal_text`'s own doc-comment states that values
    /// beyond 2^53 are stored as a lossy `(n as f64)`. So an `i64` outside the
    /// exactly-f64-representable range cannot round-trip and must be rejected
    /// rather than silently narrowed.
    #[test]
    fn ints_outside_the_exact_f64_range_are_rejected() {
        for i in [i64::MAX, i64::MIN, (1i64 << 53) + 1, -((1i64 << 53) + 1)] {
            assert!(
                matches!(
                    err(&Value::Int(i)),
                    RiLiteralError::IntNotRepresentable { .. }
                ),
                "Int({i}) must be rejected as not exactly f64-representable"
            );
        }
    }

    #[test]
    fn ints_inside_the_exact_f64_range_are_accepted() {
        assert_eq!(lit(&Value::Int(1i64 << 53)), "9007199254740992");
        assert_eq!(lit(&Value::Int(-(1i64 << 53))), "-9007199254740992");
        assert_eq!(lit(&Value::Int(9007199254740991)), "9007199254740991");
    }

    /// `lower_string_literal` strips the outer quotes and performs NO escape
    /// decoding, so an emitted `\"` comes back as a literal backslash-quote.
    /// Separately, a bare `{`/`}` makes the `string_literal` token fail and
    /// diverts the text to `interpolated_string`, which DOES decode escapes
    /// and treats `{expr}` as a hole — so a brace could evaluate source.
    #[test]
    fn strings_outside_the_verbatim_safe_charset_are_rejected() {
        for s in [
            "say \"hi\"",
            "back\\slash",
            "hole {x}",
            "open {",
            "close }",
            "two\nlines",
            "tab\there",
            "bell\u{0007}",
            "nul\u{0000}",
        ] {
            let v = Value::String(s.to_owned());
            assert!(
                matches!(err(&v), RiLiteralError::StringNotRepresentable { .. }),
                "{s:?} must be rejected as not verbatim-safe"
            );
        }
    }

    #[test]
    fn verbatim_safe_strings_are_accepted() {
        assert_eq!(lit(&Value::String("PLA".into())), "\"PLA\"");
        assert_eq!(lit(&Value::String("M3x0.5".into())), "\"M3x0.5\"");
        assert_eq!(lit(&Value::String(String::new())), "\"\"");
        assert_eq!(lit(&Value::String("naïve—Ω".into())), "\"naïve—Ω\"");
    }

    /// Dimensions with no bare built-in symbol are rejected outright. Several
    /// of these DO have `display_units` ladders — the GUI can still *show* a
    /// pressure — which is the clearest demonstration that the display table
    /// and the emission table answer different questions.
    #[test]
    fn dimensions_needing_a_compound_unit_are_rejected() {
        for dim in [
            DimensionVector::AREA,
            DimensionVector::VOLUME,
            DimensionVector::PRESSURE,
            DimensionVector::FORCE,
            DimensionVector::SOLID_ANGLE,
            DimensionVector::MONEY,
        ] {
            let v = Value::Scalar {
                si_value: 1.5,
                dimension: dim,
            };
            assert!(
                matches!(err(&v), RiLiteralError::UnrepresentableDimension { .. }),
                "{:?} must be rejected — no bare built-in symbol",
                dim.canonical_name()
            );
        }
    }

    /// The catch-all must name the kind so γ can render a useful MCP error,
    /// and must NOT embed a `{value:?}` dump (a `SampledField` payload could
    /// be enormous).
    #[test]
    fn unsupported_value_kinds_are_rejected_with_a_stable_kind_name() {
        let cases = [
            (Value::Undef, "Undef"),
            (Value::List(vec![]), "List"),
            (Value::Point(vec![]), "Point"),
            (Value::Option(None), "Option"),
            (Value::enum_unit("Fit", "Loose"), "Enum"),
        ];
        for (v, expected) in cases {
            match err(&v) {
                RiLiteralError::UnsupportedValueKind { kind } => {
                    assert_eq!(kind, expected, "kind name for {v:?}")
                }
                other => panic!("expected UnsupportedValueKind for {v:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn error_is_a_standard_error() {
        fn assert_error<E: std::error::Error>(e: &E) -> String {
            e.to_string()
        }
        let e = err(&Value::Real(f64::NAN));
        let rendered = assert_error(&e);
        assert!(
            !rendered.is_empty(),
            "Display for RiLiteralError must render something"
        );
        // Every variant renders non-empty and mentions nothing enormous.
        for v in [
            Value::Int(i64::MAX),
            Value::String("{".into()),
            Value::Scalar {
                si_value: 1.0,
                dimension: DimensionVector::PRESSURE,
            },
            Value::Undef,
        ] {
            let text = err(&v).to_string();
            assert!(!text.is_empty(), "empty Display for {v:?}");
            assert!(text.len() < 200, "Display for {v:?} is too verbose: {text}");
        }
    }

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
