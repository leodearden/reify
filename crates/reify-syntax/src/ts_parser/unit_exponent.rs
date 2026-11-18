//! Reading a unit expression's exponent, and recognising a value-level `/` that touches one.
//!
//! The rules are PRD docs/prds/unit-expressions.md §3.2. A rational exponent must be written
//! canonically, and a `/` that touches an exponent but could not be lexed into it is refused,
//! so no contiguous `^…/…` spelling is read as a value division (INV-SF-7).
//!
//! Like its siblings `detached_unit` and `fault_diagnosis`, it reads only a bare
//! `tree_sitter::Node` plus the source text and no lowering state. Each refusal comes back as
//! the diagnostic message; the caller pushes it as a located error.

use reify_core::Rational;
use tree_sitter::Node;

/// The rational value of a `unit_exponent` token's text (`2`, `-1`, `1/2`, `-1/2`), or the
/// message refusing it.
///
/// A `p/q` exponent must be in lowest terms with `q ≥ 2`; `^2/4` is refused rather than
/// reread as `^1/2`. Range is refused here only where `Rational`'s i16 cannot hold the value:
/// the unit-exponent range policy is `reify_core::unit_exponent_in_range`, applied at resolve.
pub(super) fn parse_unit_exponent(text: &str) -> Result<Rational, String> {
    let refuse = |why: String| Err(format!("unit exponent `{text}` {why}"));
    let (num_text, den_text) = match text.split_once('/') {
        Some((num, den)) => (num, Some(den)),
        None => (text, None),
    };
    let (Ok(num), Ok(den)) = (
        num_text.parse::<i16>(),
        den_text.map_or(Ok(1), str::parse::<i16>),
    ) else {
        return refuse("is out of range".to_string());
    };
    if den_text.is_none() {
        return Ok(Rational::new(num, 1));
    }
    if den == 0 {
        return refuse("has a zero denominator".to_string());
    }
    let exponent = Rational::new(num, den);
    if den == 1 || (exponent.num(), exponent.den()) != (num, den) {
        return refuse(format!(
            "is not written canonically: write `^{exponent}`, or put a space before `/` to \
             divide"
        ));
    }
    Ok(exponent)
}

/// The message refusing the value-level `/` operator `op` when it touches the end of a
/// `unit_exponent` token at the end of its left operand `left` (`1m^1/ 2`, `1m^1/-2`,
/// `1m^1/2/3`); `None` for any other `/`.
pub(super) fn slash_touching_unit_exponent(
    left: Node<'_>,
    op: Node<'_>,
    source: &str,
) -> Option<String> {
    let at = op.start_byte();
    if at == 0 || left.end_byte() != at {
        return None;
    }
    let last = left.descendant_for_byte_range(at - 1, at)?;
    if last.kind() != "unit_exponent" || last.end_byte() != at {
        return None;
    }
    Some(format!(
        "`/` directly after the unit exponent `^{}` is ambiguous: put a space before `/` to \
         divide (`1m^2 / 2`), or write a rational exponent with no spaces (`^1/2`)",
        &source[last.byte_range()]
    ))
}
