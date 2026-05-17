//! Parse-error diagnostics with 1-indexed source line numbers
//! (PRD §1 `E_GcodeParseError`).
//!
//! Step-13 RED test for task μ: pins UnknownCommand vs InvalidParameter
//! classification, and confirms that the line counter advances on every
//! physical line (blank + comment-only included).

use reify_gcode::error::{ParseError, ParseErrorKind};
use reify_gcode::parse_marlin;

#[test]
fn unknown_g_code_on_second_line() {
    let err = parse_marlin("G1 X10\nG99 X5").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 2,
            kind: ParseErrorKind::UnknownCommand("G99".to_string()),
        }
    );
}

#[test]
fn unknown_m_code_on_first_line() {
    let err = parse_marlin("M999 S1").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 1,
            kind: ParseErrorKind::UnknownCommand("M999".to_string()),
        }
    );
}

#[test]
fn invalid_parameter_float_two_dots() {
    let err = parse_marlin("G1 X1.2.3").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 1,
            kind: ParseErrorKind::InvalidParameter {
                letter: 'X',
                value: "1.2.3".to_string(),
            },
        }
    );
}

#[test]
fn line_counter_counts_blank_and_comment_lines() {
    let err = parse_marlin("G1 X10\n\n; comment\nGarbage").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 4,
            kind: ParseErrorKind::UnknownCommand("Garbage".to_string()),
        }
    );
}

// Pins documented behaviour at `marlin.rs` (parse_value) — a bare axis
// letter with no numeric body produces `InvalidParameter { letter, value:
// "" }` rather than `MissingCommand` or a panic.
#[test]
fn axis_letter_without_value_is_invalid_parameter() {
    let err = parse_marlin("G1 X").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 1,
            kind: ParseErrorKind::InvalidParameter {
                letter: 'X',
                value: String::new(),
            },
        }
    );
}

// Pins documented behaviour at `marlin.rs` (F-prefix branch) — a bare
// `F` with no numeric body produces `InvalidParameter { letter: 'F',
// value: "" }`, the same shape the inline-feedrate path emits.
#[test]
fn bare_f_without_value_is_invalid_parameter() {
    let err = parse_marlin("F").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 1,
            kind: ParseErrorKind::InvalidParameter {
                letter: 'F',
                value: String::new(),
            },
        }
    );
}

// Pins amendment-1 behaviour: when a recognised standalone feedrate
// (`F<number>`) is followed by trailing tokens, the diagnostic must
// surface BOTH the command and the offending tokens — not
// `UnknownCommand("F100")`, which would mislead a user into thinking
// the F-prefix itself was wrong.
#[test]
fn feedrate_with_trailing_tokens_is_unexpected_trailing_tokens() {
    let err = parse_marlin("F100 X10").unwrap_err();
    assert_eq!(
        err,
        ParseError {
            line: 1,
            kind: ParseErrorKind::UnexpectedTrailingTokens {
                command: "F100".to_string(),
                tokens: vec!["X10".to_string()],
            },
        }
    );
}
